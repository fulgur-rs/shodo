//! shodo-mc0: the fail-closed ruby line-measurement work allowance.
use super::accumulate_tests::{digit_siblings, doubling, one_line, profile_churn};
use super::memo_tests::*;
use crate::limits::{Limits, Warning};
use crate::ruby::line_work::WARNING;
use crate::style::LineOptions;
use crate::{AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, Paragraph};

/// `p` with `max_ruby_line_work` replaced (the limit is read from the
/// paragraph's build limits at layout time).
fn with_factor(mut p: Paragraph, factor: Option<u64>) -> Paragraph {
    std::sync::Arc::get_mut(&mut p.data)
        .unwrap()
        .limits
        .max_ruby_line_work = factor;
    p
}

fn exceeded(warnings: &[Warning]) -> usize {
    warnings.iter().filter(|w| w.message == WARNING).count()
}

/// Container measures and budget warnings of one unbreakable churn line.
fn churn_line(r: usize, factor: Option<u64>) -> (usize, usize) {
    let p = with_factor(profile_churn(r), factor);
    let mut cx = mode_context(Mode::Accumulate);
    one_line(&p, &mut cx);
    let warnings = cx.take_warnings();
    (cx.ruby_container_measures, exceeded(&warnings))
}

/// The profile churn line (shodo-2j6's remaining quadratic shape) measures
/// quadratically many containers without the limit and linearly many with
/// it, warning once per operation.
#[test]
fn churn_measures_are_bounded_by_the_allowance() {
    let (all, growth) = doubling(|r| churn_line(r, None).0, [32, 64, 128]);
    assert!(
        growth.iter().all(|g| *g >= 3.0),
        "unlimited {growth:?} {all:?}"
    );
    let (all, growth) = doubling(|r| churn_line(r, Some(1)).0, [64, 128, 256]);
    assert!(
        growth.iter().all(|g| *g <= 2.2),
        "limited {growth:?} {all:?}"
    );
    for r in [64, 128, 256] {
        assert_eq!(churn_line(r, Some(1)).1, 1, "one warning at {r}");
    }
}

/// The operation that exceeded spends at most its allowance, plus one walk
/// (the last admitted probe), plus the full measurement of the accepted
/// line's containers (`apply`).
#[test]
fn spent_work_stays_within_the_allowance_and_one_walk() {
    for factor in [1, 2] {
        let p = with_factor(profile_churn(128), Some(factor));
        let mut cx = mode_context(Mode::Accumulate);
        one_line(&p, &mut cx);
        cx.begin_reshape_operation();
        let containers = p.data.ruby.containers.len() as u64;
        let exhausted: Vec<_> = cx
            .ruby_line_work_log
            .iter()
            .filter(|w| w.exhausted())
            .collect();
        assert_eq!(exhausted.len(), 1, "factor {factor}");
        let work = exhausted[0];
        assert!(
            work.spent() <= factor * work.extent() + work.walk() + containers,
            "factor {factor}: {work:?}"
        );
    }
}

/// Zero allows no adjustment-only measurement: every fit probe answers zero
/// and the call warns once. Accepted lines still place their ruby exactly,
/// so a line wide enough for any adjustment matches the unlimited one.
#[test]
fn zero_factor_fits_without_ruby_and_places_ruby_exactly() {
    let mut p = digit_siblings(8);
    let mut wide = |factor| {
        std::sync::Arc::get_mut(&mut p.data)
            .unwrap()
            .limits
            .max_ruby_line_work = factor;
        let mut cx = mode_context(Mode::Accumulate);
        let result = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(1.0e7),
            &AtomicSizes::EMPTY,
        );
        let warnings = cx.take_warnings();
        (result_signature(result), exceeded(&warnings))
    };
    let (unlimited, none) = wide(None);
    let (zero, once) = wide(Some(0));
    assert_eq!(zero, unlimited);
    assert_eq!((none, once), (0, 1));
}

