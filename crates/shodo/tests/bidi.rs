use shodo::font::FontCollection;
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle, UnicodeBidi};
use shodo::{
    AtomicSizes, Fragment, GlyphRunView, InlineBoxFragment, LayoutContext, Line, LineConstraint,
    LineResult, ParagraphBuilder,
};

fn style(direction: Direction) -> ParagraphStyle {
    ParagraphStyle {
        direction,
        root: InlineStyle {
            font_size: 10.0,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    }
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset: 0,
    }
}

fn one_line(direction: Direction, build: impl FnOnce(&mut ParagraphBuilder)) -> Line {
    let mut b = ParagraphBuilder::new(&style(direction), &Limits::default());
    build(&mut b);
    let p = b
        .build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    let r = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    );
    let LineResult::Line(line) = r else {
        panic!("{r:?}")
    };
    line
}

fn all_lines(
    direction: Direction,
    width: f32,
    build: impl FnOnce(&mut ParagraphBuilder),
) -> Vec<Line> {
    let mut b = ParagraphBuilder::new(&style(direction), &Limits::default());
    build(&mut b);
    let p = b
        .build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut out = Vec::new();
    while let LineResult::Line(line) = p.next_line(
        &mut cx,
        token,
        &LineOptions::default(),
        &LineConstraint::new(width),
        &AtomicSizes::EMPTY,
    ) {
        token = line.break_token();
        out.push(line);
        assert!(out.len() < 100, "no progress");
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

fn rtl_isolate() -> InlineStyle {
    InlineStyle {
        direction: Direction::Rtl,
        unicode_bidi: UnicodeBidi::Isolate,
        font_size: 10.0,
        ..InlineStyle::default()
    }
}

fn padded_3_7() -> InlineEdges {
    InlineEdges {
        padding: Sides {
            inline_start: 3.0,
            inline_end: 7.0,
            ..Sides::default()
        },
        ..InlineEdges::default()
    }
}

fn runs(line: &Line) -> Vec<GlyphRunView<'_>> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .collect()
}

fn positions(run: &GlyphRunView<'_>) -> Vec<f32> {
    run.glyphs().map(|g| g.inline_position).collect()
}

#[test]
fn right_to_left_runs_reverse_inside_a_left_to_right_paragraph() {
    let line = one_line(Direction::Ltr, |b| {
        b.push_text(dom(1), "abc \u{5D0}\u{5D1}\u{5D2}");
    });
    let runs = runs(&line);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].bidi_level(), 1);
    assert_eq!(runs[1].inline_start(), 40.0);
    // Logical order alef, bet, gimel is drawn right to left.
    assert_eq!(positions(&runs[1]), [60.0, 50.0, 40.0]);
}

#[test]
fn right_to_left_paragraphs_measure_from_the_right() {
    let line = one_line(Direction::Rtl, |b| {
        b.push_text(dom(1), "\u{5D0}\u{5D1} ab");
    });
    let runs = runs(&line);
    assert_eq!(runs.len(), 2);
    // The Hebrew run comes first from the inline-start (right) edge and
    // needs no reversal; the embedded Latin run does.
    assert_eq!(
        (runs[0].inline_start(), positions(&runs[0])),
        (0.0, vec![0.0, 10.0, 20.0])
    );
    assert_eq!(
        (runs[1].inline_start(), positions(&runs[1])),
        (30.0, vec![40.0, 30.0])
    );
}

