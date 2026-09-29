fn ligature_font(substitutions: &[(&str, char)]) -> Vec<u8> {
    ligature_font_for(crate::test_support::fonts::LATIN, *b"latn", substitutions)
}

fn ligature_font_for(bytes: &[u8], script: [u8; 4], substitutions: &[(&str, char)]) -> Vec<u8> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let glyph = |c| {
        font.charmap()
            .map(c)
            .or_else(|| {
                (script == *b"kana" && c == 'ｶ')
                    .then(|| font.charmap().map('カ'))
                    .flatten()
            })
            .unwrap()
            .to_u32() as u16
    };
    let mut sets = std::collections::BTreeMap::<u16, Vec<Vec<u16>>>::new();
    for (text, target) in substitutions {
        let mut components = text.chars().map(glyph);
        let first = components.next().unwrap();
        let rest: Vec<_> = components.collect();
        let mut lig = vec![glyph(*target), rest.len() as u16 + 1];
        lig.extend(rest);
        sets.entry(first).or_default().push(lig);
    }
    let mut offsets = Vec::new();
    let mut data = Vec::new();
    let header = 3 + sets.len();
    for ligatures in sets.values() {
        offsets.push(((header + data.len()) * 2) as u16);
        let mut offset = 1 + ligatures.len();
        data.push(ligatures.len() as u16);
        for lig in ligatures {
            data.push((offset * 2) as u16);
            offset += lig.len();
        }
        for lig in ligatures {
            data.extend(lig);
        }
    }
    let mut subtable = vec![1, ((header + data.len()) * 2) as u16, sets.len() as u16];
    subtable.extend(offsets);
    subtable.extend(data);
    subtable.extend([1, sets.len() as u16]);
    subtable.extend(sets.keys());
    let mut gsub = Vec::new();
    for word in [1u16, 0, 10, 30, 44, 1] {
        gsub.extend(word.to_be_bytes());
    }
    gsub.extend(script);
    for word in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gsub.extend(word.to_be_bytes());
    }
    gsub.extend(b"liga");
    for word in [8u16, 0, 1, 0, 1, 4, 4, 0, 1, 8]
        .into_iter()
        .chain(subtable)
    {
        gsub.extend(word.to_be_bytes());
    }
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if tag == *b"cmap" && script == *b"kana" {
            // The subset has no halfwidth kana; give that source scalar
            // the existing fullwidth glyph before forcing their ligature.
            let mut chars: Vec<_> = substitutions
                .iter()
                .flat_map(|(s, c)| s.chars().chain(std::iter::once(*c)))
                .collect();
            chars.sort_unstable();
            chars.dedup();
            let mut cmap = Vec::new();
            for word in [0u16, 1, 3, 10] {
                cmap.extend(word.to_be_bytes());
            }
            cmap.extend(12u32.to_be_bytes());
            cmap.extend(12u16.to_be_bytes());
            cmap.extend(0u16.to_be_bytes());
            for word in [16 + 12 * chars.len() as u32, 0, chars.len() as u32] {
                cmap.extend(word.to_be_bytes());
            }
            for ch in chars {
                for word in [ch as u32, ch as u32, glyph(ch) as u32] {
                    cmap.extend(word.to_be_bytes());
                }
            }
            tables.push((tag, cmap));
        } else if tag != *b"GSUB" {
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
    }
    tables.push((*b"GSUB", gsub));
    tables.sort_by_key(|(tag, _)| *tag);
    let mut result = crate::font::sfnt::build_sfnt(&tables);
    result[..4].copy_from_slice(&bytes[..4]);
    result
}

#[test]
fn mixed_width_kana_whole_cluster_uses_each_source_box_for_autospace() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{FontFamily, OverflowWrap, ParagraphStyle, TextAutospace};
    use crate::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
    };
    let bytes = ligature_font_for(crate::test_support::fonts::CJK, *b"kana", &[("カｶ", '水')]);
    let font = skrifa::FontRef::from_index(&bytes, 0).unwrap();
    let metrics = font.glyph_metrics(Size::new(20.0), LocationRef::default());
    let water = font.charmap().map('水').unwrap();
    let natural = metrics.advance_width(water).unwrap();
    let hf = harfrust::FontRef::from_index(&bytes, 0).unwrap();
    let hd = harfrust::ShaperData::new(&hf);
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str("カｶ");
    buffer.guess_segment_properties();
    let direct = hd.shaper(&hf).build().shape(buffer, Default::default());
    assert_eq!(
        direct.len(),
        1,
        "the oracle must force a shared GSUB cluster"
    );
    assert_eq!(direct.glyph_infos()[0].glyph_id, water.to_u32());
    let limits = Limits {
        max_reshape_window_bytes: Some(0),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            bytes,
            0,
            FontFaceDescriptor {
                family: "Kana provenance".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Kana provenance".into())];
    style.root.font_size = 20.0;
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut child = style.root.clone();
    child.text_autospace = TextAutospace::NoAutospace;
    let mut builder = ParagraphBuilder::new(&style, &limits);
    for (node, text) in [(1, "カ"), (2, "ｶ")] {
        builder
            .open_inline(NodeId(node), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(node) }, text)
            .close_inline();
    }
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    for width in [0.0, 100.0] {
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text_range(), 0..6);
        assert!((lines[0].inline_size() - (natural + natural / 8.0)).abs() < 0.04);
        assert_eq!(
            lines[0]
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                    _ => None,
                })
                .sum::<usize>(),
            1
        );
        let child_width: f32 = lines[0]
            .fragments()
            .filter_map(|f| match f {
                Fragment::InlineBox(b) => Some(b.rect.inline_size),
                _ => None,
            })
            .sum();
        assert!(
            (child_width - natural).abs() < 0.04,
            "parent-owned spacing leaked into descendants: {child_width}"
        );
    }
}

