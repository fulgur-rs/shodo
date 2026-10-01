//! Bounded edge windows shared by candidate measurements and materialization.
use super::reshape::EdgeOverlay;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::{WarningKind, WarningSink};
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext};
use std::ops::Range;
use std::sync::Arc;

pub(super) struct Window {
    pub(super) overlay: EdgeOverlay,
    /// Glyph contributions only: box edges and anchors keep their widths.
    pub(super) changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
}
impl Window {
    pub(super) fn delta(&self, through: usize, sat: &mut Saturation) -> LayoutUnit {
        self.changes
            .iter()
            .filter(|(i, _, _)| *i < through)
            .fold(LayoutUnit::ZERO, |p, (_, old, new)| {
                p.add(new.sub(*old, sat), sat)
            })
    }
}

pub(super) fn hyphen(
    data: &ParagraphData,
    start: usize,
    end: usize,
    replacement: &crate::shape::Replacement,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<Vec<Window>> {
    if !remaining_valid(data, start, end, cx, sat) {
        return None;
    }
    let windows = measure_edit(data, start, end, Some(replacement), cx, sat);
    let generated = windows.iter().any(|w| {
        w.overlay.text.start <= replacement.text.start && replacement.text.end <= w.overlay.text.end
    });
    if !generated || !covers_partial_edges(data, start, end, &windows) {
        return None;
    }
    Some(windows)
}

pub(super) fn cost(windows: &[Window], end: usize, sat: &mut Saturation) -> LayoutUnit {
    windows
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat))
}

fn first(data: &ParagraphData, at: usize, end: usize) -> Option<usize> {
    if at >= end {
        return None;
    }
    if matches!(data.units[at].kind, UnitKind::Cluster { .. }) {
        return Some(at);
    }
    let i = data
        .selectable_clusters
        .partition_point(|i| (*i as usize) < at);
    data.selectable_clusters
        .get(i)
        .map(|i| *i as usize)
        .filter(|i| *i < end)
}
fn last(data: &ParagraphData, start: usize, end: usize) -> Option<usize> {
    if start >= end {
        return None;
    }
    if matches!(data.units[end - 1].kind, UnitKind::Cluster { .. }) {
        return Some(end - 1);
    }
    let i = data
        .selectable_clusters
        .partition_point(|i| (*i as usize) < end);
    let at = *data.selectable_clusters.get(i.checked_sub(1)?)? as usize;
    (at >= start).then_some(at)
}
fn group(data: &ParagraphData, at: usize) -> Range<usize> {
    data.units[at]
        .shared_cluster
        .as_ref()
        .map_or(at..at + 1, |c| c.units.clone())
}
fn clipped_group(data: &ParagraphData, at: usize, line: &Range<usize>) -> Range<usize> {
    let group = group(data, at);
    let start = first(data, group.start.max(line.start), at + 1).unwrap();
    let end = last(data, at, group.end.min(line.end)).unwrap() + 1;
    start..end
}
fn compatible(data: &ParagraphData, a: usize, b: usize) -> bool {
    if data.units[a].combine.is_some() || data.units[b].combine.is_some() {
        return false;
    }
    let (UnitKind::Cluster { run: a_run, .. }, UnitKind::Cluster { run: b_run, .. }) =
        (&data.units[a].kind, &data.units[b].kind)
    else {
        return false;
    };
    let (a_run, b_run) = (&data.runs[*a_run as usize], &data.runs[*b_run as usize]);
    data.units[a].level == data.units[b].level
        && a_run.font == b_run.font
        && Arc::ptr_eq(&a_run.instance, &b_run.instance)
}
fn unsafe_join(data: &ParagraphData, a: usize, b: usize) -> bool {
    data.units[a].unsafe_to_break || data.units[b].unsafe_to_concat
}
fn storage_split(data: &ParagraphData, at: usize) -> bool {
    if data.units[at].shared_cluster.is_some() {
        return false;
    }
    [last(data, 0, at), first(data, at + 1, data.units.len())]
        .into_iter()
        .flatten()
        .any(|i| data.units[i].text == data.units[at].text)
}
fn partial(data: &ParagraphData, range: &Range<usize>) -> bool {
    data.units[range.start]
        .shared_cluster
        .as_ref()
        .is_some_and(|c| {
            data.units[range.start].text.start != c.text.start
                || data.units[range.end - 1].text.end != c.text.end
        })
}

