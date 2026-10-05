//! Exact replay of a measurement's side effects on the layout context.
//!
//! A cached measurement may stand in for a fresh one only when running it
//! again would leave the context exactly as the replay does: the same
//! saturation counts, the same edge reshape budget charges with the same
//! accept/refuse outcomes, and no warnings.
//!
//! `within_line_reshape_budget` adds `bytes` to the operation's `spent` with
//! saturation, accepts iff the new `spent <= limit`, and warns iff it refuses
//! while `spent` was still `<= limit` before. Charges are therefore kept as
//! an aggregate, which decides exactly two replayable cases:
//! - every recorded charge was accepted and `spent + total <= min(limit)`:
//!   each prefix is at most the total, so every charge is accepted again;
//! - every recorded charge was refused and `spent > max(limit)`: each charge
//!   starts above its limit, so it is refused again without a warning.
//!
//! In both cases the resulting `spent` is one saturating addition of the
//! saturating total. Any other state is measured afresh. Requiring the
//! recorded outcomes (not only the current ones) matters when the warning
//! sink was suppressed during recording: a refused charge then warns into
//! nothing and would otherwise look replayable.

use crate::LayoutContext;
use crate::geometry::Saturation;

/// Aggregate of the reshape budget charges of one recorded measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Charges {
    bytes: u64,
    min_limit: u64,
    max_limit: u64,
    all_within: bool,
    none_within: bool,
}

impl Default for Charges {
    fn default() -> Self {
        Self {
            bytes: 0,
            min_limit: u64::MAX,
            max_limit: 0,
            all_within: true,
            none_within: true,
        }
    }
}

impl Charges {
    fn push(&mut self, bytes: u64, limit: u64, within: bool) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.min_limit = self.min_limit.min(limit);
        self.max_limit = self.max_limit.max(limit);
        self.all_within &= within;
        self.none_within &= !within;
    }

    fn merge(&mut self, other: &Self) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        self.min_limit = self.min_limit.min(other.min_limit);
        self.max_limit = self.max_limit.max(other.max_limit);
        self.all_within &= other.all_within;
        self.none_within &= other.none_within;
    }

    /// The aggregate of `n` copies of these charges in a row. Exact: `n`
    /// saturating additions of a saturated total equal one saturating
    /// multiplication, every limit and outcome is unchanged, and zero copies
    /// are no charges at all.
    fn times(&self, n: u64) -> Self {
        if n == 0 {
            return Self::default();
        }
        Self {
            bytes: self.bytes.saturating_mul(n),
            ..*self
        }
    }

    /// Whether replaying from `spent` gives every charge its recorded
    /// outcome without a warning. No charges always replay.
    fn replayable(&self, spent: u64) -> bool {
        (self.all_within && spent.saturating_add(self.bytes) <= self.min_limit)
            || (self.none_within && spent > self.max_limit)
    }
}

/// Side effects of one measurement that left no warning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Effects {
    charges: Charges,
    sat: Saturation,
    suppressed: bool,
}

impl Effects {
    /// Effects of measuring both, in either order: charges are kept as an
    /// order-free aggregate and Saturation as counters. `None` if the two
    /// ran under different suppression states (no single gate covers both).
    ///
    /// Exactness: when both recorded only accepted charges (or both only
    /// refused ones) the merged aggregate is uniform and the gate decides
    /// the sequence exactly. When their outcomes differ the merged aggregate
    /// is neither all-accepted nor all-refused, so the result is never
    /// replayable rather than an approximation.
    pub(crate) fn then(self, other: Self) -> Option<Self> {
        if self.suppressed != other.suppressed {
            return None;
        }
        let mut charges = self.charges;
        charges.merge(&other.charges);
        Some(Self {
            charges,
            sat: Saturation {
                saturated: self.sat.saturated.wrapping_add(other.sat.saturated),
                non_finite: self.sat.non_finite.wrapping_add(other.sat.non_finite),
            },
            suppressed: self.suppressed,
        })
    }

    /// Effects of replaying these `n` times in a row. Counters wrap like
    /// `n` separate `+=`.
    pub(crate) fn times(self, n: u64) -> Self {
        // Truncation is exact: `n` wrapping additions of `v` equal
        // `v * (n mod 2^32)` modulo `2^32`.
        let k = n as u32;
        Self {
            charges: self.charges.times(n),
            sat: Saturation {
                saturated: self.sat.saturated.wrapping_mul(k),
                non_finite: self.sat.non_finite.wrapping_mul(k),
            },
            suppressed: self.suppressed,
        }
    }

