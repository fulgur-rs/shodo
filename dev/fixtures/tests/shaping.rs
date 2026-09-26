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
