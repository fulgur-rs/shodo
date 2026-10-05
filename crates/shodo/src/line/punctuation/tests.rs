use super::PunctuationClass as P;
use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{
    FontFamily, FontMetricKind, FontSizeAdjust, FontVariation, InlineStyle, ParagraphStyle,
    TextSpacingTrim,
};
use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
use skrifa::{FontRef, MetadataProvider, raw::TableProvider};

fn cjk_tables() -> Vec<([u8; 4], Vec<u8>)> {
    let base = crate::test_support::fonts::CJK;
    (0..u16::from_be_bytes(base[4..6].try_into().unwrap()) as usize)
        .map(|n| {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(base[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(base[at + 12..at + 16].try_into().unwrap()) as usize;
            (
                base[at..at + 4].try_into().unwrap(),
                base[offset..offset + len].to_vec(),
            )
        })
        .collect()
}

fn cjk_font(tables: &mut [([u8; 4], Vec<u8>)]) -> Vec<u8> {
    tables.sort_by_key(|t| t.0);
    let mut bytes = crate::font::sfnt::build_sfnt(tables);
    bytes[..4].copy_from_slice(b"OTTO");
    bytes
}

#[test]
fn justification_filters_boundaries_inside_an_actual_punctuation_ligature() {
    use crate::Fragment;
    use crate::style::{TextAlign, TextJustify};
    let font = FontRef::new(crate::test_support::fonts::CJK).unwrap();
    let ids: Vec<_> = ['「', '日', '」']
        .iter()
        .map(|c| font.charmap().map(*c).unwrap().to_u32() as u16)
        .collect();
    let mut gsub = Vec::new();
    let words = |bytes: &mut Vec<u8>, values: &[u16]| {
        for value in values {
            bytes.extend(value.to_be_bytes());
        }
    };
    // GSUB1.0: DFLT required rlig, one type4/format1 lookup mapping
    // 「日」 to the retained 日 outline. Each input keeps its source cut.
    words(&mut gsub, &[1, 0, 10, 30, 44, 1]);
    gsub.extend(b"DFLT");
    words(&mut gsub, &[8, 4, 0, 0, 0, 0, 0, 1]);
    gsub.extend(b"rlig");
    words(&mut gsub, &[8, 0, 1, 0, 1, 4, 4, 0, 1, 8]);
    words(
        &mut gsub,
        &[1, 8, 1, 14, 1, 1, ids[0], 1, 4, ids[1], 3, ids[1], ids[2]],
    );
    let mut tables = cjk_tables();
    tables.retain(|(tag, _)| tag != b"GSUB");
    tables.push((*b"GSUB", gsub));
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            cjk_font(&mut tables),
            0,
            FontFaceDescriptor {
                family: "Ligature".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Ligature".into())],
            font_size: 16.,
            lang: Some("ja".into()),
            text_spacing_trim: TextSpacingTrim::SpaceAll,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "「日」");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert_eq!(
        p.data.glyphs.id,
        [ids[1] as u32],
        "fixture must actually ligate"
    );
    let options = crate::style::LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::InterCharacter,
        ..Default::default()
    };
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &options,
        &LineConstraint::new(48.),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("line")
    };
    assert_eq!(line.text_range(), 0..9);
    assert_eq!(line.inline_size(), 16.);
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
    let glyphs: Vec<_> = run.glyphs().collect();
    assert_eq!(glyphs.len(), 1);
    assert_eq!(glyphs[0].inline_position, 16.);
}

