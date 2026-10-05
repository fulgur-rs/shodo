//! Container accumulator (shodo-2j6): characterization, oracles, operation
//! count guards and equivalence fixtures. Shares the shodo-d77 harness.
use super::memo_tests::*;
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::accumulate::Dirty;
use crate::ruby::*;
use crate::style::UnicodeBidi;
use crate::style::{InlineStyle, LineOptions, ParagraphStyle, TabSize, WhiteSpaceCollapse};
use crate::{AtomicSizes, LayoutContext, LineConstraint, Paragraph, ParagraphBuilder};
use std::ops::Range;

/// `r` sibling rubies over "12", each with the reading "日": one
/// unbreakable line (the shodo-d77 `siblings` probe shape).
pub(super) fn digit_siblings(r: usize) -> Paragraph {
    let default = Limits::default();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    for i in 0..r as u64 {
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![base_text(3000 + i, "12", &style(24.0), &default)],
                &["日"],
                RubyOverhang::None,
                &default,
            ),
        );
    }
    finish(b)
}

/// An outer ruby whose single base holds `r` sibling rubies over "12", each
/// after a plain "日", under one long breakable reading: the outer container
/// has paired cuts inside its base, so the look-ahead endpoint moves with
/// every inner ruby while the outer container stays clipped.
pub(super) fn outer_siblings(r: usize) -> Paragraph {
    let default = Limits::default();
    let mut base = ParagraphBuilder::new(&paragraph_style(false), &default);
    for i in 0..r as u64 {
        base.push_text(
            TextSource::Generated {
                node: NodeId(2000 + i),
            },
            "日",
        );
        base.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![base_text(3000 + i, "12", &style(24.0), &default)],
                &["日"],
                RubyOverhang::None,
                &default,
            ),
        );
    }
    let reading = "にほ".repeat(r);
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![RubyContent::from_builder(base)],
            &[reading.as_str()],
            RubyOverhang::None,
            &default,
        ),
    );
    finish(b)
}

/// `next_line` at a width every candidate fits: one scan over the paragraph.
pub(super) fn one_line(p: &Paragraph, cx: &mut LayoutContext) {
    let _ = p.next_line(
        cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1.0e7),
        &AtomicSizes::EMPTY,
    );
}

pub(super) fn sibling_measures(r: usize, mode: Mode) -> usize {
    let mut cx = mode_context(mode);
    digit_siblings(r).break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
    cx.ruby_container_measures
}

pub(super) fn outer_measures(r: usize, mode: Mode) -> usize {
    let mut cx = mode_context(mode);
    one_line(&outer_siblings(r), &mut cx);
    cx.ruby_container_measures
}

/// Values at each size and the growth factor of each doubling.
pub(super) fn doubling(
    measure: impl Fn(usize) -> usize,
    sizes: [usize; 3],
) -> (Vec<usize>, Vec<f64>) {
    let all: Vec<_> = sizes.iter().map(|r| measure(*r)).collect();
    let growth = all
        .windows(2)
        .map(|pair| pair[1] as f64 / pair[0].max(1) as f64)
        .collect();
    (all, growth)
}

#[test]
fn outer_siblings_have_paired_cuts_inside_the_outer_base() {
    let r = 16;
    let p = outer_siblings(r);
    assert_eq!(p.data.ruby.containers.len(), r + 1);
    let outer = &p.data.ruby.containers[0];
    assert!(outer.cuts.len() > r / 2, "{} cuts", outer.cuts.len());
}

/// The container work the accumulator removes: without it both shapes
/// measure every visited container again at every look-ahead endpoint.
#[test]
fn sibling_container_measures_are_quadratic_without_the_accumulator() {
    for mode in [Mode::Reference, Mode::Memo] {
        let (all, growth) = doubling(|r| sibling_measures(r, mode), [16, 32, 64]);
        assert!(
            growth.iter().all(|g| *g >= 3.0),
            "siblings {mode:?}: {growth:?} {all:?}"
        );
        let (all, growth) = doubling(|r| outer_measures(r, mode), [8, 16, 32]);
        assert!(
            growth.iter().all(|g| *g >= 3.0),
            "outer {mode:?}: {growth:?} {all:?}"
        );
    }
}

/// No completed descendants: a container measured alone.
pub(super) struct NoDescendants;

impl crate::ruby::measure::Descendants for NoDescendants {
    fn add_adjustments(
        &self,
        _: &Range<usize>,
        width: LayoutUnit,
        _: &mut LayoutContext,
        _: &mut Saturation,
    ) -> LayoutUnit {
        width
    }

    fn union_areas(
        &self,
        _: &Range<usize>,
        area: crate::ruby::geometry::Bounds,
        _: &mut LayoutContext,
    ) -> crate::ruby::geometry::Bounds {
        area
    }

    fn any_content(&self, _: &Range<usize>) -> bool {
        false
    }
}

