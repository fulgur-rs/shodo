use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, TabSize, WhiteSpaceCollapse};
use shodo::{
    AtomicIntrinsics, AtomicSizes, Fragment, LayoutContext, Line, Paragraph, ParagraphBuilder,
};
use shodo_fixtures::{FONTS, load_fonts};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1.0 / 32.0,
        "actual {actual}, expected {expected}"
    );
}

fn root() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 20.0,
        ..Default::default()
    }
}

fn build(style: InlineStyle, add: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style,
            ..Default::default()
        },
        &Limits::default(),
    );
    add(&mut b);
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
}

fn paragraph(style: InlineStyle, text: &str) -> Paragraph {
    build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    })
}

fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}

fn advance(ch: char) -> f32 {
    let font = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    font.glyph_metrics(Size::new(20.0), LocationRef::default())
        .advance_width(font.charmap().map(ch).unwrap())
        .unwrap()
}

#[test]
fn tracking_changes_breaks_and_glyph_advances() {
    let natural = lines(&paragraph(root(), "ab"), 1000.0).remove(0);
    let mut style = root();
    style.letter_spacing = 2.0;
    let p = paragraph(style, "ab");
    let line = lines(&p, 1000.0).remove(0);
    close(line.inline_size(), natural.inline_size() + 2.0);
    let glyphs: Vec<_> = line
        .fragments()
        .flat_map(|f| match f {
            Fragment::GlyphRun(r) => r.glyphs().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    close(glyphs[0].inline_position, 0.0);
    close(glyphs[1].inline_position, advance('a') + 2.0);
    close(glyphs[0].advance, advance('a') + 1.0);
    close(glyphs[1].advance, advance('b') + 1.0);
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &AtomicIntrinsics::EMPTY,
    );
    close(intrinsic.min_content, line.inline_size());
    close(intrinsic.max_content, line.inline_size());
}

#[test]
fn different_style_tracking_uses_visual_half_spacing() {
    let mut style = root();
    style.letter_spacing = 2.0;
    let mut child = style.clone();
    child.letter_spacing = 4.0;
    let p = build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b")
            .close_inline();
    });
    let line = lines(&p, 1000.0).remove(0);
    close(line.inline_size(), advance('a') + advance('b') + 3.0);
    let b = line
        .fragments()
        .find_map(|f| match f {
            Fragment::GlyphRun(r) if r.node() == Some(NodeId(3)) => r.glyphs().next(),
            _ => None,
        })
        .unwrap();
    close(b.inline_position, advance('a') + 3.0);
}

#[test]
fn word_spacing_and_tabs_use_real_space_metrics() {
    for separator in [' ', '\u{a0}'] {
        let text = format!("a{separator}b");
        let natural = lines(&paragraph(root(), &text), 1000.0)
            .remove(0)
            .inline_size();
        let mut style = root();
        style.word_spacing = 3.0;
        close(
            lines(&paragraph(style, &text), 1000.0)
                .remove(0)
                .inline_size(),
            natural + 3.0,
        );
    }
    let mut style = root();
    style.white_space_collapse = WhiteSpaceCollapse::Preserve;
    style.tab_size = TabSize::Spaces(4.0);
    close(
        lines(&paragraph(style.clone(), "a\tb"), 1000.0)
            .remove(0)
            .inline_size(),
        advance(' ') * 4.0 + advance('b'),
    );
    style.tab_size = TabSize::Px(40.0);
    close(
        lines(&paragraph(style, "a\tb"), 1000.0)
            .remove(0)
            .inline_size(),
        40.0 + advance('b'),
    );
}

#[test]
fn tabs_use_block_space_and_skip_too_close_stops() {
    let mut style = root();
    style.white_space_collapse = WhiteSpaceCollapse::Preserve;
    style.tab_size = TabSize::Spaces(4.0);
    let mut child = style.clone();
    child.font_size = 40.0;
    let p = build(style.clone(), |b| {
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "\t")
            .close_inline()
            .push_text(TextSource::Generated { node: NodeId(4) }, "b");
    });
    close(
        lines(&p, 1000.0).remove(0).inline_size(),
        4.0 * advance(' ') + advance('b'),
    );
    style.tab_size = TabSize::Px(advance('a') + 1.0);
    close(
        lines(&paragraph(style, "a\tb"), 1000.0)
            .remove(0)
            .inline_size(),
        2.0 * (advance('a') + 1.0) + advance('b'),
    );
}

