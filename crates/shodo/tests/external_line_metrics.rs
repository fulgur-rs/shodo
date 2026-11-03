mod common;

use common::*;
use shodo::geometry::{BaselineKind, WritingMode};
use shodo::limits::WarningKind;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{LineHeight, LineOptions, VerticalAlign};
use shodo::{
    AtomicSize, AtomicSizes, ExternalLineMetrics, Fragment, LayoutContext, Line, LineConstraint,
    LineResult, Paragraph,
};

fn accept(p: &Paragraph, cx: &mut LayoutContext, c: &LineConstraint<'_>) -> Line {
    let LineResult::Line(line) = p.next_line(
        cx,
        p.start_token(),
        &LineOptions::default(),
        c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected line")
    };
    line
}

fn external(over: f32, under: f32) -> ExternalLineMetrics {
    ExternalLineMetrics {
        line_over: over,
        line_under: under,
    }
}

#[test]
fn outside_image_enlarges_body_baseline_without_changing_text_or_breaks() {
    let mut root = style();
    root.root.font_size = 20.;
    root.root.line_height = LineHeight::Px(30.);
    let p = build(&root, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "z next",
        );
    });
    let mut c = LineConstraint::new(25.);
    c.block_offset = 65.;
    let mut cx = LayoutContext::new();
    let normal = accept(&p, &mut cx, &c);
    assert_eq!(normal.metrics().baseline + normal.block_offset(), 86.);
    c.external_metrics = Some(external(30., 0.));
    let enlarged = accept(&p, &mut cx, &c);
    assert_eq!(enlarged.metrics().baseline + enlarged.block_offset(), 95.);
    assert_eq!(enlarged.block_size(), 39.);
    assert_eq!(enlarged.inline_size(), normal.inline_size());
    assert_eq!(enlarged.text_range(), normal.text_range());
    assert_eq!(enlarged.break_token(), normal.break_token());
    assert_eq!(enlarged.text(), normal.text());
    assert_eq!(glyphs(&enlarged), glyphs(&normal));
    assert_eq!(enlarged.fragments().count(), normal.fragments().count());
    c.external_metrics = Some(external(20., 0.));
    let small = accept(&p, &mut cx, &c);
    assert_eq!(small.metrics(), normal.metrics());
    assert_eq!(small.block_size(), normal.block_size());
}

#[test]
fn line_over_and_under_are_line_relative_in_every_writing_mode() {
    for (mode, baseline) in [
        (WritingMode::HorizontalTb, 30.),
        (WritingMode::VerticalRl, 30.),
        (WritingMode::VerticalLr, 20.),
        (WritingMode::SidewaysRl, 30.),
        (WritingMode::SidewaysLr, 30.),
    ] {
        let mut root = style();
        root.writing_mode = mode;
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        });
        let mut c = LineConstraint::new(100.);
        c.external_metrics = Some(external(30., 20.));
        let line = accept(&p, &mut LayoutContext::new(), &c);
        assert_eq!(line.block_size(), 50., "{mode:?}");
        assert_eq!(line.metrics().baseline, baseline, "{mode:?}");
        let annotations = line.annotation_metrics();
        assert_eq!(
            (annotations.space_over, annotations.space_under),
            (0., 0.),
            "{mode:?}"
        );
        assert_eq!(
            (
                annotations.unannotated_block_start,
                annotations.unannotated_block_end
            ),
            (0., 50.)
        );
    }
}

#[test]
fn external_metrics_only_affect_the_calls_selected_by_the_caller() {
    let p = paragraph("a a a");
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(8.);
    c.external_metrics = Some(external(30., 20.));
    let first = accept(&p, &mut cx, &c);
    c.external_metrics = None;
    let LineResult::Line(second) = p.next_line(
        &mut cx,
        first.break_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("continuation")
    };
    assert_eq!(first.block_size(), 50.);
    assert_eq!(second.block_size(), 10.);
}

#[test]
fn height_rejection_and_retries_use_both_external_extents() {
    let p = paragraph("a");
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(100.);
    c.external_metrics = Some(external(30., 20.));
    c.max_block_size = Some(49.);
    for _ in 0..2 {
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY
            ),
            LineResult::BlockSizeExceeded {
                needed_block_size: 50.
            }
        ));
    }
    c.max_block_size = Some(50.);
    assert_eq!(accept(&p, &mut cx, &c).block_size(), 50.);
    c.external_metrics = Some(external(40., 20.));
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY
        ),
        LineResult::BlockSizeExceeded {
            needed_block_size: 60.
        }
    ));
    c.max_block_size = None;
    assert_eq!(accept(&p, &mut cx, &c).metrics().baseline, 40.);
}

#[test]
fn external_metrics_participate_before_top_and_bottom_alignment() {
    for (align, top) in [(VerticalAlign::Top, 0.), (VerticalAlign::Bottom, 44.)] {
        let root = style();
        let mut child = root.root.clone();
        child.vertical_align = align;
        let p = build(&root, |b| {
            b.push_atomic(NodeId(2), &child, InlineEdges::default());
        });
        let mut sizes = AtomicSizes::new();
        sizes.insert(
            NodeId(2),
            AtomicSize {
                inline_size: 5.,
                block_size: 6.,
                baseline: Some(2.),
                ..Default::default()
            },
        );
        let mut c = LineConstraint::new(100.);
        c.external_metrics = Some(external(30., 20.));
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            &c,
            &sizes,
        ) else {
            panic!("atomic line")
        };
        let Fragment::Atomic(atomic) = line.fragments().next().unwrap() else {
            panic!("atomic")
        };
        assert_eq!(atomic.margin_rect.block_start, top);
        assert_eq!(line.block_size(), 50.);
    }
}

