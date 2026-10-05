//! Actual retained unit advances and visual spacing indexed by source range.
//! Tabs extend a prefix cache for their actual selected start; caller atomics
//! are indexed under their revision. Source-clipped shared clusters and
//! discretionary hyphens use bounded selected windows and a replaced spacing
//! leaf. A source-clipped shaping cluster corrects its own slices, keeping the
//! remainder of the range indexed.
//!
//! Every query charges the reshape budget and `Saturation` exactly as a
//! cold query of the same paragraph and range would, whatever earlier
//! queries left in the caches (as `windows` charges edge window cache hits):
//! a `blocks` hit replays the recorded effects of its measurement, and the
//! range costs and tab prefix keep their saturation per unit and per tab
//! step, charged for the units and tabs of each query's own range.
use super::spacing_summary::{RangeIndex, raw};
use crate::LayoutContext;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::{AtomicSizes, ParagraphData};
use std::ops::Range;

type BlockKey = (u64, usize, usize, usize);

#[derive(Debug, Default)]
pub(crate) struct RangeCache {
    root: Option<(u64, usize, u64)>,
    sets: crate::hashing::FastMap<(u64, usize), Costs>,
    /// Value slots: a query moves the index out and back without removing the
    /// key, so the map is not rehashed per query. An empty slot is rebuilt.
    pub(super) metrics:
        crate::hashing::FastMap<(u64, usize), Option<Box<super::metric_index::MetricIndex>>>,
    /// Block sizes whose measurement had no side effects (`Effects::is_empty`).
    blocks: crate::hashing::FastMap<BlockKey, LayoutUnit>,
    /// Block sizes with the effects of their measurement, replayed on a hit
    /// through the exact `line::replay` gate. Kept apart so the common entry
    /// without effects stays as small as before.
    block_effects: crate::hashing::FastMap<BlockKey, (LayoutUnit, crate::line::replay::Effects)>,
    pub(crate) neighbors:
        crate::hashing::FastMap<(u64, usize), Option<Box<crate::ruby::overhang::NeighborIndex>>>,
    /// Invalidations: caches cleared (`begin` with a new root,
    /// `vacate_slots`). Fills need no counter: a query's side effects do not
    /// depend on what the caches hold, so a measurement recorded before a
    /// fill replays exactly after it. Reuse across measurements
    /// (`ruby::accumulate`) still resets on an invalidation, conservatively.
    epoch: u64,
    /// The caches of the same dataset under the previous atomic revision.
    /// `intrinsic_sizes` measures min and max content with separate atomics
    /// and switches at every forced break and cleared float: dropping the
    /// caches at each switch rebuilt the paragraph's indexes every time,
    /// quadratic in the paragraph (shodo-mc0). At most one is kept, and only
    /// once the revisions alternate (holding both costs memory and fresh
    /// allocations, measurably slower for a single switch).
    alt: Option<Box<Stash>>,
    /// The root whose caches the last revision change of the same dataset
    /// dropped. Returning to it starts keeping `alt`.
    dropped: Option<(u64, usize, u64)>,
    /// Override of `MAX_BLOCKS`.
    #[cfg(test)]
    pub(crate) block_cap: Option<usize>,
}

/// The caches of one root, stashed by `RangeCache::begin`.
#[derive(Debug, Default)]
struct Stash {
    root: Option<(u64, usize, u64)>,
    sets: crate::hashing::FastMap<(u64, usize), Costs>,
    metrics: crate::hashing::FastMap<(u64, usize), Option<Box<super::metric_index::MetricIndex>>>,
    blocks: crate::hashing::FastMap<BlockKey, LayoutUnit>,
    block_effects: crate::hashing::FastMap<BlockKey, (LayoutUnit, crate::line::replay::Effects)>,
    neighbors:
        crate::hashing::FastMap<(u64, usize), Option<Box<crate::ruby::overhang::NeighborIndex>>>,
}

/// Entries kept in `blocks` and `block_effects` together. Reaching it clears
/// both before the next insert. A hit has exactly the side effects of a
/// fresh measurement (shodo-tj5), so clearing changes no result, charge or
/// warning, only how often a block is measured. At most about 16,384 × 60
/// bytes (key, value and `Effects`), roughly 1 MiB.
const MAX_BLOCKS: usize = 16_384;

/// A clear releases maps whose capacity grew beyond this.
const RETAINED_BLOCKS: usize = 256;

impl RangeCache {
    fn block_cap(&self) -> usize {
        #[cfg(test)]
        if let Some(cap) = self.block_cap {
            return cap;
        }
        MAX_BLOCKS
    }

    /// Entries in the block caches.
    #[cfg(test)]
    pub(crate) fn block_entries(&self) -> usize {
        self.blocks.len() + self.block_effects.len()
    }

