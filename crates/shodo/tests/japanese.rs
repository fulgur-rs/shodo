mod common;

use common::{first_line, glyphs};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource};
use shodo::style::{
    FontFamily, HangingPunctuation, InlineStyle, LineOptions, ParagraphStyle, TextSpacingTrim,
};
use shodo::{
    AtomicIntrinsics, AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn japanese_with(
    style: ParagraphStyle,
    limits: Limits,
    input: impl FnOnce(&mut ParagraphBuilder),
) -> Paragraph {
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    input(&mut builder);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

fn japanese_style(trim: TextSpacingTrim) -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 16.0,
            lang: Some("ja".into()),
            text_spacing_trim: trim,
            ..InlineStyle::default()
        },
        ..ParagraphStyle::default()
    }
}

fn japanese(text: &str, trim: TextSpacingTrim) -> Paragraph {
    japanese_with(japanese_style(trim), Limits::default(), |builder| {
        for (i, part) in text.split('\n').enumerate() {
            if i != 0 {
                builder.push_forced_break(NodeId(100 + i as u64));
            }
            builder.push_text(
                TextSource::Generated {
                    node: NodeId(1 + i as u64),
                },
                part,
            );
        }
    })
}

#[test]
fn trim_all_preserves_variation_selector_clusters_under_small_shaping_limits() {
    let text = "「\u{fe00}「\u{fe0f}日";
    for budget in [None, Some(1), Some(3), Some(6)] {
        let para = japanese_with(
            japanese_style(TextSpacingTrim::TrimAll),
            Limits {
                max_shaping_run_bytes: budget,
                max_reshape_window_bytes: Some(0),
                ..Default::default()
            },
            |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            },
        );
        let line = first_line(&para, 100., &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..text.len(), "budget {budget:?}");
        assert_eq!(line.inline_size(), 32., "budget {budget:?}");
        let visible: Vec<_> = glyphs(&line)
            .into_iter()
            .filter(|g| g.advance > 0.)
            .collect();
        assert_eq!(
            visible
                .iter()
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            [-8., 0., 16.],
            "budget {budget:?}"
        );
    }
}

#[test]
fn shared_punctuation_grapheme_across_inline_nodes_trims_once_and_maps_back() {
    use shodo::hit::{LineLayout, TextPosition};
    use shodo::mapping::{Affinity, TextOrigin};
    let style = japanese_style(TextSpacingTrim::TrimAll);
    for budget in [None, Some(1)] {
        let para = japanese_with(
            style.clone(),
            Limits {
                max_shaping_run_bytes: budget,
                max_reshape_window_bytes: Some(0),
                ..Default::default()
            },
            |b| {
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "「",
                );
                b.open_inline(NodeId(2), &style.root, InlineEdges::default());
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(3),
                        offset: 0,
                    },
                    "\u{fe00}",
                );
                b.close_inline();
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(4),
                        offset: 0,
                    },
                    "「日",
                );
            },
        );
        let line = first_line(&para, 100., &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..12, "{budget:?}");
        assert_eq!(line.inline_size(), 32., "{budget:?}");
        assert_eq!(
            glyphs(&line)
                .iter()
                .filter(|g| g.advance > 0.)
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            [-8., 0., 16.],
            "{budget:?}"
        );
        let mapping = line.offset_mapping().unwrap();
        assert_eq!(
            mapping.text_to_dom(0, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 0
            })
        );
        assert_eq!(
            mapping.text_to_dom(6, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(4),
                offset: 0
            })
        );
        let lines = [line];
        let layout = LineLayout::new(&lines);
        for (affinity, expected) in [(Affinity::Upstream, 0), (Affinity::Downstream, 6)] {
            let caret = layout
                .caret(TextPosition {
                    line: 0,
                    offset: 3,
                    affinity,
                })
                .unwrap();
            assert_eq!(caret.position.offset, expected, "{budget:?}");
        }
    }
}

#[test]
fn rtl_trim_all_keeps_the_same_physical_ink_as_ltr() {
    for trim in [TextSpacingTrim::TrimAll, TextSpacingTrim::TrimBoth] {
        let mut style = japanese_style(trim);
        style.direction = shodo::geometry::Direction::Rtl;
        let para = japanese_with(style, Limits::default(), |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "「日本」");
        });
        let line = first_line(&para, 100., &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 48., "{trim:?}");
        assert_eq!(line.text_range(), 0..12);
        // The brackets resolve to level 1 and mirror; the two Han characters
        // resolve to level 2. Coordinates increase from the right here.
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            [-8., 24., 8., 40.],
            "{trim:?}"
        );
        let ltr = first_line(
            &japanese("「日本」", trim),
            100.,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let mut actual: Vec<_> = glyphs(&line)
            .iter()
            .map(|g| (line.inline_size() - g.inline_position - 16., g.id))
            .collect();
        let mut expected: Vec<_> = glyphs(&ltr)
            .iter()
            .map(|g| (g.inline_position, g.id))
            .collect();
        actual.sort_by(|a, b| a.0.total_cmp(&b.0));
        expected.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(actual, expected, "{trim:?}");
    }
}

