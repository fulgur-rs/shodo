//! Layout annotation reservations, separate from paint overflow.
use super::Line;
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::line::fragments::{GlyphSource, RecordKind};
use crate::shape::orientation::RunOrientation;

/// Annotation overflow and reusable leading on one independently sized line.
///
/// Coordinates are line-local logical block coordinates, without block offset.
/// The unannotated box is the selected content's emphasis-free line profile,
/// aligned to the accepted dominant baseline. Content uses accepted inline
/// displacements, so top/bottom alignment cannot expose occupied leading.
/// Values describe layout reservations, including hidden readings, rather than
/// glyph ink or the untrimmed painted mark boxes in [`Line::overflow_rect`].
///
/// Over/under are line-relative: over is block-end in `vertical-lr`, and
/// block-start in the other supported writing modes. Callers own any spacing
/// adjustment between lines or blocks; querying this does not change layout.
/// Truncation preserves these accepted layout metrics, as it preserves height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnnotationMetrics {
    /// Emphasis-free line-box block-start, relative to the accepted line.
    pub unannotated_block_start: f32,
    /// Emphasis-free line-box block-end; subtract start for its block size.
    pub unannotated_block_end: f32,
    /// Reserved annotations extending past the unannotated line-over edge.
    pub overflow_over: f32,
    /// Reserved annotations extending past the unannotated line-under edge.
    pub overflow_under: f32,
    /// Unoccupied leading between the over edge and content/annotations.
    pub space_over: f32,
    /// Unoccupied leading between the under edge and content/annotations.
    pub space_under: f32,
}

/// Fixed-size cached values: the enclosing Line header owns all storage.
#[derive(Clone, Copy, Default)]
pub(crate) struct Geometry {
    pub(crate) start: LayoutUnit,
    pub(crate) end: LayoutUnit,
    overflow: [LayoutUnit; 2],
    space: [LayoutUnit; 2],
}

impl Line {
    /// Returns cached layout annotation overflow and reusable leading.
    ///
    /// For consecutive lines using the same writing mode and coordinate units,
    /// in horizontal or vertical-rl block progression a caller may borrow
    /// `min(previous.space_under, next.overflow_over)` from the preceding line.
    /// Swap over/under for vertical-lr progression. This is a policy input, not
    /// automatic collapsing or a promise about painted ink. Ordinary font
    /// content outside a short box is excluded from annotation overflow and
    /// prevents reusable space on that side. Space is contained in both the
    /// accepted and unannotated boxes, whose rounding can differ. Negative
    /// leading remains meaningful in the unannotated coordinates.
    pub fn annotation_metrics(&self) -> AnnotationMetrics {
        let g = self.annotation_geometry;
        AnnotationMetrics {
            unannotated_block_start: g.start.to_f32(),
            unannotated_block_end: g.end.to_f32(),
            overflow_over: g.overflow[0].to_f32(),
            overflow_under: g.overflow[1].to_f32(),
            space_over: g.space[0].to_f32(),
            space_under: g.space[1].to_f32(),
        }
    }

