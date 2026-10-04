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

fn space_advance(font: usize, size: f32) -> f32 {
    let font = FontRef::from_index(FONTS[font].bytes, 0).unwrap();
    font.glyph_metrics(Size::new(size), LocationRef::default())
        .advance_width(font.charmap().map(' ').unwrap())
        .unwrap()
}

#[test]
fn word_spacing_percent_resolves_against_the_space_advance() {
    // (px term, percent term) pairs from WPT word-spacing-001, including a
    // percent-only value that must still enable the spacing pass.
    for (px, percent) in [
        (0.0, 100.0),
        (0.0, -100.0),
        (0.0, -40.0),
        (0.0, 0.0),
        (0.0, 25.0),
        (40.0, 400.0),
        (-6.0, 50.0),
    ] {
        let used = px + percent / 100.0 * advance(' ');
        for separator in [' ', '\u{a0}'] {
            let text = format!("a{separator}b{separator}c");
            let natural = lines(&paragraph(root(), &text), 1000.0)
                .remove(0)
                .inline_size();
            let mut style = root();
            style.word_spacing = px;
            style.word_spacing_percent = percent;
            let p = paragraph(style, &text);
            let line = lines(&p, 1000.0).remove(0);
            close(line.inline_size(), natural + 2.0 * used);
            let intrinsic = p.intrinsic_sizes(
                &mut LayoutContext::new(),
                &Default::default(),
                &AtomicIntrinsics::EMPTY,
            );
            close(intrinsic.max_content, line.inline_size());
            let glyphs: Vec<_> = line
                .fragments()
                .flat_map(|f| match f {
                    Fragment::GlyphRun(r) => r.glyphs().collect::<Vec<_>>(),
                    _ => Vec::new(),
                })
                .collect();
            close(
                glyphs.last().unwrap().inline_position,
                advance('a') + advance('b') + 2.0 * (advance(' ') + used),
            );
        }
    }
    // -100% cancels the space exactly: the words touch.
    let mut style = root();
    style.word_spacing_percent = -100.0;
    close(
        lines(&paragraph(style, "a b"), 1000.0)
            .remove(0)
            .inline_size(),
        advance('a') + advance('b'),
    );
}

#[test]
fn word_spacing_percent_uses_each_inline_runs_selected_font() {
    let mut style = root();
    style.word_spacing_percent = 50.0;
    let mut large = style.clone();
    large.font_size = 40.0;
    let mut cjk = style.clone();
    cjk.font_families = vec![FontFamily::Named(FONTS[1].family.into())];
    let layout = |percent: f32| {
        let mut style = style.clone();
        let mut large = large.clone();
        let mut cjk = cjk.clone();
        style.word_spacing_percent = percent;
        large.word_spacing_percent = percent;
        cjk.word_spacing_percent = percent;
        let p = build(style, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a b")
                .open_inline(NodeId(2), &large, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, " a b")
                .close_inline()
                .open_inline(NodeId(4), &cjk, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(5) }, " 水 水")
                .close_inline();
        });
        let line = lines(&p, 1000.0).remove(0).inline_size();
        let intrinsic = p
            .intrinsic_sizes(
                &mut LayoutContext::new(),
                &Default::default(),
                &AtomicIntrinsics::EMPTY,
            )
            .max_content;
        close(intrinsic, line);
        line
    };
    let expected = 0.5 * space_advance(0, 20.0)
        + 2.0 * 0.5 * space_advance(0, 40.0)
        + 2.0 * 0.5 * space_advance(1, 20.0);
    close(layout(50.0), layout(0.0) + expected);
}

