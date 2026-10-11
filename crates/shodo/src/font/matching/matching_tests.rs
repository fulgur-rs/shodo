use super::*;
use crate::font::FontOptions;
use std::cell::Cell;

std::thread_local! {
    static VARIATION_CLONES: Cell<usize> = const { Cell::new(0) };
    static INFO_READS: Cell<usize> = const { Cell::new(0) };
    static FONT_READS: Cell<usize> = const { Cell::new(0) };
    static KEY_COMPARISONS: Cell<usize> = const { Cell::new(0) };
    static SHARED_LOOKUPS: Cell<usize> = const { Cell::new(0) };
static REGISTERED_FACE_VISITS: Cell<usize> = const { Cell::new(0) };
    static NATIVE_FACE_ID_LOOKUPS: Cell<usize> = const { Cell::new(0) };
    static PREPARED_RANKS: Cell<usize> = const { Cell::new(0) };
    static RANK_SCANS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_prepared_rank() {
    PREPARED_RANKS.with(|n| n.set(n.get() + 1));
}

pub(super) fn record_rank() {
    RANK_SCANS.with(|scans| scans.set(scans.get() + 1));
}

pub(super) fn record_shared_lookup() {
    SHARED_LOOKUPS.with(|n| n.set(n.get() + 1));
}

pub(super) fn record_key_comparison() {
    KEY_COMPARISONS.with(|n| n.set(n.get() + 1));
}

pub(super) fn record_registered_face_visit() {
    REGISTERED_FACE_VISITS.with(|n| n.set(n.get() + 1));
}

pub(super) fn record_native_face_id_lookup() {
    NATIVE_FACE_ID_LOOKUPS.with(|n| n.set(n.get() + 1));
}

fn fonts_without_match_cache() -> FontCollection {
    FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 0,
            ..Default::default()
        },
    )
}

#[test]
fn cache_hit_compares_few_keys_regardless_of_cache_size() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 64,
            ..Default::default()
        },
    );
    fonts
        .register(super::super::browser_tests::test_font("Web", &['a'], 600))
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    let clusters: Vec<String> = (0..64)
        .map(|i| format!("{}", char::from(b'A' + i)))
        .collect();
    for c in &clusters {
        fonts.match_cluster(&query, c);
    }
    assert_eq!(fonts.caches().matches.len(), 64);
    // The newest entry is the worst case for a front-to-back scan.
    KEY_COMPARISONS.with(|n| n.set(0));
    fonts.match_cluster(&query, clusters.last().unwrap());
    let compared = KEY_COMPARISONS.with(Cell::get);
    assert!(
        compared <= 2,
        "{compared} key comparisons for one cache hit"
    );
    assert_eq!(fonts.caches().matches.len(), 64);
}

#[test]
fn scripted_match_equals_cloning_the_query_and_reuses_the_cache() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 8,
            ..Default::default()
        },
    );
    fonts
        .register(super::super::browser_tests::test_font("Web", &['a'], 600))
        .unwrap();
    let base = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    }
    .normalized();
    for script in [*b"Latn", *b"Hani", *b"Arab"] {
        let mut cloned = base.clone();
        cloned.script = script;
        for cluster in ["a", "b"] {
            assert_eq!(
                fonts.match_scripted(&base, script, cluster).as_deref(),
                fonts.match_cluster(&cloned, cluster).as_ref(),
                "{script:?} {cluster:?}"
            );
        }
    }
    // Six distinct (script, cluster) keys are cached once each, and the
    // equivalent cloned-query lookups above hit the same entries.
    assert_eq!(fonts.caches().matches.len(), 6);
}

