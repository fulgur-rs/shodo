//! Block geometry shared by scalar candidate probes and retained output.
//! Annotation ink may overflow available leading without increasing advance.
use super::measure::{RubyFragmentMeasure, RubyMeasure};
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::line::fragments::{FragmentRecord, GlyphSource, RecordKind};
use crate::paragraph::ParagraphData;
use crate::shape::ShapedRun;
use crate::style::{LineHeight, TextOrientation};
use crate::{RubyAlign, RubyPosition};
use std::collections::HashMap;
use std::ops::Range;

#[path = "geometry_index.rs"]
mod index;

#[cfg(test)]
#[path = "tests/bounds.rs"]
mod bounds_tests;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    pub(crate) top: LayoutUnit,
    pub(crate) bottom: LayoutUnit,
}
impl Bounds {
    pub(crate) fn height(self, sat: &mut Saturation) -> LayoutUnit {
        self.bottom.sub(self.top, sat).max(LayoutUnit::ZERO)
    }
    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            top: self.top.min(other.top),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

pub(crate) struct Frame<'a> {
    data: &'a ParagraphData,
    records: &'a [FragmentRecord],
    runs: &'a [ShapedRun],
    baseline: LayoutUnit,
    shifts: &'a [LayoutUnit],
    pub(crate) block_size: LayoutUnit,
    boxes: HashMap<u32, usize>,
    parents: HashMap<u32, Option<u32>>,
}
impl<'a> Frame<'a> {
    pub(crate) fn new(
        data: &'a ParagraphData,
        units: Range<usize>,
        records: &'a [FragmentRecord],
        runs: &'a [ShapedRun],
        baseline: LayoutUnit,
        shifts: &'a [LayoutUnit],
        block_size: LayoutUnit,
    ) -> Self {
        let boxes = records
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r.kind {
                RecordKind::InlineBox { box_index, .. } => Some((box_index, i)),
                _ => None,
            })
            .collect();
        let parents = data.units[units]
            .iter()
            .map(|u| (u.item, u.parent_box))
            .collect();
        Self {
            data,
            records,
            runs,
            baseline,
            shifts,
            block_size,
            boxes,
            parents,
        }
    }

    fn font_bounds(&self, style: u32, center: LayoutUnit, sat: &mut Saturation) -> Bounds {
        let (a, d) = font_extents(self.data, style, sat);
        Bounds {
            top: center.sub(a, sat),
            bottom: center.add(d, sat),
        }
    }

    pub(crate) fn box_content(&self, box_index: Option<u32>, sat: &mut Saturation) -> Bounds {
        let style = box_index.map_or(0, |b| self.data.boxes[b as usize].style);
        let shift = box_index
            .and_then(|b| self.boxes.get(&b))
            .map_or(LayoutUnit::ZERO, |i| self.shifts[*i]);
        self.font_bounds(style, self.baseline.add(shift, sat), sat)
    }

    pub(crate) fn column_content(
        &self,
        fragment: &RubyFragmentMeasure,
        column: usize,
        sat: &mut Saturation,
    ) -> Bounds {
        let ruby = &self.data.ruby.containers[fragment.container];
        self.box_content(
            ruby.columns[fragment.column_start + column]
                .box_index
                .or(ruby.box_index),
            sat,
        )
    }

    fn owner(&self, record: &FragmentRecord) -> Option<u32> {
        match record.kind {
            RecordKind::InlineBox { box_index, .. } => Some(box_index),
            RecordKind::Glyphs { item, .. } => self.parents.get(&item).copied().flatten(),
            RecordKind::Atomic { unit, .. } => self.data.units[unit as usize].parent_box,
            RecordKind::Anchor { .. } => None,
        }
    }

    pub(crate) fn record_bounds(&self, index: usize, sat: &mut Saturation) -> Option<Bounds> {
        let center = self.baseline.add(self.shifts[index], sat);
        let (above, below) = record_extents(self.data, &self.records[index], self.runs, sat)?;
        Some(Bounds {
            top: center.sub(above, sat),
            bottom: center.add(below, sat),
        })
    }
}

