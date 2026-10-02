//! Source paint ranges reuse the finalized caret geometry, independently of
//! the single owner of each painted glyph. No shaping or layout is performed.
use crate::analysis::ItemKind;
use crate::font::{FontId, FontMetrics};
use crate::geometry::{Direction, LogicalRect, WritingMode};
use crate::node::NodeId;
use crate::style::{PaintStyle, TextDecoration};
use crate::{Fragment, GlyphOrientation, GlyphRunView, Line};
use std::collections::HashMap;
use std::ops::Range;

/// A resolved solid stroke in container-relative logical coordinates.
/// Its rectangle includes the accepted line's block offset. Rasterization,
/// clipping, physical rounding and skip-ink are the caller's responsibility.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorationRect {
    pub rect: LogicalRect,
    pub color: [u8; 4],
}

/// One source-owned processed UTF-8 interval and its final visual region.
/// This is decoration geometry, not a request to paint the glyph again.
/// Source parts of an indivisible grapheme may have coincident rectangles.
#[derive(Clone, Debug)]
pub struct PaintSpan<'a> {
    pub node: Option<NodeId>,
    pub text_range: Range<usize>,
    pub style: &'a PaintStyle,
    /// Like selection geometry: includes line block offset, final bidi and
    /// justification. Text-combine ranges run along the block axis.
    pub rect: LogicalRect,
    pub font: FontId,
    pub font_size: f32,
    pub metrics: FontMetrics,
    baseline: f32,
    under_sign: f32,
    combined: bool,
}

impl PaintSpan<'_> {
    /// A solid underline using explicit values or actual font metrics.
    /// Zero thickness/extent produces no paint. CSS decoration propagation
    /// and line-edge whitespace skipping are resolved by the caller.
    pub fn underline(&self) -> Option<DecorationRect> {
        self.resolve(
            self.style.underline?,
            self.metrics.underline_offset,
            self.metrics.underline_thickness,
        )
    }
    /// A solid strike-through using explicit values or actual font metrics.
    pub fn strikethrough(&self) -> Option<DecorationRect> {
        self.resolve(
            self.style.strikethrough?,
            self.metrics.strikeout_offset,
            self.metrics.strikeout_thickness,
        )
    }
    fn resolve(
        &self,
        value: TextDecoration,
        offset: f32,
        thickness: f32,
    ) -> Option<DecorationRect> {
        let thickness = value.thickness.unwrap_or(thickness).max(0.0);
        if thickness == 0.0
            || if self.combined {
                self.rect.block_size <= 0.0
            } else {
                self.rect.inline_size <= 0.0
            }
        {
            return None;
        }
        let center = self.baseline + self.under_sign * value.offset.unwrap_or(offset);
        let rect = if self.combined {
            LogicalRect {
                inline_start: center - thickness / 2.0,
                inline_size: thickness,
                ..self.rect
            }
        } else {
            LogicalRect {
                block_start: center - thickness / 2.0,
                block_size: thickness,
                ..self.rect
            }
        };
        Some(DecorationRect {
            rect,
            color: value.color.unwrap_or(self.style.color),
        })
    }
}

fn run_axis(line: &Line, run: GlyphRunView<'_>, offset: u32) -> (f32, f32, bool) {
    if run.orientation() == GlyphOrientation::Combined {
        let index = line
            .data
            .combine_spans
            .partition_point(|s| s.text.end <= offset);
        let combination = &line.data.combine_spans[index];
        let baseline = line.data.combine_geometry.baselines[index];
        let ltr = line.used_direction() == Direction::Ltr;
        let square = line
            .combination_at(offset)
            .expect("accepted composition")
            .square;
        return (
            square.inline_start
                + if ltr {
                    baseline
                } else {
                    combination.em - baseline
                },
            if ltr { 1.0 } else { -1.0 },
            true,
        );
    }
    let sign = if run.orientation() == GlyphOrientation::Upright {
        if line.writing_mode() == WritingMode::VerticalLr {
            -1.0
        } else {
            1.0
        }
    } else {
        run.glyph_transform().block_y
    };
    (line.block_offset() + run.baseline(), sign, false)
}

