//! Container accumulator (shodo-2j6): characterization, oracles, operation
//! count guards and equivalence fixtures. Shares the shodo-d77 harness.
use super::memo_tests::*;
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::accumulate::Dirty;
use crate::ruby::*;
use crate::style::UnicodeBidi;
use crate::style::{
    InlineStyle, LineOptions, ParagraphStyle, TabSize, VerticalAlign, WhiteSpaceCollapse,
};
use crate::{AtomicSize, AtomicSizes, LayoutContext, LineConstraint, Paragraph, ParagraphBuilder};
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
/// replace the tab prefix. The steps stay clean, so the replacement does not
/// invalidate the `RangeCache` (see `effectful_tab_siblings` for the one that
/// does).
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
    // Sweeps check reuse exactness, not the work allowance (shodo-mc0).
    cx.ruby_line_work_disabled = true;
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

/// `white-space: pre` with a 40px tab size: tabs stay tabs.
pub(super) fn pre_style() -> InlineStyle {
    InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..style(24.0)
    }
}

/// `pre_style` with a tab size whose tab steps saturate.
pub(super) fn huge_pre_style() -> InlineStyle {
    InlineStyle {
        tab_size: TabSize::Px(1.0e12),
        ..pre_style()
    }
}

fn pre_paragraph_in(style: &InlineStyle) -> ParagraphStyle {
    ParagraphStyle {
        root: style.clone(),
        ..paragraph_style(false)
    }
}

/// As `nested`, under `white-space: pre` throughout, so a tab in `text`
/// is preserved (the shodo-d77 `nestedtab` probe used `normal`, which
/// collapses the tab to a space).
pub(super) fn pre_nested(depth: usize, text: &str) -> Paragraph {
    pre_nested_in(depth, text, &pre_style())
}

/// `pre_nested` with every style replaced by `style`.
pub(super) fn pre_nested_in(depth: usize, text: &str, style: &InlineStyle) -> Paragraph {
    let limits = Limits::default();
    let mut content = base_text(30, text, style, &limits);
    for level in 1..depth {
        let mut b = ParagraphBuilder::new(&pre_paragraph_in(style), &limits);
        b.push_ruby(
            NodeId(100 + level as u64),
            style,
            annotated(vec![content], &["に"], RubyOverhang::None, &limits),
        );
        content = RubyContent::from_builder(b);
    }
    let mut b = ParagraphBuilder::new(&pre_paragraph_in(style), &limits);
    b.push_ruby(
        NodeId(100),
        style,
        annotated(vec![content], &["に"], RubyOverhang::None, &limits),
    );
    finish(b)
}

/// `r` sibling rubies over `base` under `white-space: pre`, after an
/// optional leading text.
pub(super) fn pre_siblings(r: usize, lead: Option<&str>, base: &str) -> Paragraph {
    pre_siblings_in(r, lead, base, &pre_style())
}

/// `pre_siblings` with every style replaced by `style`.
pub(super) fn pre_siblings_in(
    r: usize,
    lead: Option<&str>,
    base: &str,
    style: &InlineStyle,
) -> Paragraph {
    let limits = Limits::default();
    let mut b = ParagraphBuilder::new(&pre_paragraph_in(style), &limits);
    if let Some(text) = lead {
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    }
    for i in 0..r as u64 {
        b.push_ruby(
            NodeId(1000 + i),
            style,
            annotated(
                vec![base_text(3000 + i, base, style, &limits)],
                &["日"],
                RubyOverhang::None,
                &limits,
            ),
        );
    }
    finish(b)
}

