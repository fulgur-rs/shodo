mod common;
#[test]
fn forced_break_preserves_cloned_start_and_end_edges() {
    let mut root = style();
    root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    let mut child = root.root.clone();
    child.box_decoration_break = shodo::style::BoxDecorationBreak::Clone;
    let p = build(&root, |b| {
        b.open_inline(
            NodeId(2),
            &child,
            InlineEdges {
                padding: shodo::node::Sides {
                    inline_start: 2.0,
                    inline_end: 2.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .push_text(TextSource::Generated { node: NodeId(3) }, "aa\nbb")
        .close_inline();
    });
    let sizes = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        &AtomicIntrinsics::EMPTY,
    );
    assert_eq!((sizes.min_content, sizes.max_content), (24.0, 24.0));
}
use common::*;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use shodo::style::LineOptions;
use shodo::{
    AtomicIntrinsic, AtomicIntrinsics, FloatClear, FloatIntrinsic, FloatSide, LayoutContext,
};

#[test]
fn words_and_forced_sections_measure_intrinsics() {
    let p = paragraph("aa bbb");
    let s = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        &AtomicIntrinsics::EMPTY,
    );
    assert_eq!((s.min_content, s.max_content), (30.0, 60.0));
    let p = build(&style(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "aaa")
            .push_forced_break(NodeId(2))
            .push_text(TextSource::Generated { node: NodeId(1) }, "bb");
    });
    assert_eq!(
        p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::EMPTY
        )
        .max_content,
        30.0
    );
}

#[test]
fn block_boundary_each_line_indent_is_included_in_intrinsic_sizes() {
    let p = build(&style(), |b| {
        b.push_block_in_inline(NodeId(2))
            .push_text(TextSource::Generated { node: NodeId(3) }, "a");
    });
    for (hanging, expected) in [(false, 22.0), (true, 10.0)] {
        let options = LineOptions {
            text_indent: shodo::style::TextIndent {
                length: 12.0,
                each_line: true,
                hanging,
            },
            ..Default::default()
        };
        let sizes = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &options,
            &AtomicIntrinsics::EMPTY,
        );
        assert_eq!(
            (sizes.min_content, sizes.max_content),
            (expected, expected),
            "hanging={hanging}"
        );
    }
}

#[test]
fn atomic_widths_are_already_margin_box_widths() {
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, InlineEdges::default());
    });
    let mut a = AtomicIntrinsics::new();
    a.insert_atomic(
        NodeId(2),
        AtomicIntrinsic {
            min_content: 20.0,
            max_content: 50.0,
        },
    );
    let s = p.intrinsic_sizes(&mut LayoutContext::new(), &LineOptions::default(), &a);
    assert_eq!((s.min_content, s.max_content), (20.0, 50.0));
}

#[test]
fn floats_accumulate_on_both_sides_and_clear_resets_target() {
    let mut root = style();
    root.direction = shodo::geometry::Direction::Rtl;
    let p = build(&root, |b| {
        b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_out_of_flow(NodeId(3), OutOfFlowKind::Float);
    });
    for (side, clear, expected) in [
        (FloatSide::Right, FloatClear::None, 70.0),
        (FloatSide::Right, FloatClear::Both, 50.0),
        (FloatSide::Right, FloatClear::Right, 50.0),
        (FloatSide::Left, FloatClear::Left, 70.0),
        (FloatSide::InlineEnd, FloatClear::InlineStart, 50.0),
    ] {
        let mut a = AtomicIntrinsics::new();
        a.insert_float(
            NodeId(2),
            FloatIntrinsic {
                min_content: 20.0,
                max_content: 20.0,
                side: FloatSide::InlineStart,
                clear: FloatClear::None,
            },
        );
        a.insert_float(
            NodeId(3),
            FloatIntrinsic {
                min_content: 50.0,
                max_content: 50.0,
                side,
                clear,
            },
        );
        let s = p.intrinsic_sizes(&mut LayoutContext::new(), &LineOptions::default(), &a);
        assert_eq!(s.max_content, expected);
        assert!(s.min_content <= s.max_content);
    }
}

#[test]
fn missing_and_nonfinite_inputs_are_finite_and_warn() {
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, InlineEdges::default());
    });
    let mut cx = LayoutContext::new();
    let s = p.intrinsic_sizes(&mut cx, &LineOptions::default(), &AtomicIntrinsics::EMPTY);
    assert_eq!(s.max_content, 0.0);
    assert!(!cx.take_warnings().is_empty());
    let mut a = AtomicIntrinsics::new();
    a.insert_atomic(
        NodeId(2),
        AtomicIntrinsic {
            min_content: f32::NAN,
            max_content: f32::INFINITY,
        },
    );
    let s = p.intrinsic_sizes(&mut cx, &LineOptions::default(), &a);
    assert!(s.min_content.is_finite() && s.max_content.is_finite());
    assert!(!cx.take_warnings().is_empty());
}
