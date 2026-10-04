//! Per-operation reuse of adjustment-only ruby candidates.
//!
//! Fit probes ask for `start..end` with a growing `end`, and the look-ahead
//! extends every such probe to the same paired endpoint `through` until the
//! scan crosses it. The container core depends only on `start..through`
//! (plus the dataset and atomic revision), so it is measured once per
//! operation and replayed while its recorded side effects replay exactly
//! (`line::replay`). Entries live until the operation's reshape budget is
//! reset (`LayoutContext::begin_reshape_operation`) or the context shrinks.
//! Only look-ahead probes (`through > end`) are stored, at most `MAX_ENTRIES`
//! at once, and a clear releases a map larger than `RETAINED_CAPACITY`.
//!
//! # Validity
//!
//! An entry stands in for a fresh measurement iff every input of the core's
//! result and side effects is either part of the key, checked by the replay
//! gate, or unchanged since recording:
//! - key: dataset identity (id and address), atomic revision, `start`,
//!   `through`;
//! - gate (`line::replay::replay`): `edge_reshape_spent` against the recorded
//!   charge aggregate, and whether the warning sink is suppressed;
//! - cache state: some `RangeCache` hits skip side effects that a cold fill
//!   incurs (a `blocks` hit skips the block's reshape charges, a `sets` hit
//!   the build's saturation). A measurement that filled such a cache is not
//!   stored, and an entry replays only while `RangeCache::generation` equals
//!   the one it was recorded under, so it always replays the effects of a
//!   measurement against the same warm cache state. The edge window cache
//!   charges on hits as on misses and stores only clean windows, and the
//!   metric and neighbor index slots are side-effect free;
//! - everything else the core reads (prepared ruby data) is immutable for the
//!   operation. Nothing on the measurement path reads the warning count, and
//!   an entry exists only for a measurement that pushed no warning at all.
//!
//! This also covers the rollback of a speculative `PartialLine::index`
//! (`line/cache.rs`): a failed index restores warnings and saturation but not
//! `edge_reshape_spent`, the reshape log or this memo. Entries recorded during
//! that pass remain valid:
//! - saturation is stored as a delta and added to whatever the caller holds,
//!   exactly as measuring again would;
//! - the restored sink may be unsuppressed again; entries recorded while it
//!   was suppressed carry that state and the gate refuses them;
//! - `edge_reshape_spent` is not restored on either path. Replays charge the
//!   same aggregate bytes as a fresh measurement, so the reference and the
//!   memoized path enter the retry with equal `spent`, and the gate decides
//!   reuse against that value;
//! - no recording encloses `PartialLine::index`, so the reshape log has no
//!   open frame across the rollback (and `finish` folds any stray frame).
use crate::geometry::LayoutUnit;
use crate::line::replay::Effects;
use crate::paragraph::{AtomicSizes, ParagraphData};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MemoKey {
    id: u64,
    data: usize,
    revision: u64,
    start: usize,
    through: usize,
}