/// Actual font-content extents in logical block coordinates, before displacement.
pub(crate) fn font_extents(
    data: &ParagraphData,
    style: u32,
    sat: &mut Saturation,
) -> (LayoutUnit, LayoutUnit) {
    let s = &data.styles[style as usize];
    let m = data.style_metrics[style as usize];
    let upright = matches!(
        data.style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) && s.text_orientation != TextOrientation::Sideways;
    let (a, d) = if upright {
        m.vertical_metrics
            .map_or((s.font_size / 2.0, s.font_size / 2.0), |m| {
                (m.ascent, m.descent)
            })
    } else {
        (m.metrics.ascent, m.metrics.descent)
    };
    fixed_extents(data, a, d, sat)
}
fn fixed_extents(
    data: &ParagraphData,
    mut a: f32,
    mut d: f32,
    sat: &mut Saturation,
) -> (LayoutUnit, LayoutUnit) {
    if data.style.writing_mode == WritingMode::VerticalLr {
        std::mem::swap(&mut a, &mut d);
    }
    (
        LayoutUnit::from_f32_round(a, sat),
        LayoutUnit::from_f32_round(d, sat),
    )
}
pub(crate) fn record_extents(
    data: &ParagraphData,
    record: &FragmentRecord,
    runs: &[ShapedRun],
    sat: &mut Saturation,
) -> Option<(LayoutUnit, LayoutUnit)> {
    Some(match &record.kind {
        RecordKind::Glyphs {
            run, source, item, ..
        } => {
            let run = match source {
                GlyphSource::Overlay { run: Some(i), .. } => &runs[*i as usize],
                _ => &data.runs[*run as usize],
            };
            let m = run
                .instance
                .metrics
                .unwrap_or_else(|| data.fonts.metrics(run.font, run.font_size));
            let (a, d) = match run.orientation {
                crate::shape::orientation::RunOrientation::Upright => run
                    .instance
                    .vertical_metrics
                    .map_or((run.font_size / 2.0, run.font_size / 2.0), |v| {
                        (v.ascent, v.descent)
                    }),
                crate::shape::orientation::RunOrientation::Combined => {
                    let size = data.styles[data.items[*item as usize].style as usize].font_size;
                    (size / 2.0, size / 2.0)
                }
                _ => (m.ascent, m.descent),
            };
            fixed_extents(data, a, d, sat)
        }
        RecordKind::InlineBox { box_index, .. } => {
            let b = &data.boxes[*box_index as usize];
            let (a, d) = font_extents(data, b.style, sat);
            (
                a.add(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_start + b.edges.border.block_start,
                        sat,
                    ),
                    sat,
                ),
                d.add(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_end + b.edges.border.block_end,
                        sat,
                    ),
                    sat,
                ),
            )
        }
        RecordKind::Atomic { size, node, .. } => {
            let height = size.block_size + size.margins.block_start + size.margins.block_end;
            let baseline = size.baseline.unwrap_or(
                if data.baseline_kind(*node) == Some(crate::geometry::BaselineKind::Central) {
                    height / 2.0
                } else {
                    height
                },
            );
            fixed_extents(data, baseline, height - baseline, sat)
        }
        RecordKind::Anchor { .. } => return None,
    })
}

/// Reserved mark outsets from the primary font edge. Before-side outsets
/// are negative, so ordinary bounds unions aggregate the outward extremes.
pub(crate) fn record_emphasis(
    data: &ParagraphData,
    record: &FragmentRecord,
    runs: &[ShapedRun],
    sat: &mut Saturation,
) -> [Option<Bounds>; 2] {
    let RecordKind::Glyphs {
        run, source, item, ..
    } = &record.kind
    else {
        return [None, None];
    };
    let style = data.items[*item as usize].style;
    let Some(mark) = data.styles[style as usize].text_emphasis else {
        return [None, None];
    };
    let run = match source {
        GlyphSource::Overlay { run: Some(i), .. } => &runs[*i as usize],
        _ => &data.runs[*run as usize],
    };
    let upright = run.orientation == crate::shape::orientation::RunOrientation::Upright;
    let size = data.style_metrics[style as usize].size;
    let (a, d) = if run.orientation == crate::shape::orientation::RunOrientation::Combined {
        let half = size / 2.0;
        if crate::line::metrics::emphasis_over(mark.position, data.style.writing_mode) {
            (size, half)
        } else {
            (half, size)
        }
    } else {
        crate::line::metrics::emphasized(data, style, run.font, upright, (0.0, 0.0))
    };
    let before = crate::line::metrics::emphasis_over(mark.position, data.style.writing_mode)
        != (data.style.writing_mode == WritingMode::VerticalLr);
    let (a, d) = fixed_extents(data, a, d, sat);
    let (primary_a, primary_d) =
        record_primary_extents(data, record, runs, sat).expect("glyph primary edges");
    if before {
        let edge = primary_a.sub(a, sat);
        [
            Some(Bounds {
                top: edge,
                bottom: edge,
            }),
            None,
        ]
    } else {
        let edge = d.sub(primary_d, sat);
        [
            None,
            Some(Bounds {
                top: edge,
                bottom: edge,
            }),
        ]
    }
}

