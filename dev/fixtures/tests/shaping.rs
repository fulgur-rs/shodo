use shodo::Fragment;
use shodo::font::FontCollection;
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFeature, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};
use skrifa::{FontRef, MetadataProvider};

fn build(
    text: &str,
    style: InlineStyle,
    direction: Direction,
    fonts: &FontCollection,
    split: Option<usize>,
) -> Paragraph {
    let paragraph = ParagraphStyle {
        direction,
        root: style.clone(),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&paragraph, &Limits::default());
    let source = TextSource::Dom {
        node: NodeId(1),
        offset: 0,
    };
    if let Some(at) = split {
        b.push_text(source, &text[..at])
            .open_inline(NodeId(2), &style, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(3),
                    offset: 0,
                },
                &text[at..],
            )
            .close_inline();
    } else {
        b.push_text(source, text);
    }
    b.build(&mut LayoutContext::new(), fonts).unwrap()
}

fn glyphs(p: &Paragraph) -> Vec<shodo::Glyph> {
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(10000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .flat_map(|r| r.glyphs())
        .collect()
}

fn direct(
    text: &str,
    face: usize,
    rtl: bool,
    features: &[harfrust::Feature],
) -> Vec<(u32, u32, i32, i32, i32)> {
    let font = harfrust::FontRef::from_index(FONTS[face].bytes, 0).unwrap();
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    let mut b = harfrust::UnicodeBuffer::new();
    b.push_str(text);
    b.set_direction(if rtl {
        harfrust::Direction::RightToLeft
    } else {
        harfrust::Direction::LeftToRight
    });
    b.guess_segment_properties();
    let g = shaper.shape(b, harfrust::ShapeOptions::default().features(features));
    g.glyph_infos()
        .iter()
        .zip(g.glyph_positions())
        .map(|(i, p)| (i.glyph_id, i.cluster, p.x_advance, p.x_offset, p.y_offset))
        .collect()
}

#[test]
fn paragraph_matches_direct_arabic_shaper() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let text = "السَّلَامُ";
    let style = InlineStyle {
        lang: Some("ar".into()),
        ..Default::default()
    };
    let p = build(text, style, Direction::Rtl, &fonts.collection, None);
    let mut actual = glyphs(&p)
        .iter()
        .map(|g| (g.id, g.cluster))
        .collect::<Vec<_>>();
    let mut expected = direct(text, 2, true, &[])
        .iter()
        .map(|g| (g.0, g.1))
        .collect::<Vec<_>>();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn latin_ligatures_features_and_kerning() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let a = build(
        "office AV",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    assert_eq!(
        glyphs(&a).iter().map(|g| g.id).collect::<Vec<_>>(),
        direct("office AV", 0, false, &[])
            .iter()
            .map(|g| g.0)
            .collect::<Vec<_>>()
    );
    let style = InlineStyle {
        font_features: vec![FontFeature {
            tag: *b"liga",
            value: 0,
        }],
        ..Default::default()
    };
    let b = build("office AV", style, Direction::Ltr, &fonts.collection, None);
    assert!(glyphs(&b).len() > glyphs(&a).len());
    let nominal = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    assert!(
        glyphs(&a)
            .iter()
            .any(|g| g.id != nominal.charmap().map('f').unwrap().to_u32() && g.cluster == 1)
    );
}

#[test]
fn font_fallback_per_grapheme() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = build(
        "a日本",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let ids = line
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r.font())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![fonts.ids[0], fonts.ids[1]]);
    assert!(glyphs(&p).iter().all(|g| g.id != 0));
}

#[test]
fn node_split_preserves_arabic_joining() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let text = "السلام";
    let p = build(
        text,
        InlineStyle::default(),
        Direction::Rtl,
        &fonts.collection,
        Some(4),
    );
    let mut expected = direct(text, 2, true, &[])
        .iter()
        .map(|g| (g.0, g.1))
        .collect::<Vec<_>>();
    let mut actual = glyphs(&p)
        .iter()
        .map(|g| (g.id, g.cluster))
        .collect::<Vec<_>>();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn cross_node_ligature_single_owner_and_mapping() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = build(
        "ffi",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        Some(1),
    );
    let g = glyphs(&p);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].id, direct("ffi", 0, false, &[])[0].0);
    assert_eq!(g[0].cluster, 0);
    let mapping = p.offset_mapping().unwrap();
    assert_eq!(mapping.dom_to_text(NodeId(3), 1).map(|p| p.0), Some(2));
}