    /// Make room for one more block entry under `key`.
    fn reserve_block(&mut self, key: &BlockKey) {
        if self.blocks.contains_key(key) || self.block_effects.contains_key(key) {
            return;
        }
        if self.blocks.len() + self.block_effects.len() < self.block_cap() {
            return;
        }
        self.blocks.clear();
        self.block_effects.clear();
        if self.blocks.capacity() > RETAINED_BLOCKS {
            self.blocks.shrink_to(RETAINED_BLOCKS);
        }
        if self.block_effects.capacity() > RETAINED_BLOCKS {
            self.block_effects.shrink_to(RETAINED_BLOCKS);
        }
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Exchange the current caches with `stash`.
    fn swap(&mut self, stash: &mut Stash) {
        std::mem::swap(&mut self.root, &mut stash.root);
        std::mem::swap(&mut self.sets, &mut stash.sets);
        std::mem::swap(&mut self.metrics, &mut stash.metrics);
        std::mem::swap(&mut self.blocks, &mut stash.blocks);
        std::mem::swap(&mut self.block_effects, &mut stash.block_effects);
        std::mem::swap(&mut self.neighbors, &mut stash.neighbors);
    }

    pub(crate) fn begin(&mut self, data: &ParagraphData, atomics: &AtomicSizes) {
        let root = (
            data.id,
            data as *const ParagraphData as usize,
            atomics.revision,
        );
        // Retained nested annotation layout revisits datasets already indexed
        // as children of this root. Keep their scalar tables together, without
        // treating child materialization as a new independent paragraph. A new
        // dataset replaces the whole cache; a changed atomic revision of the
        // same dataset stashes it, and the stashed revision comes back whole.
        let key = (root.0, root.1);
        if self.root.is_some_and(|owner| owner.2 == root.2)
            && (self.sets.contains_key(&key)
                || self.metrics.contains_key(&key)
                || self.neighbors.contains_key(&key))
        {
            return;
        }
        if self.root == Some(root) {
            return;
        }
        let same_dataset = self
            .root
            .is_some_and(|owner| (owner.0, owner.1) == (root.0, root.1));
        if same_dataset {
            if let Some(mut alt) = self.alt.take() {
                if alt.root == Some(root) {
                    // Both revisions' caches stay whole: no invalidation.
                    self.swap(&mut alt);
                    self.alt = Some(alt);
                    return;
                }
                // A third revision: the stashed one is dropped below.
            } else if self.dropped == Some(root) {
                // Back to the revision dropped last: the revisions alternate,
                // so keep the current caches for the next switch. A single
                // switch (the usual intrinsic call) keeps nothing extra.
                let mut stash = Box::<Stash>::default();
                self.swap(&mut stash);
                self.alt = Some(stash);
                self.root = Some(root);
                self.invalidate();
                return;
            }
        }
        self.dropped = if same_dataset { self.root } else { None };
        self.sets.clear();
        self.blocks.clear();
        self.block_effects.clear();
        self.metrics.clear();
        self.neighbors.clear();
        self.alt = None;
        self.root = Some(root);
        self.invalidate();
    }

    /// Empty every index slot while keeping its key. The scalar caches are
    /// cleared too, so a repeated query must go through the vacated slots
    /// instead of being answered from a cached result.
    #[cfg(test)]
    pub(crate) fn vacate_slots(&mut self) {
        self.alt = None;
        self.dropped = None;
        self.sets.clear();
        self.blocks.clear();
        self.block_effects.clear();
        self.metrics.values_mut().for_each(|slot| *slot = None);
        self.neighbors.values_mut().for_each(|slot| *slot = None);
        self.invalidate();
    }

    /// Both index maps are populated and every slot holds an index.
    #[cfg(test)]
    pub(crate) fn slots_filled(&self) -> bool {
        !self.metrics.is_empty()
            && !self.neighbors.is_empty()
            && self.metrics.values().all(Option::is_some)
            && self.neighbors.values().all(Option::is_some)
    }
}

/// Cache only scalar block measurements, never child Lines or lane cursors.
/// Actual edge-window run instances supply fallback font metrics, matching
/// retained output without constructing glyph buffers for each fit probe.
///
/// A hit has the side effects of the measurement it stands for: an entry
/// with effects replays them only when `line::replay` proves that measuring
/// again would do exactly that (same budget outcomes, no warning), and is
/// measured again otherwise. A measurement that warned is not stored.
pub(crate) fn block_size(
    data: &ParagraphData,
    range: Range<usize>,
    ruby: &crate::ruby::measure::RubyMeasure,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    let key = (
        data.id,
        data as *const ParagraphData as usize,
        range.start,
        range.end,
    );
    if let Some(height) = cx.ruby_ranges.blocks.get(&key) {
        return *height;
    }
    if let Some((height, effects)) = cx.ruby_ranges.block_effects.get(&key).copied()
        && super::replay::replay(cx, &effects, sat)
    {
        return height;
    }
    let recording = super::replay::begin(cx, sat);
    let height = measure_block(data, range, ruby, atomics, cx, sat);
    let effects = super::replay::finish(cx, recording, sat);
    let cache = &mut cx.ruby_ranges;
    if effects.is_some() {
        cache.reserve_block(&key);
    }
    match effects {
        Some(effects) if effects.is_empty() => {
            cache.block_effects.remove(&key);
            cache.blocks.insert(key, height);
        }
        Some(effects) => {
            // No `blocks` entry exists: a hit there returned above.
            debug_assert!(!cache.blocks.contains_key(&key));
            cache.block_effects.insert(key, (height, effects));
        }
        None => {
            cache.block_effects.remove(&key);
        }
    }
    height
}

fn measure_block(
    data: &ParagraphData,
    range: Range<usize>,
    ruby: &crate::ruby::measure::RubyMeasure,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    let metrics = super::metric_index::measure(data, range, atomics, cx, sat);
    let mut bounds = crate::ruby::geometry::Bounds {
        top: LayoutUnit::ZERO.sub(metrics.baseline, sat),
        bottom: metrics.block_size.sub(metrics.baseline, sat),
    };
    for fragment in &ruby.fragments {
        if fragment.has_content {
            bounds = bounds.union(fragment.contribution);
        }
    }
    bounds.height(sat)
}

#[derive(Debug)]
struct Costs {
    advances: Vec<i64>,
    spacing: RangeIndex,
    hanging_advances: Vec<i64>,
    hanging_units: Vec<usize>,
    last_content: Vec<usize>,
    last_preserved: Vec<usize>,
    last_blocked_preserved: Vec<usize>,
    bad_atomics: Vec<usize>,
    /// Units whose width saturated while the costs were built, with the
    /// running (wrapping) saturation through each: a query charges those of
    /// its own range. Empty for ordinary paragraphs.
    saturated: Vec<(usize, Saturation)>,
    tabs: Vec<usize>,
    tab_prefix: Option<TabPrefix>,
}

impl Costs {
    /// Saturation of building the units of `range`.
    fn saturation(&self, range: &Range<usize>) -> Saturation {
        let through = |end: usize| match self.saturated.partition_point(|(i, _)| *i < end) {
            0 => Saturation::default(),
            k => self.saturated[k - 1].1,
        };
        difference(through(range.end), through(range.start))
    }
}

/// Extra advances of the tabs after one range start, grown as later ends
/// are asked for. Only tab units are stored, so a new start costs work in
/// the tabs it covers, not in every unit.
#[derive(Debug)]
struct TabPrefix {
    start: usize,
    /// Every tab below `through` is covered.
    through: usize,
    /// Index into `Costs::tabs` of the first tab at or after `start`.
    first: usize,
    /// `extra[k]`: the extra advance of the first `k + 1` covered tabs.
    extra: Vec<i64>,
    /// `sat[k]`: the (wrapping) saturation of computing the first `k + 1`
    /// covered tab steps. Empty while every step was clean.
    sat: Vec<Saturation>,
}

impl TabPrefix {
    /// Number of covered tabs before unit `end` (`end <= through`).
    fn count(&self, tabs: &[usize], end: usize) -> usize {
        tabs[self.first..self.first + self.extra.len()].partition_point(|t| *t < end)
    }