#[test]
fn bidi_spacing_candidates_match_final_geometry() {
    let natural = lines(&paragraph(root(), "aאבb"), 1000.0).remove(0);
    let mut style = root();
    style.letter_spacing = 3.0;
    let p = paragraph(style, "aאבb");
    let line = lines(&p, 1000.0).remove(0);
    let original: Vec<_> = natural
        .fragments()
        .flat_map(|f| match f {
            Fragment::GlyphRun(r) => r.glyphs().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    let tracked: Vec<_> = line
        .fragments()
        .flat_map(|f| match f {
            Fragment::GlyphRun(r) => r.glyphs().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    for (rank, cluster) in [0, 3, 1, 5].into_iter().enumerate() {
        let before = original.iter().find(|g| g.cluster == cluster).unwrap();
        let after = tracked.iter().find(|g| g.cluster == cluster).unwrap();
        close(
            after.inline_position,
            before.inline_position + 3.0 * rank as f32,
        );
    }
    close(line.inline_size(), natural.inline_size() + 9.0);
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &AtomicIntrinsics::EMPTY,
    );
    close(intrinsic.max_content, line.inline_size());
}

#[test]
fn tracking_keeps_forced_ligatures_and_grapheme_marks() {
    let mut style = root();
    style.font_features.push(shodo::style::FontFeature {
        tag: *b"liga",
        value: 1,
    });
    let natural = lines(&paragraph(style.clone(), "ffi"), 1000.0).remove(0);
    style.letter_spacing = 2.0;
    let tracked = lines(&paragraph(style.clone(), "ffi"), 1000.0).remove(0);
    close(tracked.inline_size(), natural.inline_size() + 4.0);
    let original: Vec<_> = natural
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().map(|g| g.id).collect::<Vec<_>>())
        .collect();
    let actual: Vec<_> = tracked
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().map(|g| g.id).collect::<Vec<_>>())
        .collect();
    assert_eq!(actual, original);
    let natural = lines(&paragraph(root(), "a\u{301}b"), 1000.0).remove(0);
    let tracked = lines(&paragraph(style, "a\u{301}b"), 1000.0).remove(0);
    close(tracked.inline_size(), natural.inline_size() + 2.0);
}

#[test]
fn cursive_words_do_not_get_tracking_or_intercharacter_gaps() {
    let mut style = root();
    style.font_families = vec![FontFamily::Named(FONTS[2].family.into())];
    let natural = lines(&paragraph(style.clone(), "سلام"), 1000.0).remove(0);
    style.letter_spacing = 5.0;
    let p = paragraph(style, "سلام");
    close(
        lines(&p, 1000.0).remove(0).inline_size(),
        natural.inline_size(),
    );
    let options = shodo::style::LineOptions {
        text_align: shodo::style::TextAlign::JustifyAll,
        text_justify: shodo::style::TextJustify::InterCharacter,
        ..Default::default()
    };
    let line = p
        .break_all(
            &mut LayoutContext::new(),
            &options,
            natural.inline_size() + 60.0,
            &AtomicSizes::EMPTY,
        )
        .remove(0);
    close(line.inline_size(), natural.inline_size());
}

#[test]
fn word_spacing_keeps_separator_centered() {
    let natural = lines(&paragraph(root(), "a b"), 1000.0).remove(0);
    let mut style = root();
    style.word_spacing = 3.0;
    let line = lines(&paragraph(style, "a b"), 1000.0).remove(0);
    let glyph = |line: &Line, offset| {
        line.fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs())
            .find(|g| g.cluster == offset)
            .unwrap()
    };
    close(
        glyph(&line, 1).inline_position,
        glyph(&natural, 1).inline_position + 1.5,
    );
    close(
        glyph(&line, 2).inline_position,
        glyph(&natural, 2).inline_position + 3.0,
    );
}