#[test]
fn empty_paragraph_and_block_events_do_not_acquire_lines() {
    let mut c = LineConstraint::new(100.);
    c.external_metrics = Some(external(30., 20.));
    c.max_block_size = Some(0.);
    let empty = paragraph("");
    assert!(matches!(
        empty.next_line(
            &mut LayoutContext::new(),
            empty.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY
        ),
        LineResult::Done
    ));
    let p = build(&style(), |b| {
        b.push_block_in_inline(NodeId(2));
    });
    assert!(matches!(
        p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY
        ),
        LineResult::BlockInInline { .. }
    ));
    let p = build(&style(), |b| {
        b.open_inline(NodeId(2), &style().root, InlineEdges::default())
            .close_inline();
    });
    let line = accept(&p, &mut LayoutContext::new(), &c);
    assert!(line.is_empty());
    assert_eq!(line.block_size(), 0.);
}

#[test]
fn forced_break_and_zero_height_text_still_receive_external_metrics() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(0.);
    for forced in [false, true] {
        let p = build(&root, |b| {
            if forced {
                b.push_forced_break(NodeId(1));
            } else {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            }
        });
        let mut c = LineConstraint::new(100.);
        c.external_metrics = Some(external(30., 20.));
        let line = accept(&p, &mut LayoutContext::new(), &c);
        assert!(!line.is_empty());
        assert_eq!(line.block_size(), 50.);
    }
}

#[test]
fn zero_extents_are_a_baseline_contribution_with_negative_leading() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(0.);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let mut c = LineConstraint::new(100.);
    assert_eq!(accept(&p, &mut LayoutContext::new(), &c).block_size(), 0.);
    c.external_metrics = Some(external(0., 0.));
    let line = accept(&p, &mut LayoutContext::new(), &c);
    assert_eq!(line.block_size(), 3.);
    assert_eq!(line.baseline(BaselineKind::Alphabetic), 3.);
}

#[test]
fn first_line_style_and_external_metrics_both_contribute() {
    let mut root = style();
    let mut first = root.root.clone();
    first.font_size = 20.;
    first.line_height = LineHeight::Px(30.);
    root.first_line = Some(first);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let mut c = LineConstraint::new(100.);
    c.external_metrics = Some(external(30., 0.));
    let line = accept(&p, &mut LayoutContext::new(), &c);
    assert_eq!((line.metrics().baseline, line.block_size()), (30., 39.));
}

#[test]
fn invalid_extents_follow_layout_length_sanitization() {
    let p = paragraph("a");
    for (over, under, warning, baseline, height) in [
        (f32::NAN, 20., WarningKind::NonFiniteInput, 8., 28.),
        (30., f32::INFINITY, WarningKind::NonFiniteInput, 30., 32.),
        (-1., 20., WarningKind::NegativeInput, 8., 28.),
        (1e8, 0., WarningKind::Saturated, 1e7, 1e7 + 2.),
    ] {
        let mut c = LineConstraint::new(100.);
        c.external_metrics = Some(external(over, under));
        let mut cx = LayoutContext::new();
        let line = accept(&p, &mut cx, &c);
        assert_eq!(line.metrics().baseline, baseline);
        assert_eq!(line.block_size(), height);
        assert!(cx.take_warnings().iter().any(|w| w.kind == warning));
    }
}

#[test]
fn break_plans_keep_their_width_only_contract() {
    use shodo::style::TextWrapStyle;
    let p = paragraph("aa aa aa aa aa aa");
    for wrap in [TextWrapStyle::Balance, TextWrapStyle::Pretty] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            40.,
            &AtomicSizes::EMPTY,
        );
        let mut c = LineConstraint::new(40.);
        c.break_plan = Some(&plan);
        let mut token = p.start_token();
        let mut cx = LayoutContext::new();
        loop {
            let normal = p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY);
            let mut enlarged = c;
            enlarged.external_metrics = Some(external(30., 20.));
            let changed = p.next_line(&mut cx, token, &options, &enlarged, &AtomicSizes::EMPTY);
            match (normal, changed) {
                (LineResult::Line(a), LineResult::Line(b)) => {
                    assert_eq!(a.break_token(), b.break_token());
                    assert_eq!(a.inline_size(), b.inline_size());
                    assert_eq!(a.text_range(), b.text_range());
                    assert_eq!(b.block_size(), 50.);
                    token = b.break_token();
                }
                (LineResult::Done, LineResult::Done) => break,
                other => panic!("unexpected results: {other:?}"),
            }
        }
        assert!(
            cx.take_warnings()
                .iter()
                .all(|w| w.kind != WarningKind::Unsupported)
        );
    }
}

#[test]
fn float_retry_preserves_external_metrics_until_acceptance() {
    let p = build(&style(), |b| {
        b.push_out_of_flow(NodeId(2), shodo::node::OutOfFlowKind::Float);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let mut c = LineConstraint::new(100.);
    c.external_metrics = Some(external(30., 20.));
    let mut cx = LayoutContext::new();
    let LineResult::FloatEncountered {
        line_start,
        float_cursor,
        ..
    } = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("float event")
    };
    assert_eq!(line_start, p.start_token());
    c.floats_placed_through = Some(float_cursor);
    assert_eq!(accept(&p, &mut cx, &c).block_size(), 50.);
}
