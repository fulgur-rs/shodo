use super::*;
use crate::limits::{LimitExceeded, LimitKind, Limits};

fn cap(n: u64) -> Limits {
    Limits {
        max_shaped_glyphs: Some(n),
        ..Limits::unlimited()
    }
}

#[test]
fn shaped_glyph_trials_check_local_and_ancestor_caps_before_charging() {
    for (outer_cap, inner_cap) in [(8, 1), (1, 8)] {
        let mut ledger = LimitsLedger::new(&cap(16));
        let outer = ledger.add_scope(&cap(outer_cap), Some(0));
        let inner = ledger.add_scope(&cap(inner_cap), Some(outer));
        ledger.frames[0].current = inner;
        ledger.record_item().unwrap();
        ledger.begin_shaping();
        let checkpoint = ledger.checkpoint();
        ledger.shaped_glyphs(0, 1, Some(1)).unwrap();
        assert_eq!(
            ledger.shaped_glyphs(0, 1, Some(2)),
            Err(LimitExceeded {
                kind: LimitKind::ShapedGlyphs,
                limit: 1,
                actual: 2,
            })
        );
        for owner in [0, outer, inner] {
            assert_eq!(ledger.used(owner, LimitKind::ShapedGlyphs), 1);
        }
        ledger.rollback(checkpoint);
        for owner in [0, outer, inner] {
            assert_eq!(ledger.used(owner, LimitKind::ShapedGlyphs), 0);
        }
        // A discarded, admitted trial leaves room for an exact-cap candidate.
        let checkpoint = ledger.checkpoint();
        ledger.shaped_glyphs(0, 1, Some(1)).unwrap();
        let selected = ledger.rollback(checkpoint);
        ledger.commit(selected).unwrap();
        assert_eq!(ledger.used(inner, LimitKind::ShapedGlyphs), 1);
    }
}

#[test]
fn shaped_glyph_trials_include_consumption_committed_after_begin_shaping() {
    let mut ledger = LimitsLedger::new(&cap(2));
    ledger.begin_shaping();
    ledger.shaped_glyphs(0, 1, None).unwrap();
    let checkpoint = ledger.checkpoint();
    assert_eq!(
        ledger.shaped_glyphs(0, 2, Some(2)),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 2,
            actual: 3,
        })
    );
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 1);
    ledger.rollback(checkpoint);
    ledger.shaped_glyphs(0, 1, None).unwrap();
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 2);
}

#[test]
fn shaped_glyph_trials_accumulate_sibling_charges_at_the_current_ancestor() {
    let mut ledger = LimitsLedger::new(&cap(16));
    let outer = ledger.add_scope(&cap(3), Some(0));
    let left = ledger.add_scope(&cap(2), Some(outer));
    let right = ledger.add_scope(&cap(2), Some(outer));
    for owner in [left, right] {
        ledger.frames[0].current = owner;
        ledger.record_item().unwrap();
    }
    ledger.begin_shaping();
    ledger
        .charge_scope(outer, LimitKind::ShapedGlyphs, 1)
        .unwrap();
    let checkpoint = ledger.checkpoint();
    ledger.shaped_glyphs(0, 1, Some(1)).unwrap();
    assert_eq!(
        ledger.shaped_glyphs(1, 2, Some(3)),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 3,
            actual: 4,
        })
    );
    assert_eq!(ledger.used(right, LimitKind::ShapedGlyphs), 0);
    ledger.rollback(checkpoint);
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 1);
    assert_eq!(ledger.used(outer, LimitKind::ShapedGlyphs), 1);
    assert_eq!(ledger.used(left, LimitKind::ShapedGlyphs), 0);
    ledger.shaped_glyphs(1, 2, None).unwrap();
}

#[test]
fn siblings_charge_their_shared_root_without_sharing_local_room() {
    let mut ledger = LimitsLedger::new(&cap(5));
    let left = ledger.add_scope(&cap(3), Some(0));
    let right = ledger.add_scope(&cap(3), Some(0));
    ledger
        .charge_scope(left, LimitKind::ShapedGlyphs, 3)
        .unwrap();
    ledger
        .charge_scope(right, LimitKind::ShapedGlyphs, 2)
        .unwrap();
    assert_eq!(
        ledger.charge_scope(right, LimitKind::ShapedGlyphs, 1),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 5,
            actual: 6
        })
    );
    assert_eq!(
        ledger.charge_scope(left, LimitKind::ShapedGlyphs, 1),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 3,
            actual: 4
        })
    );
}

#[test]
fn equal_remaining_room_reports_the_nearest_original_owner_cap() {
    let mut ledger = LimitsLedger::new(&cap(10));
    ledger.charge_scope(0, LimitKind::ShapedGlyphs, 5).unwrap();
    let child = ledger.add_scope(&cap(5), Some(0));
    assert_eq!(
        ledger.charge_scope(child, LimitKind::ShapedGlyphs, 6),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 5,
            actual: 6
        })
    );
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 5);
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 0);
}

