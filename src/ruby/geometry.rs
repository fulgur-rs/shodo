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

#[derive(Clone, Copy)]
pub(crate) struct Bounds {
    pub(crate) top: LayoutUnit,
    pub(crate) bottom: LayoutUnit,
}
impl Bounds {
    pub(crate) fn height(self, sat: &mut Saturation) -> LayoutUnit {
        self.bottom.sub(self.top, sat).max(LayoutUnit::ZERO)
    }
    fn union(self, other: Self) -> Self {
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
        let s = &self.data.styles[style as usize];
        let m = self.data.style_metrics[style as usize];
        let upright = self.data.style.writing_mode != WritingMode::HorizontalTb
            && s.text_orientation != TextOrientation::Sideways;
        let (a, d) = if upright {
            m.vertical_metrics
                .map_or((s.font_size / 2.0, s.font_size / 2.0), |m| {
                    (m.ascent, m.descent)
                })
        } else {
            (m.metrics.ascent, m.metrics.descent)
        };
        self.bounds(center, a, d, sat)
    }

    fn bounds(&self, center: LayoutUnit, mut a: f32, mut d: f32, sat: &mut Saturation) -> Bounds {
        if self.data.style.writing_mode == WritingMode::VerticalLr {
            std::mem::swap(&mut a, &mut d);
        }
        Bounds {
            top: center.sub(LayoutUnit::from_f32_round(a, sat), sat),
            bottom: center.add(LayoutUnit::from_f32_round(d, sat), sat),
        }
    }

    fn box_content(&self, box_index: Option<u32>, sat: &mut Saturation) -> Bounds {
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
        self.box_content(ruby.columns[column].box_index.or(ruby.box_index), sat)
    }

    fn owner(&self, record: &FragmentRecord) -> Option<u32> {
        match record.kind {
            RecordKind::InlineBox { box_index, .. } => Some(box_index),
            RecordKind::Glyphs { item, .. } => self.parents.get(&item).copied().flatten(),
            RecordKind::Atomic { unit, .. } => self.data.units[unit as usize].parent_box,
            RecordKind::Anchor { .. } => None,
        }
    }

    fn belongs(
        &self,
        record: &FragmentRecord,
        box_index: Option<u32>,
        units: &Range<usize>,
    ) -> bool {
        if let Some(box_index) = box_index {
            let mut owner = self.owner(record);
            while let Some(index) = owner {
                if index == box_index {
                    return true;
                }
                owner = self.data.boxes[index as usize].parent;
            }
            false
        } else {
            match &record.kind {
                RecordKind::Glyphs { text, .. } => self.data.units[units.clone()]
                    .iter()
                    .any(|u| u.text.start < text.end && text.start < u.text.end),
                RecordKind::Atomic { unit, .. } => units.contains(&(*unit as usize)),
                _ => false,
            }
        }
    }