fn expanded_hyphen_font(count: u16) -> Vec<u8> {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let dash = font.charmap().map('-').unwrap().to_u32() as u16;
    let w = font.charmap().map('W').unwrap().to_u32() as u16;
    let mut gsub = Vec::new();
    for word in [1u16, 0, 10, 30, 44, 1] {
        gsub.extend(word.to_be_bytes());
    }
    gsub.extend(b"latn");
    for word in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gsub.extend(word.to_be_bytes());
    }
    gsub.extend(b"ccmp");
    for word in [
        8u16,
        0,
        1,
        0,
        1,
        4,
        2,
        0,
        1,
        8,
        1,
        10 + 2 * count,
        1,
        8,
        count,
    ]
    .into_iter()
    .chain(std::iter::repeat_n(w, count as usize))
    .chain([1, 1, dash])
    {
        gsub.extend(word.to_be_bytes());
    }
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if tag != *b"GSUB" {
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
    }
    tables.push((*b"GSUB", gsub));
    tables.sort_by_key(|(tag, _)| *tag);
    crate::font::sfnt::build_sfnt(&tables)
}

#[test]
fn generated_hyphen_and_opposite_partial_edge_share_one_glyph_budget() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::{Limits, WarningKind};
    use crate::node::{NodeId, OutOfFlowKind, TextSource};
    use crate::style::{FontFamily, OverflowWrap, ParagraphStyle};
    use crate::{
        AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
    };
    let expanded = expanded_hyphen_font(9);
    let font = harfrust::FontRef::from_index(&expanded, 0).unwrap();
    let direct = harfrust::ShaperData::new(&font);
    let shaper = direct.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str("-");
    buffer.guess_segment_properties();
    assert_eq!(
        shaper.shape(buffer, Default::default()).len(),
        9,
        "real generated GSUB expansion"
    );
    for budget in [11, 12] {
        let limits = Limits {
            max_shaped_glyphs: Some(budget),
            ..Default::default()
        };
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
                    family: "Letters".into(),
                    unicode_ranges: vec![(32, 32), (65, 90), (97, 122), (173, 173)],
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Other".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let dash = fonts
            .register_face(
                expanded.clone(),
                0,
                FontFaceDescriptor {
                    family: "Dash".into(),
                    unicode_ranges: vec![(45, 45)],
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![
            FontFamily::Named("Letters".into()),
            FontFamily::Named("Dash".into()),
        ];
        style.root.overflow_wrap = OverflowWrap::Anywhere;
        let mut large = style.root.clone();
        large.font_size = 1000.0;
        let mut prefix = style.root.clone();
        prefix.font_families = vec![FontFamily::Named("Other".into())];
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.open_inline(NodeId(5), &prefix, Default::default())
            .push_text(TextSource::Generated { node: NodeId(1) }, "x ffi ")
            .close_inline()
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(1) }, "T\u{ad}")
            .open_inline(NodeId(3), &large, Default::default())
            .push_text(TextSource::Generated { node: NodeId(4) }, "ZZZZ")
            .close_inline();
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(p.data.glyphs.len(), 10);
        let prefixes = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1.0,
            &AtomicSizes::EMPTY,
        );
        let token = prefixes
            .iter()
            .find(|l| l.text_range().end == 3)
            .unwrap()
            .break_token();
        let mut warm = LayoutContext::new();
        let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
            &mut warm,
            token,
            &Default::default(),
            &LineConstraint::new(10000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let mut c = LineConstraint::new(200.0);
        c.floats_placed_through = Some(float_cursor);
        let LineResult::Line(line) = p.next_line(
            &mut warm,
            token,
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let LineResult::Line(cold) = p.next_line(
            &mut LayoutContext::new(),
            token,
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(line.text_range(), cold.text_range());
        assert_eq!(line.inline_size(), cold.inline_size());
        let runs: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect();
        if budget == 11 {
            assert_eq!(line.text_range(), 3..6);
            assert!(runs.iter().all(|r| r.font() != dash));
            assert!(
                warm.warnings
                    .as_slice()
                    .iter()
                    .any(|w| w.kind == WarningKind::Unsupported)
            );
        } else {
            assert_eq!(line.text_range(), 3..12);
            let generated = runs.iter().find(|r| r.font() == dash).unwrap();
            assert_eq!(generated.glyphs().len(), 9);
            assert_eq!(generated.clusters().next().unwrap().text_range, 10..12);
            let start = p.data.units.iter().position(|u| u.text.start == 3).unwrap();
            let end = p.data.units.iter().position(|u| u.text.end == 12).unwrap() + 1;
            let windows = crate::line::hyphen::line(
                &p.data,
                start,
                end,
                &mut LayoutContext::new(),
                &mut crate::geometry::Saturation::default(),
            )
            .unwrap();
            assert_eq!(windows.len(), 2, "both distinct font edges retained");
            assert_eq!(
                windows.iter().map(|w| w.overlay.store.len()).sum::<usize>(),
                12,
                "the safe prefix and generated tail share the limit"
            );
        }
        assert!(
            line.overlay
                .as_ref()
                .is_none_or(|g| g.len() as u64 <= budget)
        );
    }
}

#[test]
fn unshapeable_interior_slice_is_skipped_without_duplicate_glyphs() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, LineOptions, OverflowWrap, ParagraphStyle, TextWrapStyle};
    use crate::{
        AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
    };
    let bytes = ligature_font(&[
        ("ffii", 'f'),
        ("ffi", 'W'),
        ("ff", 'W'),
        ("fii", 'W'),
        ("ii", 'f'),
    ]);
    let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    for (text, expected) in [
        ("ffii", 1),
        ("f", 1),
        ("ff", 1),
        ("ffi", 1),
        ("fii", 1),
        ("ii", 1),
        ("i", 1),
        ("fi", 2),
    ] {
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        assert_eq!(
            shaper.shape(buffer, Default::default()).len(),
            expected,
            "direct oracle {text}"
        );
    }
    let limits = Limits {
        max_shaped_glyphs: Some(1),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            bytes.clone(),
            0,
            FontFaceDescriptor {
                family: "Budget".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Budget".into())];
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffii");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let wide = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    let width = wide[0].inline_size() + 0.01;
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        lines.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
        vec![0..1, 1..2, 2..4]
    );
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
        let mut constraint = LineConstraint::new(width);
        constraint.break_plan = Some(&plan);
        let mut token = p.start_token();
        for expected in [0..1, 1..2, 2..4] {
            let LineResult::Line(line) =
                p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
            else {
                panic!("planned line unavailable");
            };
            assert_eq!(line.text_range(), expected);
            token = line.break_token();
        }
        assert!(matches!(
            p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY),
            LineResult::Done
        ));
    }
    for l in lines {
        let range = l.text_range();
        let clusters: Vec<_> = l
            .fragments()
            .filter_map(|r| match r {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.clusters().map(|c| c.text_range).collect::<Vec<_>>())
            .collect();
        assert_eq!(clusters, vec![range]);
        assert_eq!(
            l.fragments()
                .filter_map(|r| match r {
                    Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                    _ => None,
                })
                .sum::<usize>(),
            1
        );
    }
}