/// As `tab_siblings`, with a tab size whose conversion saturates: every tab
/// step charges saturation.
pub(super) fn effectful_tab_siblings(limits: &Limits) -> Paragraph {
    let tab = InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(1.0e12),
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

/// shodo-b7d: one preserved tab no longer moves the cache epoch, so the
/// memo and the accumulator keep their linear container work.
#[test]
fn preserved_tabs_keep_nested_measures_linear() {
    let measure = |depth: usize, mode: Mode, text: &str| {
        let mut cx = mode_context(mode);
        pre_nested(depth, text).break_all(
            &mut cx,
            &LineOptions::default(),
            96.0,
            &AtomicSizes::EMPTY,
        );
        (cx.ruby_container_measures, cx.ruby_ranges.epoch())
    };
    for mode in [Mode::Memo, Mode::Accumulate] {
        let (all, growth) = doubling(|d| measure(d, mode, "日\t日").0, [16, 32, 64]);
        assert!(
            growth.iter().all(|g| *g <= 2.2),
            "{mode:?}: {growth:?} {all:?}"
        );
        for depth in [16, 32] {
            assert_eq!(
                measure(depth, mode, "日\t日").1,
                measure(depth, mode, "日").1,
                "{mode:?} {depth}: tabs must not move the epoch"
            );
        }
    }
}

#[test]
fn preserved_tabs_keep_sibling_measures_linear() {
    for (lead, base) in [(Some("\t"), "12"), (None, "1\t2"), (Some("日\t"), "12")] {
        let measure = |r: usize| {
            let mut cx = mode_context(Mode::Accumulate);
            pre_siblings(r, lead, base).break_all(
                &mut cx,
                &LineOptions::default(),
                96.0,
                &AtomicSizes::EMPTY,
            );
            cx.ruby_container_measures
        };
        let (all, growth) = doubling(measure, [16, 32, 64]);
        assert!(
            growth.iter().all(|g| *g <= 2.2),
            "{lead:?} {base:?}: {growth:?} {all:?}"
        );
    }
}

/// shodo-tj5: tab steps that saturate (`tab-size: 1e12px`) no longer
/// invalidate the cache when another start replaces the tab prefix, so the
/// memo and the accumulator stay linear as with clean tabs (shodo-b7d left
/// this shape quadratic).
#[test]
fn saturating_tabs_keep_measures_linear() {
    let nested = |depth: usize, mode: Mode| {
        let mut cx = mode_context(mode);
        pre_nested_in(depth, "日\t日", &huge_pre_style()).break_all(
            &mut cx,
            &LineOptions::default(),
            96.0,
            &AtomicSizes::EMPTY,
        );
        (cx.ruby_container_measures, cx.ruby_ranges.epoch())
    };
    for mode in [Mode::Memo, Mode::Accumulate] {
        let (all, growth) = doubling(|d| nested(d, mode).0, [16, 32, 64]);
        assert!(
            growth.iter().all(|g| *g <= 2.2),
            "{mode:?}: {growth:?} {all:?}"
        );
        assert_eq!(nested(16, mode).1, nested(32, mode).1, "{mode:?}");
    }
    let siblings = |r: usize| {
        let mut cx = mode_context(Mode::Accumulate);
        pre_siblings_in(r, Some("\t"), "12", &huge_pre_style()).break_all(
            &mut cx,
            &LineOptions::default(),
            96.0,
            &AtomicSizes::EMPTY,
        );
        cx.ruby_container_measures
    };
    let (all, growth) = doubling(siblings, [16, 32, 64]);
    assert!(growth.iter().all(|g| *g <= 2.2), "{growth:?} {all:?}");
}

/// Values, warnings and saturation of every tab fixture agree across the
/// reference, memo, accumulator and step-oracle paths, in candidates and
/// in layout (which covers intrinsic sizes).
#[test]
fn tab_fixtures_match_reference() {
    let fixtures = [
        Fixture::new("pre-nested-tab", pre_nested(12, "日\t日")),
        Fixture::new("pre-siblings-lead-tab", pre_siblings(12, Some("\t"), "12")),
        Fixture::new("pre-siblings-base-tab", pre_siblings(12, None, "1\t2")),
        Fixture::new("tab-siblings", tab_siblings(&Limits::default())),
        Fixture::new(
            "effectful-tab-siblings",
            effectful_tab_siblings(&Limits::default()),
        ),
        Fixture::new(
            "huge-pre-nested-tab",
            pre_nested_in(12, "日\t日", &huge_pre_style()),
        ),
        Fixture::new(
            "huge-pre-siblings-lead-tab",
            pre_siblings_in(12, Some("\t"), "12", &huge_pre_style()),
        ),
    ];
    let pre = PreState {
        spent: 0,
        suppressed: false,
    };
    for fixture in &fixtures {
        let (reference, _) = observe_candidates_in(
            &fixture.paragraph,
            &AtomicSizes::EMPTY,
            Mode::Reference,
            pre,
        );
        let layout = observe_layout_in(fixture, Mode::Reference);
        for mode in [Mode::Memo, Mode::Accumulate, Mode::Verify] {
            let (observed, _) =
                observe_candidates_in(&fixture.paragraph, &AtomicSizes::EMPTY, mode, pre);
            assert_eq!(observed, reference, "{} {mode:?}", fixture.name);
            assert_eq!(
                observe_layout_in(fixture, mode),
                layout,
                "{} {mode:?}",
                fixture.name
            );
        }
    }
}

/// Review focus 3 of shodo-2j6, under shodo-tj5: replacing a prefix whose
/// tab steps saturated no longer moves the cache epoch (every query charges
/// its own tab steps), so the accumulator keeps replaying.
#[test]
fn effectful_tab_prefix_replacement_keeps_replay() {
    let fixture = Fixture::new(
        "effectful-tab-siblings",
        effectful_tab_siblings(&Limits::default()),
    );
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
    assert_eq!(counters.resets, 0, "{counters:?}");
    assert!(counters.replayed > 0, "{counters:?}");
    let mut cx = mode_context(Mode::Accumulate);
    cx.ruby_ranges
        .begin(&fixture.paragraph.data, &AtomicSizes::EMPTY);
    let epoch = cx.ruby_ranges.epoch();
    let containers = &fixture.paragraph.data.ruby.containers;
    let starts = [0, containers[1].units.start, containers[2].units.start];
    sweep_in(&fixture.paragraph, &starts, &mut cx);
    assert_eq!(
        cx.ruby_ranges.epoch(),
        epoch,
        "the replacement keeps the epoch"
    );
    assert_eq!(
        observe_layout_in(&fixture, Mode::Accumulate),
        observe_layout_in(&fixture, Mode::Reference)
    );
}

/// A prefix replaced without saturation leaves the epoch alone: the
/// accumulator keeps replaying.
#[test]
fn clean_tab_prefix_replacement_keeps_replay() {
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
    assert_eq!(counters.resets, 0, "{counters:?}");
    assert!(counters.replayed > 0, "{counters:?}");
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
        // Every range in one operation: reuse exactness, not the allowance.
        cx.ruby_line_work_disabled = true;
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
                assert_eq!(observed.4.dirty, [0; 7]);
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

/// Tab prefixes replaced for alternating starts with saturating tab steps:
/// the cache epoch stays at its post-begin value, every surviving
/// accumulator holds entries of that epoch, and the values match the
/// reference.
#[test]
fn alternating_tab_starts_keep_entries_of_the_current_epoch() {
    let p = effectful_tab_siblings(&Limits::default());
    let containers = &p.data.ruby.containers;
    let starts = [0, containers[1].units.start, containers[2].units.start];
    let reference = sweep_from(&p, &starts, Mode::Reference);
    for mode in [Mode::Accumulate, Mode::Verify] {
        let mut cx = mode_context(mode);
        // The first `begin` invalidates once for the new root; capture the
        // epoch after it so only tab-driven moves are counted.
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let epoch = cx.ruby_ranges.epoch();
        let observed = sweep_in(&p, &starts, &mut cx);
        assert_eq!(
            (&observed.0, observed.1, observed.2, &observed.3),
            (&reference.0, reference.1, reference.2, &reference.3),
            "{mode:?}"
        );
        assert_eq!(
            cx.ruby_ranges.epoch(),
            epoch,
            "{mode:?}: replacing a saturating tab prefix keeps the epoch"
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

/// Clean counterpart: the same alternating starts over 40px tabs replace the
/// prefix without moving the epoch.
#[test]
fn alternating_clean_tab_starts_keep_the_epoch() {
    let p = tab_siblings(&Limits::default());
    let containers = &p.data.ruby.containers;
    let starts = [0, containers[1].units.start, containers[2].units.start];
    let reference = sweep_from(&p, &starts, Mode::Reference);
    for mode in [Mode::Accumulate, Mode::Verify] {
        let mut cx = mode_context(mode);
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let epoch = cx.ruby_ranges.epoch();
        let observed = sweep_in(&p, &starts, &mut cx);
        assert_eq!(
            (&observed.0, observed.1, observed.2, &observed.3),
            (&reference.0, reference.1, reference.2, &reference.3),
            "{mode:?}"
        );
        assert_eq!(cx.ruby_ranges.epoch(), epoch, "{mode:?}");
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
        let mut replayed_before_last = 0;
        // The first ask fills the caches and records entries, the second
        // replays them, the third replays unless the epoch moved.
        for i in 0..3 {
            if i == 2 {
                replayed_before_last = cx.ruby_replayed_containers;
                if invalidate {
                    cx.ruby_ranges.vacate_slots();
                }
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
        let last = cx.ruby_replayed_containers - replayed_before_last;
        (observed, (counters(&cx), last))
    };
    for invalidate in [false, true] {
        let (reference, _) = run(Mode::Reference, invalidate);
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, (counters, last)) = run(mode, invalidate);
            assert_eq!(observed, reference, "{mode:?} {invalidate}");
            if mode == Mode::Accumulate {
                // Without the invalidation the third ask replays everything
                // but its top position; with it, nothing replays.
                assert_eq!(last > 0, !invalidate, "{counters:?} {last}");
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
/// Profile digest changes, which `rtl_isolate_siblings` has, are handled by
/// D4-D5 (no fallback marks every position).
#[test]
fn neighbour_rule_matches_reference_on_overhang_siblings() {
    let rtl = ParagraphStyle {
        direction: Direction::Rtl,
        ..paragraph_style(false)
    };
    // (fixture, D3 must mark)
    let fixtures = [
        (
            Fixture::new(
                "overhang-siblings",
                overhang_siblings(&paragraph_style(false)),
            ),
            false,
        ),
        (
            Fixture::new("overhang-siblings-rtl", overhang_siblings(&rtl)),
            false,
        ),
        (
            Fixture::new("rtl-isolate-siblings", rtl_isolate_siblings()),
            false,
        ),
        (
            Fixture::new(
                "adjacent-overhang-siblings",
                adjacent_overhang_siblings(&paragraph_style(false)),
            ),
            true,
        ),
        (
            Fixture::new(
                "adjacent-overhang-siblings-rtl",
                adjacent_overhang_siblings(&rtl),
            ),
            true,
        ),
    ];
    for (fixture, marks) in &fixtures {
        let mut dirty = [0; 7];
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
        assert_eq!(
            observe_layout_in(fixture, Mode::Verify),
            observe_layout_in(fixture, Mode::Reference),
            "{}",
            fixture.name
        );
    }
}

/// With D1-D6 the sibling shapes measure each container a bounded number of
/// times per line: container measures grow linearly.
#[test]
fn sibling_container_measures_grow_linearly_with_the_accumulator() {
    let (all, growth) = doubling(|r| sibling_measures(r, Mode::Accumulate), [16, 32, 64]);
    assert!(
        growth.iter().all(|g| *g < 2.5),
        "siblings {growth:?} {all:?}"
    );
    let (all, growth) = doubling(|r| outer_measures(r, Mode::Accumulate), [16, 32, 64]);
    assert!(growth.iter().all(|g| *g < 2.5), "outer {growth:?} {all:?}");
    // Adjacent overhang siblings: the shape where D3 marks.
    let adjacent = |r: usize| {
        let mut cx = mode_context(Mode::Accumulate);
        let p = row(
            &paragraph_style(false),
            r,
            "日",
            "にほんご",
            "",
            RubyOverhang::Auto,
            &style(24.0),
            &Limits::default(),
        );
        one_line(&p, &mut cx);
        assert!(
            cx.ruby_dirty[Dirty::Neighbour as usize] > 0,
            "{r}: D3 marks"
        );
        cx.ruby_container_measures
    };
    let (all, growth) = doubling(adjacent, [16, 32, 64]);
    assert!(
        growth.iter().all(|g| *g < 2.5),
        "adjacent {growth:?} {all:?}"
    );
}

/// Overhang siblings inside a top/bottom aligned span that also holds a
/// larger glyph, then a distant tall plain glyph and more siblings: the
/// clipped span's partial bounds change as the line grows inside it (D5)
/// and the tall glyph changes the profile's height and above (D4).
pub(super) fn grouped_siblings(align: VerticalAlign) -> Paragraph {
    let limits = Limits::default();
    let ruby = |b: &mut ParagraphBuilder, i: u64| {
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![base_text(3000 + i, "日本", &style(24.0), &limits)],
                &["にほんご"],
                RubyOverhang::Auto,
                &limits,
            ),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId(2000 + i),
            },
            "、",
        );
    };
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.open_inline(
        NodeId(40),
        &InlineStyle {
            vertical_align: align,
            ..style(24.0)
        },
        Default::default(),
    );
    ruby(&mut b, 0);
    ruby(&mut b, 1);
    b.open_inline(NodeId(41), &style(40.0), Default::default());
    b.push_text(TextSource::Generated { node: NodeId(42) }, "語");
    b.close_inline();
    ruby(&mut b, 2);
    b.close_inline();
    b.push_text(TextSource::Generated { node: NodeId(43) }, "本");
    b.open_inline(NodeId(44), &style(64.0), Default::default());
    b.push_text(TextSource::Generated { node: NodeId(45) }, "日");
    b.close_inline();
    ruby(&mut b, 3);
    ruby(&mut b, 4);
    finish(b)
}

#[test]
fn profile_and_edge_rules_match_reference() {
    let fixtures = [
        Fixture::new("grouped-top", grouped_siblings(VerticalAlign::Top)),
        Fixture::new("grouped-bottom", grouped_siblings(VerticalAlign::Bottom)),
        Fixture::new("cursive-siblings", cursive_siblings(&Limits::default())),
        Fixture::new(
            "cursive-siblings-6",
            cursive_siblings(&limits(Some(6), None)),
        ),
    ];
    let (mut profile, mut edge) = (0, 0);
    for fixture in &fixtures {
        for pre in pre_states(&fixture.paragraph) {
            let (reference, _) =
                observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Reference, pre);
            for mode in [Mode::Accumulate, Mode::Verify] {
                let (observed, counters) =
                    observe_candidates_in(&fixture.paragraph, &fixture.atomics, mode, pre);
                assert_eq!(observed, reference, "{} {mode:?} {pre:?}", fixture.name);
                profile += counters.dirty[Dirty::Profile as usize];
                edge += counters.dirty[Dirty::Edge as usize];
            }
        }
        assert_eq!(
            observe_layout_in(fixture, Mode::Verify),
            observe_layout_in(fixture, Mode::Reference),
            "{}",
            fixture.name
        );
    }
    assert!(profile > 0 && edge > 0, "profile {profile} edge {edge}");
}

/// Container work grows linearly with the sibling count (the reference and
/// the memo grow quadratically:
/// `sibling_container_measures_are_quadratic_without_the_accumulator`).
#[test]
fn sibling_container_measures_grow_linearly() {
    let (all, growth) = doubling(|r| sibling_measures(r, Mode::Accumulate), [32, 64, 128]);
    assert!(
        growth.iter().all(|g| *g <= 2.6),
        "break_all: {growth:?} {all:?}"
    );
    let one = |r: usize| {
        let mut cx = mode_context(Mode::Accumulate);
        one_line(&digit_siblings(r), &mut cx);
        cx.ruby_container_measures
    };
    let (all, growth) = doubling(one, [32, 64, 128]);
    assert!(
        growth.iter().all(|g| *g <= 2.6),
        "next_line: {growth:?} {all:?}"
    );
}

/// A plain glyph of `normal` px, a ruby whose trailing neighbour opens a
/// top-aligned span, then siblings inside the span before an atomic inline
/// of `block` px (no descent) that grows the span's partial bounds without
/// changing the line's height or above: D5 alone must mark the siblings in
/// the span (their units) and the first ruby (its neighbour unit).
pub(super) fn partial_group_siblings(normal: f32, block: f32) -> Fixture {
    let limits = Limits::default();
    let ruby = |b: &mut ParagraphBuilder, i: u64| {
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
    };
    let text = |b: &mut ParagraphBuilder, node: u64, text: &str| {
        b.push_text(TextSource::Generated { node: NodeId(node) }, text);
    };
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &limits);
    b.open_inline(NodeId(44), &style(normal), Default::default());
    text(&mut b, 45, "日");
    b.close_inline();
    ruby(&mut b, 0);
    b.open_inline(
        NodeId(40),
        &InlineStyle {
            vertical_align: VerticalAlign::Top,
            ..style(24.0)
        },
        Default::default(),
    );
    text(&mut b, 46, "本");
    ruby(&mut b, 1);
    text(&mut b, 47, "、");
    ruby(&mut b, 2);
    text(&mut b, 48, "、");
    b.push_atomic(NodeId(99), &style(24.0), Default::default());
    text(&mut b, 49, "、");
    ruby(&mut b, 3);
    b.close_inline();
    text(&mut b, 50, "本");
    let mut fixture = Fixture::new(format!("partial-group-{normal}-{block}"), finish(b));
    fixture.atomics.insert(
        NodeId(99),
        AtomicSize {
            inline_size: 24.0,
            block_size: block,
            ..Default::default()
        },
    );
    fixture
}

/// D5 against the reference and the step oracle where the partial bounds
/// of a top-aligned span change while the line's height and above do not
/// (D4 does not fire for those steps). Without D5's container branch the
/// siblings inside the span miss; without its neighbour branch the ruby
/// before the span, whose trailing neighbour is the span's first glyph,
/// misses (checked by disabling each branch while writing this test).
#[test]
fn edge_rule_covers_partial_groups_on_units_and_neighbour_units() {
    for block in [56.0, 60.0] {
        let fixture = partial_group_siblings(48.0, block);
        let mut dirty = [0; 7];
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
        assert!(
            dirty[Dirty::Edge as usize] > 0,
            "{}: {dirty:?}",
            fixture.name
        );
        assert_eq!(
            observe_layout_in(&fixture, Mode::Verify),
            observe_layout_in(&fixture, Mode::Reference),
            "{}",
            fixture.name
        );
    }
}

/// A ruby whose trailing neighbour is the only glyph of a top-aligned span,
/// then a plain ruby and a tall plain glyph of `tall` px and another ruby:
/// the tall glyph changes the line's height and above, moving the span's
/// glyph; the first ruby reads the profile only through that neighbour.
pub(super) fn grouped_neighbour_siblings(tall: f32) -> Paragraph {
    let limits = Limits::default();
    let ruby = |b: &mut ParagraphBuilder, i: u64| {
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
    };
    let text = |b: &mut ParagraphBuilder, node: u64, text: &str| {
        b.push_text(TextSource::Generated { node: NodeId(node) }, text);
    };
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &limits);
    text(&mut b, 45, "本");
    ruby(&mut b, 0);
    b.open_inline(
        NodeId(40),
        &InlineStyle {
            vertical_align: VerticalAlign::Top,
            ..style(24.0)
        },
        Default::default(),
    );
    text(&mut b, 46, "本");
    b.close_inline();
    ruby(&mut b, 1);
    text(&mut b, 47, "、");
    b.open_inline(NodeId(44), &style(tall), Default::default());
    text(&mut b, 48, "日");
    b.close_inline();
    ruby(&mut b, 2);
    text(&mut b, 49, "本");
    finish(b)
}

/// D4 against the reference and the step oracle where the first ruby reads
/// the profile only through its grouped neighbour unit
/// (`overhang::neighbor_bounds` under the container's share). Without D4
/// that ruby misses at every step the tall glyph enters (checked by
/// disabling D4 while writing this test).
#[test]
fn profile_rule_covers_grouped_neighbour_units() {
    let fixture = Fixture::new("grouped-neighbour", grouped_neighbour_siblings(48.0));
    let mut dirty = [0; 7];
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
    assert!(dirty[Dirty::Profile as usize] > 0, "{dirty:?}");
    assert_eq!(
        observe_layout_in(&fixture, Mode::Verify),
        observe_layout_in(&fixture, Mode::Reference),
        "{}",
        fixture.name
    );
}

#[test]
fn outer_base_siblings_match_reference() {
    let fixture = Fixture::new("outer-siblings", outer_siblings(6));
    for pre in pre_states(&fixture.paragraph) {
        let (reference, _) =
            observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Reference, pre);
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, _) =
                observe_candidates_in(&fixture.paragraph, &fixture.atomics, mode, pre);
            assert_eq!(observed, reference, "{mode:?} {pre:?}");
        }
    }
    assert_eq!(
        observe_layout_in(&fixture, Mode::Verify),
        observe_layout_in(&fixture, Mode::Reference)
    );
}