/// Profile charges of a detached share count for the frame below the
/// container recording, so the container's own effects are empty and the
/// enclosing recording holds `calls` copies of the profile.
#[test]
fn detached_share_keeps_profile_charges_out_of_container_effects() {
    use crate::line::metric_index::{ProfileShare, content_shared};
    use crate::line::replay;
    let p = arabic(&limits(Some(64), None), Direction::Ltr);
    let data = &p.data;
    let n = data.units.len();
    let mut charged = false;
    for end in 2..=n {
        let selected = 1..end;
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
        let mut sat = Saturation::default();
        let empty = {
            let recording = replay::begin(&mut cx, &sat);
            replay::finish(&mut cx, recording, &sat).unwrap()
        };
        let outer = replay::begin(&mut cx, &sat);
        let mut share = ProfileShare::detached();
        let mut owns = Vec::new();
        for _ in 0..2 {
            share.begin_container(replay::depth(&cx).checked_sub(1));
            let recording = replay::begin(&mut cx, &sat);
            for _ in 0..2 {
                content_shared(
                    data,
                    selected.clone(),
                    std::slice::from_ref(&selected),
                    &[None],
                    &AtomicSizes::EMPTY,
                    &mut share,
                    &mut cx,
                    &mut sat,
                );
            }
            let own = replay::finish(&mut cx, recording, &sat).unwrap();
            let note = share.note();
            assert_eq!(note.calls, 2, "{selected:?}");
            owns.push(own.without_sat(note.sat));
        }
        let total = replay::finish(&mut cx, outer, &sat).unwrap();
        let profile = share.effects().unwrap();
        assert!(!share.refreshed(), "{selected:?}");
        assert_eq!(owns, vec![empty, empty], "{selected:?}");
        assert_eq!(Some(total), empty.then(profile.times(4)), "{selected:?}");
        charged |= profile.bytes() > 0;
    }
    assert!(charged, "some selected range must charge edge windows");
}

/// A container note records whether the container read a top/bottom group
/// and which visual neighbours its allowances used.
#[test]
fn container_notes_record_profile_groups_and_neighbours() {
    use crate::line::metric_index::ProfileShare;
    let note = |p: &Paragraph| {
        let data = &p.data;
        let selected = 0..data.units.len();
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
        let mut share = ProfileShare::detached();
        share.begin_container(None);
        let fragment = crate::ruby::measure::measure_one(
            data,
            &selected,
            0,
            &NoDescendants,
            &mut share,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        )
        .unwrap();
        (fragment.units, share.note())
    };
    let (units, plain) = note(&overhang(&Limits::default()));
    assert!(plain.neighbour_dependent, "{plain:?}");
    let [before, after] = plain.neighbours;
    assert!(
        before.is_some_and(|u| u < units.start) && after.is_some_and(|u| u >= units.end),
        "{plain:?} {units:?}"
    );
    assert!(!plain.profile && plain.calls > 0, "{plain:?}");
    let (_, grouped) = note(&vertical_align(&Limits::default()));
    assert!(grouped.profile, "{grouped:?}");
    assert!(!grouped.neighbour_dependent, "{grouped:?}");
}

#[test]
// Removed ranges are a list; a single range in it is intended.
#[allow(clippy::single_range_in_vec_init)]
fn selection_digest_differences_name_their_source_ranges() {
    use crate::line::metric_index::SelectionDigest;
    let base = SelectionDigest {
        height: 1,
        above: 2,
        partial: vec![(0..4, 7, 8)],
        removed: vec![10..12],
        replacements: vec![(11, [Some((1, 2)), None, None, Some((1, 2))])],
    };
    assert!(base.changed_ranges(&base).is_empty());
    assert!(!base.profile_changed(&base));
    let moved = SelectionDigest {
        above: 3,
        ..base.clone()
    };
    assert!(moved.profile_changed(&base));
    assert!(moved.changed_ranges(&base).is_empty());
    let sorted = |mut ranges: Vec<Range<usize>>| {
        ranges.sort_by_key(|r| (r.start, r.end));
        ranges
    };
    let regrouped = SelectionDigest {
        partial: vec![(0..4, 7, 9), (20..24, 1, 1)],
        ..base.clone()
    };
    assert_eq!(
        sorted(regrouped.changed_ranges(&base)),
        vec![0..4, 0..4, 20..24]
    );
    let rewindowed = SelectionDigest {
        removed: vec![12..14],
        replacements: vec![],
        ..base.clone()
    };
    assert_eq!(
        sorted(rewindowed.changed_ranges(&base)),
        vec![10..12, 11..12, 12..14]
    );
}

#[test]
fn detached_profiles_digest_partial_groups() {
    use crate::line::metric_index::{ProfileShare, content_shared};
    let p = vertical_align(&Limits::default());
    let data = &p.data;
    let n = data.units.len();
    let digest = |selected: Range<usize>| {
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
        let mut share = ProfileShare::detached();
        share.begin_container(None);
        content_shared(
            data,
            selected.clone(),
            std::slice::from_ref(&selected),
            &[None],
            &AtomicSizes::EMPTY,
            &mut share,
            &mut cx,
            &mut Saturation::default(),
        );
        share.digest().unwrap()
    };
    let whole = digest(0..n);
    assert!(whole.partial.is_empty(), "{whole:?}");
    let clipped: Vec<_> = (1..n)
        .map(|end| digest(0..end))
        .filter(|d| !d.partial.is_empty())
        .collect();
    assert!(!clipped.is_empty(), "some end must clip a top/bottom group");
    assert!(clipped.iter().all(|d| !d.changed_ranges(&whole).is_empty()));
}