/// Edge windows are a pure function of the paragraph, unit range and glyph
/// budget, and a line scan revisits the same few windows at every break
/// candidate. Keep results for the most recent paragraph only, so memory stays
/// bounded regardless of how many paragraphs a context lays out. Only clean,
/// unedited results are retained.
///
/// Retention is bounded by an accounted cost, not just the entry count: a
/// hostile font can expand one window into a very large glyph run, so the
/// total is capped in glyph-equivalents and oversized results are not cached.
#[derive(Default)]
pub(crate) struct EdgeShapeCache {
    owner: Option<(u64, usize)>,
    entries: crate::hashing::FastMap<(usize, usize, Option<u64>), ShapedWindow>,
    cost: usize,
}

impl std::fmt::Debug for EdgeShapeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeShapeCache")
            .field("entries", &self.entries.len())
            .field("cost", &self.cost)
            .finish()
    }
}

type OwnedShapedWindow = (crate::shape::GlyphStore, Vec<crate::shape::ShapedRun>);
type ShapedWindow = std::sync::Arc<OwnedShapedWindow>;

// Keep uncacheable results on the same allocation-free owned path. Boxing the
// larger variant would add an allocation to every edited/oversized window.
#[allow(clippy::large_enum_variant)]
enum WindowHandle {
    Owned(OwnedShapedWindow),
    Shared(ShapedWindow),
}

impl std::ops::Deref for WindowHandle {
    type Target = OwnedShapedWindow;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(window) => window,
            Self::Shared(window) => window,
        }
    }
}

impl WindowHandle {
    fn into_owned(self) -> OwnedShapedWindow {
        match self {
            Self::Owned(window) => window,
            Self::Shared(window) => {
                std::sync::Arc::try_unwrap(window).unwrap_or_else(|shared| (*shared).clone())
            }
        }
    }
}

fn cache_owned_window(
    shaped: OwnedShapedWindow,
    key: (usize, usize, Option<u64>),
    cache: &mut EdgeShapeCache,
) -> WindowHandle {
    // A miss keeps its original output vectors. Clone exactly one len-sized
    // cache snapshot, as before, instead of shrinking every growing vector and
    // then cloning again if this newly shaped window is selected.
    let shared = std::sync::Arc::new(shaped.clone());
    cache.insert(key, &shared);
    WindowHandle::Owned(shaped)
}

const EDGE_SHAPE_CACHE_ENTRIES: usize = 256;
/// Total retained cost in glyph-equivalents (about 26 bytes each): roughly
/// 0.9 MiB at most.
const EDGE_SHAPE_CACHE_COST: usize = 1 << 15;
/// A single window costing more than this is shaped every time instead.
const EDGE_SHAPE_ENTRY_COST_MAX: usize = 1 << 10;

fn window_cost(window: &OwnedShapedWindow) -> usize {
    // A run holds an `Arc` and a few scalars; count it as a few glyphs.
    window.0.len() + window.1.len() * 4
}