    /// Extra advance of the covered tabs before unit `end` (`end <= through`).
    fn before(&self, tabs: &[usize], end: usize) -> i64 {
        match self.count(tabs, end) {
            0 => 0,
            k => self.extra[k - 1],
        }
    }

    /// Saturation of the covered tab steps before unit `end`.
    fn saturation(&self, tabs: &[usize], end: usize) -> Saturation {
        match self.count(tabs, end) {
            0 => Saturation::default(),
            _ if self.sat.is_empty() => Saturation::default(),
            k => self.sat[k - 1],
        }
    }
}

/// Add saturation to the caller's counters. Counters only ever increment;
/// wrapping matches release `+=` and `line::replay`.
fn absorb(sat: &mut Saturation, step: Saturation) {
    sat.saturated = sat.saturated.wrapping_add(step.saturated);
    sat.non_finite = sat.non_finite.wrapping_add(step.non_finite);
}

/// `a - b` of running (wrapping) saturation counts.
fn difference(a: Saturation, b: Saturation) -> Saturation {
    Saturation {
        saturated: a.saturated.wrapping_sub(b.saturated),
        non_finite: a.non_finite.wrapping_sub(b.non_finite),
    }
}

/// Build the range costs of a paragraph. Charges nothing: the saturation
/// of measuring each unit is kept in `Costs::saturated` for the queries.
fn build(data: &ParagraphData, atomics: &AtomicSizes, cx: &mut LayoutContext) -> Costs {
    #[cfg(test)]
    {
        cx.ruby_range_build_units += data.units.len();
    }
    let mut advances = vec![0_i64];
    let mut hanging_advances = vec![0_i64];
    let mut hanging_units = Vec::new();
    let mut last_content = vec![0];
    let mut last_preserved = vec![0];
    let mut last_blocked_preserved = vec![0];
    let mut bad_atomics = Vec::new();
    let mut saturated = Vec::new();
    let mut running = Saturation::default();
    let mut tabs = Vec::new();
    let mut local_warnings = crate::limits::WarningSink::new(Some(0));
    for (i, unit) in data.units.iter().enumerate() {
        #[cfg(test)]
        {
            cx.ruby_measure_visits += 1;
        }
        let mut sat = Saturation::default();
        let width = if let UnitKind::Atomic { node } = unit.kind {
            let mut local_sat = Saturation::default();
            let value = atomics
                .get(node)
                .map(|value| crate::sanitize::atomic(*value, &mut local_warnings, &mut local_sat));
            let width = value.map_or(LayoutUnit::ZERO, |value| {
                LayoutUnit::from_f32_round(
                    value.inline_size + value.margins.inline_sum(),
                    &mut local_sat,
                )
            });
            if value.is_none() || !local_warnings.take().is_empty() || !local_sat.is_clean() {
                bad_atomics.push(i);
            }
            width
        } else if matches!(unit.kind, UnitKind::Tab) && unit.combine.is_none() {
            tabs.push(i);
            LayoutUnit::ZERO
        } else {
            super::scan::unit_width(data, unit, LayoutUnit::ZERO, atomics, cx, &mut sat)
        };
        let width = width.add(data.unit_spacing[i].word, &mut sat);
        if !sat.is_clean() {
            absorb(&mut running, sat);
            saturated.push((i, running));
        }
        advances.push(advances.last().unwrap() + i64::from(width.raw()));
        let hanging = super::whitespace::hangable(data, i);
        hanging_advances.push(
            hanging_advances.last().unwrap() + if hanging { i64::from(width.raw()) } else { 0 },
        );
        if hanging {
            hanging_units.push(i);
        }
        last_content.push(if !hanging && !super::whitespace::transparent(data, i) {
            i + 1
        } else {
            *last_content.last().unwrap()
        });
        last_preserved.push(if hanging && super::whitespace::preserved(data, i) {
            i + 1
        } else {
            *last_preserved.last().unwrap()
        });
        let blocks = if let UnitKind::Close { box_index } = unit.kind {
            let e = data.boxes[box_index as usize].edges;
            e.padding.inline_end != 0.0 || e.border.inline_end != 0.0
        } else {
            false
        };
        last_blocked_preserved.push(if blocks {
            *last_preserved.last().unwrap()
        } else {
            *last_blocked_preserved.last().unwrap()
        });
    }
    let spacing = RangeIndex::new(
        data.units.iter().enumerate().map(|(i, u)| {
            (
                if matches!(u.kind, UnitKind::Tab) && u.combine.is_none() {
                    data.base_level
                } else {
                    u.level
                },
                data.unit_spacing[i].summary,
            )
        }),
        Some(data),
    );
    Costs {
        advances,
        spacing,
        hanging_advances,
        hanging_units,
        last_content,
        last_preserved,
        last_blocked_preserved,
        bad_atomics,
        saturated,
        tabs,
        tab_prefix: None,
    }
}

pub(super) fn width(
    data: &ParagraphData,
    range: Range<usize>,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<LayoutUnit> {
    #[cfg(test)]
    {
        cx.ruby_width_calls += 1;
    }
    let first = data
        .selectable_clusters
        .partition_point(|u| (*u as usize) < range.start);
    let shared = data
        .selectable_clusters
        .get(first)
        .map(|i| &data.units[*i as usize])
        .and_then(|u| u.shared_cluster.as_ref())
        .filter(|c| {
            data.units[range.start].text.start > c.text.start
                && data.units[range.start].text.start < c.text.end
        });
    let mut corrections = Vec::new();
    if let Some(shared) = shared {
        let begin = shared.slices.partition_point(|i| *i < range.start);
        let finish = shared.slices.partition_point(|i| *i < range.end);
        for &i in &shared.slices[begin..finish] {
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 1;
            }
            let actual = super::scan::unit_width_from(
                data,
                &data.units[i],
                data.units[range.start].text.start,
                LayoutUnit::ZERO,
                atomics,
                cx,
                sat,
            );
            corrections.push((i, actual.sub(data.units[i].slice_advance, sat)));
        }
    }
    let clipped_delta = corrections
        .iter()
        .fold(LayoutUnit::ZERO, |w, (_, delta)| w.add(*delta, sat));
    let hyphen_end = super::plan::hyphen_end(data, range.end).filter(|end| *end > range.start);
    let hyphen_windows =
        hyphen_end.and_then(|end| super::hyphen::line(data, range.start, end, cx, sat));
    let hyphen_leaf = hyphen_windows.as_ref().and(hyphen_end).map(|end| {
        (
            end - 1,
            super::spacing::hyphen_unit_summary(data, end - 1, sat),
        )
    });
    if cx
        .ruby_ranges
        .root
        .is_none_or(|root| root.2 != atomics.revision)
    {
        cx.ruby_ranges.begin(data, atomics);
    }
    let key = (data.id, data as *const ParagraphData as usize);
    if !cx.ruby_ranges.sets.contains_key(&key) {
        let costs = build(data, atomics, cx);
        cx.ruby_ranges.sets.insert(key, costs);
    }
    let costs = cx.ruby_ranges.sets.get_mut(&key)?;
    absorb(sat, costs.saturation(&range));
    // Each tab step's value and saturation depend only on the paragraph,
    // the range start and the tab (`clipped_delta` is the same for every
    // end past the tab: a preserved tab is its own item, so no shared
    // cluster slice lies after it). Steps are stored with their saturation
    // and every query charges the steps of its tabs, computed now or
    // earlier, so replacing the prefix for another start changes no
    // query's effects.
    let mut tab_total = 0_i64;
    if !costs.tabs.is_empty() {
        if costs
            .tab_prefix
            .as_ref()
            .is_none_or(|p| p.start != range.start)
        {
            costs.tab_prefix = Some(TabPrefix {
                start: range.start,
                through: range.start,
                first: costs.tabs.partition_point(|t| *t < range.start),
                extra: Vec::new(),
                sat: Vec::new(),
            });
        }
        let prefix = costs.tab_prefix.as_mut().unwrap();
        if prefix.through < range.end {
            // The start's decoration is the same for every step: measure it
            // once and count its saturation per step, as calling it per
            // step did.
            let mut decoration: Option<(LayoutUnit, Saturation)> = None;
            let from = prefix.first + prefix.extra.len();
            let to = costs.tabs.partition_point(|t| *t < range.end);
            for &i in &costs.tabs[from..to] {
                #[cfg(test)]
                {
                    cx.ruby_measure_visits += 1;
                }
                let previous = prefix.extra.last().copied().unwrap_or(0);
                let mut step = Saturation::default();
                let (start_decoration, decoration_sat) = *decoration.get_or_insert_with(|| {
                    let mut s = Saturation::default();
                    let w = super::decoration::width(data, range.start, true, &mut s);
                    (w, s)
                });
                let pos = raw(
                    costs.advances[i] - costs.advances[range.start] + previous,
                    &mut step,
                )
                .add(clipped_delta, &mut step)
                .add(start_decoration, &mut step)
                .add(
                    costs
                        .spacing
                        .query(range.start..i, Some(data))
                        .width(&mut step),
                    &mut step,
                );
                absorb(&mut step, decoration_sat);
                let extra =
                    i64::from(super::scan::tab_width(data, &data.units[i], pos, &mut step).raw());
                prefix.extra.push(previous + extra);
                if !step.is_clean() && prefix.sat.is_empty() {
                    // The first saturating step: earlier steps were clean.
                    prefix
                        .sat
                        .resize(prefix.extra.len() - 1, Saturation::default());
                    prefix.sat.push(step);
                } else if !prefix.sat.is_empty() {
                    let mut running = prefix.sat.last().copied().unwrap_or_default();
                    absorb(&mut running, step);
                    prefix.sat.push(running);
                }
            }
            prefix.through = range.end;
        }
        tab_total = prefix.before(&costs.tabs, range.end);
        absorb(sat, prefix.saturation(&costs.tabs, range.end));
    }
    let costs = cx.ruby_ranges.sets.get(&key)?;
    let mut stop = range
        .start
        .max(costs.last_content[range.end])
        .max(costs.last_blocked_preserved[range.end]);
    if super::whitespace::obstructed(data, range.end) {
        stop = stop.max(costs.last_preserved[range.end]);
    }
    let next = costs.hanging_units.partition_point(|u| *u < stop);
    let hang = costs
        .hanging_units
        .get(next)
        .copied()
        .filter(|i| *i < range.end)
        .unwrap_or(range.end);
    let trailing_tabs = costs
        .tab_prefix
        .as_ref()
        .filter(|p| p.start == range.start)
        .map_or(0, |p| {
            p.before(&costs.tabs, range.end) - p.before(&costs.tabs, hang)
        });
    let trailing = costs.hanging_advances[range.end] - costs.hanging_advances[hang] + trailing_tabs;
    let natural = raw(
        costs.advances[range.end] - costs.advances[range.start] + tab_total - trailing,
        sat,
    );
    let natural = natural.add(
        corrections
            .iter()
            .filter(|(i, _)| *i < hang)
            .fold(LayoutUnit::ZERO, |w, (_, delta)| w.add(*delta, sat)),
        sat,
    );
    let summary = costs
        .spacing
        .query_replace(range.start..hang, hyphen_leaf, Some(data));
    let width = natural
        .add(summary.width(sat), sat)
        .add(super::decoration::width(data, range.start, true, sat), sat)
        .add(super::decoration::width(data, range.end, false, sat), sat);
    #[cfg(test)]
    {
        cx.ruby_measure_visits += costs.spacing.take_visits();
    }
    let bad_start = costs.bad_atomics.partition_point(|i| *i < range.start);
    let bad_end = costs.bad_atomics.partition_point(|i| *i < range.end);
    // Only selected invalid caller values report warnings; the sink's cap also
    // bounds work on repeated hostile candidates after its suppression marker.
    let bad: Vec<_> = costs.bad_atomics[bad_start..bad_end]
        .iter()
        .copied()
        .take(if cx.warnings.is_suppressed() {
            0
        } else {
            data.limits
                .max_warnings
                .unwrap_or(u64::MAX)
                .saturating_add(1)
                .min(usize::MAX as u64) as usize
        })
        .collect();
    for i in bad {
        if cx.warnings.is_suppressed() {
            break;
        }
        super::scan::unit_width(data, &data.units[i], LayoutUnit::ZERO, atomics, cx, sat);
    }
    let delta = match hyphen_windows {
        Some(windows) => super::windows::cost(&windows, range.end, sat),
        None => super::windows::delta(data, range.start, range.end, cx, sat),
    };
    let width = width.add(delta, sat);
    let adjustment = super::punctuation::edges(
        data,
        summary,
        0,
        super::punctuation::last_edge(data, range.end),
        None,
        LayoutUnit::MAX,
        width,
        sat,
    );
    Some(width.sub(adjustment.removed(sat), sat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{
        BoxDecorationBreak, FontFamily, InlineStyle, ParagraphStyle, UnicodeBidi,
        WhiteSpaceCollapse,
    };
    use crate::{Paragraph, ParagraphBuilder};

    fn fonts() -> FontCollection {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::test_support::fonts::CJK.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
    }

    #[test]
    fn indexed_discretionary_hyphens_match_actual_bidi_spacing_and_edge_windows() {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            let root = InlineStyle {
                font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
                font_size: 12.0,
                letter_spacing: 1.5,
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction,
                    root: root.clone(),
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}");
            b.open_inline(
                NodeId(2),
                &InlineStyle {
                    unicode_bidi: UnicodeBidi::Isolate,
                    letter_spacing: 2.5,
                    box_decoration_break: BoxDecorationBreak::Clone,
                    ..root
                },
                InlineEdges {
                    padding: Sides {
                        inline_start: 3.0,
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            b.push_text(TextSource::Generated { node: NodeId(3) }, "cd\u{ad}");
            b.close_inline();
            b.push_text(TextSource::Generated { node: NodeId(4) }, "ef");
            let mut cx = LayoutContext::new();
            let p = b.build(&mut cx, &fonts).unwrap();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .expect("manual hyphen uses actual indexed costs");
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "{direction:?}: {start}..{end}");
                }
            }
        }
    }

    #[test]
    fn indexed_source_clipped_shared_cluster_keeps_actual_remaining_glyph_width() {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
            font_size: 12.0,
            overflow_wrap: crate::style::OverflowWrap::Anywhere,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            for suffix in ["WWWW", "\tWWWW "] {
                let mut b = ParagraphBuilder::new(
                    &ParagraphStyle {
                        root: root.clone(),
                        direction,
                        ..Default::default()
                    },
                    &Limits::default(),
                );
                for (node, text) in [(1, "f"), (2, "f"), (3, "i"), (4, suffix)] {
                    b.push_text(
                        TextSource::Dom {
                            node: NodeId(node),
                            offset: 40,
                        },
                        text,
                    );
                }
                let mut cx = LayoutContext::new();
                let p = b.build(&mut cx, &fonts).unwrap();
                assert!(p.data.units.iter().any(|u| {
                    u.shared_cluster
                        .as_ref()
                        .is_some_and(|c| c.slices.len() == 3)
                }));
                cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                for start in 0..p.data.units.len() {
                    for end in start + 1..=p.data.units.len() {
                        let actual = width(
                            &p.data,
                            start..end,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut Saturation::default(),
                        )
                        .expect("only the clipped source cluster needs bounded correction");
                        let expected = super::super::plan::selected_raw(
                            &p.data,
                            start,
                            end,
                            LayoutUnit::ZERO,
                            LayoutUnit::ZERO,
                            0,
                            None,
                            LayoutUnit::MAX,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut Saturation::default(),
                        )
                        .content;
                        assert_eq!(
                            actual, expected,
                            "{direction:?}, {suffix:?}: source clipped {start}..{end}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn indexed_tabs_and_atomic_ranges_match_actual_selected_windows_after_revision() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            letter_spacing: 1.5,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t");
        b.push_atomic(NodeId(9), &root, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(2) }, "本\t語 ");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let mut atomics = AtomicSizes::new();
        for size in [17.0, 31.0] {
            atomics.insert(
                NodeId(9),
                crate::AtomicSize {
                    inline_size: size,
                    margins: Sides {
                        inline_start: 2.0,
                        inline_end: 3.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            cx.ruby_ranges.begin(&p.data, &atomics);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &atomics,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .expect("dynamic range must use the index");
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &atomics,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "atomic{size}: {start}..{end}");
                }
            }
        }
    }

    #[test]
    fn indexing_a_missing_atomic_warns_only_when_its_range_is_selected() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        b.push_atomic(NodeId(9), &root, InlineEdges::default());
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        cx.take_warnings();
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        assert_eq!(
            width(
                &p.data,
                0..1,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default()
            )
            .unwrap()
            .to_f32(),
            24.0
        );
        assert!(cx.take_warnings().is_empty());
        width(
            &p.data,
            0..p.data.units.len(),
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        )
        .unwrap();
        assert!(
            cx.take_warnings()
                .iter()
                .any(|w| w.kind == crate::limits::WarningKind::MissingAtomicSize)
        );
    }

    #[test]
    fn indexed_actual_ranges_match_selected_edge_spacing_and_cloned_boxes() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::test_support::fonts::CJK.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            let root = InlineStyle {
                font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
                font_size: 24.0,
                letter_spacing: 1.5,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction,
                    root: root.clone(),
                    ..Default::default()
                },
                &limits,
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "日本 ");
            b.open_inline(
                NodeId(2),
                &InlineStyle {
                    font_size: 12.0,
                    unicode_bidi: UnicodeBidi::Isolate,
                    box_decoration_break: BoxDecorationBreak::Clone,
                    ..root
                },
                InlineEdges {
                    padding: Sides {
                        inline_start: 3.0,
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            b.push_text(TextSource::Generated { node: NodeId(3) }, "にほん ");
            b.close_inline();
            b.push_text(TextSource::Generated { node: NodeId(4) }, "語ご ");
            let mut cx = LayoutContext::new();
            let p = b.build(&mut cx, &fonts).unwrap();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .unwrap();
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "{direction:?}: units{start}..{end}");
                }
            }
        }
    }