/// The carried contract of a detached share: a container's own effects plus
/// `calls` copies of the profile effects, replayed from any `spent`, leave
/// exactly the reference path's `edge_reshape_spent`, warnings and
/// Saturation, or refuse and change nothing. Next to the reshape budget the
/// gate accepts exactly while measuring afresh stays within it.
#[test]
fn detached_container_with_replayed_profile_matches_the_reference() {
    use crate::line::metric_index::ProfileShare;
    use crate::line::replay;
    // `max_reshape_window_bytes` times `EDGE_RESHAPE_LINE_WINDOWS` (64).
    let limit = 64 * 64;
    let p = arabic(&limits(Some(64), None), Direction::Ltr);
    let data = &p.data;
    let n = data.units.len();
    // Measure container 0 on the reference path from `spent`, returning its
    // effects (if clean), and the context's spent, warnings and Saturation.
    let reference = |selected: &Range<usize>, spent: u64| {
        let mut cx = mode_context(Mode::Reference);
        cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
        cx.edge_reshape_spent = spent;
        let mut sat = Saturation::default();
        let recording = replay::begin(&mut cx, &sat);
        let fragment = crate::ruby::measure::measure_one(
            data,
            selected,
            0,
            &NoDescendants,
            &mut ProfileShare::default(),
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        let effects = replay::finish(&mut cx, recording, &sat);
        let warnings = cx.warnings.as_slice().len();
        (
            fragment.is_some(),
            effects,
            cx.edge_reshape_spent,
            warnings,
            sat,
        )
    };
    let mut charged = false;
    let mut near_budget = false;
    let mut own_charged = false;
    for end in 2..=n {
        let selected = 1..end;
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
        let mut sat = Saturation::default();
        let outer = replay::begin(&mut cx, &sat);
        let mut share = ProfileShare::detached();
        share.begin_container(replay::depth(&cx).checked_sub(1));
        let recording = replay::begin(&mut cx, &sat);
        let fragment = crate::ruby::measure::measure_one(
            data,
            &selected,
            0,
            &NoDescendants,
            &mut share,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut sat,
        );
        let own = replay::finish(&mut cx, recording, &sat).unwrap();
        let total = replay::finish(&mut cx, outer, &sat).unwrap();
        if fragment.is_none() {
            continue;
        }
        let note = share.note();
        assert!(!share.refreshed(), "{selected:?}");
        let Some(profile) = share.effects() else {
            assert_eq!(note.calls, 0, "{selected:?}");
            continue;
        };
        let composed = own
            .without_sat(note.sat)
            .then(profile.times(u64::from(note.calls)))
            .unwrap();
        // The enclosing recording holds the profile charges exactly once per
        // call, so it equals the composition.
        assert_eq!(total, composed, "{selected:?}");
        let (_, recorded, ..) = reference(&selected, 0);
        assert_eq!(recorded, Some(composed), "{selected:?}");
        charged |= profile.bytes() > 0;
        own_charged |= own.bytes() > 0;
        let bytes = composed.bytes();
        assert!(bytes <= limit, "{selected:?} {bytes}");
        let mut spents = vec![0, limit - bytes];
        if bytes > 0 {
            spents.push(limit - bytes + 1);
        }
        for spent in spents {
            let (_, _, ref_spent, ref_warnings, ref_sat) = reference(&selected, spent);
            let mut replay_cx = LayoutContext::new();
            replay_cx.edge_reshape_spent = spent;
            let mut replay_sat = Saturation::default();
            let replayed = replay::replay(&mut replay_cx, &composed, &mut replay_sat);
            if replayed {
                assert_eq!(ref_warnings, 0, "{selected:?} {spent}");
                assert_eq!(replay_cx.warnings.as_slice().len(), 0);
                assert_eq!(
                    replay_cx.edge_reshape_spent, ref_spent,
                    "{selected:?} {spent}"
                );
                assert_eq!(replay_sat, ref_sat, "{selected:?} {spent}");
            } else {
                assert_eq!(replay_cx.edge_reshape_spent, spent, "{selected:?} {spent}");
                assert_eq!(replay_sat, Saturation::default(), "{selected:?} {spent}");
            }
            if spent == limit - bytes {
                // Measuring afresh ends exactly at the limit: accepted.
                assert!(replayed, "{selected:?} {spent}");
            } else if spent > limit - bytes {
                // One byte more: measuring afresh crosses the limit and
                // warns, so the composition must refuse.
                assert!(ref_warnings > 0 && !replayed, "{selected:?} {spent}");
                near_budget = true;
            }
        }
    }
    assert!(charged, "some selected range must charge edge windows");
    assert!(near_budget, "some selected range must reach the budget");
    assert!(
        own_charged,
        "some container must charge its own edge windows"
    );
}

/// `r` sibling rubies over `base` with one `reading` each, and `between`
/// plain text before every ruby and after the last (none when empty).
#[allow(clippy::too_many_arguments)]
pub(super) fn row(
    paragraph: &ParagraphStyle,
    r: usize,
    base: &str,
    reading: &str,
    between: &str,
    overhang: RubyOverhang,
    style: &InlineStyle,
    limits: &Limits,
) -> Paragraph {
    let mut b = ParagraphBuilder::new(paragraph, limits);
    let text = |b: &mut ParagraphBuilder, node: u64| {
        if !between.is_empty() {
            b.push_text(TextSource::Generated { node: NodeId(node) }, between);
        }
    };
    for i in 0..r as u64 {
        text(&mut b, 2000 + i);
        b.push_ruby(
            NodeId(1000 + i),
            style,
            annotated(
                vec![base_text(3000 + i, base, style, limits)],
                &[reading],
                overhang,
                limits,
            ),
        );
    }
    text(&mut b, 1999);
    finish(b)
}

/// Cursive bases and cursive text between them: edge windows are unsafe and
/// charge the reshape budget; overhang `Auto` reads the neighbours.
pub(super) fn cursive_siblings(limits: &Limits) -> Paragraph {
    row(
        &paragraph_style(false),
        4,
        "بببب",
        "に",
        "ببب",
        RubyOverhang::Auto,
        &style(24.0),
        limits,
    )
}

/// Preserved tabs between siblings: width queries for different starts
/// replace the tab prefix (a `RangeCache` invalidation).
pub(super) fn tab_siblings(limits: &Limits) -> Paragraph {
    let tab = InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..style(24.0)
    };
    row(
        &ParagraphStyle {
            root: tab.clone(),
            ..paragraph_style(false)
        },
        4,
        "日本",
        "にほんご",
        "\t",
        RubyOverhang::None,
        &tab,
        limits,
    )
}

