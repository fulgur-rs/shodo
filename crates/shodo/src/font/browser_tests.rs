use super::*;
use crate::style::FontStyle;

/// Repository-owned synthetic font, deliberately independent of installed fonts.
pub(super) fn test_font(family: &str, chars: &[char], width: u16) -> Vec<u8> {
    test_font_with_typographic_name(family, None, chars, width)
}

pub(super) fn test_font_with_typographic_name(
    family: &str,
    typographic_family: Option<&str>,
    chars: &[char],
    width: u16,
) -> Vec<u8> {
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
    let mut names = vec![
        (1, family.to_owned()),
        (2, "Regular".into()),
        (4, format!("{family} Regular")),
        (6, format!("{family}-Regular")),
    ];
    if let Some(typographic_family) = typographic_family {
        names.push((16, typographic_family.to_owned()));
    }
    let mut name = vec![0, 0];
    name.extend_from_slice(&(names.len() as u16).to_be_bytes());
    name.extend_from_slice(&((6 + 12 * names.len()) as u16).to_be_bytes());
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

pub(super) fn font_with_axes(axes: &[([u8; 4], [i32; 3])]) -> Vec<u8> {
    let bytes = test_font("Axes", &['a'], 600);
    let font = skrifa::FontRef::new(&bytes).unwrap();
    let mut tables: Vec<_> = font
        .table_directory()
        .table_records()
        .iter()
        .map(|record| {
            (
                record.tag().to_be_bytes(),
                font.table_data(record.tag()).unwrap().as_bytes().to_vec(),
            )
        })
        .collect();
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, axes.len() as u16, 20, 0, 4] {
        fvar.extend(field.to_be_bytes());
    }
    for (tag, values) in axes {
        fvar.extend(tag);
        for value in values {
            fvar.extend((value * 65536).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    sfnt::build_sfnt(&tables)
}

#[test]
fn invalid_variation_axes_are_rejected_before_registration_changes_the_layer() {
    let limits = Limits::default();
    let shared = no_system();
    for tag in [*b"wght", *b"wdth", *b"slnt", *b"ital", *b"opsz", *b"TEST"] {
        for values in [[900, 400, 100], [100, 99, 900], [100, 901, 900]] {
            let bytes = font_with_axes(&[(*b"GOOD", [-10, 0, 10]), (tag, values)]);
            let fonts = FontCollection::for_document(&shared, &limits);
            for result in [
                fonts.register(bytes.clone()),
                fonts.register_face(bytes.clone(), 0, descriptor("Axes")),
                fonts.register_sources(descriptor("Axes"), vec![FontSource::Data(bytes, 0)]),
            ] {
                assert!(
                    matches!(result, Err(FontError::Malformed(_))),
                    "{tag:?}: {values:?}: {result:?}"
                );
            }
            assert_eq!(fonts.generation(), 0);
            assert_eq!(fonts.state().blob_bytes, 0);
            assert!(fonts.state().faces.is_empty());
            assert_eq!(
                fonts
                    .register_face(test_font("Valid", &['a'], 600), 0, descriptor("Valid"))
                    .unwrap()
                    .index(),
                0
            );
        }
    }
}

#[test]
fn constant_axes_and_endpoint_defaults_keep_matching_and_shaping() {
    use crate::style::{FontFamily, FontVariation, InlineStyle, ParagraphStyle};

    for values in [[100, 100, 900], [100, 900, 900], [400, 400, 400]] {
        let fonts = no_system();
        let id = fonts
            .register_face(
                font_with_axes(&[
                    (*b"wght", values),
                    (*b"slnt", [-10, -10, -10]),
                    (*b"opsz", [12, 12, 12]),
                ]),
                0,
                descriptor("Axes"),
            )
            .unwrap();
        let query = FontQuery {
            families: vec![FontFamily::Named("Axes".into())],
            ..Default::default()
        };
        let found = fonts.match_cluster(&query, "a").unwrap();
        assert_eq!(found.id, id);
        assert!(found.variations.contains(&FontVariation {
            tag: *b"wght",
            value: 400.
        }));
        assert!(found.variations.contains(&FontVariation {
            tag: *b"slnt",
            value: -10.
        }));

        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: query.families,
                font_variations: vec![FontVariation {
                    tag: *b"opsz",
                    value: 999.,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &Limits::default());
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        );
        let mut context = crate::LayoutContext::new();
        let paragraph = builder.build(&mut context, &fonts).unwrap();
        let crate::LineResult::Line(line) = paragraph.next_line(
            &mut context,
            paragraph.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(100.),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!("expected line")
        };
        let run = line
            .fragments()
            .find_map(|fragment| match fragment {
                crate::Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.glyphs().count(), 1);
        assert!(run.variations().contains(&FontVariation {
            tag: *b"opsz",
            value: 12.
        }));
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
fn default_ignorables_do_not_require_cmap_or_unicode_range_coverage() {
    let fonts = no_system();
    let mut desc = descriptor("Primary");
    desc.unicode_ranges = vec![(0x20, 0x7e)];
    let primary = fonts
        .register_face(test_font("Primary", &[' ', 'a'], 600), 0, desc)
        .unwrap();
    add_face(
        &fonts,
        "Fallback",
        (400., 400.),
        &[' ', 'a', '\u{301}', '\u{34f}', '\u{180e}', '\u{2060}'],
    );
    let q = query(&["Primary", "Fallback"], 400.);
    for ch in [
        '\u{34f}',
        '\u{180e}',
        '\u{2060}',
        '\u{200c}',
        '\u{fe0f}',
        '\u{e0100}',
    ] {
        for text in [
            ch.to_string(),
            format!("a{ch}"),
            format!(" {ch}"),
            format!("a{}", ch.to_string().repeat(2048)),
        ] {
            for _ in 0..2 {
                assert_eq!(
                    fonts.match_cluster(&q, &text).unwrap().id,
                    primary,
                    "{ch:?}"
                );
            }
        }
    }
    assert_ne!(fonts.match_cluster(&q, "a\u{301}").unwrap().id, primary);
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
    assert_eq!(fonts.caches().matches.len(), 1);
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
    assert!(unbounded.caches().matches.is_empty());
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
fn ch_advance_matches_shaped_zero_at_exact_fit_width() {
    let fonts = no_system();
    let id = fonts
        .register_face(test_font("Exact", &['0'], 1000), 0, descriptor("Exact"))
        .unwrap();
    let query = query(&["Exact"], 400.);
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 16.0,
            font_families: query.families.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "0",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.runs[0].font, id);
    let shaped = paragraph.data.glyphs.advance[0].to_f32();
    assert_eq!(shaped, 16.0);

    let unit = fonts.resolve_ch(&query, 16.0);
    assert_eq!(unit.id, Some(id));
    assert_eq!(unit.advance, shaped);
    assert_eq!(fonts.resolve_ch(&query, 16.0), unit);
    assert_eq!(fonts.resolve_unit_uncached(&query, 16.0, '0', 0.5), unit);
    let ten_ch = ChLength {
        query,
        size: 16.0,
        factor: 10.0,
    }
    .resolve(&fonts)
    .unwrap();
    assert_eq!(ten_ch.advance, 160.0);
}

#[test]
fn ch_advance_matches_shaping_at_intermediate_hvar_axis() {
    let base = test_font("Variable Zero", &['0'], 1000);
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(base[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(base[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(base[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            base[at..at + 4].try_into().unwrap(),
            base[offset..offset + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for value in [1u16, 0, 16, 2, 1, 20, 0, 8] {
        fvar.extend(value.to_be_bytes());
    }
    fvar.extend(b"wght");
    for value in [400i32, 400, 900] {
        fvar.extend((value << 16).to_be_bytes());
    }
    fvar.extend([0, 0, 1, 0]);
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
    for value in [1u16, 1, 0, 16384, 16384, 2, 1, 1, 0] {
        hvar.extend(value.to_be_bytes());
    }
    for _ in 0..2 {
        hvar.extend(1i16.to_be_bytes());
    }
    tables.push((*b"fvar", fvar));
    tables.push((*b"HVAR", hvar));
    tables.sort_by_key(|t| t.0);
    let fonts = no_system();
    let mut desc = descriptor("Variable Zero");
    desc.weight = (400., 900.);
    let id = fonts
        .register_face(sfnt::build_sfnt(&tables), 0, desc)
        .unwrap();
    let query = query(&["Variable Zero"], 650.);
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 16.,
            font_families: query.families.clone(),
            font_weight: 650.,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "0",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.runs[0].font, id);
    let shaped = paragraph.data.glyphs.advance[0].to_f32();
    assert_eq!(shaped, 16.015625);
    let unit = fonts.resolve_ch(&query, 16.);
    assert_eq!(unit.id, Some(id));
    assert_eq!(unit.advance, shaped);
    assert_eq!(fonts.resolve_ch(&query, 16.), unit);
    assert_eq!(fonts.resolve_unit_uncached(&query, 16., '0', 0.5), unit);
    for weight in [400., 650., 900.] {
        let varied = FontQuery {
            weight,
            ..query.clone()
        };
        assert_eq!(
            fonts.resolve_ch(&varied, 16.),
            fonts.resolve_unit_uncached(&varied, 16., '0', 0.5),
            "weight {weight}"
        );
    }
}

#[test]
fn unit_cache_tracks_script_language_fallback_and_generic_changes() {
    let fonts = no_system();
    let ja = fonts
        .register_face(
            test_font("Japanese", &['水'], 600),
            0,
            descriptor("Japanese"),
        )
        .unwrap();
    let zh = fonts
        .register_face(test_font("Chinese", &['水'], 800), 0, descriptor("Chinese"))
        .unwrap();
    fonts.set_fallback_families(*b"Hani", Some("ja".into()), vec!["Japanese".into()]);
    fonts.set_fallback_families(*b"Hani", Some("zh".into()), vec!["Chinese".into()]);
    let query = FontQuery {
        families: vec![],
        script: *b"Hani",
        language: Some("ja".into()),
        ..Default::default()
    };
    let ja_unit = fonts.resolve_ic(&query, 16.);
    assert_eq!(ja_unit.id, Some(ja));
    assert_eq!(fonts.resolve_ic(&query, 16.), ja_unit);
    let zh_query = FontQuery {
        language: Some("zh".into()),
        ..query.clone()
    };
    let zh_unit = fonts.resolve_ic(&zh_query, 16.);
    assert_eq!(zh_unit.id, Some(zh));
    assert_ne!(ja_unit.advance, zh_unit.advance);
    assert_eq!(fonts.resolve_ic(&zh_query, 16.), zh_unit);
    let latin_query = FontQuery {
        script: *b"Latn",
        ..query.clone()
    };
    assert_eq!(
        fonts.resolve_ic(&latin_query, 16.),
        fonts.resolve_unit_uncached(&latin_query, 16., '水', 1.)
    );
    fonts.set_fallback_families(*b"Hani", Some("ja".into()), vec!["Chinese".into()]);
    assert_eq!(fonts.resolve_ic(&query, 16.), zh_unit);
    assert_eq!(fonts.resolve_unit_uncached(&query, 16., '水', 1.), zh_unit);

    let generic = FontQuery::default();
    let _before_generic = fonts.resolve_ic(&generic, 16.);
    fonts.set_generic_families(
        crate::style::GenericFamily::SansSerif,
        vec!["Japanese".into()],
    );
    assert_eq!(fonts.resolve_ic(&generic, 16.).id, Some(ja));
    fonts.set_generic_families(
        crate::style::GenericFamily::SansSerif,
        vec!["Chinese".into()],
    );
    assert_eq!(fonts.resolve_ic(&generic, 16.).id, Some(zh));
}

fn shaper_test_advance(data: &peniko::FontData, shared: &harfrust::ShaperData) -> i32 {
    let font = harfrust::FontRef::from_index(data.data.as_ref(), data.index).unwrap();
    let shaper = shared.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str("a");
    buffer.set_script(harfrust::script::LATIN);
    buffer.set_direction(harfrust::Direction::LeftToRight);
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    assert_eq!(shaped.glyph_infos().len(), 1);
    assert_eq!(shaped.glyph_infos()[0].glyph_id, 1);
    assert_eq!(shaped.glyph_infos()[0].cluster, 0);
    shaped.glyph_positions()[0].x_advance
}

#[test]
fn recent_shaper_hit_inspects_one_actual_entry() {
    let fonts = no_system();
    let mut ids = Vec::new();
    for n in 0..32 {
        let id = fonts
            .register(test_font(&format!("Recent{n}"), &['a'], 500 + n))
            .unwrap();
        fonts.shaper_data(id).unwrap();
        ids.push(id);
    }
    let id = ids[31];
    let data = fonts.font_data(id).unwrap();
    let last = fonts.shaper_data(id).unwrap();
    assert_eq!(fonts.caches().shapers.len(), 32);
    SHAPER_SEARCH_COMPARISONS.with(|count| count.set(0));
    let hit = fonts.shaper_data(id).unwrap();
    let inspected = SHAPER_SEARCH_COMPARISONS.with(|count| count.get());
    assert!(Arc::ptr_eq(&last, &hit));
    assert_eq!(shaper_test_advance(&data, &hit), 531);
    assert_eq!(
        inspected, 1,
        "an MRU hit must inspect one actual shaper entry"
    );
}

#[test]
fn shaper_hit_promotes_lru_and_retained_handles_survive_layer_drop() {
    let fonts = FontCollection::with_options(
        &Limits {
            max_shaper_cache_entries: Some(2),
            ..Default::default()
        },
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let a = fonts.register(test_font("A", &['a'], 500)).unwrap();
    let b = fonts.register(test_font("B", &['a'], 600)).unwrap();
    let c = fonts.register(test_font("C", &['a'], 700)).unwrap();
    let d = fonts.register(test_font("D", &['a'], 800)).unwrap();
    let data = fonts.font_data(a).unwrap();
    let first = fonts.shaper_data(a).unwrap();
    let victim = Arc::downgrade(&fonts.shaper_data(b).unwrap());
    assert!(victim.upgrade().is_some());
    assert!(Arc::ptr_eq(&first, &fonts.shaper_data(a).unwrap()));
    fonts.shaper_data(c).unwrap();
    assert!(victim.upgrade().is_none());
    assert_eq!(
        fonts
            .caches()
            .shapers
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        [a.index(), c.index()]
    );
    fonts.shaper_data(d).unwrap();
    assert_eq!(Arc::strong_count(&first), 1);
    assert_eq!(shaper_test_advance(&data, &first), 500);
    drop(fonts);
    assert_eq!(shaper_test_advance(&data, &first), 500);
}

#[test]
fn shaper_cache_qualifies_shared_and_document_indices_and_zero_retention() {
    let shared = no_system();
    let shared_id = shared.register(test_font("Shared", &['a'], 500)).unwrap();
    let original = shared.shaper_data(shared_id).unwrap();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    doc.register(test_font("Unused", &['a'], 700)).unwrap();
    let own = doc.register(test_font("Own", &['a'], 900)).unwrap();
    assert_eq!(shared_id.index(), own.index());
    assert!(Arc::ptr_eq(&original, &doc.shaper_data(shared_id).unwrap()));
    let own_handle = doc.shaper_data(own).unwrap();
    assert!(!Arc::ptr_eq(&original, &own_handle));
    assert_eq!(
        shaper_test_advance(&doc.font_data(own).unwrap(), &own_handle),
        900
    );
    assert_eq!(
        shaper_test_advance(&doc.font_data(shared_id).unwrap(), &original),
        500
    );
    let zero = FontCollection::with_options(
        &Limits {
            max_shaper_cache_entries: Some(0),
            ..Default::default()
        },
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = zero.register(test_font("Zero", &['a'], 550)).unwrap();
    let a = zero.shaper_data(id).unwrap();
    let b = zero.shaper_data(id).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(zero.caches().shapers.is_empty());
    assert_eq!(shaper_test_advance(&zero.font_data(id).unwrap(), &a), 550);
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
    assert_eq!(fonts.caches().shapers.len(), 1);
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
    assert!(empty.caches().shapers.is_empty());
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

#[test]
fn style_selection_precedes_character_coverage() {
    let fonts = no_system();
    add_face(&fonts, "First", (400., 400.), &['a']);
    add_face(&fonts, "First", (700., 700.), &['b']);
    let fallback = add_face(&fonts, "Second", (400., 400.), &['b']);
    assert_eq!(
        fonts
            .match_cluster(&query(&["First", "Second"], 400.), "b")
            .unwrap()
            .id,
        fallback
    );
}

#[test]
fn css_oblique_maps_to_negative_slnt_and_respects_descriptor() {
    let plain = test_font("Variable", &['a'], 600);
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(plain[4..6].try_into().unwrap()) as usize {
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
    fvar.extend_from_slice(b"slnt");
    for value in [-20i32, 0, 0] {
        fvar.extend_from_slice(&(value << 16).to_be_bytes());
    }
    fvar.extend_from_slice(&[0, 0, 1, 0]);
    tables.push((*b"fvar", fvar));
    for (style, expected) in [(FontStyle::Oblique(10.), -10.), (FontStyle::Normal, 0.)] {
        let fonts = no_system();
        let mut desc = descriptor("Web");
        desc.style = style;
        fonts
            .register_face(sfnt::build_sfnt(&tables), 0, desc)
            .unwrap();
        let mut q = query(&["Web"], 400.);
        q.style = FontStyle::Oblique(15.);
        let found = fonts.match_cluster(&q, "a").unwrap();
        assert_eq!(
            found.variations,
            vec![crate::style::FontVariation {
                tag: *b"slnt",
                value: expected
            }]
        );
        assert_eq!(found.skew, None);
    }
}

#[test]
fn local_aliases_share_reloaded_source_bytes_within_layer() {
    let bytes = test_font("Native", &['a'], 600);
    let limits = Limits {
        max_layer_blob_bytes: Some(bytes.len() as u64),
        ..Default::default()
    };
    let fonts = no_system();
    let doc = FontCollection::for_document(&fonts, &limits);
    let source = fontique::SourceId::new();
    let first = doc
        .register_source_blob(
            Blob::from(bytes.clone()),
            0,
            descriptor("One"),
            Some(source),
        )
        .unwrap();
    let second = doc
        .register_source_blob(Blob::from(bytes), 0, descriptor("Two"), Some(source))
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        doc.font_data(first).unwrap().data.id(),
        doc.font_data(second).unwrap().data.id()
    );
    assert_eq!(doc.state().retained_sources.len(), 1);
}

#[test]
fn default_ignorables_emit_no_glyphs_without_losing_base_or_space() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let chars = [' ', 'X', '\u{34f}', '\u{180e}', '\u{2060}'];
    let id = fonts
        .register_face(
            test_font("Primary", &chars, 600),
            0,
            crate::font::FontFaceDescriptor {
                family: "Primary".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 10.0,
            font_families: vec![crate::style::FontFamily::Named("Primary".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    for ch in ['\u{34f}', '\u{180e}', '\u{2060}'] {
        for text in [
            format!("XX{ch}XX"),
            format!("XX {ch}XX"),
            format!("{ch}XX{ch}"),
            ch.to_string(),
        ] {
            let mut builder = crate::ParagraphBuilder::new(&style, &limits);
            builder.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                &text,
            );
            let p = builder
                .build(&mut crate::LayoutContext::new(), &fonts)
                .unwrap();
            assert_eq!(p.text(), text);
            let expected: Vec<_> = text
                .chars()
                .filter_map(|c| match c {
                    ' ' => Some(1),
                    'X' => Some(2),
                    _ => None,
                })
                .collect();
            assert_eq!(p.data.glyphs.id, expected, "{text:?}");
            assert!(p.data.glyphs.advance.iter().all(|a| a.to_f32() == 6.0));
            assert!(p.data.runs.iter().all(|run| run.font == id));
        }
    }
}

#[test]
fn default_ignorables_retain_line_break_and_grapheme_semantics() {
    let fonts = no_system();
    fonts
        .register_face(
            test_font("Primary", &[' ', 'X'], 600),
            0,
            descriptor("Primary"),
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 10.0,
            font_families: vec![crate::style::FontFamily::Named("Primary".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    for (text, width, cut) in [
        ("XX\u{2060}XX", 12.0, 5),
        ("XX \u{180e}XX", 18.0, 6),
        ("XX \u{34f}XX", 18.0, 5),
    ] {
        let unspaced = text.replace(' ', "");
        let mut normal = crate::ParagraphBuilder::new(&style, &Limits::default());
        normal.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &unspaced,
        );
        let p = normal
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        assert_eq!(p.text(), unspaced);
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            width,
            &crate::AtomicSizes::EMPTY,
        );
        assert_eq!(
            lines.len(),
            1,
            "{text:?}: controls must retain standard no-break behavior"
        );
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = seen.clone();
        let mut anywhere = crate::ParagraphBuilder::new(&style, &Limits::default());
        anywhere.with_line_break_override(move |context| {
            observed
                .lock()
                .unwrap()
                .push((context.text.to_owned(), context.offset));
            crate::LineBreakOverride::Allow
        });
        anywhere.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            text,
        );
        let p = anywhere
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            width,
            &crate::AtomicSizes::EMPTY,
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text_range())
                .collect::<Vec<_>>(),
            vec![0..cut, cut..text.len()],
            "{text:?}"
        );
        let seen = seen.lock().unwrap();
        assert!(seen.iter().all(|(logical, _)| logical == text));
        if text.contains('\u{34f}') {
            assert!(
                seen.iter().all(|(_, at)| *at != 3),
                "space + CGJ is a single grapheme"
            );
        }
    }
}

#[test]
fn default_ignorables_keep_glyph_free_dom_items_and_grapheme_limits() {
    let fonts = no_system();
    fonts
        .register_face(test_font("Primary", &['X'], 600), 0, descriptor("Primary"))
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 10.0,
            font_families: vec![crate::style::FontFamily::Named("Primary".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    for text in ["\u{2060}\u{180e}", "\u{2060}XX", "XX\u{180e}"] {
        let mut builder = crate::ParagraphBuilder::new(&style, &Limits::default());
        for (offset, ch) in text.char_indices() {
            builder.push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(offset as u64 + 1),
                    offset: 0,
                },
                &ch.to_string(),
            );
        }
        let p = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            1000.0,
            &crate::AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1, "{text:?}");
        assert_eq!(lines[0].text_range(), 0..text.len());
        let clusters = lines[0]
            .fragments()
            .filter_map(|fragment| match fragment {
                crate::Fragment::GlyphRun(run) => Some(
                    run.clusters()
                        .map(|cluster| cluster.text_range)
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(
            clusters
                .iter()
                .map(|range| range.end - range.start)
                .sum::<usize>(),
            text.len()
        );
        if text.starts_with('\u{2060}') && text.contains('X') {
            let visible = lines[0]
                .fragments()
                .filter_map(|fragment| match fragment {
                    crate::Fragment::GlyphRun(run) if run.glyphs().len() != 0 => Some(run.node()),
                    _ => None,
                })
                .next()
                .unwrap();
            assert_eq!(visible, Some(crate::node::NodeId(4)));
        }
        let mut constraint = crate::LineConstraint::new(1000.0);
        constraint.max_graphemes = Some(1);
        let crate::LineResult::Line(first) = p.next_line(
            &mut crate::LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &constraint,
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!("expected first grapheme");
        };
        assert_eq!(
            first.text_range(),
            0..text.chars().next().unwrap().len_utf8()
        );
    }
}

#[test]
fn default_ignorables_keep_standalone_soft_hyphen_shaping() {
    for budget in [None, Some(2)] {
        let limits = Limits {
            max_shaping_run_bytes: budget,
            ..Default::default()
        };
        let fonts = no_system();
        fonts
            .register_face(
                test_font("Primary", &['-', 'X'], 600),
                0,
                descriptor("Primary"),
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 10.0,
                font_families: vec![crate::style::FontFamily::Named("Primary".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut shy_style = style.root.clone();
        shy_style.font_size = 11.0;
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "XX",
        );
        builder
            .open_inline(crate::node::NodeId(2), &shy_style, Default::default())
            .push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(2),
                },
                "\u{ad}",
            )
            .close_inline();
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(3),
            },
            "XX",
        );
        let p = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        assert_eq!(p.text(), "XX\u{ad}XX");
        assert_eq!(p.data.glyphs.len(), 4);
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            20.0,
            &crate::AtomicSizes::EMPTY,
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text_range())
                .collect::<Vec<_>>(),
            vec![0..4, 4..6]
        );
        let ids = lines[0]
            .fragments()
            .filter_map(|fragment| match fragment {
                crate::Fragment::GlyphRun(run) => Some(run.glyphs()),
                _ => None,
            })
            .flatten()
            .map(|glyph| glyph.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, [2, 2, 1], "budget={budget:?}: generated hyphen");
    }
}

#[test]
fn default_ignorables_allow_justified_hyphens_without_shared_glyphs() {
    let fonts = no_system();
    fonts
        .register_face(test_font("Primary", &['-'], 600), 0, descriptor("Primary"))
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 10.0,
            font_families: vec![crate::style::FontFamily::Named("Primary".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &Limits::default());
    builder
        .push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "\u{ad}",
        )
        .push_atomic(crate::node::NodeId(2), &style.root, Default::default());
    let p = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(p.data.glyphs.len(), 0);
    let mut atomics = crate::AtomicSizes::new();
    atomics.insert(
        crate::node::NodeId(2),
        crate::AtomicSize {
            inline_size: 20.0,
            block_size: 10.0,
            ..Default::default()
        },
    );
    let options = crate::style::LineOptions {
        text_align: crate::style::TextAlign::Justify,
        ..Default::default()
    };
    let lines = p.break_all(&mut crate::LayoutContext::new(), &options, 6.0, &atomics);
    let glyphs = lines[0]
        .fragments()
        .filter_map(|fragment| match fragment {
            crate::Fragment::GlyphRun(run) => Some(run.glyphs()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(glyphs.len(), 1);
    assert_eq!(glyphs[0].id, 1);
    assert_eq!(glyphs[0].advance, 6.0);
}