#[test]
fn cache_evicts_least_recently_used_entry_first() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 2,
            ..Default::default()
        },
    );
    fonts
        .register(super::super::browser_tests::test_font("Web", &['a'], 600))
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    // Observe the shared cache: skip this thread's front cache every time.
    let shared_match = |c: &str| {
        thread_cache::clear();
        fonts.match_cluster(&query, c);
    };
    let hit_cost = |c: &str| {
        KEY_COMPARISONS.with(|n| n.set(0));
        shared_match(c);
        KEY_COMPARISONS.with(Cell::get)
    };
    shared_match("A");
    shared_match("B");
    shared_match("A"); // A is now most recent
    shared_match("C"); // evicts B, not A
    assert!(
        hit_cost("A") > 0,
        "A must still be cached: a hit compares its key"
    );
    // A miss on B compares no cached key against an equal entry; re-inserting
    // it must not have kept a stale copy.
    assert_eq!(fonts.caches().matches.len(), 2);
    shared_match("B"); // evicts C (A was just touched)
    let a_after = hit_cost("A");
    assert!(a_after > 0);
    assert_eq!(fonts.caches().matches.len(), 2);
}

pub(super) fn record_info_read() {
    INFO_READS.with(|reads| reads.set(reads.get() + 1));
}

pub(super) fn record_font_read() {
    FONT_READS.with(|reads| reads.set(reads.get() + 1));
}

#[test]
fn registered_matching_reuses_metadata_for_uncached_clusters() {
    check_uncached_reads(1, 1);
}

#[test]
fn registered_matching_does_not_reparse_color_while_sorting() {
    check_uncached_reads(3, 3);
}