#[test]
fn spacing_break_candidates_and_plans_use_actual_geometry() {
    for tracking in [-2.0, 2.0] {
        let mut style = root();
        style.letter_spacing = tracking;
        style.word_spacing = 3.0;
        style.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
        let p = paragraph(style, "WW WW");
        let expected = 2.0 * advance('W') + tracking;
        let actual = lines(&p, expected + 0.1);
        assert_eq!(
            actual.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
            vec![0..3, 3..5]
        );
        for line in &actual {
            close(line.inline_size(), expected);
        }
        for wrap in [
            shodo::style::TextWrapStyle::Auto,
            shodo::style::TextWrapStyle::Balance,
            shodo::style::TextWrapStyle::Pretty,
        ] {
            let options = shodo::style::LineOptions {
                text_wrap_style: wrap,
                ..Default::default()
            };
            let plan = p.plan_breaks(
                &mut LayoutContext::new(),
                &options,
                expected + 0.1,
                &AtomicSizes::EMPTY,
            );
            let mut constraint = shodo::LineConstraint::new(expected + 0.1);
            constraint.break_plan = Some(&plan);
            let shodo::LineResult::Line(line) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!();
            };
            assert_eq!(line.text_range(), 0..3);
            close(line.inline_size(), expected);
        }
    }
}

#[test]
fn owned_rtl_word_justification_preserves_marks() {
    for direction in [
        shodo::geometry::Direction::Ltr,
        shodo::geometry::Direction::Rtl,
    ] {
        let fonts = load_fonts(&Limits::default()).unwrap();
        let mut style = root();
        style.font_families = vec![FontFamily::Named(FONTS[2].family.into())];
        style.letter_spacing = 3.0;
        style.word_spacing = 2.0;
        style.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                direction,
                root: style,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "لَبَت لَ\u{ad}بَت");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let width = (1..200)
            .map(|v| v as f32)
            .find(|w| {
                let line = lines(&p, *w).remove(0);
                line.text_range().end == 17 && *w - line.inline_size() > 0.2
            })
            .unwrap();
        let natural = lines(&p, width).remove(0);
        let options = shodo::style::LineOptions {
            text_align: shodo::style::TextAlign::JustifyAll,
            text_justify: shodo::style::TextJustify::InterWord,
            ..Default::default()
        };
        let justified = p
            .break_all(
                &mut LayoutContext::new(),
                &options,
                width,
                &AtomicSizes::EMPTY,
            )
            .remove(0);
        close(justified.inline_size(), width);
        assert_eq!(justified.text_range(), natural.text_range());
        let glyphs = |line: &Line| {
            line.fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .flat_map(|r| r.glyphs())
                .collect::<Vec<_>>()
        };
        let before = glyphs(&natural);
        let after = glyphs(&justified);
        for first in &before {
            let first_after = after
                .iter()
                .find(|g| g.cluster == first.cluster && g.id == first.id)
                .unwrap();
            for mark in before.iter().filter(|g| g.cluster == first.cluster) {
                let mark_after = after
                    .iter()
                    .find(|g| g.cluster == mark.cluster && g.id == mark.id)
                    .unwrap();
                close(
                    mark_after.inline_position - first_after.inline_position,
                    mark.inline_position - first.inline_position,
                );
            }
        }
    }
}

#[test]
fn resource_split_marks_keep_one_typographic_spacing_unit() {
    let limits = Limits {
        max_shaping_run_bytes: Some(2),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let text = "a\u{301}\u{301}\u{301}b";
    let layout = |tracking| {
        let mut style = root();
        style.letter_spacing = tracking;
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style,
                ..Default::default()
            },
            &limits,
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
        lines(
            &b.build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap(),
            1000.0,
        )
        .remove(0)
    };
    let natural = layout(0.0);
    let tracked = layout(3.0);
    close(tracked.inline_size(), natural.inline_size() + 3.0);
    let glyphs = |line: &Line| {
        line.fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs())
            .collect::<Vec<_>>()
    };
    let before = glyphs(&natural);
    let after = glyphs(&tracked);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        assert_eq!((a.id, a.cluster), (b.id, b.cluster));
        close(
            b.inline_position,
            a.inline_position + if a.cluster == 7 { 3.0 } else { 0.0 },
        );
    }
}