#[test]
fn unshapeable_ligature_remainder_retains_whole_source() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, LineOptions, OverflowWrap, ParagraphStyle, TextWrapStyle};
    use crate::{
        AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
    };
    use skrifa::MetadataProvider;
    let bytes = ligature_font(&[("ffi", 'f'), ("ff", 'W')]);
    let font = skrifa::FontRef::from_index(&bytes, 0).unwrap();
    let f = font.charmap().map('f').unwrap().to_u32() as u16;
    let i = font.charmap().map('i').unwrap().to_u32() as u16;
    let w = font.charmap().map('W').unwrap().to_u32() as u16;
    // Every initial prefix fits one glyph; the remainder fi needs two.
    let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
    let direct = harfrust::ShaperData::new(&font);
    let shaper = direct.shaper(&font).build();
    for (text, ids) in [
        ("ffi", vec![f as u32]),
        ("ff", vec![w as u32]),
        ("fi", vec![f as u32, i as u32]),
    ] {
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        assert_eq!(
            shaper
                .shape(buffer, Default::default())
                .glyph_infos()
                .iter()
                .map(|g| g.glyph_id)
                .collect::<Vec<_>>(),
            ids
        );
    }
    let limits = Limits {
        max_shaped_glyphs: Some(1),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            bytes.clone(),
            0,
            FontFaceDescriptor {
                family: "Budget".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Budget".into())];
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let wide = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    let width = wide[0].inline_size() + 0.01;
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
        let mut constraint = LineConstraint::new(width);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned line unavailable");
        };
        assert_eq!(line.text_range(), 0..3);
        assert!(matches!(
            p.next_line(
                &mut cx,
                line.break_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY
            ),
            LineResult::Done
        ));
        let lines = p.break_all(&mut cx, &options, width, &AtomicSizes::EMPTY);
        assert_eq!(
            lines.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
            vec![0..3]
        );
        assert_eq!(
            lines
                .iter()
                .flat_map(|l| l.fragments())
                .filter_map(|r| match r {
                    Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                    _ => None,
                })
                .sum::<usize>(),
            1
        );
    }
}

