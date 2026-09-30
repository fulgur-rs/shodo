use super::*;
use crate::font::FontCollection;
use crate::geometry::LayoutUnit;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;

#[test]
fn missing_vorg_uses_vmtx_top_bearing_for_vertical_origin() {
    // CJK 水 has yMax=838 in this pinned outline and vmtx TSB=42.
    // Remove VORG and change only its TSB to 142: origin becomes 980.
    let original = crate::test_support::fonts::CJK;
    let face = skrifa::FontRef::from_index(original, 0).unwrap();
    let gid = face.charmap().map('水').unwrap().to_u32() as usize;
    let mut tables = Vec::new();
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    for n in 0..count {
        let at = 12 + n * 16;
        let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
        if &tag == b"VORG" {
            continue;
        }
        let start = u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
        let mut data = original[start..start + len].to_vec();
        if &tag == b"vmtx" {
            data[gid * 4 + 2..gid * 4 + 4].copy_from_slice(&142i16.to_be_bytes());
        }
        tables.push((tag, data));
    }
    let bytes = crate::font::sfnt::build_sfnt(&tables);
    let derived = skrifa::FontRef::from_index(&bytes, 0).unwrap();
    assert!(derived.vorg().is_err());
    assert_eq!(
        derived
            .vmtx()
            .unwrap()
            .side_bearing(skrifa::GlyphId::new(gid as u32)),
        Some(142)
    );
    let bounds = derived
        .glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .bounds(skrifa::GlyphId::new(gid as u32))
        .unwrap();
    assert_eq!(bounds.y_max, 838.0);
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let registered = fonts
        .register_face(
            bytes,
            0,
            crate::font::FontFaceDescriptor {
                family: "No VORG".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: crate::geometry::WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_size: 16.0,
            font_families: vec![crate::style::FontFamily::Named("No VORG".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "水",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.runs[0].font, registered);
    assert_eq!(paragraph.data.glyphs.id[0], gid as u32);
    assert_eq!(paragraph.data.glyphs.advance[0].to_f32(), 16.0);
    assert_eq!(paragraph.data.glyphs.offset_inline[0].to_f32(), 15.6875);
    assert_eq!(paragraph.data.glyphs.offset_block[0].to_f32(), 8.0);
}

fn shape(
    text: &str,
    size: f32,
    limits: &Limits,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    let font = FontCollection::new(&Limits::default()).primary_font();
    let mut store = GlyphStore::default();
    let mut runs = Vec::new();
    let mut sat = Saturation::default();
    shape_item(
        &mut store, &mut runs, text, 0, 0, font, size, limits, &mut sat,
    )?;
    Ok((store, runs))
}

#[test]
fn one_em_per_character() {
    let (g, runs) = shape("abc", 10.0, &Limits::default()).unwrap();
    assert_eq!(g.len(), 3);
    let px: Vec<f32> = g.pen.iter().map(|p| p.to_f32()).collect();
    assert_eq!(px, vec![0.0, 10.0, 20.0]);
    assert!(g.advance.iter().all(|a| a.to_f32() == 10.0));
    assert_eq!(g.cluster, vec![0, 1, 2]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, 0..3);
}

#[test]
fn combining_marks_have_zero_advance_and_an_offset() {
    let (g, _) = shape("e\u{301}x", 10.0, &Limits::default()).unwrap();
    assert_eq!(g.advance[1], LayoutUnit::ZERO);
    assert_eq!(g.offset_inline[1].to_f32(), -5.0);
    assert_eq!(g.cluster[1], 1);
    assert_eq!(g.pen[2].to_f32(), 10.0);
}

#[test]
fn pen_positions_restart_before_saturating() {
    // 1e6 px per glyph: 16 glyphs fit under 2^30 units (about 1.68e7 px).
    let text = "a".repeat(40);
    let (g, runs) = shape(&text, 1.0e6, &Limits::default()).unwrap();
    let sizes: Vec<u32> = runs.iter().map(|r| r.glyphs.end - r.glyphs.start).collect();
    assert_eq!(sizes, vec![16, 16, 8]);
    assert_eq!(g.pen[16], LayoutUnit::ZERO);
    // Differences inside a run stay exact.
    assert_eq!((g.pen[15] - g.pen[14]).to_f32(), 1.0e6);
    assert_eq!(runs[1].text, 16..32);
}

#[test]
fn glyph_count_limit_is_checked_before_pushing() {
    let limits = Limits {
        max_shaped_glyphs: Some(2),
        ..Limits::default()
    };
    let err = shape("abc", 10.0, &limits).unwrap_err();
    assert_eq!(err.kind, LimitKind::ShapedGlyphs);
}
#[test]
fn arabic_joining_retains_shaper_safety_flags() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::ARABIC.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named(
                "Shodo Fixture Arabic".into(),
            )],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "السلام",
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(p.data.units.iter().any(|u| u.unsafe_to_break));
    assert!(p.data.units.iter().any(|u| u.unsafe_to_concat));
}
#[test]
fn optical_sizing_and_explicit_variations_survive_public_views() {
    let bytes = crate::test_support::fonts::LATIN;
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            bytes[at..at + 4].try_into().unwrap(),
            bytes[offset..offset + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, 2, 20, 0, 8] {
        fvar.extend(field.to_be_bytes());
    }
    for (tag, values) in [(b"wght", [100i32, 400, 900]), (b"opsz", [8, 12, 72])] {
        fvar.extend(tag);
        for value in values {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Variable".into(),
                weight: (100.0, 900.0),
                ..Default::default()
            },
        )
        .unwrap();
    for (optical, explicit, want) in [
        (true, None, vec![1.0, 1.0]),
        (false, None, vec![1.0, 0.0]),
        (true, Some(8.0), vec![1.0, -1.0]),
    ] {
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 72.0,
                font_families: vec![crate::style::FontFamily::Named("Variable".into())],
                font_weight: 900.0,
                font_optical_sizing: optical,
                font_variations: explicit
                    .map(|value| crate::style::FontVariation {
                        tag: *b"opsz",
                        value,
                    })
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        let crate::LineResult::Line(line) = p.next_line(
            &mut crate::LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let run = line
            .fragments()
            .find_map(|f| {
                if let crate::Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            run.normalized_coords()
                .iter()
                .map(|c| c.to_f32())
                .collect::<Vec<_>>(),
            want
        );
        assert!(
            run.variations()
                .iter()
                .any(|v| v.tag == *b"wght" && v.value == 900.0)
        );
    }
}
#[test]
fn real_font_pen_splits_before_prefix_overflow() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Latin".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"W".repeat(32),
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(p.data.runs.len() > 1);
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
    );
    assert_eq!(p.data.glyphs.len(), 32);
}

#[test]
fn expanded_single_cluster_pen_splits_without_new_breaks() {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let a = font.charmap().map('a').unwrap().to_u32() as u16;
    let w = font.charmap().map('W').unwrap().to_u32() as u16;
    // One ccmp MultipleSubst expands a single input cluster to32 glyphs.
    let mut gsub = Vec::new();
    for value in [1u16, 0, 10, 30, 44, 1] {
        gsub.extend(value.to_be_bytes());
    }
    gsub.extend(b"latn");
    for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gsub.extend(value.to_be_bytes());
    }
    gsub.extend(b"ccmp");
    for value in [8u16, 0, 1, 0, 1, 4, 2, 0, 1, 8, 1, 74, 1, 8, 32] {
        gsub.extend(value.to_be_bytes());
    }
    for _ in 0..32 {
        gsub.extend(w.to_be_bytes());
    }
    for value in [1u16, 1, a] {
        gsub.extend(value.to_be_bytes());
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
    let bytes = crate::font::sfnt::build_sfnt(&tables);
    let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
    let direct_data = harfrust::ShaperData::new(&font);
    let shaper = direct_data.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str("a");
    buffer.guess_segment_properties();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    assert_eq!(shaped.len(), 32, "direct test substitution must expand");
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Expansion".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Expansion".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "a",
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert_eq!(p.data.glyphs.len(), 32, "test substitution must expand");
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
    );
    assert!(p.data.runs.len() > 1);
    assert!(
        p.data.units[..p.data.units.len() - 1]
            .iter()
            .all(|u| u.break_after == crate::analysis::units::BreakClass::Prohibited)
    );
}

