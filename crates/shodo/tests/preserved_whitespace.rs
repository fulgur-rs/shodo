mod common;
use common::*;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    LineOptions, TabSize, TextAlign, TextWrapMode, TextWrapStyle, WhiteSpaceCollapse,
};
use shodo::{AtomicIntrinsics, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult};

fn preserved(text: &str, collapse: WhiteSpaceCollapse, wrap: TextWrapMode) -> shodo::Paragraph {
    let mut s = style();
    s.root.white_space_collapse = collapse;
    s.root.text_wrap_mode = wrap;
    s.root.tab_size = TabSize::Px(40.0);
    build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
    })
}

#[test]
fn pre_wrap_consumes_hanging_tab_without_losing_its_source_or_advance() {
    let p = preserved("a\tbbbb", WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
    let line = first_line(&p, 30.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.text_range(), 0..2);
    assert_eq!(line.inline_size(), 10.0);
    assert_eq!(line.hang_end(), 30.0);
    assert_eq!(line.break_reason(), shodo::BreakReason::Regular);
    let layout = shodo::hit::LineLayout::new(std::slice::from_ref(&line));
    let position = |offset| shodo::hit::TextPosition {
        line: 0,
        offset,
        affinity: shodo::mapping::Affinity::Downstream,
    };
    let rects = layout.selection_rects(position(1), position(2));
    assert!(!rects.is_empty(), "preserved tab remains selectable");
    assert_eq!(rects.iter().map(|r| r.inline_size).sum::<f32>(), 30.0);
}

#[test]
fn forced_and_final_pre_wrap_whitespace_only_hangs_its_overflowing_part() {
    for text in ["a\t", "a\t\nb"] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
        for (width, expected) in [(100.0, 40.0), (25.0, 25.0), (5.0, 10.0)] {
            let line = first_line(&p, width, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(line.inline_size(), expected, "{text:?} at {width}");
        }
    }
    for text in ["a ", "a \nb"] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
        for (width, expected) in [(50.0, 20.0), (15.0, 15.0)] {
            let options = LineOptions {
                text_align: TextAlign::Center,
                text_align_last: shodo::style::TextAlignLast::Center,
                ..Default::default()
            };
            let line = first_line(&p, width, &options, &AtomicSizes::EMPTY);
            assert_eq!(line.inline_size(), expected, "{text:?} at {width}");
            let first = line
                .fragments()
                .find_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r.inline_start()),
                    _ => None,
                })
                .unwrap();
            assert_eq!(first, (width - expected) / 2.0);
        }
    }
}

#[test]
fn trailing_whitespace_includes_retained_and_hanging_advances() {
    for (text, advance) in [
        ("a ", 10.0),
        ("a \nb", 10.0),
        ("a\t", 30.0),
        ("a\t\nb", 30.0),
    ] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
        for width in [100.0, 15.0, 5.0] {
            let line = first_line(&p, width, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(line.trailing_whitespace(), advance, "{text:?} at {width}");
            assert_eq!(line.inline_size() + line.hang_end(), 10.0 + advance);
        }
    }
    for (text, advance) in [("a  bbbb", 20.0), ("a\tbbbb", 30.0)] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
        let line = first_line(&p, 30.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.break_reason(), shodo::BreakReason::Regular);
        assert_eq!(line.trailing_whitespace(), advance);
        assert_eq!(line.inline_size(), 10.0);
    }
}

#[test]
fn trailing_whitespace_is_independent_of_hanging_policy() {
    for (collapse, wrap) in [
        (WhiteSpaceCollapse::Preserve, TextWrapMode::NoWrap),
        (WhiteSpaceCollapse::BreakSpaces, TextWrapMode::Wrap),
    ] {
        let p = preserved("a \t", collapse, wrap);
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.trailing_whitespace(), 30.0);
        assert_eq!(line.hang_end(), 0.0);
        assert_eq!(line.inline_size(), 40.0);
    }
    let p = preserved(" \t", WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.trailing_whitespace(), 40.0);
    for text in ["a", "a b", "a\u{a0}"] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.trailing_whitespace(), 0.0, "{text:?}");
    }
}