#[test]
fn an_inline_box_split_by_bidi_becomes_two_fragments() {
    let line = one_line(Direction::Ltr, |b| {
        b.open_inline(
            NodeId(1),
            &InlineStyle {
                font_size: 10.0,
                ..InlineStyle::default()
            },
            InlineEdges::default(),
        )
        .push_text(dom(2), "ab \u{5D0}\u{5D1}")
        .close_inline()
        .push_text(dom(3), " \u{5D2}\u{5D3}");
    });
    let boxes: Vec<InlineBoxFragment> = line
        .fragments()
        .filter_map(|f| {
            if let Fragment::InlineBox(b) = f {
                Some(b)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(boxes.len(), 2);
    assert!(boxes.iter().all(|b| b.node == NodeId(1)));
    // Visual order: [ab ] [ gimel-dalet reversed] [alef-bet reversed].
    assert_eq!(
        (boxes[0].rect.inline_start, boxes[0].rect.inline_size),
        (0.0, 30.0)
    );
    assert_eq!(
        (boxes[0].has_start_edge, boxes[0].has_end_edge),
        (true, false)
    );
    assert_eq!(
        (boxes[1].rect.inline_start, boxes[1].rect.inline_size),
        (60.0, 20.0)
    );
    assert_eq!(
        (boxes[1].has_start_edge, boxes[1].has_end_edge),
        (false, true)
    );
}

#[test]
fn an_rtl_box_in_an_ltr_paragraph_has_its_start_edge_on_the_right() {
    let line = one_line(Direction::Ltr, |b| {
        b.open_inline(
            NodeId(1),
            &InlineStyle {
                direction: Direction::Rtl,
                unicode_bidi: UnicodeBidi::Isolate,
                font_size: 10.0,
                ..InlineStyle::default()
            },
            InlineEdges {
                padding: Sides {
                    inline_start: 3.0,
                    inline_end: 7.0,
                    ..Sides::default()
                },
                ..InlineEdges::default()
            },
        )
        .push_text(dom(2), "\u{5D0}\u{5D1}")
        .close_inline();
    });
    let boxes: Vec<InlineBoxFragment> = line
        .fragments()
        .filter_map(|f| {
            if let Fragment::InlineBox(b) = f {
                Some(b)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(boxes.len(), 1);
    let b = boxes[0];
    assert_eq!((b.has_start_edge, b.has_end_edge), (true, true));
    // The box's own direction is RTL, so its logical start (padding-inline-
    // start = 3px) is on the visual right and its end (padding-inline-end =
    // 7px) is on the visual left.
    assert_eq!(b.content_rect.inline_start, b.rect.inline_start + 7.0);
    assert_eq!(b.content_rect.inline_size, b.rect.inline_size - 10.0);
    // The box's edges are laid out with its content, not split from it by
    // the outer level UAX #9 assigns to the isolate initiator/terminator:
    // the run renders inside content_rect, flush with its start.
    let runs = runs(&line);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].inline_start(), b.content_rect.inline_start);
    assert!(
        positions(&runs[0])
            .iter()
            .all(|&p| p >= b.content_rect.inline_start)
    );
}

#[test]
fn inline_box_reports_when_its_direction_differs_from_the_paragraph() {
    for (paragraph_direction, inline_direction, expected_reversed) in [
        (Direction::Ltr, Direction::Ltr, false),
        (Direction::Ltr, Direction::Rtl, true),
        (Direction::Rtl, Direction::Ltr, true),
        (Direction::Rtl, Direction::Rtl, false),
    ] {
        let text = if inline_direction == Direction::Rtl {
            "\u{5D0}\u{5D1}"
        } else {
            "ab"
        };
        let line = one_line(paragraph_direction, |b| {
            b.open_inline(
                NodeId(1),
                &InlineStyle {
                    direction: inline_direction,
                    unicode_bidi: UnicodeBidi::Isolate,
                    font_size: 10.0,
                    ..InlineStyle::default()
                },
                InlineEdges::default(),
            )
            .push_text(dom(2), text)
            .close_inline();
        });
        let boxes = boxes(&line);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].has_start_edge);
        assert_eq!(boxes[0].start_edge_is_reversed, expected_reversed);
    }
}

#[test]
fn an_isolate_closing_after_a_soft_break_keeps_its_end_edge_on_the_line() {
    let lines = all_lines(Direction::Ltr, 55.0, |b| {
        b.open_inline(NodeId(1), &rtl_isolate(), padded_3_7())
            .push_text(dom(2), "abc ")
            .close_inline()
            .push_text(dom(3), "def");
    });
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    assert_eq!(first.len(), 1);
    assert_eq!(
        (
            first[0].node,
            first[0].has_start_edge,
            first[0].has_end_edge
        ),
        (NodeId(1), true, true)
    );
    assert!(
        boxes(&lines[1]).iter().all(|b| b.node != NodeId(1)),
        "{:?}",
        boxes(&lines[1])
    );
}

#[test]
fn nested_closes_after_an_isolate_stay_on_the_line() {
    let plain = InlineStyle {
        font_size: 10.0,
        ..InlineStyle::default()
    };
    let isolate = InlineStyle {
        unicode_bidi: UnicodeBidi::Isolate,
        ..plain.clone()
    };
    let lines = all_lines(Direction::Ltr, 45.0, |b| {
        b.open_inline(NodeId(1), &plain, InlineEdges::default())
            .open_inline(NodeId(2), &isolate, InlineEdges::default())
            .push_text(dom(3), "ab ")
            .close_inline()
            .close_inline()
            .push_text(dom(4), "cd");
    });
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    assert_eq!(first.len(), 2);
    assert!(first.iter().all(|b| b.has_start_edge && b.has_end_edge));
    assert!(boxes(&lines[1]).is_empty(), "{:?}", boxes(&lines[1]));
}

#[test]
fn trailing_whitespace_takes_the_paragraph_level() {
    let lines = all_lines(Direction::Ltr, 45.0, |b| {
        b.push_text(dom(1), "\u{5D0}\u{5D1}\u{5D2} \u{5D3}\u{5D4}\u{5D5}");
    });
    assert_eq!(lines.len(), 2);
    let first = runs(&lines[0]);
    assert_eq!(first.len(), 2);
    // UAX #9 L1: the hanging space is reset to the paragraph level, so it
    // sits after the Hebrew run at the line end instead of before it.
    assert_eq!(
        (first[0].inline_start(), first[0].inline_size()),
        (0.0, 30.0)
    );
    assert_eq!(positions(&first[0]), [20.0, 10.0, 0.0]);
    assert_eq!((first[1].inline_start(), first[1].bidi_level()), (30.0, 0));
    assert_eq!(lines[0].inline_size(), 30.0);
    let second = runs(&lines[1]);
    assert_eq!(positions(&second[0]), [20.0, 10.0, 0.0]);
}

#[test]
fn tabs_take_the_paragraph_level() {
    let pre = InlineStyle {
        font_size: 10.0,
        white_space_collapse: shodo::style::WhiteSpaceCollapse::Preserve,
        tab_size: shodo::style::TabSize::Px(40.0),
        ..InlineStyle::default()
    };
    let line = one_line(Direction::Ltr, |b| {
        b.open_inline(NodeId(1), &pre, InlineEdges::default())
            .push_text(dom(2), "\u{5D0}\t\u{5D1}")
            .close_inline();
    });
    // UAX #9 L1: a segment separator splits the right-to-left text into
    // two runs, each on its own side of the tab.
    let runs = runs(&line);
    assert_eq!(
        runs.iter().map(|r| r.inline_start()).collect::<Vec<_>>(),
        [0.0, 40.0]
    );
    assert_eq!(runs[0].text_range(), 0..2);
}

#[test]
fn an_rtl_isolate_keeps_its_edges_around_its_content_before_a_hanging_space() {
    let lines = all_lines(Direction::Ltr, 55.0, |b| {
        b.open_inline(NodeId(1), &rtl_isolate(), padded_3_7())
            .push_text(dom(2), "\u{5D0}\u{5D1}\u{5D2} ")
            .close_inline()
            .push_text(dom(3), "def");
    });
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    assert_eq!(first.len(), 1);
    let b = first[0];
    assert_eq!((b.has_start_edge, b.has_end_edge), (true, true));
    // Visual order: [end edge 7][content 30][start edge 3][hanging space].
    assert_eq!((b.rect.inline_start, b.rect.inline_size), (0.0, 40.0));
    assert_eq!(
        (b.content_rect.inline_start, b.content_rect.inline_size),
        (7.0, 30.0)
    );
    let runs = runs(&lines[0]);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].inline_start(), 7.0);
    assert_eq!(positions(&runs[0]), [27.0, 17.0, 7.0]);
    assert_eq!(runs[1].inline_start(), 40.0);
    assert!(boxes(&lines[1]).is_empty());
}