/// Siblings whose odd bases hold an atomic inline without a caller size:
/// their measurements warn (`MissingAtomicSize`) until the sink suppresses.
pub(super) fn atomic_siblings(limits: &Limits) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    for i in 0..6u64 {
        let mut base = ParagraphBuilder::new(&paragraph_style(false), limits);
        base.push_text(
            TextSource::Generated {
                node: NodeId(3000 + i),
            },
            "日",
        );
        if i % 2 == 1 {
            base.push_atomic(NodeId(90 + i), &style(24.0), Default::default());
        }
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![RubyContent::from_builder(base)],
                &["にほんご"],
                RubyOverhang::None,
                limits,
            ),
        );
    }
    finish(b)
}

/// Siblings with 1e6 px readings: each adjustment is about 6.4e7 layout
/// units, so the candidate's running total crosses `i32::MAX`.
pub(super) fn huge_siblings(r: usize) -> Paragraph {
    let default = Limits::default();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    for i in 0..r as u64 {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: base_text(3000 + i, "12", &style(24.0), &default),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: base_text(4000 + i, "日", &style(1.0e6), &default),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        b.push_ruby(NodeId(1000 + i), &style(24.0), ruby);
    }
    finish(b)
}

/// Growing sweeps of `candidate_adjustment` from `start` in one operation:
/// values, Saturation, spent bytes, oracle mismatches and counters.
type Sweep = (Vec<LayoutUnit>, Saturation, u64, Vec<String>, Counters);

pub(super) fn sweep_from(p: &Paragraph, starts: &[usize], mode: Mode) -> Sweep {
    sweep_in(p, starts, &mut mode_context(mode))
}

/// `sweep_from` on a prepared context.
pub(super) fn sweep_in(p: &Paragraph, starts: &[usize], cx: &mut LayoutContext) -> Sweep {
    let data = &p.data;
    let n = data.units.len();
    let mut sat = Saturation::default();
    cx.begin_reshape_operation();
    let mut values = Vec::new();
    for end in 1..=n {
        for &start in starts {
            if start < end {
                values.push(crate::ruby::measure::candidate_adjustment(
                    data,
                    start,
                    end,
                    &AtomicSizes::EMPTY,
                    cx,
                    &mut sat,
                ));
                assert!(
                    cx.ruby_memo.accumulator_keys().len()
                        <= crate::ruby::accumulate::MAX_ACCUMULATORS
                );
            }
        }
    }
    let misses = std::mem::take(&mut cx.ruby_oracle_misses);
    (values, sat, cx.edge_reshape_spent, misses, counters(cx))
}

#[test]
fn accumulator_matches_reference_on_shodo_d77_fixtures() {
    for fixture in fixtures() {
        for pre in pre_states(&fixture.paragraph) {
            let (reference, _) =
                observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Reference, pre);
            for mode in [Mode::Memo, Mode::Verify] {
                let (observed, _) =
                    observe_candidates_in(&fixture.paragraph, &fixture.atomics, mode, pre);
                assert_eq!(observed, reference, "{} {mode:?} {pre:?}", fixture.name);
            }
        }
        assert_eq!(
            observe_layout_in(&fixture, Mode::Verify),
            observe_layout_in(&fixture, Mode::Reference),
            "{}",
            fixture.name
        );
    }
    let all = fixtures();
    assert_eq!(
        observe_warm_in(&all, Mode::Verify),
        observe_warm_in(&all, Mode::Reference)
    );
}

/// Each step measures every position at most once, so the accumulator never
/// does more container work than the through memo; clean runs do replay.
#[test]
fn accumulator_never_measures_more_containers_than_the_memo() {
    for r in [8, 16, 32] {
        assert!(
            sibling_measures(r, Mode::Accumulate) <= sibling_measures(r, Mode::Memo),
            "siblings {r}"
        );
        assert!(
            outer_measures(r, Mode::Accumulate) <= outer_measures(r, Mode::Memo),
            "outer {r}"
        );
    }
    let mut cx = mode_context(Mode::Accumulate);
    digit_siblings(16).break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
    assert!(counters(&cx).replayed > 0, "{:?}", counters(&cx));
}