#[test]
fn named_registered_candidates_use_ascii_case_folded_indices_in_registration_order() {
    let fonts = fonts_without_match_cache();
    let bytes = super::super::browser_tests::test_font("Internal", &['a'], 600);
    let register = |index| {
        let family = match index {
            7 => "Web",
            64 => "wEb",
            127 => "WEB",
            3 => "Web-Ä",
            _ => "Noise",
        };
        fonts
            .register_face(
                bytes.clone(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .index
    };
    let mut expected = Vec::new();
    for index in 0..64 {
        let slot = register(index);
        if index == 7 {
            expected.push(slot);
        }
    }

    REGISTERED_FACE_VISITS.with(|n| n.set(0));
    let actual = fonts.registered_candidates(Some("WEB"));
    assert_eq!(
        actual
            .iter()
            .map(|candidate| candidate.id.index)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        REGISTERED_FACE_VISITS.with(Cell::get),
        expected.len(),
        "named lookup should inspect only matching family slots"
    );

    for index in 64..128 {
        let slot = register(index);
        if [64, 127].contains(&index) {
            expected.push(slot);
        }
    }
    REGISTERED_FACE_VISITS.with(|n| n.set(0));
    let actual = fonts.registered_candidates(Some("WEB"));
    assert_eq!(
        actual
            .iter()
            .map(|candidate| candidate.id.index)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        REGISTERED_FACE_VISITS.with(Cell::get),
        expected.len(),
        "new registrations should append to the family index"
    );

    REGISTERED_FACE_VISITS.with(|n| n.set(0));
    assert!(fonts.registered_candidates(Some("web-ä")).is_empty());
    assert_eq!(
        REGISTERED_FACE_VISITS.with(Cell::get),
        0,
        "family matching folds ASCII only"
    );
}

#[test]
fn native_candidate_face_ids_use_the_identity_index() {
    let fonts = fonts_without_match_cache();
    let bytes = super::super::browser_tests::test_font("Shared Native", &['a'], 600);
    let registered: Vec<_> = (0..64)
        .map(|_| fonts.register(bytes.clone()).unwrap())
        .collect();

    REGISTERED_FACE_VISITS.with(|n| n.set(0));
    assert!(
        fonts
            .registered_candidates(Some("Shared Native"))
            .is_empty()
    );
    assert_eq!(REGISTERED_FACE_VISITS.with(Cell::get), 0);

    NATIVE_FACE_ID_LOOKUPS.with(|n| n.set(0));
    let candidates = fonts.native_candidates("Shared Native");

    assert_eq!(candidates.len(), registered.len());
    let mut actual: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate.id.index)
        .collect();
    let mut expected: Vec<_> = registered.iter().map(|id| id.index).collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert_eq!(
        NATIVE_FACE_ID_LOOKUPS.with(Cell::get),
        candidates.len(),
        "each catalog candidate should perform one face identity lookup"
    );
}

#[test]
fn native_materialization_rechecks_the_identity_index_after_candidate_creation() {
    let fonts = fonts_without_match_cache();
    let bytes = super::super::browser_tests::test_font("Shared Native", &['a'], 600);
    let registered: Vec<_> = (0..64)
        .map(|_| fonts.register(bytes.clone()).unwrap())
        .collect();
    let mut candidate = fonts
        .native_candidates("Shared Native")
        .into_iter()
        .max_by_key(|candidate| candidate.id.index)
        .unwrap();
    let expected = *registered.iter().max_by_key(|id| id.index).unwrap();
    assert_eq!(candidate.id, expected);

    // Simulate another query materializing the same catalog face after this
    // candidate was created but before best_match acquires the state lock.
    candidate.id.index = u32::MAX;
    let face_count = fonts.state().faces.len();
    NATIVE_FACE_ID_LOOKUPS.with(|n| n.set(0));

    let matched = fonts
        .best_match(
            vec![candidate],
            &FontQuery::default(),
            &mut FontCluster::new("a"),
            true,
        )
        .unwrap();

    assert_eq!(matched.id, expected);
    assert_eq!(fonts.state().faces.len(), face_count);
    assert_eq!(NATIVE_FACE_ID_LOOKUPS.with(Cell::get), 1);
}

#[test]
fn document_family_indices_stay_local_and_parent_matching_is_preserved() {
    let shared = fonts_without_match_cache();
    let shared_id = shared
        .register_face(
            super::super::browser_tests::test_font("Root Internal", &['a'], 600),
            0,
            FontFaceDescriptor {
                family: "Shared Family".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let document = FontCollection::for_document(&shared, &Limits::default());
    let document_id = document
        .register_face(
            super::super::browser_tests::test_font("Document Internal", &['b'], 600),
            0,
            FontFaceDescriptor {
                family: "shared family".into(),
                ..Default::default()
            },
        )
        .unwrap();

    let local = document.registered_candidates(Some("SHARED FAMILY"));
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].id, document_id);
    let query = FontQuery {
        families: vec![crate::style::FontFamily::Named("Shared Family".into())],
        ..Default::default()
    };
    assert_eq!(document.match_cluster(&query, "a").unwrap().id, shared_id);
    assert_eq!(document.match_cluster(&query, "b").unwrap().id, document_id);
}

#[test]
fn bulk_registered_named_and_nameless_faces_keep_cached_metadata() {
    for named in [true, false] {
        let mut bytes = super::super::browser_tests::test_font("Internal", &['a', 'b'], 600);
        if !named {
            let font = FontRef::new(&bytes).unwrap();
            let tables: Vec<_> = font
                .table_directory()
                .table_records()
                .iter()
                .filter(|record| record.tag() != skrifa::raw::types::Tag::new(b"name"))
                .map(|record| {
                    (
                        record.tag().to_be_bytes(),
                        font.table_data(record.tag()).unwrap().as_bytes().to_vec(),
                    )
                })
                .collect();
            bytes = super::super::sfnt::build_sfnt(&tables);
        }
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: 0,
                ..Default::default()
            },
        );
        let id = fonts.register(bytes).unwrap();
        // Use the registered last-resort path, including nameless fonts
        // that fontique cannot expose as a named family.
        fonts.set_fallback_families(*b"Latn", None, Vec::new());
        let query = FontQuery {
            families: Vec::new(),
            ..Default::default()
        };
        for cluster in ["a", "b"] {
            INFO_READS.with(|reads| reads.set(0));
            FONT_READS.with(|reads| reads.set(0));
            assert_eq!(fonts.match_cluster(&query, cluster).unwrap().id, id);
            assert_eq!(
                (INFO_READS.with(Cell::get), FONT_READS.with(Cell::get)),
                (0, 1),
                "named={named}, cluster={cluster}"
            );
        }
    }
}

