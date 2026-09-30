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
    pub(crate) right_columns: std::collections::HashMap<(usize, usize), usize>,
    pub(crate) columns: Vec<LayoutUnit>,
    pub(crate) lanes: Vec<LaneMeasure>,
    pub(crate) level_overhang: Vec<(LayoutUnit, LayoutUnit)>,
    pub(crate) adjustment: LayoutUnit,
    pub(crate) whole_area: super::geometry::Bounds,
    pub(crate) contribution: super::geometry::Bounds,
    pub(crate) has_content: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct LaneMeasure {
    pub(crate) lane: usize,
    pub(crate) units: Range<usize>,
    pub(crate) width: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) cross_width: Option<LayoutUnit>,
    pub(crate) overhang: (LayoutUnit, LayoutUnit),
}

fn intersect(a: &Range<usize>, b: &Range<usize>) -> Range<usize> {
    let start = a.start.max(b.start);
    start..a.end.min(b.end).max(start)
}

/// Prepared columns are in source-unit order; trim empty boundary columns
/// exactly as the full-array first/last nonempty scan did.
fn selected_columns(ruby: &super::prepare::PreparedRuby, units: &Range<usize>) -> Range<usize> {
    let first = ruby.columns.partition_point(|c| c.units.end <= units.start);
    let last = ruby.columns.partition_point(|c| c.units.start < units.end);
    if first >= last {
        return 0..0;
    }
    let columns = &ruby.columns[first..last];
    let Some(start) = columns.iter().position(|c| !c.units.is_empty()) else {
        return 0..0;
    };
    let end = columns.iter().rposition(|c| !c.units.is_empty()).unwrap() + 1;
    first + start..first + end
}

