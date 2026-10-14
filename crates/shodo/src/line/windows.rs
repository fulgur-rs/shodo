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
        changes_delta(&self.changes, through, sat)
    }
}

/// Width and coverage results need no glyph ownership after measurement.
struct MeasuredWindow {
    glyphs: Range<u32>,
    text: Range<u32>,
    glyph_count: usize,
    changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
}

impl MeasuredWindow {
    fn delta(&self, through: usize, sat: &mut Saturation) -> LayoutUnit {
        changes_delta(&self.changes, through, sat)
    }
}

/// Specialize the same validation and arithmetic for compact measurements and
/// final owned overlays, without an intermediate vector of glyph handles.
trait WindowOutput {
    const RETAIN_SELECTED: bool;
    fn from_shape(
        glyphs: Range<u32>,
        text: Range<u32>,
        shaped: WindowHandle,
        changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
        replacement: Option<&crate::shape::Replacement>,
    ) -> Self;
    fn glyphs(&self) -> &Range<u32>;
    fn glyph_count(&self) -> usize;
}

impl WindowOutput for Window {
    const RETAIN_SELECTED: bool = true;
    fn from_shape(
        glyphs: Range<u32>,
        text: Range<u32>,
        shaped: WindowHandle,
        changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
        replacement: Option<&crate::shape::Replacement>,
    ) -> Self {
        let hyphen = replacement
            .filter(|r| text.start <= r.text.start && r.text.end <= text.end)
            .map(|r| r.text.clone());
        let (store, runs) = shaped.into_owned();
        Self {
            overlay: EdgeOverlay {
                glyphs,
                text,
                store,
                runs,
                hyphen,
            },
            changes,
        }
    }

    fn glyphs(&self) -> &Range<u32> {
        &self.overlay.glyphs
    }

    fn glyph_count(&self) -> usize {
        self.overlay.store.len()
    }
}

impl WindowOutput for MeasuredWindow {
    const RETAIN_SELECTED: bool = false;
    fn from_shape(
        glyphs: Range<u32>,
        text: Range<u32>,
        shaped: WindowHandle,
        changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
        _replacement: Option<&crate::shape::Replacement>,
    ) -> Self {
        Self {
            glyphs,
            text,
            glyph_count: shaped.0.len(),
            changes,
        }
    }

    fn glyphs(&self) -> &Range<u32> {
        &self.glyphs
    }

    fn glyph_count(&self) -> usize {
        self.glyph_count
    }
}

fn changes_delta(
    changes: &[(usize, LayoutUnit, LayoutUnit)],
    through: usize,
    sat: &mut Saturation,
) -> LayoutUnit {
    changes
        .iter()
        .filter(|(i, _, _)| *i < through)
        .fold(LayoutUnit::ZERO, |p, (_, old, new)| {
            p.add(new.sub(*old, sat), sat)
        })
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
    let windows = measure_edit::<Window>(data, start, end, Some(replacement), cx, sat);
    let generated = windows.iter().any(|w| {
        w.overlay.text.start <= replacement.text.start && replacement.text.end <= w.overlay.text.end
    });
    if !generated
        || !covers_partial_edges(data, start, end, windows.iter().map(|w| &w.overlay.text))
    {
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
        && a_run.shaping_input == b_run.shaping_input
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
/// total counts retained vector capacity and a metadata allowance. Oversized
/// results are not cached. Once full, preserve resident results rather than
/// repeatedly discarding the entire cache during a long candidate scan.
/// A quarter of the byte budget is reserved for compact candidate width/validity
/// records. They replay the full measurement's budget and saturation effects
/// through `replay`, including the partial-cluster suffix check. They own no
/// drawing glyphs, so later candidates still fit after glyph admission stops.
/// Verified drawing windows may borrow unused measurement reservation. Growing
/// measurements release that supplemental table first; ordinary residents stay.
#[derive(Default)]
pub(crate) struct EdgeShapeCache {
    owner: Option<(u64, usize)>,
    entries: crate::hashing::FastMap<(usize, usize, Option<u64>), ShapedWindow>,
    selected: crate::hashing::FastMap<(usize, usize, Option<u64>), ShapedWindow>,
    selected_cost: usize,
    // Ordinary glyph snapshots only; supplemental snapshots share the separate
    // measurement reservation and cannot reduce ordinary glyph admission.
    cost: usize,
    measurements: Vec<RetainedMeasurement>,
    measurement_effects: Vec<super::replay::Effects>,
    effect_indices: crate::hashing::FastMap<super::replay::Effects, u16>,
    #[cfg(test)]
    pub(crate) measurement_hits: usize,
    #[cfg(test)]
    pub(crate) measurement_misses: usize,
}

impl std::fmt::Debug for EdgeShapeCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("EdgeShapeCache");
        debug
            .field("entries", &self.entries.len())
            .field("selected", &self.selected.len())
            .field("selected_cost", &self.selected_cost)
            .field("cost", &self.cost)
            .field("measurement_cost", &self.measurement_storage_cost())
            .field("measurements", &self.measurements.len())
            .field("effect_records", &self.measurement_effects.len());
        #[cfg(test)]
        debug
            .field("measurement_hits", &self.measurement_hits)
            .field("measurement_misses", &self.measurement_misses);
        debug.finish()
    }
}

