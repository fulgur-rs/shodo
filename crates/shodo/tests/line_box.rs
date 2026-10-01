mod common;
#[test]
fn preserved_tab_and_empty_forced_inline_keep_participating_height() {
    let mut root = style();
    root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "\t");
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 10.0);
    assert!(!l.is_empty());
    let mut child = root.root.clone();
    child.font_size = 20.0;
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .push_forced_break(NodeId(3))
            .close_inline();
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 20.0);
    assert_eq!(l.baseline(BaselineKind::Alphabetic), 16.0);
}
use common::*;
use shodo::geometry::BaselineKind;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{LineOptions, VerticalAlign};
use shodo::{AtomicSize, AtomicSizes, Fragment};

#[test]
fn opening_edges_stay_with_atomic_after_soft_break() {
    let root = style();
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "aa")
            .open_inline(
                NodeId(2),
                &root.root,
                InlineEdges {
                    padding: Sides {
                        inline_start: 2.0,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            )
            .push_atomic(NodeId(3), &root.root, InlineEdges::default())
            .close_inline();
    });
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(3),
        AtomicSize {
            inline_size: 20.0,
            ..Default::default()
        },
    );
    let l = first_line(&p, 25.0, &LineOptions::default(), &sizes);
    assert_eq!(l.inline_size(), 20.0);
    assert!(l.fragments().all(|f| !matches!(f, Fragment::InlineBox(_))));
}

#[test]
fn central_atomic_baseline_aligns_to_parent_central_baseline() {
    let mut root = style();
    root.writing_mode = shodo::geometry::WritingMode::VerticalRl;
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, InlineEdges::default());
    });
    assert_eq!(p.required_baseline(NodeId(2)), Some(BaselineKind::Central));
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 10.0,
            block_size: 30.0,
            ..Default::default()
        },
    );
    let l = first_line(&p, 100.0, &LineOptions::default(), &sizes);
    let Fragment::Atomic(a) = l.fragments().next().unwrap() else {
        panic!()
    };
    assert_eq!(a.baseline, l.baseline(BaselineKind::Central));
    assert_eq!(a.margin_rect.block_start, 0.0);
    assert_eq!(l.block_size(), 30.0);
}

#[test]
fn height_limit_is_pure_and_can_be_retried() {
    let p = paragraph("a");
    let mut cx = shodo::LayoutContext::new();
    let mut c = shodo::LineConstraint::new(100.0);
    for limit in [9.0, 9.0, 0.0, -1.0, f32::NAN] {
        c.max_block_size = Some(limit);
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY
            ),
            shodo::LineResult::BlockSizeExceeded {
                needed_block_size: 10.0
            }
        ));
    }
    for limit in [Some(10.0), None] {
        c.max_block_size = limit;
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY
            ),
            shodo::LineResult::Line(_)
        ));
    }
}

#[test]
fn block_boundary_respects_each_line_indent() {
    let p = build(&style(), |b| {
        b.push_block_in_inline(NodeId(2))
            .push_text(TextSource::Generated { node: NodeId(3) }, "a");
    });
    let mut cx = shodo::LayoutContext::new();
    let c = shodo::LineConstraint::new(100.0);
    for (each_line, hanging, expected) in
        [(false, false, 0.0), (true, false, 5.0), (true, true, 0.0)]
    {
        let mut o = LineOptions::default();
        o.text_indent.length = 5.0;
        o.text_indent.each_line = each_line;
        o.text_indent.hanging = hanging;
        let shodo::LineResult::BlockInInline { token_after, .. } =
            p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
        else {
            panic!()
        };
        let shodo::LineResult::Line(l) =
            p.next_line(&mut cx, token_after, &o, &c, &AtomicSizes::EMPTY)
        else {
            panic!()
        };
        assert_eq!(glyphs(&l)[0].inline_position, expected);
    }
}

#[test]
fn atomic_height_retry_and_zero_height_block_prefix() {
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, InlineEdges::default());
    });
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 10.0,
            block_size: 30.0,
            baseline: Some(20.0),
            ..AtomicSize::default()
        },
    );
    let mut c = shodo::LineConstraint::new(100.0);
    c.max_block_size = Some(20.0);
    let mut cx = shodo::LayoutContext::new();
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &c,
            &sizes
        ),
        shodo::LineResult::BlockSizeExceeded {
            needed_block_size: 30.0
        }
    ));
    c.max_block_size = None;
    c.available_inline_size = 1.0;
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &c,
            &sizes
        ),
        shodo::LineResult::Line(_)
    ));
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &root.root, InlineEdges::default())
            .push_block_in_inline(NodeId(3))
            .close_inline();
    });
    c.max_block_size = Some(0.0);
    let shodo::LineResult::Line(l) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(l.block_size(), 0.0);
}

#[test]
fn larger_inline_expands_line_height() {
    let root = style();
    let mut child = root.root.clone();
    child.font_size = 20.0;
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "ab")
            .close_inline();
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 20.0);
    assert_eq!(l.baseline(BaselineKind::Alphabetic), 16.0);
}