/// Review focus 1: a profile replay that crosses the budget re-measures the
/// profile mid-step; the step finishes live and the accumulator resets.
#[test]
fn profile_measured_again_mid_step_resets_and_matches_reference() {
    let window = 8;
    let limit = window * RESHAPE_WINDOWS;
    let p = cursive_siblings(&limits(Some(window), None));
    let mut resets = 0;
    for spent in (limit - 96..=limit + 1).step_by(4) {
        let pre = PreState {
            spent,
            suppressed: false,
        };
        let (reference, _) = observe_candidates_in(&p, &AtomicSizes::EMPTY, Mode::Reference, pre);
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, counters) = observe_candidates_in(&p, &AtomicSizes::EMPTY, mode, pre);
            assert_eq!(observed, reference, "{mode:?} spent {spent}");
            resets += counters.resets;
        }
    }
    // The sweeps above charge far more than the budget in single-container
    // probes before any multi-container step runs, so the crossing rarely
    // lands inside a step. Fresh single probes `0..end` started just below
    // the limit put it inside the step's profile measurement (which warns,
    // so the step stores no entry) or a later profile replay (refused and
    // measured again); then the probes continue past exhaustion.
    let data = &p.data;
    let n = data.units.len();
    let key = crate::ruby::accumulate::AccumulatorKey::new(data, &AtomicSizes::EMPTY, 0);
    let probe = |mode: Mode, spent: u64, end: usize| {
        let mut cx = mode_context(mode);
        cx.warnings.set_max(data.limits.max_warnings);
        cx.begin_reshape_operation();
        cx.edge_reshape_spent = spent;
        let mut sat = Saturation::default();
        let mut values = Vec::new();
        let mut emptied = true;
        for end in [end, n, end] {
            values.push(crate::ruby::measure::candidate_adjustment(
                data,
                0,
                end,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut sat,
            ));
            if cx.ruby_accumulator_resets > 0 && values.len() == 1 {
                emptied = cx.ruby_memo.accumulator(key).is_none_or(|a| a.len() == 0);
            }
        }
        let observed = (
            values,
            sat,
            cx.edge_reshape_spent,
            cx.take_warnings(),
            std::mem::take(&mut cx.ruby_oracle_misses),
        );
        (observed, counters(&cx), emptied)
    };
    let mut refusals = 0;
    for end in 1..=n {
        for spent in (limit - 200..=limit + 1).step_by(3) {
            let (reference, ..) = probe(Mode::Reference, spent, end);
            for mode in [Mode::Accumulate, Mode::Verify] {
                let (observed, counters, emptied) = probe(mode, spent, end);
                assert_eq!(observed, reference, "{mode:?} spent {spent} end {end}");
                assert!(
                    emptied,
                    "a reset step keeps no entry: {mode:?} {spent} {end}"
                );
                resets += counters.resets;
                refusals += counters.refusals;
            }
        }
    }
    assert!(resets > 0, "some step must measure its profile twice");
    assert!(refusals > 0, "some profile replay must be refused");
}

/// Review focus 2: entries recorded while the sink was unsuppressed never
/// replay once a later sibling's warning suppresses it.
#[test]
fn suppression_flip_mid_scan_refuses_older_entries() {
    for warnings in [Some(1), Some(2), Some(5)] {
        let fixture = Fixture::new("atomic-siblings", atomic_siblings(&limits(None, warnings)));
        let pre = PreState {
            spent: 0,
            suppressed: false,
        };
        let (reference, _) = observe_candidates_in(
            &fixture.paragraph,
            &AtomicSizes::EMPTY,
            Mode::Reference,
            pre,
        );
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, _) =
                observe_candidates_in(&fixture.paragraph, &AtomicSizes::EMPTY, mode, pre);
            assert_eq!(observed, reference, "{warnings:?} {mode:?}");
        }
        assert_eq!(
            observe_layout_in(&fixture, Mode::Verify),
            observe_layout_in(&fixture, Mode::Reference),
            "{warnings:?}"
        );
    }
}

/// Review focus 3: a tab prefix replaced while a container is recorded moves
/// the cache epoch; the step stops replaying and the accumulator resets.
#[test]
fn tab_prefix_replacement_mid_step_stops_replay() {
    let fixture = Fixture::new("tab-siblings", tab_siblings(&Limits::default()));
    let pre = PreState {
        spent: 0,
        suppressed: false,
    };
    let (reference, _) = observe_candidates_in(
        &fixture.paragraph,
        &AtomicSizes::EMPTY,
        Mode::Reference,
        pre,
    );
    let (observed, counters) = observe_candidates_in(
        &fixture.paragraph,
        &AtomicSizes::EMPTY,
        Mode::Accumulate,
        pre,
    );
    assert_eq!(observed, reference);
    assert!(counters.resets > 0, "{counters:?}");
    assert_eq!(
        observe_layout_in(&fixture, Mode::Accumulate),
        observe_layout_in(&fixture, Mode::Reference)
    );
}

/// Review focus 4: a replayed run whose running total would saturate is
/// added one value at a time, counting every saturation.
#[test]
fn huge_readings_saturate_like_reference() {
    let p = huge_siblings(40);
    let reference = sweep_from(&p, &[0], Mode::Reference);
    assert!(
        reference.1.saturated > 0,
        "the readings must saturate the total"
    );
    for mode in [Mode::Memo, Mode::Accumulate, Mode::Verify] {
        let observed = sweep_from(&p, &[0], mode);
        assert_eq!(
            (&observed.0, observed.1, observed.2, &observed.3),
            (&reference.0, reference.1, reference.2, &reference.3),
            "{mode:?}"
        );
        if mode == Mode::Accumulate {
            assert!(observed.4.replayed > 0, "{:?}", observed.4);
        }
    }
    let fixture = Fixture::new("huge", huge_siblings(40));
    assert_eq!(
        observe_layout_in(&fixture, Mode::Accumulate),
        observe_layout_in(&fixture, Mode::Reference)
    );
}

/// Review focus 5: three starts alternate in one operation; the least
/// recently used accumulator is evicted and restarted exactly.
#[test]
fn three_keys_alternating_evict_and_restart_exactly() {
    let p = digit_siblings(12);
    let containers = &p.data.ruby.containers;
    let starts = [0, containers[2].units.start, containers[5].units.start];
    let reference = sweep_from(&p, &starts, Mode::Reference);
    for mode in [Mode::Accumulate, Mode::Verify] {
        let observed = sweep_from(&p, &starts, mode);
        assert_eq!(
            (&observed.0, observed.1, observed.2, &observed.3),
            (&reference.0, reference.1, reference.2, &reference.3),
            "{mode:?}"
        );
    }
}

