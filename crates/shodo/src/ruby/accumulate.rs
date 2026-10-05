//! Container-level differential accumulator for adjustment-only ruby
//! candidates (shodo-2j6).
//!
//! A fit scan asks for `start..through` with a growing `through`. The
//! through memo (`super::memo`) replays a core only for a repeated `through`;
//! sibling rubies move `through` at every ruby, and measuring every
//! intersecting container again made one line quadratic. An accumulator
//! keeps, for one `start`, the latest measurement of every visited
//! container ("position" = index in the walk's visit order) and measures
//! live only the dirty positions; every clean run between them is replayed
//! as one aggregate taken from an ordered segment tree.
//!
//! # Lifetime and memory
//!
//! Accumulators live in `RubyMemo` and are reset with it at every
//! `begin_reshape_operation` (including the mid-operation reset by
//! `Paragraph::ruby_line`, which runs after every fit probe of the line) and
//! dropped by `shrink_to`. At most `MAX_ACCUMULATORS` exist (the intrinsic
//! scan alternates two starts); each holds at most `MAX_CONTAINERS`
//! positions — a larger walk uses the through memo path alone. A reset
//! releases vectors that grew beyond `RETAINED_CONTAINERS`.
//!
//! # What a container measurement depends on
//!
//! `measure::measure_one` reads: its clipped units; the completed
//! descendants (adjustment, whole area, content flag of every position that
//! starts in a base or in the units and ends by its end); the step's
//! selected line profile — height and above (for top/bottom groups), the
//! partially selected groups, the edge-window removals and replacements;
//! the visual neighbours of its target among the selected event units; and
//! the range caches. Prepared ruby data is immutable for the operation.
//!
//! # Dirty rules
//!
//! - D1 new: positions visited for the first time.
//! - D2 clipped: positions whose clipped units changed.
//! - D3 neighbour: for every newly selected event unit `x` and each side
//!   `s`, the neighbour-dependent positions that read side `s` and whose
//!   recorded side-`s` neighbour (`None` at a line edge included) equals
//!   side `s` of `overhang::visual_neighbours(selected, x)`. See "Why D3 is
//!   sufficient" below.
//! - D4 profile: profile-dependent positions (`ContainerNote::profile`)
//!   when the profile's height or above changed.
//! - D5 edge: positions whose units or recorded neighbour units intersect a
//!   partial group, removal or replacement that differs from the previous
//!   step (`SelectionDigest::changed_ranges`). See "Why D4 and D5 are
//!   sufficient" below.
//! - D6 ancestor: ancestors of every dirty position, and of every position
//!   whose live measurement changed its values.
//!
//! Positions without a replayable entry ("unstored") are always dirty.
//!
//! # Why D3 is sufficient
//!
//! An allowance records `around(selected, target)` (`ContainerNote::
//! neighbours`): the last event unit visually before the target's visually
//! first unit and the first event unit visually after its visually last
//! unit. That edge unit may be a non-event (an `Open`/`Close` without
//! inline-axis edges, say), so the recorded neighbours are not the visual
//! neighbours of any single unit (`visual_neighbours`), and "the containers
//! holding a neighbour of a new event" can miss a changed target.
//!
//! What holds instead, for a target whose clipped units did not change (a
//! changed target is D2 dirty) and a fixed `start`: with fixed levels, UAX #9
//! swaps two units iff the minimum level between them is odd, for every
//! range holding both, so a longer selection only inserts the new units into
//! the old visual order. If the target's side-`s` neighbour changed from
//! `b0`, an event unit was inserted between `b0` (or the line edge) and the
//! target's edge unit, and it is a new one: an old event there would have
//! been `b0`. Of those inserted events, the one closest to `b0` has only new
//! events and non-events between itself and `b0`, so its own side-`s`
//! neighbour is `b0` (`None` when `b0` is `None`). Hence a changed position
//! recorded a side-`s` neighbour equal to side `s` of some new event's
//! visual neighbours, which is what D3 looks up in `Accumulator::neighbours`.
//! Neither an event inside the target nor a visually contiguous target is
//! needed (brute-forced in `ruby::accumulate_tests`). Both queries use the
//! same index and the same `rtl` (the base level), so their sides agree.
//!
//! Only the sides an allowance read are indexed. That is sound because which
//! sides a position reads does not depend on the neighbours or the profile:
//! `overhang::allowances` decides `leading_side`/`trailing_side` from
//! `columns.edges(target)` and the clipped bases and columns alone, and
//! whether it runs at all (`lane_overhang`: overhang `Auto`, a positive
//! excess, a nonzero cap) depends on range-cache widths of the clipped
//! units and on the completed descendants' adjustments. A position none of
//! whose clipped units or descendants changed (D2, D6) therefore reads the
//! same sides as recorded, so a side it never read cannot start mattering
//! without the position being dirty for another reason.
//!
//! The rule is applied conservatively: an entry whose allowances read one
//! side twice with different neighbours (`ContainerNote::mixed`) is not
//! stored, so it is measured live at every step. Changes of the neighbours'
//! own geometry follow the profile and are covered by D4 and D5.
//!
//! # Why D4 and D5 are sufficient
//!
//! Every read of the selected profile goes through `content_shared`, for the
//! clipped bases (and the column or ruby boxes) of the container, and for
//! the single unit and enclosing boxes of every neighbour an allowance
//! measures (`overhang::neighbor_bounds`), all under the position's
//! detached share, so one `ContainerNote` covers them all. Of the profile it
//! reads only what `SelectionDigest` holds:
//! - height and above, only through top/bottom groups: shifted group content
//!   summaries of a range, `group_delta` of a partial group's raw content,
//!   and `box_delta` of a grouped box (column, ruby or neighbour box, or an
//!   ancestor whose paint is added). Each of these sets
//!   `ContainerNote::profile` (`grouped` prefix count, box groups, grouped
//!   ancestors), so D4 marks every position whose values can follow them
//!   (a partial group's raw content counts because every unit with content
//!   inside a group's span belongs to the group, so `grouped` sees it);
//! - the partial groups, removals and replacements, only where they
//!   intersect a queried range (`content_query` visits no other unit), or
//!   through the partial bounds of the group of a box. A queried range lies
//!   in the container's units or is a recorded neighbour unit, and a grouped
//!   box holds a unit of the container (column and ruby boxes, ancestors of
//!   a base edge) or the neighbour unit (neighbour boxes), so its group's
//!   units (contiguous, and holding every unit of the box) intersect the
//!   container's units or that neighbour unit. D5 marks every position
//!   whose (unclipped, so possibly more) units or indexed neighbour units
//!   intersect a range that differs between the digests; a neighbour is
//!   measured only on a read side, and read sides are indexed.
//!
//! Hence a position that D1-D5 leave clean reads the same profile values as
//! when it was recorded; D6 adds the ancestors.
//!
//! # Replay
//!
//! An entry keeps the container's own effects without the shared profile
//! (a detached `ProfileShare` routes the profile's charges to the enclosing
//! frame) and the number of profile calls. A clean run replays
//! `own ⊕ calls × P`, where `P` is this step's profile, measured live by the
//! top position exactly where the reference measures it, through the exact
//! `line::replay` gate. Its adjustments are added in reverse structural order;
//! the tree's prefix extremes prove that no addition saturates, otherwise the
//! values are added one by one with `LayoutUnit::add`.
//!
//! # Cache state
//!
//! An entry is stored only when its recording moved neither
//! `RangeCache::fills` nor `RangeCache::epoch`: every cache query it made was
//! a hit, and stays one until the epoch moves. An accumulator is reset when
//! the epoch differs from the one its entries were recorded under.
//!
//! # Speculative index rollback
//!
//! A failed `PartialLine::index` (`line/cache.rs`) restores warnings and
//! Saturation but not `edge_reshape_spent`, the reshape log or `RubyMemo`, so
//! accumulators recorded during that pass survive into the retry. Their
//! entries stay valid, for the reasons the through memo's do (`super::memo`):
//! - an entry exists only for a measurement that pushed no warning; its
//!   effects carry the sink's suppression state, so after the sink is
//!   restored to unsuppressed the replay gate refuses entries recorded while
//!   it was suppressed (they are measured live, as the reference does);
//! - Saturation is stored as a delta (`own`, the profile's `P`) and added to
//!   whatever the caller holds, as measuring again would add it;
//! - `edge_reshape_spent` is restored on neither path, so the reference and
//!   the accumulator enter the retry with equal `spent`; every replay is
//!   decided against that value by the exact gate;
//! - the classification state (`through`, digest, clipped units) describes
//!   the recorded measurements, not the warning sink, and the cache state is
//!   covered by the epoch check, which the rollback does not touch;
//! - no recording encloses `PartialLine::index`, so no frame of the reshape
//!   log is open across the rollback.

