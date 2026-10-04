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

/// End a recording. Its charges also count for the enclosing recording.
/// Returns `None` if the measurement pushed any warning.
pub(crate) fn finish(
    cx: &mut LayoutContext,
    recording: Recording,
    sat: &Saturation,
) -> Option<Effects> {
    // Frames above `depth` belong to recordings that did not finish (an
    // unwound measurement); fold them in so no charge is lost.
    let mut charges = Charges::default();
    while cx.reshape_log.frames.len() > recording.depth {
        let frame = cx.reshape_log.frames.pop().unwrap();
        charges.merge(&frame);
    }
    if let Some(parent) = cx.reshape_log.frames.last_mut() {
        parent.merge(&charges);
    }
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

/// Apply recorded effects if that is exactly what measuring again would do.
/// Returns `false`, changing nothing, otherwise.
pub(crate) fn replay(cx: &mut LayoutContext, effects: &Effects, sat: &mut Saturation) -> bool {
    if cx.warnings.is_suppressed() != effects.suppressed
        || !effects.charges.replayable(cx.edge_reshape_spent)
    {
        return false;
    }
    cx.edge_reshape_spent = cx.edge_reshape_spent.saturating_add(effects.charges.bytes);
    if let Some(top) = cx.reshape_log.frames.last_mut() {
        top.merge(&effects.charges);
    }
    // Counters only ever increment; `+=` in release wraps the same way.
    sat.saturated = sat.saturated.wrapping_add(effects.sat.saturated);
    sat.non_finite = sat.non_finite.wrapping_add(effects.sat.non_finite);
    true
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
}