/// The clipped outer ruby is measured at every step; its descendant reads
/// are range queries, so all ruby work stays near linear.
#[test]
fn outer_base_siblings_grow_linearly() {
    let (all, growth) = doubling(|r| outer_measures(r, Mode::Accumulate), [32, 64, 128]);
    assert!(
        growth.iter().all(|g| *g <= 2.6),
        "measures {growth:?} {all:?}"
    );
    let visits = |r: usize| {
        let mut cx = mode_context(Mode::Accumulate);
        one_line(&outer_siblings(r), &mut cx);
        cx.ruby_measure_visits
    };
    let (all, growth) = doubling(visits, [32, 64, 128]);
    assert!(
        growth.iter().all(|g| *g <= 2.6),
        "visits {growth:?} {all:?}"
    );
}

/// An outer ruby whose base ends with plain text after its inner siblings:
/// probes inside the tail move the outer container's clipped end while the
/// last inner sibling (the top position) stays unchanged, so only D2 marks
/// the outer container.
fn outer_tail_siblings() -> Paragraph {
    let default = Limits::default();
    let mut base = ParagraphBuilder::new(&paragraph_style(false), &default);
    for i in 0..3u64 {
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
    base.push_text(TextSource::Generated { node: NodeId(2999) }, "日日日日");
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    b.push_ruby(
        NodeId(100),
        &style(24.0),
        annotated(
            vec![RubyContent::from_builder(base)],
            &["にほにほにほにほ"],
            RubyOverhang::None,
            &default,
        ),
    );
    finish(b)
}

/// Every sibling shape the dirty rules distinguish.
pub(super) fn sibling_fixtures() -> Vec<Fixture> {
    let default = Limits::default();
    vec![
        Fixture::new("digit-siblings", digit_siblings(6)),
        Fixture::new("outer-siblings", outer_siblings(4)),
        Fixture::new("outer-tail-siblings", outer_tail_siblings()),
        Fixture::new(
            "overhang-siblings",
            overhang_siblings(&paragraph_style(false)),
        ),
        // Text between the rubies above keeps every clean position's
        // recorded neighbour; adjacent rubies are the shape D3 marks.
        Fixture::new(
            "adjacent-overhang-siblings",
            adjacent_overhang_siblings(&paragraph_style(false)),
        ),
        Fixture::new("rtl-isolate-siblings", rtl_isolate_siblings()),
        Fixture::new("grouped-top", grouped_siblings(VerticalAlign::Top)),
        Fixture::new("grouped-bottom", grouped_siblings(VerticalAlign::Bottom)),
        Fixture::new("cursive-siblings", cursive_siblings(&default)),
        Fixture::new(
            "cursive-siblings-6",
            cursive_siblings(&limits(Some(6), None)),
        ),
        Fixture::new(
            "kerning-siblings",
            row(
                &paragraph_style(false),
                4,
                "AV",
                "に",
                "To",
                RubyOverhang::Auto,
                &style(24.0),
                &default,
            ),
        ),
        Fixture::new(
            "anywhere-siblings",
            row(
                &paragraph_style(false),
                5,
                "日本語",
                "にほんご",
                "",
                RubyOverhang::None,
                &anywhere(24.0),
                &default,
            ),
        ),
        Fixture::new("tab-siblings", tab_siblings(&default)),
        Fixture::new("effectful-tab-siblings", effectful_tab_siblings(&default)),
        Fixture::new("atomic-siblings", atomic_siblings(&limits(None, Some(2)))),
        Fixture::new(
            "first-line-siblings",
            row(
                &paragraph_style(true),
                4,
                "日本",
                "にほんご",
                "、",
                RubyOverhang::Auto,
                &style(24.0),
                &default,
            ),
        ),
    ]
}

/// Sibling fixtures checked per test: the reference sweeps are cubic in
/// debug builds, so the fixtures are split over tests that run in parallel.
const SIBLING_CHUNK: usize = 2;
const SIBLING_CHUNKS: usize = 8;

/// All `(start, end)` probes (growing, shrinking, restarted in a new
/// operation) and line layout (break_all warm/cold, narrow retries through
/// `PartialLine::index`, floats, intrinsic sizes), on every path, with the
/// step oracle, for chunk `chunk` of `sibling_fixtures`.
fn sibling_fixtures_match_reference_in(chunk: usize) {
    let fixtures = sibling_fixtures();
    for fixture in fixtures
        .iter()
        .skip(chunk * SIBLING_CHUNK)
        .take(SIBLING_CHUNK)
    {
        for pre in pre_states(&fixture.paragraph) {
            let (reference, _) =
                observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Reference, pre);
            for mode in [Mode::Memo, Mode::Accumulate, Mode::Verify] {
                let (observed, _) =
                    observe_candidates_in(&fixture.paragraph, &fixture.atomics, mode, pre);
                assert_eq!(observed, reference, "{} {mode:?} {pre:?}", fixture.name);
            }
        }
        let reference = observe_layout_in(fixture, Mode::Reference);
        for mode in [Mode::Accumulate, Mode::Verify] {
            assert_eq!(
                observe_layout_in(fixture, mode),
                reference,
                "{} {mode:?}",
                fixture.name
            );
        }
    }
}