#[test]
fn missing_font_notdef_warning() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = build(
        "\u{10ffff}",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    assert_eq!(glyphs(&p)[0].id, 0);
    assert!(
        p.warnings().iter().any(
            |w| w.kind == shodo::limits::WarningKind::Unsupported && w.message.contains("font")
        )
    );
}

#[test]
fn rtl_mark_positions_and_visual_order() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let text = "السَّلَامُ";
    let reference = direct(text, 2, true, &[]);
    let units = FontRef::from_index(FONTS[2].bytes, 0)
        .unwrap()
        .metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .units_per_em as f32;
    let scale = 16.0 / units;
    let total = reference.iter().map(|g| g.2 as f32 * scale).sum::<f32>();
    for direction in [Direction::Ltr, Direction::Rtl] {
        let p = build(
            text,
            InlineStyle::default(),
            direction,
            &fonts.collection,
            None,
        );
        let mut actual = glyphs(&p)
            .iter()
            .map(|g| (g.id, g.cluster, g.inline_position, g.block_offset))
            .collect::<Vec<_>>();
        let mut pen = 0.0;
        let mut expected = reference
            .iter()
            .map(|g| {
                let x = pen + g.3 as f32 * scale;
                let advance = g.2 as f32 * scale;
                pen += advance;
                (
                    g.0,
                    g.1,
                    if direction == Direction::Rtl {
                        total - x - advance
                    } else {
                        x
                    },
                    -g.4 as f32 * scale,
                )
            })
            .collect::<Vec<_>>();
        actual.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(a.2.total_cmp(&b.2)));
        expected.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(a.2.total_cmp(&b.2)));
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(&expected) {
            assert_eq!((a.0, a.1), (e.0, e.1));
            assert!(
                (a.2 - e.2).abs() < 0.1,
                "{direction:?} glyph{} cluster{} x{} expected{}",
                a.0,
                a.1,
                a.2,
                e.2
            );
            assert!((a.3 - e.3).abs() < 0.03);
        }
    }
}

#[test]
fn forced_boundary_stops_arabic_joining() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&Default::default(), &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "س")
        .push_forced_break(NodeId(2))
        .push_text(TextSource::Generated { node: NodeId(3) }, "س");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let all = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    let want = direct("س", 2, true, &[])[0].0;
    assert_eq!(all.len(), 2);
    for line in all {
        let ids = line
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![want]);
    }
}

#[test]
fn run_byte_limits_and_giant_grapheme() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (text, budget) in [
        ("abcdefghijklmnop".to_owned(), 4),
        (format!("a{}", "\u{301}".repeat(16)), 2),
    ] {
        let limits = Limits {
            max_shaping_run_bytes: Some(budget),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&Default::default(), &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(10000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let runs = line
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert!(runs.iter().all(|r| r.text_range().len() <= budget as usize));
        if budget == 2 {
            assert!(p.warnings().iter().any(|w| w.message.contains("grapheme")));
        }
        assert!(!glyphs(&p).is_empty());
    }
}

#[test]
fn shaped_output_limit_before_retention() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let limits = Limits {
        max_shaped_glyphs: Some(2),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&Default::default(), &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "abcdef");
    assert_eq!(
        b.build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap_err()
            .kind,
        shodo::limits::LimitKind::ShapedGlyphs
    );
}