#[test]
fn cached_float_retries_match_cold_spacing_and_tab_offsets() {
    for tracking in [-2.0, 2.0] {
        for tab in [false, true] {
            let mut style = root();
            style.letter_spacing = tracking;
            style.word_spacing = 3.0;
            style.white_space_collapse = WhiteSpaceCollapse::Preserve;
            style.tab_size = TabSize::Spaces(4.0);
            let p = build(style, |b| {
                b.push_text(
                    TextSource::Generated { node: NodeId(1) },
                    if tab { "a\tb " } else { "a b " },
                )
                .push_out_of_flow(NodeId(2), shodo::node::OutOfFlowKind::Float)
                .push_text(TextSource::Generated { node: NodeId(3) }, "WW WW");
            });
            let mut warm = LayoutContext::new();
            let shodo::LineResult::FloatEncountered {
                float_cursor,
                inline_position,
                ..
            } = p.next_line(
                &mut warm,
                p.start_token(),
                &Default::default(),
                &shodo::LineConstraint::new(1000.0),
                &AtomicSizes::EMPTY,
            )
            else {
                panic!();
            };
            assert!(inline_position.is_finite());
            for width in [200.0, 120.0, 80.0, 60.0, 40.0, 20.0] {
                let constraint = shodo::LineConstraint {
                    floats_placed_through: Some(float_cursor),
                    inline_start_offset: if tab { 5.0 } else { 0.0 },
                    ..shodo::LineConstraint::new(width)
                };
                let shodo::LineResult::Line(a) = p.next_line(
                    &mut warm,
                    p.start_token(),
                    &Default::default(),
                    &constraint,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!();
                };
                let shodo::LineResult::Line(b) = p.next_line(
                    &mut LayoutContext::new(),
                    p.start_token(),
                    &Default::default(),
                    &constraint,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!();
                };
                assert_eq!(a.text_range(), b.text_range());
                close(a.inline_size(), b.inline_size());
                let glyphs = |line: &Line| {
                    line.fragments()
                        .filter_map(|f| match f {
                            Fragment::GlyphRun(r) => Some(r),
                            _ => None,
                        })
                        .flat_map(|r| r.glyphs())
                        .map(|g| (g.id, g.cluster, g.inline_position, g.advance))
                        .collect::<Vec<_>>()
                };
                assert_eq!(glyphs(&a), glyphs(&b));
            }
        }
    }
}

#[test]
fn consecutive_atomics_are_one_tracking_unit() {
    let mut style = root();
    style.letter_spacing = 3.0;
    let atomic = style.clone();
    let p = build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .push_atomic(NodeId(2), &atomic, InlineEdges::default())
            .push_atomic(NodeId(3), &atomic, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(4) }, "b");
    });
    let mut sizes = AtomicSizes::new();
    for id in [2, 3] {
        sizes.insert(
            NodeId(id),
            shodo::AtomicSize {
                inline_size: 10.0,
                block_size: 10.0,
                baseline: Some(8.0),
                ..Default::default()
            },
        );
    }
    let line = p
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &sizes,
        )
        .remove(0);
    close(line.inline_size(), advance('a') + 20.0 + advance('b') + 6.0);
    let atomics: Vec<_> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::Atomic(a) => Some(a),
            _ => None,
        })
        .collect();
    close(atomics[0].border_rect.inline_size, 10.0);
    close(atomics[1].border_rect.inline_size, 10.0);
    close(atomics[0].border_rect.inline_start, advance('a') + 3.0);
    close(atomics[1].border_rect.inline_start, advance('a') + 13.0);
}

#[test]
fn tracking_half_spacing_belongs_to_each_inline() {
    let mut style = root();
    style.letter_spacing = 2.0;
    let mut child = style.clone();
    child.letter_spacing = 4.0;
    let p = build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .open_inline(
                NodeId(2),
                &child,
                InlineEdges {
                    padding: shodo::node::Sides {
                        inline_start: 2.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(3) }, "b")
            .close_inline();
    });
    let line = lines(&p, 1000.0).remove(0);
    close(line.inline_size(), advance('a') + advance('b') + 5.0);
    for fragment in line.fragments() {
        if let Fragment::GlyphRun(r) = fragment {
            let expected = if r.node() == Some(NodeId(1)) {
                advance('a') + 1.0
            } else {
                advance('b') + 2.0
            };
            close(r.inline_size(), expected);
        }
    }
}