fn check_uncached_reads(face_count: usize, expected_reads: usize) {
    // Reintroducing candidate metadata parsing or color parsing in the
    // sort comparator must exceed one font read per registered candidate.
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: 0,
            ..Default::default()
        },
    );
    let mut expected = None;
    for _ in 0..face_count {
        expected = Some(
            fonts
                .register_face(
                    super::super::browser_tests::test_font("Internal", &['a', 'b', 'c'], 600),
                    0,
                    FontFaceDescriptor {
                        family: "Web".into(),
                        ..Default::default()
                    },
                )
                .unwrap(),
        );
    }
    let query = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    for cluster in ["a", "b", "c"] {
        INFO_READS.with(|reads| reads.set(0));
        FONT_READS.with(|reads| reads.set(0));
        let found = fonts.match_cluster(&query, cluster).unwrap();
        assert_eq!(Some(found.id), expected, "latest equal-ranked face wins");
        assert_eq!(
            (INFO_READS.with(Cell::get), FONT_READS.with(Cell::get)),
            (0, expected_reads),
            "metadata is reused and coverage/color share one real font read for {face_count} faces/{cluster}"
        );
    }
}

pub(super) fn record_variation_clone() {
    VARIATION_CLONES.with(|count| count.set(count.get() + 1));
}

#[test]
fn best_candidate_costs_at_most_one_rank_per_surviving_face() {
    // Ordering every coverage survivor and keeping only the head pays for
    // comparisons whose results are discarded. Selecting the head directly
    // must stay within one rank computation per surviving candidate.
    for faces in [8usize, 32, 128] {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: 0,
                ..Default::default()
            },
        );
        let mut latest = None;
        for _ in 0..faces {
            latest = Some(
                fonts
                    .register_face(
                        super::super::browser_tests::test_font("Internal", &['a', 'b', 'c'], 600),
                        0,
                        FontFaceDescriptor {
                            family: "Web".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap(),
            );
        }
        let query = FontQuery {
            families: vec![FontFamily::Named("Web".into())],
            ..Default::default()
        };
        RANK_SCANS.with(|scans| scans.set(0));
        let found = fonts.match_cluster(&query, "a").unwrap();
        // Equal ranks keep the most recently registered face.
        assert_eq!(Some(found.id), latest, "{faces} equal-ranked faces");
        let scanned = RANK_SCANS.with(Cell::get);
        assert!(
            scanned <= faces,
            "{faces} equal-ranked faces ranked {scanned} candidates"
        );
    }
}

fn multi_axis_font() -> Vec<u8> {
    let bytes = super::super::browser_tests::test_font("Axes", &['a', 'b'], 400);
    let font = FontRef::new(&bytes).unwrap();
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
    for field in [1u16, 0, 16, 2, 5, 20, 0, 24] {
        fvar.extend_from_slice(&field.to_be_bytes());
    }
    for (tag, values) in [
        (*b"wght", [100i32, 400, 900]),
        (*b"wdth", [50, 100, 150]),
        (*b"slnt", [-20, 0, 0]),
        (*b"ital", [0, 0, 1]),
        (*b"opsz", [8, 14, 144]),
    ] {
        fvar.extend_from_slice(&tag);
        for value in values {
            fvar.extend_from_slice(&(value << 16).to_be_bytes());
        }
        fvar.extend_from_slice(&[0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    super::super::sfnt::build_sfnt(&tables)
}

fn axes_fonts(cap: usize, style: FontStyle) -> (FontCollection, FontQuery) {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            multi_axis_font(),
            0,
            FontFaceDescriptor {
                family: "Axes".into(),
                weight: (100., 900.),
                width: (50., 150.),
                style,
                ..Default::default()
            },
        )
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Named("Axes".into())],
        weight: 650.,
        width: 80.,
        style,
        ..Default::default()
    }
    .normalized();
    (fonts, query)
}

#[test]
fn scripted_variable_warm_hit_does_not_clone_variation_storage() {
    let (fonts, query) = axes_fonts(8, FontStyle::Normal);
    let first = fonts.match_scripted(&query, *b"Latn", "a").unwrap();
    assert_eq!(
        first.variations,
        vec![
            FontVariation {
                tag: *b"wght",
                value: 650.
            },
            FontVariation {
                tag: *b"wdth",
                value: 80.
            },
            FontVariation {
                tag: *b"slnt",
                value: 0.
            },
            FontVariation {
                tag: *b"ital",
                value: 0.
            }
        ]
    );
    VARIATION_CLONES.with(|count| count.set(0));
    let second = fonts.match_scripted(&query, *b"Latn", "a").unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.variations[0].value, 650.);
    assert_eq!(
        VARIATION_CLONES.with(Cell::get),
        0,
        "warm internal hit cloned nonempty variations"
    );
}

