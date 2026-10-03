use super::*;
use crate::font::FontCollection;
use crate::geometry::LayoutUnit;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;

#[test]
fn combined_width_trials_reuse_selected_result_for_two_shaper_calls() {
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
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
            text_combine_upright: crate::style::TextCombineUpright::All,
            text_autospace: crate::style::TextAutospace::NoAutospace,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "12",
    );

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.set(0));
    COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.set(0));
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();

    assert!(
        paragraph
            .data
            .shape_items
            .iter()
            .all(|item| item.width_feature == Some(*b"hwid"))
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 2);
    assert_eq!(
        COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get()),
        0
    );
    assert!(COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.get()) > 0);
}

#[test]
fn combined_width_reuse_preserves_global_glyph_limit_failure() {
    let limits = Limits {
        max_shaped_glyphs: Some(3),
        ..Limits::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = crate::style::InlineStyle {
        font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
        ..Default::default()
    };
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: root.clone(),
        ..Default::default()
    };
    let mut combined = root;
    combined.text_combine_upright = crate::style::TextCombineUpright::All;
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    for node in [1, 2] {
        let node = crate::node::NodeId(node);
        builder
            .open_inline(node, &combined, crate::node::InlineEdges::default())
            .push_text(crate::node::TextSource::Generated { node }, "12")
            .close_inline();
    }

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let error = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap_err();
    assert_eq!(
        (error.kind, error.limit, error.actual),
        (LimitKind::ShapedGlyphs, 3, 4)
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 5);
}