#[test]
fn zero_sum_tracking_still_preserves_each_inline_half() {
    let mut style = root();
    style.letter_spacing = 2.0;
    let mut child = style.clone();
    child.letter_spacing = -2.0;
    let p = build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b")
            .close_inline();
    });
    let line = lines(&p, 1000.0).remove(0);
    close(line.inline_size(), advance('a') + advance('b'));
    for f in line.fragments() {
        if let Fragment::GlyphRun(r) = f {
            if r.node() == Some(NodeId(1)) {
                close(r.inline_size(), advance('a') + 1.0);
            } else {
                close(r.inline_size(), advance('b') - 1.0);
                close(r.glyphs().next().unwrap().inline_position, advance('a'));
            }
        }
    }
}

#[test]
fn rtl_block_tracking_moves_each_visual_character_once() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let layout = |tracking| {
        let mut style = root();
        style.letter_spacing = tracking;
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                direction: shodo::geometry::Direction::Rtl,
                root: style,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "aאבb");
        lines(
            &b.build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap(),
            1000.0,
        )
        .remove(0)
    };
    let natural = layout(0.0);
    let tracked = layout(3.0);
    close(tracked.inline_size(), natural.inline_size() + 9.0);
    let glyphs = |line: &Line| {
        line.fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs())
            .collect::<Vec<_>>()
    };
    let mut before = glyphs(&natural);
    before.sort_by(|a, b| a.inline_position.total_cmp(&b.inline_position));
    let after = glyphs(&tracked);
    for (rank, a) in before.iter().enumerate() {
        let b = after.iter().find(|g| g.cluster == a.cluster).unwrap();
        close(b.inline_position, a.inline_position + 3.0 * rank as f32);
    }
}

#[test]
fn unequal_bidi_edge_styles_change_candidate_widths() {
    let natural = lines(&paragraph(root(), "aאב"), 1000.0).remove(0);
    let natural_pair = lines(&paragraph(root(), "aא"), 1000.0).remove(0);
    let mut style = root();
    style.letter_spacing = 2.0;
    style.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
    let mut alef = style.clone();
    alef.letter_spacing = 6.0;
    let mut bet = style.clone();
    bet.letter_spacing = 10.0;
    let p = build(style, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .open_inline(NodeId(2), &alef, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "א")
            .close_inline()
            .open_inline(NodeId(4), &bet, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(5) }, "ב")
            .close_inline();
    });
    // Visual a→bet→alef has gaps (2+10)/2 + (10+6)/2 = 14.
    close(
        lines(&p, 1000.0).remove(0).inline_size(),
        natural.inline_size() + 14.0,
    );
    let narrow = lines(&p, natural_pair.inline_size() + 4.1);
    assert_eq!(narrow[0].text_range(), 0..3);
    close(narrow[0].inline_size(), natural_pair.inline_size() + 4.0);
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &AtomicIntrinsics::EMPTY,
    );
    close(intrinsic.max_content, natural.inline_size() + 14.0);
}

#[test]
fn visible_soft_hyphen_tracking_reaches_intrinsics_and_plans() {
    let prefix = lines(&paragraph(root(), "ab-"), 1000.0)
        .remove(0)
        .inline_size()
        + 4.0;
    let suffix = lines(&paragraph(root(), "cd"), 1000.0)
        .remove(0)
        .inline_size()
        + 2.0;
    let maximum = lines(&paragraph(root(), "abcd"), 1000.0)
        .remove(0)
        .inline_size()
        + 6.0;
    let mut style = root();
    style.letter_spacing = 2.0;
    let p = paragraph(style, "ab\u{ad}cd");
    let sizes = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &AtomicIntrinsics::EMPTY,
    );
    close(sizes.min_content, prefix.max(suffix));
    close(sizes.max_content, maximum);
    for wrap in [
        shodo::style::TextWrapStyle::Auto,
        shodo::style::TextWrapStyle::Balance,
        shodo::style::TextWrapStyle::Pretty,
    ] {
        let options = shodo::style::LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            prefix + 0.1,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = shodo::LineConstraint::new(prefix + 0.1);
        constraint.break_plan = Some(&plan);
        let shodo::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!();
        };
        assert_eq!(line.text_range(), 0..4);
        close(line.inline_size(), prefix);
        let shodo::LineResult::Line(last) = p.next_line(
            &mut LayoutContext::new(),
            line.break_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!();
        };
        close(last.inline_size(), suffix);
    }
}
