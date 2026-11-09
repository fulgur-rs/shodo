//! Inline box edges sit on the sides of the box's own `direction`, not on
//! the side its bidi level reorders them to (CSS Writing Modes 4 §2.4.1).
//! The expected positions match Chromium.

use crate::font::FontFaceDescriptor;
use crate::geometry::Direction;
use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId, Sides, TextSource};
use crate::style::{FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle, UnicodeBidi};
use crate::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

fn fonts() -> crate::font::FontCollection {
    let limits = Limits::unlimited();
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn style(direction: Direction) -> ParagraphStyle {
    ParagraphStyle {
        direction,
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Latin".into())],
            font_size: 10.0,
            line_height: LineHeight::Px(12.0),
            direction,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn child(parent: &ParagraphStyle, direction: Direction, unicode_bidi: UnicodeBidi) -> InlineStyle {
    InlineStyle {
        direction,
        unicode_bidi,
        ..parent.root.clone()
    }
}

/// Inline-start padding 9 and inline-end padding 4.
fn edges() -> InlineEdges {
    InlineEdges {
        padding: Sides {
            inline_start: 9.0,
            inline_end: 4.0,
            ..Sides::default()
        },
        ..InlineEdges::default()
    }
}

fn source(offset: usize) -> TextSource {
    TextSource::Dom {
        node: NodeId(0),
        offset: offset as u32,
    }
}

fn lines(builder: ParagraphBuilder, width: f32) -> Vec<crate::Line> {
    let p = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();
    p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}

/// `ab` + box (node 1) holding `cd` + `ef` on one line. The fixture font
/// kerns no pair among these letters.
fn simple(para: &ParagraphStyle, inner: &InlineStyle) -> crate::Line {
    let mut b = ParagraphBuilder::new(para, &Limits::unlimited());
    b.push_text(source(0), "ab")
        .open_inline(NodeId(1), inner, edges())
        .push_text(source(2), "cd")
        .close_inline()
        .push_text(source(4), "ef");
    let mut lines = lines(b, 1.0e4);
    assert_eq!(lines.len(), 1);
    lines.pop().unwrap()
}

/// Inline start of the `index`th glyph run in line order. Isolate controls
/// shift text offsets, so runs are found by position instead.
fn run_start(line: &crate::Line, index: usize) -> f32 {
    let mut starts: Vec<f32> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r.inline_start()),
            _ => None,
        })
        .collect();
    starts.sort_by(f32::total_cmp);
    starts[index]
}

fn boxes(line: &crate::Line, node: u64) -> Vec<crate::InlineBoxFragment> {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::InlineBox(b) if b.node == NodeId(node) => Some(b),
            _ => None,
        })
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn end(r: crate::geometry::LogicalRect) -> f32 {
    r.inline_start + r.inline_size
}

/// The box's only fragment on `line`, with its leading and trailing edge
/// widths in line order.
fn sides(line: &crate::Line, node: u64) -> (crate::InlineBoxFragment, f32, f32) {
    let [b] = boxes(line, node)[..] else {
        panic!("one fragment of box {node}")
    };
    (
        b,
        b.content_rect.inline_start - b.rect.inline_start,
        end(b.rect) - end(b.content_rect),
    )
}

#[test]
fn an_opposite_direction_box_puts_its_start_edge_on_the_far_side() {
    // Chromium: `XY<span style="direction:rtl[;unicode-bidi:isolate];
    // padding-left:4px;padding-right:9px">AB</span>CD` puts AB 4px after
    // XY: the right-to-left box's inline-start padding is on its right.
    let para = style(Direction::Ltr);
    for unicode_bidi in [UnicodeBidi::Normal, UnicodeBidi::Isolate] {
        let line = simple(&para, &child(&para, Direction::Rtl, unicode_bidi));
        let (b, leading, trailing) = sides(&line, 1);
        assert!(b.has_start_edge && b.has_end_edge && b.start_edge_is_reversed);
        assert!(
            close(leading, 4.0) && close(trailing, 9.0),
            "{unicode_bidi:?}"
        );
        assert!(close(run_start(&line, 1), b.content_rect.inline_start));
        assert!(close(run_start(&line, 2), end(b.rect)));
    }
}

#[test]
fn a_same_direction_box_keeps_its_start_edge_first() {
    let para = style(Direction::Ltr);
    for unicode_bidi in [UnicodeBidi::Normal, UnicodeBidi::Isolate] {
        let line = simple(&para, &child(&para, Direction::Ltr, unicode_bidi));
        let (b, leading, trailing) = sides(&line, 1);
        assert!(!b.start_edge_is_reversed);
        assert!(
            close(leading, 9.0) && close(trailing, 4.0),
            "{unicode_bidi:?}"
        );
    }
    // A right-to-left paragraph mirrors it: the line runs from the right.
    let para = style(Direction::Rtl);
    let line = simple(&para, &child(&para, Direction::Rtl, UnicodeBidi::Normal));
    let (_, leading, trailing) = sides(&line, 1);
    assert!(close(leading, 9.0) && close(trailing, 4.0));
}

