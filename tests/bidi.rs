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
            &FontCollection::new(&Limits::default()),
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
            &FontCollection::new(&Limits::default()),
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
