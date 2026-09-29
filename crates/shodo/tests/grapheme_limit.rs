mod common;

use common::*;
use shodo::hit::{LineLayout, TextPosition};
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, TextTransform, WhiteSpaceCollapse};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult};

fn ranges(p: &shodo::Paragraph, limit: usize) -> Vec<std::ops::Range<usize>> {
    p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        1000.0,
        limit,
        &AtomicSizes::EMPTY,
    )
    .iter()
    .map(|line| line.text_range())
    .collect()
}

#[test]
fn grapheme_limit_yields_contiguous_lines_and_progresses_from_zero() {
    let p = paragraph("abcdef");
    let options = LineOptions::default();
    let ranges = |limit| {
        p.break_all_with_grapheme_limit(
            &mut LayoutContext::new(),
            &options,
            100.0,
            limit,
            &AtomicSizes::EMPTY,
        )
        .iter()
        .map(|line| line.text_range())
        .collect::<Vec<_>>()
    };
    assert_eq!(ranges(2), vec![0..2, 2..4, 4..6]);
    assert_eq!(ranges(0), vec![0..1, 1..2, 2..3, 3..4, 4..5, 5..6]);
    assert_eq!(ranges(6), vec![0..6]);
    assert_eq!(ranges(10), vec![0..6]);
    assert!(
        paragraph("")
            .break_all_with_grapheme_limit(
                &mut LayoutContext::new(),
                &options,
                100.0,
                2,
                &AtomicSizes::EMPTY,
            )
            .is_empty()
    );
}

#[test]
fn width_can_break_before_the_character_cap() {
    let p = paragraph("abcdef");
    let lines = p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        25.0,
        3,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text_range())
            .collect::<Vec<_>>(),
        vec![0..2, 2..4, 4..6]
    );
    assert!(lines.iter().all(|line| line.inline_size() <= 25.0));
}

#[test]
fn unicode_graphemes_are_kept_whole() {
    for text in ["あいうえ", "a\u{301}b", "👩‍👩‍👧‍👧x", "ffi"] {
        let p = paragraph(text);
        let lines = p.break_all_with_grapheme_limit(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            1000.0,
            1,
            &AtomicSizes::EMPTY,
        );
        let mut cursor = 0;
        for line in &lines {
            let range = line.text_range();
            assert_eq!(range.start, cursor, "{text:?}");
            assert!(text.is_char_boundary(range.end), "{text:?}");
            assert!(range.end > cursor, "{text:?}");
            cursor = range.end;
        }
        assert_eq!(cursor, p.text().len(), "{text:?}");
        let expected = if text == "ffi" {
            3
        } else if text == "あいうえ" {
            4
        } else {
            2
        };
        assert_eq!(lines.len(), expected, "{text:?}");
    }
}

#[test]
fn caret_positions_follow_the_accepted_grapheme_ranges() {
    let p = paragraph("a\u{301}b");
    let lines = p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        1000.0,
        1,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|line| line.text_range())
            .collect::<Vec<_>>(),
        vec![0..3, 3..4]
    );
    let layout = LineLayout::new(&lines);
    for (line, offset) in [(0, 0), (0, 3), (1, 3), (1, 4)] {
        let caret = layout.caret(TextPosition {
            line,
            offset,
            affinity: Affinity::Downstream,
        });
        assert_eq!(caret.unwrap().position.offset, offset);
    }
}

#[test]
fn transform_expansion_uses_safe_source_boundary() {
    let mut s = style();
    s.root.text_transform = TextTransform::Uppercase;
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "ßa",
        );
    });
    assert_eq!(p.text(), "SSA");
    assert_eq!(ranges(&p, 1), vec![0..2, 2..3]);
}

#[test]
fn first_line_transform_maps_cut_back_to_normal_text() {
    let mut s = style();
    s.first_line = Some(InlineStyle {
        text_transform: TextTransform::Lowercase,
        ..s.root.clone()
    });
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "İx",
        );
    });
    let lines = p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        1000.0,
        1,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text_range(), 0..3);
    assert_eq!(lines[1].text_range(), 2..3);
}

