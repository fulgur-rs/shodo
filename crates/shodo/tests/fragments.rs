use shodo::font::FontCollection;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource};
use shodo::style::{BoxDecorationBreak, InlineStyle, LineOptions, ParagraphStyle, VerticalAlign};
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
    let style = style();
    para_with_style(&style, build)
}

fn para_with_style(style: &ParagraphStyle, build: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(style, &Limits::default());
    build(&mut b);
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::with_options(
            &Limits::default(),
            shodo::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        ),
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
fn glyphs_get_returns_none_for_an_out_of_range_index() {
    // Two separate text pushes shape into two runs, so the second run's
    // glyph range starts at a non-zero offset, which is what exercises the
    // overflow in a naive `start + index` bounds check.
    let p = para(|b| {
        b.push_text(dom(1), "ab").push_text(dom(2), "cd");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let Some(Fragment::GlyphRun(run)) = line.fragment(1) else {
        panic!()
    };
    assert_eq!(run.glyphs().get(usize::MAX), None);
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
        (0.0, 35.0)
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
fn inline_box_baselines_match_their_child_runs_after_vertical_alignment() {
    for vertical_align in [
        VerticalAlign::Baseline,
        VerticalAlign::Top,
        VerticalAlign::Bottom,
        VerticalAlign::Middle,
        VerticalAlign::TextTop,
        VerticalAlign::TextBottom,
    ] {
        let child = InlineStyle {
            font_size: 6.0,
            vertical_align,
            ..span()
        };
        let p = para(|b| {
            b.push_text(dom(1), "x")
                .open_inline(NodeId(2), &child, InlineEdges::default())
                .push_text(dom(3), "y")
                .close_inline();
        });
        let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
        let inline_box = boxes(line)
            .into_iter()
            .find(|b| b.node == NodeId(2))
            .unwrap();
        let child_baseline = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::GlyphRun(run) if run.node() == Some(NodeId(3)) => Some(run.baseline()),
                _ => None,
            })
            .unwrap();
        assert_eq!(inline_box.baseline, child_baseline, "{vertical_align:?}");
    }
}

#[test]
fn sliced_inline_box_offsets_follow_composite_fragment_order() {
    let edges = InlineEdges {
        margin: Sides {
            inline_start: 0.1,
            inline_end: 0.2,
            ..Default::default()
        },
        padding: Sides {
            inline_start: 5.0,
            inline_end: 5.0,
            ..Default::default()
        },
        ..InlineEdges::default()
    };
    let slice = InlineStyle {
        box_decoration_break: BoxDecorationBreak::Slice,
        ..span()
    };
    let p = para(|b| {
        b.open_inline(NodeId(1), &slice, edges)
            .push_text(dom(2), "aaa bbb")
            .close_inline();
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        45.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    let parts: Vec<_> = lines.iter().map(|line| boxes(line)[0]).collect();
    assert_eq!(parts[0].slice_offset, Some(0.0));
    assert_eq!(parts[1].slice_offset, Some(parts[0].rect.inline_size));

    for (paragraph_direction, text) in [
        (shodo::geometry::Direction::Rtl, "אבג דהו"),
        (shodo::geometry::Direction::Ltr, "אבג דהו"),
    ] {
        let mut paragraph_style = style();
        paragraph_style.direction = paragraph_direction;
        paragraph_style.root.direction = paragraph_direction;
        let rtl_slice = InlineStyle {
            direction: shodo::geometry::Direction::Rtl,
            box_decoration_break: BoxDecorationBreak::Slice,
            ..span()
        };
        let p = para_with_style(&paragraph_style, |b| {
            b.open_inline(NodeId(5), &rtl_slice, edges)
                .push_text(dom(6), text)
                .close_inline();
        });
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            45.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 2, "{paragraph_direction:?}");
        let parts: Vec<_> = lines.iter().map(|line| boxes(line)[0]).collect();
        assert_eq!(parts[0].slice_offset, Some(0.0), "{paragraph_direction:?}");
        assert_eq!(
            parts[1].slice_offset,
            Some(parts[0].rect.inline_size),
            "{paragraph_direction:?}"
        );
    }

    let limited = p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        45.0,
        4,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(limited.len(), 2);
    let limited_parts: Vec<_> = limited.iter().map(|line| boxes(line)[0]).collect();
    assert_eq!(limited_parts[0].slice_offset, Some(0.0));
    assert_eq!(
        limited_parts[1].slice_offset,
        Some(limited_parts[0].rect.inline_size)
    );

    let streamed = all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert!(
        streamed
            .iter()
            .flat_map(boxes)
            .all(|fragment| fragment.slice_offset.is_none())
    );

    let clone = InlineStyle {
        box_decoration_break: BoxDecorationBreak::Clone,
        ..span()
    };
    let p = para(|b| {
        b.open_inline(NodeId(3), &clone, edges)
            .push_text(dom(4), "aaa bbb")
            .close_inline();
    });
    let cloned = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        55.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(cloned.len(), 2);
    assert!(
        cloned
            .iter()
            .flat_map(boxes)
            .all(|fragment| fragment.slice_offset.is_none())
    );
}

#[test]
fn wrapped_inline_box_excludes_collapsible_space_but_keeps_preserved_space() {
    use shodo::style::WhiteSpaceCollapse;
    for (collapse, expected_width) in [
        (WhiteSpaceCollapse::Collapse, 35.0),
        (WhiteSpaceCollapse::PreserveBreaks, 35.0),
        (WhiteSpaceCollapse::Preserve, 45.0),
        (WhiteSpaceCollapse::PreserveSpaces, 45.0),
        (WhiteSpaceCollapse::BreakSpaces, 45.0),
    ] {
        let mut inline = span();
        inline.white_space_collapse = collapse;
        let p = para(|b| {
            b.open_inline(
                NodeId(1),
                &inline,
                InlineEdges {
                    padding: Sides {
                        inline_start: 5.0,
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(dom(2), "aaa bbb")
            .close_inline();
        });
        let lines = all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
        assert_eq!(lines.len(), 2, "{collapse:?}");
        let b = boxes(&lines[0])[0];
        assert_eq!(b.rect.inline_size, expected_width, "{collapse:?}");
        assert_eq!(
            b.content_rect.inline_size,
            expected_width - 5.0,
            "{collapse:?}"
        );
        assert_eq!(lines[0].text_range(), 0..4);
        let glyphs: Vec<_> = lines[0]
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run.glyphs()),
                _ => None,
            })
            .flatten()
            .collect();
        // The source space and its advance remain available for selection;
        // excluding it from background geometry must not move the glyphs.
        assert_eq!(
            glyphs.iter().map(|g| g.inline_position).collect::<Vec<_>>(),
            [5.0, 15.0, 25.0, 35.0]
        );
        assert_eq!(glyphs.last().unwrap().advance, 10.0);
        assert_eq!(boxes(&lines[1])[0].rect.inline_size, 35.0);
    }
}

#[test]
fn terminal_collapsible_space_trims_its_ancestors_without_trimming_an_earlier_sibling() {
    let edges = |padding| InlineEdges {
        padding: Sides {
            inline_start: padding,
            inline_end: padding,
            ..Default::default()
        },
        ..Default::default()
    };
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), InlineEdges::default())
            .open_inline(NodeId(2), &span(), edges(2.0))
            .push_text(dom(3), "x")
            .close_inline()
            .open_inline(NodeId(4), &span(), edges(3.0))
            .push_text(dom(5), "aa ")
            .close_inline()
            .close_inline()
            .push_text(dom(6), "bbb");
    });
    let lines = all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    let rects: Vec<_> = first
        .iter()
        .map(|b| {
            (
                b.node,
                b.rect.inline_start,
                b.rect.inline_size,
                b.content_rect.inline_size,
            )
        })
        .collect();
    assert_eq!(
        rects,
        [
            (NodeId(1), 0.0, 40.0, 40.0),
            (NodeId(2), 0.0, 14.0, 10.0),
            (NodeId(4), 14.0, 26.0, 20.0)
        ]
    );
    assert!(first.iter().all(|b| b.has_start_edge && b.has_end_edge));
    assert!(boxes(&lines[1]).is_empty());
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
fn a_line_starting_on_a_close_keeps_the_boxs_end_edge() {
    let padded = InlineEdges {
        padding: Sides {
            inline_start: 5.0,
            inline_end: 5.0,
            ..Sides::default()
        },
        ..InlineEdges::default()
    };
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), padded)
            .push_text(dom(2), "ab")
            .push_forced_break(NodeId(3))
            .close_inline()
            .push_text(dom(4), "cd");
    });
    let lines = all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert_eq!(lines.len(), 2);
    let second = boxes(&lines[1]);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].node, NodeId(1));
    assert_eq!(
        (second[0].has_start_edge, second[0].has_end_edge),
        (false, true)
    );
}