/// Carried (review of Task 4): replayed containers that made several profile
/// calls (allowances read neighbour bounds through the shared profile)
/// replay `own + m * P` with a profile that charges reshape bytes. Every
/// probe is asked twice: an exact probe is not memoized, so the second ask
/// reaches the accumulator with the same `through` and replays.
#[test]
fn repeated_profile_calls_replay_with_their_charges() {
    let window = 8;
    let limit = window * RESHAPE_WINDOWS;
    let p = row(
        &paragraph_style(false),
        4,
        "ب",
        "にほんご",
        "ببب",
        RubyOverhang::Auto,
        &style(24.0),
        &limits(Some(window), None),
    );
    let data = &p.data;
    let n = data.units.len();
    let run = |mode: Mode, pre: PreState| {
        let mut cx = mode_context(mode);
        if pre.suppressed {
            cx.warnings.set_max(Some(0));
            cx.warnings
                .push(crate::limits::WarningKind::Unsupported, "pre-existing");
        } else {
            cx.warnings.set_max(data.limits.max_warnings);
        }
        cx.begin_reshape_operation();
        cx.edge_reshape_spent = pre.spent;
        let mut sat = Saturation::default();
        let mut values = Vec::new();
        for start in 0..n {
            for end in start + 1..=n {
                for _ in 0..2 {
                    values.push(crate::ruby::measure::candidate_adjustment(
                        data,
                        start,
                        end,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut sat,
                    ));
                }
            }
        }
        let observed = (
            values,
            sat,
            cx.edge_reshape_spent,
            cx.take_warnings(),
            std::mem::take(&mut cx.ruby_oracle_misses),
        );
        (observed, cx.ruby_repeated_profile_replays)
    };
    let mut repeated = 0;
    for spent in [0, limit - 16, limit + 1] {
        for suppressed in [false, true] {
            let pre = PreState { spent, suppressed };
            let (reference, _) = run(Mode::Reference, pre);
            for mode in [Mode::Memo, Mode::Accumulate, Mode::Verify] {
                let (observed, replays) = run(mode, pre);
                assert_eq!(observed, reference, "{mode:?} {pre:?}");
                if mode == Mode::Accumulate {
                    repeated += replays;
                }
            }
        }
    }
    assert!(
        repeated > 0,
        "some replayed run must repeat a charging profile"
    );
}

/// Carried (review of Task 5): a walk longer than the accumulator cap
/// releases the accumulator and measures on the through-memo path, exactly.
#[test]
fn walks_beyond_the_cap_fall_back_to_the_memo_path() {
    let p = digit_siblings(12);
    let key = crate::ruby::accumulate::AccumulatorKey::new(&p.data, &AtomicSizes::EMPTY, 0);
    let reference = sweep_from(&p, &[0], Mode::Reference);
    let memo = sweep_from(&p, &[0], Mode::Memo);
    for cap in [0, 4] {
        for mode in [Mode::Accumulate, Mode::Verify] {
            let mut cx = mode_context(mode);
            cx.ruby_accumulate_cap = Some(cap);
            let observed = sweep_in(&p, &[0], &mut cx);
            assert_eq!(
                (&observed.0, observed.1, observed.2, &observed.3),
                (&reference.0, reference.1, reference.2, &reference.3),
                "cap {cap} {mode:?}"
            );
            // The last walks exceed the cap: their accumulator is gone.
            assert!(cx.ruby_memo.accumulator(key).is_none(), "cap {cap}");
            if cap == 0 {
                // Never used: exactly the through-memo path's work.
                assert_eq!(observed.4.measures, memo.4.measures);
                assert_eq!(observed.4.replayed, 0);
                assert_eq!(observed.4.dirty, [0; 8]);
            }
        }
    }
}

/// Carried (review of Task 5): when the segment tree's prefix guard refuses
/// a replayed run's total, its adjustments are added one by one in the
/// reference order, so values and Saturation counts stay identical.
#[test]
fn saturating_replayed_runs_add_one_value_at_a_time() {
    let p = huge_siblings(40);
    let reference = sweep_from(&p, &[0], Mode::Reference);
    let mut cx = mode_context(Mode::Accumulate);
    let observed = sweep_in(&p, &[0], &mut cx);
    assert_eq!(
        (&observed.0, observed.1, observed.2, &observed.3),
        (&reference.0, reference.1, reference.2, &reference.3)
    );
    assert!(cx.ruby_sequential_replays > 0, "{:?}", observed.4);
}

/// Tab prefixes replaced for alternating starts move the cache epoch inside
/// and between steps; every surviving accumulator holds entries of the
/// current epoch only, and the values match the reference.
#[test]
fn alternating_tab_starts_keep_entries_of_the_current_epoch() {
    let p = tab_siblings(&Limits::default());
    let containers = &p.data.ruby.containers;
    let starts = [0, containers[1].units.start, containers[2].units.start];
    let reference = sweep_from(&p, &starts, Mode::Reference);
    for mode in [Mode::Accumulate, Mode::Verify] {
        let mut cx = mode_context(mode);
        let epoch = cx.ruby_ranges.epoch();
        let observed = sweep_in(&p, &starts, &mut cx);
        assert_eq!(
            (&observed.0, observed.1, observed.2, &observed.3),
            (&reference.0, reference.1, reference.2, &reference.3),
            "{mode:?}"
        );
        assert!(
            cx.ruby_ranges.epoch() != epoch,
            "the starts must replace the tab prefix"
        );
        // Every surviving accumulator was recorded under the current epoch.
        for key in cx.ruby_memo.accumulator_keys().into_iter().flatten() {
            let acc = cx.ruby_memo.accumulator(key).unwrap();
            assert!(
                acc.len() == 0 || acc.epoch == cx.ruby_ranges.epoch(),
                "{mode:?}"
            );
        }
    }
}