    pub(crate) fn measure_annotations(
        &mut self,
        mut annotation: [Option<LayoutUnit>; 2],
        root_strut: bool,
        sat: &mut Saturation,
    ) {
        if self.empty {
            self.annotation_geometry = Geometry::default();
            return;
        }
        let mut content: Option<(LayoutUnit, LayoutUnit)> = None;
        for (index, record) in self.fragments.iter().enumerate() {
            #[cfg(test)]
            super::annotation_probe::record();
            let center = self.baseline.add(self.block_shifts[index], sat);
            let extents = match &record.kind {
                RecordKind::Glyphs {
                    run, source, item, ..
                } => {
                    let run = match source {
                        GlyphSource::Overlay { run: Some(run), .. } => {
                            &self.overlay_runs[*run as usize]
                        }
                        _ => &self.data.runs[*run as usize],
                    };
                    let style = self.data.items[*item as usize].style;
                    let size = run.font_size;
                    let upright = run.orientation == RunOrientation::Upright;
                    let raw = match run.orientation {
                        RunOrientation::Combined => (size / 2., size / 2.),
                        RunOrientation::Upright => run
                            .instance
                            .vertical_metrics
                            .map_or((size / 2., size / 2.), |v| (v.ascent, v.descent)),
                        _ => {
                            let m = run
                                .instance
                                .metrics
                                .unwrap_or_else(|| self.data.fonts.metrics(run.font, size));
                            (m.ascent, m.descent)
                        }
                    };
                    let (a, d) = if run.orientation == RunOrientation::Combined {
                        raw
                    } else {
                        crate::line::metrics::em_content_extents(
                            &self.data, style, run.font, size, upright, raw,
                        )
                    };
                    let extents = fixed(&self.data, a, d, sat);
                    let marks = crate::ruby::geometry::record_emphasis(
                        &self.data,
                        record,
                        &self.overlay_runs,
                        sat,
                    );
                    if marks.iter().any(Option::is_some) {
                        let (a, d) = crate::ruby::geometry::record_primary_extents(
                            &self.data,
                            record,
                            &self.overlay_runs,
                            sat,
                        )
                        .expect("glyph primary edges");
                        let offsets = self
                            .emphasis_offsets
                            .get(index)
                            .copied()
                            .unwrap_or_default();
                        if let Some(mark) = marks[0] {
                            let edge = center.sub(a, sat).sub(offsets.0, sat).add(mark.top, sat);
                            include(&mut annotation[0], edge, true);
                        }
                        if let Some(mark) = marks[1] {
                            let edge = center.add(d, sat).add(offsets.1, sat).add(mark.bottom, sat);
                            include(&mut annotation[1], edge, false);
                        }
                    }
                    Some(extents)
                }
                RecordKind::Atomic { unit, .. } => {
                    let extents = crate::ruby::geometry::record_extents(
                        &self.data,
                        record,
                        &self.overlay_runs,
                        sat,
                    )
                    .expect("atomic edges");
                    let style =
                        self.data.items[self.data.units[*unit as usize].item as usize].style;
                    if self.data.styles[style as usize]
                        .text_emphasis
                        .is_some_and(|e| {
                            crate::line::metrics::emphasis_over(
                                e.position,
                                self.data.style.writing_mode,
                            )
                        })
                    {
                        let side =
                            usize::from(self.data.style.writing_mode == WritingMode::VerticalLr);
                        let mark = LayoutUnit::from_f32_round(
                            self.data.style_metrics[style as usize].size / 2.,
                            sat,
                        );
                        let edge = if side == 0 {
                            center.sub(extents.0, sat).sub(mark, sat)
                        } else {
                            center.add(extents.1, sat).add(mark, sat)
                        };
                        include(&mut annotation[side], edge, side == 0);
                    }
                    Some(extents)
                }
                RecordKind::InlineBox { .. } | RecordKind::Anchor { .. } => None,
            };
            if let Some((a, d)) = extents {
                let top = center.sub(a, sat);
                let bottom = center.add(d, sat);
                content =
                    Some(content.map_or((top, bottom), |old| (old.0.min(top), old.1.max(bottom))));
            }
        }
        let g = &mut self.annotation_geometry;
        let mut content = content.unwrap_or((self.baseline, self.baseline));
        if let Some(edge) = annotation[0] {
            content.0 = content.0.min(edge);
        }
        if let Some(edge) = annotation[1] {
            content.1 = content.1.max(edge);
        }
        let size = LayoutUnit::from_f32_round(self.data.style_metrics[0].size, sat);
        if root_strut && content.1.sub(content.0, sat) < size {
            let leading = g.end.sub(g.start, sat).sub(size, sat).max(LayoutUnit::ZERO);
            let half = LayoutUnit::from_raw(leading.raw() / 2);
            // Secure the contributing root's em box without discarding real
            // displaced content or marks that lie beyond its centered floor.
            content = (
                content.0.min(g.start.add(half, sat)),
                content.1.max(g.end.sub(half, sat)),
            );
        }
        let overflow = [
            annotation[0].map_or(LayoutUnit::ZERO, |edge| {
                g.start.sub(edge, sat).max(LayoutUnit::ZERO)
            }),
            annotation[1].map_or(LayoutUnit::ZERO, |edge| {
                edge.sub(g.end, sat).max(LayoutUnit::ZERO)
            }),
        ];
        let space = [
            content
                .0
                .sub(g.start.max(LayoutUnit::ZERO), sat)
                .max(LayoutUnit::ZERO),
            g.end
                .min(self.block_size)
                .sub(content.1, sat)
                .max(LayoutUnit::ZERO),
        ];
        let reverse = self.data.style.writing_mode == WritingMode::VerticalLr;
        g.overflow = if reverse {
            [overflow[1], overflow[0]]
        } else {
            overflow
        };
        g.space = if reverse { [space[1], space[0]] } else { space };
    }
}

fn include(slot: &mut Option<LayoutUnit>, edge: LayoutUnit, before: bool) {
    *slot = Some(slot.map_or(
        edge,
        |old| if before { old.min(edge) } else { old.max(edge) },
    ));
}

fn fixed(
    data: &crate::paragraph::ParagraphData,
    a: f32,
    d: f32,
    sat: &mut Saturation,
) -> (LayoutUnit, LayoutUnit) {
    let (a, d) = if data.style.writing_mode == WritingMode::VerticalLr {
        (d, a)
    } else {
        (a, d)
    };
    (
        LayoutUnit::from_f32_round(a, sat),
        LayoutUnit::from_f32_round(d, sat),
    )
}