    fn record_bounds(&self, index: usize, sat: &mut Saturation) -> Option<Bounds> {
        let center = self.baseline.add(self.shifts[index], sat);
        match &self.records[index].kind {
            RecordKind::Glyphs {
                run, source, item, ..
            } => {
                let run = match source {
                    GlyphSource::Overlay { run: Some(i), .. } => &self.runs[*i as usize],
                    _ => &self.data.runs[*run as usize],
                };
                let m = run
                    .instance
                    .metrics
                    .unwrap_or_else(|| self.data.fonts.metrics(run.font, run.font_size));
                let (a, d) = match run.orientation {
                    crate::shape::orientation::RunOrientation::Upright => run
                        .instance
                        .vertical_metrics
                        .map_or((run.font_size / 2.0, run.font_size / 2.0), |v| {
                            (v.ascent, v.descent)
                        }),
                    crate::shape::orientation::RunOrientation::Combined => {
                        let size = self.data.styles[self.data.items[*item as usize].style as usize]
                            .font_size;
                        (size / 2.0, size / 2.0)
                    }
                    _ => (m.ascent, m.descent),
                };
                Some(self.bounds(center, a, d, sat))
            }
            RecordKind::InlineBox { box_index, .. } => {
                let b = &self.data.boxes[*box_index as usize];
                let mut bounds = self.font_bounds(b.style, center, sat);
                bounds.top = bounds.top.sub(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_start + b.edges.border.block_start,
                        sat,
                    ),
                    sat,
                );
                bounds.bottom = bounds.bottom.add(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_end + b.edges.border.block_end,
                        sat,
                    ),
                    sat,
                );
                Some(bounds)
            }
            RecordKind::Atomic { size, node, .. } => {
                let height = size.block_size + size.margins.block_start + size.margins.block_end;
                let baseline = size.baseline.unwrap_or(
                    if self.data.baseline_kind(*node)
                        == Some(crate::geometry::BaselineKind::Central)
                    {
                        height / 2.0
                    } else {
                        height
                    },
                );
                Some(self.bounds(center, baseline, height - baseline, sat))
            }
            RecordKind::Anchor { .. } => None,
        }
    }

    fn column_area(
        &self,
        fragment: &RubyFragmentMeasure,
        column: usize,
        sat: &mut Saturation,
    ) -> Bounds {
        let ruby = &self.data.ruby.containers[fragment.container];
        let mut bounds = self.column_content(fragment, column, sat);
        for (i, record) in self.records.iter().enumerate() {
            if self.belongs(
                record,
                ruby.columns[column].box_index,
                &fragment.bases[column],
            ) && let Some(other) = self.record_bounds(i, sat)
            {
                bounds = bounds.union(other);
            }
        }
        bounds
    }

    fn line_height(&self, fragment: &RubyFragmentMeasure, sat: &mut Saturation) -> LayoutUnit {
        let ruby = &self.data.ruby.containers[fragment.container];
        let style = ruby
            .box_index
            .map_or(0, |b| self.data.boxes[b as usize].style) as usize;
        let s = &self.data.styles[style];
        let m = self.data.style_metrics[style];
        let height = match s.line_height {
            LineHeight::Px(v) => v,
            LineHeight::Number(n) => n * s.font_size,
            LineHeight::Normal => {
                if self.data.style.writing_mode != WritingMode::HorizontalTb
                    && s.text_orientation != TextOrientation::Sideways
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
}

pub(crate) struct LaneBlock {
    pub(crate) block: LayoutUnit,
    pub(crate) inline_size: Option<LayoutUnit>,
}
pub(crate) struct BlockLayout {
    pub(crate) lanes: Vec<Vec<LaneBlock>>,
    pub(crate) shift: LayoutUnit,
    pub(crate) advance: LayoutUnit,
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
    let mut whole: Vec<(Range<usize>, Bounds)> = Vec::new();
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
            let area = frame.column_area(fragment, i, sat);
            base = Some(base.map_or(area, |b| b.union(area)));
        }
        let mut base = base.unwrap_or_else(|| frame.box_content(ruby.box_index, sat));
        for (units, area) in &whole {
            if fragment.units.start <= units.start && units.end <= fragment.units.end {
                base = base.union(*area);
            }
        }
        let mut selected: Vec<_> = fragment
            .lanes
            .iter()
            .map(|_| LaneBlock {
                block: LayoutUnit::ZERO,
                inline_size: None,
            })
            .collect();
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
        let mut levels = vec![LayoutUnit::ZERO; ruby.levels.len()];
        for (i, (lane, height)) in fragment.lanes.iter().zip(heights).enumerate() {
            let source = &ruby.lanes[lane.lane];
            let style = ruby.levels[source.level];
            if super::measure::inter_character(data, style) {
                let first = source.columns.start.max(
                    fragment
                        .bases
                        .iter()
                        .position(|r| !r.is_empty())
                        .unwrap_or(source.columns.start),
                );
                let last = source.columns.end.min(
                    fragment
                        .bases
                        .iter()
                        .rposition(|r| !r.is_empty())
                        .map_or(source.columns.end, |i| i + 1),
                );
                let right = super::measure::rightmost(data, ruby, &(first..last));
                let content = frame.column_content(fragment, right, sat);
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
        for (level, height) in levels.iter().enumerate() {
            for (i, (lane, child_height)) in fragment.lanes.iter().zip(heights).enumerate() {
                if ruby.lanes[lane.lane].level != level || selected[i].inline_size.is_some() {
                    continue;
                }
                selected[i].block = if before[level] {
                    base.top.sub(over, sat).sub(*child_height, sat)
                } else {
                    base.bottom.add(under, sat)
                };
            }
            if before[level] {
                over = over.add(*height, sat);
            } else {
                under = under.add(*height, sat);
            }
        }
        whole.push((
            fragment.units.clone(),
            Bounds {
                top: base.top.sub(over, sat),
                bottom: base.bottom.add(under, sat),
            },
        ));
        let leading = frame
            .line_height(fragment, sat)
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
        let own = Bounds {
            top: base.top.sub(half, sat).sub(extra_over, sat),
            bottom: base
                .bottom
                .add(remainder, sat)
                .add(extra.sub(extra_over, sat), sat),
        };
        contribution = contribution.union(own);
        lanes.push(selected);
    }
    BlockLayout {
        lanes,
        shift: LayoutUnit::ZERO.sub(contribution.top, sat),
        advance: contribution.height(sat),
    }
}
