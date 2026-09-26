use shodo::font::FontCollection;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, InlineBoxFragment, LayoutContext, Line, LineConstraint,
    LineResult, Paragraph, ParagraphBuilder,
};

fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_size: 10.0,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    }
}

fn span() -> InlineStyle {
    InlineStyle {
        font_size: 10.0,
        ..InlineStyle::default()
    }
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset: 0,
    }
}

fn para(build: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(&style(), &Limits::default());
    build(&mut b);
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::new(&Limits::default()),
    )
    .unwrap()
}

fn all_lines(
    p: &Paragraph,
    width: f32,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
) -> Vec<Line> {
    let mut token = p.start_token();
    let mut out = Vec::new();
    while let LineResult::Line(line) = p.next_line(
        cx,
        token,
        &LineOptions::default(),
        &LineConstraint::new(width),
        atomics,
    ) {
        token = line.break_token();
        out.push(line);
    }
    out
}

fn boxes(line: &Line) -> Vec<InlineBoxFragment> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::InlineBox(b) = f {
                Some(b)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn glyph_runs_split_at_item_boundaries() {
    let p = para(|b| {
        b.push_text(dom(1), "ab")
            .open_inline(NodeId(2), &span(), InlineEdges::default())
            .push_text(dom(3), "cd")
            .close_inline()
            .push_text(dom(4), "ef");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let runs: Vec<_> = line
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(runs.len(), 3);
    assert_eq!(
        runs.iter().map(|r| r.node()).collect::<Vec<_>>(),
        [Some(NodeId(1)), Some(NodeId(3)), Some(NodeId(4))]
    );
    let positions: Vec<f32> = runs
        .iter()
        .flat_map(|r| r.glyphs().map(|g| g.inline_position))
        .collect();
    assert_eq!(positions, [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    assert_eq!(runs[1].glyphs().len(), 2);
    assert_eq!(runs[1].glyphs().get(1).unwrap().inline_position, 30.0);
    assert!(runs[0].font_data().is_some());
    assert!(line.font_data(runs[0].font()).is_some());
    assert_eq!(line.fragments().len(), 4, "3 runs + the span's inline box");
}

#[test]
fn glyph_offsets_are_applied_on_top_of_pen_positions() {
    let p = para(|b| {
        b.push_text(dom(1), "e\u{301}");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let Some(Fragment::GlyphRun(run)) = line.fragment(0) else {
        panic!()
    };
    let glyphs: Vec<_> = run.glyphs().collect();
    assert_eq!((glyphs[1].inline_position, glyphs[1].advance), (5.0, 0.0));
}

#[test]
fn inline_boxes_carry_edges_only_where_they_start_and_end() {
    let padded = InlineEdges {
        padding: Sides {
            inline_start: 5.0,
            inline_end: 5.0,
            block_start: 2.0,
            block_end: 2.0,
        },
        ..InlineEdges::default()
    };
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), padded)
            .push_text(dom(2), "aaa bbb")
            .close_inline();
    });
    let lines = all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    let second = boxes(&lines[1]);
    assert_eq!(
        (first[0].has_start_edge, first[0].has_end_edge),
        (true, false)
    );
    assert_eq!(
        (second[0].has_start_edge, second[0].has_end_edge),
        (false, true)
    );
    assert_eq!(
        (first[0].rect.inline_start, first[0].rect.inline_size),
        (0.0, 45.0)
    );
    assert_eq!(first[0].content_rect.inline_start, 5.0);
    assert_eq!(
        (
            second[0].rect.inline_size,
            second[0].content_rect.inline_size
        ),
        (35.0, 30.0)
    );
    // Block padding extends the border box around the 10px content area.
    assert_eq!(
        (first[0].rect.block_start, first[0].rect.block_size),
        (-2.0, 14.0)
    );
    assert_eq!(
        (
            first[0].content_rect.block_start,
            first[0].content_rect.block_size
        ),
        (0.0, 10.0)
    );
}

#[test]
fn nested_boxes_point_at_their_parent_fragment() {
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), InlineEdges::default())
            .open_inline(NodeId(2), &span(), InlineEdges::default())
            .push_text(dom(3), "aaa bbb")
            .close_inline()
            .close_inline();
    });
    for line in all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new()) {
        let fragments: Vec<_> = line.fragments().collect();
        let Fragment::InlineBox(outer) = &fragments[0] else {
            panic!()
        };
        let Fragment::InlineBox(inner) = &fragments[1] else {
            panic!()
        };
        assert_eq!((outer.node, outer.parent), (NodeId(1), None));
        assert_eq!((inner.node, inner.parent), (NodeId(2), Some(0)));
    }
}

#[test]
fn atomics_sit_on_the_baseline() {
    let p = para(|b| {
        b.push_text(dom(1), "a")
            .push_atomic(NodeId(2), &span(), InlineEdges::default());
    });
    let mut atomics = AtomicSizes::new();
    let margins = Sides {
        inline_start: 1.0,
        inline_end: 1.0,
        ..Sides::default()
    };
    atomics.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 20.0,
            block_size: 30.0,
            baseline: None,
            margins,
        },
    );
    let line = &all_lines(&p, 100.0, &atomics, &mut LayoutContext::new())[0];
    let atomic = line
        .fragments()
        .find_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        (
            atomic.margin_rect.inline_start,
            atomic.margin_rect.inline_size
        ),
        (10.0, 22.0)
    );
    assert_eq!(
        (
            atomic.border_rect.inline_start,
            atomic.border_rect.inline_size
        ),
        (11.0, 20.0)
    );
    // No baseline given: the bottom of the margin box sits on the baseline (8px).
    assert_eq!(
        (
            atomic.margin_rect.block_start,
            atomic.margin_rect.block_size
        ),
        (-22.0, 30.0)
    );
    assert_eq!(atomic.baseline, 8.0);
}

#[test]
fn missing_atomic_sizes_warn_and_collapse_to_zero() {
    let p = para(|b| {
        b.push_atomic(NodeId(2), &span(), InlineEdges::default());
    });
    let mut cx = LayoutContext::new();
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut cx)[0];
    assert!(
        cx.take_warnings()
            .iter()
            .any(|w| w.kind == WarningKind::MissingAtomicSize)
    );
    assert_eq!(line.inline_size(), 0.0);
    assert!(
        !line.is_empty(),
        "an atomic inline is content even when zero-sized"
    );
}

#[test]
fn out_of_flow_boxes_leave_anchors() {
    let p = para(|b| {
        b.push_text(dom(1), "ab")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Absolute)
            .push_text(dom(3), "c");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let anchor = line
        .fragments()
        .find_map(|f| {
            if let Fragment::OutOfFlowAnchor(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        (anchor.node, anchor.kind, anchor.inline_position),
        (NodeId(2), OutOfFlowKind::Absolute, 20.0)
    );
}
