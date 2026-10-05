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
    /// Original index of the first column; column arrays and span keys are local.
    pub(crate) column_start: usize,
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

impl RubyFragmentMeasure {
    pub(crate) fn local_columns(&self, columns: &Range<usize>) -> Range<usize> {
        local_columns(columns, self.column_start, self.bases.len())
    }
}

/// Clip an original span to a compact candidate window and rebase its indices.
pub(crate) fn local_columns(columns: &Range<usize>, start: usize, len: usize) -> Range<usize> {
    columns.start.saturating_sub(start).min(len)..columns.end.saturating_sub(start).min(len)
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

pub(super) fn intersect(a: &Range<usize>, b: &Range<usize>) -> Range<usize> {
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

pub(super) fn cut_at_or_after(ruby: &super::prepare::PreparedRuby, unit: usize) -> usize {
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

/// Adjustment-only candidate for fit probes (line scan, partial-line index,
/// intrinsic sizes). Accepted lines call `candidate` through `apply`, which
/// also keeps the fragments. The container core is memoized per operation by
/// `start..through` (`super::memo`); a miss measures it through the
/// container accumulator (`super::accumulate`), which re-measures only the
/// containers whose inputs changed since the previous `through`. The
/// end-dependent look-ahead tail is always computed live, after the core,
/// preserving the side-effect order.
pub(crate) fn candidate_adjustment(
    data: &ParagraphData,
    start: usize,
    end: usize,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    cx.ruby_ranges.begin(data, atomics);
    if start >= end || data.ruby.containers.is_empty() {
        return LayoutUnit::ZERO;
    }
    if !cx.reuse_enabled() {
        // The reference path measures every probe in full.
        return candidate_inner(data, start, end, atomics, cx, sat).adjustment;
    }
    // Fit probes grow `end` for a fixed `start`: resume the previous walk.
    let mut walk = cx.ruby_memo.take_walk();
    let through = super::memo::advance(&mut walk, data, start, end);
    let key = super::memo::MemoKey::new(data, atomics, start, through);
    let generation = cx.ruby_ranges.generation();
    let core = match cx.ruby_memo.get(&key) {
        Some(entry)
            if entry.generation == generation
                && crate::line::replay::replay(cx, &entry.effects, sat) =>
        {
            #[cfg(test)]
            {
                cx.ruby_memo_hits += 1;
            }
            entry.adjustment
        }
        _ if through == end => {
            // No look-ahead: the key can still replay the entry of an earlier
            // look-ahead probe with the same `through` (above), but it is not
            // stored. Fit scans ask for an exact key once per scan, so storing
            // it only grows the memo by one entry per probe (one per unit in
            // `intrinsic_sizes`). Measure as the reference path does.
            let containers = walk.as_ref().map_or(&[][..], |w| w.visited());
            super::accumulate::core(data, start, through, containers, atomics, cx, sat)
        }
        _ => {
            let containers = walk.as_ref().map_or(&[][..], |w| w.visited());
            let recording = crate::line::replay::begin(cx, sat);
            let adjustment =
                super::accumulate::core(data, start, through, containers, atomics, cx, sat);
            let effects = crate::line::replay::finish(cx, recording, sat);
            // A recording that filled a cache whose later hits skip side
            // effects is not what measuring again would do; measure again.
            match effects.filter(|_| cx.ruby_ranges.generation() == generation) {
                Some(effects) => cx.ruby_memo.insert(
                    key,
                    super::memo::MemoEntry {
                        adjustment,
                        effects,
                        generation,
                    },
                ),
                None => cx.ruby_memo.remove(&key),
            }
            adjustment
        }
    };
    cx.ruby_memo.put_walk(walk);
    lookahead(data, start, end, through, core, atomics, cx, sat)
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
    let (through, containers) = walk(data, start, end, cx);
    let mut measure = measure_containers(data, start..through, &containers, atomics, cx, sat);
    measure.adjustment = lookahead(
        data,
        start,
        end,
        through,
        measure.adjustment,
        atomics,
        cx,
        sat,
    );
    measure
}

/// Visit the intersecting containers in structural order, extending `end` to
/// the next legal paired endpoint. The visited list is a function of
/// `start..through`: the containers with `units.end > start` and
/// `units.start < through`, in index order.
pub(super) fn walk(
    data: &ParagraphData,
    start: usize,
    end: usize,
    _cx: &mut LayoutContext,
) -> (usize, Vec<usize>) {
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
                _cx.ruby_measure_visits += 1;
            }
            let clipped = (*through).min(ruby.units.end);
            *through = (*through).max(ruby.cuts[cut_at_or_after(ruby, clipped)].unit);
            containers.push(container);
        },
    );
    (through, containers)
}