#[test]
fn space_tab_size_includes_resolved_word_spacing_percent() {
    let mut style = root();
    style.white_space_collapse = WhiteSpaceCollapse::Preserve;
    style.tab_size = TabSize::Spaces(4.0);
    style.word_spacing = 1.0;
    style.word_spacing_percent = 50.0;
    close(
        lines(&paragraph(style, "a\tb"), 1000.0)
            .remove(0)
            .inline_size(),
        (advance(' ') * 1.5 + 1.0) * 4.0 + advance('b'),
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

fn ic(size: f32) -> f32 {
    let font = FontRef::from_index(FONTS[1].bytes, 0).unwrap();
    font.glyph_metrics(Size::new(size), LocationRef::default())
        .advance_width(font.charmap().map('水').unwrap())
        .unwrap()
}

#[test]
fn autospace_uses_visual_classes_and_real_ic() {
    for text in ["水a", "a水", "水1", "1水"] {
        let mut no = root();
        no.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let natural = lines(&paragraph(no, text), 1000.0).remove(0);
        let actual = lines(&paragraph(root(), text), 1000.0).remove(0);
        close(actual.inline_size(), natural.inline_size() + ic(20.0) / 8.0);
    }
    for tail in ["אב", "אב♥"] {
        let layout = |autospace| {
            let mut style = root();
            style.text_autospace = autospace;
            let mut rtl = style.clone();
            rtl.direction = shodo::geometry::Direction::Rtl;
            rtl.unicode_bidi = shodo::style::UnicodeBidi::Isolate;
            let p = build(style, |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "水")
                    .open_inline(NodeId(2), &rtl, InlineEdges::default())
                    .push_text(TextSource::Generated { node: NodeId(3) }, tail)
                    .close_inline();
            });
            lines(&p, 1000.0).remove(0)
        };
        let natural = layout(shodo::style::TextAutospace::NoAutospace);
        let actual = layout(shodo::style::TextAutospace::Normal);
        // The isolate's visual first character is bet or the heart.
        close(
            actual.inline_size(),
            natural.inline_size() + if tail == "אב" { ic(20.0) / 8.0 } else { 0.0 },
        );
    }
}

#[test]
fn autospace_cross_node_boundary_uses_containing_style() {
    let mut style = root();
    style.font_size = 32.0;
    let mut child = root();
    child.text_autospace = shodo::style::TextAutospace::NoAutospace;
    child.font_size = 12.0;
    let p = build(style, |b| {
        b.open_inline(NodeId(1), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(2) }, "水")
            .close_inline()
            .open_inline(NodeId(3), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(4) }, "a")
            .close_inline();
    });
    let natural = lines(&paragraph(child, "水a"), 1000.0).remove(0);
    let line = lines(&p, 1000.0).remove(0);
    close(line.inline_size(), natural.inline_size() + ic(32.0) / 8.0);
    for f in line.fragments() {
        if let Fragment::InlineBox(b) = f
            && b.node == NodeId(1)
        {
            close(b.rect.inline_size, ic(12.0));
        }
    }
}

#[test]
fn autospace_removed_at_soft_wrap_and_intrinsics() {
    for mode in [
        shodo::style::TextWrapStyle::Auto,
        shodo::style::TextWrapStyle::Balance,
        shodo::style::TextWrapStyle::Pretty,
    ] {
        let mut style = root();
        style.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
        let p = paragraph(style, "水a");
        let mut no = root();
        no.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let natural = lines(&paragraph(no, "水a"), 1000.0).remove(0).inline_size();
        let options = shodo::style::LineOptions {
            text_wrap_style: mode,
            ..Default::default()
        };
        let wrapped = p.break_all(
            &mut LayoutContext::new(),
            &options,
            natural + ic(20.0) / 16.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(wrapped.len(), 2);
        close(wrapped[0].inline_size(), ic(20.0));
        close(wrapped[1].inline_size(), advance('a'));
        assert_eq!(wrapped[0].text_range(), 0..3);
        assert_eq!(wrapped[1].text_range(), 3..4);
        let intrinsic = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &options,
            &AtomicIntrinsics::EMPTY,
        );
        close(intrinsic.max_content, natural + ic(20.0) / 8.0);
        close(intrinsic.min_content, ic(20.0).max(advance('a')));
    }
}

#[test]
fn autospace_classification_and_grapheme_marks() {
    for (text, gaps) in [
        ("水e\u{301}", 1),
        ("e\u{301}水", 1),
        ("水ｶ", 1),
        ("水1", 1),
        ("水Ａ", 0),
        ("水１", 0),
        ("水가", 0),
        ("水。a", 0),
        ("水 a", 0),
        ("水♥a", 0),
        ("水\u{200d}a", 1),
        ("水\u{200b}a", 0),
        ("a\u{200b}水", 0),
        ("水\u{200b}1", 0),
        ("1\u{200b}水", 0),
        ("水\u{200b}\u{200b}a", 0),
        ("水\u{200b}\u{200d}a", 0),
        ("水\u{200b}a水", 1),
        ("々a", 1),
        ("㇀a", 1),
    ] {
        let mut no = root();
        no.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let natural = lines(&paragraph(no, text), 1000.0).remove(0);
        let actual = lines(&paragraph(root(), text), 1000.0).remove(0);
        close(
            actual.inline_size(),
            natural.inline_size() + gaps as f32 * ic(20.0) / 8.0,
        );
    }
}