/// Primary font edges used by emphasis painting, in logical block coordinates.
pub(crate) fn record_primary_extents(
    data: &ParagraphData,
    record: &FragmentRecord,
    runs: &[ShapedRun],
    sat: &mut Saturation,
) -> Option<(LayoutUnit, LayoutUnit)> {
    let RecordKind::Glyphs {
        run, source, item, ..
    } = &record.kind
    else {
        return None;
    };
    let run = match source {
        GlyphSource::Overlay { run: Some(i), .. } => &runs[*i as usize],
        _ => &data.runs[*run as usize],
    };
    let metrics = data.style_metrics[data.items[*item as usize].style as usize];
    let half = metrics.size / 2.0;
    let (a, d) = match run.orientation {
        crate::shape::orientation::RunOrientation::Combined => (half, half),
        crate::shape::orientation::RunOrientation::Upright => metrics
            .vertical_metrics
            .map_or((half, half), |v| (v.ascent, v.descent)),
        _ => (metrics.metrics.ascent, metrics.metrics.descent),
    };
    Some(fixed_extents(data, a, d, sat))
}

pub(crate) struct LaneBlock {
    pub(crate) block: LayoutUnit,
    pub(crate) inline_size: Option<LayoutUnit>,
}
pub(crate) struct BlockLayout {
    pub(crate) lanes: Vec<Vec<LaneBlock>>,
    pub(crate) shift: LayoutUnit,
    pub(crate) advance: LayoutUnit,
    pub(crate) emphasis_offsets: Vec<(LayoutUnit, LayoutUnit)>,
}

/// Resolve line-relative sides once for both width allowances and placement.
/// Horizontal InterCharacter levels do not consume an alternating side.
pub(crate) fn level_sides(data: &ParagraphData, ruby: &super::prepare::PreparedRuby) -> Vec<bool> {
    let mut before = vec![true; ruby.levels.len()];
    let mut previous = None;
    for (i, style) in ruby.levels.iter().enumerate() {
        if super::measure::inter_character(data, *style) {
            continue;
        }
        let over = match style.position {
            RubyPosition::Under => false,
            RubyPosition::Alternate | RubyPosition::AlternateUnder => {
                previous.map_or(style.position == RubyPosition::Alternate, |p: bool| !p)
            }
            _ => true,
        };
        previous = Some(over);
        before[i] = over != (data.style.writing_mode == WritingMode::VerticalLr);
    }
    before
}

fn line_height(
    data: &ParagraphData,
    ruby: &super::prepare::PreparedRuby,
    sat: &mut Saturation,
) -> LayoutUnit {
    let style = ruby.box_index.map_or(0, |b| data.boxes[b as usize].style) as usize;
    let s = &data.styles[style];
    let m = data.style_metrics[style];
    let height = match s.line_height {
        LineHeight::Px(v) => v,
        LineHeight::Number(n) => n * s.font_size,
        LineHeight::Normal => {
            if matches!(
                data.style.writing_mode,
                WritingMode::VerticalRl | WritingMode::VerticalLr
            ) && s.text_orientation != TextOrientation::Sideways
            {
                m.vertical_metrics
                    .map_or(s.font_size, |v| v.ascent + v.descent + v.line_gap)
            } else {
                m.metrics.ascent + m.metrics.descent + m.metrics.line_gap
            }
        }
    };
    LayoutUnit::from_f32_ceil(height, sat)
}

