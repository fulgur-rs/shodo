//! Container accumulator (shodo-2j6): characterization, oracles, operation
//! count guards and equivalence fixtures. Shares the shodo-d77 harness.
use super::memo_tests::*;
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::LineOptions;
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