#[test]
fn nested_boxes_closing_on_a_continuation_line_each_keep_their_end_edge() {
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), InlineEdges::default())
            .open_inline(NodeId(2), &span(), InlineEdges::default())
            .push_text(dom(3), "ab")
            .push_forced_break(NodeId(4))
            .close_inline()
            .close_inline()
            .push_text(dom(5), "cd");
    });
    let lines = all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert_eq!(lines.len(), 2);
    let fragments: Vec<_> = lines[1].fragments().collect();
    let Fragment::InlineBox(outer) = &fragments[0] else {
        panic!()
    };
    let Fragment::InlineBox(inner) = &fragments[1] else {
        panic!()
    };
    assert_eq!((outer.node, outer.has_end_edge), (NodeId(1), true));
    assert_eq!(
        (inner.node, inner.has_end_edge, inner.parent),
        (NodeId(2), true, Some(0))
    );
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
    // The atomic's bottom baseline raises the line baseline to fit its height.
    assert_eq!(
        (
            atomic.margin_rect.block_start,
            atomic.margin_rect.block_size
        ),
        (0.0, 30.0)
    );
    assert_eq!(atomic.baseline, 30.0);
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

#[test]
fn non_finite_edges_become_zero_with_a_warning() {
    let edges = InlineEdges {
        padding: Sides {
            inline_start: f32::NAN,
            block_start: f32::INFINITY,
            ..Sides::default()
        },
        border: Sides {
            inline_end: -4.0,
            ..Sides::default()
        },
        margin: Sides {
            inline_start: -3.0,
            ..Sides::default()
        },
    };
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), edges)
            .push_text(dom(2), "ab")
            .close_inline();
    });
    let kinds: Vec<_> = p.warnings().iter().map(|w| w.kind).collect();
    assert!(kinds.contains(&WarningKind::NonFiniteInput), "{kinds:?}");
    assert!(kinds.contains(&WarningKind::NegativeInput), "{kinds:?}");
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let b = boxes(line)[0];
    for v in [
        b.rect.inline_start,
        b.rect.inline_size,
        b.rect.block_start,
        b.rect.block_size,
        b.content_rect.inline_start,
        b.content_rect.inline_size,
    ] {
        assert!(v.is_finite(), "{b:?}");
    }
    // Negative margins are kept; negative borders become 0.
    assert_eq!((b.rect.inline_start, b.rect.inline_size), (-3.0, 20.0));
    assert_eq!(b.content_rect.inline_start, -3.0);
    assert_eq!((b.rect.block_start, b.rect.block_size), (0.0, 10.0));
}

#[test]
fn non_finite_atomic_edges_are_sanitized() {
    let edges = InlineEdges {
        padding: Sides {
            inline_start: f32::NAN,
            ..Sides::default()
        },
        ..InlineEdges::default()
    };
    let p = para(|b| {
        b.push_atomic(NodeId(1), &span(), edges);
    });
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NonFiniteInput)
    );
}