#[test]
fn zero_width_space_blocks_autospace_but_stays_zero_width_and_breakable() {
    // WPT text-autospace-no-001: `normal` with U+200B at each script
    // boundary renders like `no-autospace` without it.
    let mut no = root();
    no.text_autospace = shodo::style::TextAutospace::NoAutospace;
    let plain = "国国AA国国AA国国";
    let separated = "国国\u{200b}AA\u{200b}国国\u{200b}AA\u{200b}国国";
    let natural = lines(&paragraph(no.clone(), plain), 1000.0).remove(0);
    for style in [root(), no] {
        let p = paragraph(style, separated);
        close(
            lines(&p, 1000.0).remove(0).inline_size(),
            natural.inline_size(),
        );
        let intrinsic = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &AtomicIntrinsics::EMPTY,
        );
        close(intrinsic.max_content, natural.inline_size());
    }
    // Placement agrees with measurement: the Latin glyph sits directly
    // after the ideograph, and tracking still crosses U+200B.
    let mut tracked = root();
    tracked.letter_spacing = 2.0;
    let positions = |style: InlineStyle, text: &str| {
        lines(&paragraph(style, text), 1000.0)
            .remove(0)
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r.glyphs().map(|g| g.inline_position).collect()),
                _ => None,
            })
            .flat_map(|v: Vec<f32>| v)
            .collect::<Vec<_>>()
    };
    let mut tracked_no = tracked.clone();
    tracked_no.text_autospace = shodo::style::TextAutospace::NoAutospace;
    let actual = positions(tracked, "水\u{200b}a");
    let expected = positions(tracked_no, "水a");
    close(*actual.last().unwrap(), *expected.last().unwrap());
    close(*expected.last().unwrap(), ic(20.0) + 2.0);
    // The boundary keeps its break opportunity: a line just wider than
    // `水` wraps after U+200B without a reserved autospace gap.
    let p = paragraph(root(), "水\u{200b}a");
    let wrapped = lines(&p, ic(20.0) + 1.0);
    assert_eq!(wrapped.len(), 2);
    close(wrapped[0].inline_size(), ic(20.0));
    close(wrapped[1].inline_size(), advance('a'));
}

#[test]
fn zero_width_space_in_its_own_inline_box_blocks_autospace() {
    let layout = |auto| {
        let mut style = root();
        style.text_autospace = auto;
        let p = build(style.clone(), |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "水")
                .open_inline(NodeId(2), &style, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "\u{200b}")
                .close_inline()
                .push_text(TextSource::Generated { node: NodeId(4) }, "a");
        });
        lines(&p, 1000.0).remove(0)
    };
    close(
        layout(shodo::style::TextAutospace::Normal).inline_size(),
        layout(shodo::style::TextAutospace::NoAutospace).inline_size(),
    );
}

#[test]
fn autospace_and_tracking_compose_with_justification() {
    let mut style = root();
    style.letter_spacing = 2.0;
    style.word_spacing = 3.0;
    let p = paragraph(style, "水a b");
    let mut no = root();
    no.text_autospace = shodo::style::TextAutospace::NoAutospace;
    let natural = lines(&paragraph(no, "水a b"), 1000.0).remove(0);
    let plain = lines(&p, 1000.0).remove(0);
    close(
        plain.inline_size(),
        natural.inline_size() + 6.0 + 3.0 + ic(20.0) / 8.0,
    );
    let options = shodo::style::LineOptions {
        text_align: shodo::style::TextAlign::JustifyAll,
        text_justify: shodo::style::TextJustify::InterWord,
        ..Default::default()
    };
    let justified = p
        .break_all(
            &mut LayoutContext::new(),
            &options,
            plain.inline_size() + 30.0,
            &AtomicSizes::EMPTY,
        )
        .remove(0);
    close(justified.inline_size(), plain.inline_size() + 30.0);
    let glyphs = |line: &Line| {
        line.fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .flat_map(|r| r.glyphs())
            .collect::<Vec<_>>()
    };
    let before = glyphs(&plain);
    let after = glyphs(&justified);
    assert_eq!(
        before.iter().map(|g| g.id).collect::<Vec<_>>(),
        after.iter().map(|g| g.id).collect::<Vec<_>>()
    );
    close(after[1].inline_position, before[1].inline_position);
    close(
        after.last().unwrap().inline_position,
        before.last().unwrap().inline_position + 30.0,
    );
    close(
        lines(&p, 1000.0).remove(0).inline_size(),
        plain.inline_size(),
    );
}