    pub(crate) fn sat(&self) -> Saturation {
        self.sat
    }

    /// No charges and no saturation, recorded with an unsuppressed sink
    /// that took no warning. Measuring again then gives the same result in
    /// any state: no charge was made, so no budget outcome or `spent` value
    /// was read, and nothing on the path reads the suppression state. Such
    /// effects may be replayed without `replay`'s gate (which would refuse
    /// them under a suppressed sink). A recording under a suppressed sink
    /// may have dropped a warning, so it is never empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.charges == Charges::default() && self.sat.is_clean() && !self.suppressed
    }

    /// These effects without `sat` (a part recorded elsewhere).
    pub(crate) fn without_sat(self, sat: Saturation) -> Self {
        Self {
            sat: Saturation {
                saturated: self.sat.saturated.wrapping_sub(sat.saturated),
                non_finite: self.sat.non_finite.wrapping_sub(sat.non_finite),
            },
            ..self
        }
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> u64 {
        self.charges.bytes
    }
}

/// Charge aggregates of the measurements currently being recorded,
/// innermost last.
#[derive(Debug, Default)]
pub(crate) struct ReshapeLog {
    frames: Vec<Charges>,
}

impl ReshapeLog {
    pub(crate) fn clear(&mut self) {
        self.frames.clear();
    }
}

pub(crate) struct Recording {
    depth: usize,
    sat: Saturation,
    checkpoint: Option<usize>,
    suppressed: bool,
}

/// Called by every reshape budget charge with its outcome.
pub(crate) fn record_charge(cx: &mut LayoutContext, bytes: u64, limit: u64, within: bool) {
    if let Some(top) = cx.reshape_log.frames.last_mut() {
        top.push(bytes, limit, within);
    }
}

pub(crate) fn begin(cx: &mut LayoutContext, sat: &Saturation) -> Recording {
    cx.reshape_log.frames.push(Charges::default());
    Recording {
        depth: cx.reshape_log.frames.len() - 1,
        sat: *sat,
        checkpoint: cx.warnings.checkpoint(),
        suppressed: cx.warnings.is_suppressed(),
    }
}

/// Open recording frames; the innermost has index `depth - 1`.
pub(crate) fn depth(cx: &LayoutContext) -> usize {
    cx.reshape_log.frames.len()
}

/// Pop the frames down to `depth` (the recording's own and those of any
/// unfinished recording above it) into one aggregate.
fn pop_frames(cx: &mut LayoutContext, depth: usize) -> Charges {
    // Frames above `depth` belong to recordings that did not finish (an
    // unwound measurement); fold them in so no charge is lost.
    let mut charges = Charges::default();
    while cx.reshape_log.frames.len() > depth {
        let frame = cx.reshape_log.frames.pop().unwrap();
        charges.merge(&frame);
    }
    charges
}

fn recorded(
    cx: &LayoutContext,
    recording: &Recording,
    charges: Charges,
    sat: &Saturation,
) -> Option<Effects> {
    // A suppressed sink drops every push, so it cannot have changed.
    let clean = cx.warnings.is_suppressed() == recording.suppressed
        && cx.warnings.checkpoint() == recording.checkpoint;
    clean.then_some(Effects {
        charges,
        sat: Saturation {
            saturated: sat.saturated.wrapping_sub(recording.sat.saturated),
            non_finite: sat.non_finite.wrapping_sub(recording.sat.non_finite),
        },
        suppressed: recording.suppressed,
    })
}

/// End a recording. Its charges also count for the enclosing recording.
/// Returns `None` if the measurement pushed any warning.
pub(crate) fn finish(
    cx: &mut LayoutContext,
    recording: Recording,
    sat: &Saturation,
) -> Option<Effects> {
    let charges = pop_frames(cx, recording.depth);
    if let Some(parent) = cx.reshape_log.frames.last_mut() {
        parent.merge(&charges);
    }
    recorded(cx, &recording, charges, sat)
}

/// `finish`, but the charges count for the frame at index `frame` (or for
/// no recording) instead of the enclosing one. A detached profile share
/// keeps the profile's charges out of the container recording this way.
pub(crate) fn finish_to(
    cx: &mut LayoutContext,
    recording: Recording,
    sat: &Saturation,
    frame: Option<usize>,
) -> Option<Effects> {
    let charges = pop_frames(cx, recording.depth);
    debug_assert!(
        frame.is_none_or(|i| i < cx.reshape_log.frames.len()),
        "finish_to target frame {frame:?} is not open"
    );
    if let Some(target) = frame.and_then(|i| cx.reshape_log.frames.get_mut(i)) {
        target.merge(&charges);
    }
    recorded(cx, &recording, charges, sat)
}

