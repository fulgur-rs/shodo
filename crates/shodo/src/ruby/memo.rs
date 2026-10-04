//! Per-operation reuse of adjustment-only ruby candidates.
//!
//! Fit probes ask for `start..end` with a growing `end`, and the look-ahead
//! extends every such probe to the same paired endpoint `through` until the
//! scan crosses it. The container core depends only on `start..through`
//! (plus the dataset and atomic revision), so it is measured once per
//! operation and replayed while its recorded side effects replay exactly
//! (`line::replay`). Entries live until the operation's reshape budget is
//! reset (`LayoutContext::begin_reshape_operation`) or the context shrinks.
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

#[derive(Debug, Default)]
pub(crate) struct RubyMemo {
    entries: crate::hashing::FastMap<MemoKey, MemoEntry>,
}

impl RubyMemo {
    pub(crate) fn get(&self, key: &MemoKey) -> Option<MemoEntry> {
        self.entries.get(key).copied()
    }

    pub(crate) fn insert(&mut self, key: MemoKey, entry: MemoEntry) {
        self.entries.insert(key, entry);
    }

    pub(crate) fn remove(&mut self, key: &MemoKey) {
        self.entries.remove(key);
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