#[test]
fn public_variable_match_owns_its_variations() {
    let (fonts, query) = axes_fonts(8, FontStyle::Normal);
    let mut owned = fonts.match_cluster(&query, "a").unwrap();
    owned.variations[0].value = 100.;
    owned.variations.clear();
    let fresh = fonts.match_cluster(&query, "a").unwrap();
    assert_eq!(
        fresh.variations[0],
        FontVariation {
            tag: *b"wght",
            value: 650.
        }
    );
    assert_eq!(fresh.variations.len(), 4);
}

#[test]
fn internal_variable_results_keep_clamps_style_and_cache_bounds() {
    for cap in [0, 1, 8] {
        for (style, slant, italic) in [
            (FontStyle::Normal, 0., 0.),
            (FontStyle::Oblique(30.), -20., 0.),
            (FontStyle::Italic, 0., 1.),
        ] {
            let (fonts, mut query) = axes_fonts(cap, style);
            query.weight = 1000.;
            query.width = 200.;
            for cluster in ["a", "b", "a", "\u{10ffff}", "\u{10ffff}"] {
                let found = fonts.match_scripted(&query, *b"Latn", cluster);
                if cluster == "\u{10ffff}" {
                    assert!(found.is_none());
                    continue;
                }
                let found = found.unwrap();
                assert_eq!(
                    found.variations,
                    vec![
                        FontVariation {
                            tag: *b"wght",
                            value: 900.
                        },
                        FontVariation {
                            tag: *b"wdth",
                            value: 150.
                        },
                        FontVariation {
                            tag: *b"slnt",
                            value: slant
                        },
                        FontVariation {
                            tag: *b"ital",
                            value: italic
                        }
                    ]
                );
                assert!(!found.embolden);
                assert_eq!(found.skew, None);
            }
            assert!(fonts.caches().matches.len() <= cap);
        }
    }
}

#[test]
fn old_variable_matches_survive_generation_invalidation() {
    for document in [false, true] {
        let (root, _) = axes_fonts(8, FontStyle::Normal);
        let second_id = root
            .register_face(
                multi_axis_font(),
                0,
                FontFaceDescriptor {
                    family: "Second".into(),
                    weight: (100., 900.),
                    width: (50., 150.),
                    ..Default::default()
                },
            )
            .unwrap();
        root.set_generic_families(GenericFamily::SansSerif, vec!["Axes".into()]);
        let fonts = if document {
            FontCollection::for_document(&root, &Limits::default())
        } else {
            root.clone()
        };
        let query = FontQuery {
            weight: 650.,
            width: 80.,
            ..Default::default()
        }
        .normalized();
        let old = fonts.match_scripted(&query, *b"Latn", "a").unwrap();
        root.set_generic_families(GenericFamily::SansSerif, vec!["Second".into()]);
        let new = fonts.match_scripted(&query, *b"Latn", "a").unwrap();
        assert_eq!(new.id, second_id);
        assert_ne!(new.id, old.id);
        assert_eq!(
            old.variations[0],
            FontVariation {
                tag: *b"wght",
                value: 650.
            }
        );
        assert_eq!(new.variations[0].value, 650.);
        assert_eq!(fonts.caches().matches.len(), 1);
    }
}

#[test]
fn distinct_clusters_reuse_registered_candidate_preparation() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut expected = None;
    for _ in 0..4 {
        expected = Some(
            fonts
                .register_face(
                    super::super::browser_tests::test_font("Internal", &['水', '日', '本'], 600),
                    0,
                    FontFaceDescriptor {
                        family: "Web".into(),
                        ..Default::default()
                    },
                )
                .unwrap(),
        );
    }
    let query = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    REGISTERED_FACE_VISITS.with(|n| n.set(0));
    PREPARED_RANKS.with(|n| n.set(0));
    FONT_READS.with(|n| n.set(0));
    for cluster in ["水", "日", "本"] {
        assert_eq!(
            Some(fonts.match_cluster(&query, cluster).unwrap().id),
            expected
        );
    }
    assert_eq!(
        REGISTERED_FACE_VISITS.with(Cell::get),
        4,
        "prepare the four faces once, including the initial miss"
    );
    assert_eq!(PREPARED_RANKS.with(Cell::get), 4);
    assert_eq!(
        FONT_READS.with(Cell::get),
        12,
        "coverage still checks each face for each distinct cluster"
    );
}