#[test]
fn trial_rollback_preserves_prefix_and_commit_preserves_producer_failure() {
    let mut ledger = LimitsLedger::new(&cap(10));
    let child = ledger.add_scope(&cap(5), Some(0));
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 2)
        .unwrap();
    let checkpoint = ledger.checkpoint();
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 1)
        .unwrap();
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 2)
        .unwrap();
    let trial = ledger.rollback(checkpoint);
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 2);
    ledger.commit(trial).unwrap();
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 5);
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 5);
}

fn reservations(owner: ScopeId, amounts: &[u64]) -> Reservations {
    Reservations(
        amounts
            .iter()
            .map(|&amount| Charge {
                owner,
                kind: LimitKind::ShapedGlyphs,
                amount,
                shape_items: false,
                reach: Reach::Chain,
            })
            .collect(),
    )
}

#[test]
fn trial_commit_reports_the_original_failing_producer_without_shaping() {
    let mut ledger = LimitsLedger::new(&cap(10));
    let child = ledger.add_scope(&cap(5), Some(0));
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 2)
        .unwrap();
    let checkpoint = ledger.checkpoint();
    ledger.apply_unchecked(Charge {
        owner: child,
        kind: LimitKind::ShapedGlyphs,
        amount: 1,
        shape_items: false,
        reach: Reach::Chain,
    });
    ledger.apply_unchecked(Charge {
        owner: child,
        kind: LimitKind::ShapedGlyphs,
        amount: 3,
        shape_items: false,
        reach: Reach::Chain,
    });
    let trial = ledger.rollback(checkpoint);
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 2);
    assert_eq!(
        ledger.commit(trial),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 5,
            actual: 6
        })
    );
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 3);
}

#[test]
fn trial_preflight_keeps_spent_clean_after_failed_sibling_reservations() {
    let mut ledger = LimitsLedger::new(&cap(5));
    let left = ledger.add_scope(&cap(3), Some(0));
    let right = ledger.add_scope(&cap(3), Some(0));
    ledger
        .charge_scope(right, LimitKind::ShapedGlyphs, 1)
        .unwrap();
    let mut mixed = reservations(left, &[3]);
    mixed.0.extend(reservations(right, &[2]).0);
    assert!(!ledger.can_commit(&mixed));
    for _ in 0..2 {
        let mut fits = reservations(left, &[2]);
        fits.0.extend(reservations(right, &[2]).0);
        assert!(ledger.can_commit(&fits));
    }
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 1);
    assert_eq!(ledger.used(left, LimitKind::ShapedGlyphs), 0);
    assert_eq!(ledger.used(right, LimitKind::ShapedGlyphs), 1);
    assert!(ledger.journal.is_empty());
    assert!(ledger.checkpoints.is_empty());
}

#[test]
fn saturated_trial_sums_match_individual_producer_charges() {
    for limit in [None, Some(u64::MAX - 1)] {
        let limits = Limits {
            max_shaped_glyphs: limit,
            ..Limits::unlimited()
        };
        let mut ledger = LimitsLedger::new(&limits);
        let child = ledger.add_scope(&limits, Some(0));
        ledger
            .charge_scope(child, LimitKind::ShapedGlyphs, u64::MAX - 2)
            .unwrap();
        assert!(ledger.can_commit(&reservations(child, &[1])));
        assert_eq!(
            ledger.can_commit(&reservations(child, &[1, 1])),
            limit.is_none()
        );
        assert_eq!(
            ledger.can_commit(&reservations(child, &[u64::MAX, 2])),
            limit.is_none()
        );
        assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), u64::MAX - 2);
        let result = ledger.commit(reservations(child, &[u64::MAX, 2]));
        assert_eq!(result.is_ok(), limit.is_none());
        if limit.is_none() {
            assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), u64::MAX);
        }
    }
}