#[test]
fn fallback_variation_and_size_adjust_use_the_actual_blank() {
    let mut tables = cjk_tables();
    let count = FontRef::new(crate::test_support::fonts::CJK)
        .unwrap()
        .maxp()
        .unwrap()
        .num_glyphs();
    let mut fvar = Vec::new();
    for value in [1u16, 0, 16, 2, 1, 20, 0, 8] {
        fvar.extend(value.to_be_bytes());
    }
    fvar.extend(b"wght");
    for value in [100i32, 400, 900] {
        fvar.extend((value << 16).to_be_bytes());
    }
    fvar.extend([0, 0, 1, 0]);
    // A real HVAR table adds 600 font units to every advance at wght=900.
    // The outlines retain their original bounds, so halfwidth trimming
    // becomes unsafe after the ic-width adjustment keeps advances at 32px.
    let mut hvar = Vec::new();
    for value in [1u16, 0] {
        hvar.extend(value.to_be_bytes());
    }
    for value in [20u32, 0, 0, 0] {
        hvar.extend(value.to_be_bytes());
    }
    hvar.extend(1u16.to_be_bytes());
    hvar.extend(12u32.to_be_bytes());
    hvar.extend(1u16.to_be_bytes());
    hvar.extend(22u32.to_be_bytes());
    for value in [1u16, 1, 0, 16384, 16384, count, 1, 1, 0] {
        hvar.extend(value.to_be_bytes());
    }
    for _ in 0..count {
        hvar.extend(600i16.to_be_bytes());
    }
    tables.push((*b"fvar", fvar));
    tables.push((*b"HVAR", hvar));
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
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
        .register_face(
            cjk_font(&mut tables),
            0,
            FontFaceDescriptor {
                family: "Variable".into(),
                weight: (100., 900.),
                ..Default::default()
            },
        )
        .unwrap();
    for (weight, expected, blank) in [(400., 80., 16.), (900., 96., 0.)] {
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![
                    FontFamily::Named("Latin".into()),
                    FontFamily::Named("Variable".into()),
                ],
                font_size: 16.,
                lang: Some("ja".into()),
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: weight,
                }],
                font_size_adjust: Some(FontSizeAdjust {
                    metric: FontMetricKind::IcWidth,
                    value: 2.,
                }),
                text_spacing_trim: TextSpacingTrim::Normal,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "「「日");
        let para = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(
            para.data.punctuation[0].advance.to_f32(),
            32.,
            "weight {weight}"
        );
        assert_eq!(
            para.data.punctuation[0].left.to_f32(),
            blank,
            "weight {weight}"
        );
        // The default location is represented by empty normalized coords.
        if weight == 900. {
            assert!(para.data.runs.iter().all(|r| r.instance.coords.len() == 1));
        }
        let LineResult::Line(line) = para.next_line(
            &mut LayoutContext::new(),
            para.start_token(),
            &Default::default(),
            &LineConstraint::new(200.),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        assert_eq!(line.inline_size(), expected, "weight {weight}");
        assert_eq!(line.text_range(), 0..9);
    }
}

#[test]
fn proportional_punctuation_keeps_its_advance_and_ink() {
    let font = FontRef::new(crate::test_support::fonts::CJK).unwrap();
    let opening = font.charmap().map('「').unwrap().to_u32() as usize;
    let mut tables = cjk_tables();
    let hmtx = &mut tables.iter_mut().find(|t| t.0 == *b"hmtx").unwrap().1;
    hmtx[opening * 4..opening * 4 + 2].copy_from_slice(&800u16.to_be_bytes());
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            cjk_font(&mut tables),
            0,
            FontFaceDescriptor {
                family: "Proportional".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Proportional".into())],
            font_size: 20.,
            lang: Some("ja".into()),
            text_spacing_trim: TextSpacingTrim::TrimAll,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "「「日");
    let para = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert_eq!(para.data.punctuation[0].advance.to_f32(), 16.);
    assert_eq!(para.data.punctuation[0].left.to_f32(), 0.);
    let LineResult::Line(line) = para.next_line(
        &mut LayoutContext::new(),
        para.start_token(),
        &Default::default(),
        &LineConstraint::new(200.),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("line")
    };
    assert_eq!(line.inline_size(), 52.);
}

