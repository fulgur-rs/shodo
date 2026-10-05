//! Container accumulator (shodo-2j6): characterization, oracles, operation
//! count guards and equivalence fixtures. Shares the shodo-d77 harness.
use super::memo_tests::*;
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::LineOptions;
use crate::{AtomicSizes, LayoutContext, LineConstraint, Paragraph, ParagraphBuilder};

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