#[test]
fn reservation_preflight_matches_sequential_replay_for_nested_siblings() {
    let limits = [None, Some(0), Some(1), Some(2), Some(3), Some(5)];
    let amounts = [0, 1, 2, 3];
    for root in limits {
        for left in limits {
            for right in limits {
                let limits = |max_shaped_glyphs| Limits {
                    max_shaped_glyphs,
                    ..Limits::unlimited()
                };
                let mut ledger = LimitsLedger::new(&limits(root));
                let left_id = ledger.add_scope(&limits(left), Some(0));
                let right_id = ledger.add_scope(&limits(right), Some(0));
                let owners = [left_id, right_id, 0];
                for len in 1..=3usize {
                    let total = (3 * amounts.len()).pow(len as u32);
                    for code in 0..total {
                        let mut code = code;
                        let charges: Vec<_> = (0..len)
                            .map(|_| {
                                let digit = code % (3 * amounts.len());
                                code /= 3 * amounts.len();
                                (owners[digit % 3], amounts[digit / 3])
                            })
                            .collect();
                        let candidate = Reservations(
                            charges
                                .iter()
                                .map(|&(owner, amount)| Charge {
                                    owner,
                                    kind: LimitKind::ShapedGlyphs,
                                    amount,
                                    shape_items: false,
                                    reach: Reach::Chain,
                                })
                                .collect(),
                        );
                        let checkpoint = ledger.checkpoint();
                        let replay = charges.iter().all(|&(owner, amount)| {
                            ledger
                                .charge_scope(owner, LimitKind::ShapedGlyphs, amount)
                                .is_ok()
                        });
                        ledger.rollback(checkpoint);
                        assert_eq!(
                            ledger.can_commit(&candidate),
                            replay,
                            "{root:?} {left:?} {right:?} {charges:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn source_cut_allowances_are_not_replayed_into_first_line() {
    let mut ledger = LimitsLedger::new(&Limits::unlimited());
    let capture = ledger.capture();
    ledger.charge(LimitKind::AnalysisBytes, 4).unwrap();
    ledger.record_replay(false);
    ledger.charge(LimitKind::AnalysisBytes, 9).unwrap();
    ledger.record_replay(true);
    ledger.charge(LimitKind::AnalysisBytes, 7).unwrap();
    let replay = ledger.captured(capture);
    ledger.commit(replay).unwrap();
    assert_eq!(ledger.used(0, LimitKind::AnalysisBytes), 31);
}

#[test]
fn shaping_runs_and_input_items_keep_independent_counters_under_one_cap() {
    let mut ledger = LimitsLedger::new(&Limits {
        max_items: Some(2),
        ..Limits::unlimited()
    });
    ledger.record_item().unwrap();
    ledger.record_item().unwrap();
    ledger.shape_item(0).unwrap();
    ledger.shape_item(1).unwrap();
    assert_eq!(
        ledger.record_item(),
        Err(LimitExceeded {
            kind: LimitKind::Items,
            limit: 2,
            actual: 3
        })
    );
    assert_eq!(
        ledger.shape_item(0),
        Err(LimitExceeded {
            kind: LimitKind::Items,
            limit: 2,
            actual: 3
        })
    );
}

#[test]
fn public_analysis_remains_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<crate::ParagraphAnalysis>();
}

#[test]
fn trial_journal_snapshots_each_touched_scope_once() {
    let mut ledger = LimitsLedger::new(&Limits::unlimited());
    let mut owner = 0;
    for _ in 0..16 {
        owner = ledger.add_scope(&Limits::unlimited(), Some(owner));
    }
    let checkpoint = ledger.checkpoint();
    for _ in 0..128 {
        ledger.apply_unchecked(Charge {
            owner,
            kind: LimitKind::ShapedGlyphs,
            amount: 1,
            shape_items: false,
            reach: Reach::Chain,
        });
    }
    assert_eq!(ledger.journal.len(), 128, "one producer record per window");
    let snapshots = ledger.snapshots.len();
    assert_eq!(
        snapshots, 17,
        "one undo snapshot per touched scope, independent of window count"
    );
    ledger.rollback(checkpoint);
    assert_eq!(ledger.used(owner, LimitKind::ShapedGlyphs), 0);
}

#[test]
fn nested_trial_rollback_restores_outer_snapshots_and_accepted_prefix() {
    let mut ledger = LimitsLedger::new(&cap(100));
    let child = ledger.add_scope(&cap(100), Some(0));
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 3)
        .unwrap();
    let outer = ledger.checkpoint();
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 5)
        .unwrap();
    let inner = ledger.checkpoint();
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 7)
        .unwrap();
    assert_eq!(ledger.snapshots.len(), 4);
    let selected = ledger.rollback(inner);
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 8);
    ledger.commit(selected).unwrap();
    assert_eq!(
        ledger.snapshots.len(),
        2,
        "inner commit reuses the existing outer snapshots"
    );
    ledger
        .charge_scope(child, LimitKind::ShapedGlyphs, 11)
        .unwrap();
    let selected = ledger.rollback(outer);
    assert_eq!(ledger.used(0, LimitKind::ShapedGlyphs), 3);
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 3);
    assert!(ledger.scopes.iter().all(|scope| scope.snapshot.is_none()));
    ledger.commit(selected).unwrap();
    assert_eq!(ledger.used(child, LimitKind::ShapedGlyphs), 26);
}

#[test]
fn nested_trial_commit_preserves_inner_producer_boundaries_in_outer_journal() {
    let mut ledger = LimitsLedger::new(&cap(100));
    let outer = ledger.checkpoint();
    let inner = ledger.checkpoint();
    ledger.charge(LimitKind::ShapedGlyphs, 3).unwrap();
    ledger.charge(LimitKind::ShapedGlyphs, 5).unwrap();
    let inner = ledger.rollback(inner);
    ledger.commit(inner).unwrap();
    let outer = ledger.rollback(outer);
    ledger.charge(LimitKind::ShapedGlyphs, 98).unwrap();
    assert_eq!(
        ledger.commit(outer),
        Err(LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 100,
            actual: 101
        })
    );
}