/// Warm contexts over every fixture, with the step oracle; the per-fixture
/// probes and line layout run in the `_N` chunks below, which together cover
/// every sibling fixture.
#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle() {
    assert!(SIBLING_CHUNK * SIBLING_CHUNKS >= sibling_fixtures().len());
    let mut all = fixtures();
    all.extend(sibling_fixtures());
    assert_eq!(
        observe_warm_in(&all, Mode::Verify),
        observe_warm_in(&all, Mode::Reference)
    );
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_0() {
    sibling_fixtures_match_reference_in(0);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_1() {
    sibling_fixtures_match_reference_in(1);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_2() {
    sibling_fixtures_match_reference_in(2);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_3() {
    sibling_fixtures_match_reference_in(3);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_4() {
    sibling_fixtures_match_reference_in(4);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_5() {
    sibling_fixtures_match_reference_in(5);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_6() {
    sibling_fixtures_match_reference_in(6);
}

#[test]
fn sibling_fixtures_match_reference_with_the_step_oracle_7() {
    sibling_fixtures_match_reference_in(7);
}

/// The fixtures reach every rule, replay and reset path (equivalence must
/// not hold vacuously).
#[test]
fn sibling_fixtures_reach_every_accumulator_path() {
    let mut total = Counters::default();
    for fixture in sibling_fixtures() {
        let pre = PreState {
            spent: 0,
            suppressed: false,
        };
        let (_, c) =
            observe_candidates_in(&fixture.paragraph, &fixture.atomics, Mode::Accumulate, pre);
        total.replayed += c.replayed;
        total.resets += c.resets;
        for (sum, n) in total.dirty.iter_mut().zip(c.dirty) {
            *sum += n;
        }
    }
    assert!(total.replayed > 0 && total.resets > 0, "{total:?}");
    for reason in [
        Dirty::Unstored,
        Dirty::New,
        Dirty::Clipped,
        Dirty::Neighbour,
        Dirty::Profile,
        Dirty::Edge,
        Dirty::Ancestor,
    ] {
        assert!(total.dirty[reason as usize] > 0, "{reason:?}: {total:?}");
    }
}

/// Cursive words before cursive siblings: the narrow retry's speculative
/// `PartialLine::index` crosses the budget and is rolled back (warnings and
/// Saturation, not spent bytes or accumulators).
fn cursive_words_then_siblings(limits: &Limits) -> Paragraph {
    let mut b = ParagraphBuilder::new(&paragraph_style(false), limits);
    b.open_inline(NodeId(3), &style(6.0), Default::default());
    b.push_text(
        TextSource::Generated { node: NodeId(1) },
        &"بببببب ".repeat(8),
    );
    b.close_inline();
    for i in 0..4u64 {
        b.push_ruby(
            NodeId(1000 + i),
            &style(24.0),
            annotated(
                vec![base_text(3000 + i, "بببب", &style(24.0), limits)],
                &["に"],
                RubyOverhang::Auto,
                limits,
            ),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId(2000 + i),
            },
            "ببب",
        );
    }
    finish(b)
}

/// The wide scan, then narrow retries of the same token. Up to a 48-byte
/// window the wide scan itself exceeds the budget (warns, not retained, no
/// index); from 56 bytes the 999 retry's index stays within it and serves
/// 998. With 52 and 54 bytes the wide scan is clean and retained, the 999
/// retry's index crosses the limit and is rolled back, and the 998 retry
/// indexes again while replaying the accumulators the rolled-back pass
/// recorded (measured while writing this test).
#[test]
fn narrow_retry_rollback_with_siblings_matches_reference() {
    for window in [8, 12, 16, 52, 54, 64] {
        let p = cursive_words_then_siblings(&limits(Some(window), None));
        let observe = |mode: Mode| {
            let mut cx = mode_context(mode);
            let mut out = Vec::new();
            let mut indexed = Vec::new();
            let mut replayed = Vec::new();
            for width in [1000.0f32, 999.0, 998.0, 120.0] {
                cx.cache_prepare_visits = 0;
                let before = cx.ruby_replayed_containers;
                let result = p.next_line(
                    &mut cx,
                    p.start_token(),
                    &LineOptions::default(),
                    &LineConstraint::new(width),
                    &AtomicSizes::EMPTY,
                );
                out.push(result_signature(result));
                out.push(format!("{:?}", cx.take_warnings()));
                out.push(format!("{:?}", std::mem::take(&mut cx.ruby_oracle_misses)));
                indexed.push(cx.cache_prepare_visits);
                replayed.push(cx.ruby_replayed_containers - before);
            }
            (out, indexed, replayed)
        };
        let (reference, ..) = observe(Mode::Reference);
        for mode in [Mode::Accumulate, Mode::Verify] {
            let (observed, indexed, replayed) = observe(mode);
            assert_eq!(observed, reference, "window {window} {mode:?}");
            if mode == Mode::Accumulate && (window == 52 || window == 54) {
                assert_eq!(observed[1], "[]", "the wide scan must be retained");
                assert!(indexed[1] > 0, "the 999 retry must index: {indexed:?}");
                assert!(
                    indexed[2] > 0,
                    "the rolled-back index is redone: {indexed:?}"
                );
                assert!(
                    replayed[2] > 0,
                    "the retry after the rollback must replay: {replayed:?}"
                );
            }
        }
    }
}

/// A walk longer than the cap is measured as the through memo does and its
/// accumulator is released; results stay those of the reference.
#[test]
fn cap_overflow_falls_back_and_releases_the_accumulator() {
    let p = digit_siblings(12);
    let data = &p.data;
    let n = data.units.len();
    let run = |mode: Mode, cap: Option<usize>| {
        let mut cx = mode_context(mode);
        cx.ruby_accumulate_cap = cap;
        // The memo fallback's exactness, not the wide-walk refusal.
        cx.ruby_line_work_disabled = true;
        let mut sat = Saturation::default();
        cx.begin_reshape_operation();
        let values: Vec<_> = (1..=n)
            .map(|end| {
                crate::ruby::measure::candidate_adjustment(
                    data,
                    0,
                    end,
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut sat,
                )
            })
            .collect();
        (
            values,
            sat,
            cx.edge_reshape_spent,
            cx.ruby_memo.accumulator_keys(),
            counters(&cx),
        )
    };
    let (reference, ref_sat, ref_spent, _, _) = run(Mode::Reference, None);
    let (values, sat, spent, keys, capped) = run(Mode::Accumulate, Some(3));
    assert_eq!((&values, sat, spent), (&reference, ref_sat, ref_spent));
    let key = crate::ruby::accumulate::AccumulatorKey::new(data, &AtomicSizes::EMPTY, 0);
    assert!(!keys.contains(&Some(key)), "{keys:?}");
    let (values, _, _, _, uncapped) = run(Mode::Accumulate, None);
    assert_eq!(values, reference);
    assert!(
        capped.replayed < uncapped.replayed,
        "{capped:?} {uncapped:?}"
    );
}

/// Accumulator memory stays within its per-position bound and is released
/// when the next operation starts.
#[test]
fn accumulator_memory_is_bounded_and_released_per_operation() {
    use crate::ruby::accumulate::{BYTES_PER_CONTAINER, RETAINED_CONTAINERS};
    for siblings in [100, 400] {
        let p = many_siblings(siblings);
        let data = &p.data;
        let mut cx = mode_context(Mode::Accumulate);
        let mut sat = Saturation::default();
        cx.begin_reshape_operation();
        for end in 1..=data.units.len() {
            crate::ruby::measure::candidate_adjustment(
                data,
                0,
                end,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut sat,
            );
        }
        let (len, bytes) = cx.ruby_memo.accumulator_footprint();
        assert!(len >= siblings, "{siblings}: {len} positions");
        assert!(
            bytes <= len * BYTES_PER_CONTAINER,
            "{siblings}: {len} positions, {bytes} bytes"
        );
        cx.begin_reshape_operation();
        let (len, bytes) = cx.ruby_memo.accumulator_footprint();
        assert_eq!(len, 0);
        assert!(
            bytes <= RETAINED_CONTAINERS * BYTES_PER_CONTAINER,
            "{siblings}: {bytes} bytes retained"
        );
    }
}

/// shodo-tj5: range cache queries charge the same cold or warm, so a step
/// whose measurements fill the caches stores its entries, and the same step
/// again replays them with the same effects as the reference.
#[test]
fn entries_recorded_while_caches_fill_are_stored() {
    let p = digit_siblings(8);
    let data = &p.data;
    let n = data.units.len();
    let key = crate::ruby::accumulate::AccumulatorKey::new(data, &AtomicSizes::EMPTY, 0);
    let ask = |cx: &mut LayoutContext| {
        let mut sat = Saturation::default();
        let before = cx.edge_reshape_spent;
        // An exact probe (`through == end`) is not memoized: every ask
        // reaches the accumulator.
        let value = crate::ruby::measure::candidate_adjustment(
            data,
            0,
            n,
            &AtomicSizes::EMPTY,
            cx,
            &mut sat,
        );
        let acc = cx.ruby_memo.accumulator(key).unwrap();
        (
            (value, sat, cx.edge_reshape_spent - before),
            acc.len(),
            acc.unstored.len(),
        )
    };
    let mut cx = mode_context(Mode::Accumulate);
    cx.begin_reshape_operation();
    let (cold, len, unstored) = ask(&mut cx);
    assert_eq!((len, unstored), (8, 0), "entries recorded while filling");
    let replayed = cx.ruby_replayed_containers;
    let (warm, _, _) = ask(&mut cx);
    assert!(
        cx.ruby_replayed_containers > replayed,
        "the warm ask replays"
    );
    assert_eq!(warm, cold);
    let mut reference = mode_context(Mode::Reference);
    reference.begin_reshape_operation();
    let (first, _, _) = {
        let mut sat = Saturation::default();
        let value = crate::ruby::measure::candidate_adjustment(
            data,
            0,
            n,
            &AtomicSizes::EMPTY,
            &mut reference,
            &mut sat,
        );
        ((value, sat, reference.edge_reshape_spent), 0, 0)
    };
    assert_eq!(first, cold);
}

/// Descendant reads of one unbreakable line of outer-base siblings.
fn outer_descendant_reads(r: usize, mode: Mode) -> usize {
    let mut cx = mode_context(mode);
    one_line(&outer_siblings(r), &mut cx);
    cx.ruby_descendant_reads
}

/// Carried (review of Task 9): end to end, the clipped outer container's
/// descendant reads are answered by the segment tree, so they grow linearly
/// with the sibling count (the reference iterates every completed fragment
/// at every step: quadratic).
#[test]
fn outer_base_descendant_reads_grow_linearly() {
    let (all, growth) = doubling(
        |r| outer_descendant_reads(r, Mode::Accumulate),
        [32, 64, 128],
    );
    assert!(growth.iter().all(|g| *g <= 2.2), "{growth:?} {all:?}");
    let (all, growth) = doubling(|r| outer_descendant_reads(r, Mode::Reference), [8, 16, 32]);
    assert!(
        growth.iter().all(|g| *g >= 3.0),
        "reference {growth:?} {all:?}"
    );
}

/// Carried (review of Task 4): on charging cursive siblings (edge windows
/// charge the reshape budget, overhang `Auto` reads neighbour bounds through
/// the shared profile, so containers make several profile calls), replayed
/// runs repeat a charging profile (`own + m * P`, `m > 1`) and some step's
/// profile measurement warns, everywhere matching the reference. The
/// allowances' empty-base fallback call (`overhang::columns`, a second
/// `content_shared`) is reached here too: clipped selections leave a
/// container's bases empty (checked with temporary instrumentation while
/// writing this test).
#[test]
fn charging_sibling_profiles_repeat_and_warn_like_the_reference() {
    let (mut repeated, mut warned) = (0, 0);
    // A 6-byte window warns in the profile measurement (from any `spent`
    // while the sink is unsuppressed); 8 bytes repeat without warning.
    for window in [6, 8] {
        let p = cursive_siblings(&limits(Some(window), None));
        for pre in pre_states(&p) {
            let (reference, _) =
                observe_candidates_in(&p, &AtomicSizes::EMPTY, Mode::Reference, pre);
            for mode in [Mode::Accumulate, Mode::Verify] {
                let (observed, c) = observe_candidates_in(&p, &AtomicSizes::EMPTY, mode, pre);
                assert_eq!(observed, reference, "window {window} {mode:?} {pre:?}");
                if mode == Mode::Accumulate {
                    repeated += c.repeated_profile;
                    warned += c.profile_warnings;
                }
            }
        }
    }
    assert!(
        repeated > 0,
        "some replayed run must repeat a charging profile"
    );
    assert!(warned > 0, "some step's profile measurement must warn");
}

/// A remaining worst case (shodo-mc0): siblings inside a top-aligned span,
/// each followed by a glyph larger than every earlier one, so the partial
/// group and the profile change at every step and every profile-dependent
/// position is measured again.
pub(super) fn profile_churn(r: usize) -> Paragraph {
    let default = Limits::default();
    let mut b = ParagraphBuilder::new(&paragraph_style(false), &default);
    b.open_inline(
        NodeId(40),
        &InlineStyle {
            vertical_align: VerticalAlign::Top,
            ..style(24.0)
        },
        Default::default(),
    );
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
        b.open_inline(
            NodeId(5000 + i),
            &style(24.0 + i as f32),
            Default::default(),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId(6000 + i),
            },
            "日",
        );
        b.close_inline();
    }
    b.close_inline();
    finish(b)
}

#[test]
#[ignore = "report for the shodo-2j6 record"]
fn j6_operation_counts_report() {
    type Shape = (&'static str, fn(usize) -> Paragraph);
    let shapes: [Shape; 3] = [
        ("siblings", digit_siblings),
        ("outer", outer_siblings),
        ("churn", profile_churn),
    ];
    for (shape, build) in shapes {
        for mode in [Mode::Reference, Mode::Memo, Mode::Accumulate] {
            for r in [16, 32, 64, 128] {
                if mode != Mode::Accumulate && r > 64 {
                    continue;
                }
                let p = build(r);
                let mut cx = mode_context(mode);
                one_line(&p, &mut cx);
                let c = counters(&cx);
                println!(
                    "{{\"shape\":\"{shape}\",\"mode\":\"{mode:?}\",\"r\":{r},\"container_measures\":{},\"replayed\":{},\"width_calls\":{},\"scalar_calls\":{},\"dirty\":{:?},\"resets\":{}}}",
                    c.measures,
                    c.replayed,
                    cx.ruby_width_calls,
                    cx.ruby_scalar_calls,
                    c.dirty,
                    c.resets
                );
            }
        }
    }
}

#[test]
#[ignore = "report for the shodo-b7d record"]
fn b7d_operation_counts_report() {
    type Shape = (&'static str, fn(usize) -> Paragraph);
    let shapes: [Shape; 6] = [
        ("nested", |r| pre_nested(r, "日")),
        ("nestedtab", |r| pre_nested(r, "日\t日")),
        ("siblings", |r| pre_siblings(r, None, "12")),
        ("siblingstab", |r| pre_siblings(r, Some("\t"), "12")),
        ("nestedhugetab", |r| {
            pre_nested_in(r, "日\t日", &huge_pre_style())
        }),
        ("siblingshugetab", |r| {
            pre_siblings_in(r, Some("\t"), "12", &huge_pre_style())
        }),
    ];
    for (shape, build) in shapes {
        for mode in [Mode::Reference, Mode::Memo, Mode::Accumulate] {
            for r in [16, 32, 64, 128] {
                if mode == Mode::Reference && r > 64 {
                    continue;
                }
                let p = build(r);
                let mut cx = mode_context(mode);
                p.break_all(&mut cx, &LineOptions::default(), 96.0, &AtomicSizes::EMPTY);
                let c = counters(&cx);
                println!(
                    "{{\"shape\":\"{shape}\",\"mode\":\"{mode:?}\",\"r\":{r},\"container_measures\":{},\"replayed\":{},\"hits\":{},\"width_calls\":{},\"epoch\":{}}}",
                    c.measures,
                    c.replayed,
                    c.hits,
                    cx.ruby_width_calls,
                    cx.ruby_ranges.epoch()
                );
            }
        }
    }
}