/// Inline records avoid a glyph store, run instances, and a hash-table slot
/// allowance for every candidate. Keys stay sorted; an ordinary line scan
/// appends new starts and inserts only among the latest start's endpoints.
#[derive(Clone, Copy)]
struct CandidateMeasurement {
    delta: LayoutUnit,
    valid: bool,
    effects: super::replay::Effects,
}

#[derive(Clone, Copy)]
struct RetainedMeasurement {
    // The paragraph's source/unit indices are ordinarily u32. Out-of-range
    // queries bypass optional retention instead of truncating their keys.
    key: (u32, u32),
    delta: LayoutUnit,
    effects: u16,
    valid: bool,
}

type OwnedShapedWindow = (crate::shape::GlyphStore, Vec<crate::shape::ShapedRun>);
type ShapedWindow = std::sync::Arc<OwnedShapedWindow>;

// Keep uncacheable results on the same allocation-free owned path. Boxing the
// larger variant would add an allocation to every edited/oversized window.
#[allow(clippy::large_enum_variant)]
enum WindowHandle {
    Owned(OwnedShapedWindow),
    // Clean, unedited, small shape refused by ordinary snapshot admission.
    // Only a final validated drawing output may borrow measurement space.
    UnretainedClean(OwnedShapedWindow),
    Shared(ShapedWindow),
}

impl std::ops::Deref for WindowHandle {
    type Target = OwnedShapedWindow;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(window) | Self::UnretainedClean(window) => window,
            Self::Shared(window) => window,
        }
    }
}