use super::geometry::Bounds;
use super::measure::Descendants;
use crate::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::line::metric_index::{ProfileShare, SelectionDigest};
use crate::line::replay::Effects;
use crate::paragraph::{AtomicSizes, ParagraphData};
use std::collections::BTreeSet;
use std::ops::Range;

/// Positions one accumulator holds at most.
pub(crate) const MAX_CONTAINERS: usize = 16_384;
/// Accumulators kept at once (least recently used evicted).
pub(crate) const MAX_ACCUMULATORS: usize = 2;
/// Capacity kept across resets; larger vectors are released.
pub(crate) const RETAINED_CONTAINERS: usize = 256;
/// Heap bound per position, including `Vec` doubling slack.
#[cfg(test)]
pub(crate) const BYTES_PER_CONTAINER: usize = 640;
/// Neighbour key of a side read at a line edge (no neighbour).
pub(crate) const NO_NEIGHBOUR: u32 = u32::MAX;

/// Key of a recorded or queried neighbour in `Accumulator::neighbours`.
pub(crate) fn neighbour_key(unit: Option<u32>) -> u32 {
    unit.unwrap_or(NO_NEIGHBOUR)
}

/// Identity of an accumulator: dataset (id and address), atomic revision and
/// the probes' fixed `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AccumulatorKey {
    id: u64,
    data: usize,
    revision: u64,
    start: usize,
}

impl AccumulatorKey {
    pub(crate) fn new(data: &ParagraphData, atomics: &AtomicSizes, start: usize) -> Self {
        Self {
            id: data.id,
            data: data as *const ParagraphData as usize,
            revision: atomics.revision,
            start,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(start: usize) -> Self {
        Self {
            id: 1,
            data: 2,
            revision: 3,
            start,
        }
    }
}

/// Why a position is measured live (test histogram `cx.ruby_dirty`).
/// Index 7 of the histogram is unused (it counted a removed fallback that
/// marked every position).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dirty {
    Unstored = 0,
    New = 1,
    Clipped = 2,
    Neighbour = 3,
    Profile = 4,
    Edge = 5,
    Ancestor = 6,
}

/// Running sums of a sequence of adjustments: the total and the extremes
/// over its nonempty prefixes, in `i64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Prefix {
    len: u32,
    sum: i64,
    min: i64,
    max: i64,
}

impl Prefix {
    pub(crate) const EMPTY: Self = Self {
        len: 0,
        sum: 0,
        min: 0,
        max: 0,
    };

    pub(crate) fn leaf(value: LayoutUnit) -> Self {
        let v = i64::from(value.raw());
        Self {
            len: 1,
            sum: v,
            min: v,
            max: v,
        }
    }

    /// `self` followed by `next`.
    pub(crate) fn then(self, next: Self) -> Self {
        if self.len == 0 {
            return next;
        }
        if next.len == 0 {
            return self;
        }
        Self {
            len: self.len + next.len,
            sum: self.sum + next.sum,
            min: self.min.min(self.sum + next.min),
            max: self.max.max(self.sum + next.max),
        }
    }

    /// Whether adding the values to `start` one by one with
    /// `LayoutUnit::add` never saturates: every running sum is an `i32`.
    pub(crate) fn fits(self, start: LayoutUnit) -> bool {
        let s = i64::from(start.raw());
        self.len == 0
            || (s + self.min >= i64::from(i32::MIN) && s + self.max <= i64::from(i32::MAX))
    }

    /// The sequential sum from `start`; exact only when `fits(start)`.
    pub(crate) fn total(self, start: LayoutUnit) -> LayoutUnit {
        LayoutUnit::from_raw((i64::from(start.raw()) + self.sum) as i32)
    }
}

/// Aggregate own effects of a run: replayable only if every position has an
/// entry and all ran under the same suppression state.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Replay {
    Empty,
    Effects(Effects),
    Refused,
}

impl Replay {
    pub(crate) fn then(self, other: Self) -> Self {
        match (self, other) {
            (Self::Empty, x) | (x, Self::Empty) => x,
            (Self::Effects(a), Self::Effects(b)) => a.then(b).map_or(Self::Refused, Self::Effects),
            _ => Self::Refused,
        }
    }
}

/// Segment tree summary of a run of positions.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    /// Adjustments in structural order (how an ancestor adds descendants).
    pub(crate) forward: Prefix,
    /// Adjustments in reverse structural order (how a candidate adds them).
    pub(crate) reverse: Prefix,
    pub(crate) area: Option<Bounds>,
    pub(crate) content: bool,
    /// Largest clipped end of a present position.
    pub(crate) max_end: usize,
    pub(crate) effects: Replay,
    /// Profile calls of the run.
    pub(crate) calls: u64,
}

impl Node {
    pub(crate) const EMPTY: Self = Self {
        forward: Prefix::EMPTY,
        reverse: Prefix::EMPTY,
        area: None,
        content: false,
        max_end: 0,
        effects: Replay::Empty,
        calls: 0,
    };

    /// `self` covers the positions right before `next`.
    pub(crate) fn then(self, next: Self) -> Self {
        Self {
            forward: self.forward.then(next.forward),
            reverse: next.reverse.then(self.reverse),
            area: match (self.area, next.area) {
                (Some(a), Some(b)) => Some(a.union(b)),
                (a, None) | (None, a) => a,
            },
            content: self.content || next.content,
            max_end: self.max_end.max(next.max_end),
            effects: self.effects.then(next.effects),
            calls: self.calls + next.calls,
        }
    }
}