pub(crate) struct Tracks {
    pub(crate) lanes: Vec<LaneBlock>,
    pub(crate) base: Bounds,
    pub(crate) whole: Bounds,
    pub(crate) contribution: Bounds,
    pub(crate) annotation_extents: (LayoutUnit, LayoutUnit),
}

/// The same track stack serves indexed clearance and retained child output.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tracks(
    data: &ParagraphData,
    ruby: &super::prepare::PreparedRuby,
    column_start: usize,
    bases: &[Range<usize>],
    lanes: &[super::measure::LaneMeasure],
    mut base: Bounds,
    contents: &[Bounds],
    right_columns: &std::collections::HashMap<(usize, usize), usize>,
    heights: &[LayoutUnit],
    has_content: bool,
    original_base: Bounds,
    emphasis: [Option<Bounds>; 2],
    sat: &mut Saturation,
) -> Tracks {
    let mut selected: Vec<_> = lanes
        .iter()
        .map(|_| LaneBlock {
            block: LayoutUnit::ZERO,
            inline_size: None,
        })
        .collect();
    if !has_content {
        let empty = Bounds {
            top: LayoutUnit::ZERO,
            bottom: LayoutUnit::ZERO,
        };
        return Tracks {
            lanes: selected,
            base: empty,
            whole: empty,
            contribution: empty,
            annotation_extents: (LayoutUnit::ZERO, LayoutUnit::ZERO),
        };
    }
    let before = level_sides(data, ruby);
    let mut levels = vec![LayoutUnit::ZERO; ruby.levels.len()];
    for (i, (lane, height)) in lanes.iter().zip(heights).enumerate() {
        let source = &ruby.lanes[lane.lane];
        let style = ruby.levels[source.level];
        if super::measure::inter_character(data, style) {
            let columns = super::measure::local_columns(&source.columns, column_start, bases.len());
            let right = super::measure::rightmost(right_columns, &columns);
            let content = contents[right];
            let height = lane.width.max(content.height(sat));
            let centered = if style.align == RubyAlign::Start {
                LayoutUnit::ZERO
            } else {
                LayoutUnit::from_raw(content.height(sat).sub(height, sat).raw() / 2)
            };
            let top = content.top.add(centered, sat);
            selected[i] = LaneBlock {
                block: top,
                inline_size: Some(height),
            };
            base = base.union(Bounds {
                top,
                bottom: top.add(height, sat),
            });
        } else {
            levels[source.level] = levels[source.level].max(*height);
        }
    }
    let mut over = LayoutUnit::ZERO;
    let mut under = LayoutUnit::ZERO;
    // Keep each side's prefix separate: empty levels still affect the side
    // alternation, and inter-character lanes have already expanded `base`.
    let mut offsets = Vec::with_capacity(levels.len());
    for (level, height) in levels.iter().enumerate() {
        #[cfg(test)]
        track_tests::visit();
        offsets.push(if before[level] { over } else { under });
        if before[level] {
            over = over.add(*height, sat);
        } else {
            under = under.add(*height, sat);
        }
    }
    for (i, (lane, child_height)) in lanes.iter().zip(heights).enumerate() {
        #[cfg(test)]
        track_tests::visit();
        if selected[i].inline_size.is_some() {
            continue;
        }
        let level = ruby.lanes[lane.lane].level;
        selected[i].block = if before[level] {
            base.top.sub(offsets[level], sat).sub(*child_height, sat)
        } else {
            base.bottom.add(offsets[level], sat)
        };
    }
    let whole = Bounds {
        top: base.top.sub(over, sat),
        bottom: base.bottom.add(under, sat),
    };
    let leading = line_height(data, ruby, sat)
        .sub(base.height(sat), sat)
        .max(LayoutUnit::ZERO);
    let half = LayoutUnit::from_raw(leading.raw() / 2);
    let remainder = leading.sub(half, sat);
    let total = over.add(under, sat);
    let extra = total.sub(leading, sat).max(LayoutUnit::ZERO);
    let extra_over = if total == LayoutUnit::ZERO {
        LayoutUnit::ZERO
    } else {
        LayoutUnit::from_raw(
            ((i64::from(extra.raw()) * i64::from(over.raw())) / i64::from(total.raw())) as i32,
        )
    };
    let mut own = Bounds {
        top: base.top.sub(half, sat).sub(extra_over, sat),
        bottom: base
            .bottom
            .add(remainder, sat)
            .add(extra.sub(extra_over, sat), sat),
    };
    let annotation_extents = (
        original_base.top.sub(whole.top, sat).max(LayoutUnit::ZERO),
        whole
            .bottom
            .sub(original_base.bottom, sat)
            .max(LayoutUnit::ZERO),
    );
    // Only the occupied side moves its marks to the outer annotation edge.
    // Independent marks are already included by the ordinary line profile.
    if annotation_extents.0 != LayoutUnit::ZERO
        && let Some(mark) = emphasis[0]
    {
        let edge = whole.top.add(mark.top, sat);
        own = own.union(Bounds {
            top: edge,
            bottom: edge,
        });
    }
    if annotation_extents.1 != LayoutUnit::ZERO
        && let Some(mark) = emphasis[1]
    {
        let edge = whole.bottom.add(mark.bottom, sat);
        own = own.union(Bounds {
            top: edge,
            bottom: edge,
        });
    }
    Tracks {
        lanes: selected,
        base,
        whole,
        contribution: own,
        annotation_extents,
    }
}