#[test]
fn variant_ligatures_and_author_precedence_reach_shaper() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let none = InlineStyle {
        font_variant_ligatures: shodo::style::FontVariantLigatures {
            none: true,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        glyphs(&build(
            "ffi",
            none.clone(),
            Direction::Ltr,
            &fonts.collection,
            None
        ))
        .len(),
        3
    );
    let author = InlineStyle {
        font_features: vec![FontFeature {
            tag: *b"liga",
            value: 1,
        }],
        ..none
    };
    assert_eq!(
        glyphs(&build(
            "ffi",
            author,
            Direction::Ltr,
            &fonts.collection,
            None
        ))
        .len(),
        1
    );
    let normal = build(
        "AV",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    let none = build(
        "AV",
        InlineStyle {
            font_kerning: shodo::style::FontKerning::None,
            ..Default::default()
        },
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    assert!(
        glyphs(&none).iter().map(|g| g.advance).sum::<f32>()
            > glyphs(&normal).iter().map(|g| g.advance).sum::<f32>()
    );
}

#[test]
fn synthesis_size_adjust_and_run_instance_are_public() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = InlineStyle {
        font_weight: 700.0,
        font_style: shodo::style::FontStyle::Italic,
        font_size_adjust: Some(shodo::style::FontSizeAdjust {
            metric: shodo::style::FontMetricKind::ExHeight,
            value: 0.5,
        }),
        ..Default::default()
    };
    let p = build("ab", style, Direction::Ltr, &fonts.collection, None);
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let run = line
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    assert!(run.embolden());
    assert!(run.skew().is_some());
    assert!(run.normalized_coords().is_empty());
    let font = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let m = font.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let expected = 16.0 * 0.5 / (m.x_height.unwrap() / m.units_per_em as f32);
    assert!((run.font_size() - expected).abs() < 0.02);
    assert!(run.font_data().is_some());
}

#[test]
fn combining_mark_style_is_not_replaced_by_base_style() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle::default();
    let mark = InlineStyle {
        font_size: 32.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    )
    .open_inline(NodeId(2), &mark, InlineEdges::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 0,
        },
        "\u{301}",
    )
    .close_inline();
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let mark = line.fragments().find_map(|f| {
        if let Fragment::GlyphRun(r) = f {
            if r.node() == Some(NodeId(3)) {
                Some(r)
            } else {
                None
            }
        } else {
            None
        }
    });
    let mark = mark.unwrap();
    assert_eq!(mark.font_size(), 32.0);
    assert!(mark.glyphs().all(|g| g.id != 0));
    assert_eq!(p.text(), "a\u{301}");
}

#[test]
fn script_extensions_and_paired_brackets_preserve_context() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (text, probes) in [
        ("aーカ", vec![(0, *b"Latn"), (1, *b"Kana"), (4, *b"Kana")]),
        ("a(سلام)b", vec![(1, *b"Latn"), (10, *b"Latn")]),
        ("(سلام)", vec![(0, *b"Arab"), (9, *b"Arab")]),
        ("a\u{301}", vec![(0, *b"Latn")]),
    ] {
        let p = build(
            text,
            InlineStyle::default(),
            Direction::Ltr,
            &fonts.collection,
            None,
        );
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        for (offset, expected) in probes {
            let actual = line
                .fragments()
                .find_map(|f| {
                    if let Fragment::GlyphRun(r) = f {
                        if r.text_range().contains(&offset) {
                            Some(r.script())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(actual, expected, "{text:?} at {offset}");
        }
    }
}

#[test]
fn invalid_language_without_transform_warns_and_uses_root() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = build(
        "office",
        InlineStyle {
            lang: Some("%%%bad%%%".into()),
            ..Default::default()
        },
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    assert!(p.warnings().iter().any(|w| w.message.contains("lang")));
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert!(
        line.fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r.language())
                } else {
                    None
                }
            })
            .all(|language| language.is_none())
    );
}

#[test]
fn unknown_language_warns_but_registered_extensions_are_preserved() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (lang, unknown) in [
        ("zz", true),
        ("en-US-u-ca-gregory", false),
        ("qaa", false),
        ("zza", false),
    ] {
        let p = build(
            "office",
            InlineStyle {
                lang: Some(lang.into()),
                ..Default::default()
            },
            Direction::Ltr,
            &fonts.collection,
            None,
        );
        assert_eq!(
            p.warnings().iter().any(|w| w.message.contains("lang")),
            unknown,
            "{lang}"
        );
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert!(
            line.fragments()
                .filter_map(|f| {
                    if let Fragment::GlyphRun(r) = f {
                        Some(r.language())
                    } else {
                        None
                    }
                })
                .all(|language| language == if unknown { None } else { Some(lang) })
        );
    }
}