#[test]
fn a_hanging_space_does_not_split_an_isolate_of_deeper_content() {
    // Latin text in a right-to-left isolate is at level 2 while the space
    // before the isolate's end resolves to level 1 before L1. The box must
    // come out exactly as it does without the space, followed by the space.
    let with_space = all_lines(Direction::Ltr, 55.0, |b| {
        b.open_inline(NodeId(1), &rtl_isolate(), padded_3_7())
            .push_text(dom(2), "abc ")
            .close_inline()
            .push_text(dom(3), "def");
    });
    let without = one_line(Direction::Ltr, |b| {
        b.open_inline(NodeId(1), &rtl_isolate(), padded_3_7())
            .push_text(dom(2), "abc")
            .close_inline();
    });
    assert_eq!(with_space.len(), 2);
    let rects = |line: &Line| {
        boxes(line)
            .iter()
            .map(|b| (b.rect, b.content_rect, b.has_start_edge, b.has_end_edge))
            .collect::<Vec<_>>()
    };
    assert_eq!(rects(&with_space[0]), rects(&without));
    let (a, b) = (runs(&with_space[0]), runs(&without));
    assert_eq!(a.len(), 2);
    assert_eq!(positions(&a[0]), positions(&b[0]));
    assert_eq!((a[1].inline_start(), a[1].bidi_level()), (40.0, 0));
}