#[test]
fn negative_positioning_advances_obey_run_pen_budget() {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let glyph = skrifa::FontRef::from_index(bytes, 0)
        .unwrap()
        .charmap()
        .map('W')
        .unwrap()
        .to_u32() as u16;
    let mut gpos = Vec::new();
    for value in [1u16, 0, 10, 30, 44, 1] {
        gpos.extend(value.to_be_bytes());
    }
    gpos.extend(b"latn");
    for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gpos.extend(value.to_be_bytes());
    }
    gpos.extend(b"kern");
    for value in [
        8u16,
        0,
        1,
        0,
        1,
        4,
        1,
        0,
        1,
        8,
        1,
        8,
        4,
        (-2000i16) as u16,
        1,
        1,
        glyph,
    ] {
        gpos.extend(value.to_be_bytes());
    }
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if tag != *b"GPOS" {
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
    }
    tables.push((*b"GPOS", gpos));
    tables.sort_by_key(|(tag, _)| *tag);
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Negative".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Negative".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"W".repeat(64),
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(
        p.data.glyphs.advance.iter().all(|a| a.raw() < 0),
        "test positioning must make advances negative"
    );
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|p| i64::from(p.raw()).abs() <= i64::from(RUN_PEN_LIMIT))
    );
}

#[test]
fn tiny_run_windows_share_one_resolved_instance() {
    for real in [false, true] {
        let limits = Limits {
            max_shaping_run_bytes: Some(1),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        if real {
            fonts
                .register_face(
                    crate::test_support::fonts::LATIN.to_vec(),
                    0,
                    crate::font::FontFaceDescriptor {
                        family: "Latin".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                font_features: vec![crate::style::FontFeature {
                    tag: *b"liga",
                    value: 0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"a".repeat(32),
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert_eq!(p.data.runs.len(), 32);
        assert!(
            p.data
                .runs
                .iter()
                .all(|r| Arc::ptr_eq(&r.instance, &p.data.runs[0].instance)),
            "real={real}"
        );
    }
}

#[test]
fn smaller_run_budget_automatically_releases_retained_scratch() {
    let limits = Limits::unlimited();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Latin".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = crate::LayoutContext::new();
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"a".repeat(4096),
    );
    b.build(&mut cx, &fonts).unwrap();
    assert!(cx.scratch_bytes > 1000);
    let limits = Limits {
        max_shaping_run_bytes: Some(8),
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "a",
    );
    b.build(&mut cx, &fonts).unwrap();
    assert!(cx.scratch_bytes <= 8 * 128);
}

#[test]
fn shaping_windows_borrow_source_scalars() {
    for real in [false, true] {
        for budget in [1, 1024] {
            let limits = Limits {
                max_shaping_run_bytes: Some(budget),
                ..Default::default()
            };
            let fonts = FontCollection::with_options(
                &limits,
                crate::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            if real {
                fonts
                    .register_face(
                        crate::test_support::fonts::LATIN.to_vec(),
                        0,
                        crate::font::FontFaceDescriptor {
                            family: "Latin".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            let style = crate::style::ParagraphStyle {
                root: crate::style::InlineStyle {
                    font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                    font_features: vec![crate::style::FontFeature {
                        tag: *b"liga",
                        value: 0,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut b = crate::ParagraphBuilder::new(&style, &limits);
            b.push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(1),
                    offset: 7,
                },
                &"abcd".repeat(8),
            );
            let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
            let original: Vec<_> = p
                .data
                .shape_items
                .iter()
                .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                .collect();
            crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
            let mut warnings = crate::limits::WarningSink::default();
            let (glyphs, runs) = shape_items(
                &mut crate::LayoutContext::new(),
                &p.data.shape_items,
                &p.data.styles,
                &fonts,
                style.writing_mode,
                &limits,
                &mut warnings,
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(glyphs.id, p.data.glyphs.id);
            assert_eq!(glyphs.cluster, p.data.glyphs.cluster);
            assert_eq!(glyphs.advance, p.data.glyphs.advance);
            assert_eq!(glyphs.pen, p.data.glyphs.pen);
            assert_eq!(
                runs.iter()
                    .map(|r| (r.text.clone(), r.item))
                    .collect::<Vec<_>>(),
                p.data
                    .runs
                    .iter()
                    .map(|r| (r.text.clone(), r.item))
                    .collect::<Vec<_>>()
            );
            assert_eq!(runs.len(), if budget == 1 { 32 } else { 1 });
            assert_eq!(warnings.take().is_empty(), real);
            assert_eq!(
                p.data
                    .shape_items
                    .iter()
                    .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                    .collect::<Vec<_>>(),
                original,
                "retained original scalar ownership must be preserved"
            );
            assert_eq!(
                crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
                0,
                "real={real}, budget={budget}: shaping a window must borrow its scalars"
            );
        }
    }
}