/// Normalization groups lanes by level and orders nonoverlapping column spans
/// within each level. Return original indices for the dense paired-cut table.
fn selected_lanes<'a>(
    ruby: &'a super::prepare::PreparedRuby,
    columns: &'a Range<usize>,
) -> impl Iterator<Item = usize> + 'a {
    (0..ruby.levels.len()).flat_map(move |level| {
        let first = ruby.lanes.partition_point(|lane| lane.level < level);
        let last = ruby.lanes.partition_point(|lane| lane.level <= level);
        let lanes = &ruby.lanes[first..last];
        if columns.is_empty() {
            return first..first;
        }
        let start = lanes.partition_point(|lane| lane.columns.end <= columns.start);
        let end = lanes.partition_point(|lane| lane.columns.start < columns.end);
        first + start..first + end
    })
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
    let mut containers = Vec::new();
    data.ruby.intervals.intersecting(
        &data.ruby.containers,
        start,
        &mut through,
        |container, through| {
            let ruby = &data.ruby.containers[container];
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 1;
            }
            let clipped = (*through).min(ruby.units.end);
            *through = (*through).max(ruby.cuts[cut_at_or_after(ruby, clipped)].unit);
            containers.push(container);
        },
    );
    let selected = start..through;
    let mut measure = RubyMeasure::default();
    // Reverse structural traversal resolves children before parents. Index those
    // completed fragments by source start so siblings are never compared as
    // potential descendants of every subsequent column/container. Container
    // identity preserves ancestors that share a clipped continuation start.
    let mut completed = std::collections::BTreeMap::<(usize, usize), usize>::new();
    for container in containers.into_iter().rev() {
        let ruby = &data.ruby.containers[container];
        #[cfg(test)]
        {
            cx.ruby_measure_visits += 1;
        }
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
            for (_, &index) in completed.range((base.start, 0)..(base.end, 0)) {
                let nested = &measure.fragments[index];
                #[cfg(test)]
                {
                    cx.ruby_measure_visits += 1;
                }
                if nested.units.end <= base.end {
                    width = width.add(nested.adjustment, sat);
                }
            }
            base_widths.push(width);
        }
        let selected_columns = selected_columns(ruby, &units);
        let mut lanes = Vec::new();
        for index in selected_lanes(ruby, &selected_columns) {
            let lane = &ruby.lanes[index];
            #[cfg(test)]
            {
                cx.ruby_lane_visits += 1;
            }
            let range = begin.lanes[index]..finish.lanes[index];
            if range.is_empty() {
                continue;
            }
            let child = &lane.paragraph.data;
            let raw = crate::line::ruby_range_width(child, range.clone(), atomics, cx, sat);
            let nested = candidate_inner(child, range.start, range.end, atomics, cx, sat);
            let width = raw.add(nested.adjustment, sat);
            let block_size =
                crate::line::range::block_size(child, range.clone(), &nested, atomics, cx, sat);
            let cross_width = inter_character(data, ruby.levels[lane.level]).then_some(block_size);
            lanes.push(LaneMeasure {
                lane: index,
                units: range,
                width,
                block_size,
                cross_width,
                overhang: (LayoutUnit::ZERO, LayoutUnit::ZERO),
            });
        }
        // Merge keeps the source-paired pieces; same-line width is one level
        // spanning the associated selected columns, not permanent raw merging.
        let mut separate = Vec::new();
        let mut right_columns = std::collections::HashMap::new();
        if ruby
            .levels
            .iter()
            .any(|style| inter_character(data, *style))
        {
            for columns in selected_lanes(ruby, &selected_columns)
                .map(|index| intersect(&ruby.lanes[index].columns, &selected_columns))
                .chain(std::iter::once(selected_columns.clone()))
            {
                if !columns.is_empty() {
                    right_columns
                        .entry((columns.start, columns.end))
                        .or_insert_with(|| {
                            super::overhang::rightmost_column(data, &bases, &columns, cx)
                        });
                }
            }
        }
        let mut merged = vec![None; ruby.levels.len()];
        let mut cross_columns = vec![LayoutUnit::ZERO; bases.len()];
        for lane in &lanes {
            if let Some(width) = lane.cross_width {
                let columns = intersect(&ruby.lanes[lane.lane].columns, &selected_columns);
                if !columns.is_empty() {
                    let column = rightmost(&right_columns, &columns);
                    cross_columns[column] = cross_columns[column].add(width, sat);
                }
            }
        }
        let geometry = super::overhang::columns(data, ruby, &selected, &bases, atomics, cx, sat);
        let mut area = geometry.area;
        for (_, &index) in completed.range((units.start, 0)..(units.end, 0)) {
            let nested = &measure.fragments[index];
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 1;
            }
            if nested.units.end <= units.end {
                area = area.union(nested.whole_area);
            }
        }
        let heights: Vec<_> = lanes.iter().map(|l| l.block_size).collect();
        let has_content =
            !crate::line::metric_index::measure(data, units.clone(), atomics, cx, sat).empty
                || heights.iter().any(|h| *h != LayoutUnit::ZERO)
                || completed
                    .range((units.start, 0)..(units.end, 0))
                    .any(|(_, &i)| {
                        let child = &measure.fragments[i];
                        child.units.end <= units.end && child.has_content
                    });
        let tracks = super::geometry::tracks(
            data,
            ruby,
            &bases,
            &lanes,
            area,
            &geometry.contents,
            &right_columns,
            &heights,
            has_content,
            sat,
        );
        let sides = super::geometry::level_sides(data, ruby);
        let mut level_overhang = vec![(LayoutUnit::ZERO, LayoutUnit::ZERO); ruby.levels.len()];
        for l in &mut lanes {
            let source = &ruby.lanes[l.lane];
            if l.cross_width.is_some() {
                continue;
            } else if merging(data, ruby.levels[source.level]) {
                let width = merged[source.level].get_or_insert(LayoutUnit::ZERO);
                *width = width.add(l.width, sat);
            } else {
                let columns = intersect(&source.columns, &selected_columns);
                if !columns.is_empty() {
                    l.overhang = lane_overhang(
                        data,
                        ruby,
                        &selected,
                        &bases,
                        &base_widths,
                        &cross_columns,
                        &right_columns,
                        &columns,
                        l.width,
                        ruby.levels[source.level],
                        sides[source.level],
                        tracks.base,
                        &source.paragraph.data,
                        atomics,
                        cx,
                        sat,
                    );
                    separate.push(SpanWidth {
                        width: l
                            .width
                            .sub(l.overhang.0, sat)
                            .sub(l.overhang.1, sat)
                            .sub(
                                internal_cross(&right_columns, &cross_columns, &columns, sat),
                                sat,
                            )
                            .max(LayoutUnit::ZERO),
                        columns,
                    });
                }
            }
        }
        for (level, width) in merged
            .into_iter()
            .enumerate()
            .filter_map(|(level, width)| width.map(|w| (level, w)))
        {
            let child = ruby
                .lanes
                .iter()
                .find(|l| l.level == level)
                .map(|l| &*l.paragraph.data)
                .unwrap();
            let allowance = lane_overhang(
                data,
                ruby,
                &selected,
                &bases,
                &base_widths,
                &cross_columns,
                &right_columns,
                &selected_columns,
                width,
                ruby.levels[level],
                sides[level],
                tracks.base,
                child,
                atomics,
                cx,
                sat,
            );
            level_overhang[level] = allowance;
            separate.push(SpanWidth {
                columns: selected_columns.clone(),
                width: width
                    .sub(allowance.0, sat)
                    .sub(allowance.1, sat)
                    .sub(
                        internal_cross(&right_columns, &cross_columns, &selected_columns, sat),
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
        completed.insert((units.start, container), measure.fragments.len());
        measure.fragments.push(RubyFragmentMeasure {
            container,
            units,
            bases,
            base_widths,
            base_columns,
            cross_columns,
            right_columns,
            columns,
            lanes,
            level_overhang,
            adjustment,
            whole_area: tracks.whole,
            contribution: tracks.contribution,
            has_content,
        });
    }
    if through > end {
        let full = crate::line::ruby_range_width(data, selected, atomics, cx, sat);
        let consumed = crate::line::ruby_range_width(data, start..end, atomics, cx, sat);
        measure.adjustment = measure.adjustment.add(full.sub(consumed, sat), sat);
    }
    measure
}

#[allow(clippy::too_many_arguments)]
fn lane_overhang(
    data: &ParagraphData,
    ruby: &super::prepare::PreparedRuby,
    selected: &Range<usize>,
    bases: &[Range<usize>],
    base: &[LayoutUnit],
    cross: &[LayoutUnit],
    right_columns: &std::collections::HashMap<(usize, usize), usize>,
    columns: &Range<usize>,
    width: LayoutUnit,
    style: super::RubyStyle,
    before_track: bool,
    area: super::geometry::Bounds,
    child: &ParagraphData,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (LayoutUnit, LayoutUnit) {
    if style.overhang != super::RubyOverhang::Auto {
        return (LayoutUnit::ZERO, LayoutUnit::ZERO);
    }
    let natural = columns
        .clone()
        .fold(LayoutUnit::ZERO, |w, i| w.add(base[i], sat))
        .add(internal_cross(right_columns, cross, columns, sat), sat);
    let excess = width.sub(natural, sat).max(LayoutUnit::ZERO);
    if excess == LayoutUnit::ZERO {
        return (LayoutUnit::ZERO, LayoutUnit::ZERO);
    }
    let cap = LayoutUnit::from_f32_round(child.style_metrics[0].ic / 2.0, sat);
    let (before, after) = super::overhang::allowances(
        data,
        ruby,
        selected,
        bases,
        columns,
        before_track,
        area,
        cap,
        atomics,
        cx,
        sat,
    );
    let total = excess.min(before.add(after, sat));
    let leading = before.min(total.div_i32(2));
    let trailing = after.min(total.sub(leading, sat));
    let leading = leading.add(
        before
            .sub(leading, sat)
            .min(total.sub(leading, sat).sub(trailing, sat)),
        sat,
    );
    (leading, trailing)
}

pub(crate) fn merging(data: &ParagraphData, style: super::RubyStyle) -> bool {
    style.merge == super::RubyMerge::Merge && !inter_character(data, style)
}

pub(crate) fn inter_character(data: &ParagraphData, style: super::RubyStyle) -> bool {
    style.position == super::RubyPosition::InterCharacter
        && data.style.writing_mode == crate::geometry::WritingMode::HorizontalTb
}

pub(crate) fn rightmost(
    right_columns: &std::collections::HashMap<(usize, usize), usize>,
    columns: &Range<usize>,
) -> usize {
    right_columns
        .get(&(columns.start, columns.end))
        .copied()
        .unwrap_or(columns.end - 1)
}

pub(crate) fn internal_cross(
    right_columns: &std::collections::HashMap<(usize, usize), usize>,
    cross: &[LayoutUnit],
    columns: &Range<usize>,
    sat: &mut Saturation,
) -> LayoutUnit {
    if columns.is_empty() {
        return LayoutUnit::ZERO;
    }
    let right = rightmost(right_columns, columns);
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