#[test]
fn combined_square_survives_an_adjacent_owned_ligature_window() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::geometry::WritingMode;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{FontFamily, ParagraphStyle, TextAutospace, TextCombineUpright, WordBreak};
    use crate::{
        AtomicSizes, Fragment, GlyphOrientation, LayoutContext, LineConstraint, LineResult,
        ParagraphBuilder,
    };
    let limits = Default::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        ("Latin", crate::test_support::fonts::LATIN),
        ("Arabic", crate::test_support::fonts::ARABIC),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for (family, combined_text) in [("Arabic", "بب"), ("Latin", "ab")] {
            let mut root = ParagraphStyle {
                writing_mode: mode,
                ..Default::default()
            };
            root.root.font_families = vec![FontFamily::Named("Latin".into())];
            root.root.font_size = 16.0;
            root.root.text_autospace = TextAutospace::NoAutospace;
            root.root.word_break = WordBreak::BreakAll;
            let mut combined = root.root.clone();
            combined.font_families = vec![FontFamily::Named(family.into())];
            combined.text_combine_upright = TextCombineUpright::All;
            let make = |suffix| {
                let mut b = ParagraphBuilder::new(&root, &limits);
                b.open_inline(NodeId(1), &combined, InlineEdges::default());
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(2),
                        offset: 0,
                    },
                    combined_text,
                );
                b.close_inline();
                if suffix {
                    b.push_text(
                        TextSource::Dom {
                            node: NodeId(3),
                            offset: 0,
                        },
                        "ffi",
                    );
                }
                b.build(&mut LayoutContext::new(), &fonts).unwrap()
            };
            let shape = |line: &crate::Line| {
                line.fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(run)
                            if run.orientation() == GlyphOrientation::Combined =>
                        {
                            Some((
                                run.glyph_transform(),
                                run.glyphs()
                                    .map(|g| {
                                        (
                                            g.id,
                                            g.advance,
                                            g.inline_position - run.inline_start(),
                                            g.block_offset,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            ))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let alone = make(false).break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                100.0,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(
                alone[0].inline_size(),
                16.0,
                "an internal shaping edge cannot replace the square cost"
            );
            let p = make(true);
            let LineResult::Line(line) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &Default::default(),
                &LineConstraint::new(24.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("line")
            };
            assert!(
                line.overlay.is_some(),
                "real ffi source cut must exercise an owned window"
            );
            assert!(
                line.text_range().end > combined_text.len()
                    && line.text_range().end < combined_text.len() + 3,
                "must cut inside neighboring ffi: {:?}",
                line.text_range()
            );
            assert_eq!(line.text_combinations().count(), 1);
            assert_eq!(shape(&line), shape(&alone[0]));
        }
    }
}

#[test]
fn adjacent_unsafe_edges_with_different_fonts_keep_both_glyphs() {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, ParagraphStyle};
    use crate::{
        AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
    };
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let latin = fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let cjk = fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Cjk".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![
        FontFamily::Named("Latin".into()),
        FontFamily::Named("Cjk".into()),
    ];
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "z a日");
    let mut p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let LineResult::Line(first) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let token = first.break_token();
    drop(first);
    let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
    data.units
        .iter_mut()
        .find(|u| u.text == (2..3))
        .unwrap()
        .unsafe_to_concat = true;
    data.units
        .iter_mut()
        .find(|u| u.text == (3..6))
        .unwrap()
        .unsafe_to_break = true;
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        token,
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    let runs: Vec<_> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.iter().map(|r| r.glyphs().len()).sum::<usize>(), 2);
    assert_eq!(
        runs.iter().map(|r| r.font()).collect::<Vec<_>>(),
        vec![latin, cjk]
    );
    assert_eq!(
        runs.iter().map(|r| r.text_range()).collect::<Vec<_>>(),
        vec![2..3, 3..6]
    );
}
