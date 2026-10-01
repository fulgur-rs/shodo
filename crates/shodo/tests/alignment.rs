mod common;
use common::*;
use shodo::style::{LineOptions, TextAlign, TextAlignLast, TextJustify};
use shodo::{AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult};

#[test]
fn alignment_and_indent_use_available_space() {
    let p = paragraph("ab");
    for (align, expected) in [
        (TextAlign::Start, 0.0),
        (TextAlign::End, 80.0),
        (TextAlign::Center, 40.0),
    ] {
        let o = LineOptions {
            text_align: align,
            ..LineOptions::default()
        };
        assert_eq!(
            glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[0].inline_position,
            expected
        );
    }
    let mut o = LineOptions {
        text_align: TextAlign::Center,
        ..LineOptions::default()
    };
    o.text_indent.length = 10.0;
    let mut c = LineConstraint::new(80.0);
    c.inline_start_offset = 10.0;
    let LineResult::Line(l) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &o,
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(glyphs(&l)[0].inline_position, 45.0);
}

#[test]
fn justification_excludes_hanging_spaces_and_preserves_other_lines() {
    let p = paragraph("a b c");
    let o = LineOptions {
        text_align: TextAlign::Justify,
        ..LineOptions::default()
    };
    let plain = first_line(&p, 40.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    let justified = first_line(&p, 40.0, &o, &AtomicSizes::EMPTY);
    assert_eq!(glyphs(&justified)[2].inline_position, 30.0);
    assert_eq!(glyphs(&plain)[2].inline_position, 20.0);
    assert_eq!(justified.inline_size(), 40.0);
    assert_eq!(glyphs(&justified)[3].advance, 10.0);
}

#[test]
fn last_line_auto_and_explicit_justify() {
    let p = paragraph("a b");
    for (align, last, expected) in [
        (TextAlign::Justify, TextAlignLast::Auto, 20.0),
        (TextAlign::JustifyAll, TextAlignLast::Auto, 90.0),
        (TextAlign::Start, TextAlignLast::Justify, 90.0),
    ] {
        let o = LineOptions {
            text_align: align,
            text_align_last: last,
            ..LineOptions::default()
        };
        assert_eq!(
            glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[2].inline_position,
            expected
        );
    }
}

#[test]
fn cluster_advances_and_random_access_share_adjusted_positions() {
    let p = paragraph("a\u{301}b");
    let o = LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::InterCharacter,
        ..LineOptions::default()
    };
    let l = first_line(&p, 40.0, &o, &AtomicSizes::EMPTY);
    let Fragment::GlyphRun(r) = l.fragments().next().unwrap() else {
        panic!()
    };
    assert_eq!(r.glyphs().get(1), r.glyphs().nth(1));
    let clusters: Vec<_> = r.clusters().collect();
    assert_eq!(clusters.len(), 2);
    assert_eq!(clusters[0].text_range, 0..3);
    assert_eq!(clusters[0].advance, 30.0);
    assert_eq!(clusters[0].shaping_advance, 10.0);
    assert_eq!(glyphs(&l)[1].inline_position, 5.0);
}

#[test]
fn rtl_physical_alignment_and_no_justification() {
    let mut root = style();
    root.direction = shodo::geometry::Direction::Rtl;
    let p = build(&root, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "אב",
        );
    });
    for (a, x) in [
        (TextAlign::Start, 0.0),
        (TextAlign::End, 80.0),
        (TextAlign::Left, 80.0),
        (TextAlign::Right, 0.0),
    ] {
        let o = LineOptions {
            text_align: a,
            ..LineOptions::default()
        };
        let l = first_line(&p, 100.0, &o, &AtomicSizes::EMPTY);
        assert_eq!(glyphs(&l)[0].inline_position, x);
    }
    let p = paragraph("a b");
    let o = LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::None,
        ..LineOptions::default()
    };
    assert_eq!(
        glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[2].inline_position,
        // Disabling justification keeps the natural advances at line start.
        20.0
    );
    let o = LineOptions {
        text_align_last: TextAlignLast::Start,
        ..o
    };
    assert_eq!(
        glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[2].inline_position,
        20.0
    );
}

#[test]
fn disabled_justification_matches_wpt_start_alignment() {
    // WPT text-justify-none-001 compares these three scripts against plain
    // start alignment even though text-align-last requests justification.
    for text in ["Latin text", "日本 文字", "อักษรไทย อักษรไทย"]
    {
        let p = paragraph(text);
        let options = LineOptions {
            text_align_last: TextAlignLast::Justify,
            text_justify: TextJustify::None,
            ..Default::default()
        };
        let line = first_line(&p, 1000.0, &options, &AtomicSizes::EMPTY);
        let start = glyphs(&line)
            .iter()
            .map(|g| g.inline_position)
            .fold(f32::INFINITY, f32::min);
        assert_eq!(start, 0.0, "{text:?} must stay at line start");
    }
}