#[test]
fn ruby_base_glyph_limits_keep_the_regular_trial_and_final_shape_path() {
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
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let base_style = crate::style::InlineStyle {
        font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
        text_combine_upright: crate::style::TextCombineUpright::All,
        ..Default::default()
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: base_style.clone(),
        ..Default::default()
    };
    let base = crate::RubyContent::text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(2),
        },
        "12",
        &base_style,
        &Limits {
            max_shaped_glyphs: Some(1),
            ..Limits::default()
        },
    );
    let annotation = crate::RubyContent::text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(3),
        },
        "あ",
        &base_style,
        &limits,
    );
    let ruby = crate::Ruby::new(
        vec![crate::RubyBase {
            node: crate::node::NodeId(2),
            content: base,
            align: crate::RubyAlign::default(),
        }],
        vec![crate::RubyLevel {
            annotations: vec![crate::RubyAnnotation {
                node: crate::node::NodeId(3),
                content: annotation,
                span: crate::RubySpan::Auto,
                visibility: crate::RubyVisibility::Visible,
            }],
            style: crate::RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = crate::ParagraphBuilder::new(&paragraph_style, &limits);
    builder.push_ruby(crate::node::NodeId(1), &base_style, ruby);

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let error = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap_err();
    assert_eq!((error.kind, error.limit), (LimitKind::ShapedGlyphs, 1));
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 3);
}

#[test]
fn combined_width_reuse_replays_selected_warnings_through_the_global_cap() {
    let limits = Limits {
        max_shaping_run_bytes: Some(0),
        max_warnings: Some(1),
        ..Limits::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
            text_combine_upright: crate::style::TextCombineUpright::All,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "12",
    );

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(
        paragraph
            .warnings()
            .iter()
            .map(|warning| warning.kind)
            .collect::<Vec<_>>(),
        [
            crate::limits::WarningKind::Unsupported,
            crate::limits::WarningKind::Suppressed,
        ]
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 4);
}

#[test]
fn split_items_share_retained_author_features() {
    for real in [false, true] {
        let limits = Limits::default();
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
                font_features: vec![
                    crate::style::FontFeature {
                        tag: *b"liga",
                        value: 0
                    };
                    256
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        for _ in 0..32 {
            builder.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                "a",
            );
            builder.push_forced_break(crate::node::NodeId(1));
        }
        features::STYLE_FEATURE_BUILDS.with(|count| count.set(0));
        let p = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        assert_eq!(p.data.runs.len(), 32);
        let first = &p.data.runs[0].instance.features;
        assert_eq!(first.len(), 256);
        for run in &p.data.runs {
            assert_eq!(
                run.instance.features.as_ptr(),
                first.as_ptr(),
                "real={real}"
            );
        }
        assert_eq!(features::STYLE_FEATURE_BUILDS.with(|count| count.get()), 1);
        for _ in 0..2 {
            let (_, edge) = shape_window(
                &p.data,
                &p.data.units[0],
                &mut crate::LayoutContext::new(),
                &mut crate::limits::WarningSink::default(),
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(edge[0].instance.features.as_ptr(), first.as_ptr());
        }
        let replacement = Replacement {
            text: 0..1,
            c: 'b',
            font: None,
        };
        let (_, edited) = shape_window_edit(
            &p.data,
            &p.data.units[0],
            None,
            Some(&replacement),
            &mut crate::LayoutContext::new(),
            &mut crate::limits::WarningSink::default(),
            &mut Saturation::default(),
        )
        .unwrap();
        assert_eq!(edited[0].instance.features.as_ptr(), first.as_ptr());
    }
}

#[test]
fn shared_features_preserve_style_orientation_and_width_settings() {
    let p = edge_input_paragraph(false, 1024);
    let mut styles = vec![p.data.styles[p.data.shape_items[0].style as usize].clone(); 2];
    styles[1].font_features.push(crate::style::FontFeature {
        tag: *b"liga",
        value: 1,
    });
    let mut items = Vec::new();
    for _ in 0..2 {
        for (orientation, width, style) in [
            (orientation::RunOrientation::Horizontal, None, 0),
            (orientation::RunOrientation::Upright, None, 0),
            (orientation::RunOrientation::Horizontal, Some(*b"hwid"), 0),
            (orientation::RunOrientation::Horizontal, None, 1),
        ] {
            let mut item = p.data.shape_items[0].clone();
            item.orientation = orientation;
            item.width_feature = width;
            item.style = style;
            items.push(item);
        }
    }
    let (_, runs) = shape_items(
        &mut crate::LayoutContext::new(),
        &items,
        &styles,
        &p.data.fonts,
        WritingMode::HorizontalTb,
        &Limits::default(),
        &mut crate::limits::WarningSink::default(),
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(runs.len(), 8);
    for (i, run) in runs.iter().enumerate() {
        let expected = features::for_item(&styles[items[i].style as usize], &items[i]);
        assert_eq!(
            format!("{:?}", run.instance.features),
            format!("{expected:?}")
        );
        assert_eq!(
            run.instance.features.as_ptr(),
            runs[i % 4].instance.features.as_ptr()
        );
    }
}

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
        &mut store, &mut runs, text, 0, 0, font, size, limits, &mut sat, 0,
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
    for at in 0..32 {
        gsub.extend(if at % 3 == 0 { a } else { w }.to_be_bytes());
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
    // Distinct glyphs and advances expose a missing RTL part reversal, which a
    // uniform expansion cannot catch. Force the retained item's bidi level so
    // the same pinned Latin substitution exercises both shaping directions.
    for rtl in [false, true] {
        let mut items = p.data.shape_items.clone();
        for item in &mut items {
            item.level = u8::from(rtl);
        }
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("a");
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(if rtl {
            harfrust::Direction::RightToLeft
        } else {
            harfrust::Direction::LeftToRight
        });
        let expected = shaper.shape(buffer, harfrust::ShapeOptions::default());
        let mut warnings = crate::limits::WarningSink::default();
        let mut sat = Saturation::default();
        let (glyphs, runs) = shape_items(
            &mut crate::LayoutContext::new(),
            &items,
            &p.data.styles,
            &fonts,
            style.writing_mode,
            &limits,
            &mut warnings,
            &mut sat,
        )
        .unwrap();
        assert!(runs.len() > 1);
        assert!(sat.is_clean(), "storage splits must avoid pen saturation");
        assert!(runs.iter().all(|run| run.text == (0..1)));
        assert!(glyphs.cluster.iter().all(|cluster| *cluster == 0));
        let mut visual_runs: Vec<_> = runs.iter().collect();
        if rtl {
            visual_runs.reverse();
        }
        let actual: Vec<_> = visual_runs
            .into_iter()
            .flat_map(|run| {
                glyphs.id[run.glyphs.start as usize..run.glyphs.end as usize]
                    .iter()
                    .copied()
            })
            .collect();
        assert_eq!(
            actual,
            expected
                .glyph_infos()
                .iter()
                .map(|g| g.glyph_id)
                .collect::<Vec<_>>(),
            "rtl={rtl}: splitting must preserve intra-cluster visual order"
        );
        assert!(
            glyphs
                .pen
                .iter()
                .all(|pen| pen.raw().abs() <= RUN_PEN_LIMIT)
        );
        let warnings = warnings.take();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, crate::limits::WarningKind::Unsupported);
        assert!(
            warnings[0]
                .message
                .contains("glyph cluster exceeds run pen budget")
        );
    }
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

#[test]
fn unedited_edge_windows_borrow_source_scalars() {
    // Reintroducing a clipped scalar Vec must fail the actual Clone check;
    // changing the clip/budget must fail offsets and warning/progress checks.
    for real in [false, true] {
        for budget in [1, 1024] {
            let p = edge_input_paragraph(real, budget);
            let ownership: Vec<_> = p
                .data
                .shape_items
                .iter()
                .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                .collect();
            let mut unit = p.data.units[5].clone();
            unit.text = 5..31;
            crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
            let mut warnings = crate::limits::WarningSink::default();
            let (glyphs, runs) = shape_window_budget(
                &p.data,
                &unit,
                None,
                &mut crate::LayoutContext::new(),
                &mut warnings,
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(glyphs.cluster, (5..31).collect::<Vec<_>>());
            assert_eq!(glyphs.id, p.data.glyphs.id[5..31]);
            let expected_runs: Vec<_> = if budget == 1 {
                (5..31).map(|at| at..at + 1).collect()
            } else {
                std::iter::once(5..31).collect()
            };
            assert_eq!(
                runs.iter().map(|run| run.text.clone()).collect::<Vec<_>>(),
                expected_runs
            );
            assert_eq!(
                warnings.take().len(),
                if real {
                    0
                } else if budget == 1 {
                    26
                } else {
                    1
                }
            );
            assert_eq!(
                p.data
                    .shape_items
                    .iter()
                    .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                    .collect::<Vec<_>>(),
                ownership
            );
            assert_eq!(
                crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
                0,
                "real={real}, budget={budget}: unedited edge input must borrow the26 source scalars"
            );
        }
    }
}

fn edge_input_paragraph(real: bool, budget: u64) -> crate::Paragraph {
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
            font_kerning: crate::style::FontKerning::None,
            font_features: vec![crate::style::FontFeature {
                tag: *b"liga",
                value: 0,
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Dom {
            node: crate::node::NodeId(1),
            offset: 7,
        },
        "abcdefghijklmnopqrstuvwxyz0123456789",
    );
    builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap()
}

#[test]
fn compatible_unedited_edge_keeps_owned_merge_and_same_full_shaping() {
    let mut p = edge_input_paragraph(true, 1024);
    let mut unit = p.data.units[5].clone();
    unit.text = 5..31;
    let mut warnings = crate::limits::WarningSink::default();
    let expected = shape_window_budget(
        &p.data,
        &unit,
        None,
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert!(warnings.take().is_empty());
    let data = Arc::get_mut(&mut p.data).unwrap();
    let first = &mut data.shape_items[0];
    let mut second = first.clone();
    second.scalars = first.scalars.split_off(18);
    second.before = first.scalars[13..].iter().map(|s| s.c).collect();
    first.after = second.scalars[..5].iter().map(|s| s.c).collect();
    first.end = 18;
    data.shape_items.push(second);
    crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
    let actual = shape_window_budget(
        &p.data,
        &unit,
        None,
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(
        crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
        26
    );
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert!(warnings.take().is_empty());
}

#[test]
fn font_substitution_edge_keeps_owned_scalars_and_source_end() {
    let p = edge_input_paragraph(true, 1024);
    let mut unit = p.data.units[5].clone();
    unit.text = 5..31;
    let replacement = Replacement {
        text: 5..6,
        c: '‑',
        font: None,
    };
    let mut warnings = crate::limits::WarningSink::default();
    crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
    let (glyphs, runs) = shape_window_edit(
        &p.data,
        &unit,
        None,
        Some(&replacement),
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(
        crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
        26
    );
    assert_eq!(glyphs.cluster, (5..31).collect::<Vec<_>>());
    assert_eq!(glyphs.id[0], 0);
    assert_eq!(
        runs.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
        [5..6, 6..31]
    );
    assert_eq!(p.data.shape_items[0].scalars[5].c, 'f');
    assert_eq!(p.data.shape_items[0].scalars[5].end, 6);
    assert_eq!(warnings.take().len(), 1);
}
