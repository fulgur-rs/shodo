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
//! - D3 neighbour: neighbour-dependent positions that contain a visual
//!   neighbour of a newly selected event unit. With fixed levels, UAX #9
//!   swaps two units iff the minimum level between them is odd, for every
//!   range holding both, so a longer line only inserts the new units into
//!   the old visual order; the neighbours that change are those adjacent
//!   to an inserted event unit, and the containers whose edge they are
//!   contain them.
//! - D4 profile: profile-dependent positions when height or above changed.
//! - D5 edge: positions whose units or neighbour units intersect a partial
//!   group, removal or replacement that differs from the previous step.
//! - D6 ancestor: ancestors of every dirty position, and of every position
//!   whose live measurement changed its values.
//!
//! Positions without a replayable entry ("unstored") are always dirty.
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
#![allow(dead_code)]

use super::geometry::Bounds;
use crate::geometry::LayoutUnit;
use crate::line::metric_index::SelectionDigest;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dirty {
    Unstored = 0,
    New = 1,
    Clipped = 2,
    Neighbour = 3,
    Profile = 4,
    Edge = 5,
    Ancestor = 6,
    Full = 7,
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
    /// Visual neighbours read by allowances.
    pub(crate) neighbours: [Option<u32>; 2],
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
    /// `(neighbour unit, position)` of neighbour-dependent positions.
    pub(crate) neighbours: BTreeSet<(u32, u32)>,
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
        let pos = self.entries.len() as u32;
        self.entries
            .push(Entry::placeholder(self.open.last().copied()));
        self.open.push(pos);
        self.unstored.insert(pos);
        self.update(pos as usize);
    }

    /// Replace the entry of `pos`, keeping the indexes and the tree in step.
    /// `full_end` is the container's unclipped end.
    pub(crate) fn store(&mut self, pos: usize, entry: Entry, full_end: usize) {
        let p = pos as u32;
        let old = &self.entries[pos];
        if old.neighbour_dependent {
            self.dependents -= 1;
            for unit in old.neighbours.iter().flatten() {
                self.neighbours.remove(&(*unit, p));
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
            for unit in entry.neighbours.iter().flatten() {
                self.neighbours.insert((*unit, p));
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

    #[cfg(test)]
    fn push_raw(&mut self, entry: Entry) {
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
            + self.neighbours.len() * 2 * size_of::<(u32, u32)>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Saturation;

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
        e.neighbours = [Some(0), Some(9)];
        let end = e.units.end;
        acc.store(1, e.clone(), end);
        assert!(acc.unstored.contains(&1) && acc.profile.contains(&1));
        assert!(acc.neighbours.contains(&(0, 1)) && acc.neighbours.contains(&(9, 1)));
        assert!(!acc.clipped.contains(&1));
        assert_eq!(acc.dependents, 1);
        e.own = Some(effects(1, false));
        e.profile = false;
        e.neighbour_dependent = false;
        acc.store(1, e, end + 1);
        assert!(!acc.unstored.contains(&1) && !acc.profile.contains(&1));
        assert!(!acc.neighbours.iter().any(|(_, p)| *p == 1));
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
}