#[test]
fn chinese_punctuation_respects_script_and_region_subtags() {
    for lang in [
        "zh-Hant",
        "zh-Hant-HK",
        "ZH-hant-tw",
        "zh-TW",
        "zh-HK",
        "zh-MO",
        "zh-TW-x-test",
    ] {
        for ch in ['、', '。', '，', '．', '：', '；'] {
            assert_eq!(classify(ch, Some(lang)), P::Middle, "{lang}: {ch}");
        }
    }
    for lang in [
        "zh",
        "zh-CN",
        "zh-Hans-CN",
        "zh-Hans-TW",
        "zh-SG",
        "zh-Hans-x-test",
    ] {
        for ch in ['、', '。', '，', '．', '：', '；'] {
            assert_eq!(classify(ch, Some(lang)), P::Closing, "{lang}: {ch}");
        }
    }
    for lang in [
        None,
        Some("ja-JP"),
        Some("en-Hant"),
        Some("zhx"),
        Some("zh-u-rg-twzzzz"),
    ] {
        assert_eq!(classify('、', lang), P::Closing, "{lang:?}");
    }
    for lang in [None, Some("ja-JP"), Some("en-Hant"), Some("zhx")] {
        assert_eq!(classify('：', lang), P::Middle, "{lang:?}");
    }
}

fn add_cmap_format12_mappings(tables: &mut [([u8; 4], Vec<u8>)], mappings: &[(u32, u16)]) {
    let cmap = &mut tables
        .iter_mut()
        .find(|table| table.0 == *b"cmap")
        .unwrap()
        .1;
    let records = u16::from_be_bytes([cmap[2], cmap[3]]) as usize;
    let offset = (0..records)
        .find_map(|index| {
            let at = 4 + index * 8;
            let subtable = u32::from_be_bytes(cmap[at + 4..at + 8].try_into().unwrap()) as usize;
            (u16::from_be_bytes(cmap[subtable..subtable + 2].try_into().unwrap()) == 12)
                .then_some(subtable)
        })
        .unwrap();
    let length = u32::from_be_bytes(cmap[offset + 4..offset + 8].try_into().unwrap()) as usize;
    assert_eq!(offset + length, cmap.len());
    let count = u32::from_be_bytes(cmap[offset + 12..offset + 16].try_into().unwrap()) as usize;
    let mut groups: Vec<_> = (0..count)
        .map(|index| {
            let at = offset + 16 + index * 12;
            (
                u32::from_be_bytes(cmap[at..at + 4].try_into().unwrap()),
                u32::from_be_bytes(cmap[at + 4..at + 8].try_into().unwrap()),
                u32::from_be_bytes(cmap[at + 8..at + 12].try_into().unwrap()),
            )
        })
        .collect();
    groups.extend(
        mappings
            .iter()
            .map(|(codepoint, glyph)| (*codepoint, *codepoint, u32::from(*glyph))),
    );
    groups.sort_by_key(|group| group.0);
    assert!(groups.windows(2).all(|pair| pair[0].1 < pair[1].0));

    let mut updated = cmap[..offset + 16].to_vec();
    for (start, end, glyph) in &groups {
        updated.extend(start.to_be_bytes());
        updated.extend(end.to_be_bytes());
        updated.extend(glyph.to_be_bytes());
    }
    let updated_length = (updated.len() - offset) as u32;
    updated[offset + 4..offset + 8].copy_from_slice(&updated_length.to_be_bytes());
    updated[offset + 12..offset + 16].copy_from_slice(&(groups.len() as u32).to_be_bytes());
    *cmap = updated;
}

fn quote_font(proportional: bool) -> Vec<u8> {
    let base = FontRef::new(crate::test_support::fonts::CJK).unwrap();
    let charmap = base.charmap();
    let quote_glyph = charmap.map('、').unwrap().to_u32() as u16;
    let fullwidth_glyph = charmap.map('水').unwrap().to_u32() as u16;
    let mut tables = cjk_tables();
    if proportional {
        let hmtx = &mut tables
            .iter_mut()
            .find(|table| table.0 == *b"hmtx")
            .unwrap()
            .1;
        hmtx[usize::from(quote_glyph) * 4..usize::from(quote_glyph) * 4 + 2]
            .copy_from_slice(&500u16.to_be_bytes());
    }
    let quote_mapping = if proportional {
        quote_glyph
    } else {
        fullwidth_glyph
    };
    let mut mappings = vec![
        ('\u{2019}' as u32, quote_mapping),
        ('\u{201d}' as u32, quote_mapping),
    ];
    mappings.extend([
        ('卜' as u32, fullwidth_glyph),
        ('一' as u32, fullwidth_glyph),
    ]);
    add_cmap_format12_mappings(&mut tables, &mappings);
    cjk_font(&mut tables)
}