/// Latest measurement of one position.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    /// Clipped source units.
    pub(crate) units: Range<usize>,
    pub(crate) adjustment: LayoutUnit,
    pub(crate) whole_area: Bounds,
    pub(crate) has_content: bool,
    /// False when the container did not intersect the selection (never the
    /// case for a visited container) or before its first measurement.
    pub(crate) present: bool,
    /// Own effects without the shared profile, if replayable.
    pub(crate) own: Option<Effects>,
    /// Profile calls (`content_shared`), each replaying the step's profile.
    pub(crate) calls: u32,
    /// Nearest enclosing visited position.
    pub(crate) parent: Option<u32>,
    /// Recorded allowance neighbours `(before, after)` (`around` over the
    /// clipped target); a slot is meaningful only where `read` is set.
    pub(crate) neighbours: [Option<u32>; 2],
    /// Sides whose neighbour an allowance read.
    pub(crate) read: [bool; 2],
    pub(crate) neighbour_dependent: bool,
    /// Read a top/bottom group (depends on the profile's height and above).
    pub(crate) profile: bool,
}

impl Entry {
    pub(crate) fn placeholder(parent: Option<u32>) -> Self {
        Self {
            units: 0..0,
            adjustment: LayoutUnit::ZERO,
            whole_area: Bounds {
                top: LayoutUnit::ZERO,
                bottom: LayoutUnit::ZERO,
            },
            has_content: false,
            present: false,
            own: None,
            calls: 0,
            parent,
            neighbours: [None, None],
            read: [false; 2],
            neighbour_dependent: false,
            profile: false,
        }
    }

    /// Same values as an ancestor reads them.
    pub(crate) fn same_values(&self, other: &Self) -> bool {
        self.present == other.present
            && self.units == other.units
            && self.adjustment == other.adjustment
            && self.whole_area.top == other.whole_area.top
            && self.whole_area.bottom == other.whole_area.bottom
            && self.has_content == other.has_content
    }

    /// Same measurement, including its effects and dependencies.
    #[cfg(test)]
    pub(crate) fn same_entry(&self, other: &Self) -> bool {
        self.same_values(other)
            && self.own == other.own
            && self.calls == other.calls
            && self.neighbours == other.neighbours
            && self.read == other.read
            && self.neighbour_dependent == other.neighbour_dependent
            && self.profile == other.profile
    }

    pub(crate) fn leaf(&self) -> Node {
        let effects = self.own.map_or(Replay::Refused, Replay::Effects);
        let calls = u64::from(self.calls);
        if !self.present {
            return Node {
                effects,
                calls,
                ..Node::EMPTY
            };
        }
        Node {
            forward: Prefix::leaf(self.adjustment),
            reverse: Prefix::leaf(self.adjustment),
            area: Some(self.whole_area),
            content: self.has_content,
            max_end: self.units.end,
            effects,
            calls,
        }
    }
}

/// Per-start state; see the module documentation.
#[derive(Debug, Default)]
pub(crate) struct Accumulator {
    key: Option<AccumulatorKey>,
    /// `through` of the latest step (0 after a reset).
    pub(crate) through: usize,
    /// `RangeCache::epoch` every stored entry was recorded under.
    pub(crate) epoch: u64,
    pub(crate) entries: Vec<Entry>,
    /// Internal nodes of a tree with `nodes.len()` (a power of two) leaves;
    /// leaf `i` is `entries[i].leaf()`.
    nodes: Vec<Node>,
    /// Profile digest of the latest step.
    pub(crate) digest: Option<SelectionDigest>,
    /// Positions whose clipped units end before their container.
    pub(crate) clipped: BTreeSet<u32>,
    /// Positions without a replayable entry.
    pub(crate) unstored: BTreeSet<u32>,
    /// Profile-dependent positions.
    pub(crate) profile: BTreeSet<u32>,
    /// `(side, neighbour key, position)` of every side a neighbour-dependent
    /// position read; the key is the unit, or `NO_NEIGHBOUR` at a line edge.
    pub(crate) neighbours: BTreeSet<(u8, u32, u32)>,
    /// Number of neighbour-dependent positions.
    pub(crate) dependents: usize,
    /// Enclosing chain of the latest appended position.
    open: Vec<u32>,
}