#[test]
fn trailing_whitespace_ignores_bidi_controls_and_inline_end_edges() {
    let p = preserved(
        "\u{202e}abc \u{202c}",
        WhiteSpaceCollapse::Preserve,
        TextWrapMode::Wrap,
    );
    let line = first_line(&p, 35.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.trailing_whitespace(), 10.0);
    assert_eq!(line.hang_end(), 5.0);

    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.open_inline(
            NodeId(2),
            &s.root,
            InlineEdges {
                padding: Sides {
                    inline_end: 5.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .push_text(TextSource::Generated { node: NodeId(1) }, "a ")
        .push_out_of_flow(NodeId(3), shodo::node::OutOfFlowKind::Absolute)
        .close_inline();
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.trailing_whitespace(), 10.0);
    assert_eq!(line.inline_size(), 25.0);
    assert_eq!(line.hang_end(), 0.0);
}

#[test]
fn trailing_whitespace_uses_final_justified_advances() {
    let p = preserved("a b ", WhiteSpaceCollapse::BreakSpaces, TextWrapMode::Wrap);
    let options = LineOptions {
        text_align: TextAlign::JustifyAll,
        ..Default::default()
    };
    let line = first_line(&p, 100.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(line.inline_size(), 100.0);
    assert_eq!(line.trailing_whitespace(), 40.0);
}

#[test]
fn in_flow_objects_and_combined_squares_end_trailing_whitespace() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a ")
            .push_atomic(NodeId(2), &s.root, InlineEdges::default());
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.trailing_whitespace(), 0.0);

    s.writing_mode = shodo::geometry::WritingMode::VerticalRl;
    s.root.text_combine_upright = shodo::style::TextCombineUpright::All;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a ");
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.text_combinations().len(), 1);
    assert_eq!(line.trailing_whitespace(), 0.0);
}

#[test]
fn a_close_marker_before_the_next_combined_square_is_transparent() {
    let mut s = style();
    s.writing_mode = shodo::geometry::WritingMode::VerticalRl;
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    s.root.text_combine_upright = shodo::style::TextCombineUpright::All;
    let mut child = s.root.clone();
    child.text_combine_upright = shodo::style::TextCombineUpright::None;
    let p = build(&s, |b| {
        b.open_inline(NodeId(1), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(2) }, "a ")
            .close_inline()
            .push_text(TextSource::Generated { node: NodeId(3) }, "12");
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        15.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text_range(), 0..2);
    assert_eq!(lines[1].text_combinations().len(), 1);
    assert_eq!(lines[0].hang_end(), 10.0);
    assert_eq!(lines[0].trailing_whitespace(), 10.0);
}

#[test]
fn nowrap_retains_preserved_spaces_and_tabs_but_keeps_forced_breaks() {
    for (text, width) in [("a ", 20.0), ("a\t", 40.0), ("a \nb", 20.0)] {
        let p = preserved(text, WhiteSpaceCollapse::Preserve, TextWrapMode::NoWrap);
        let line = first_line(&p, 5.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), width, "{text:?}");
        assert_eq!(
            line.text_range().end,
            if text.contains('\n') { 3 } else { 2 }
        );
    }
}

#[test]
fn break_spaces_wraps_between_preserved_spaces_and_keeps_trailing_width() {
    let p = preserved("a  b", WhiteSpaceCollapse::BreakSpaces, TextWrapMode::Wrap);
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        20.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        [(0..2, 20.0), (2..4, 20.0)]
    );
    let p = preserved(
        "a\t\tb",
        WhiteSpaceCollapse::BreakSpaces,
        TextWrapMode::Wrap,
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        40.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        [(0..2, 40.0), (2..3, 40.0), (3..4, 10.0)]
    );
    let p = preserved(
        "a  \nb",
        WhiteSpaceCollapse::BreakSpaces,
        TextWrapMode::NoWrap,
    );
    let line = first_line(&p, 5.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!((line.text_range(), line.inline_size()), (0..4, 30.0));
}

