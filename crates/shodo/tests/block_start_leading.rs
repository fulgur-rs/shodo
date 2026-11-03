mod common;

use common::*;
use shodo::geometry::BaselineKind;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{LineHeight, LineOptions, VerticalAlign};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
};

fn accept(
    p: &Paragraph,
    cx: &mut LayoutContext,
    c: &LineConstraint<'_>,
    sizes: &AtomicSizes,
) -> Line {
    let LineResult::Line(line) =
        p.next_line(cx, p.start_token(), &LineOptions::default(), c, sizes)
    else {
        panic!("expected line")
    };
    line
}

#[test]
fn extra_leading_keeps_negative_descent_and_original_line_content() {
    for (height, leading, expected_baseline) in [
        (0., 24., 30.),
        (10., 19., 30.),
        (30., 9., 30.),
        (100., 0., 56.),
    ] {
        let mut root = style();
        root.root.font_size = 20.;
        root.root.line_height = LineHeight::Px(height);
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "z");
        });
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(100.);
        c.block_offset = 65.;
        let normal = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
        c.block_start_leading = leading;
        let enlarged = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
        assert_eq!(
            enlarged.baseline(BaselineKind::Alphabetic),
            expected_baseline
        );
        assert_eq!(enlarged.block_size(), height + leading);
        assert_eq!(enlarged.block_offset(), 65.);
        assert_eq!(enlarged.block_start_leading(), leading);
        assert_eq!(enlarged.text(), normal.text());
        assert_eq!(enlarged.text_range(), normal.text_range());
        assert_eq!(enlarged.break_token(), normal.break_token());
        assert_eq!(enlarged.inline_size(), normal.inline_size());
    }
}

#[test]
fn extra_leading_moves_top_aligned_content_after_alignment() {
    let mut root = style();
    root.root.font_size = 20.;
    root.root.line_height = LineHeight::Px(30.);
    let mut child = root.root.clone();
    child.vertical_align = VerticalAlign::Top;
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &child, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "z");
    });
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 20.,
            block_size: 100.,
            ..Default::default()
        },
    );
    let mut c = LineConstraint::new(100.);
    c.block_start_leading = 9.;
    let line = accept(&p, &mut LayoutContext::new(), &c, &sizes);
    assert_eq!(line.block_size(), 109.);
    assert_eq!(line.baseline(BaselineKind::Alphabetic), 30.);
    let top = line
        .fragments()
        .find_map(|f| match f {
            Fragment::Atomic(a) => Some(a.margin_rect.block_start),
            _ => None,
        })
        .unwrap();
    assert_eq!(top, 9.);
}

#[test]
fn height_retries_do_not_reuse_a_different_leading() {
    let p = build(&style(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "z");
    });
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(100.);
    c.block_start_leading = 9.;
    let line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
    c.max_block_size = Some(line.block_size() - 1.);
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY
        ),
        LineResult::BlockSizeExceeded { .. }
    ));
    c.block_start_leading = 0.;
    assert_eq!(
        accept(&p, &mut cx, &c, &AtomicSizes::EMPTY).block_size(),
        line.block_size() - 9.
    );
}

#[test]
fn annotations_and_combined_text_move_without_reusing_the_prefix() {
    use shodo::geometry::WritingMode;
    use shodo::style::{TextCombineUpright, TextEmphasis, TextEmphasisPosition, TextEmphasisShape};
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let mut root = style();
        root.writing_mode = mode;
        root.root.text_combine_upright = TextCombineUpright::All;
        root.root.text_emphasis = Some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        });
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "12");
        });
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(100.);
        let normal = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
        c.block_start_leading = 9.;
        let line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
        assert_eq!(line.block_size(), normal.block_size() + 9., "{mode:?}");
        assert_eq!(
            line.metrics().baseline,
            normal.metrics().baseline + 9.,
            "{mode:?}"
        );
        let (a, b) = (normal.annotation_metrics(), line.annotation_metrics());
        assert_eq!(b.unannotated_block_start, a.unannotated_block_start + 9.);
        assert_eq!(b.unannotated_block_end, a.unannotated_block_end + 9.);
        assert_eq!(
            (
                b.space_over,
                b.space_under,
                b.overflow_over,
                b.overflow_under
            ),
            (
                a.space_over,
                a.space_under,
                a.overflow_over,
                a.overflow_under
            )
        );
        for (a, b) in normal.text_combinations().zip(line.text_combinations()) {
            assert_eq!(b.square.block_start, a.square.block_start + 9.);
            assert_eq!(b.square.block_size, a.square.block_size);
        }
        let a = normal.overflow_rect();
        let b = line.overflow_rect();
        assert_eq!(b.block_start, a.block_start + 9.);
        assert_eq!(b.block_size, a.block_size);
    }
}