impl Accumulator {
    pub(crate) fn key(&self) -> Option<AccumulatorKey> {
        self.key
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Forget every position, releasing vectors beyond the retained size.
    pub(crate) fn reset(&mut self, key: Option<AccumulatorKey>, epoch: u64) {
        self.key = key;
        self.through = 0;
        self.epoch = epoch;
        self.digest = None;
        if self.entries.capacity() > RETAINED_CONTAINERS
            || self.nodes.capacity() > 2 * RETAINED_CONTAINERS
        {
            self.entries = Vec::new();
            self.nodes = Vec::new();
            self.open = Vec::new();
        } else {
            self.entries.clear();
            self.nodes.clear();
            self.open.clear();
        }
        self.clipped.clear();
        self.unstored.clear();
        self.profile.clear();
        self.neighbours.clear();
        self.dependents = 0;
    }

    fn cap(&self) -> usize {
        self.nodes.len()
    }

    fn node(&self, i: usize) -> Node {
        let cap = self.cap();
        if i >= cap {
            self.entries.get(i - cap).map_or(Node::EMPTY, Entry::leaf)
        } else {
            self.nodes[i]
        }
    }

    fn rebuild(&mut self) {
        let mut cap = self.cap().max(1);
        while cap < self.entries.len() {
            cap *= 2;
        }
        self.nodes.clear();
        self.nodes.resize(cap, Node::EMPTY);
        for i in (1..cap).rev() {
            self.nodes[i] = self.node(2 * i).then(self.node(2 * i + 1));
        }
    }

    fn update(&mut self, pos: usize) {
        if pos >= self.cap() {
            self.rebuild();
            return;
        }
        let mut i = (pos + self.cap()) / 2;
        while i >= 1 {
            self.nodes[i] = self.node(2 * i).then(self.node(2 * i + 1));
            i /= 2;
        }
    }

    /// Summary of the positions in `range`, in structural order.
    pub(crate) fn query(&self, range: Range<usize>) -> Node {
        let cap = self.cap();
        let (mut l, mut r) = (range.start + cap, range.end + cap);
        let (mut left, mut right) = (Node::EMPTY, Node::EMPTY);
        while l < r {
            if l & 1 == 1 {
                left = left.then(self.node(l));
                l += 1;
            }
            if r & 1 == 1 {
                r -= 1;
                right = self.node(r).then(right);
            }
            l /= 2;
            r /= 2;
        }
        left.then(right)
    }

    /// Append the next visited `container` (an unmeasured, dirty position).
    /// `containers` is the walk's visit order, which holds every position.
    pub(crate) fn push(&mut self, data: &ParagraphData, containers: &[usize], container: usize) {
        let units = &data.ruby.containers[container].units;
        while let Some(&top) = self.open.last() {
            if data.ruby.containers[containers[top as usize]].units.end <= units.start {
                self.open.pop();
            } else {
                break;
            }
        }
        debug_assert!(self.entries.len() < MAX_CONTAINERS, "accumulator cap");
        let pos = self.entries.len() as u32;
        self.entries
            .push(Entry::placeholder(self.open.last().copied()));
        self.open.push(pos);
        self.unstored.insert(pos);
        self.update(pos as usize);
    }

    /// Start a step for `start..through` (the key's start). Returns the
    /// number of positions kept from the previous step; the walk's new
    /// suffix is appended unmeasured.
    fn prepare(
        &mut self,
        key: AccumulatorKey,
        data: &ParagraphData,
        through: usize,
        containers: &[usize],
        epoch: u64,
    ) -> usize {
        // Another dataset, revision or start, a shorter look-ahead, or an
        // invalidated cache: nothing recorded before can be reused. A longer
        // `through` only appends to the walk (`memo::advance`, point 2).
        if self.key != Some(key)
            || through < self.through
            || containers.len() < self.entries.len()
            || epoch != self.epoch
        {
            self.reset(Some(key), epoch);
        }
        let kept = self.entries.len();
        for &container in &containers[kept..] {
            self.push(data, containers, container);
        }
        kept
    }

    /// Replace the entry of `pos`, keeping the indexes and the tree in step.
    /// `full_end` is the container's unclipped end.
    pub(crate) fn store(&mut self, pos: usize, entry: Entry, full_end: usize) {
        let p = pos as u32;
        let old = &self.entries[pos];
        if old.neighbour_dependent {
            self.dependents -= 1;
            for side in 0..2 {
                if old.read[side] {
                    let key = neighbour_key(old.neighbours[side]);
                    self.neighbours.remove(&(side as u8, key, p));
                }
            }
        }
        self.profile.remove(&p);
        self.unstored.remove(&p);
        self.clipped.remove(&p);
        if entry.own.is_none() {
            self.unstored.insert(p);
        }
        if entry.profile {
            self.profile.insert(p);
        }
        if entry.neighbour_dependent {
            self.dependents += 1;
            for side in 0..2 {
                if entry.read[side] {
                    let key = neighbour_key(entry.neighbours[side]);
                    self.neighbours.insert((side as u8, key, p));
                }
            }
        }
        if entry.present && entry.units.end < full_end {
            self.clipped.insert(p);
        }
        self.entries[pos] = entry;
        self.update(pos);
    }

    pub(crate) fn parent(&self, pos: u32) -> Option<u32> {
        self.entries[pos as usize].parent
    }

    /// Innermost visited position whose container holds `unit`. `containers`
    /// is the walk's visit order (sorted by container start, properly
    /// nested), so every container holding `unit` encloses the last visited
    /// one starting at or before it.
    pub(crate) fn owner(
        &self,
        data: &ParagraphData,
        containers: &[usize],
        unit: usize,
    ) -> Option<u32> {
        let visited = &containers[..self.len()];
        let last = visited.partition_point(|c| data.ruby.containers[*c].units.start <= unit);
        let mut pos = last.checked_sub(1).map(|p| p as u32);
        while let Some(p) = pos {
            if data.ruby.containers[containers[p as usize]]
                .units
                .contains(&unit)
            {
                return Some(p);
            }
            pos = self.parent(p);
        }
        None
    }

    #[cfg(test)]
    pub(crate) fn push_raw(&mut self, entry: Entry) {
        debug_assert!(self.entries.len() < MAX_CONTAINERS, "accumulator cap");
        let pos = self.entries.len();
        self.entries.push(Entry::placeholder(None));
        self.unstored.insert(pos as u32);
        self.update(pos);
        self.store(pos, entry, usize::MAX);
    }

    /// Heap held, counting ordered-set elements at twice their size.
    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>()
            + self.nodes.capacity() * size_of::<Node>()
            + self.open.capacity() * size_of::<u32>()
            + (self.clipped.len() + self.unstored.len() + self.profile.len()) * 2 * size_of::<u32>()
            + self.neighbours.len() * 2 * size_of::<(u8, u32, u32)>()
    }
}

fn max_containers(_cx: &LayoutContext) -> usize {
    #[cfg(test)]
    if let Some(cap) = _cx.ruby_accumulate_cap {
        return cap;
    }
    MAX_CONTAINERS
}