impl WindowHandle {
    fn into_owned(self) -> OwnedShapedWindow {
        match self {
            Self::Owned(window) | Self::UnretainedClean(window) => window,
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
    // Cloning produces len-sized vectors. Refuse admission before allocating
    // that snapshot when it cannot fit; the owned miss remains usable.
    if !cache.can_insert(&key, window_storage_cost(&shaped, true)) {
        return WindowHandle::UnretainedClean(shaped);
    }
    // A miss keeps its original output vectors. Clone exactly one len-sized
    // cache snapshot, as before, instead of shrinking every growing vector and
    // then cloning again if this newly shaped window is selected.
    let shared = std::sync::Arc::new(shaped.clone());
    cache.insert(key, &shared);
    WindowHandle::Owned(shaped)
}

const EDGE_SHAPE_CACHE_ENTRIES: usize = 2048;
/// Accounted retained bytes, including a conservative metadata allowance.
/// This bounds cache storage, rather than process RSS or allocator overhead.
const EDGE_SHAPE_CACHE_COST: usize = 1 << 20;
// Reserve a quarter for compact width/validity measurements, so drawing snapshots
// cannot prevent later candidates from being remembered in a long paragraph.
const EDGE_MEASUREMENT_CACHE_COST: usize = EDGE_SHAPE_CACHE_COST / 4;
const EDGE_GLYPH_CACHE_COST: usize = EDGE_SHAPE_CACHE_COST - EDGE_MEASUREMENT_CACHE_COST;
const EDGE_MEASUREMENT_GROWTH: usize = 64;
const EDGE_EFFECT_GROWTH: usize = 16;
/// A single window costing more than this is shaped every time instead.
const EDGE_SHAPE_ENTRY_COST_MAX: usize = 1 << 10;

fn window_cost(window: &OwnedShapedWindow) -> usize {
    // A run holds an `Arc` and a few scalars; count it as a few glyphs.
    window.0.len() + window.1.len() * 4
}

fn window_storage_cost(window: &OwnedShapedWindow, snapshot: bool) -> usize {
    use crate::shape::ByteBasis;
    use std::mem::size_of;
    let basis = if snapshot {
        ByteBasis::Len
    } else {
        ByteBasis::Capacity
    };
    let (store, runs) = window;
    let run_count = match basis {
        ByteBasis::Len => runs.len(),
        ByteBasis::Capacity => runs.capacity(),
    };
    // The Arc owns the tuple and two counters. Allow four table slots per
    // entry plus control bytes to cover spare HashMap capacity as it grows.
    let metadata = size_of::<OwnedShapedWindow>()
        + 2 * size_of::<usize>()
        + 4 * size_of::<((usize, usize, Option<u64>), ShapedWindow)>()
        + 2 * size_of::<usize>();
    let mut cost =
        metadata + store.retained_bytes(basis) + run_count * size_of::<crate::shape::ShapedRun>();
    // Edge shaping resolves fresh instances. Their Arcs keep coordinates,
    // variations, language and features alive as well as the run scalars.
    // Count preparation once per consecutive input, preserving the conservative
    // admission cost even when different inputs now share their instance Arc.
    let mut previous = None;
    for run in runs {
        if previous
            .is_some_and(|(p, input)| input == run.shaping_input && Arc::ptr_eq(p, &run.instance))
        {
            continue;
        }
        let instance = &run.instance;
        previous = Some((instance, run.shaping_input));
        cost += instance.retained_bytes();
    }
    cost
}

fn effect_index_cost() -> usize {
    // Four slots plus control bytes per full Effects key cover table slack,
    // as in drawing snapshot accounting. The indexed Effects vector is
    // counted separately by actual capacity.
    4 * std::mem::size_of::<(super::replay::Effects, u16)>() + 4 * std::mem::size_of::<usize>()
}

fn measurement_cost(entry_capacity: usize, effect_capacity: usize, effect_count: usize) -> usize {
    entry_capacity * std::mem::size_of::<RetainedMeasurement>()
        + effect_capacity * std::mem::size_of::<super::replay::Effects>()
        + effect_count * effect_index_cost()
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
        self.entries.len() + self.selected.len()
    }

    pub(crate) fn clear(&mut self) {
        self.entries = Default::default();
        self.selected = Default::default();
        self.selected_cost = 0;
        self.cost = 0;
        self.measurements = Vec::new();
        self.measurement_effects = Vec::new();
        self.effect_indices = Default::default();
        #[cfg(test)]
        {
            self.measurement_hits = 0;
            self.measurement_misses = 0;
        }
        self.owner = None;
    }

    fn get(&self, key: &(usize, usize, Option<u64>)) -> Option<&ShapedWindow> {
        self.entries.get(key).or_else(|| self.selected.get(key))
    }

    fn can_insert(&self, key: &(usize, usize, Option<u64>), cost: usize) -> bool {
        let previous = self.entries.get(key);
        let retained = self.cost - previous.map_or(0, |w| window_storage_cost(w, false));
        (previous.is_some() || self.entries.len() < EDGE_SHAPE_CACHE_ENTRIES)
            && cost <= EDGE_GLYPH_CACHE_COST - retained
    }

    fn measurement_storage_cost(&self) -> usize {
        measurement_cost(
            self.measurements.capacity(),
            self.measurement_effects.capacity(),
            self.effect_indices.len(),
        )
    }

    fn release_selected(&mut self) {
        // Release table capacity along with glyph Arcs, so freed entries do
        // not leave unaccounted hash-table storage behind.
        self.selected = Default::default();
        self.selected_cost = 0;
    }

    fn measurement(&self, key: (usize, usize)) -> Option<CandidateMeasurement> {
        let compact = (u32::try_from(key.0).ok()?, u32::try_from(key.1).ok()?);
        self.measurements
            .binary_search_by_key(&compact, |m| m.key)
            .ok()
            .map(|i| {
                let hit = self.measurements[i];
                CandidateMeasurement {
                    delta: hit.delta,
                    valid: hit.valid,
                    effects: self.measurement_effects[usize::from(hit.effects)],
                }
            })
    }

    fn remember_measurement(&mut self, key: (usize, usize), measurement: CandidateMeasurement) {
        let (Ok(start), Ok(end)) = (u32::try_from(key.0), u32::try_from(key.1)) else {
            return;
        };
        let key = (start, end);
        let found = self.measurements.binary_search_by_key(&key, |m| m.key);
        let existing_effect = self.effect_indices.get(&measurement.effects).copied();
        let Ok(new_effect_index) = u16::try_from(self.measurement_effects.len()) else {
            return;
        };
        let grow_entries =
            found.is_err() && self.measurements.len() == self.measurements.capacity();
        let grow_effects = existing_effect.is_none()
            && self.measurement_effects.len() == self.measurement_effects.capacity();
        let entry_capacity = self.measurements.capacity()
            + if grow_entries {
                EDGE_MEASUREMENT_GROWTH
            } else {
                0
            };
        let effect_capacity =
            self.measurement_effects.capacity() + if grow_effects { EDGE_EFFECT_GROWTH } else { 0 };
        let effect_count = self.effect_indices.len() + usize::from(existing_effect.is_none());
        // Refuse without allocating when either side cannot fit.
        if measurement_cost(entry_capacity, effect_capacity, effect_count)
            > EDGE_MEASUREMENT_CACHE_COST
        {
            return;
        }
        // `with_capacity` may over-allocate, so check the actual capacities
        // again before retaining the new buffers.
        let entries = grow_entries.then(|| Vec::with_capacity(entry_capacity));
        let effects = grow_effects.then(|| Vec::with_capacity(effect_capacity));
        let actual_cost = measurement_cost(
            entries
                .as_ref()
                .map_or(self.measurements.capacity(), Vec::capacity),
            effects
                .as_ref()
                .map_or(self.measurement_effects.capacity(), Vec::capacity),
            effect_count,
        );
        if actual_cost > EDGE_MEASUREMENT_CACHE_COST {
            return;
        }
        if self.selected_cost + actual_cost > EDGE_MEASUREMENT_CACHE_COST {
            self.release_selected();
        }
        if let Some(mut grown) = entries {
            grown.extend_from_slice(&self.measurements);
            self.measurements = grown;
        }
        if let Some(mut grown) = effects {
            grown.extend_from_slice(&self.measurement_effects);
            self.measurement_effects = grown;
        }
        let effect_index = existing_effect.unwrap_or_else(|| {
            self.measurement_effects.push(measurement.effects);
            self.effect_indices
                .insert(measurement.effects, new_effect_index);
            new_effect_index
        });
        let retained = RetainedMeasurement {
            key,
            delta: measurement.delta,
            valid: measurement.valid,
            effects: effect_index,
        };
        match found {
            Ok(index) => self.measurements[index] = retained,
            Err(index) => self.measurements.insert(index, retained),
        }
    }

    /// Returns whether the window was retained.
    fn insert(&mut self, key: (usize, usize, Option<u64>), window: &ShapedWindow) -> bool {
        if window_cost(window) > EDGE_SHAPE_ENTRY_COST_MAX {
            return false;
        }
        let cost = window_storage_cost(window, false);
        if !self.can_insert(&key, cost) {
            return false;
        }
        if !self.entries.contains_key(&key)
            && self.entries.len() + self.selected.len() >= EDGE_SHAPE_CACHE_ENTRIES
        {
            self.release_selected();
        }
        if let Some(previous) = self.entries.insert(key, window.clone()) {
            self.cost -= window_storage_cost(&previous, false);
        }
        self.cost += cost;
        true
    }

    fn remember_selected(&mut self, key: (usize, usize, Option<u64>), window: &WindowHandle) {
        let WindowHandle::UnretainedClean(shaped) = window else {
            return;
        };
        if self.get(&key).is_some()
            || self.entries.len() + self.selected.len() >= EDGE_SHAPE_CACHE_ENTRIES
            || window_cost(shaped) > EDGE_SHAPE_ENTRY_COST_MAX
        {
            return;
        }
        let cost = window_storage_cost(shaped, true);
        if cost > EDGE_MEASUREMENT_CACHE_COST - self.measurement_storage_cost() - self.selected_cost
        {
            return;
        }
        let snapshot = Arc::new(shaped.clone());
        // Snapshot vectors are len-sized; account actual capacity after cloning.
        let cost = window_storage_cost(&snapshot, false);
        if cost > EDGE_MEASUREMENT_CACHE_COST - self.measurement_storage_cost() - self.selected_cost
        {
            return;
        }
        self.selected.insert(key, snapshot);
        self.selected_cost += cost;
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
    let within = cx.edge_reshape_spent <= limit;
    crate::line::replay::record_charge(cx, bytes, limit, within);
    if within {
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
fn materialize<T: WindowOutput>(
    data: &ParagraphData,
    line: &Range<usize>,
    mut range: Range<usize>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
    budget: Option<u64>,
    replacement: Option<&crate::shape::Replacement>,
) -> Option<T> {
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
        if T::RETAIN_SELECTED {
            cx.edge_shapes
                .remember_selected((range.start, range.end, budget), &shaped);
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
        return Some(T::from_shape(
            begin.start..end.end,
            data.units[range.start].text.start..data.units[range.end - 1].text.end,
            shaped,
            changes,
            replacement,
        ));
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

fn measure_edit<T: WindowOutput>(
    data: &ParagraphData,
    start: usize,
    end: usize,
    replacement: Option<&crate::shape::Replacement>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<T> {
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
    let mut windows: Vec<T> = Vec::new();
    for range in ranges {
        let retained = windows.iter().map(|w| w.glyph_count() as u64).sum::<u64>();
        let budget = data
            .limits
            .max_shaped_glyphs
            .map(|max| max.saturating_sub(retained));
        if let Some(window) = materialize::<T>(data, &line, range, cx, sat, budget, replacement) {
            if let Some(previous) = windows.last()
                && previous.glyphs().end > window.glyphs().start
            {
                // Revalidation may grow two formerly disjoint windows together.
                let merged = first_at..last_at + 1;
                windows.clear();
                if let Some(window) = materialize::<T>(
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
    measure_edit::<MeasuredWindow>(data, start, end, None, cx, sat)
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat))
}

fn covers_partial_edges<'a>(
    data: &ParagraphData,
    start: usize,
    end: usize,
    windows: impl Iterator<Item = &'a Range<u32>> + Clone,
) -> bool {
    [first(data, start, end), last(data, start, end)]
        .into_iter()
        .flatten()
        .all(|at| {
            let range = clipped_group(data, at, &(start..end));
            !partial(data, &range)
                || windows.clone().any(|text| {
                    text.start <= data.units[range.start].text.start
                        && text.end >= data.units[range.end - 1].text.end
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
    cx.edge_shapes.begin(data);
    if let Some(hit) = cx.edge_shapes.measurement((start, end))
        && super::replay::replay(cx, &hit.effects, sat)
    {
        #[cfg(test)]
        {
            cx.edge_shapes.measurement_hits += 1;
        }
        return (hit.delta, hit.valid);
    }
    #[cfg(test)]
    {
        cx.edge_shapes.measurement_misses += 1;
    }
    let recording = super::replay::begin(cx, sat);
    let windows = measure_edit::<MeasuredWindow>(data, start, end, None, cx, sat);
    let delta = windows
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat));
    let covered = covers_partial_edges(data, start, end, windows.iter().map(|w| &w.text));
    let edge_windows = !windows.is_empty();
    drop(windows);
    let partial = covered && has_partial_remainder(data, start, end);
    let valid = covered && (!partial || suffix_covered(data, end, cx, sat));
    // No charges (an unlimited reshape budget) do not prove that measuring did
    // no work, so retain any result that shaped an edge or a suffix. A trivial
    // result (no windows, valid, no suffix, no effects) is cheaper to recompute.
    let trivial = !edge_windows && covered && !partial;
    if let Some(effects) = super::replay::finish(cx, recording, sat)
        && !(trivial && effects.is_empty())
    {
        cx.edge_shapes.remember_measurement(
            (start, end),
            CandidateMeasurement {
                delta,
                valid,
                effects,
            },
        );
    }
    (delta, valid)
}

fn has_partial_remainder(data: &ParagraphData, start: usize, end: usize) -> bool {
    last(data, start, end).is_some_and(|at| group(data, at).end > end)
}

fn suffix_covered(
    data: &ParagraphData,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> bool {
    let windows = measure_edit::<MeasuredWindow>(data, end, data.units.len(), None, cx, sat);
    covers_partial_edges(data, end, data.units.len(), windows.iter().map(|w| &w.text))
}

fn remaining_valid(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> bool {
    !has_partial_remainder(data, start, end) || suffix_covered(data, end, cx, sat)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod edge_shape_cache_tests;

#[cfg(test)]
mod candidate_tests;