#[test]
fn nested_boxes_each_take_their_own_sides() {
    // Chromium: `AB<b2 rtl isolate><b3 ltr><b4 ltr isolate>CD` with
    // inline-start 3 / inline-end 7 on each box puts b3 7px after AB (b2's
    // end edge is on its left), b4 3px later and CD 3px after that.
    let para = style(Direction::Ltr);
    let pad = InlineEdges {
        padding: Sides {
            inline_start: 3.0,
            inline_end: 7.0,
            ..Sides::default()
        },
        ..InlineEdges::default()
    };
    let mut b = ParagraphBuilder::new(&para, &Limits::unlimited());
    b.push_text(source(0), "ab")
        .open_inline(
            NodeId(2),
            &child(&para, Direction::Rtl, UnicodeBidi::Isolate),
            pad,
        )
        .open_inline(
            NodeId(3),
            &child(&para, Direction::Ltr, UnicodeBidi::Normal),
            pad,
        )
        .open_inline(
            NodeId(4),
            &child(&para, Direction::Ltr, UnicodeBidi::Isolate),
            pad,
        )
        .push_text(source(2), "cd")
        .close_inline()
        .close_inline()
        .close_inline();
    let line = lines(b, 1.0e4).pop().unwrap();
    let (outer, leading, trailing) = sides(&line, 2);
    assert!(close(leading, 7.0) && close(trailing, 3.0));
    let (middle, leading, trailing) = sides(&line, 3);
    assert!(close(
        middle.rect.inline_start,
        outer.rect.inline_start + 7.0
    ));
    assert!(close(leading, 3.0) && close(trailing, 7.0));
    let (inner, leading, trailing) = sides(&line, 4);
    assert!(close(
        inner.rect.inline_start,
        middle.rect.inline_start + 3.0
    ));
    assert!(close(leading, 3.0) && close(trailing, 7.0));
    assert!(close(run_start(&line, 1), inner.content_rect.inline_start));
}

#[test]
fn a_wrapped_opposite_direction_box_puts_each_edge_on_its_own_line_side() {
    // Chromium, 60px wide: `XY<span style="direction:rtl;padding-left:4px;
    // padding-right:9px">AB CD EF</span>GH` keeps AB right after XY (the
    // start padding is right of it) and puts EF 4px into the last line.
    let para = style(Direction::Ltr);
    let inner = child(&para, Direction::Rtl, UnicodeBidi::Normal);
    let (one, ..) = sides(&simple(&para, &inner), 1);
    let mut b = ParagraphBuilder::new(&para, &Limits::unlimited());
    b.push_text(source(0), "ab")
        .open_inline(NodeId(1), &inner, edges())
        .push_text(source(2), "cd ef gh")
        .close_inline()
        .push_text(source(10), "ij");
    // Room for "ab", "cd" and the start padding, but not for "ef".
    let lines = lines(b, end(one.rect) + 1.0);
    assert!(lines.len() >= 2);
    let (head, leading, trailing) = sides(&lines[0], 1);
    assert!(head.has_start_edge && !head.has_end_edge);
    assert!(close(leading, 0.0) && close(trailing, 9.0));
    assert!(close(run_start(&lines[0], 1), head.rect.inline_start));
    let last = lines.last().unwrap();
    let (tail, leading, trailing) = sides(last, 1);
    assert!(!tail.has_start_edge && tail.has_end_edge);
    assert!(close(leading, 4.0) && close(trailing, 0.0));
    assert!(close(run_start(last, 0), tail.content_rect.inline_start));
}

#[test]
fn an_outer_edge_leaving_a_split_inner_box_joins_its_parts() {
    // Cloned edges start the second line at the level of its first content.
    // A right-to-left box C inside a left-to-right box B, both cloned,
    // continues with Hebrew then Latin: L2 puts B's start edge between C's
    // Hebrew and Latin pieces, and once it moves out C is one part whose
    // start edge goes after the Latin text, on C's right.
    let para = style(Direction::Ltr);
    let cloned = |direction| InlineStyle {
        box_decoration_break: crate::style::BoxDecorationBreak::Clone,
        ..child(&para, direction, UnicodeBidi::Normal)
    };
    let mut b = ParagraphBuilder::new(&para, &Limits::unlimited());
    b.push_text(source(0), "ab ")
        .open_inline(NodeId(1), &cloned(Direction::Ltr), edges())
        .open_inline(NodeId(2), &cloned(Direction::Rtl), edges())
        .push_text(source(3), "cd \u{5d0}\u{5d1} ef")
        .close_inline()
        .close_inline();
    let (one, ..) = sides(
        &simple(&para, &child(&para, Direction::Ltr, UnicodeBidi::Normal)),
        1,
    );
    // Room for "ab cd" and the edges, but not for the Hebrew word.
    let lines = lines(b, end(one.rect) + 30.0);
    assert!(lines.len() >= 2, "{}", lines.len());
    let second = &lines[1];
    let (outer, leading, _) = sides(second, 1);
    assert!(outer.has_start_edge && close(leading, 9.0));
    let (inner, leading, trailing) = sides(second, 2);
    assert!(inner.has_start_edge && inner.has_end_edge);
    assert!(close(leading, 4.0) && close(trailing, 9.0));
    // The text fills the content box: no edge is left between the words.
    let runs: Vec<(f32, f32)> = second
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some((r.inline_start(), r.inline_start() + r.inline_size())),
            _ => None,
        })
        .collect();
    let first = runs.iter().map(|r| r.0).fold(f32::INFINITY, f32::min);
    let last = runs.iter().map(|r| r.1).fold(f32::NEG_INFINITY, f32::max);
    assert!(
        close(first, inner.content_rect.inline_start),
        "{runs:?} {inner:?}"
    );
    assert!(close(last, end(inner.content_rect)), "{runs:?} {inner:?}");
}