/// Step oracle switch (test only).
fn verify(_cx: &LayoutContext) -> bool {
    #[cfg(test)]
    {
        _cx.ruby_accumulate_verify
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Mark `pos` dirty unless it is not below the top position (which every
/// step measures first) or already dirty. Returns whether it was marked.
fn mark(
    dirty: &mut BTreeSet<u32>,
    top: usize,
    pos: u32,
    _reason: Dirty,
    _cx: &mut LayoutContext,
) -> bool {
    if pos as usize >= top || !dirty.insert(pos) {
        return false;
    }
    #[cfg(test)]
    {
        _cx.ruby_dirty[_reason as usize] += 1;
    }
    true
}

/// Sum of the container adjustments of `start..through` (whose walk
/// visited `containers`), with exactly the result and side effects of
/// `measure::measure_containers`, measuring live only the positions whose
/// inputs changed since the latest step for the same start.
pub(crate) fn core(
    data: &ParagraphData,
    start: usize,
    through: usize,
    containers: &[usize],
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    let key = AccumulatorKey::new(data, atomics, start);
    let cap = max_containers(cx);
    if containers.len() > cap {
        // Too many positions to keep: release them and measure as the
        // through memo path does (exact, never approximated).
        cx.ruby_memo.drop_accumulator(key);
    }
    // With one container, the top position is all there is to measure.
    if !cx.accumulate_enabled() || containers.len() < 2 || containers.len() > cap {
        return super::measure::measure_containers(
            data,
            start..through,
            containers,
            atomics,
            cx,
            sat,
        )
        .adjustment;
    }
    let mut accumulator = cx.ruby_memo.take_accumulator(key);
    let kept = accumulator.prepare(key, data, through, containers, cx.ruby_ranges.epoch());
    let mut step = Step {
        data,
        containers,
        start,
        selected: start..through,
        atomics,
        profile: ProfileShare::detached(),
        total: LayoutUnit::ZERO,
        top: containers.len() - 1,
        replays: true,
    };
    step.run(&mut accumulator, kept, cx, sat);
    cx.ruby_memo.put_accumulator(accumulator);
    step.total
}

struct Step<'a> {
    data: &'a ParagraphData,
    containers: &'a [usize],
    start: usize,
    selected: Range<usize>,
    atomics: &'a AtomicSizes,
    /// The step's shared profile, detached from container recordings.
    profile: ProfileShare,
    /// Running sum, added in reverse structural order like the reference.
    total: LayoutUnit,
    /// The last position, measured first.
    top: usize,
    /// Clean runs may still replay: the profile was measured once (by the
    /// top position) and the cache epoch has not moved.
    replays: bool,
}

impl Step<'_> {
    fn run(
        &mut self,
        acc: &mut Accumulator,
        kept: usize,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) {
        let top = self.top;
        // A reset accumulator has no clean position to classify.
        let fresh = acc.digest.is_none();
        let mut dirty = BTreeSet::new();
        // The top position makes the step's first profile call exactly where
        // the reference does; the profile it measures is this step's `P`.
        self.live(acc, top, &mut dirty, cx, sat);
        let digest = self.profile.digest();
        if digest.is_none() {
            self.replays = false;
        }
        for &pos in acc.unstored.range(..top as u32) {
            let reason = if pos as usize >= kept {
                Dirty::New
            } else {
                Dirty::Unstored
            };
            mark(&mut dirty, top, pos, reason, cx);
        }
        if self.replays
            && !fresh
            && let Some(digest) = &digest
        {
            self.classify(acc, digest, &mut dirty, cx);
        }
        // Reverse structural order, as `measure_containers`: the clean run
        // above each dirty position, then the dirty position. Live
        // measurements may mark lower positions (D6), which the next lookup
        // finds.
        let mut hi = top;
        loop {
            let next = dirty.range(..hi as u32).next_back().copied();
            let lo = next.map_or(0, |d| d as usize + 1);
            self.segment(acc, lo..hi, &mut dirty, cx, sat);
            let Some(d) = next else {
                break;
            };
            self.live(acc, d as usize, &mut dirty, cx, sat);
            hi = d as usize;
        }
        if self.replays {
            acc.through = self.selected.end;
            acc.digest = digest;
        } else {
            acc.reset(acc.key, cx.ruby_ranges.epoch());
            #[cfg(test)]
            {
                cx.ruby_accumulator_resets += 1;
            }
        }
    }

    /// D2-D6 for the positions kept from the previous step (see the module
    /// documentation).
    fn classify(
        &self,
        acc: &Accumulator,
        digest: &SelectionDigest,
        dirty: &mut BTreeSet<u32>,
        cx: &mut LayoutContext,
    ) {
        let top = self.top;
        for &pos in acc.clipped.range(..top as u32) {
            let ruby = &self.data.ruby.containers[self.containers[pos as usize]];
            if super::measure::intersect(&self.selected, &ruby.units)
                != acc.entries[pos as usize].units
            {
                mark(dirty, top, pos, Dirty::Clipped, cx);
            }
        }
        self.neighbours(acc, dirty, cx);
        if let Some(old) = acc.digest.as_ref() {
            // D4: every group moves with the profile's height and above.
            if old.profile_changed(digest) {
                for &pos in acc.profile.range(..top as u32) {
                    mark(dirty, top, pos, Dirty::Profile, cx);
                }
            }
            // D5: changed partial groups, edge-window removals and
            // replacements.
            for range in old.changed_ranges(digest) {
                self.edge(acc, &range, dirty, cx);
            }
        }
        self.close(acc, dirty, cx);
    }

    /// D5: positions whose units or recorded neighbour units intersect
    /// `range`. A container's units are compared unclipped, which can only
    /// mark more than needed.
    fn edge(
        &self,
        acc: &Accumulator,
        range: &Range<usize>,
        dirty: &mut BTreeSet<u32>,
        cx: &mut LayoutContext,
    ) {
        if range.is_empty() {
            return;
        }
        let top = self.top;
        // A container intersecting `range` holds `range.start` (the
        // innermost such container and its ancestors) or starts inside it.
        let mut owner = acc.owner(self.data, self.containers, range.start);
        while let Some(q) = owner {
            mark(dirty, top, q, Dirty::Edge, cx);
            owner = acc.parent(q);
        }
        let visited = &self.containers[..acc.len()];
        let start_of = |c: &usize| self.data.ruby.containers[*c].units.start;
        let first = visited.partition_point(|c| start_of(c) <= range.start);
        let last = visited.partition_point(|c| start_of(c) < range.end);
        for pos in first..last {
            mark(dirty, top, pos as u32, Dirty::Edge, cx);
        }
        // Neighbour units are read through `content_shared` too
        // (`overhang::allowances`); only read sides are indexed, and a
        // neighbour is measured only on a read side.
        let lo = u32::try_from(range.start).unwrap_or(u32::MAX);
        let hi = u32::try_from(range.end).unwrap_or(u32::MAX);
        for side in 0..2u8 {
            for &(_, _, pos) in acc.neighbours.range((side, lo, 0)..(side, hi, 0)) {
                mark(dirty, top, pos, Dirty::Edge, cx);
            }
        }
    }

    /// D3: for every newly selected event unit and each side, the positions
    /// that read that side and recorded the unit's neighbour on it (see the
    /// module documentation). `visual_neighbours` answers for the single
    /// unit; the recorded keys are `around` over whole targets.
    fn neighbours(&self, acc: &Accumulator, dirty: &mut BTreeSet<u32>, cx: &mut LayoutContext) {
        if acc.dependents == 0 {
            return;
        }
        for x in acc.through..self.selected.end {
            if !super::overhang::is_event(self.data, x) {
                continue;
            }
            let (before, after) =
                super::overhang::visual_neighbours(self.data, &self.selected, x, cx);
            for (side, unit) in [before, after].into_iter().enumerate() {
                let key = neighbour_key(unit.map(|u| u as u32));
                let side = side as u8;
                for &(_, _, pos) in acc.neighbours.range((side, key, 0)..=(side, key, u32::MAX)) {
                    mark(dirty, self.top, pos, Dirty::Neighbour, cx);
                }
            }
        }
    }

    /// D6: the ancestors of every dirty position.
    fn close(&self, acc: &Accumulator, dirty: &mut BTreeSet<u32>, cx: &mut LayoutContext) {
        let seeds: Vec<u32> = dirty.iter().copied().collect();
        for seed in seeds {
            let mut parent = acc.parent(seed);
            while let Some(q) = parent {
                if !mark(dirty, self.top, q, Dirty::Ancestor, cx) {
                    break;
                }
                parent = acc.parent(q);
            }
        }
    }

    /// Replay the clean positions `run` as one aggregate, or measure them
    /// live in reverse structural order when the gate refuses.
    fn segment(
        &mut self,
        acc: &mut Accumulator,
        run: Range<usize>,
        dirty: &mut BTreeSet<u32>,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) {
        if run.is_empty() {
            return;
        }
        if self.replays && !verify(cx) {
            let node = acc.query(run.clone());
            // Detached profile contract: the run's own effects plus one copy
            // of this step's profile per recorded profile call.
            let combined = match node.effects {
                Replay::Effects(own) => self
                    .profile
                    .effects()
                    .and_then(|profile| own.then(profile.times(node.calls))),
                Replay::Empty | Replay::Refused => None,
            };
            if let Some(effects) = combined
                && crate::line::replay::replay(cx, &effects, sat)
            {
                if node.reverse.fits(self.total) {
                    self.total = node.reverse.total(self.total);
                } else {
                    // Some running sum saturates: add one value at a time in
                    // the reference order, counting every saturation.
                    for pos in run.clone().rev() {
                        let entry = &acc.entries[pos];
                        if entry.present {
                            self.total = self.total.add(entry.adjustment, sat);
                        }
                    }
                    #[cfg(test)]
                    {
                        cx.ruby_sequential_replays += 1;
                    }
                }
                #[cfg(test)]
                {
                    cx.ruby_replayed_containers += run.len();
                    if node.calls > run.len() as u64
                        && self.profile.effects().is_some_and(|p| p.bytes() > 0)
                    {
                        cx.ruby_repeated_profile_replays += 1;
                    }
                }
                return;
            }
        }
        for pos in run.rev() {
            self.clean(acc, pos, dirty, cx, sat);
        }
    }

    /// Measure a clean position live. Under the step oracle, compare the
    /// measurement with its entry whenever the entry would have replayed.
    fn clean(
        &mut self,
        acc: &mut Accumulator,
        pos: usize,
        dirty: &mut BTreeSet<u32>,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) {
        #[cfg(test)]
        let expected = {
            let entry = &acc.entries[pos];
            let would_replay = self.replays
                && cx.ruby_accumulate_verify
                && entry
                    .own
                    .and_then(|own| own.then(self.profile.effects()?.times(u64::from(entry.calls))))
                    .is_some_and(|effects| crate::line::replay::replayable(cx, &effects));
            would_replay.then(|| entry.clone())
        };
        self.live(acc, pos, dirty, cx, sat);
        #[cfg(test)]
        if let Some(expected) = expected
            && self.replays
        {
            cx.ruby_oracle_checks += 1;
            let measured = &acc.entries[pos];
            if !expected.same_entry(measured) {
                cx.ruby_oracle_misses.push(format!(
                    "{:?} position {pos}: entry {expected:?}, measured {measured:?}",
                    self.selected
                ));
            }
        }
    }

    /// Measure `pos` live and store its entry; mark its ancestors when the
    /// values they read changed (D6).
    fn live(
        &mut self,
        acc: &mut Accumulator,
        pos: usize,
        dirty: &mut BTreeSet<u32>,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) {
        let container = self.containers[pos];
        let full_end = self.data.ruby.containers[container].units.end;
        let (fills, epoch) = (cx.ruby_ranges.fills(), cx.ruby_ranges.epoch());
        self.profile
            .begin_container(crate::line::replay::depth(cx).checked_sub(1));
        let recording = crate::line::replay::begin(cx, sat);
        let fragment = {
            let completed = Tree {
                acc: &*acc,
                data: self.data,
                containers: self.containers,
                start: self.start,
                from: pos + 1,
            };
            super::measure::measure_one(
                self.data,
                &self.selected,
                container,
                &completed,
                &mut self.profile,
                self.atomics,
                cx,
                sat,
            )
        };
        let effects = crate::line::replay::finish(cx, recording, sat);
        // An invalidated cache or a profile measured again (a refused replay
        // or a warning) leaves no fixed `P` and no comparable cache state:
        // stop replaying for the rest of the step and reset afterwards.
        if cx.ruby_ranges.epoch() != epoch || self.profile.refreshed() {
            self.replays = false;
        }
        let note = self.profile.note();
        // Stored only if every cache query was a hit (no fill, no
        // invalidation), so measuring again under the same epoch repeats
        // exactly these effects.
        // A side read twice with different neighbours has no single key in
        // the D3 index: such an entry is not stored (always measured live).
        let storable = self.replays && cx.ruby_ranges.fills() == fills && !note.mixed;
        let own = effects
            .filter(|_| storable)
            .map(|e| e.without_sat(note.sat));
        let parent = acc.entries[pos].parent;
        let neighbours = note.neighbours.map(|u| u.map(|u| u as u32));
        let entry = match fragment {
            Some(f) => {
                self.total = self.total.add(f.adjustment, sat);
                Entry {
                    units: f.units,
                    adjustment: f.adjustment,
                    whole_area: f.whole_area,
                    has_content: f.has_content,
                    present: true,
                    own,
                    calls: note.calls,
                    parent,
                    neighbours,
                    read: note.read,
                    neighbour_dependent: note.neighbour_dependent,
                    profile: note.profile,
                }
            }
            None => Entry {
                own,
                calls: note.calls,
                neighbours,
                read: note.read,
                neighbour_dependent: note.neighbour_dependent,
                profile: note.profile,
                ..Entry::placeholder(parent)
            },
        };
        let changed = !acc.entries[pos].same_values(&entry);
        acc.store(pos, entry, full_end);
        if changed {
            let mut parent = acc.parent(pos as u32);
            while let Some(q) = parent {
                if !mark(dirty, self.top, q, Dirty::Ancestor, cx) {
                    break;
                }
                parent = acc.parent(q);
            }
        }
    }
}

/// Completed descendants of a position, answered from the segment tree:
/// the later positions (all already measured or replayed in this step)
/// whose clipped start lies in a range and whose clipped end does not pass
/// it, in structural order. Each read is one range query (counted as one
/// test visit) whenever the tree reproduces the reference loop exactly;
/// otherwise the reference loop runs (one visit per position).
struct Tree<'a> {
    acc: &'a Accumulator,
    data: &'a ParagraphData,
    containers: &'a [usize],
    start: usize,
    from: usize,
}