#[test]
fn cached_and_planned_preserved_space_lines_match_direct_geometry() {
    let p = preserved("a  b  ", WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap);
    let mut cx = LayoutContext::new();
    for width in [100.0, 25.0, 40.0, 100.0, 15.0] {
        let LineResult::Line(cached) = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        let direct = first_line(&p, width, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(
            (cached.text_range(), cached.inline_size()),
            (direct.text_range(), direct.inline_size())
        );
        if width == 100.0 {
            assert_eq!(cached.inline_size(), 60.0);
            assert_eq!(cached.trailing_whitespace(), 20.0);
        }
        assert_eq!(cached.trailing_whitespace(), direct.trailing_whitespace());
    }
    for wrap in [TextWrapStyle::Balance, TextWrapStyle::Pretty] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(&mut cx, &options, 100.0, &AtomicSizes::EMPTY);
        let mut constraint = LineConstraint::new(100.0);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        assert_eq!((line.text_range(), line.inline_size()), (0..6, 60.0));
        assert_eq!(line.trailing_whitespace(), 20.0);
    }
}

#[test]
fn intrinsic_widths_distinguish_conditional_hanging_from_preserved_nowrap() {
    for (mode, wrap, min, max) in [
        (WhiteSpaceCollapse::Preserve, TextWrapMode::Wrap, 10.0, 40.0),
        (
            WhiteSpaceCollapse::Preserve,
            TextWrapMode::NoWrap,
            40.0,
            40.0,
        ),
        (
            WhiteSpaceCollapse::BreakSpaces,
            TextWrapMode::Wrap,
            40.0,
            40.0,
        ),
    ] {
        let p = preserved("a\t", mode, wrap);
        let size = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::EMPTY,
        );
        assert_eq!(
            (size.min_content, size.max_content),
            (min, max),
            "{mode:?} {wrap:?}"
        );
    }
}

#[test]
fn nested_nonzero_end_padding_prevents_whitespace_hanging() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.open_inline(
            NodeId(2),
            &s.root,
            InlineEdges {
                padding: Sides {
                    inline_end: 5.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a ",
        )
        .close_inline();
    });
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.inline_size(), 25.0);
}

#[test]
fn preserved_trailing_tracking_is_included_in_max_content() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    s.root.letter_spacing = 2.0;
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a ",
        );
    });
    let sizes = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        &AtomicIntrinsics::EMPTY,
    );
    assert_eq!((sizes.min_content, sizes.max_content), (10.0, 22.0));
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.inline_size(), 22.0);
    assert_eq!(line.trailing_whitespace(), 12.0);
}

#[test]
fn mixed_whitespace_styles_and_first_line_style_keep_their_own_policy() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let mut child = s.root.clone();
    child.white_space_collapse = WhiteSpaceCollapse::BreakSpaces;
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a",
        )
        .open_inline(NodeId(2), &child, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(3),
                offset: 0,
            },
            "  ",
        )
        .close_inline();
    });
    assert_eq!(
        first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY).inline_size(),
        30.0
    );
    child.font_size = 20.0;
    s.first_line = Some(child);
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a  b",
        );
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        25.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        [(0..3, 20.0), (3..4, 10.0)]
    );
}

