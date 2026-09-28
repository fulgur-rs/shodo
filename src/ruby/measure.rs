//! Candidate column widths derived from the actual accepted base/lane windows.
use crate::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::{AtomicSizes, ParagraphData};
use std::ops::Range;

pub(crate) struct SpanWidth {
    pub(crate) columns: Range<usize>,
    pub(crate) width: LayoutUnit,
}

/// Extra spanning width expands all associated columns equally. Resolve short
/// spans first, then distribute fixed-point remainders in stable source order.
pub(crate) fn column_widths(
    base: &[LayoutUnit],
    spans: &[SpanWidth],
    sat: &mut Saturation,
) -> Vec<LayoutUnit> {
    let mut widths = base.to_vec();
    let mut spans: Vec<_> = spans.iter().collect();
    spans.sort_by_key(|span| (span.columns.len(), span.columns.start));
    for span in spans {
        let columns = &mut widths[span.columns.clone()];
        if columns.is_empty() {
            continue;
        }
        let current = columns
            .iter()
            .fold(LayoutUnit::ZERO, |sum, w| sum.add(*w, sat));
        let extra = span.width.sub(current, sat).max(LayoutUnit::ZERO).raw() as u64;
        let each = extra / columns.len() as u64;
        let remainder = extra % columns.len() as u64;
        for (i, column) in columns.iter_mut().enumerate() {
            let amount = each + u64::from((i as u64) < remainder);
            *column = column.add(LayoutUnit::from_raw(amount as i32), sat);
        }
    }
    widths
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RubyMeasure {
    pub(crate) adjustment: LayoutUnit,
    pub(crate) fragments: Vec<RubyFragmentMeasure>,
}

#[derive(Clone, Debug)]
pub(crate) struct RubyFragmentMeasure {
    pub(crate) container: usize,
    pub(crate) units: Range<usize>,
    pub(crate) bases: Vec<Range<usize>>,
    pub(crate) base_widths: Vec<LayoutUnit>,
    pub(crate) base_columns: Vec<LayoutUnit>,
    pub(crate) cross_columns: Vec<LayoutUnit>,
    pub(crate) columns: Vec<LayoutUnit>,
    pub(crate) lanes: Vec<LaneMeasure>,
    pub(crate) adjustment: LayoutUnit,
}

#[derive(Clone, Debug)]
pub(crate) struct LaneMeasure {
    pub(crate) lane: usize,
    pub(crate) units: Range<usize>,
    pub(crate) width: LayoutUnit,
    pub(crate) cross_width: Option<LayoutUnit>,
}

fn intersect(a: &Range<usize>, b: &Range<usize>) -> Range<usize> {
    let start = a.start.max(b.start);
    start..a.end.min(b.end).max(start)
}

fn cut_at_or_after(ruby: &super::prepare::PreparedRuby, unit: usize) -> usize {
    ruby.cuts
        .partition_point(|c| c.unit < unit)
        .min(ruby.cuts.len() - 1)
}

fn cut_at_or_before(ruby: &super::prepare::PreparedRuby, unit: usize) -> usize {
    ruby.cuts
        .partition_point(|c| c.unit <= unit)
        .saturating_sub(1)
}

/// Measure actual base/lane windows. Partial scanning probes look ahead to the
/// next legal paired endpoint; accepted fragments use that same endpoint.
pub(crate) fn candidate(
    data: &ParagraphData,
    start: usize,
    end: usize,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> RubyMeasure {
    cx.ruby_ranges.begin(data, atomics);
    candidate_inner(data, start, end, atomics, cx, sat)
}

pub(crate) fn candidate_inner(
    data: &ParagraphData,
    start: usize,
    end: usize,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> RubyMeasure {
    if start >= end || data.ruby.containers.is_empty() {
        return RubyMeasure::default();
    }
    let mut through = end;
    for ruby in &data.ruby.containers {
        if start < ruby.units.end && ruby.units.start < through {
            let clipped = through.min(ruby.units.end);
            through = through.max(ruby.cuts[cut_at_or_after(ruby, clipped)].unit);
        }
    }
    let selected = start..through;
    let mut measure = RubyMeasure::default();
    for (container, ruby) in data.ruby.containers.iter().enumerate().rev() {
        let units = intersect(&selected, &ruby.units);
        if units.is_empty() {
            continue;
        }
        let begin = &ruby.cuts[cut_at_or_before(ruby, units.start)];
        let finish = &ruby.cuts[cut_at_or_after(ruby, units.end)];
        let bases: Vec<_> = ruby
            .columns
            .iter()
            .map(|c| intersect(&units, &c.units))
            .collect();
        let mut base_widths = Vec::with_capacity(bases.len());
        for (column, base) in ruby.columns.iter().zip(&bases) {
            let mut width = crate::line::ruby_base_width(
                data,
                column.units.clone(),
                base.clone(),
                atomics,
                cx,
                sat,
            );
            for nested in &measure.fragments {
                if base.start <= nested.units.start && nested.units.end <= base.end {
                    width = width.add(nested.adjustment, sat);
                }
            }
            base_widths.push(width);
        }
        let mut lanes = Vec::new();
        for (index, lane) in ruby.lanes.iter().enumerate() {
            let range = begin.lanes[index]..finish.lanes[index];
            if range.is_empty() {
                continue;
            }
            let child = &lane.paragraph.data;
            let raw = crate::line::ruby_range_width(child, range.clone(), atomics, cx, sat);
            let nested = candidate_inner(child, range.start, range.end, atomics, cx, sat);
            let width = raw.add(nested.adjustment, sat);
            let cross_width = inter_character(data, ruby.levels[lane.level]).then(|| {
                crate::line::range::block_size(child, range.clone(), &nested, atomics, cx, sat)
            });
            lanes.push(LaneMeasure {
                lane: index,
                units: range,
                width,
                cross_width,
            });
        }
        // Merge keeps the source-paired pieces; same-line width is one level
        // spanning the associated selected columns, not permanent raw merging.
        let mut separate = Vec::new();
        let first = bases.iter().position(|r| !r.is_empty()).unwrap_or(0);
        let last = bases
            .iter()
            .rposition(|r| !r.is_empty())
            .map_or(first, |i| i + 1);
        let selected_columns = first..last;
        let mut merged = vec![None; ruby.levels.len()];
        let mut cross_columns = vec![LayoutUnit::ZERO; bases.len()];
        for lane in &lanes {
            if let Some(width) = lane.cross_width {
                let columns = intersect(&ruby.lanes[lane.lane].columns, &selected_columns);
                if !columns.is_empty() {
                    let column = rightmost(data, ruby, &columns);
                    cross_columns[column] = cross_columns[column].add(width, sat);
                }
            }
        }
        for l in &lanes {
            let source = &ruby.lanes[l.lane];
            if l.cross_width.is_some() {
                continue;
            } else if merging(data, ruby.levels[source.level]) {
                let width = merged[source.level].get_or_insert(LayoutUnit::ZERO);
                *width = width.add(l.width, sat);
            } else {
                let columns = intersect(&source.columns, &selected_columns);
                if !columns.is_empty() {
                    separate.push(SpanWidth {
                        width: l
                            .width
                            .sub(
                                internal_cross(data, ruby, &cross_columns, &columns, sat),
                                sat,
                            )
                            .max(LayoutUnit::ZERO),
                        columns,
                    });
                }
            }
        }
        for width in merged.into_iter().flatten() {
            separate.push(SpanWidth {
                columns: selected_columns.clone(),
                width: width
                    .sub(
                        internal_cross(data, ruby, &cross_columns, &selected_columns, sat),
                        sat,
                    )
                    .max(LayoutUnit::ZERO),
            });
        }
        let base_columns = column_widths(&base_widths, &separate, sat);
        let columns: Vec<_> = base_columns
            .iter()
            .zip(&cross_columns)
            .map(|(base, cross)| base.add(*cross, sat))
            .collect();
        let raw_base = base_widths
            .iter()
            .fold(LayoutUnit::ZERO, |w, b| w.add(*b, sat));
        let width = columns.iter().fold(LayoutUnit::ZERO, |w, b| w.add(*b, sat));
        let adjustment = width.sub(raw_base, sat);
        measure.adjustment = measure.adjustment.add(adjustment, sat);
        measure.fragments.push(RubyFragmentMeasure {
            container,
            units,
            bases,
            base_widths,
            base_columns,
            cross_columns,
            columns,
            lanes,
            adjustment,
        });
    }
    if through > end {
        let full = crate::line::ruby_range_width(data, selected, atomics, cx, sat);
        let consumed = crate::line::ruby_range_width(data, start..end, atomics, cx, sat);
        measure.adjustment = measure.adjustment.add(full.sub(consumed, sat), sat);
    }
    measure
}

pub(crate) fn merging(data: &ParagraphData, style: super::RubyStyle) -> bool {
    style.merge == super::RubyMerge::Merge && !inter_character(data, style)
}

pub(crate) fn inter_character(data: &ParagraphData, style: super::RubyStyle) -> bool {
    style.position == super::RubyPosition::InterCharacter
        && data.style.writing_mode == crate::geometry::WritingMode::HorizontalTb
}

pub(crate) fn rightmost(
    data: &ParagraphData,
    ruby: &super::prepare::PreparedRuby,
    columns: &Range<usize>,
) -> usize {
    let direction = ruby.box_index.map_or(data.style.direction, |b| {
        data.styles[data.boxes[b as usize].style as usize].direction
    });
    if direction == crate::geometry::Direction::Ltr {
        columns.end - 1
    } else {
        columns.start
    }
}

pub(crate) fn internal_cross(
    data: &ParagraphData,
    ruby: &super::prepare::PreparedRuby,
    cross: &[LayoutUnit],
    columns: &Range<usize>,
    sat: &mut Saturation,
) -> LayoutUnit {
    if columns.is_empty() {
        return LayoutUnit::ZERO;
    }
    let right = rightmost(data, ruby, columns);
    columns
        .clone()
        .filter(|i| *i != right)
        .fold(LayoutUnit::ZERO, |w, i| w.add(cross[i], sat))
}

/// Reserve each column's additional space on its real closing box, or its
/// anonymous base boundary. Glyph advances remain owned by the shaper;
/// alignment moves them separately.
pub(crate) fn apply(
    data: &ParagraphData,
    start: usize,
    scan: &mut crate::line::Scan,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    if scan.ruby.is_some() || data.ruby.containers.is_empty() {
        return;
    }
    let measure = candidate(data, start, scan.end, atomics, cx, sat);
    for fragment in &measure.fragments {
        for (i, ((base, natural), column)) in fragment
            .bases
            .iter()
            .zip(&fragment.base_widths)
            .zip(&fragment.base_columns)
            .enumerate()
        {
            let extra = column.sub(*natural, sat);
            let ruby = &data.ruby.containers[fragment.container];
            let cross = fragment.cross_columns[i];
            if cross != LayoutUnit::ZERO {
                let base_box = ruby.columns[i].box_index;
                // Strong text may oppose the base's declared direction.
                // Reserve on the edge that actually reorders to physical right.
                let right_at_start = data.base_level % 2 == 1;
                let unit = base
                    .clone()
                    .find(|j| {
                        let u = &data.units[*j];
                        let same = u.level % 2 == data.base_level % 2;
                        match u.kind {
                            crate::analysis::units::UnitKind::Open { box_index }
                                if Some(box_index) == base_box =>
                            {
                                same == right_at_start
                            }
                            crate::analysis::units::UnitKind::Close { box_index }
                                if Some(box_index) == base_box =>
                            {
                                same != right_at_start
                            }
                            _ => false,
                        }
                    })
                    .or_else(|| {
                        base.clone().find(|i| {
                            matches!(
                                data.items[data.units[*i].item as usize].kind,
                                crate::analysis::ItemKind::RubyBoundary {
                                    boundary: super::builder::Boundary::BaseOpen(_),
                                    ..
                                }
                            )
                        })
                    });
                if let Some(unit) = unit {
                    scan.widths[unit - start] = scan.widths[unit - start].add(cross, sat);
                }
            }
            if extra == LayoutUnit::ZERO {
                continue;
            }
            let unit = base
                .clone()
                .rev()
                .find(|i| {
                    matches!(
                        data.units[*i].kind,
                        crate::analysis::units::UnitKind::Close { .. }
                            | crate::analysis::units::UnitKind::Cluster { .. }
                            | crate::analysis::units::UnitKind::Atomic { .. }
                    )
                })
                .or_else(|| {
                    base.clone().rev().find(|i| {
                        matches!(
                            data.items[data.units[*i].item as usize].kind,
                            crate::analysis::ItemKind::RubyBoundary {
                                boundary: super::builder::Boundary::BaseClose(_),
                                ..
                            }
                        )
                    })
                });
            if let Some(unit) = unit {
                scan.widths[unit - start] = scan.widths[unit - start].add(extra, sat);
            }
        }
    }
    scan.content = scan.content.add(measure.adjustment, sat);
    scan.ruby = Some(measure);
}