impl Tree<'_> {
    /// Positions from `from` whose clipped start lies in `range`; clipped
    /// starts grow with the position (containers are sorted by start).
    fn span(&self, range: &Range<usize>) -> Range<usize> {
        let clipped = |c: &usize| self.data.ruby.containers[*c].units.start.max(self.start);
        let len = self.acc.len();
        let from = self.from.min(len);
        let later = &self.containers[from..len];
        let first = later.partition_point(|c| clipped(c) < range.start);
        let last = later.partition_point(|c| clipped(c) < range.end);
        from + first..from + last
    }

    /// The reference loop over `span`, used when the tree cannot prove that
    /// every position passes the end filter or that no addition saturates.
    fn add_each(
        &self,
        span: Range<usize>,
        range: &Range<usize>,
        mut width: LayoutUnit,
        _cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        for pos in span {
            let entry = &self.acc.entries[pos];
            #[cfg(test)]
            {
                _cx.ruby_measure_visits += 1;
            }
            if entry.present && entry.units.end <= range.end {
                width = width.add(entry.adjustment, sat);
            }
        }
        width
    }
}

impl Descendants for Tree<'_> {
    /// Exact from the tree when every present position of the span ends by
    /// `range.end` (so `forward` is the reference's sequence) and no running
    /// sum leaves `i32` (so `LayoutUnit::add` never saturates and the sum is
    /// the plain total); otherwise the reference loop.
    fn add_adjustments(
        &self,
        range: &Range<usize>,
        width: LayoutUnit,
        _cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        let span = self.span(range);
        let node = self.acc.query(span.clone());
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if node.max_end <= range.end && node.forward.fits(width) {
            return node.forward.total(width);
        }
        self.add_each(span, range, width, _cx, sat)
    }

    /// `Bounds::union` is a min/max, so the tree's union equals the
    /// sequential one in any grouping once every present position passes
    /// the end filter; otherwise the reference loop.
    fn union_areas(&self, range: &Range<usize>, area: Bounds, _cx: &mut LayoutContext) -> Bounds {
        let span = self.span(range);
        let node = self.acc.query(span.clone());
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if node.max_end <= range.end {
            return node.area.map_or(area, |a| area.union(a));
        }
        let mut area = area;
        for pos in span {
            let entry = &self.acc.entries[pos];
            #[cfg(test)]
            {
                _cx.ruby_measure_visits += 1;
            }
            if entry.present && entry.units.end <= range.end {
                area = area.union(entry.whole_area);
            }
        }
        area
    }

    fn any_content(&self, range: &Range<usize>) -> bool {
        let span = self.span(range);
        let node = self.acc.query(span.clone());
        if node.max_end <= range.end {
            return node.content;
        }
        span.map(|pos| &self.acc.entries[pos])
            .any(|e| e.present && e.units.end <= range.end && e.has_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn value(seed: &mut u64) -> LayoutUnit {
        match next(seed) % 6 {
            0 => LayoutUnit::MAX,
            1 => LayoutUnit::MIN,
            2 => LayoutUnit::from_raw((next(seed) % 2000) as i32 - 1000),
            3 => LayoutUnit::from_raw(i32::MAX / 3),
            4 => LayoutUnit::from_raw(i32::MIN / 3),
            _ => LayoutUnit::from_raw(next(seed) as i32),
        }
    }

    fn effects(bytes: u64, suppressed: bool) -> Effects {
        let mut cx = crate::LayoutContext::new();
        if suppressed {
            cx.warnings.set_max(Some(0));
            cx.warnings
                .push(crate::limits::WarningKind::Unsupported, "suppress");
        }
        let sat = Saturation::default();
        let recording = crate::line::replay::begin(&mut cx, &sat);
        cx.edge_reshape_spent = bytes;
        crate::line::replay::record_charge(&mut cx, bytes, u64::MAX, true);
        crate::line::replay::finish(&mut cx, recording, &sat).unwrap()
    }

    fn entry(seed: &mut u64, at: usize) -> Entry {
        let top = LayoutUnit::from_raw((next(seed) % 100) as i32 - 50);
        Entry {
            units: at..at + 1 + (next(seed) % 3) as usize,
            adjustment: value(seed),
            whole_area: Bounds {
                top,
                bottom: top + LayoutUnit::from_raw(10),
            },
            has_content: next(seed).is_multiple_of(2),
            present: !next(seed).is_multiple_of(8),
            own: match next(seed) % 5 {
                0 => None,
                1 => Some(effects(3, true)),
                _ => Some(effects(next(seed) % 50, false)),
            },
            calls: (next(seed) % 4) as u32,
            parent: None,
            neighbours: [None, None],
            read: [false; 2],
            neighbour_dependent: false,
            profile: false,
        }
    }

    /// `fits` is exactly "sequential `LayoutUnit::add` never saturates", and
    /// then `total` is the sequential result; `then` composes prefixes.
    #[test]
    fn prefix_guard_is_exactly_no_sequential_saturation() {
        let mut seed = 0x9e37_79b9_7f4a_7c15;
        let fold = |values: &[LayoutUnit]| {
            values
                .iter()
                .fold(Prefix::EMPTY, |p, v| p.then(Prefix::leaf(*v)))
        };
        for _ in 0..20_000 {
            let len = (next(&mut seed) % 6) as usize;
            let values: Vec<_> = (0..len).map(|_| value(&mut seed)).collect();
            let start = value(&mut seed);
            let split = (next(&mut seed) as usize) % (len + 1);
            let prefix = fold(&values);
            assert_eq!(
                fold(&values[..split]).then(fold(&values[split..])),
                prefix,
                "{values:?}"
            );
            let mut sat = Saturation::default();
            let sequential = values.iter().fold(start, |sum, v| sum.add(*v, &mut sat));
            assert_eq!(
                prefix.fits(start),
                sat.saturated == 0,
                "{start:?} {values:?}"
            );
            if prefix.fits(start) {
                assert_eq!(prefix.total(start), sequential, "{start:?} {values:?}");
            }
        }
    }

    #[test]
    fn tree_queries_match_a_linear_fold() {
        let mut seed = 0x2545_f491_4f6c_dd1d;
        let mut acc = Accumulator::default();
        for len in [1usize, 2, 3, 5, 8, 13, 33] {
            acc.reset(None, 0);
            for at in 0..len {
                acc.push_raw(entry(&mut seed, at));
            }
            for _ in 0..len {
                let pos = (next(&mut seed) as usize) % len;
                let replacement = entry(&mut seed, pos);
                acc.store(pos, replacement, usize::MAX);
            }
            for start in 0..=len {
                for end in start..=len {
                    let linear =
                        (start..end).fold(Node::EMPTY, |n, p| n.then(acc.entries[p].leaf()));
                    assert_eq!(
                        format!("{:?}", acc.query(start..end)),
                        format!("{linear:?}"),
                        "{len}: {start}..{end}"
                    );
                }
            }
        }
    }

    #[test]
    fn store_keeps_the_indexes_in_step() {
        let mut seed = 7;
        let mut acc = Accumulator::default();
        for at in 0..4 {
            acc.push_raw(entry(&mut seed, at));
        }
        let mut e = entry(&mut seed, 1);
        e.present = true;
        e.own = None;
        e.profile = true;
        e.neighbour_dependent = true;
        e.neighbours = [Some(0), None];
        e.read = [true, true];
        let end = e.units.end;
        acc.store(1, e.clone(), end);
        assert!(acc.unstored.contains(&1) && acc.profile.contains(&1));
        assert!(
            acc.neighbours.contains(&(0, 0, 1)) && acc.neighbours.contains(&(1, NO_NEIGHBOUR, 1))
        );
        assert!(!acc.clipped.contains(&1));
        assert_eq!(acc.dependents, 1);
        e.own = Some(effects(1, false));
        e.profile = false;
        e.neighbour_dependent = false;
        acc.store(1, e, end + 1);
        assert!(!acc.unstored.contains(&1) && !acc.profile.contains(&1));
        assert!(!acc.neighbours.iter().any(|(_, _, p)| *p == 1));
        assert!(acc.clipped.contains(&1));
        assert_eq!(acc.dependents, 0);
    }

    #[test]
    fn reset_releases_large_allocations_only() {
        let mut seed = 11;
        let mut acc = Accumulator::default();
        for at in 0..1000 {
            acc.push_raw(entry(&mut seed, at));
        }
        assert!(
            acc.heap_bytes() <= 1000 * BYTES_PER_CONTAINER,
            "{}",
            acc.heap_bytes()
        );
        acc.reset(None, 0);
        assert_eq!(
            (acc.len(), acc.entries.capacity(), acc.nodes.capacity()),
            (0, 0, 0)
        );
        for at in 0..100 {
            acc.push_raw(entry(&mut seed, at));
        }
        let kept = (acc.entries.capacity(), acc.nodes.capacity());
        acc.reset(None, 0);
        assert_eq!((acc.entries.capacity(), acc.nodes.capacity()), kept);
        assert!(
            acc.unstored.is_empty()
                && acc.neighbours.is_empty()
                && acc.profile.is_empty()
                && acc.clipped.is_empty()
        );
    }

    #[test]
    fn replay_aggregates_refuse_mixed_suppression() {
        let a = Replay::Effects(effects(1, false));
        let b = Replay::Effects(effects(2, true));
        assert!(matches!(a.then(Replay::Empty), Replay::Effects(_)));
        assert!(matches!(a.then(b), Replay::Refused));
        assert!(matches!(a.then(Replay::Refused), Replay::Refused));
        assert!(matches!(Replay::Empty.then(Replay::Empty), Replay::Empty));
    }

    /// `Effects::then` is order-free (saturating byte sum, limit min/max,
    /// outcome conjunctions, wrapping Saturation sums), so the tree may
    /// compose replay aggregates in position order without loss.
    #[test]
    fn replay_aggregates_are_order_free() {
        let mut seed = 0x51_7cc1_b727_220a;
        for _ in 0..200 {
            let suppressed = next(&mut seed).is_multiple_of(2);
            let a = Replay::Effects(effects(next(&mut seed) % 50, suppressed));
            let b = Replay::Effects(effects(next(&mut seed) % 50, suppressed));
            assert_eq!(format!("{:?}", a.then(b)), format!("{:?}", b.then(a)));
        }
    }

    /// Independent of `Node::then`: `forward` is the prefix of the present
    /// adjustments in position order and `reverse` the one in reverse
    /// position order (how a candidate adds them), including the guard.
    #[test]
    fn tree_directions_match_independent_folds() {
        let mut seed = 0x0123_4567_89ab_cdef;
        let mut acc = Accumulator::default();
        for len in [1usize, 2, 3, 6, 11, 21] {
            acc.reset(None, 0);
            for at in 0..len {
                acc.push_raw(entry(&mut seed, at));
            }
            for start in 0..=len {
                for end in start..=len {
                    let values: Vec<LayoutUnit> = acc.entries[start..end]
                        .iter()
                        .filter(|e| e.present)
                        .map(|e| e.adjustment)
                        .collect();
                    let fold = |values: &mut dyn Iterator<Item = &LayoutUnit>| {
                        values.fold(Prefix::EMPTY, |p, v| p.then(Prefix::leaf(*v)))
                    };
                    let node = acc.query(start..end);
                    assert_eq!(node.forward, fold(&mut values.iter()), "{start}..{end}");
                    assert_eq!(
                        node.reverse,
                        fold(&mut values.iter().rev()),
                        "{start}..{end}"
                    );
                    for _ in 0..4 {
                        let base = value(&mut seed);
                        for (prefix, order) in [
                            (node.forward, values.clone()),
                            (node.reverse, values.iter().rev().copied().collect()),
                        ] {
                            let mut sat = Saturation::default();
                            let sequential = order.iter().fold(base, |s, v| s.add(*v, &mut sat));
                            assert_eq!(prefix.fits(base), sat.saturated == 0, "{start}..{end}");
                            if prefix.fits(base) {
                                assert_eq!(prefix.total(base), sequential, "{start}..{end}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// `Tree` answers every read like the reference loop over the same
    /// positions (sum, saturation count, area union, content), at one visit
    /// when the tree proves the aggregate exact and one more per position
    /// of the span when it falls back. Positions are filtered by brute force
    /// here, independent of `Tree::span`.
    #[test]
    fn tree_reads_match_the_reference_loop() {
        let p = crate::ruby::accumulate_tests::outer_siblings(12);
        let data = &p.data;
        let containers: Vec<usize> = (0..data.ruby.containers.len()).collect();
        let mut seed = 0x6a09_e667_f3bc_c908;
        let (mut fast, mut slow) = ([0usize; 3], [0usize; 3]);
        // Fallbacks forced by saturation alone (every position passes).
        let mut saturating = 0;
        for round in 0..300 {
            let start = if round % 2 == 0 {
                0
            } else {
                (next(&mut seed) % 6) as usize
            };
            let clipped = |c: usize| data.ruby.containers[c].units.start.max(start);
            let mut acc = Accumulator::default();
            for &c in &containers {
                let units = &data.ruby.containers[c].units;
                let at = clipped(c);
                let mut e = entry(&mut seed, at);
                e.units.end = if next(&mut seed).is_multiple_of(3) {
                    at + 1 + (next(&mut seed) as usize) % units.len()
                } else {
                    units.end.max(at + 1)
                };
                if round % 3 != 0 {
                    e.adjustment = LayoutUnit::from_raw((next(&mut seed) % 2000) as i32 - 1000);
                }
                acc.push_raw(e);
            }
            let limit = acc.entries.iter().map(|e| e.units.end).max().unwrap_or(0) + 1;
            for _ in 0..20 {
                let from = (next(&mut seed) as usize) % (containers.len() + 1);
                let a = (next(&mut seed) as usize) % limit;
                let b = a + (next(&mut seed) as usize) % (limit - a + 1);
                let range = a..b;
                let tree = Tree {
                    acc: &acc,
                    data,
                    containers: &containers,
                    start,
                    from,
                };
                let span: Vec<usize> = (from..acc.len())
                    .filter(|&pos| range.contains(&clipped(containers[pos])))
                    .collect();
                let all_pass = span.iter().all(|&pos| {
                    let e = &acc.entries[pos];
                    !e.present || e.units.end <= range.end
                });
                let qualifying: Vec<&Entry> = span
                    .iter()
                    .map(|&pos| &acc.entries[pos])
                    .filter(|e| e.present && e.units.end <= range.end)
                    .collect();
                let mut cx = crate::LayoutContext::new();

                let width = value(&mut seed);
                let mut expected_sat = Saturation::default();
                let expected = qualifying
                    .iter()
                    .fold(width, |w, e| w.add(e.adjustment, &mut expected_sat));
                let mut sat = Saturation::default();
                let visits = cx.ruby_measure_visits;
                let got = tree.add_adjustments(&range, width, &mut cx, &mut sat);
                assert_eq!(got, expected, "{start} {from} {range:?}");
                assert_eq!(sat.saturated, expected_sat.saturated, "{range:?}");
                let exact = all_pass && expected_sat.saturated == 0;
                let cost = if exact { 1 } else { 1 + span.len() };
                assert_eq!(cx.ruby_measure_visits - visits, cost, "{range:?}");
                if exact {
                    fast[0] += 1
                } else {
                    slow[0] += 1
                }
                if all_pass && !exact {
                    saturating += 1;
                }

                let area = Bounds {
                    top: LayoutUnit::from_raw(-5),
                    bottom: LayoutUnit::from_raw(5),
                };
                let expected = qualifying.iter().fold(area, |a, e| a.union(e.whole_area));
                let visits = cx.ruby_measure_visits;
                let got = tree.union_areas(&range, area, &mut cx);
                assert_eq!(
                    (got.top, got.bottom),
                    (expected.top, expected.bottom),
                    "{range:?}"
                );
                let cost = if all_pass { 1 } else { 1 + span.len() };
                assert_eq!(cx.ruby_measure_visits - visits, cost, "{range:?}");
                if all_pass {
                    fast[1] += 1
                } else {
                    slow[1] += 1
                }

                let expected = qualifying.iter().any(|e| e.has_content);
                assert_eq!(tree.any_content(&range), expected, "{range:?}");
                if all_pass { fast[2] += 1 } else { slow[2] += 1 }
            }
        }
        // Both paths of every read are exercised.
        assert!(
            fast.iter().chain(&slow).all(|n| *n > 50),
            "fast {fast:?} slow {slow:?}"
        );
        assert!(saturating > 10, "{saturating}");
    }
}