#[test]
fn many_boxes_on_one_reordered_line_are_laid_out() {
    const SPANS: usize = 20_000;
    let span = InlineStyle {
        font_size: 10.0,
        ..InlineStyle::default()
    };
    let mut b = ParagraphBuilder::new(&style(Direction::Ltr), &Limits::default());
    for n in 0..SPANS {
        b.open_inline(NodeId(n as u64), &span, InlineEdges::default())
            .push_text(dom(n as u64), "a")
            .close_inline();
    }
    b.push_text(dom(SPANS as u64), "\u{5D0}");
    let p = b
        .build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                shodo::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1.0e9),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let boxes = boxes(&line);
    assert_eq!(boxes.len(), SPANS);
    assert_eq!(
        boxes[SPANS - 1].rect.inline_start,
        10.0 * (SPANS - 1) as f32
    );
    assert!(boxes.iter().all(|b| b.parent.is_none()));
}

#[test]
fn nested_boxes_split_by_bidi_point_at_their_enclosing_fragment() {
    let span = InlineStyle {
        font_size: 10.0,
        ..InlineStyle::default()
    };
    let line = one_line(Direction::Ltr, |b| {
        b.open_inline(NodeId(1), &span, InlineEdges::default())
            .push_text(dom(2), "ab ")
            .open_inline(NodeId(3), &span, InlineEdges::default())
            .push_text(dom(4), "c \u{5D0}")
            .close_inline()
            .push_text(dom(5), "\u{5D1}")
            .close_inline()
            .push_text(dom(6), " \u{5D2}");
    });
    let fragments: Vec<_> = line.fragments().collect();
    for f in &fragments {
        let Fragment::InlineBox(b) = f else { continue };
        let Some(parent) = b.parent else {
            assert_eq!(b.node, NodeId(1));
            continue;
        };
        let Fragment::InlineBox(p) = &fragments[parent] else {
            panic!("parent is not a box")
        };
        assert_eq!((b.node, p.node), (NodeId(3), NodeId(1)));
        assert!(p.rect.inline_start <= b.rect.inline_start);
        assert!(
            b.rect.inline_start + b.rect.inline_size <= p.rect.inline_start + p.rect.inline_size
        );
    }
    assert_eq!(
        fragments
            .iter()
            .filter(|f| matches!(f, Fragment::InlineBox(b) if b.node == NodeId(3)))
            .count(),
        2
    );
}

fn padded_5_5() -> InlineEdges {
    InlineEdges {
        padding: Sides {
            inline_start: 5.0,
            inline_end: 5.0,
            ..Sides::default()
        },
        ..InlineEdges::default()
    }
}

/// Lays out `lead` + box(`text`, padding 5/5) + "def" at 70px and returns the
/// first line's inline box geometry.
fn first_line_box_rects(
    direction: Direction,
    lead: &str,
    span: InlineStyle,
    text: &str,
) -> Vec<(f32, f32, f32, f32)> {
    let lines = all_lines(direction, 70.0, |b| {
        b.push_text(dom(1), lead)
            .open_inline(NodeId(2), &span, padded_5_5())
            .push_text(dom(3), text)
            .close_inline()
            .push_text(dom(4), "def");
    });
    assert_eq!(lines.len(), 2);
    boxes(&lines[0])
        .iter()
        .map(|b| {
            (
                b.rect.inline_start,
                b.rect.inline_size,
                b.content_rect.inline_start,
                b.content_rect.inline_size,
            )
        })
        .collect()
}

#[test]
fn a_collapsible_hanging_space_is_excluded_from_same_direction_boxes() {
    let span = InlineStyle {
        font_size: 10.0,
        ..InlineStyle::default()
    };
    // A right-to-left letter elsewhere on the line must not change the
    // geometry of a left-to-right box whose trailing space hangs.
    let latin = first_line_box_rects(Direction::Ltr, "x", span.clone(), "abc ");
    let hebrew = first_line_box_rects(Direction::Ltr, "\u{5D0}", span, "abc ");
    assert_eq!(latin, [(10.0, 40.0, 15.0, 30.0)]);
    assert_eq!(hebrew, latin);
}