#[test]
fn rtl_hanging_uses_the_inline_start_and_end_after_mirroring() {
    let mut style = japanese_style(TextSpacingTrim::TrimBoth);
    style.direction = shodo::geometry::Direction::Rtl;
    let para = japanese_with(style, Limits::default(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "「日本」");
    });
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            first: true,
            last: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let line = first_line(&para, 32., &options, &AtomicSizes::EMPTY);
    assert_eq!(line.text_range(), 0..12);
    assert_eq!(line.inline_size(), 40.);
    assert_eq!((line.hang_start(), line.hang_end()), (8., 8.));
    assert_eq!(
        glyphs(&line)
            .iter()
            .map(|g| g.inline_position)
            .collect::<Vec<_>>(),
        [-16., 16., 0., 32.]
    );
}

#[test]
fn hanging_start_carets_keep_all_source_offsets_including_outside_the_line() {
    use shodo::hit::{LineLayout, TextPosition};
    use shodo::mapping::Affinity;
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            first: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let line = first_line(
        &japanese("「日本", TextSpacingTrim::SpaceAll),
        100.,
        &options,
        &AtomicSizes::EMPTY,
    );
    let lines = [line];
    let layout = LineLayout::new(&lines);
    for (offset, x) in [(0, -16.), (3, 0.), (6, 16.), (9, 32.)] {
        let caret = layout
            .caret(TextPosition {
                line: 0,
                offset,
                affinity: Affinity::Downstream,
            })
            .unwrap();
        assert_eq!(caret.rect.inline_start, x, "offset {offset}");
    }
    assert_eq!(
        layout
            .hit_test(-15., lines[0].block_size() / 2.)
            .unwrap()
            .position
            .offset,
        0
    );
}

#[test]
fn start_trim_is_included_when_choosing_the_latest_fitting_discretionary_hyphen() {
    let mut style = japanese_style(TextSpacingTrim::TrimStart);
    style.root.hyphenate_character = Some("-".into());
    let reference = japanese_with(style.clone(), Limits::default(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "「abcd-");
    });
    let width = first_line(
        &reference,
        200.,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    )
    .inline_size();
    for with_float in [false, true] {
        let para = japanese_with(style.clone(), Limits::default(), |b| {
            if with_float {
                b.push_out_of_flow(NodeId(99), OutOfFlowKind::Float);
            }
            b.push_text(
                TextSource::Generated { node: NodeId(1) },
                "「ab\u{ad}cd\u{ad}ef",
            );
        });
        let mut cx = LayoutContext::new();
        let mut constraint = LineConstraint::new(width);
        if with_float {
            let LineResult::FloatEncountered { float_cursor, .. } = para.next_line(
                &mut cx,
                para.start_token(),
                &LineOptions::default(),
                &LineConstraint::new(200.),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("float")
            };
            constraint.floats_placed_through = Some(float_cursor);
        }
        let LineResult::Line(line) = para.next_line(
            &mut cx,
            para.start_token(),
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        assert_eq!(line.text_range(), 0..if with_float { 14 } else { 11 });
        assert_eq!(line.inline_size(), width);
    }
}