    /// Only clearing the caches moves the epoch: building costs, tab
    /// prefixes (extended, covered or replaced) and block measurements do
    /// not.
    #[test]
    fn only_cleared_caches_move_the_epoch() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(1.0e12),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t本\t語");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let n = p.data.units.len();
        assert!(n >= 4, "{n}");
        let query = |cx: &mut LayoutContext, range: Range<usize>| {
            width(
                &p.data,
                range,
                &AtomicSizes::EMPTY,
                cx,
                &mut Saturation::default(),
            )
            .unwrap();
        };
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let e = cx.ruby_ranges.epoch();
        // Build, extend, cover and replace saturating tab prefixes.
        for range in [0..2, 0..n, 0..3, 1..n, 0..n, 2..n] {
            query(&mut cx, range);
        }
        block_size(
            &p.data,
            0..2,
            &Default::default(),
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        );
        assert_eq!(cx.ruby_ranges.epoch(), e);
        // A new atomic revision clears every cache.
        let mut atomics = AtomicSizes::new();
        atomics.insert(NodeId(9), crate::AtomicSize::default());
        cx.ruby_ranges.begin(&p.data, &atomics);
        assert_eq!(cx.ruby_ranges.epoch(), e + 1);
        // Vacating the slots invalidates.
        cx.ruby_ranges.vacate_slots();
        assert_eq!(cx.ruby_ranges.epoch(), e + 2);
    }

    /// A prefix whose early tab steps are clean and a later one saturates:
    /// a query charges exactly the saturating steps before its end, whether
    /// the prefix computed them now, earlier, or for another start first.
    #[test]
    fn covered_tab_steps_charge_like_computed_ones() {
        let clean = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        let huge = InlineStyle {
            tab_size: crate::style::TabSize::Px(1.0e12),
            ..clean.clone()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: clean.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t本");
        b.open_inline(NodeId(2), &huge, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(3) }, "\t語");
        b.close_inline();
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let tabs: Vec<usize> = p
            .data
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| matches!(u.kind, UnitKind::Tab))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(tabs.len(), 2, "{tabs:?}");
        let (first, second) = (tabs[0], tabs[1]);
        let n = p.data.units.len();
        let query = |cx: &mut LayoutContext, range: Range<usize>| {
            let mut sat = Saturation::default();
            width(&p.data, range, &AtomicSizes::EMPTY, cx, &mut sat).unwrap();
            sat
        };
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        // Clean steps charge nothing, computed or covered.
        assert!(query(&mut cx, 0..first + 1).is_clean());
        assert!(query(&mut cx, 0..second).is_clean());
        // The saturating step charges when computed and again when covered.
        let full = query(&mut cx, 0..n);
        assert!(!full.is_clean());
        assert_eq!(query(&mut cx, 0..n), full);
        // Still charged after the prefix was replaced for another start.
        assert!(!query(&mut cx, 1..n).is_clean());
        assert_eq!(query(&mut cx, 0..n), full);
        // Covered prefix ends before the saturating tab: clean again.
        assert!(query(&mut cx, 0..second).is_clean());
    }

    /// A Latin paragraph whose ligature and kerning edges need reshape
    /// windows (charging the budget) for some ranges, with preserved tabs
    /// and optionally one unit whose width saturates.
    fn charging_paragraph(tab_size: crate::style::TabSize, saturating_unit: bool) -> Paragraph {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
            font_size: 12.0,
            overflow_wrap: crate::style::OverflowWrap::Anywhere,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size,
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        for (node, text) in [(1, "f"), (2, "f"), (3, "i"), (4, "\tAVAW\tWAy fi")] {
            b.push_text(
                TextSource::Dom {
                    node: NodeId(node),
                    offset: 40,
                },
                text,
            );
        }
        let mut p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        if saturating_unit {
            // Sanitized styles keep every single unit below `LayoutUnit::MAX`;
            // force one so building the range costs saturates.
            let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
            let unit = data
                .units
                .iter()
                .position(|u| matches!(u.kind, UnitKind::Cluster { .. }) && u.text.start > 3)
                .unwrap();
            data.unit_spacing[unit].word = LayoutUnit::MAX;
        }
        p
    }

    /// shodo-tj5: a `blocks` hit charges the reshape budget and saturation
    /// exactly as the cold measurement that filled it, and so does a cold
    /// measurement after the caches are vacated.
    #[test]
    fn block_size_charges_the_same_cold_and_warm() {
        let p = charging_paragraph(crate::style::TabSize::Px(40.0), false);
        let n = p.data.units.len();
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let measure = |cx: &mut LayoutContext, range: Range<usize>| {
            cx.begin_reshape_operation();
            let mut sat = Saturation::default();
            let height = block_size(
                &p.data,
                range,
                &Default::default(),
                &AtomicSizes::EMPTY,
                cx,
                &mut sat,
            );
            (height, sat, cx.edge_reshape_spent)
        };
        let mut charged = 0;
        for start in 0..n {
            for end in start + 1..=n {
                let cold = measure(&mut cx, start..end);
                let warm = measure(&mut cx, start..end);
                assert_eq!(cold, warm, "warm {start}..{end}");
                charged += usize::from(cold.2 > 0);
            }
        }
        cx.ruby_ranges.vacate_slots();
        for start in 0..n {
            for end in start + 1..=n {
                let again = measure(&mut cx, start..end);
                let mut fresh = LayoutContext::new();
                fresh.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                assert_eq!(again, measure(&mut fresh, start..end), "{start}..{end}");
            }
        }
        assert!(charged > 0, "some range must charge the reshape budget");
    }

    /// shodo-mc0: the block caches stay within their cap, and crossing it
    /// (a clear before the insert) leaves every query's height, saturation,
    /// spent bytes and warnings equal to a context that never cached.
    #[test]
    fn block_cap_crossing_matches_a_fresh_context() {
        let p = charging_paragraph(crate::style::TabSize::Px(40.0), false);
        let n = p.data.units.len();
        let measure = |cx: &mut LayoutContext, range: Range<usize>| {
            cx.begin_reshape_operation();
            let mut sat = Saturation::default();
            let height = block_size(
                &p.data,
                range,
                &Default::default(),
                &AtomicSizes::EMPTY,
                cx,
                &mut sat,
            );
            (height, sat, cx.edge_reshape_spent, cx.take_warnings())
        };
        let mut capped = LayoutContext::new();
        capped.ruby_ranges.block_cap = Some(2);
        capped.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let mut clears = 0;
        for _ in 0..2 {
            for start in 0..n {
                for end in start + 1..=n {
                    let before = capped.ruby_ranges.block_entries();
                    let got = measure(&mut capped, start..end);
                    let after = capped.ruby_ranges.block_entries();
                    clears += usize::from(after < before);
                    assert!(after <= 2, "{after} entries");
                    let mut fresh = LayoutContext::new();
                    fresh.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                    assert_eq!(got, measure(&mut fresh, start..end), "{start}..{end}");
                }
            }
        }
        assert!(clears > 0, "the cap must be crossed");
    }

    /// shodo-tj5: a `blocks` entry with effects whose replay the gate refuses
    /// (the operation's budget is nearly spent, or the sink's suppression
    /// changed) is measured again, exactly as in a context that never cached
    /// it: same height, saturation, spent bytes and warnings, also for the
    /// next query after a measurement that warned (it is not stored).
    #[test]
    fn refused_block_replays_measure_like_a_fresh_context() {
        let p = charging_paragraph(crate::style::TabSize::Px(40.0), false);
        let n = p.data.units.len();
        let limit = p.data.limits.max_reshape_window_bytes.unwrap() * 64;
        let measure = |cx: &mut LayoutContext, range: Range<usize>| {
            let mut sat = Saturation::default();
            let height = block_size(
                &p.data,
                range,
                &Default::default(),
                &AtomicSizes::EMPTY,
                cx,
                &mut sat,
            );
            (height, sat, cx.edge_reshape_spent, cx.take_warnings())
        };
        // Bring a context to the preset state: spent bytes and suppression.
        let preset = |cx: &mut LayoutContext, spent: u64, suppress: bool| {
            cx.begin_reshape_operation();
            cx.take_warnings();
            cx.warnings.set_max(None);
            cx.edge_reshape_spent = spent;
            if suppress {
                cx.warnings.set_max(Some(0));
                cx.warnings
                    .push(crate::limits::WarningKind::Unsupported, "suppress");
                assert!(cx.warnings.is_suppressed());
            }
        };
        let mut refused = 0;
        let mut warned = 0;
        for start in 0..n {
            for end in start + 1..=n {
                let mut warm = LayoutContext::new();
                warm.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                preset(&mut warm, 0, false);
                if measure(&mut warm, start..end).2 == 0 {
                    continue;
                }
                for (spent, suppress) in [(limit - 1, false), (0, true)] {
                    let mut fresh = LayoutContext::new();
                    fresh.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                    preset(&mut warm, spent, suppress);
                    preset(&mut fresh, spent, suppress);
                    let before = warm.ruby_replay_refusals;
                    // Twice: the second query follows a measurement that
                    // may have warned (and so was not stored).
                    for _ in 0..2 {
                        let got = measure(&mut warm, start..end);
                        let want = measure(&mut fresh, start..end);
                        warned += usize::from(!want.3.is_empty());
                        assert_eq!(got, want, "{start}..{end} {spent} {suppress}");
                    }
                    refused += warm.ruby_replay_refusals - before;
                }
            }
        }
        assert!(refused > 0, "some replay must be refused");
        assert!(warned > 0, "some refused measurement must warn");
    }

    /// shodo-tj5: `width` charges depend only on the range, not on the
    /// queries before it: a history run forwards, backwards and query by
    /// query in fresh contexts gives the same value and saturation per range.
    #[test]
    fn width_charges_do_not_depend_on_query_order() {
        for (tab_size, saturating_unit) in [
            (crate::style::TabSize::Px(40.0), false),
            (crate::style::TabSize::Px(40.0), true),
            (crate::style::TabSize::Px(1.0e12), false),
        ] {
            let p = charging_paragraph(tab_size, saturating_unit);
            let n = p.data.units.len();
            let mut history = Vec::new();
            for start in 0..n {
                for end in start + 1..=n {
                    history.push(start..end);
                }
            }
            let run = |cx: &mut LayoutContext, range: Range<usize>| {
                cx.begin_reshape_operation();
                let mut sat = Saturation::default();
                let value = width(&p.data, range, &AtomicSizes::EMPTY, cx, &mut sat);
                (value, sat, cx.edge_reshape_spent)
            };
            let mut forwards = LayoutContext::new();
            forwards.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            let mut backwards = LayoutContext::new();
            backwards.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            let ahead: Vec<_> = history
                .iter()
                .map(|r| run(&mut forwards, r.clone()))
                .collect();
            let mut behind: Vec<_> = history
                .iter()
                .rev()
                .map(|r| run(&mut backwards, r.clone()))
                .collect();
            behind.reverse();
            let mut saturated = false;
            for ((range, a), b) in history.iter().zip(&ahead).zip(&behind) {
                let mut fresh = LayoutContext::new();
                fresh.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                let alone = run(&mut fresh, range.clone());
                assert_eq!(
                    a, &alone,
                    "{tab_size:?} {saturating_unit} forwards {range:?}"
                );
                assert_eq!(
                    b, &alone,
                    "{tab_size:?} {saturating_unit} backwards {range:?}"
                );
                saturated |= !alone.1.is_clean();
            }
            if saturating_unit || tab_size == crate::style::TabSize::Px(1.0e12) {
                assert!(saturated, "{tab_size:?} {saturating_unit}: must saturate");
            }
            if tab_size == crate::style::TabSize::Px(1.0e12) {
                // The first tab's own step saturates, also when the tab is
                // the computed prefix's first step and hangs at the end.
                let tab = p
                    .data
                    .units
                    .iter()
                    .position(|u| matches!(u.kind, UnitKind::Tab))
                    .unwrap();
                let i = history.iter().position(|r| *r == (0..tab + 1)).unwrap();
                assert!(!ahead[i].1.is_clean(), "{:?}", ahead[i]);
            }
        }
    }

    /// One paragraph per golden fixture: preserved tabs under different
    /// tab sizes, a ligature clipped by the range start, hanging trailing
    /// tabs and a tab interval that saturates.
    fn tab_golden_paragraphs() -> Vec<(&'static str, Paragraph)> {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = |family: &str, tab_size: crate::style::TabSize| InlineStyle {
            font_families: vec![FontFamily::Named(family.into())],
            font_size: 24.0,
            letter_spacing: 1.5,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size,
            ..Default::default()
        };
        let build = |root: InlineStyle, texts: &[(u64, &str)]| {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: root.clone(),
                    ..Default::default()
                },
                &Limits::default(),
            );
            for (node, text) in texts {
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(*node),
                        offset: 40,
                    },
                    text,
                );
            }
            b.build(&mut LayoutContext::new(), &fonts).unwrap()
        };
        use crate::style::TabSize;
        vec![
            (
                "cjk",
                build(
                    style("Shodo Fixture CJK", TabSize::Px(40.0)),
                    &[(1, "日\t本\t\t語\t \t")],
                ),
            ),
            (
                "spaces",
                build(
                    style("Shodo Fixture CJK", TabSize::Spaces(3.0)),
                    &[(1, "日本\t語\t日")],
                ),
            ),
            (
                "ligature",
                build(
                    InlineStyle {
                        font_size: 12.0,
                        letter_spacing: 0.0,
                        overflow_wrap: crate::style::OverflowWrap::Anywhere,
                        ..style("Shodo Fixture Latin", TabSize::Px(40.0))
                    },
                    &[(1, "f"), (2, "f"), (3, "i"), (4, "\tWW\tW ")],
                ),
            ),
            (
                "huge",
                build(
                    style("Shodo Fixture CJK", TabSize::Px(1.0e12)),
                    &[(1, "日\t本\t語\t")],
                ),
            ),
        ]
    }

    /// Value and saturation of every query of a fixed history, one context
    /// per fixture (see `tab_golden_paragraphs`).
    fn tab_golden() -> String {
        let mut out = String::new();
        for (name, p) in tab_golden_paragraphs() {
            if name == "ligature" {
                assert!(p.data.units.iter().any(|u| {
                    u.shared_cluster
                        .as_ref()
                        .is_some_and(|c| c.slices.len() == 3)
                }));
            }
            let n = p.data.units.len();
            let mut history = vec![0..2, 0..n, 0..3, 1..n, 0..n, 2..n, n - 2..n, 1..3, 0..n];
            for start in 0..n {
                for end in start + 1..=n {
                    history.push(start..end);
                }
            }
            for end in (1..=n).rev() {
                history.push(0..end);
            }
            let mut cx = LayoutContext::new();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for range in history {
                let mut sat = Saturation::default();
                let value = width(
                    &p.data,
                    range.clone(),
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut sat,
                )
                .map(|w| w.raw());
                out.push_str(&format!(
                    "{name} {range:?} {value:?} {} {}\n",
                    sat.saturated, sat.non_finite
                ));
            }
        }
        out
    }

    /// Byte-identity with the previous tab prefix: the golden file was
    /// captured on the code before shodo-b7d. Regenerate only on purpose
    /// with `SHODO_UPDATE_GOLDEN=1`.
    #[test]
    fn tab_width_queries_match_the_golden_record() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/line/testdata/b7d_tab_golden.txt"
        );
        let got = tab_golden();
        if std::env::var_os("SHODO_UPDATE_GOLDEN").is_some() {
            std::fs::write(path, &got).unwrap();
            panic!("golden updated; rerun without SHODO_UPDATE_GOLDEN");
        }
        let want = std::fs::read_to_string(path).expect("golden file");
        assert_eq!(got, want);
        // shodo-tj5: a repeated query charges what it charged the first time,
        // whatever ran in between.
        let mut seen = std::collections::HashMap::new();
        for line in want.lines() {
            let mut words = line.splitn(3, ' ');
            let key = (words.next(), words.next());
            let rest = words.next();
            assert_eq!(*seen.entry(key).or_insert(rest), rest, "{line}");
        }
        // The record must exercise saturation, a clipped ligature and tabs.
        assert!(
            want.lines()
                .any(|l| l.starts_with("huge ") && !l.ends_with(" 0 0")),
            "the huge tab size must saturate"
        );
    }
}