/// The step oracle is not vacuous: under `Mode::Verify` clean entries that
/// would have replayed are measured live and compared.
#[test]
fn step_oracle_compares_entries() {
    let mut checks = 0;
    for p in [digit_siblings(16), huge_siblings(12)] {
        let mut cx = mode_context(Mode::Verify);
        let (.., misses, _) = sweep_in(&p, &[0], &mut cx);
        assert!(misses.is_empty(), "{misses:?}");
        checks += cx.ruby_oracle_checks;
        let mut cx = mode_context(Mode::Verify);
        p.break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
        assert!(
            cx.ruby_oracle_misses.is_empty(),
            "{:?}",
            cx.ruby_oracle_misses
        );
        checks += cx.ruby_oracle_checks;
    }
    assert!(checks > 0);
}

/// Carried (review of Task 5): a cache invalidation between two steps for
/// the same start and `through` moves the epoch; taking the accumulator back
/// finds entries of another epoch and resets it, so nothing replays.
#[test]
fn epoch_moved_between_steps_resets_on_take() {
    let p = digit_siblings(8);
    let data = &p.data;
    let n = data.units.len();
    let run = |mode: Mode, invalidate: bool| {
        let mut cx = mode_context(mode);
        cx.begin_reshape_operation();
        let mut sat = Saturation::default();
        let mut values = Vec::new();
        // The first ask fills the caches (its entries are not stored), the
        // second records entries against warm caches, the third may replay.
        for i in 0..3 {
            if i == 2 && invalidate {
                cx.ruby_ranges.vacate_slots();
            }
            // An exact probe (`through == end`) is not memoized: every ask
            // reaches the accumulator with the same `through`.
            values.push(crate::ruby::measure::candidate_adjustment(
                data,
                0,
                n,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut sat,
            ));
        }
        let observed = (
            values,
            sat,
            cx.edge_reshape_spent,
            cx.take_warnings(),
            std::mem::take(&mut cx.ruby_oracle_misses),
        );
        (observed, counters(&cx))
    };
    for invalidate in [false, true] {
        let (reference, _) = run(Mode::Reference, invalidate);
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, counters) = run(mode, invalidate);
            assert_eq!(observed, reference, "{mode:?} {invalidate}");
            if mode == Mode::Accumulate {
                // Without the invalidation the third ask replays everything
                // but its top position; with it, nothing replays.
                assert_eq!(counters.replayed > 0, !invalidate, "{counters:?}");
            }
        }
    }
}