#[test]
fn autospace_checks_the_intervening_physical_inline_edge() {
    for direction in [
        shodo::geometry::Direction::Ltr,
        shodo::geometry::Direction::Rtl,
    ] {
        for left_edge in [true, false] {
            let layout = |auto| {
                let mut style = root();
                style.text_autospace = auto;
                let mut child = style.clone();
                child.direction = direction;
                child.unicode_bidi = shodo::style::UnicodeBidi::Isolate;
                let mut edges = InlineEdges::default();
                if left_edge == (direction == shodo::geometry::Direction::Ltr) {
                    edges.padding.inline_start = 2.0;
                } else {
                    edges.padding.inline_end = 2.0;
                }
                let p = build(style, |b| {
                    b.push_text(TextSource::Generated { node: NodeId(1) }, "水")
                        .open_inline(NodeId(2), &child, edges)
                        .push_text(TextSource::Generated { node: NodeId(3) }, "אב")
                        .close_inline();
                });
                lines(&p, 1000.0).remove(0)
            };
            let no = layout(shodo::style::TextAutospace::NoAutospace);
            let yes = layout(shodo::style::TextAutospace::Normal);
            close(
                yes.inline_size(),
                no.inline_size() + if left_edge { 0.0 } else { ic(20.0) / 8.0 },
            );
        }
    }
}

#[test]
fn autospace_empty_edges_block_but_transparent_markers_do_not() {
    for content in ["", "\u{ad}", "\u{200e}"] {
        for edge in [0.0, 2.0] {
            let layout = |auto| {
                let mut style = root();
                style.text_autospace = auto;
                let p = build(style.clone(), |b| {
                    b.push_text(TextSource::Generated { node: NodeId(1) }, "水")
                        .open_inline(
                            NodeId(2),
                            &style,
                            InlineEdges {
                                padding: shodo::node::Sides {
                                    inline_start: edge,
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        )
                        .push_text(TextSource::Generated { node: NodeId(3) }, content)
                        .close_inline()
                        .push_text(TextSource::Generated { node: NodeId(4) }, "a");
                });
                lines(&p, 1000.0).remove(0)
            };
            let no = layout(shodo::style::TextAutospace::NoAutospace);
            let yes = layout(shodo::style::TextAutospace::Normal);
            assert!(
                (yes.inline_size()
                    - no.inline_size()
                    - if edge == 0.0 { ic(20.0) / 8.0 } else { 0.0 })
                .abs()
                    < 1.0 / 32.0,
                "content {content:?}, edge {edge}: {} vs {}",
                yes.inline_size(),
                no.inline_size()
            );
        }
    }
}

#[test]
fn autospace_first_line_uses_its_transformed_text_and_ic() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut first = root();
    first.font_size = 32.0;
    first.text_transform = shodo::style::TextTransform::FullWidth;
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: root(),
            first_line: Some(first.clone()),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "水a")
        .push_forced_break(NodeId(2))
        .push_text(TextSource::Generated { node: NodeId(3) }, "水a");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = lines(&p, 1000.0);
    assert_eq!(actual.len(), 2);
    first.text_autospace = shodo::style::TextAutospace::NoAutospace;
    close(
        actual[0].inline_size(),
        lines(&paragraph(first, "水a"), 1000.0)
            .remove(0)
            .inline_size(),
    );
    close(
        actual[1].inline_size(),
        ic(20.0) + advance('a') + ic(20.0) / 8.0,
    );
}

#[test]
fn autospace_float_retries_and_tiny_windows_keep_final_geometry() {
    for budget in [None, Some(0), Some(2)] {
        let limits = Limits {
            max_shaping_run_bytes: budget,
            max_reshape_window_bytes: budget,
            ..Default::default()
        };
        let fonts = load_fonts(&limits).unwrap();
        let mut style = root();
        style.letter_spacing = 2.0;
        style.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style,
                ..Default::default()
            },
            &limits,
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "水a")
            .push_out_of_flow(NodeId(2), shodo::node::OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(3) }, "水e\u{301} 水a");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
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
            panic!()
        };
        close(
            inline_position,
            ic(20.0) + advance('a') + 2.0 + ic(20.0) / 8.0,
        );
        for width in [100.0, 60.0, 35.0, 20.0] {
            let constraint = shodo::LineConstraint {
                floats_placed_through: Some(float_cursor),
                ..shodo::LineConstraint::new(width)
            };
            let shodo::LineResult::Line(a) = p.next_line(
                &mut warm,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            let shodo::LineResult::Line(b) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            assert_eq!(a.text_range(), b.text_range());
            close(a.inline_size(), b.inline_size());
            let glyphs = |l: &Line| {
                l.fragments()
                    .filter_map(|f| {
                        if let Fragment::GlyphRun(r) = f {
                            Some(r)
                        } else {
                            None
                        }
                    })
                    .flat_map(|r| r.glyphs())
                    .map(|g| (g.id, g.cluster, g.inline_position, g.advance))
                    .collect::<Vec<_>>()
            };
            assert_eq!(glyphs(&a), glyphs(&b));
        }
    }
}