impl EdgeShapeCache {
    fn begin(&mut self, data: &ParagraphData) {
        let owner = (data.id, data as *const ParagraphData as usize);
        if self.owner != Some(owner) {
            self.clear();
            self.owner = Some(owner);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn clear(&mut self) {
        self.entries = Default::default();
        self.cost = 0;
        self.owner = None;
    }

    fn get(&self, key: &(usize, usize, Option<u64>)) -> Option<&ShapedWindow> {
        self.entries.get(key)
    }

    /// Returns whether the window was retained.
    fn insert(&mut self, key: (usize, usize, Option<u64>), window: &ShapedWindow) -> bool {
        let cost = window_cost(window);
        if cost > EDGE_SHAPE_ENTRY_COST_MAX {
            return false;
        }
        if self.entries.len() >= EDGE_SHAPE_CACHE_ENTRIES
            || self.cost + cost > EDGE_SHAPE_CACHE_COST
        {
            self.entries.clear();
            self.cost = 0;
        }
        if let Some(previous) = self.entries.insert(key, window.clone()) {
            self.cost -= window_cost(&previous);
        }
        self.cost += cost;
        true
    }
}

/// Each edge window is bounded by `max_reshape_window_bytes`, but a line scan
/// asks for one per break candidate, so a long unsafe-joined run (for example
/// cursive text under `word-break: break-all`) multiplies that bound by the
/// candidate count. Cap the bytes requested per `next_line` or `intrinsic_sizes`
/// call at this many windows; beyond it shared glyphs are kept and a warning is
/// emitted. First-line intrinsic passes share this cap.
const EDGE_RESHAPE_LINE_WINDOWS: u64 = 64;

/// Charge a request against the current operation's reshape budget, whether or
/// not the cache can answer it, so the outcome never depends on what an earlier
/// layout left in the context.
fn within_line_reshape_budget(
    data: &ParagraphData,
    range: &Range<usize>,
    cx: &mut LayoutContext,
) -> bool {
    let Some(window) = data.limits.max_reshape_window_bytes else {
        return true;
    };
    let limit = window.saturating_mul(EDGE_RESHAPE_LINE_WINDOWS);
    let bytes = u64::from(data.units[range.end - 1].text.end - data.units[range.start].text.start);
    let before = cx.edge_reshape_spent;
    cx.edge_reshape_spent = before.saturating_add(bytes);
    if cx.edge_reshape_spent <= limit {
        return true;
    }
    if before <= limit {
        cx.warnings.push(
            WarningKind::Unsupported,
            "line edge reshape budget exceeded; keeping shared glyphs",
        );
    }
    false
}

fn shape(
    data: &ParagraphData,
    range: &Range<usize>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
    budget: Option<u64>,
    replacement: Option<&crate::shape::Replacement>,
) -> Option<WindowHandle> {
    if !within_line_reshape_budget(data, range, cx) {
        return None;
    }
    let key = (range.start, range.end, budget);
    if replacement.is_none() {
        cx.edge_shapes.begin(data);
        if let Some(hit) = cx.edge_shapes.get(&key) {
            return Some(WindowHandle::Shared(std::sync::Arc::clone(hit)));
        }
    }
    let mut unit = data.units[range.start].clone();
    unit.text = unit.text.start..data.units[range.end - 1].text.end;
    #[cfg(test)]
    {
        data.edge_shape_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        data.edge_shape_bytes.fetch_add(
            (unit.text.end - unit.text.start) as usize,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let mut warnings = WarningSink::new(data.limits.max_warnings);
    let saturation_before = *sat;
    let result =
        crate::shape::shape_window_edit(data, &unit, budget, replacement, cx, &mut warnings, sat);
    let warned = warnings.take();
    let clean = warned.is_empty() && *sat == saturation_before;
    let result = result.map(|shaped| {
        if replacement.is_none() && clean && window_cost(&shaped) <= EDGE_SHAPE_ENTRY_COST_MAX {
            cache_owned_window(shaped, key, &mut cx.edge_shapes)
        } else {
            WindowHandle::Owned(shaped)
        }
    });
    for w in warned {
        cx.warnings.push(w.kind, w.message);
    }
    result
}

fn within_window_budget(data: &ParagraphData, range: &Range<usize>) -> bool {
    data.limits.max_reshape_window_bytes.is_none_or(|max| {
        u64::from(data.units[range.end - 1].text.end - data.units[range.start].text.start) <= max
    })
}

fn expand_original(data: &ParagraphData, line: &Range<usize>, range: &mut Range<usize>) -> bool {
    if !within_window_budget(data, range) {
        return false;
    }
    while let Some(previous) = last(data, line.start, range.start)
        && compatible(data, previous, range.start)
        && unsafe_join(data, previous, range.start)
    {
        #[cfg(test)]
        data.window_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        range.start = clipped_group(data, previous, line).start;
        if !within_window_budget(data, range) {
            return false;
        }
    }
    while let Some(next) = first(data, range.end, line.end)
        && compatible(data, range.end - 1, next)
        && unsafe_join(data, range.end - 1, next)
    {
        #[cfg(test)]
        data.window_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        range.end = clipped_group(data, next, line).end;
        if !within_window_budget(data, range) {
            return false;
        }
    }
    true
}

fn window_budget_warning(cx: &mut LayoutContext) {
    cx.warnings.push(
        crate::limits::WarningKind::Unsupported,
        "unsafe edge exceeds reshape window budget; retaining shared glyphs",
    );
}

/// Re-shape to original safe boundaries, then check new concatenation flags.
/// A right-hand probe detects a changed join without submitting raw markers.
fn materialize(
    data: &ParagraphData,
    line: &Range<usize>,
    mut range: Range<usize>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
    budget: Option<u64>,
    replacement: Option<&crate::shape::Replacement>,
) -> Option<Window> {
    if !expand_original(data, line, &mut range) {
        window_budget_warning(cx);
        return None;
    }
    loop {
        if storage_split(data, range.start) || storage_split(data, range.end - 1) {
            cx.warnings.push(
                WarningKind::Unsupported,
                "resource-split shaping cluster retained whole at line edge",
            );
            return None;
        }
        let mut shaped = shape(data, &range, cx, sat, budget, replacement)?;
        if shaped.0.flags.first().is_some_and(|f| f & 2 != 0)
            && let Some(previous) = last(data, line.start, range.start)
            && compatible(data, previous, range.start)
        {
            range.start = clipped_group(data, previous, line).start;
            if !expand_original(data, line, &mut range) {
                window_budget_warning(cx);
                return None;
            }
            continue;
        }
        if let Some(next) = first(data, range.end, line.end)
            && compatible(data, range.end - 1, next)
        {
            let probe = range.start..clipped_group(data, next, line).end;
            // Keep only one temporary SoA at a time during validation.
            drop(shaped);
            let tested = shape(data, &probe, cx, sat, budget, replacement)?;
            let boundary = data.units[next].text.start;
            let unsafe_probe = tested
                .0
                .cluster
                .iter()
                .position(|c| *c >= boundary)
                .is_none_or(|i| tested.0.flags[i] & 3 != 0);
            drop(tested);
            if unsafe_probe {
                range.end = probe.end;
                if !expand_original(data, line, &mut range) {
                    window_budget_warning(cx);
                    return None;
                }
                continue;
            }
            shaped = shape(data, &range, cx, sat, budget, replacement)?;
        }
        let mut changes = Vec::new();
        let mut at = range.start;
        // `selectable_clusters` is sorted and the groups below are visited in
        // order, so one forward cursor replaces two binary searches per group.
        let selectable = &data.selectable_clusters;
        let mut cursor = selectable.partition_point(|u| (*u as usize) < range.start);
        while let Some(i) = first(data, at, range.end) {
            let end = group(data, i).end.min(range.end);
            while selectable.get(cursor).is_some_and(|u| (*u as usize) < i) {
                cursor += 1;
            }
            let begin = cursor;
            while selectable.get(cursor).is_some_and(|u| (*u as usize) < end) {
                cursor += 1;
            }
            for index in selectable[begin..cursor].iter().map(|i| *i as usize) {
                let old = super::scan::unit_width_from(
                    data,
                    &data.units[index],
                    data.units[line.start].text.start,
                    LayoutUnit::ZERO,
                    &AtomicSizes::EMPTY,
                    cx,
                    sat,
                );
                changes.push((index, old, LayoutUnit::ZERO));
            }
            at = end;
        }
        let mut unit = 0;
        for (cluster, advance) in shaped.0.cluster.iter().zip(&shaped.0.advance) {
            while unit + 1 < changes.len() && data.units[changes[unit + 1].0].text.start <= *cluster
            {
                unit += 1;
            }
            changes[unit].2 = changes[unit].2.add(*advance, sat);
        }
        let UnitKind::Cluster { glyphs: begin, .. } = &data.units[range.start].kind else {
            unreachable!()
        };
        let UnitKind::Cluster { glyphs: end, .. } = &data.units[range.end - 1].kind else {
            unreachable!()
        };
        let (store, runs) = shaped.into_owned();
        return Some(Window {
            overlay: EdgeOverlay {
                glyphs: begin.start..end.end,
                text: data.units[range.start].text.start..data.units[range.end - 1].text.end,
                store,
                runs,
                hyphen: replacement
                    .filter(|r| {
                        data.units[range.start].text.start <= r.text.start
                            && r.text.end <= data.units[range.end - 1].text.end
                    })
                    .map(|r| r.text.clone()),
            },
            changes,
        });
    }
}

pub(super) fn measure(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<Window> {
    measure_edit(data, start, end, None, cx, sat)
}

fn measure_edit(
    data: &ParagraphData,
    start: usize,
    end: usize,
    replacement: Option<&crate::shape::Replacement>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<Window> {
    let line = start..end;
    let (Some(first_at), Some(last_at)) = (first(data, start, end), last(data, start, end)) else {
        return Vec::new();
    };
    let first_range = clipped_group(data, first_at, &line);
    let mut last_range = clipped_group(data, last_at, &line);
    if replacement.is_some()
        && let Some(previous) = last(data, start, last_range.start)
        && data.units[previous].level == data.units[last_at].level
        && data
            .shaping_barriers
            .get(
                data.shaping_barriers
                    .partition_point(|i| (*i as usize) <= previous),
            )
            .is_none_or(|i| (*i as usize) >= last_range.start)
    {
        last_range.start = clipped_group(data, previous, &line).start;
    }
    let before = last(data, 0, start);
    let needs_first = data.units[first_at].combine.is_none()
        && (partial(data, &first_range)
            || start > 0
                && (data.units[first_at].unsafe_to_concat
                    || before.is_some_and(|i| data.units[i].unsafe_to_break)));
    let needs_last = data.units[last_at].combine.is_none()
        && (replacement.is_some()
            || partial(data, &last_range)
            || data.units[last_at].unsafe_to_break);
    let mut ranges = Vec::new();
    if needs_first {
        ranges.push(first_range);
    }
    if needs_last {
        ranges.push(last_range);
    }
    ranges.retain_mut(|range| {
        let fits = expand_original(data, &line, range);
        if !fits {
            window_budget_warning(cx);
        }
        fits
    });
    if ranges.len() == 2 && ranges[0].end > ranges[1].start {
        ranges[0].end = ranges[0].end.max(ranges[1].end);
        ranges.pop();
    }
    let mut windows: Vec<Window> = Vec::new();
    for range in ranges {
        let retained = windows
            .iter()
            .map(|w| w.overlay.store.len() as u64)
            .sum::<u64>();
        let budget = data
            .limits
            .max_shaped_glyphs
            .map(|max| max.saturating_sub(retained));
        if let Some(window) = materialize(data, &line, range, cx, sat, budget, replacement) {
            if let Some(previous) = windows.last()
                && previous.overlay.glyphs.end > window.overlay.glyphs.start
            {
                // Revalidation may grow two formerly disjoint windows together.
                let merged = first_at..last_at + 1;
                windows.clear();
                if let Some(window) = materialize(
                    data,
                    &line,
                    merged,
                    cx,
                    sat,
                    data.limits.max_shaped_glyphs,
                    replacement,
                ) {
                    windows.push(window);
                }
                break;
            }
            windows.push(window);
        }
    }
    windows
}

pub(super) fn delta(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    measure(data, start, end, cx, sat)
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat))
}

fn covers_partial_edges(
    data: &ParagraphData,
    start: usize,
    end: usize,
    windows: &[Window],
) -> bool {
    [first(data, start, end), last(data, start, end)]
        .into_iter()
        .flatten()
        .all(|at| {
            let range = clipped_group(data, at, &(start..end));
            !partial(data, &range)
                || windows.iter().any(|window| {
                    window.overlay.text.start <= data.units[range.start].text.start
                        && window.overlay.text.end >= data.units[range.end - 1].text.end
                })
        })
}

/// A shared ligature may only be sliced when the selected source and its
/// remaining suffix both have owned glyphs. Ordinary whole-cluster edges
/// can still use the documented warned shared fallback.
pub(super) fn candidate(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (LayoutUnit, bool) {
    let windows = measure(data, start, end, cx, sat);
    let delta = cost(&windows, end, sat);
    let covered = covers_partial_edges(data, start, end, &windows);
    drop(windows);
    let valid = covered && remaining_valid(data, start, end, cx, sat);
    (delta, valid)
}

fn remaining_valid(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> bool {
    let remainder = last(data, start, end).is_some_and(|at| group(data, at).end > end);
    !remainder || {
        let windows = measure(data, end, data.units.len(), cx, sat);
        covers_partial_edges(data, end, data.units.len(), &windows)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod edge_shape_cache_tests;