/// Brute force of the claim D3 relies on, over UAX #9 reorderings of fixed
/// levels: when `start..old` grows to `start..new`, a target's recorded side
/// `s` neighbour changes only to a newly selected event unit, and its old
/// value (`None` included) is side `s` of the visual neighbours of some new
/// event unit. Targets without event units and visually split targets are
/// included. Returns the number of side changes seen.
fn check_d3_premise(seed: u64, cases: usize, max_len: u64, max_level: u64) -> usize {
    use crate::line::spacing_summary::VisualNeighbors;
    let mut seed = seed;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut changes = 0;
    let mut check = |levels: &[u8], events: &[bool], start: usize| {
        let len = levels.len();
        let index = VisualNeighbors::new(levels.iter().zip(events).map(|(l, e)| (*l, *e)));
        for old in start + 1..len {
            for new in old + 1..=len {
                for rtl in [false, true] {
                    let mut sides: [Vec<Option<usize>>; 2] = [Vec::new(), Vec::new()];
                    for x in (old..new).filter(|x| events[*x]) {
                        let (before, after) = index.around(&(start..new), &(x..x + 1), rtl);
                        sides[0].push(before);
                        sides[1].push(after);
                    }
                    for t0 in start..old {
                        for t1 in t0 + 1..=old {
                            let target = t0..t1;
                            let was = index.around(&(start..old), &target, rtl);
                            let now = index.around(&(start..new), &target, rtl);
                            for (side, (w, n)) in
                                [(was.0, now.0), (was.1, now.1)].into_iter().enumerate()
                            {
                                if w == n {
                                    continue;
                                }
                                changes += 1;
                                assert!(
                                    sides[side].contains(&w)
                                        && n.is_some_and(|u| (old..new).contains(&u) && events[u]),
                                    "levels {levels:?} events {events:?} {start}..{old}->{new} \
                                     target {target:?} rtl {rtl} side {side}: {was:?} -> {now:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    };
    // The shape that refutes "a changed target holds a neighbour of a new
    // event": target 2..4 is visually split and starts at non-event 3.
    check(&[2, 2, 2, 1, 1], &[true, true, true, false, true], 1);
    for _ in 0..cases {
        let len = 2 + (next() % (max_len - 1)) as usize;
        let levels: Vec<u8> = (0..len).map(|_| (next() % (max_level + 1)) as u8).collect();
        let events: Vec<bool> = (0..len).map(|_| next() % 3 != 0).collect();
        let start = (next() as usize) % (len - 1);
        check(&levels, &events, start);
    }
    changes
}

#[test]
fn growing_selections_change_recorded_neighbours_only_through_new_event_units() {
    let changes = check_d3_premise(0x51f1_5e3d_2b1a_9c07, 3000, 8, 3);
    assert!(changes > 0);
}

/// The larger sweep behind D3 (60 000 cases, lengths up to 10, levels up to
/// 4); run with `--ignored` (seconds in release, minutes in debug).
#[test]
#[ignore]
fn growing_selections_change_recorded_neighbours_only_through_new_event_units_sweep() {
    let changes = check_d3_premise(0x2545_f491_4f6c_dd1d, 60_000, 10, 4);
    assert!(changes > 0);
}

/// Sibling rubies whose readings are wider than their one-glyph bases, with
/// plain neighbours: overhang `Auto` reads the visual neighbours.
pub(super) fn overhang_siblings(paragraph: &ParagraphStyle) -> Paragraph {
    row(
        paragraph,
        4,
        "日",
        "にほんご",
        "本",
        RubyOverhang::Auto,
        &style(24.0),
        &Limits::default(),
    )
}

/// Overhang siblings separated by isolated Latin text in an RTL paragraph:
/// the visual neighbours come from bidi reordering.
pub(super) fn rtl_isolate_siblings() -> Paragraph {
    let limits = Limits::default();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            direction: Direction::Rtl,
            ..paragraph_style(false)
        },
        &limits,
    );
    for i in 0..4u64 {
        b.open_inline(
            NodeId(4000 + i),
            &InlineStyle {
                unicode_bidi: UnicodeBidi::Isolate,
                ..style(24.0)
            },
            Default::default(),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId(2000 + i),
            },
            "ab",
        );
        b.close_inline();
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![base_text(3000 + i, "日", &style(24.0), &limits)],
                &["にほんご"],
                RubyOverhang::Auto,
                &limits,
            ),
        );
    }
    b.push_text(TextSource::Generated { node: NodeId(1999) }, "cd");
    finish(b)
}

/// Overhang siblings with nothing between them: a ruby's trailing side is
/// read at the line edge (`None`) until the next ruby is selected, so D3
/// must find it through the recorded `None` key.
pub(super) fn adjacent_overhang_siblings(paragraph: &ParagraphStyle) -> Paragraph {
    row(
        paragraph,
        4,
        "日",
        "にほんご",
        "",
        RubyOverhang::Auto,
        &style(24.0),
        &Limits::default(),
    )
}

/// D3 against the reference and the step oracle on overhang siblings. With
/// plain text between the rubies no clean position's recorded neighbour
/// ever changes (the trailing text is selected while its ruby is still the
/// top position), so D3 marks nothing there; adjacent rubies need marks.
/// The new-event fallback is gone: no `Full` mark unless the profile digest
/// changed, which the isolates of `rtl_isolate_siblings` do (open partial
/// groups; D4-D5 replace that fallback in the next commit).
#[test]
fn neighbour_rule_matches_reference_on_overhang_siblings() {
    let rtl = ParagraphStyle {
        direction: Direction::Rtl,
        ..paragraph_style(false)
    };
    // (fixture, D3 must mark, digest stable on every step)
    let fixtures = [
        (
            Fixture::new(
                "overhang-siblings",
                overhang_siblings(&paragraph_style(false)),
            ),
            false,
            true,
        ),
        (
            Fixture::new("overhang-siblings-rtl", overhang_siblings(&rtl)),
            false,
            true,
        ),
        (
            Fixture::new("rtl-isolate-siblings", rtl_isolate_siblings()),
            false,
            false,
        ),
        (
            Fixture::new(
                "adjacent-overhang-siblings",
                adjacent_overhang_siblings(&paragraph_style(false)),
            ),
            true,
            true,
        ),
        (
            Fixture::new(
                "adjacent-overhang-siblings-rtl",
                adjacent_overhang_siblings(&rtl),
            ),
            true,
            true,
        ),
    ];
    for (fixture, marks, stable) in &fixtures {
        let mut dirty = [0; 8];
        for pre in pre_states(&fixture.paragraph) {
            let (reference, _) =
                observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Reference, pre);
            for mode in [Mode::Accumulate, Mode::Verify] {
                let (observed, counters) =
                    observe_candidates_in(&fixture.paragraph, &fixture.atomics, mode, pre);
                assert_eq!(observed, reference, "{} {mode:?} {pre:?}", fixture.name);
                for (sum, d) in dirty.iter_mut().zip(counters.dirty) {
                    *sum += d;
                }
            }
        }
        if *marks {
            assert!(
                dirty[Dirty::Neighbour as usize] > 0,
                "{}: D3 must mark positions {dirty:?}",
                fixture.name
            );
        }
        if *stable {
            assert_eq!(
                dirty[Dirty::Full as usize],
                0,
                "{}: {dirty:?}",
                fixture.name
            );
        }
        assert_eq!(
            observe_layout_in(fixture, Mode::Verify),
            observe_layout_in(fixture, Mode::Reference),
            "{}",
            fixture.name
        );
    }
}

/// With D1-D3 and D6 the sibling shapes measure each container a bounded
/// number of times per line: container measures grow linearly.
#[test]
fn sibling_container_measures_grow_linearly_with_the_accumulator() {
    let (all, growth) = doubling(|r| sibling_measures(r, Mode::Accumulate), [16, 32, 64]);
    assert!(
        growth.iter().all(|g| *g < 2.5),
        "siblings {growth:?} {all:?}"
    );
    let (all, growth) = doubling(|r| outer_measures(r, Mode::Accumulate), [16, 32, 64]);
    assert!(growth.iter().all(|g| *g < 2.5), "outer {growth:?} {all:?}");
}