/// A degraded call gives the same lines, sizes and warnings cold, in a
/// context warmed by earlier calls, and after a wider retained line of the
/// same token (`PartialLine::index`).
#[test]
fn degraded_layout_does_not_depend_on_earlier_calls() {
    let p = with_factor(profile_churn(48), Some(1));
    let options = LineOptions::default();
    let mut warm = mode_context(Mode::Accumulate);
    let mut warned = 0;
    for width in [96.0f32, 1.0e7, 300.0] {
        let mut cold = mode_context(Mode::Accumulate);
        let mut got = Vec::new();
        for cx in [&mut warm, &mut cold] {
            let lines = p.break_all(cx, &options, width, &AtomicSizes::EMPTY);
            let warnings = cx.take_warnings();
            warned += exceeded(&warnings);
            got.push((lines.iter().map(signature).collect::<Vec<_>>(), warnings));
        }
        assert_eq!(got[0], got[1], "break_all {width}");
        let mut cold = mode_context(Mode::Accumulate);
        let a = p.intrinsic_sizes(&mut warm, &options, &AtomicIntrinsics::EMPTY);
        let b = p.intrinsic_sizes(&mut cold, &options, &AtomicIntrinsics::EMPTY);
        assert_eq!(a, b, "intrinsic after {width}");
        assert_eq!(warm.take_warnings(), cold.take_warnings());
    }
    assert!(warned > 0, "the allowance must be exceeded");
    for (wide, narrow) in [(1.0e7f32, 300.0f32), (1.0e7, 96.0), (600.0, 300.0)] {
        let line = |cx: &mut LayoutContext, width: f32| {
            let result = p.next_line(
                cx,
                p.start_token(),
                &options,
                &LineConstraint::new(width),
                &AtomicSizes::EMPTY,
            );
            (result_signature(result), cx.take_warnings())
        };
        let mut retained = mode_context(Mode::Accumulate);
        line(&mut retained, wide);
        let mut cold = mode_context(Mode::Accumulate);
        assert_eq!(
            line(&mut retained, narrow),
            line(&mut cold, narrow),
            "{wide} -> {narrow}"
        );
    }
}

/// With the default limits, none of the equivalence fixtures (nested,
/// siblings, Arabic, atomics, hyphens, separators, first-line, ...) nor the
/// linear-guard shapes reach the allowance.
#[test]
fn default_allowance_is_not_reached_by_the_fixtures() {
    assert_eq!(Limits::default().max_ruby_line_work, Some(16));
    for fixture in fixtures() {
        let observed = observe_layout_in(&fixture, Mode::Accumulate);
        assert!(
            !observed.iter().any(|line| line.contains(WARNING)),
            "{}",
            fixture.name
        );
    }
    for p in [digit_siblings(256), profile_churn(32)] {
        let mut cx = mode_context(Mode::Accumulate);
        one_line(&p, &mut cx);
        assert!(!cx.ruby_line_work.exhausted());
        assert_eq!(exceeded(&cx.take_warnings()), 0);
    }
}

/// A walk wider than an accumulator keeps is measured once; the next probe
/// of the operation is refused (the through memo would measure the whole
/// walk again at every step). An operation with a single wide probe stays
/// exact.
#[test]
fn one_wide_walk_is_measured_and_the_next_probe_refused() {
    let p = digit_siblings(16);
    let mut capped = mode_context(Mode::Accumulate);
    capped.ruby_accumulate_cap = Some(4);
    one_line(&p, &mut capped);
    assert_eq!(exceeded(&capped.take_warnings()), 1);
    // Intrinsic sizes probe an unbreakable row once, at its end.
    let options = LineOptions::default();
    capped.ruby_accumulate_cap = Some(4);
    let narrow = p.intrinsic_sizes(&mut capped, &options, &AtomicIntrinsics::EMPTY);
    assert_eq!(exceeded(&capped.take_warnings()), 0);
    let mut wide = mode_context(Mode::Accumulate);
    assert_eq!(
        narrow,
        p.intrinsic_sizes(&mut wide, &options, &AtomicIntrinsics::EMPTY)
    );
}