#[test]
fn float_retry_cache_preserves_space_policy_across_shrinking_widths() {
    for mode in [
        WhiteSpaceCollapse::Preserve,
        WhiteSpaceCollapse::BreakSpaces,
    ] {
        let mut s = style();
        s.root.white_space_collapse = mode;
        let p = build(&s, |b| {
            b.push_out_of_flow(NodeId(2), shodo::node::OutOfFlowKind::Float)
                .push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "a  b  ",
                );
        });
        let mut cx = LayoutContext::new();
        let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(100.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("float")
        };
        for width in [100.0, 20.0, 40.0, 15.0, 100.0] {
            let c = LineConstraint {
                floats_placed_through: Some(float_cursor),
                ..LineConstraint::new(width)
            };
            let LineResult::Line(actual) = p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("line")
            };
            let LineResult::Line(cold) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("cold line")
            };
            assert_eq!(
                (actual.text_range(), actual.inline_size(), actual.hang_end()),
                (cold.text_range(), cold.inline_size(), cold.hang_end()),
                "{mode:?} {width}"
            );
            if width == 100.0 {
                assert_eq!(actual.inline_size(), 60.0);
            }
            if width == 20.0 {
                assert_eq!(
                    (actual.text_range(), actual.inline_size()),
                    if mode == WhiteSpaceCollapse::Preserve {
                        (0..6, 10.0)
                    } else {
                        (0..5, 20.0)
                    }
                );
            }
        }
    }
}

#[test]
fn hanging_tab_does_not_pull_the_next_float_back_across_a_soft_break() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    s.root.tab_size = TabSize::Px(40.0);
    let p = build(&s, |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a\t",
        )
        .push_out_of_flow(NodeId(2), shodo::node::OutOfFlowKind::Float)
        .push_text(
            TextSource::Dom {
                node: NodeId(3),
                offset: 0,
            },
            "bbbb",
        );
    });
    let line = first_line(&p, 30.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!((line.text_range(), line.inline_size()), (0..2, 10.0));
    let LineResult::FloatEncountered {
        inline_position, ..
    } = p.next_line(
        &mut LayoutContext::new(),
        line.break_token(),
        &LineOptions::default(),
        &LineConstraint::new(30.0),
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("next-line float")
    };
    assert_eq!(inline_position, 0.0);
}

#[test]
fn retained_bidi_trailing_space_stays_at_the_paragraph_end() {
    for (mode, wrap, text, width, range, positions) in [
        (
            WhiteSpaceCollapse::BreakSpaces,
            TextWrapMode::Wrap,
            "אבג דהו",
            45.0,
            0..7,
            vec![(0, 20.0), (2, 10.0), (4, 0.0), (6, 30.0)],
        ),
        (
            WhiteSpaceCollapse::Preserve,
            TextWrapMode::Wrap,
            "\u{202e}abc \u{202c}",
            100.0,
            0..10,
            vec![(3, 20.0), (4, 10.0), (5, 0.0), (6, 30.0)],
        ),
        (
            WhiteSpaceCollapse::Preserve,
            TextWrapMode::NoWrap,
            "\u{202e}abc \u{202c}",
            100.0,
            0..10,
            vec![(3, 20.0), (4, 10.0), (5, 0.0), (6, 30.0)],
        ),
        (
            WhiteSpaceCollapse::Preserve,
            TextWrapMode::Wrap,
            "\u{202e}abc \u{202c}",
            35.0,
            0..10,
            vec![(3, 20.0), (4, 10.0), (5, 0.0), (6, 30.0)],
        ),
    ] {
        let p = preserved(text, mode, wrap);
        let line = first_line(&p, width, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), range, "{mode:?} {wrap:?} {width}");
        let mut actual: Vec<_> = glyphs(&line)
            .iter()
            .filter(|g| g.advance > 0.0)
            .map(|g| (g.cluster, g.inline_position))
            .collect();
        actual.sort_by_key(|g| g.0);
        assert_eq!(actual, positions, "{mode:?} {wrap:?} {width}");
        let space = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) if r.text_range().contains(&6) => Some(r.bidi_level()),
                _ => None,
            })
            .unwrap();
        assert_eq!(space, 0);
    }
}