#[test]
fn an_anchor_between_hanging_spaces_keeps_its_position() {
    let preserved = InlineStyle {
        font_size: 10.0,
        white_space_collapse: shodo::style::WhiteSpaceCollapse::BreakSpaces,
        ..InlineStyle::default()
    };
    let anchor_at = |lead: &str| {
        let lines = all_lines(Direction::Ltr, 45.0, |b| {
            b.open_inline(NodeId(1), &preserved, InlineEdges::default())
                .push_text(dom(2), lead)
                .push_out_of_flow(NodeId(3), shodo::node::OutOfFlowKind::Absolute)
                .push_text(dom(4), " xyz")
                .close_inline();
        });
        lines[0]
            .fragments()
            .find_map(|f| match f {
                Fragment::OutOfFlowAnchor(a) => Some(a.inline_position),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(anchor_at("ab "), 30.0);
    assert_eq!(anchor_at("\u{5D0}\u{5D1} "), 30.0);
}

#[test]
fn a_collapsible_hanging_space_is_excluded_from_same_direction_rtl_boxes() {
    let span = InlineStyle {
        font_size: 10.0,
        direction: Direction::Rtl,
        ..InlineStyle::default()
    };
    let text = "\u{5D0}\u{5D1}\u{5D2} ";
    let hebrew = first_line_box_rects(Direction::Rtl, "\u{5D3}", span.clone(), text);
    let latin = first_line_box_rects(Direction::Rtl, "x", span, text);
    assert_eq!(hebrew, [(10.0, 40.0, 15.0, 30.0)]);
    assert_eq!(latin, hebrew);
}

#[test]
fn reordered_wrapped_boxes_keep_preserved_space_and_trim_collapsible_space() {
    use shodo::style::WhiteSpaceCollapse;
    for (direction, lead, text) in [
        (Direction::Ltr, "x", "abc def"),
        (Direction::Ltr, "א", "abc def"),
        (Direction::Rtl, "א", "אבג דהו"),
        (Direction::Rtl, "x", "אבג דהו"),
    ] {
        for (collapse, expected) in [
            (WhiteSpaceCollapse::Collapse, 35.0),
            (WhiteSpaceCollapse::PreserveBreaks, 35.0),
            (WhiteSpaceCollapse::Preserve, 45.0),
            (WhiteSpaceCollapse::PreserveSpaces, 45.0),
            (WhiteSpaceCollapse::BreakSpaces, 45.0),
        ] {
            let inline = InlineStyle {
                font_size: 10.0,
                direction,
                white_space_collapse: collapse,
                ..Default::default()
            };
            let lines = all_lines(direction, 55.0, |b| {
                b.push_text(dom(1), lead)
                    .open_inline(NodeId(2), &inline, padded_5_5())
                    .push_text(dom(3), text)
                    .close_inline();
            });
            assert_eq!(lines.len(), 2, "{direction:?}/{lead}/{collapse:?}");
            let first = boxes(&lines[0]);
            assert_eq!(first.len(), 1);
            assert_eq!(
                first[0].rect.inline_size, expected,
                "{direction:?}/{lead}/{collapse:?}"
            );
            assert_eq!(
                first[0].content_rect.inline_size,
                expected - 5.0,
                "{direction:?}/{lead}/{collapse:?}"
            );
        }
    }
}

#[test]
fn preserved_hanging_space_keeps_its_box_owner_after_bidi_level_reset() {
    use shodo::style::WhiteSpaceCollapse;
    for (collapse, expected) in [
        (WhiteSpaceCollapse::Collapse, 30.0),
        (WhiteSpaceCollapse::PreserveBreaks, 30.0),
        (WhiteSpaceCollapse::Preserve, 40.0),
        (WhiteSpaceCollapse::PreserveSpaces, 40.0),
    ] {
        let mut inline = rtl_isolate();
        inline.white_space_collapse = collapse;
        let lines = all_lines(Direction::Ltr, 35.0, |b| {
            b.open_inline(NodeId(1), &inline, InlineEdges::default())
                .push_text(dom(2), "אבג דהו")
                .close_inline();
        });
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].hang_end(), 10.0);
        let first = boxes(&lines[0]);
        assert_eq!(
            first.iter().map(|b| b.rect.inline_size).sum::<f32>(),
            expected,
            "{collapse:?}"
        );
        assert_eq!(
            first
                .iter()
                .map(|b| b.content_rect.inline_size)
                .sum::<f32>(),
            expected,
            "{collapse:?}"
        );
        let runs = runs(&lines[0]);
        assert_eq!(runs.last().unwrap().inline_start(), 30.0);
        assert_eq!(runs.last().unwrap().glyphs().next().unwrap().advance, 10.0);
    }
}