fn gate(cx: &LayoutContext, effects: &Effects) -> bool {
    cx.warnings.is_suppressed() == effects.suppressed
        && effects.charges.replayable(cx.edge_reshape_spent)
}

/// Whether `replay` would apply `effects` now; changes nothing.
#[cfg(test)]
pub(crate) fn replayable(cx: &LayoutContext, effects: &Effects) -> bool {
    gate(cx, effects)
}

fn apply(
    cx: &mut LayoutContext,
    effects: &Effects,
    sat: &mut Saturation,
    frame: Option<usize>,
) -> bool {
    debug_assert!(
        frame.is_none_or(|i| i < cx.reshape_log.frames.len()),
        "replay target frame {frame:?} is not open"
    );
    if !gate(cx, effects) {
        #[cfg(test)]
        {
            cx.ruby_replay_refusals += 1;
        }
        return false;
    }
    cx.edge_reshape_spent = cx.edge_reshape_spent.saturating_add(effects.charges.bytes);
    if let Some(target) = frame.and_then(|i| cx.reshape_log.frames.get_mut(i)) {
        target.merge(&effects.charges);
    }
    // Counters only ever increment; `+=` in release wraps the same way.
    sat.saturated = sat.saturated.wrapping_add(effects.sat.saturated);
    sat.non_finite = sat.non_finite.wrapping_add(effects.sat.non_finite);
    true
}

/// Apply recorded effects if that is exactly what measuring again would do.
/// Returns `false`, changing nothing, otherwise.
pub(crate) fn replay(cx: &mut LayoutContext, effects: &Effects, sat: &mut Saturation) -> bool {
    let top = cx.reshape_log.frames.len().checked_sub(1);
    apply(cx, effects, sat, top)
}