#[test]
fn normal_trims_adjacent_opening_blank_without_changing_glyph_shapes() {
    // The checked-in CJK face has 1000-unit advances at 1000 units/em
    // for 「 and 日. At size 16, two brackets plus 日 are naturally 48.
    let spaced = first_line(
        &japanese("「「日", TextSpacingTrim::SpaceAll),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    let normal = first_line(
        &japanese("「「日", TextSpacingTrim::Normal),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(spaced.inline_size(), 48.0);
    assert_eq!(normal.inline_size(), 40.0);
    let normal_glyphs = glyphs(&normal);
    assert_eq!(normal_glyphs[1].inline_position, 8.0);
    assert_eq!(normal_glyphs[2].inline_position, 24.0);
    assert_eq!(
        normal_glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
        glyphs(&spaced).iter().map(|g| g.id).collect::<Vec<_>>()
    );
    assert_eq!(normal.text_range(), 0..9);
}

#[test]
fn trim_start_moves_opening_ink_and_reduces_line_measure() {
    let line = first_line(
        &japanese("「日", TextSpacingTrim::TrimStart),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 24.0);
    assert_eq!(glyphs(&line)[0].inline_position, -8.0);
    assert_eq!(glyphs(&line)[1].inline_position, 8.0);
    assert_eq!(line.hang_start(), 0.0);
}

#[test]
fn first_hanging_includes_advance_and_keeps_the_source_and_ink() {
    let line = first_line(
        &japanese("「日本", TextSpacingTrim::SpaceAll),
        100.0,
        &LineOptions {
            hanging_punctuation: HangingPunctuation {
                first: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 48.0);
    assert_eq!(line.hang_start(), 16.0);
    assert_eq!(glyphs(&line)[0].inline_position, -16.0);
    assert_eq!(glyphs(&line)[1].inline_position, 0.0);
    assert_eq!(line.text_range(), 0..9);
    assert!(line.overflow_rect().inline_start < 0.0);
}

#[test]
fn first_hung_opening_advance_is_included_in_inline_size() {
    let mut style = japanese_style(TextSpacingTrim::SpaceAll);
    style.root.font_size = 10.0;
    let para = japanese_with(style, Limits::default(), |builder| {
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "「日本");
    });
    let line = first_line(
        &para,
        100.0,
        &LineOptions {
            hanging_punctuation: HangingPunctuation {
                first: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &AtomicSizes::EMPTY,
    );

    assert_eq!(line.hang_start(), 10.0);
    assert_eq!(line.inline_size(), 30.0);
}

#[test]
fn conditional_hanging_reports_only_the_part_that_does_not_fit() {
    let para = japanese("日本、", TextSpacingTrim::SpaceAll);
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            allow_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let narrow = first_line(&para, 44.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(narrow.inline_size(), 44.0);
    assert_eq!(narrow.hang_end(), 4.0);
    assert_eq!(narrow.text_range(), 0..9);
    assert_eq!(glyphs(&narrow)[2].inline_position, 32.0);
    let fitting = first_line(&para, 48.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(fitting.inline_size(), 48.0);
    assert_eq!(fitting.hang_end(), 0.0);
}

#[test]
fn normal_closing_is_trimmed_only_when_needed_to_fit() {
    for text in ["日本、", "日本。", "日本」"] {
        let para = japanese(text, TextSpacingTrim::Normal);
        for (width, expected) in [(40.0, 40.0), (48.0, 48.0)] {
            let line = first_line(&para, width, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(line.text_range(), 0..9, "{text}");
            assert_eq!(line.inline_size(), expected, "{text}");
            assert_eq!(glyphs(&line)[2].inline_position, 32.0, "{text}");
            assert_eq!(line.hang_end(), 0.0);
        }
    }
}

#[test]
fn ideographic_space_and_ascii_quotes_hang_by_their_actual_advances() {
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            first: true,
            last: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let space = first_line(
        &japanese("\u{3000}日", TextSpacingTrim::SpaceAll),
        16.,
        &options,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        (space.inline_size(), space.hang_start(), space.hang_end()),
        (32., 16., 0.)
    );
    assert_eq!(space.text_range(), 0..6);
    let para = japanese("\"日\"", TextSpacingTrim::SpaceAll);
    let natural = first_line(&para, 200., &LineOptions::default(), &AtomicSizes::EMPTY);
    let shaped = glyphs(&natural);
    let opening = shaped[0].advance;
    let closing = shaped[2].advance;
    let line = first_line(&para, 16., &options, &AtomicSizes::EMPTY);
    assert_eq!(
        (line.inline_size(), line.hang_start(), line.hang_end()),
        (16. + opening, opening, closing)
    );
    assert_eq!(line.text_range(), 0..5);
    assert_eq!(glyphs(&line)[0].inline_position, -opening);
}

#[test]
fn punctuation_hang_composes_with_conditionally_hanging_preserved_space() {
    let mut style = japanese_style(TextSpacingTrim::SpaceAll);
    style.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    let para = japanese_with(style, Limits::default(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日本、 ");
    });
    let natural = first_line(&para, 100., &LineOptions::default(), &AtomicSizes::EMPTY);
    let space = glyphs(&natural).last().unwrap().advance;
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            force_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    for (width, measure, hang) in [(32., 32., 16. + space), (100., 32. + space, 16.)] {
        let line = first_line(&para, width, &options, &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..10);
        assert_eq!((line.inline_size(), line.hang_end()), (measure, hang));
        assert_eq!(glyphs(&line)[2].inline_position, 32.);
    }
}

fn inter_character_options() -> LineOptions {
    LineOptions {
        text_align: shodo::style::TextAlign::JustifyAll,
        text_justify: shodo::style::TextJustify::InterCharacter,
        ..Default::default()
    }
}

#[test]
fn japanese_justification_expands_han_boundaries_but_not_bracket_neighbors() {
    for (text, width, positions) in [
        ("日本語", 80., vec![0., 32., 64.]),
        ("日「日本」語", 128., vec![0., 16., 32., 80., 96., 112.]),
    ] {
        let line = first_line(
            &japanese(text, TextSpacingTrim::SpaceAll),
            width,
            &inter_character_options(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.inline_size(), width, "{text}");
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            positions,
            "{text}"
        );
    }
}

#[test]
fn japanese_justification_does_not_expand_either_side_of_protected_punctuation() {
    for ch in [
        '「', '」', '、', '。', '・', '：', '；', '！', '？', '‐', '‑', '‒', '–', '—', '―', '〜',
        '゠', '\u{3000}',
    ] {
        let text = format!("日{ch}本");
        let para = japanese(&text, TextSpacingTrim::SpaceAll);
        let natural = first_line(&para, 200., &LineOptions::default(), &AtomicSizes::EMPTY);
        let line = first_line(
            &para,
            natural.inline_size() + 32.,
            &inter_character_options(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.inline_size(), natural.inline_size(), "{ch}");
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            glyphs(&natural)
                .iter()
                .map(|g| g.inline_position + 16.)
                .collect::<Vec<_>>(),
            "{ch}"
        );
    }
}

#[test]
fn japanese_justification_keeps_consecutive_ellipsis_and_dash_unseparated() {
    for text in ["……", "――", "——"] {
        let para = japanese(text, TextSpacingTrim::SpaceAll);
        let natural = first_line(&para, 200., &LineOptions::default(), &AtomicSizes::EMPTY);
        let line = first_line(
            &para,
            natural.inline_size() + 32.,
            &inter_character_options(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.inline_size(), natural.inline_size(), "{text}");
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            glyphs(&natural)
                .iter()
                .map(|g| g.inline_position + 16.)
                .collect::<Vec<_>>(),
            "{text}"
        );
    }
}

#[test]
fn japanese_justification_preserves_explicit_latin_inter_character_spacing() {
    for language in ["ja", "en"] {
        for text in ["abc", "a·b", "a‐b"] {
            // Japanese hyphens and middle dots have exclusions; the English
            // override keeps explicit Western inter-character behavior.
            if language == "ja" && text != "abc" {
                continue;
            }
            let mut style = japanese_style(TextSpacingTrim::SpaceAll);
            style.root.lang = Some(language.into());
            let para = japanese_with(style, Limits::default(), |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            });
            let natural = first_line(&para, 200., &LineOptions::default(), &AtomicSizes::EMPTY);
            let line = first_line(
                &para,
                natural.inline_size() + 32.,
                &inter_character_options(),
                &AtomicSizes::EMPTY,
            );
            assert_eq!(
                line.inline_size(),
                natural.inline_size() + 32.,
                "{language}: {text}"
            );
            assert_eq!(
                glyphs(&line)
                    .iter()
                    .map(|g| g.inline_position)
                    .collect::<Vec<_>>(),
                glyphs(&natural)
                    .iter()
                    .enumerate()
                    .map(|(i, g)| g.inline_position + i as f32 * 16.)
                    .collect::<Vec<_>>(),
                "{language}: {text}"
            );
        }
    }
}

#[test]
fn japanese_justification_expands_han_kana_and_preserves_selector_graphemes() {
    let plain = first_line(
        &japanese("日あカ本", TextSpacingTrim::SpaceAll),
        112.,
        &inter_character_options(),
        &AtomicSizes::EMPTY,
    );
    let visible_ids: Vec<_> = glyphs(&plain).iter().map(|g| g.id).collect();
    assert!(plain.fragments().all(|f| match f {
        shodo::Fragment::GlyphRun(r) => r.font_data().is_some(),
        _ => true,
    }));
    for text in ["日あカ本", "日\u{fe00}あカ本"] {
        let para = japanese(text, TextSpacingTrim::SpaceAll);
        let line = first_line(&para, 112., &inter_character_options(), &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..text.len());
        assert_eq!(line.inline_size(), 112.);
        assert_eq!(
            glyphs(&line)
                .iter()
                .filter(|g| visible_ids.contains(&g.id))
                .map(|g| g.inline_position)
                .collect::<Vec<_>>(),
            [0., 32., 64., 96.]
        );
    }
}

#[test]
fn space_first_preserves_first_and_forced_heads_but_trims_soft_heads() {
    for (text, second_width) in [("「日「日", 24.0), ("「日\n「日", 32.0)] {
        let para = japanese(text, TextSpacingTrim::SpaceFirst);
        let first = first_line(&para, 32.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(first.inline_size(), 32.0);
        let LineResult::Line(second) = para.next_line(
            &mut LayoutContext::new(),
            first.break_token(),
            &LineOptions::default(),
            &LineConstraint::new(32.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("second line")
        };
        assert_eq!(second.inline_size(), second_width);
        assert_eq!(
            glyphs(&second)[0].inline_position,
            if second_width == 24.0 { -8.0 } else { 0.0 }
        );
    }
}

#[test]
fn force_end_and_last_ignore_full_advance_even_when_punctuation_fits() {
    for (text, hanging) in [
        (
            "日本、",
            HangingPunctuation {
                force_end: true,
                ..Default::default()
            },
        ),
        (
            "日本」",
            HangingPunctuation {
                last: true,
                ..Default::default()
            },
        ),
    ] {
        let line = first_line(
            &japanese(text, TextSpacingTrim::SpaceAll),
            100.0,
            &LineOptions {
                hanging_punctuation: hanging,
                ..Default::default()
            },
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.inline_size(), 32.0);
        assert_eq!(line.hang_end(), 16.0);
        assert_eq!(glyphs(&line)[2].inline_position, 32.0);
        assert_eq!(line.text_range(), 0..9);
    }
}

#[test]
fn conditional_and_forced_hanging_have_distinct_intrinsic_widths() {
    let para = japanese("日本、", TextSpacingTrim::SpaceAll);
    for (hanging, min, max) in [
        (HangingPunctuation::default(), 32.0, 48.0),
        (
            HangingPunctuation {
                allow_end: true,
                ..Default::default()
            },
            16.0,
            48.0,
        ),
        (
            HangingPunctuation {
                force_end: true,
                ..Default::default()
            },
            16.0,
            32.0,
        ),
    ] {
        let sizes = para.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions {
                hanging_punctuation: hanging,
                ..Default::default()
            },
            &AtomicIntrinsics::default(),
        );
        assert_eq!((sizes.min_content, sizes.max_content), (min, max));
    }
}

#[test]
fn first_hanging_is_also_excluded_from_intrinsic_sizes() {
    let sizes = japanese("「日本", TextSpacingTrim::SpaceAll).intrinsic_sizes(
        &mut LayoutContext::new(),
        &LineOptions {
            hanging_punctuation: HangingPunctuation {
                first: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &AtomicIntrinsics::default(),
    );
    assert_eq!((sizes.min_content, sizes.max_content), (16.0, 32.0));
}

#[test]
fn spacing_trim_values_control_fullwidth_edges_and_middle_punctuation() {
    for (trim, width, first, final_position) in [
        (TextSpacingTrim::Normal, 48.0, 0.0, 32.0),
        (TextSpacingTrim::SpaceAll, 48.0, 0.0, 32.0),
        (TextSpacingTrim::TrimStart, 40.0, -8.0, 24.0),
        (TextSpacingTrim::SpaceFirst, 48.0, 0.0, 32.0),
        (TextSpacingTrim::TrimBoth, 32.0, -8.0, 24.0),
        (TextSpacingTrim::TrimAll, 32.0, -8.0, 24.0),
        (TextSpacingTrim::Auto, 32.0, -8.0, 24.0),
    ] {
        let line = first_line(
            &japanese("「日」", trim),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.inline_size(), width, "{trim:?}");
        assert_eq!(glyphs(&line)[0].inline_position, first, "{trim:?}");
        assert_eq!(glyphs(&line)[2].inline_position, final_position, "{trim:?}");
    }
    let line = first_line(
        &japanese("日・日", TextSpacingTrim::TrimAll),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 40.0);
    assert_eq!(glyphs(&line)[1].inline_position, 12.0);
    assert_eq!(glyphs(&line)[2].inline_position, 24.0);
}

#[test]
fn trim_all_does_not_repeat_adjacent_or_hanging_removals() {
    let line = first_line(
        &japanese("「「日", TextSpacingTrim::TrimAll),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 32.0);
    assert_eq!(glyphs(&line)[1].inline_position, 0.0);
    assert_eq!(glyphs(&line)[2].inline_position, 16.0);
    let line = first_line(
        &japanese("日本、", TextSpacingTrim::TrimAll),
        100.0,
        &LineOptions {
            hanging_punctuation: HangingPunctuation {
                force_end: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 32.0);
    assert_eq!(line.hang_end(), 8.0);
}

#[test]
fn last_hanging_fits_before_transparent_inline_end_markers() {
    let style = japanese_style(TextSpacingTrim::SpaceAll);
    let inline = style.root.clone();
    let para = japanese_with(style, Limits::default(), |b| {
        b.open_inline(NodeId(2), &inline, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "日本」")
            .close_inline();
    });
    let line = first_line(
        &para,
        32.0,
        &LineOptions {
            hanging_punctuation: HangingPunctuation {
                last: true,
                ..Default::default()
            },
            ..Default::default()
        },
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.text_range(), 0..9);
    assert_eq!(line.inline_size(), 32.0);
    assert_eq!(line.hang_end(), 16.0);
}

#[test]
fn borders_and_padding_block_hanging_but_margin_does_not() {
    for (edges, expected_hang, expected_size) in [
        (
            InlineEdges {
                padding: Sides {
                    inline_start: 4.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            0.0,
            52.0,
        ),
        (
            InlineEdges {
                border: Sides {
                    inline_start: 4.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            0.0,
            52.0,
        ),
        (
            InlineEdges {
                margin: Sides {
                    inline_start: 4.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            16.0,
            52.0,
        ),
    ] {
        let style = japanese_style(TextSpacingTrim::SpaceAll);
        let inline = style.root.clone();
        let para = japanese_with(style, Limits::default(), |b| {
            b.open_inline(NodeId(2), &inline, edges)
                .push_text(TextSource::Generated { node: NodeId(3) }, "「日本")
                .close_inline();
        });
        let line = first_line(
            &para,
            100.0,
            &LineOptions {
                hanging_punctuation: HangingPunctuation {
                    first: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &AtomicSizes::EMPTY,
        );
        assert_eq!(line.hang_start(), expected_hang);
        assert_eq!(line.inline_size(), expected_size);
    }
}

#[test]
fn adjacent_punctuation_uses_asymmetric_font_size_rules_across_items() {
    for (closing_size, opening_size, width, position) in [
        (16.0, 16.0, 24.0, 8.0),
        (32.0, 16.0, 40.0, 24.0),
        (16.0, 32.0, 40.0, 8.0),
    ] {
        let style = japanese_style(TextSpacingTrim::Normal);
        let mut closing = style.root.clone();
        closing.font_size = closing_size;
        let mut opening = style.root.clone();
        opening.font_size = opening_size;
        let para = japanese_with(style, Limits::default(), |b| {
            b.open_inline(NodeId(2), &closing, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "」")
                .close_inline()
                .open_inline(NodeId(4), &opening, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(5) }, "「")
                .close_inline();
        });
        let line = first_line(&para, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), width);
        assert_eq!(glyphs(&line)[1].inline_position, position);
        assert_eq!(line.text_range(), 0..6);
    }
}

#[test]
fn first_line_font_instance_and_following_line_have_their_own_trim_amounts() {
    let mut style = japanese_style(TextSpacingTrim::TrimBoth);
    let mut first = style.root.clone();
    first.font_size = 32.0;
    style.first_line = Some(first);
    let para = japanese_with(style, Limits::default(), |b| {
        b.push_text(
            TextSource::Generated { node: NodeId(1) },
            "「日本」「日本」",
        );
    });
    let lines = para.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        96.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text_range(), 0..12);
    assert_eq!(lines[0].inline_size(), 96.0);
    assert_eq!(glyphs(&lines[0])[0].inline_position, -16.0);
    assert_eq!(lines[1].text_range(), 12..24);
    assert_eq!(lines[1].inline_size(), 48.0);
    assert_eq!(glyphs(&lines[1])[0].inline_position, -8.0);
}

#[test]
fn conditional_hanging_retries_same_token_after_height_rejection() {
    let para = japanese("日本、", TextSpacingTrim::SpaceAll);
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            allow_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let token = para.start_token();
    let mut constraint = LineConstraint::new(44.0);
    constraint.max_block_size = Some(0.0);
    assert!(matches!(
        para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY),
        LineResult::BlockSizeExceeded { .. }
    ));
    constraint.max_block_size = None;
    let LineResult::Line(line) =
        para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
    else {
        panic!("retry")
    };
    let fresh = first_line(&para, 44.0, &options, &AtomicSizes::EMPTY);
    assert_eq!(line.text_range(), 0..9);
    assert_eq!((line.inline_size(), line.hang_end()), (44.0, 4.0));
    assert_eq!(
        glyphs(&line)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>(),
        glyphs(&fresh)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>()
    );
}

#[test]
fn conditional_hanging_float_cache_reuses_the_same_width_and_ink_rules() {
    let para = japanese_with(
        japanese_style(TextSpacingTrim::SpaceAll),
        Limits::default(),
        |b| {
            b.push_out_of_flow(NodeId(20), OutOfFlowKind::Float)
                .push_text(TextSource::Generated { node: NodeId(1) }, "日本、");
        },
    );
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            allow_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let token = para.start_token();
    let LineResult::FloatEncountered {
        line_start,
        float_cursor,
        ..
    } = para.next_line(
        &mut cx,
        token,
        &options,
        &LineConstraint::new(64.0),
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("float")
    };
    assert_eq!(line_start, token);
    let mut constraint = LineConstraint::new(44.0);
    constraint.inline_start_offset = 12.0;
    constraint.floats_placed_through = Some(float_cursor);
    let LineResult::Line(cached) =
        para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
    else {
        panic!("cached")
    };
    let LineResult::Line(fresh) = para.next_line(
        &mut LayoutContext::new(),
        token,
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("fresh")
    };
    // The out-of-flow source anchor occupies one U+FFFC (three UTF-8 bytes)
    // before the nine bytes of visible Japanese text.
    assert_eq!(cached.text_range(), 0..12);
    assert_eq!((cached.inline_size(), cached.hang_end()), (44.0, 4.0));
    assert_eq!(glyphs(&cached)[2].inline_position, 44.0);
    assert_eq!(
        glyphs(&cached)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>(),
        glyphs(&fresh)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>()
    );
}

#[test]
fn planned_greedy_breaks_preserve_punctuation_measure_and_positions() {
    let para = japanese("「日本、日「日本」日日本", TextSpacingTrim::TrimBoth);
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            first: true,
            allow_end: true,
            last: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let greedy = para.break_all(
        &mut LayoutContext::new(),
        &options,
        44.0,
        &AtomicSizes::EMPTY,
    );
    let mut cx = LayoutContext::new();
    let plan = para.plan_breaks(&mut cx, &options, 44.0, &AtomicSizes::EMPTY);
    let mut constraint = LineConstraint::new(44.0);
    constraint.break_plan = Some(&plan);
    let mut token = para.start_token();
    for expected in &greedy {
        let LineResult::Line(line) =
            para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
        else {
            panic!("planned")
        };
        assert_eq!(line.text_range(), expected.text_range());
        assert_eq!(
            (line.inline_size(), line.hang_start(), line.hang_end()),
            (
                expected.inline_size(),
                expected.hang_start(),
                expected.hang_end()
            )
        );
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| (g.id, g.inline_position))
                .collect::<Vec<_>>(),
            glyphs(expected)
                .iter()
                .map(|g| (g.id, g.inline_position))
                .collect::<Vec<_>>()
        );
        token = line.break_token();
    }
    assert!(matches!(
        para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY),
        LineResult::Done
    ));
}

#[test]
fn first_and_last_hanging_share_one_typographic_advance() {
    for text in ["\"", "”", "“"] {
        for trim in [
            TextSpacingTrim::SpaceAll,
            TextSpacingTrim::TrimBoth,
            TextSpacingTrim::TrimAll,
        ] {
            let para = japanese(text, trim);
            let options = LineOptions {
                hanging_punctuation: HangingPunctuation {
                    first: true,
                    last: true,
                    ..Default::default()
                },
                ..Default::default()
            };
            let line = first_line(&para, 100.0, &options, &AtomicSizes::EMPTY);
            assert_eq!(line.inline_size(), line.hang_start(), "{text:?} {trim:?}");
            assert_eq!(line.hang_end(), 0.0, "already hung at the start");
            assert!(line.hang_start() > 0.0);
            assert_eq!(line.text_range(), 0..text.len());
            let sizes = para.intrinsic_sizes(
                &mut LayoutContext::new(),
                &options,
                &AtomicIntrinsics::default(),
            );
            assert_eq!((sizes.min_content, sizes.max_content), (0.0, 0.0));
            let plain = first_line(
                &japanese(text, TextSpacingTrim::SpaceAll),
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            assert_eq!(glyphs(&line)[0].inline_position, -glyphs(&plain)[0].advance);
        }
    }
}

#[test]
fn slice_inline_end_padding_blocks_hanging_only_on_its_actual_end() {
    use shodo::style::BoxDecorationBreak;
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            force_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    for (decoration, text, width, end, measure, hang, min_content, max_content) in [
        (
            BoxDecorationBreak::Slice,
            "日本、日本",
            32.0,
            9,
            32.0,
            16.0,
            20.0,
            84.0,
        ),
        (
            BoxDecorationBreak::Clone,
            "日本、日本",
            32.0,
            3,
            20.0,
            0.0,
            36.0,
            84.0,
        ),
        (
            BoxDecorationBreak::Slice,
            "日本、",
            100.0,
            9,
            52.0,
            0.0,
            36.0,
            52.0,
        ),
    ] {
        let style = japanese_style(TextSpacingTrim::SpaceAll);
        let inline = InlineStyle {
            box_decoration_break: decoration,
            ..style.root.clone()
        };
        let para = japanese_with(style, Limits::default(), |b| {
            b.open_inline(
                NodeId(2),
                &inline,
                InlineEdges {
                    padding: Sides {
                        inline_end: 4.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(3) }, text)
            .close_inline();
        });
        let sizes = para.intrinsic_sizes(
            &mut LayoutContext::new(),
            &options,
            &AtomicIntrinsics::default(),
        );
        assert_eq!(
            (sizes.min_content, sizes.max_content),
            (min_content, max_content),
            "{decoration:?} {text}"
        );
        let line = first_line(&para, width, &options, &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..end, "{decoration:?} {text}");
        assert_eq!((line.inline_size(), line.hang_end()), (measure, hang));
        assert_eq!(glyphs(&line)[0].inline_position, 0.0);
        if hang > 0.0 {
            assert_eq!(glyphs(&line)[2].inline_position, 32.0);
        }
        let plan = para.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            width,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = LineConstraint::new(width);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(planned) = para.next_line(
            &mut LayoutContext::new(),
            para.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned line")
        };
        assert_eq!(
            (
                planned.text_range(),
                planned.inline_size(),
                planned.hang_end()
            ),
            (line.text_range(), measure, hang)
        );
    }
}

#[test]
fn slice_inline_start_padding_does_not_block_trim_on_a_continuation() {
    use shodo::style::BoxDecorationBreak;
    for (decoration, end, measure, position) in [
        (BoxDecorationBreak::Slice, 9, 24.0, -8.0),
        // Opening punctuation stays with the following ideograph even when
        // the untrimmed pair and cloned padding overflow the 32px constraint.
        (BoxDecorationBreak::Clone, 9, 36.0, 4.0),
    ] {
        let style = japanese_style(TextSpacingTrim::TrimStart);
        let inline = InlineStyle {
            box_decoration_break: decoration,
            ..style.root.clone()
        };
        let para = japanese_with(style, Limits::default(), |b| {
            b.open_inline(
                NodeId(2),
                &inline,
                InlineEdges {
                    padding: Sides {
                        inline_start: 4.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(3) }, "日「日本")
            .close_inline();
        });
        let first = first_line(&para, 32.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(first.text_range(), 0..3);
        let LineResult::Line(line) = para.next_line(
            &mut LayoutContext::new(),
            first.break_token(),
            &LineOptions::default(),
            &LineConstraint::new(32.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("continuation")
        };
        assert_eq!(line.text_range(), 3..end, "{decoration:?}");
        assert_eq!(line.inline_size(), measure);
        assert_eq!(glyphs(&line)[0].inline_position, position);
    }
}

#[test]
fn slice_end_padding_keeps_hanging_consistent_through_a_float_retry() {
    let style = japanese_style(TextSpacingTrim::SpaceAll);
    let inline = style.root.clone();
    let para = japanese_with(style, Limits::default(), |b| {
        b.push_out_of_flow(NodeId(20), OutOfFlowKind::Float)
            .open_inline(
                NodeId(2),
                &inline,
                InlineEdges {
                    padding: Sides {
                        inline_end: 4.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(3) }, "日本、日本")
            .close_inline();
    });
    let options = LineOptions {
        hanging_punctuation: HangingPunctuation {
            force_end: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let token = para.start_token();
    let mut cx = LayoutContext::new();
    let LineResult::FloatEncountered { float_cursor, .. } = para.next_line(
        &mut cx,
        token,
        &options,
        &LineConstraint::new(200.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("float")
    };
    let mut constraint = LineConstraint::new(32.0);
    constraint.inline_start_offset = 12.0;
    constraint.floats_placed_through = Some(float_cursor);
    let LineResult::Line(cached) =
        para.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
    else {
        panic!("cached")
    };
    let LineResult::Line(fresh) = para.next_line(
        &mut LayoutContext::new(),
        token,
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("fresh")
    };
    assert_eq!(cached.text_range(), 0..12);
    assert_eq!((cached.inline_size(), cached.hang_end()), (32.0, 16.0));
    assert_eq!(glyphs(&cached)[2].inline_position, 44.0);
    assert_eq!(
        (fresh.text_range(), fresh.inline_size(), fresh.hang_end()),
        (cached.text_range(), 32.0, 16.0)
    );
    assert_eq!(
        glyphs(&cached)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>(),
        glyphs(&fresh)
            .iter()
            .map(|g| (g.id, g.inline_position))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rtl_slice_padding_blocks_the_actual_logical_hanging_edge() {
    for (start, end, expected_start, expected_end) in [(4.0, 0.0, 0.0, 16.0), (0.0, 4.0, 16.0, 0.0)]
    {
        let mut style = japanese_style(TextSpacingTrim::SpaceAll);
        style.direction = shodo::geometry::Direction::Rtl;
        style.root.direction = shodo::geometry::Direction::Rtl;
        let inline = style.root.clone();
        let para = japanese_with(style, Limits::default(), |b| {
            b.open_inline(
                NodeId(2),
                &inline,
                InlineEdges {
                    padding: Sides {
                        inline_start: start,
                        inline_end: end,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .push_text(TextSource::Generated { node: NodeId(3) }, "「日本」")
            .close_inline();
        });
        let options = LineOptions {
            hanging_punctuation: HangingPunctuation {
                first: true,
                last: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let line = first_line(&para, 100.0, &options, &AtomicSizes::EMPTY);
        assert_eq!(line.text_range(), 0..12);
        assert_eq!(
            (line.hang_start(), line.hang_end()),
            (expected_start, expected_end)
        );
        assert_eq!(line.inline_size(), 52.0 + expected_start);
    }
}