impl Line {
    /// Retained source paint and resolved decoration geometry. The returned
    /// source-ordered spans borrow this accepted line (including first-line
    /// styles), and include actual fallback instance metrics. Tabs use their
    /// style's primary font. Atomics and zero-width collapsed regions are absent;
    /// surviving hanging whitespace retains its selection advance.
    ///
    /// Builds shared source geometry once per call; retain the spans for repeated paint.
    /// Glyphs must still be drawn once from `fragments()`/`paint_style()`.
    /// This does not implement CSS decorating-box propagation or skip-ink.
    pub fn paint_spans(&self) -> Vec<PaintSpan<'_>> {
        let segments = crate::hit::paint_segments(self);
        let mut runs: Vec<_> = self
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(run) = f {
                    Some(run)
                } else {
                    None
                }
            })
            .collect();
        runs.sort_by_key(|r| r.text_range().start);
        let box_shifts: HashMap<_, _> = self
            .fragments
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                if let crate::line::fragments::RecordKind::InlineBox { box_index, .. } = r.kind {
                    Some((box_index, self.block_shifts[i].to_f32()))
                } else {
                    None
                }
            })
            .collect();
        let tab_parents: HashMap<_, _> = self
            .tabs
            .iter()
            .map(|tab| {
                let unit = &self.data.units[tab.unit as usize];
                (unit.text.start, unit.parent_box)
            })
            .collect();
        let mut result = Vec::new();
        for (text, rect) in segments {
            let first = self
                .data
                .items
                .partition_point(|i| i.text.end <= text.start);
            for item in &self.data.items[first..] {
                if item.text.start >= text.end {
                    break;
                }
                if !matches!(item.kind, ItemKind::Text | ItemKind::Tab) {
                    continue;
                }
                let from = text.start.max(item.text.start);
                let to = text.end.min(item.text.end);
                if from >= to {
                    continue;
                }
                let style = &self.data.styles[item.style as usize];
                if rect.inline_size == 0.0
                    && self.data.text[from as usize..to as usize]
                        .chars()
                        .all(char::is_whitespace)
                    && style.white_space_collapse == crate::style::WhiteSpaceCollapse::Collapse
                {
                    continue;
                }
                let index = runs.partition_point(|r| r.text_range().end <= from as usize);
                let run = runs
                    .get(index)
                    .copied()
                    .filter(|r| r.text_range().start <= from as usize);
                let primary = self.data.style_metrics[item.style as usize];
                let (font, font_size, metrics, baseline, under_sign, combined) = if let Some(run) =
                    run
                {
                    let (base, sign, combined) = run_axis(self, run, from);
                    (
                        run.font(),
                        run.font_size(),
                        run.metrics(),
                        base,
                        sign,
                        combined,
                    )
                } else {
                    // A tab can have no shaped glyph/run. Retain its containing
                    // inline's accepted baseline rather than the root baseline.
                    let parent = tab_parents.get(&from).copied().flatten();
                    let shift = parent
                        .and_then(|b| box_shifts.get(&b))
                        .copied()
                        .unwrap_or(0.0);
                    let combined = self.combination_at(from);
                    if let Some(square) = combined {
                        let index = self
                            .data
                            .combine_spans
                            .partition_point(|s| s.text.end <= from);
                        let base = self.data.combine_geometry.baselines[index];
                        let ltr = self.used_direction() == Direction::Ltr;
                        (
                            primary.font,
                            primary.size,
                            primary.metrics,
                            square.square.inline_start
                                + if ltr {
                                    base
                                } else {
                                    square.square.inline_size - base
                                },
                            if ltr { 1.0 } else { -1.0 },
                            true,
                        )
                    } else {
                        let under_sign = if self.writing_mode() == WritingMode::VerticalLr {
                            -1.0
                        } else {
                            1.0
                        };
                        let orientation = crate::shape::orientation::resolve(
                            self.writing_mode(),
                            style.text_orientation,
                            '\t',
                        );
                        // Mixed tabs use the same alphabetic origin as
                        // sideways glyphs within a central-baseline inline.
                        let center_shift = if orientation == GlyphOrientation::SidewaysClockwise
                            && matches!(
                                self.writing_mode(),
                                WritingMode::VerticalRl | WritingMode::VerticalLr
                            )
                            && style.text_orientation != crate::style::TextOrientation::Sideways
                        {
                            under_sign * (primary.metrics.ascent - primary.metrics.descent) / 2.0
                        } else {
                            0.0
                        };
                        (
                            primary.font,
                            primary.size,
                            primary.metrics,
                            self.block_offset() + self.baseline.to_f32() + shift + center_shift,
                            under_sign,
                            false,
                        )
                    }
                };
                result.push(PaintSpan {
                    node: item.node,
                    text_range: from as usize..to as usize,
                    style: &style.paint,
                    rect,
                    font,
                    font_size,
                    metrics,
                    baseline,
                    under_sign,
                    combined,
                });
            }
        }
        result.sort_by_key(|s| s.text_range.start);
        result
    }
}