fn quote_paragraph(
    text: &str,
    writing_mode: crate::geometry::WritingMode,
    proportional: bool,
) -> crate::Paragraph {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            quote_font(proportional),
            0,
            FontFaceDescriptor {
                family: "QuoteMetrics".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        writing_mode,
        root: InlineStyle {
            font_families: vec![FontFamily::Named("QuoteMetrics".into())],
            font_size: 20.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn intrinsic_proportional_quotes_only_change_horizontal_pairing() {
    use crate::geometry::{LayoutUnit, WritingMode};

    for quote_char in ['’', '”'] {
        let text = format!("日）{quote_char}（日");
        let paragraph = quote_paragraph(&text, WritingMode::HorizontalTb, true);
        let quote_offset = text.find(quote_char).unwrap() as u32;
        let before_offset = text.find('）').unwrap() as u32;
        let after_offset = text.find('（').unwrap() as u32;
        let quote = *paragraph
            .data
            .punctuation
            .iter()
            .find(|p| p.source == quote_offset)
            .unwrap();
        assert_eq!(quote.class, P::Pe, "{quote_char}");
        assert_eq!(quote.advance.to_f32(), 10.0, "{quote_char}");

        let mut before = *paragraph
            .data
            .punctuation
            .iter()
            .find(|p| p.source == before_offset)
            .unwrap();
        let mut after = *paragraph
            .data
            .punctuation
            .iter()
            .find(|p| p.source == after_offset)
            .unwrap();
        before.right = LayoutUnit::from_raw(5 * 64);
        after.left = LayoutUnit::from_raw(5 * 64);
        assert_eq!(pair(before, quote), (before.right, LayoutUnit::ZERO));
        assert_eq!(pair(quote, after), (LayoutUnit::ZERO, LayoutUnit::ZERO));
    }
}

fn paragraph_without_fullwidth_metric(text: &str) -> crate::Paragraph {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "LatinQuotes".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("LatinQuotes".into())],
            font_size: 20.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn proportional_quotes_keep_closing_class_for_vertical_or_unknown_metrics() {
    use crate::geometry::WritingMode;

    let vertical = quote_paragraph("日）’（日", WritingMode::VerticalRl, true);
    let quote = vertical
        .data
        .punctuation
        .iter()
        .find(|p| p.source == "日）".len() as u32)
        .unwrap();
    assert_eq!(quote.class, P::Closing);

    let unknown = paragraph_without_fullwidth_metric("’");
    assert_eq!(unknown.data.punctuation[0].class, P::Closing);
}

#[test]
fn fullwidth_quote_keeps_closing_class() {
    use crate::geometry::WritingMode;

    let paragraph = quote_paragraph("日）’（日", WritingMode::HorizontalTb, false);
    let quote = paragraph
        .data
        .punctuation
        .iter()
        .find(|p| p.source == "日）".len() as u32)
        .unwrap();
    assert_eq!(quote.class, P::Closing);
    assert_eq!(quote.advance.to_f32(), 20.0);
}

#[test]
fn proportional_quote_with_multiple_glyphs_keeps_closing_class() {
    use crate::geometry::WritingMode;

    let text = "’\u{0301}";
    let paragraph = quote_paragraph(text, WritingMode::HorizontalTb, true);
    let quote_offset = text.find('’').unwrap() as u32;
    let cluster_glyphs = paragraph
        .data
        .glyphs
        .cluster
        .iter()
        .filter(|offset| **offset == quote_offset)
        .count();
    assert_eq!(cluster_glyphs, 2);

    let quote = paragraph
        .data
        .punctuation
        .iter()
        .find(|p| p.source == quote_offset)
        .unwrap();
    assert_eq!(quote.class, P::Closing);
}
