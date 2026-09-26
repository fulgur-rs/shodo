use super::*;

/// Repository-owned synthetic font, deliberately independent of installed fonts.
pub(super) fn test_font(family: &str, chars: &[char], width: u16) -> Vec<u8> {
    let mut head = vec![0; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut hhea = vec![0; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[4..6].copy_from_slice(&750i16.to_be_bytes());
    hhea[6..8].copy_from_slice(&(-250i16).to_be_bytes());
    hhea[8..10].copy_from_slice(&100i16.to_be_bytes());
    let count = chars.len() as u16 + 1;
    hhea[34..36].copy_from_slice(&count.to_be_bytes());
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend_from_slice(&count.to_be_bytes());
    let mut hmtx = Vec::new();
    for _ in 0..count {
        hmtx.extend_from_slice(&width.to_be_bytes());
        hmtx.extend_from_slice(&0u16.to_be_bytes());
    }
    // Format 12: one single-character group for each sorted code point.
    let mut cmap = vec![0, 0, 0, 1, 0, 3, 0, 10, 0, 0, 0, 12, 0, 12, 0, 0];
    cmap.extend_from_slice(&(16u32 + 12 * chars.len() as u32).to_be_bytes());
    cmap.extend_from_slice(&0u32.to_be_bytes());
    cmap.extend_from_slice(&(chars.len() as u32).to_be_bytes());
    let mut chars = chars.to_vec();
    chars.sort();
    chars.dedup();
    for (index, ch) in chars.iter().enumerate() {
        cmap.extend_from_slice(&(*ch as u32).to_be_bytes());
        cmap.extend_from_slice(&(*ch as u32).to_be_bytes());
        cmap.extend_from_slice(&(index as u32 + 1).to_be_bytes());
    }
    let names = [
        (1, family.to_owned()),
        (2, "Regular".into()),
        (4, format!("{family} Regular")),
        (6, format!("{family}-Regular")),
    ];
    let mut name = vec![0, 0, 0, 4, 0, 54];
    let mut strings = Vec::new();
    for (id, value) in names {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for field in [
            3u16,
            1,
            0x0409,
            id,
            encoded.len() as u16,
            strings.len() as u16,
        ] {
            name.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend(encoded);
    }
    name.extend(strings);
    sfnt::build_sfnt(&[
        (*b"cmap", cmap),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
        (*b"name", name),
    ])
}

fn descriptor(family: &str) -> FontFaceDescriptor {
    FontFaceDescriptor {
        family: family.into(),
        ..Default::default()
    }
}

#[test]
fn css_face_registration_preserves_ranges_and_selected_face() {
    let fonts = FontCollection::new(&Limits::default());
    let mut desc = descriptor("Web");
    desc.weight = (300., 700.);
    desc.unicode_ranges = vec![(0x30, 0x39)];
    let id = fonts
        .register_face(test_font("Internal", &['0', 'a'], 600), 0, desc.clone())
        .unwrap();
    assert_eq!(fonts.face_descriptor(id), Some(desc));
    assert_eq!(fonts.font_data(id).unwrap().index, 0);
    assert_eq!(fonts.generation(), 1);
}

#[test]
fn invalid_css_descriptors_and_face_indices_are_transactional() {
    let fonts = FontCollection::new(&Limits::default());
    let bytes = test_font("Internal", &['a'], 600);
    assert!(
        fonts
            .register_face(bytes.clone(), 1, descriptor("Web"))
            .is_err()
    );
    for weight in [(700., 300.), (f32::NAN, 400.), (0., 400.), (400., 1001.)] {
        let mut desc = descriptor("Web");
        desc.weight = weight;
        assert!(fonts.register_face(bytes.clone(), 0, desc).is_err());
    }
    for range in [(0x110000, 0x110001), (90, 65)] {
        let mut desc = descriptor("Web");
        desc.unicode_ranges = vec![range];
        assert!(fonts.register_face(bytes.clone(), 0, desc).is_err());
    }
    assert!(
        fonts
            .register_face(bytes.clone(), 0, descriptor(" "))
            .is_err()
    );
    assert_eq!(fonts.generation(), 0);
    assert_eq!(
        fonts
            .register_face(bytes, 0, descriptor("Web"))
            .unwrap()
            .index(),
        1
    );
}

#[test]
fn css_registration_obeys_layer_budgets_and_isolation() {
    let shared = FontCollection::new(&Limits::default());
    let limits = Limits {
        max_faces_per_layer: Some(1),
        ..Default::default()
    };
    let doc = FontCollection::for_document(&shared, &limits);
    let other = FontCollection::for_document(&shared, &Limits::default());
    let bytes = test_font("Internal", &['a'], 600);
    let id = doc
        .register_face(bytes.clone(), 0, descriptor("Web"))
        .unwrap();
    assert!(other.font_data(id).is_none());
    assert!(shared.face_descriptor(id).is_none());
    assert!(matches!(
        doc.register_face(bytes, 0, descriptor("Web")),
        Err(FontError::Limit(_))
    ));
    assert_eq!(doc.generation(), 1);
}

#[test]
fn css_registration_selects_ttc_face_without_retaining_other_faces() {
    let first = test_font("First", &['a'], 400);
    let second = test_font("Second", &['b'], 700);
    let offsets = [20usize, 20 + first.len()];
    let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    for offset in offsets {
        ttc.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    for (mut face, offset) in [(first, offsets[0]), (second, offsets[1])] {
        let count = u16::from_be_bytes([face[4], face[5]]) as usize;
        for table in 0..count {
            let at = 12 + table * 16 + 8;
            let value = u32::from_be_bytes(face[at..at + 4].try_into().unwrap());
            face[at..at + 4].copy_from_slice(&(value + offset as u32).to_be_bytes());
        }
        ttc.extend(face);
    }
    let shared = FontCollection::new(&Limits::default());
    let limits = Limits {
        max_faces_per_layer: Some(1),
        ..Default::default()
    };
    let doc = FontCollection::for_document(&shared, &limits);
    let id = doc.register_face(ttc, 1, descriptor("Web")).unwrap();
    assert_eq!(fonts_data_index(&doc, id), 1);
    assert_eq!(id.index(), 0);
}

fn fonts_data_index(fonts: &FontCollection, id: FontId) -> u32 {
    fonts.font_data(id).unwrap().index
}

fn no_system() -> FontCollection {
    FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}
fn query(families: &[&str], weight: f32) -> FontQuery {
    FontQuery {
        families: families
            .iter()
            .map(|name| crate::style::FontFamily::Named((*name).into()))
            .collect(),
        weight,
        ..Default::default()
    }
}
fn add_face(fonts: &FontCollection, family: &str, weight: (f32, f32), chars: &[char]) -> FontId {
    let mut desc = descriptor(family);
    desc.weight = weight;
    fonts
        .register_face(test_font(family, chars, 600), 0, desc)
        .unwrap()
}

#[test]
fn family_order_precedes_document_layer_priority() {
    let shared = no_system();
    let early = add_face(&shared, "Early", (400., 400.), &['a']);
    let doc = FontCollection::for_document(&shared, &Limits::default());
    let late = add_face(&doc, "Late", (400., 400.), &['a']);
    assert_eq!(
        doc.match_cluster(&query(&["Early", "Late"], 400.), "a")
            .unwrap()
            .id,
        early
    );
    assert_eq!(
        doc.match_cluster(&query(&["Late", "Early"], 400.), "a")
            .unwrap()
            .id,
        late
    );
    let override_id = add_face(&doc, "EARLY", (400., 400.), &['a']);
    assert_eq!(
        doc.match_cluster(&query(&["Early"], 400.), "a").unwrap().id,
        override_id
    );
}

#[test]
fn css_weight_ranges_and_400_to_500_search_order() {
    let fonts = no_system();
    let light = add_face(&fonts, "Web", (300., 300.), &['a']);
    let medium = add_face(&fonts, "Web", (500., 500.), &['a']);
    let bold = add_face(&fonts, "Web", (700., 900.), &['a']);
    for (weight, id) in [
        (200., light),
        (400., medium),
        (450., medium),
        (600., bold),
        (800., bold),
        (1000., bold),
    ] {
        assert_eq!(
            fonts
                .match_cluster(&query(&["Web"], weight), "a")
                .unwrap()
                .id,
            id
        );
    }
    let regular = add_face(&fonts, "Web", (400., 400.), &['a']);
    assert_eq!(
        fonts.match_cluster(&query(&["Web"], 400.), "a").unwrap().id,
        regular
    );
}

#[test]
fn unicode_ranges_and_whole_cluster_cmap_are_required() {
    let fonts = no_system();
    let mut desc = descriptor("Web");
    desc.unicode_ranges = vec![(0x61, 0x61)];
    let restricted = fonts
        .register_face(test_font("Internal", &['a', 'b'], 600), 0, desc)
        .unwrap();
    let complete = add_face(&fonts, "Fallback", (400., 400.), &['a', 'b', '\u{301}']);
    assert_eq!(
        fonts
            .match_cluster(&query(&["Web", "Fallback"], 400.), "a")
            .unwrap()
            .id,
        restricted
    );
    assert_eq!(
        fonts
            .match_cluster(&query(&["Web", "Fallback"], 400.), "b")
            .unwrap()
            .id,
        complete
    );
    assert_eq!(
        fonts
            .match_cluster(&query(&["Web", "Fallback"], 400.), "a\u{301}")
            .unwrap()
            .id,
        complete
    );
    assert!(fonts.match_cluster(&query(&["Web"], 400.), "z").is_none());
    assert!(fonts.match_cluster(&query(&["Web"], 400.), "").is_none());
}

#[test]
fn cached_misses_and_generic_results_follow_shared_generation() {
    let fonts = no_system();
    let doc = FontCollection::for_document(&fonts, &Limits::default());
    let q = query(&["Later"], 400.);
    assert!(doc.match_cluster(&q, "a").is_none());
    let id = add_face(&fonts, "Later", (400., 400.), &['a']);
    assert_eq!(doc.match_cluster(&q, "a").unwrap().id, id);
    fonts.set_generic_families(crate::style::GenericFamily::SansSerif, vec!["Later".into()]);
    assert_eq!(
        doc.match_cluster(&FontQuery::default(), "a").unwrap().id,
        id
    );
    let new = add_face(&fonts, "New", (400., 400.), &['a']);
    fonts.set_generic_families(crate::style::GenericFamily::SansSerif, vec!["New".into()]);
    assert_eq!(
        doc.match_cluster(&FontQuery::default(), "a").unwrap().id,
        new
    );
}

#[test]
fn locale_fallback_selects_cjk_faces_deterministically() {
    let fonts = no_system();
    let jp = add_face(&fonts, "Japanese", (400., 400.), &['漢']);
    let cn = add_face(&fonts, "Chinese", (400., 400.), &['漢']);
    fonts.set_fallback_families(*b"Hani", Some("ja".into()), vec!["Japanese".into()]);
    fonts.set_fallback_families(*b"Hani", Some("zh-Hans".into()), vec!["Chinese".into()]);
    for (language, id) in [("ja-JP", jp), ("zh-Hans-CN", cn)] {
        let q = FontQuery {
            families: vec![],
            script: *b"Hani",
            language: Some(language.into()),
            ..Default::default()
        };
        assert_eq!(fonts.match_cluster(&q, "漢").unwrap().id, id);
    }
}

#[test]
fn width_and_style_matching_precede_weight() {
    use crate::style::FontStyle;
    let fonts = no_system();
    let normal = add_face(&fonts, "Web", (400., 400.), &['a']);
    let mut desc = descriptor("Web");
    desc.width = (75., 75.);
    desc.style = FontStyle::Italic;
    desc.weight = (700., 700.);
    let narrow = fonts
        .register_face(test_font("Web", &['a'], 600), 0, desc)
        .unwrap();
    let mut q = query(&["Web"], 400.);
    q.width = 80.;
    q.style = FontStyle::Italic;
    assert_eq!(fonts.match_cluster(&q, "a").unwrap().id, narrow);
    q.width = 100.;
    assert_eq!(fonts.match_cluster(&q, "a").unwrap().id, normal);
}

#[test]
fn raw_bundled_fonts_are_matched_by_intrinsic_family() {
    let fonts = no_system();
    let id = fonts.register(test_font("Native", &['a'], 600)).unwrap();
    assert_eq!(
        fonts
            .match_cluster(&query(&["Native"], 400.), "a")
            .unwrap()
            .id,
        id
    );
}

#[test]
fn emoji_and_text_presentation_prefer_color_and_monochrome_faces() {
    let fonts = no_system();
    let text = add_face(&fonts, "Symbol", (400., 400.), &['☺', '😀']);
    // Rebuild table directory with a minimal color marker for selection.
    let plain = test_font("Color", &['☺', '😀'], 600);
    let count = u16::from_be_bytes([plain[4], plain[5]]) as usize;
    let mut tables = Vec::new();
    for n in 0..count {
        let at = 12 + n * 16;
        let tag = plain[at..at + 4].try_into().unwrap();
        let start = u32::from_be_bytes(plain[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(plain[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((tag, plain[start..start + len].to_vec()));
    }
    tables.push((*b"COLR", vec![0; 14]));
    let color = fonts
        .register_face(sfnt::build_sfnt(&tables), 0, descriptor("Symbol"))
        .unwrap();
    for (cluster, id) in [
        ("☺", text),
        ("☺\u{fe0f}", color),
        ("😀", color),
        ("😀\u{fe0e}", text),
    ] {
        assert_eq!(
            fonts
                .match_cluster(&query(&["Symbol"], 400.), cluster)
                .unwrap()
                .id,
            id
        );
    }
}

#[test]
fn css_descriptor_controls_synthesis_instead_of_internal_font_metadata() {
    let fonts = no_system();
    let id = add_face(&fonts, "Web", (700., 700.), &['a']);
    let found = fonts.match_cluster(&query(&["Web"], 700.), "a").unwrap();
    assert_eq!(found.id, id);
    assert!(
        !found.embolden,
        "a CSS bold face must not be emboldened again"
    );
    let mut desc = descriptor("Italic");
    desc.style = crate::style::FontStyle::Italic;
    let italic = fonts
        .register_face(test_font("Internal", &['a'], 600), 0, desc)
        .unwrap();
    let mut q = query(&["Italic"], 400.);
    q.style = crate::style::FontStyle::Italic;
    let found = fonts.match_cluster(&q, "a").unwrap();
    assert_eq!(found.id, italic);
    assert_eq!(found.skew, None);
}

#[test]
fn oblique_matching_follows_css_direction_above_and_below_eleven_degrees() {
    use crate::style::FontStyle;
    let fonts = no_system();
    let mut ids = Vec::new();
    for angle in [5., 10., 20., -5., -10., -20.] {
        let mut desc = descriptor("Slant");
        desc.style = FontStyle::Oblique(angle);
        ids.push(
            fonts
                .register_face(test_font("Slant", &['a'], 600), 0, desc)
                .unwrap(),
        );
    }
    for (angle, index) in [(12., 2), (8., 0), (-12., 5), (-8., 3)] {
        let mut q = query(&["Slant"], 400.);
        q.style = FontStyle::Oblique(angle);
        assert_eq!(fonts.match_cluster(&q, "a").unwrap().id, ids[index]);
    }
}

#[test]
fn later_css_faces_win_identical_descriptors_and_cache_is_bounded() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 1,
            ..Default::default()
        },
    );
    add_face(&fonts, "Web", (400., 400.), &['a', 'b']);
    let last = add_face(&fonts, "Web", (400., 400.), &['a', 'b']);
    let q = query(&["Web"], 400.);
    assert_eq!(fonts.match_cluster(&q, "a").unwrap().id, last);
    assert_eq!(fonts.match_cluster(&q, "b").unwrap().id, last);
    assert_eq!(fonts.state().matches.len(), 1);
    let unbounded = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 0,
            ..Default::default()
        },
    );
    add_face(&unbounded, "Web", (400., 400.), &['a']);
    assert!(unbounded.match_cluster(&q, "a").is_some());
    assert!(unbounded.state().matches.is_empty());
}

#[test]
fn variable_weight_matches_inside_css_range_and_returns_clamped_axis_value() {
    let plain = test_font("Variable", &['a'], 600);
    let count = u16::from_be_bytes([plain[4], plain[5]]) as usize;
    let mut tables = Vec::new();
    for n in 0..count {
        let at = 12 + n * 16;
        let start = u32::from_be_bytes(plain[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(plain[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            plain[at..at + 4].try_into().unwrap(),
            plain[start..start + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, 1, 20, 0, 8] {
        fvar.extend_from_slice(&field.to_be_bytes());
    }
    fvar.extend_from_slice(b"wght");
    for value in [100i32, 400, 900] {
        fvar.extend_from_slice(&(value << 16).to_be_bytes());
    }
    fvar.extend_from_slice(&[0, 0, 1, 0]);
    tables.push((*b"fvar", fvar));
    let fonts = no_system();
    let mut desc = descriptor("Web");
    desc.weight = (600., 800.);
    let id = fonts
        .register_face(sfnt::build_sfnt(&tables), 0, desc)
        .unwrap();
    for (requested, value) in [(400., 600.), (700., 700.), (900., 800.)] {
        let found = fonts
            .match_cluster(&query(&["Web"], requested), "a")
            .unwrap();
        assert_eq!(found.id, id);
        assert_eq!(
            found.variations,
            vec![crate::style::FontVariation {
                tag: *b"wght",
                value
            }]
        );
        assert!(!found.embolden);
    }
}

#[test]
fn local_resolves_full_and_postscript_names_without_rewriting_css_family() {
    let shared = no_system();
    shared.register(test_font("Native", &['a'], 600)).unwrap();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    for name in ["Native Regular", "Native-Regular"] {
        let id = doc
            .register_sources(descriptor("Alias"), vec![FontSource::Local(name.into())])
            .unwrap();
        assert_eq!(doc.face_descriptor(id).unwrap().family, "Alias");
        assert_eq!(
            doc.match_cluster(&query(&["Alias"], 400.), "a").unwrap().id,
            id
        );
        assert!(shared.font_data(id).is_none());
    }
    assert!(
        doc.register_sources(descriptor("Bad"), vec![FontSource::Local("Native".into())])
            .is_err()
    );
}

#[test]
fn font_source_order_and_failed_source_fallback_are_preserved() {
    let shared = no_system();
    let native = shared.register(test_font("Native", &['a'], 500)).unwrap();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    let data = test_font("Web", &['a'], 900);
    let local_first = doc
        .register_sources(
            descriptor("LocalFirst"),
            vec![
                FontSource::Local("Native-Regular".into()),
                FontSource::Data(data.clone(), 0),
            ],
        )
        .unwrap();
    assert_eq!(
        doc.font_data(local_first).unwrap().data.id(),
        shared.font_data(native).unwrap().data.id()
    );
    let data_first = doc
        .register_sources(
            descriptor("DataFirst"),
            vec![
                FontSource::Data(data.clone(), 0),
                FontSource::Local("Native-Regular".into()),
            ],
        )
        .unwrap();
    assert_ne!(
        doc.font_data(data_first).unwrap().data.id(),
        shared.font_data(native).unwrap().data.id()
    );
    let fallback = doc
        .register_sources(
            descriptor("Fallback"),
            vec![
                FontSource::Local("Missing-Regular".into()),
                FontSource::Data(vec![1, 2, 3], 0),
                FontSource::Data(data, 0),
            ],
        )
        .unwrap();
    assert_eq!(
        doc.match_cluster(&query(&["Fallback"], 400.), "a")
            .unwrap()
            .id,
        fallback
    );
    assert_eq!(doc.generation(), 3);
}

#[test]
fn local_aliases_retain_shared_blob_once_and_source_limits_fail_closed() {
    let shared = no_system();
    let bytes = test_font("Native", &['a'], 500);
    let length = bytes.len() as u64;
    shared.register(bytes).unwrap();
    let limits = Limits {
        max_layer_blob_bytes: Some(length),
        ..Default::default()
    };
    let doc = FontCollection::for_document(&shared, &limits);
    doc.register_sources(
        descriptor("One"),
        vec![FontSource::Local("Native-Regular".into())],
    )
    .unwrap();
    doc.register_sources(
        descriptor("Two"),
        vec![FontSource::Local("Native-Regular".into())],
    )
    .unwrap();
    assert_eq!(doc.state().blob_bytes, length);
    assert!(matches!(
        doc.register_sources(
            descriptor("Denied"),
            vec![
                FontSource::Data(test_font("Other", &['a'], 600), 0),
                FontSource::Local("Native-Regular".into())
            ]
        ),
        Err(FontError::Limit(_))
    ));
    assert_eq!(doc.generation(), 2);
}

#[test]
fn real_font_metrics_replace_stub_values_and_unknown_faces_are_absent() {
    let fonts = no_system();
    let id = add_face(&fonts, "Metrics", (400., 400.), &['0', '水']);
    let m = fonts.metrics_with_coords(id, 10., &[]).unwrap();
    assert_eq!((m.ascent, m.descent, m.line_gap), (7.5, 2.5, 1.));
    assert_eq!(fonts.metrics(id, 10.), m);
    assert!(
        fonts
            .metrics_with_coords(
                FontId {
                    layer: u32::MAX,
                    index: 0
                },
                10.,
                &[]
            )
            .is_none()
    );
    assert!(fonts.metrics_with_coords(id, f32::NAN, &[]).is_none());
}

#[test]
fn font_units_report_selected_fallback_face_and_css_missing_defaults() {
    let fonts = no_system();
    let zero = fonts
        .register_face(test_font("Digits", &['0'], 600), 0, descriptor("Digits"))
        .unwrap();
    let water = fonts
        .register_face(test_font("CJK", &['水'], 1000), 0, descriptor("CJK"))
        .unwrap();
    let q = query(&["Digits", "CJK"], 400.);
    let ch = fonts.resolve_ch(&q, 10.);
    assert_eq!((ch.id, ch.advance), (Some(zero), 6.));
    let ic = fonts.resolve_ic(&q, 10.);
    assert_eq!(ic.id, Some(water));
    assert!((ic.advance - 10.).abs() < 0.0001);
    let empty = no_system();
    assert_eq!(
        (
            empty.resolve_ch(&q, 10.).id,
            empty.resolve_ch(&q, 10.).advance
        ),
        (None, 5.)
    );
    assert_eq!(
        (
            empty.resolve_ic(&q, 10.).id,
            empty.resolve_ic(&q, 10.).advance
        ),
        (None, 10.)
    );
}

#[test]
fn shaper_cache_is_shared_bounded_and_zero_capacity_still_returns_data() {
    let limits = Limits {
        max_shaper_cache_entries: Some(1),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let first = add_face(&fonts, "One", (400., 400.), &['a']);
    let second = add_face(&fonts, "Two", (400., 400.), &['b']);
    let a = fonts.shaper_data(first).unwrap();
    assert!(Arc::ptr_eq(&a, &fonts.clone().shaper_data(first).unwrap()));
    let doc = FontCollection::for_document(&fonts, &Limits::default());
    assert!(Arc::ptr_eq(&a, &doc.shaper_data(first).unwrap()));
    let _b = fonts.shaper_data(second).unwrap();
    assert_eq!(fonts.state().shapers.len(), 1);
    assert!(!Arc::ptr_eq(&a, &fonts.shaper_data(first).unwrap()));
    let limits = Limits {
        max_shaper_cache_entries: Some(0),
        ..Default::default()
    };
    let empty = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = add_face(&empty, "Zero", (400., 400.), &['a']);
    assert!(empty.shaper_data(id).is_some());
    assert!(empty.state().shapers.is_empty());
}

#[test]
fn weak_layer_handles_observe_release_without_retaining_document_fonts() {
    let fonts = no_system();
    let doc = FontCollection::for_document(&fonts, &Limits::default());
    let id = add_face(&doc, "Document", (400., 400.), &['a']);
    let handle = doc.layer_handle();
    assert_eq!(handle.id(), id.layer());
    let clone = doc.clone();
    drop(doc);
    assert!(handle.is_alive());
    drop(clone);
    assert!(!handle.is_alive());
    assert!(fonts.layer_handle().is_alive());
}

#[test]
fn shaper_data_and_handles_are_send_sync() {
    fn check<T: Send + Sync>() {}
    check::<harfrust::ShaperData>();
    check::<WeakFontLayer>();
}

#[test]
fn layer_allocator_fails_on_exhaustion_instead_of_reusing_an_id() {
    let counter = AtomicU32::new(u32::MAX - 1);
    assert_eq!(allocate_layer_id(&counter), Some(u32::MAX - 1));
    assert_eq!(allocate_layer_id(&counter), None);
    assert_eq!(counter.load(Ordering::Relaxed), u32::MAX);
}

#[test]
fn vertical_font_metrics_use_vhea_and_report_absence() {
    let fonts = no_system();
    let plain = test_font("Horizontal", &['a'], 600);
    let id = fonts
        .register_face(plain.clone(), 0, descriptor("Horizontal"))
        .unwrap();
    assert!(fonts.vertical_metrics(id, 10., &[]).is_none());
    let count = u16::from_be_bytes([plain[4], plain[5]]) as usize;
    let mut tables = Vec::new();
    for n in 0..count {
        let at = 12 + n * 16;
        let start = u32::from_be_bytes(plain[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(plain[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            plain[at..at + 4].try_into().unwrap(),
            plain[start..start + len].to_vec(),
        ));
    }
    let mut vhea = vec![0; 36];
    vhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    vhea[4..6].copy_from_slice(&500i16.to_be_bytes());
    vhea[6..8].copy_from_slice(&(-500i16).to_be_bytes());
    vhea[8..10].copy_from_slice(&200i16.to_be_bytes());
    tables.push((*b"vhea", vhea));
    let id = fonts
        .register_face(sfnt::build_sfnt(&tables), 0, descriptor("Vertical"))
        .unwrap();
    let m = fonts.vertical_metrics(id, 10., &[]).unwrap();
    assert_eq!((m.ascent, m.descent, m.line_gap), (5., 5., 2.));
}