#[test]
fn prepared_registered_selection_matches_uncached_attributes_and_coverage() {
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, weight, ranges, bytes) in [
        ("Web", (400., 400.), vec![(97, 97)], multi_axis_font()),
        ("Web", (400., 400.), vec![(98, 98)], multi_axis_font()),
        ("Web", (700., 700.), vec![], multi_axis_font()),
        (
            "Fallback",
            (400., 400.),
            vec![],
            super::super::browser_tests::test_font("Fallback", &['a', 'b', '水'], 600),
        ),
    ] {
        fonts
            .register_face(
                bytes,
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    weight,
                    unicode_ranges: ranges,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    for style in [
        FontStyle::Normal,
        FontStyle::Italic,
        FontStyle::Oblique(-12.),
        FontStyle::Oblique(14.),
    ] {
        for weight in [100., 400., 450., 650., 900.] {
            for width in [50., 100., 150.] {
                for name in [Some("Web"), Some("Fallback"), None] {
                    let query = FontQuery {
                        style,
                        weight,
                        width,
                        ..Default::default()
                    };
                    for text in ["a", "b", "ab", "水", "a\u{fe0e}", "b\u{fe0f}", "z"] {
                        let old = fonts.best_match(
                            fonts.registered_candidates(name),
                            &query,
                            &mut FontCluster::new(text),
                            name.is_some(),
                        );
                        assert_eq!(
                            fonts.registered_match(name, &query, &mut FontCluster::new(text)),
                            old,
                            "{name:?}/{query:?}/{text}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn prepared_candidates_keep_cluster_presentation_and_query_synthesis_dynamic() {
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mono = super::super::browser_tests::test_font("Internal", &['☺', '😀'], 600);
    let font = FontRef::new(&mono).unwrap();
    let mut tables: Vec<_> = font
        .table_directory()
        .table_records()
        .iter()
        .map(|r| {
            (
                r.tag().to_be_bytes(),
                font.table_data(r.tag()).unwrap().as_bytes().to_vec(),
            )
        })
        .collect();
    tables.push((*b"COLR", vec![0; 14]));
    let color = super::super::sfnt::build_sfnt(&tables);
    let color_id = fonts
        .register_face(
            color,
            0,
            FontFaceDescriptor {
                family: "Emoji".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mono_id = fonts
        .register_face(
            mono,
            0,
            FontFaceDescriptor {
                family: "Emoji".into(),
                ..Default::default()
            },
        )
        .unwrap();
    PREPARED_RANKS.with(|n| n.set(0));
    for presentation in [
        FontPresentation::Auto,
        FontPresentation::Text,
        FontPresentation::Emoji,
    ] {
        for (text, auto_color) in [
            ("☺", false),
            ("☺\u{fe0e}", false),
            ("☺\u{fe0f}", true),
            ("😀", true),
        ] {
            for synth in [false, true] {
                let query = FontQuery {
                    families: vec![FontFamily::Named("Emoji".into())],
                    presentation,
                    weight: 650.,
                    style: FontStyle::Italic,
                    script: *b"Zyyy",
                    language: Some("ja".into()),
                    synthesis: FontSynthesis {
                        weight: synth,
                        style: synth,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let expected = match presentation {
                    FontPresentation::Auto => auto_color,
                    FontPresentation::Text => false,
                    FontPresentation::Emoji => true,
                };
                let found = fonts.match_cluster(&query, text).unwrap();
                assert_eq!(found.id, if expected { color_id } else { mono_id });
                assert_eq!(found.embolden, synth);
                assert_eq!(found.skew, if synth { Some(14.) } else { None });
            }
        }
    }
    assert_eq!(
        PREPARED_RANKS.with(Cell::get),
        2,
        "presentation/language/script/synthesis are not preparation keys"
    );
}

fn web_fonts(cap: usize) -> (FontCollection, FontQuery) {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    );
    fonts
        .register(super::super::browser_tests::test_font("Web", &['a'], 600))
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Named("Web".into())],
        ..Default::default()
    };
    (fonts, query)
}

#[test]
fn a_repeated_match_on_one_thread_skips_the_shared_cache() {
    let (fonts, query) = web_fonts(8);
    thread_cache::clear();
    let first = fonts.match_cluster(&query, "a");
    assert!(first.is_some());
    SHARED_LOOKUPS.with(|n| n.set(0));
    for _ in 0..4 {
        assert_eq!(fonts.match_cluster(&query, "a"), first);
    }
    assert_eq!(SHARED_LOOKUPS.with(Cell::get), 0);
    // Another thread starts cold and is served by the shared entry.
    let fonts_for_thread = fonts.clone();
    let query_for_thread = query.clone();
    let (other, lookups) = std::thread::spawn(move || {
        let found = fonts_for_thread.match_cluster(&query_for_thread, "a");
        (found, SHARED_LOOKUPS.with(Cell::get))
    })
    .join()
    .unwrap();
    assert_eq!(other, first);
    assert_eq!(lookups, 1);
}

#[test]
fn a_thread_cache_hit_shares_its_result_without_cloning_variations() {
    let (fonts, query) = web_fonts(8);
    thread_cache::clear();
    let base = query.normalized();
    let first = fonts.match_scripted(&base, *b"Latn", "a").unwrap();
    let again = fonts.match_scripted(&base, *b"Latn", "a").unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &again));
}

#[test]
fn the_thread_cache_follows_generation_changes() {
    let (fonts, _) = web_fonts(8);
    let other = fonts
        .register_face(
            super::super::browser_tests::test_font("Other", &['a'], 600),
            0,
            FontFaceDescriptor {
                family: "Other".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let query = FontQuery {
        families: vec![FontFamily::Generic(GenericFamily::Serif)],
        ..Default::default()
    };
    thread_cache::clear();
    fonts.set_generic_families(GenericFamily::Serif, vec!["Web".into()]);
    let web = fonts.match_cluster(&query, "a").unwrap().id;
    assert_ne!(web, other);
    fonts.set_generic_families(GenericFamily::Serif, vec!["Other".into()]);
    assert_eq!(fonts.match_cluster(&query, "a").unwrap().id, other);
}

#[test]
fn the_thread_cache_keeps_layers_apart() {
    let (shared, query) = web_fonts(8);
    let document = FontCollection::for_document(&shared, &Limits::default());
    let local = document
        .register_face(
            super::super::browser_tests::test_font("Web", &['a'], 600),
            0,
            FontFaceDescriptor {
                family: "Web".into(),
                ..Default::default()
            },
        )
        .unwrap();
    thread_cache::clear();
    let from_shared = shared.match_cluster(&query, "a").unwrap().id;
    let from_document = document.match_cluster(&query, "a").unwrap().id;
    assert_eq!(from_document, local);
    assert_ne!(from_shared, local);
    assert_eq!(shared.match_cluster(&query, "a").unwrap().id, from_shared);
}

#[test]
fn concurrent_matches_agree_with_sequential_ones() {
    let (fonts, query) = web_fonts(16);
    let clusters: Vec<String> = ('a'..='z').map(String::from).collect();
    let expected: Vec<_> = clusters
        .iter()
        .map(|c| fonts.match_cluster(&query, c))
        .collect();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let fonts = fonts.clone();
            let query = query.clone();
            let clusters = clusters.clone();
            std::thread::spawn(move || {
                (0..8)
                    .flat_map(|_| clusters.iter().map(|c| fonts.match_cluster(&query, c)))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for thread in threads {
        let found = thread.join().unwrap();
        for chunk in found.chunks(clusters.len()) {
            assert_eq!(chunk, expected.as_slice());
        }
    }
}

#[test]
fn the_thread_cache_keeps_no_more_entries_of_a_collection_than_its_cap() {
    let (fonts, query) = web_fonts(2);
    thread_cache::clear();
    for c in 'a'..='z' {
        fonts.match_cluster(&query, &c.to_string());
    }
    assert!(thread_cache::len() <= 2, "{}", thread_cache::len());
}