/// `replay`, merging the charges into the frame at index `frame` (or into
/// no recording) instead of the innermost one.
pub(crate) fn replay_to(
    cx: &mut LayoutContext,
    effects: &Effects,
    sat: &mut Saturation,
    frame: Option<usize>,
) -> bool {
    apply(cx, effects, sat, frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::WarningKind;

    fn sat(saturated: u32) -> Saturation {
        Saturation {
            saturated,
            non_finite: 0,
        }
    }

    /// Record `charges` as `(bytes, limit)` from the current `spent`, exactly
    /// the way `within_line_reshape_budget` charges and warns.
    fn record(cx: &mut LayoutContext, charges: &[(u64, u64)], extra_sat: u32) -> Option<Effects> {
        let mut s = Saturation::default();
        let recording = begin(cx, &s);
        for (bytes, limit) in charges {
            let before = cx.edge_reshape_spent;
            cx.edge_reshape_spent = before.saturating_add(*bytes);
            let within = cx.edge_reshape_spent <= *limit;
            record_charge(cx, *bytes, *limit, within);
            if !within && before <= *limit {
                cx.warnings.push(
                    WarningKind::Unsupported,
                    "line edge reshape budget exceeded; keeping shared glyphs",
                );
            }
        }
        s.saturated += extra_sat;
        finish(cx, recording, &s)
    }

    #[test]
    fn only_unsuppressed_recordings_without_effects_are_empty() {
        let mut cx = LayoutContext::new();
        assert!(record(&mut cx, &[], 0).unwrap().is_empty());
        assert!(!record(&mut cx, &[], 1).unwrap().is_empty());
        assert!(!record(&mut cx, &[(0, 100)], 0).unwrap().is_empty());
        cx.warnings.set_max(Some(0));
        cx.warnings.push(WarningKind::Unsupported, "suppress");
        assert!(cx.warnings.is_suppressed());
        let suppressed = record(&mut cx, &[], 0).unwrap();
        assert!(!suppressed.is_empty());
    }

    #[test]
    fn accepted_charges_replay_while_they_still_fit() {
        let mut cx = LayoutContext::new();
        let effects = record(&mut cx, &[(10, 100), (20, 100)], 2).unwrap();
        cx.edge_reshape_spent = 70;
        let mut s = sat(1);
        assert!(replay(&mut cx, &effects, &mut s));
        assert_eq!(cx.edge_reshape_spent, 100);
        assert_eq!(s, sat(3));
    }

    #[test]
    fn accepted_charges_do_not_replay_across_the_limit() {
        let mut cx = LayoutContext::new();
        let effects = record(&mut cx, &[(10, 100), (20, 100)], 0).unwrap();
        cx.edge_reshape_spent = 71;
        let mut s = sat(1);
        assert!(!replay(&mut cx, &effects, &mut s));
        assert_eq!(cx.edge_reshape_spent, 71);
        assert_eq!(s, sat(1));
    }

    #[test]
    fn refused_charges_replay_only_past_every_limit() {
        let mut cx = LayoutContext::new();
        cx.edge_reshape_spent = 500;
        let effects = record(&mut cx, &[(10, 100), (20, 200)], 0).unwrap();
        cx.edge_reshape_spent = 200;
        assert!(!replay(&mut cx, &effects, &mut Saturation::default()));
        cx.edge_reshape_spent = 201;
        assert!(replay(&mut cx, &effects, &mut Saturation::default()));
        assert_eq!(cx.edge_reshape_spent, 231);
    }

    #[test]
    fn mixed_outcomes_never_replay() {
        let mut cx = LayoutContext::new();
        cx.edge_reshape_spent = 90;
        let effects = record(&mut cx, &[(5, 100), (20, 100)], 0);
        // The crossing charge warned, so nothing replayable was recorded.
        assert!(effects.is_none());
        let mut cx = LayoutContext::new();
        cx.warnings.set_max(Some(0));
        cx.warnings.push(WarningKind::Unsupported, "suppress");
        cx.edge_reshape_spent = 90;
        let effects = record(&mut cx, &[(5, 100), (20, 100)], 0).unwrap();
        for spent in [0, 90, 101, 1000] {
            cx.edge_reshape_spent = spent;
            assert!(
                !replay(&mut cx, &effects, &mut Saturation::default()),
                "{spent}"
            );
        }
    }

    #[test]
    fn warnings_or_a_changed_suppression_state_prevent_reuse() {
        let mut cx = LayoutContext::new();
        let mut s = Saturation::default();
        let recording = begin(&mut cx, &s);
        cx.warnings.push(WarningKind::Unsupported, "measured");
        assert!(finish(&mut cx, recording, &s).is_none());
        let recording = begin(&mut cx, &s);
        s.non_finite += 1;
        let effects = finish(&mut cx, recording, &s).unwrap();
        cx.warnings.set_max(Some(0));
        cx.warnings.push(WarningKind::Unsupported, "suppress");
        assert!(!replay(&mut cx, &effects, &mut Saturation::default()));
    }

    #[test]
    fn nothing_charged_replays_at_any_spent() {
        let mut cx = LayoutContext::new();
        let effects = record(&mut cx, &[], 0).unwrap();
        cx.edge_reshape_spent = u64::MAX;
        assert!(replay(&mut cx, &effects, &mut Saturation::default()));
        assert_eq!(cx.edge_reshape_spent, u64::MAX);
    }

    #[test]
    fn nested_replays_are_recorded_by_the_enclosing_recording() {
        let mut cx = LayoutContext::new();
        let inner = record(&mut cx, &[(10, 100)], 1).unwrap();
        cx.edge_reshape_spent = 0;
        let s = Saturation::default();
        let outer = begin(&mut cx, &s);
        let mut s = s;
        assert!(replay(&mut cx, &inner, &mut s));
        cx.edge_reshape_spent = cx.edge_reshape_spent.saturating_add(5);
        record_charge(&mut cx, 5, 100, true);
        let outer = finish(&mut cx, outer, &s).unwrap();
        cx.edge_reshape_spent = 80;
        assert!(replay(&mut cx, &outer, &mut Saturation::default()));
        assert_eq!(cx.edge_reshape_spent, 95);
        cx.edge_reshape_spent = 86;
        assert!(!replay(&mut cx, &outer, &mut Saturation::default()));
    }

    #[test]
    fn repeated_effects_equal_recording_the_charges_repeatedly() {
        let mut cx = LayoutContext::new();
        let once = record(&mut cx, &[(10, 100), (5, 100)], 2).unwrap();
        cx.edge_reshape_spent = 0;
        let thrice = record(
            &mut cx,
            &[
                (10, 100),
                (5, 100),
                (10, 100),
                (5, 100),
                (10, 100),
                (5, 100),
            ],
            6,
        )
        .unwrap();
        assert_eq!(once.times(3), thrice);
        cx.edge_reshape_spent = 0;
        assert_eq!(once.times(0), record(&mut cx, &[], 0).unwrap());
        let mut cx = LayoutContext::new();
        let big = record(&mut cx, &[(u64::MAX / 2, u64::MAX)], 0).unwrap();
        assert_eq!(big.times(3).bytes(), u64::MAX);
    }

    #[test]
    fn sequenced_effects_equal_one_recording_of_both() {
        let mut cx = LayoutContext::new();
        let a = record(&mut cx, &[(10, 100)], 1).unwrap();
        cx.edge_reshape_spent = 0;
        let b = record(&mut cx, &[(20, 200)], 2).unwrap();
        cx.edge_reshape_spent = 0;
        let both = record(&mut cx, &[(10, 100), (20, 200)], 3).unwrap();
        assert_eq!(a.then(b), Some(both));
        assert_eq!(b.then(a), Some(both));
        let mut suppressed = LayoutContext::new();
        suppressed.warnings.set_max(Some(0));
        suppressed
            .warnings
            .push(WarningKind::Unsupported, "suppress");
        let c = record(&mut suppressed, &[(10, 100)], 1).unwrap();
        assert_eq!(a.then(c), None);
        cx.edge_reshape_spent = 0;
        let more = record(&mut cx, &[(10, 100)], 3).unwrap();
        assert_eq!(more.without_sat(sat(2)), a);
        assert_eq!(more.sat(), sat(3));
    }

    #[test]
    fn mixed_outcome_sequences_and_repeats_stay_exact() {
        // Accepted then refused charges: no single aggregate decides the
        // sequence, so the composition must never replay.
        let mut cx = LayoutContext::new();
        let accepted = record(&mut cx, &[(10, 100)], 0).unwrap();
        cx.edge_reshape_spent = 500;
        let refused = record(&mut cx, &[(10, 100)], 0).unwrap();
        // Uniformly refused effects compose into replayable refused effects.
        let both_refused = refused.then(refused).unwrap();
        cx.edge_reshape_spent = 101;
        assert!(replayable(&cx, &both_refused));
        assert!(replayable(&cx, &refused.times(5)));
        cx.edge_reshape_spent = 100;
        assert!(!replayable(&cx, &both_refused));
        for effects in [
            accepted.then(refused).unwrap(),
            refused.then(accepted).unwrap(),
        ] {
            for spent in [0, 50, 100, 101, 1000] {
                cx.edge_reshape_spent = spent;
                assert!(!replayable(&cx, &effects), "{spent}");
            }
        }
        // Repetition keeps the accepted-charge gate exact: three copies need
        // room for all three totals below the limit.
        cx.edge_reshape_spent = 70;
        assert!(replayable(&cx, &accepted.times(3)));
        cx.edge_reshape_spent = 71;
        assert!(!replayable(&cx, &accepted.times(3)));
    }

    #[test]
    fn targeted_charges_skip_the_innermost_recording() {
        let mut cx = LayoutContext::new();
        let s = Saturation::default();
        let outer = begin(&mut cx, &s);
        let parent = depth(&cx).checked_sub(1);
        assert_eq!(parent, Some(0));
        let container = begin(&mut cx, &s);
        // A profile measured inside the container charges the outer frame.
        let profile = begin(&mut cx, &s);
        cx.edge_reshape_spent += 10;
        record_charge(&mut cx, 10, 100, true);
        let effects = finish_to(&mut cx, profile, &s, parent).unwrap();
        // So does a replay of it.
        let mut s2 = s;
        assert!(replay_to(&mut cx, &effects, &mut s2, parent));
        // The container's own charge stays in its frame.
        cx.edge_reshape_spent += 5;
        record_charge(&mut cx, 5, 100, true);
        let own = finish(&mut cx, container, &s2).unwrap();
        let total = finish(&mut cx, outer, &s2).unwrap();
        assert_eq!(depth(&cx), 0);
        cx.edge_reshape_spent = 0;
        assert_eq!(own, record(&mut cx, &[(5, 100)], 0).unwrap());
        cx.edge_reshape_spent = 0;
        assert_eq!(
            total,
            record(&mut cx, &[(10, 100), (10, 100), (5, 100)], 0).unwrap()
        );
        // Without a target frame the charges go nowhere, as with no recording.
        cx.edge_reshape_spent = 0;
        assert!(replay_to(
            &mut cx,
            &effects,
            &mut Saturation::default(),
            None
        ));
        assert_eq!(cx.edge_reshape_spent, 10);
    }

    #[test]
    fn replayable_decides_like_replay_without_applying() {
        let mut cx = LayoutContext::new();
        let effects = record(&mut cx, &[(10, 100)], 1).unwrap();
        for spent in [0, 90, 91, 200] {
            cx.edge_reshape_spent = spent;
            let decided = replayable(&cx, &effects);
            assert_eq!(cx.edge_reshape_spent, spent);
            assert_eq!(
                decided,
                replay(&mut cx, &effects, &mut Saturation::default()),
                "{spent}"
            );
        }
    }
}