/// `heights` are actual scalar child measurements or retained child advances.
pub(crate) fn layout(
    frame: &Frame<'_>,
    measure: &RubyMeasure,
    heights: &[Vec<LayoutUnit>],
    sat: &mut Saturation,
) -> BlockLayout {
    let data = frame.data;
    let mut lanes = Vec::with_capacity(measure.fragments.len());
    let mut offsets = Vec::new();
    let bounds = index::RecordBounds::new(frame, sat);
    let mut whole = index::CompletedBounds::default();
    let mut contribution = Bounds {
        top: LayoutUnit::ZERO,
        bottom: frame.block_size,
    };
    for (fragment, heights) in measure.fragments.iter().zip(heights) {
        let ruby = &data.ruby.containers[fragment.container];
        let mut base: Option<Bounds> = None;
        for (i, units) in fragment.bases.iter().enumerate() {
            if units.is_empty() {
                continue;
            }
            let mut area = frame.column_content(fragment, i, sat);
            if let Some(other) = bounds.column(
                frame,
                ruby.columns[fragment.column_start + i].box_index,
                units,
            ) {
                area = area.union(other);
            }
            base = Some(base.map_or(area, |b| b.union(area)));
        }
        let mut base = base.unwrap_or_else(|| frame.box_content(ruby.box_index, sat));
        let original_base = base;
        let emphasis = bounds.emphasis(frame, &fragment.units);
        if fragment.has_content {
            base = whole.include_children(&fragment.units, base);
        }
        let contents: Vec<_> = (0..fragment.bases.len())
            .map(|i| frame.column_content(fragment, i, sat))
            .collect();
        let result = tracks(
            data,
            ruby,
            fragment.column_start,
            &fragment.bases,
            &fragment.lanes,
            base,
            &contents,
            &fragment.right_columns,
            heights,
            fragment.has_content,
            original_base,
            emphasis,
            sat,
        );
        if fragment.has_content {
            whole.insert(fragment.units.clone(), fragment.container, result.whole);
            contribution = contribution.union(result.contribution);
        }
        if emphasis.iter().any(Option::is_some) {
            offsets.push((
                fragment.units.clone(),
                [
                    (result.annotation_extents.0 != LayoutUnit::ZERO).then_some(result.whole.top),
                    (result.annotation_extents.1 != LayoutUnit::ZERO)
                        .then_some(result.whole.bottom),
                ],
            ));
        }
        lanes.push(result.lanes);
    }
    BlockLayout {
        lanes,
        shift: LayoutUnit::ZERO.sub(contribution.top, sat),
        advance: contribution.height(sat),
        emphasis_offsets: index::emphasis_offsets(frame, offsets, sat),
    }
}

#[cfg(test)]
#[path = "tests/tracks.rs"]
mod track_tests;