#[test]
fn atomic_baseline_and_height_expand_line() {
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, InlineEdges::default());
    });
    let mut a = AtomicSizes::new();
    a.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 10.0,
            block_size: 30.0,
            baseline: Some(20.0),
            ..AtomicSize::default()
        },
    );
    let l = first_line(&p, 100.0, &LineOptions::default(), &a);
    assert_eq!(l.block_size(), 30.0);
    assert_eq!(l.baseline(BaselineKind::Alphabetic), 20.0);
}

#[test]
fn inline_block_padding_is_paint_geometry_only() {
    let root = style();
    let p = build(&root, |b| {
        b.open_inline(
            NodeId(2),
            &root.root,
            InlineEdges {
                padding: Sides {
                    block_start: 100.0,
                    ..Sides::default()
                },
                ..InlineEdges::default()
            },
        )
        .push_text(TextSource::Generated { node: NodeId(3) }, "a")
        .close_inline();
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 10.0);
    let box_ = l
        .fragments()
        .find_map(|f| {
            if let Fragment::InlineBox(v) = f {
                Some(v)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(box_.rect.block_size, 110.0);
}

#[test]
fn empty_before_block_has_zero_height_but_edges_are_content() {
    let root = style();
    for (edge, expected_empty) in [(0.0, true), (2.0, false)] {
        let p = build(&root, |b| {
            b.open_inline(
                NodeId(2),
                &root.root,
                InlineEdges {
                    border: Sides {
                        inline_start: edge,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            )
            .push_block_in_inline(NodeId(3))
            .close_inline();
        });
        let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(l.is_empty(), expected_empty);
        assert_eq!(l.block_size(), if expected_empty { 0.0 } else { 10.0 });
        assert_ne!(l.break_token(), p.start_token());
    }
}

#[test]
fn vertical_align_length_moves_child_baseline_and_expands_height() {
    let root = style();
    let mut child = root.root.clone();
    child.vertical_align = VerticalAlign::Length(5.0);
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "a")
            .close_inline();
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 15.0);
    assert_eq!(l.baseline(BaselineKind::Alphabetic), 13.0);
    let r = l
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(r.baseline(), 8.0);
}

#[test]
fn nested_alignment_accumulates_and_top_bottom_align_to_line_edges() {
    let root = style();
    for (align, expected) in [(VerticalAlign::Top, 8.0), (VerticalAlign::Bottom, 28.0)] {
        let mut large = root.root.clone();
        large.font_size = 30.0;
        let mut small = root.root.clone();
        small.vertical_align = align;
        let p = build(&root, |b| {
            b.open_inline(NodeId(2), &large, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "a")
                .close_inline()
                .open_inline(NodeId(4), &small, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(5) }, "b")
                .close_inline();
        });
        let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(l.block_size(), 30.0);
        let baselines: Vec<_> = l
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r.baseline()),
                _ => None,
            })
            .collect();
        assert_eq!(baselines, vec![24.0, expected]);
    }
    let mut raised = root.root.clone();
    raised.vertical_align = VerticalAlign::Length(5.0);
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &raised, InlineEdges::default())
            .open_inline(NodeId(3), &raised, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(4) }, "a")
            .close_inline()
            .close_inline();
    });
    let l = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 20.0);
    assert_eq!(l.baseline(BaselineKind::Alphabetic), 18.0);
}

#[test]
fn forced_empty_line_keeps_strut_and_direct_block_is_not_a_line() {
    let root = style();
    let p = build(&root, |b| {
        b.push_forced_break(NodeId(2));
    });
    assert_eq!(
        first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY).block_size(),
        10.0
    );
    let p = build(&root, |b| {
        b.push_block_in_inline(NodeId(2));
    });
    assert!(matches!(
        p.next_line(
            &mut shodo::LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            &shodo::LineConstraint::new(100.0),
            &AtomicSizes::EMPTY
        ),
        shodo::LineResult::BlockInInline { .. }
    ));
}

#[test]
fn cloned_edges_are_reserved_and_painted_on_continuations() {
    let root = style();
    let mut child = root.root.clone();
    child.box_decoration_break = shodo::style::BoxDecorationBreak::Clone;
    let p = build(&root, |b| {
        b.open_inline(
            NodeId(2),
            &child,
            InlineEdges {
                padding: Sides {
                    inline_start: 2.0,
                    inline_end: 3.0,
                    ..Sides::default()
                },
                ..InlineEdges::default()
            },
        )
        .push_text(TextSource::Generated { node: NodeId(3) }, "a b c")
        .close_inline();
    });
    let mut token = p.start_token();
    let mut count = 0;
    loop {
        match p.next_line(
            &mut shodo::LayoutContext::new(),
            token,
            &LineOptions::default(),
            &shodo::LineConstraint::new(20.0),
            &AtomicSizes::EMPTY,
        ) {
            shodo::LineResult::Line(l) => {
                token = l.break_token();
                count += 1;
                assert_eq!(l.inline_size(), 15.0);
                let b = l
                    .fragments()
                    .find_map(|f| match f {
                        Fragment::InlineBox(b) => Some(b),
                        _ => None,
                    })
                    .unwrap();
                assert!(b.has_start_edge && b.has_end_edge);
                assert_eq!(glyphs(&l)[0].inline_position, 2.0);
            }
            shodo::LineResult::Done => break,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(count, 3);
}