#[test]
fn atomic_and_forced_break_respect_count_and_progress() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .push_atomic(NodeId(2), &s.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b\nc");
    });
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 10.0,
            block_size: 10.0,
            baseline: Some(8.0),
            ..Default::default()
        },
    );
    let lines = p.break_all_with_grapheme_limit(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        1000.0,
        1,
        &sizes,
    );
    assert_eq!(lines.len(), 4);
    assert!(
        lines
            .iter()
            .all(|line| line.text_range().start < line.text_range().end)
    );
}

#[test]
fn forced_break_after_exact_limit_stays_on_the_same_line() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\nb");
    });
    assert_eq!(ranges(&p, 1), vec![0..2, 2..3]);

    let p = build(&s, |b| {
        b.open_inline(NodeId(2), &s.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .close_inline()
            .push_forced_break(NodeId(3))
            .push_text(TextSource::Generated { node: NodeId(4) }, "b");
    });
    assert_eq!(
        p.break_all_with_grapheme_limit(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            1000.0,
            1,
            &AtomicSizes::EMPTY,
        )
        .len(),
        2
    );

    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .open_inline(NodeId(2), &s.root, InlineEdges::default())
            .push_forced_break(NodeId(3))
            .close_inline()
            .push_text(TextSource::Generated { node: NodeId(4) }, "b");
    });
    assert_eq!(ranges(&p, 1), vec![0..2, 2..3]);
}

#[test]
fn processed_space_and_preserved_tab_count_as_one() {
    assert_eq!(ranges(&paragraph("a b"), 2), vec![0..2, 2..3]);
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\tb");
    });
    assert_eq!(ranges(&p, 1), vec![0..1, 1..2, 2..3]);
}

#[test]
fn constraint_and_partial_cache_do_not_reuse_unlimited_scan() {
    let p = paragraph("abcdef");
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let base = LineConstraint::new(100.0);
    let LineResult::Line(whole) = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &base,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(whole.text_range(), 0..6);
    let mut capped = base;
    capped.max_graphemes = Some(2);
    let LineResult::Line(first) = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &capped,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(first.text_range(), 0..2);
    let LineResult::Line(second) = p.next_line(
        &mut cx,
        first.break_token(),
        &options,
        &capped,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(second.text_range(), 2..4);
    let LineResult::Line(whole_again) = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &base,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(whole_again.text_range(), 0..6);
}

#[test]
fn width_only_break_plan_falls_back_to_the_grapheme_limit() {
    let p = paragraph("abcdef");
    let options = LineOptions::default();
    let plan = p.plan_breaks(
        &mut LayoutContext::new(),
        &options,
        100.0,
        &AtomicSizes::EMPTY,
    );
    let mut c = LineConstraint::new(100.0);
    c.max_graphemes = Some(2);
    c.break_plan = Some(&plan);
    let LineResult::Line(first) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &options,
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(first.text_range(), 0..2);
}

#[test]
fn height_and_float_retry_keep_the_grapheme_limit() {
    use shodo::node::OutOfFlowKind;
    let p = build(&style(), |b| {
        b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
    });
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let mut c = LineConstraint::new(100.0);
    c.max_graphemes = Some(2);
    let LineResult::FloatEncountered {
        float_cursor,
        line_start,
        ..
    } = p.next_line(&mut cx, p.start_token(), &options, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    c.floats_placed_through = Some(float_cursor);
    c.max_block_size = Some(0.0);
    assert!(matches!(
        p.next_line(&mut cx, line_start, &options, &c, &AtomicSizes::EMPTY),
        LineResult::BlockSizeExceeded { .. }
    ));
    c.max_block_size = None;
    let LineResult::Line(first) =
        p.next_line(&mut cx, line_start, &options, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    assert_eq!(first.text_range(), 0..5);
}