/// A probe measured through a later paired endpoint charges the look-ahead
/// units beyond `end` as part of its adjustment.
#[allow(clippy::too_many_arguments)]
pub(super) fn lookahead(
    data: &ParagraphData,
    start: usize,
    end: usize,
    through: usize,
    adjustment: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    if through <= end {
        return adjustment;
    }
    let full = crate::line::ruby_range_width(data, start..through, atomics, cx, sat);
    let consumed = crate::line::ruby_range_width(data, start..end, atomics, cx, sat);
    adjustment.add(full.sub(consumed, sat), sat)
}

/// Completed descendant fragments, as the containers measured so far in one
/// candidate expose them to an enclosing container: every completed
/// fragment that starts in `range` and ends by `range.end`, in structural
/// order (all descendants inside the range, not only children).
pub(super) trait Descendants {
    /// `width` plus each such fragment's adjustment, in structural order.
    fn add_adjustments(
        &self,
        range: &Range<usize>,
        width: LayoutUnit,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit;
    /// `area` united with each such fragment's whole area.
    fn union_areas(
        &self,
        range: &Range<usize>,
        area: super::geometry::Bounds,
        cx: &mut LayoutContext,
    ) -> super::geometry::Bounds;
    /// Whether any such fragment has content.
    fn any_content(&self, range: &Range<usize>) -> bool;
}

/// Completed fragments of `measure_containers`, indexed by source start so
/// siblings are never compared as potential descendants of every later
/// column/container. Container identity preserves ancestors that share a
/// clipped continuation start.
struct Completed<'a> {
    index: &'a std::collections::BTreeMap<(usize, usize), usize>,
    fragments: &'a [RubyFragmentMeasure],
}

impl Descendants for Completed<'_> {
    fn add_adjustments(
        &self,
        range: &Range<usize>,
        mut width: LayoutUnit,
        _cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        for (_, &index) in self.index.range((range.start, 0)..(range.end, 0)) {
            let nested = &self.fragments[index];
            #[cfg(test)]
            {
                _cx.ruby_measure_visits += 1;
                _cx.ruby_descendant_reads += 1;
            }
            if nested.units.end <= range.end {
                width = width.add(nested.adjustment, sat);
            }
        }
        width
    }

    fn union_areas(
        &self,
        range: &Range<usize>,
        mut area: super::geometry::Bounds,
        _cx: &mut LayoutContext,
    ) -> super::geometry::Bounds {
        for (_, &index) in self.index.range((range.start, 0)..(range.end, 0)) {
            let nested = &self.fragments[index];
            #[cfg(test)]
            {
                _cx.ruby_measure_visits += 1;
                _cx.ruby_descendant_reads += 1;
            }
            if nested.units.end <= range.end {
                area = area.union(nested.whole_area);
            }
        }
        area
    }

    fn any_content(&self, range: &Range<usize>) -> bool {
        self.index
            .range((range.start, 0)..(range.end, 0))
            .any(|(_, &i)| {
                let child = &self.fragments[i];
                child.units.end <= range.end && child.has_content
            })
    }
}