#[test]
fn font_layer_survives_owner_drop() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let weak = fonts.collection.layer_handle();
    let p = build(
        "office",
        InlineStyle::default(),
        Direction::Ltr,
        &fonts.collection,
        None,
    );
    let mut cx = LayoutContext::new();
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    drop(p);
    drop(fonts);
    cx.shrink_to(0);
    assert!(weak.is_alive());
    assert!(
        line.fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .all(|r| r.font_data().is_some() && r.glyphs().all(|g| g.id != 0))
    );
    drop(line);
    drop(cx);
    assert!(!weak.is_alive());
}

#[test]
fn every_size_adjust_metric_reaches_public_font_size() {
    use shodo::style::{FontFamily, FontMetricKind, FontSizeAdjust};
    use skrifa::{
        instance::{LocationRef, Size},
        raw::TableProvider,
    };
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (metric, face, c) in [
        (FontMetricKind::ExHeight, 0, 'a'),
        (FontMetricKind::CapHeight, 0, 'A'),
        (FontMetricKind::ChWidth, 0, '0'),
        (FontMetricKind::IcWidth, 1, '水'),
        (FontMetricKind::IcHeight, 1, '水'),
    ] {
        let font = FontRef::from_index(FONTS[face].bytes, 0).unwrap();
        let m = font.metrics(Size::unscaled(), LocationRef::default());
        let expected_metric = match metric {
            FontMetricKind::ExHeight => m.x_height.unwrap(),
            FontMetricKind::CapHeight => m.cap_height.unwrap(),
            FontMetricKind::ChWidth | FontMetricKind::IcWidth => font
                .glyph_metrics(Size::unscaled(), LocationRef::default())
                .advance_width(font.charmap().map(c).unwrap())
                .unwrap(),
            FontMetricKind::IcHeight => font
                .vmtx()
                .unwrap()
                .advance(font.charmap().map(c).unwrap())
                .unwrap() as f32,
        };
        let style = InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[face].family.into())],
            font_size_adjust: Some(FontSizeAdjust { metric, value: 0.6 }),
            ..Default::default()
        };
        let p = build(
            &c.to_string(),
            style,
            Direction::Ltr,
            &fonts.collection,
            None,
        );
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let run = line
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        let expected_size = 16.0 * 0.6 * m.units_per_em as f32 / expected_metric;
        assert!((run.font_size() - expected_size).abs() < 0.02, "{metric:?}");
    }
}

#[test]
fn language_feature_precedence_matches_direct_shaper() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for language in ["tr", "en-US-u-ca-gregory"] {
        let style = InlineStyle {
            lang: Some(language.into()),
            font_features: vec![FontFeature {
                tag: *b"liga",
                value: 0,
            }],
            ..Default::default()
        };
        let p = build("fi", style, Direction::Ltr, &fonts.collection, None);
        let font = harfrust::FontRef::from_index(FONTS[0].bytes, 0).unwrap();
        let data = harfrust::ShaperData::new(&font);
        let shaper = data.shaper(&font).build();
        let mut b = harfrust::UnicodeBuffer::new();
        b.push_str("fi");
        b.set_language(language.parse().unwrap());
        b.guess_segment_properties();
        let features = [harfrust::Feature::new(harfrust::Tag::new(b"liga"), 0, ..)];
        let shaped = shaper.shape(b, harfrust::ShapeOptions::default().features(&features));
        assert_eq!(
            glyphs(&p).iter().map(|g| g.id).collect::<Vec<_>>(),
            shaped
                .glyph_infos()
                .iter()
                .map(|i| i.glyph_id)
                .collect::<Vec<_>>()
        );
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert!(
            line.fragments()
                .filter_map(|f| if let Fragment::GlyphRun(r) = f {
                    Some(r.language())
                } else {
                    None
                })
                .all(|l| l == Some(language))
        );
    }
}