#[test]
fn anywhere_keeps_breaks_between_preserved_spaces_with_cloned_padding() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    s.root.line_break = shodo::style::LineBreak::Anywhere;
    s.root.box_decoration_break = shodo::style::BoxDecorationBreak::Clone;
    let p = build(&s, |b| {
        b.open_inline(
            NodeId(1),
            &s.root,
            InlineEdges {
                padding: Sides {
                    inline_end: 5.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .push_text(TextSource::Generated { node: NodeId(2) }, "a  b")
        .close_inline();
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        25.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        vec![(0..2, 25.0), (2..4, 25.0)]
    );
}

#[test]
fn preserved_space_does_not_override_a_word_joiner_prohibition() {
    let p = preserved(
        "a \u{2060}b",
        WhiteSpaceCollapse::Preserve,
        TextWrapMode::Wrap,
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        15.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        vec![(0..6, 30.0)]
    );
    assert_eq!(
        p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::EMPTY
        )
        .min_content,
        30.0
    );
}

#[test]
fn out_of_flow_anchors_do_not_hide_nonzero_end_padding() {
    for kind in [
        shodo::node::OutOfFlowKind::Float,
        shodo::node::OutOfFlowKind::Absolute,
    ] {
        let mut s = style();
        s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let p = build(&s, |b| {
            b.open_inline(
                NodeId(1),
                &s.root,
                InlineEdges {
                    padding: Sides {
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(2) }, "a b ")
            .push_out_of_flow(NodeId(3), kind)
            .close_inline();
        });
        let mut cx = LayoutContext::new();
        let mut constraint = LineConstraint::new(35.0);
        let mut token = p.start_token();
        let mut actual = Vec::new();
        loop {
            match p.next_line(
                &mut cx,
                token,
                &LineOptions::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) {
                LineResult::FloatEncountered { float_cursor, .. } => {
                    constraint.floats_placed_through = Some(float_cursor)
                }
                LineResult::Line(l) => {
                    actual.push((l.text_range(), l.inline_size()));
                    token = l.break_token();
                }
                _ => break,
            }
        }
        assert_eq!(actual, vec![(0..2, 10.0), (2..7, 25.0)], "{kind:?}");
        let sizes = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicIntrinsics::EMPTY,
        );
        assert_eq!(
            (sizes.min_content, sizes.max_content),
            (25.0, 45.0),
            "{kind:?}"
        );
        // Prime a whole-line float cache, then shrink to the literal split.
        let mut cached = LayoutContext::new();
        let mut wide = LineConstraint::new(100.0);
        loop {
            match p.next_line(
                &mut cached,
                p.start_token(),
                &LineOptions::default(),
                &wide,
                &AtomicSizes::EMPTY,
            ) {
                LineResult::FloatEncountered { float_cursor, .. } => {
                    wide.floats_placed_through = Some(float_cursor)
                }
                LineResult::Line(l) => {
                    assert_eq!((l.text_range(), l.inline_size()), (0..7, 45.0));
                    break;
                }
                _ => panic!("wide line"),
            }
        }
        wide.available_inline_size = 35.0;
        let LineResult::Line(line) = p.next_line(
            &mut cached,
            p.start_token(),
            &LineOptions::default(),
            &wide,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("cached line")
        };
        assert_eq!((line.text_range(), line.inline_size()), (0..2, 10.0));
    }
}

#[test]
fn retained_bidi_space_tracking_follows_visual_order() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::BreakSpaces;
    s.root.letter_spacing = 2.0;
    let p = build(&s, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "אבג דהו");
    });
    let line = first_line(&p, 47.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(
        (line.text_range(), line.inline_size(), line.hang_end()),
        (0..7, 46.0, 0.0)
    );
    let mut positions: Vec<_> = glyphs(&line)
        .iter()
        .map(|g| (g.cluster, g.inline_position))
        .collect();
    positions.sort_by_key(|g| g.0);
    assert_eq!(positions, vec![(0, 24.0), (2, 12.0), (4, 0.0), (6, 36.0)]);
}