#[test]
fn ruby_is_translated_after_its_own_reservation_without_double_moving_its_child() {
    use shodo::{Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan};
    let root = style();
    let limits = shodo::limits::Limits::default();
    let content = |node, text| {
        RubyContent::text(
            TextSource::Generated { node: NodeId(node) },
            text,
            &root.root,
            &limits,
        )
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: content(11, "ab"),
            align: Default::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(12),
                content: content(13, "x"),
                span: RubySpan::Auto,
                visibility: Default::default(),
            }],
            style: Default::default(),
        }],
    )
    .unwrap();
    let p = build(&root, |b| {
        b.push_ruby(NodeId(9), &root.root, ruby);
    });
    let mut c = LineConstraint::new(100.);
    let mut cx = LayoutContext::new();
    let normal = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
    c.block_start_leading = 9.;
    let line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
    assert_eq!(line.block_size(), normal.block_size() + 9.);
    assert_eq!(line.metrics().baseline, normal.metrics().baseline + 9.);
    let a = normal.ruby_annotations().next().unwrap();
    let b = line.ruby_annotations().next().unwrap();
    assert_eq!(b.transform().block_offset, a.transform().block_offset + 9.);
    assert_eq!(b.line().metrics(), a.line().metrics());
    assert_eq!(b.line().block_size(), a.line().block_size());
}

#[test]
fn empty_lines_stay_empty_and_forced_breaks_receive_leading() {
    let empty = build(&style(), |_| {});
    let mut c = LineConstraint::new(100.);
    c.block_start_leading = 9.;
    let result = empty.next_line(
        &mut LayoutContext::new(),
        empty.start_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    match result {
        LineResult::Done => {}
        LineResult::Line(line) => {
            assert!(line.is_empty());
            assert_eq!(line.block_size(), 0.);
            assert_eq!(line.block_start_leading(), 0.);
        }
        other => panic!("unexpected {other:?}"),
    }
    let p = build(&style(), |b| {
        b.push_forced_break(NodeId(1));
    });
    let mut cx = LayoutContext::new();
    let line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
    assert_eq!(line.block_start_leading(), 9.);
    let normal = accept(&p, &mut cx, &LineConstraint::new(100.), &AtomicSizes::EMPTY);
    assert_eq!(line.block_size(), normal.block_size() + 9.);
}

#[test]
fn leading_is_sanitized_and_survives_ellipsis() {
    let p = paragraph("abcdefghij");
    for value in [-1., f32::NAN, f32::INFINITY] {
        let mut c = LineConstraint::new(100.);
        c.block_start_leading = value;
        let mut cx = LayoutContext::new();
        let line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
        assert_eq!(line.block_start_leading(), 0.);
        assert!(!cx.take_warnings().is_empty());
    }
    let mut c = LineConstraint::new(100.);
    c.block_start_leading = 9.;
    let mut cx = LayoutContext::new();
    let mut line = accept(&p, &mut cx, &c, &AtomicSizes::EMPTY);
    let metrics = line.metrics();
    let advance = line.block_size();
    line.truncate_with_ellipsis(&mut cx, 20.);
    assert_eq!(line.metrics(), metrics);
    assert_eq!(line.block_size(), advance);
    assert_eq!(line.block_start_leading(), 9.);
}