impl MemoKey {
    pub(crate) fn new(
        data: &ParagraphData,
        atomics: &AtomicSizes,
        start: usize,
        through: usize,
    ) -> Self {
        Self {
            id: data.id,
            data: data as *const ParagraphData as usize,
            revision: atomics.revision,
            start,
            through,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MemoEntry {
    /// Sum of the container adjustments, before the look-ahead correction.
    pub(crate) adjustment: LayoutUnit,
    pub(crate) effects: Effects,
    /// `RangeCache::generation` during the whole recording. The entry
    /// replays only while it is unchanged: a recording that filled a cache
    /// whose later hits skip side effects is never stored.
    pub(crate) generation: u64,
}

/// Entries held at once. Reaching it clears the memo before the next insert.
/// A fit scan asks for one `through` over a contiguous run of growing ends,
/// so clearing loses at most the rest of runs already left behind, never the
/// current one, and the look-ahead reuse of a scan stays linear. Repeat scans
/// in one operation (a `PartialLine::index` after a line scan) replay older
/// entries only while they fit: 1024 is twice the default
/// `max_nesting_depth`, so a maximally nested chain's look-ahead endpoints
/// from one scan all fit. A bucket takes 105 bytes (key 40, entry 64, one
/// control byte) and 1024 entries need 2048 buckets: about 210 KiB at most.
pub(crate) const MAX_ENTRIES: usize = 1024;

/// Capacity kept across operations. A larger map (an operation with many
/// look-ahead endpoints, e.g. a long line of sibling rubies) is freed when the
/// memo is cleared, so it does not stay pinned for the context's lifetime;
/// ordinary lines (tens of endpoints) keep reusing their allocation. The
/// largest retained map has 256 buckets (224 usable), about 27 KiB.
pub(crate) const RETAINED_CAPACITY: usize = 256;

#[derive(Debug, Default)]
pub(crate) struct RubyMemo {
    entries: crate::hashing::FastMap<MemoKey, MemoEntry>,
    /// Look-ahead walk of the latest probe start (see `advance`).
    walk: Option<WalkState>,
    /// Clears forced by `MAX_ENTRIES`.
    #[cfg(test)]
    pub(crate) overflow_clears: usize,
}

impl RubyMemo {
    pub(crate) fn get(&self, key: &MemoKey) -> Option<MemoEntry> {
        self.entries.get(key).copied()
    }

    pub(crate) fn insert(&mut self, key: MemoKey, entry: MemoEntry) {
        if self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(&key) {
            // Dropping entries only means measuring again, which is exactly
            // what the reference path does.
            self.entries.clear();
            #[cfg(test)]
            {
                self.overflow_clears += 1;
            }
        }
        self.entries.insert(key, entry);
    }

    pub(crate) fn remove(&mut self, key: &MemoKey) {
        self.entries.remove(key);
    }

    /// Move the walk out while the core is measured; an unwound measurement
    /// leaves `None`, which restarts from a full walk. A nested candidate
    /// measured meanwhile finds `None` too and walks in full.
    pub(crate) fn take_walk(&mut self) -> Option<WalkState> {
        self.walk.take()
    }

    pub(crate) fn put_walk(&mut self, walk: Option<WalkState>) {
        self.walk = walk;
    }

    /// Forget every entry and the walk, releasing a map that grew beyond
    /// `RETAINED_CAPACITY` (the walk's vectors are dropped with it).
    pub(crate) fn clear(&mut self) {
        if self.entries.capacity() > RETAINED_CAPACITY {
            self.entries = Default::default();
        } else {
            self.entries.clear();
        }
        self.walk = None;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(through: usize) -> MemoKey {
        MemoKey {
            id: 1,
            data: 2,
            revision: 3,
            start: 0,
            through,
        }
    }

    fn entry() -> MemoEntry {
        let mut cx = crate::LayoutContext::new();
        let sat = crate::geometry::Saturation::default();
        let recording = crate::line::replay::begin(&mut cx, &sat);
        MemoEntry {
            adjustment: LayoutUnit::ZERO,
            effects: crate::line::replay::finish(&mut cx, recording, &sat).unwrap(),
            generation: 0,
        }
    }

    #[test]
    fn entries_are_capped_and_large_maps_are_released() {
        let mut memo = RubyMemo::default();
        for through in 0..3 * MAX_ENTRIES {
            memo.insert(key(through), entry());
            assert!(memo.len() <= MAX_ENTRIES, "{through}");
            // The latest key always survives a forced clear.
            assert!(memo.get(&key(through)).is_some());
        }
        assert_eq!(memo.overflow_clears, 2);
        // Re-inserting a present key never clears.
        memo.insert(key(3 * MAX_ENTRIES - 1), entry());
        assert_eq!(memo.overflow_clears, 2);
        assert!(memo.capacity() > RETAINED_CAPACITY);
        memo.clear();
        assert_eq!((memo.len(), memo.capacity()), (0, 0));
        // A small map keeps its allocation across operations.
        for through in 0..64 {
            memo.insert(key(through), entry());
        }
        let capacity = memo.capacity();
        assert!(capacity <= RETAINED_CAPACITY);
        memo.clear();
        assert_eq!((memo.len(), memo.capacity()), (0, capacity));
    }
}

/// Look-ahead walk for one `start` with a non-decreasing `end`.
#[derive(Debug)]
pub(crate) struct WalkState {
    /// Dataset identity (id and address), as in `MemoKey`.
    owner: (u64, usize),
    start: usize,
    end: usize,
    /// Containers in the full walk's visit order.
    visited: Vec<usize>,
    /// Positions in `visited` whose look-ahead was clipped by the bound.
    clipped: Vec<usize>,
    /// First container index a resumed walk may still visit.
    resume: usize,
}

impl WalkState {
    pub(crate) fn visited(&self) -> &[usize] {
        &self.visited
    }
}

/// Bound from which `extend` is the identity for `ruby`: below it the
/// container stays clipped and is re-applied. Point 3 of `advance` puts every
/// cut in `units`, making this `units.end`; taking the last cut into account
/// as well keeps the walk exact even if a cut were ever placed past it.
fn settled_from(ruby: &super::prepare::PreparedRuby) -> usize {
    let last = ruby.cuts[ruby.cuts.len() - 1].unit;
    debug_assert!(
        last <= ruby.units.end,
        "cut {last} after container end {}",
        ruby.units.end
    );
    ruby.units.end.max(last)
}

/// One look-ahead step of `measure::walk`: extend the bound `t` to the next
/// paired cut of `ruby` at or after `min(t, units.end)`.
fn extend(ruby: &super::prepare::PreparedRuby, through: usize) -> usize {
    let cut = super::measure::cut_at_or_after(ruby, through.min(ruby.units.end));
    through.max(ruby.cuts[cut].unit)
}

/// The full walk's `through` for `start..end`, leaving `visited` equal to the
/// full walk's visit order (`measure::walk`). A different dataset or start, or
/// a smaller end, restarts from an empty state.
///
/// # Why this reproduces the full walk exactly
///
/// Write `f_i(t) = max(t, cuts_i[cut_at_or_after(min(t, e_i))].unit)` for
/// container `i` with source units `s_i..e_i`.
///
/// 1. Start-ordered preorder. `ContainerIndex::intersecting` is a left-to-right
///    fold over containers sorted by `s_i`: a container with `e_i <= start`
///    is skipped (the `ends[node] <= start` prune does not stop the walk),
///    and the first container with `s_i >= t` (the current bound) stops it,
///    because every later container starts there too and `t` changes only
///    when a container is visited. So the full walk for `end` visits exactly
///    `V(end) = { i < J : e_i > start }` in index order, where `J` is the
///    first index whose `s_J` reaches the bound accumulated so far, and
///    `through = f_{v_k}(...f_{v_1}(end))`.
/// 2. Monotonicity in `through`. `min(t, e_i)` is monotone, `cut_at_or_after`
///    is a monotone partition point over cuts sorted by unit, and `max(t, .)`
///    keeps `f_i(t) >= t`. By induction over the visit order, a larger `end`
///    gives every container of `V(end)` a bound at least as large as before.
///    Hence each container of `V(end)` is still visited (its `s_i` stays below
///    a bound that only grew), and `J` can only move right: `V(end')` is
///    `V(end)` followed by containers with indices `> max V(end)`. Indices
///    between `max V(end)` and the old `J` that were skipped had `e_i <= start`
///    and stay skipped, so resuming `intersecting_from` at `max V(end) + 1`
///    with the bound reached after the prefix yields the full walk's suffix
///    (its stop test, applied to the first leaf `>= from` of each subtree,
///    only prunes indices at which the full walk stops as well).
/// 3. Fully contained containers are no-ops. Every paired cut of a container
///    lies in `s_i..=e_i`: the base cuts come from `Index::safe(ruby.units)`
///    and `Index::bases` moves a cut only to a base end or `range.end`
///    (`index.rs`), and source-matched first-line cuts are a subset of them,
///    possibly without the container's own end cut. So when `t >= e_i`, the
///    selected cut unit is `<= e_i <= t` and `f_i(t) = t`, for this and every
///    larger bound. A container visited with `t >= e_i` therefore never moves
///    the bound again for this start; only the clipped ones (`t < e_i`) are
///    re-applied, in visit order, and a re-applied container whose bound
///    reaches `e_i` leaves the clipped list for good.
///
///    The walk does not rely on this placement: it treats a container as
///    clipped while `t < max(e_i, c_i)`, where `c_i` is its last cut unit
///    (`settled_from`). For `t >= max(e_i, c_i)` the selected cut is at most
///    `c_i <= t`, so `f_i(t) = t` holds for any cut placement, and the debug
///    assertion `c_i <= e_i` only documents the expected invariant.
///
/// Like `measure::walk`, this assumes every container has at least one cut.
pub(crate) fn advance(
    state: &mut Option<WalkState>,
    data: &ParagraphData,
    start: usize,
    end: usize,
) -> usize {
    let owner = (data.id, data as *const ParagraphData as usize);
    let containers = &data.ruby.containers;
    if !state
        .as_ref()
        .is_some_and(|s| s.owner == owner && s.start == start && s.end <= end)
    {
        *state = Some(WalkState {
            owner,
            start,
            end,
            visited: Vec::new(),
            clipped: Vec::new(),
            resume: 0,
        });
    }
    let WalkState {
        end: last_end,
        visited,
        clipped,
        resume,
        ..
    } = state.as_mut().expect("walk state was just ensured");
    let mut through = end;
    // Re-apply the clipped prefix containers; the unclipped ones are
    // identities for every bound at least as large as before (point 3).
    let mut kept = 0;
    for i in 0..clipped.len() {
        super::index::visit();
        let position = clipped[i];
        let ruby = &containers[visited[position]];
        if through < settled_from(ruby) {
            clipped[kept] = position;
            kept += 1;
        }
        through = extend(ruby, through);
    }
    clipped.truncate(kept);
    data.ruby.intervals.intersecting_from(
        containers,
        start,
        *resume,
        &mut through,
        |container, through| {
            let ruby = &containers[container];
            if *through < settled_from(ruby) {
                clipped.push(visited.len());
            }
            *through = extend(ruby, *through);
            visited.push(container);
            *resume = container + 1;
        },
    );
    *last_end = end;
    through
}