/// Measure the visited containers of `selected` (children before parents).
pub(super) fn measure_containers(
    data: &ParagraphData,
    selected: Range<usize>,
    containers: &[usize],
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> RubyMeasure {
    let mut measure = RubyMeasure::default();
    // Every container below resolves its columns against `selected`; measure
    // that line profile once per candidate.
    let mut profile = crate::line::metric_index::ProfileShare::default();
    // Reverse structural traversal resolves children before parents.
    let mut index = std::collections::BTreeMap::<(usize, usize), usize>::new();
    for &container in containers.iter().rev() {
        let completed = Completed {
            index: &index,
            fragments: &measure.fragments,
        };
        let Some(fragment) = measure_one(
            data,
            &selected,
            container,
            &completed,
            &mut profile,
            atomics,
            cx,
            sat,
        ) else {
            continue;
        };
        measure.adjustment = measure.adjustment.add(fragment.adjustment, sat);
        index.insert((fragment.units.start, container), measure.fragments.len());
        measure.fragments.push(fragment);
    }
    measure
}

/// Measure one container of `selected` against the completed descendants
/// and the candidate's shared line profile. `None` if the container does
/// not intersect `selected`.
#[allow(clippy::too_many_arguments)]
pub(super) fn measure_one(
    data: &ParagraphData,
    selected: &Range<usize>,
    container: usize,
    completed: &dyn Descendants,
    profile: &mut crate::line::metric_index::ProfileShare,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<RubyFragmentMeasure> {
    let ruby = &data.ruby.containers[container];
    #[cfg(test)]
    {
        cx.ruby_measure_visits += 1;
    }
    let units = intersect(selected, &ruby.units);
    if units.is_empty() {
        return None;
    }
    #[cfg(test)]
    {
        cx.ruby_container_measures += 1;
    }
    let begin = &ruby.cuts[cut_at_or_before(ruby, units.start)];
    let finish = &ruby.cuts[cut_at_or_after(ruby, units.end)];
    let source_columns = selected_columns(ruby, &units);
    let column_start = source_columns.start;
    let source_bases = &ruby.columns[source_columns.clone()];
    let bases: Vec<_> = source_bases
        .iter()
        .map(|c| intersect(&units, &c.units))
        .collect();
    let mut base_widths = Vec::with_capacity(bases.len());
    for (column, base) in source_bases.iter().zip(&bases) {
        #[cfg(test)]
        {
            cx.ruby_column_visits += 1;
        }
        let width = crate::line::ruby_base_width(
            data,
            column.units.clone(),
            base.clone(),
            atomics,
            cx,
            sat,
        );
        base_widths.push(completed.add_adjustments(base, width, cx, sat));
    }
    let selected_columns = 0..bases.len();
    let mut lanes = Vec::new();
    for index in selected_lanes(ruby, &source_columns) {
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
        for columns in selected_lanes(ruby, &source_columns)
            .map(|index| local_columns(&ruby.lanes[index].columns, column_start, bases.len()))
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
            let columns = local_columns(&ruby.lanes[lane.lane].columns, column_start, bases.len());
            if !columns.is_empty() {
                let column = rightmost(&right_columns, &columns);
                cross_columns[column] = cross_columns[column].add(width, sat);
            }
        }
    }
    let geometry = super::overhang::columns(
        data,
        ruby,
        column_start,
        selected,
        &bases,
        atomics,
        profile,
        cx,
        sat,
    );
    let area = completed.union_areas(&units, geometry.area, cx);
    let heights: Vec<_> = lanes.iter().map(|l| l.block_size).collect();
    let has_content = !crate::line::metric_index::measure(data, units.clone(), atomics, cx, sat)
        .empty
        || heights.iter().any(|h| *h != LayoutUnit::ZERO)
        || completed.any_content(&units);
    let tracks = super::geometry::tracks(
        data,
        ruby,
        column_start,
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
            let columns = local_columns(&source.columns, column_start, bases.len());
            if !columns.is_empty() {
                l.overhang = lane_overhang(
                    data,
                    ruby,
                    selected,
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
                    profile,
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
        // The cap belongs to the original first lane in this level,
        // even when that lane is outside the candidate window.
        let first_lane = ruby.lanes.partition_point(|lane| lane.level < level);
        let child = &ruby.lanes[first_lane].paragraph.data;
        let allowance = lane_overhang(
            data,
            ruby,
            selected,
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
            profile,
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
    Some(RubyFragmentMeasure {
        container,
        units,
        column_start,
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
    })
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
    share: &mut crate::line::metric_index::ProfileShare,
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
        share,
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
                let base_box = ruby.columns[fragment.column_start + i].box_index;
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