#[test]
fn disabled_justification_preserves_logical_start_and_indent_in_all_flows() {
    use shodo::geometry::{Direction, WritingMode};
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut s = style();
            s.writing_mode = mode;
            s.direction = direction;
            s.root.direction = direction;
            let text = if direction == Direction::Rtl {
                "אב"
            } else {
                "ab"
            };
            let p = build(&s, |b| {
                b.push_text(
                    shodo::node::TextSource::Generated {
                        node: shodo::node::NodeId(1),
                    },
                    text,
                );
            });
            for (align, last) in [
                (TextAlign::Start, TextAlignLast::Justify),
                (TextAlign::JustifyAll, TextAlignLast::Auto),
            ] {
                let mut options = LineOptions {
                    text_align: align,
                    text_align_last: last,
                    text_justify: TextJustify::None,
                    ..Default::default()
                };
                options.text_indent.length = 12.0;
                let line = first_line(&p, 100.0, &options, &AtomicSizes::EMPTY);
                let start = line
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r) => Some(r.inline_start()),
                        _ => None,
                    })
                    .fold(f32::INFINITY, f32::min);
                assert_eq!(start, 12.0, "{mode:?}/{direction:?}/{align:?}");
                assert_eq!(line.inline_size(), 20.0);
            }
        }
    }

    // A plaintext RTL paragraph in an LTR block has its start at the other
    // edge. Two 10px glyphs end at 100 - 12 = 88px, hence start at 68px.
    let mut s = style();
    s.unicode_bidi_plaintext = true;
    let p = build(&s, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "אב",
        );
    });
    let mut options = LineOptions {
        text_align_last: TextAlignLast::Justify,
        text_justify: TextJustify::None,
        ..Default::default()
    };
    options.text_indent.length = 12.0;
    let line = first_line(&p, 100.0, &options, &AtomicSizes::EMPTY);
    let Fragment::GlyphRun(run) = line.fragment(0).unwrap() else {
        panic!()
    };
    assert_eq!(run.inline_start(), 68.0);
    assert_eq!(run.inline_start() + run.inline_size(), 88.0);
}

#[test]
fn disabled_justification_keeps_wrapped_start_and_explicit_last_end() {
    let p = paragraph("a b c");
    let options = LineOptions {
        text_align: TextAlign::Justify,
        text_align_last: TextAlignLast::End,
        text_justify: TextJustify::None,
        ..Default::default()
    };
    let first = first_line(&p, 40.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(first.break_reason(), shodo::BreakReason::Regular);
    assert_eq!(glyphs(&first)[0].inline_position, 0.0);
    assert_eq!(glyphs(&first)[2].inline_position, 20.0);
    let LineResult::Line(last) = p.next_line(
        &mut LayoutContext::new(),
        first.break_token(),
        &options,
        &LineConstraint::new(40.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(glyphs(&last)[0].inline_position, 30.0);

    let mut s = style();
    s.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    let p = build(&s, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "a b\nc",
        );
    });
    let options = LineOptions {
        text_align_last: TextAlignLast::Justify,
        text_justify: TextJustify::None,
        ..Default::default()
    };
    let first = first_line(&p, 100.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(first.break_reason(), shodo::BreakReason::Forced);
    assert_eq!(glyphs(&first)[0].inline_position, 0.0);
    let LineResult::Line(last) = p.next_line(
        &mut LayoutContext::new(),
        first.break_token(),
        &options,
        &LineConstraint::new(100.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(glyphs(&last)[0].inline_position, 0.0);
}

#[test]
fn enabled_unexpandable_justification_and_explicit_alignment_keep_their_fallbacks() {
    let p = paragraph("ab");
    for (last, justify, expected) in [
        (TextAlignLast::Justify, TextJustify::InterWord, 40.0),
        (TextAlignLast::Center, TextJustify::None, 40.0),
        (TextAlignLast::End, TextJustify::None, 80.0),
    ] {
        let options = LineOptions {
            text_align_last: last,
            text_justify: justify,
            ..Default::default()
        };
        let line = first_line(&p, 100.0, &options, &AtomicSizes::EMPTY);
        assert_eq!(
            glyphs(&line)[0].inline_position,
            expected,
            "{last:?}/{justify:?}"
        );
        assert_eq!(line.inline_size(), 20.0);
    }
}

#[test]
fn inter_word_justification_expands_unicode_word_separators() {
    for separator in [
        '\u{a0}',
        '\u{1361}',
        '\u{10100}',
        '\u{10101}',
        '\u{1039f}',
        '\u{1091f}',
    ] {
        let text = format!("a{separator}b");
        for justify in [TextJustify::Auto, TextJustify::InterWord] {
            let options = LineOptions {
                text_align: TextAlign::JustifyAll,
                text_justify: justify,
                ..Default::default()
            };
            let line = first_line(&paragraph(&text), 100.0, &options, &AtomicSizes::EMPTY);
            assert_eq!(line.inline_size(), 100.0, "{separator:?}/{justify:?}");
            let glyphs = glyphs(&line);
            assert_eq!(glyphs[0].inline_position, 0.0);
            assert_eq!(
                glyphs[1].inline_position, 45.0,
                "half the 70px spare precedes the separator"
            );
            assert_eq!(
                glyphs[1].advance, 80.0,
                "the separator absorbs the 70px spare width"
            );
            assert_eq!(glyphs[2].inline_position, 90.0);
        }
    }
}

#[test]
fn nbsp_keeps_its_nonbreaking_and_nonhanging_behavior() {
    let line = first_line(
        &paragraph("a\u{a0}b"),
        25.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        line.text_range(),
        0..4,
        "NBSP does not create a wrap opportunity"
    );
    assert_eq!(line.inline_size(), 30.0);
    let line = first_line(
        &paragraph("a\u{a0}"),
        15.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 20.0);
    assert_eq!(line.hang_end(), 0.0);
    assert_eq!(line.trailing_whitespace(), 0.0);
}
