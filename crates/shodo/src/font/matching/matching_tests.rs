use super::*;
use crate::font::FontOptions;
use std::cell::Cell;

std::thread_local! {
    static INFO_READS: Cell<usize> = const { Cell::new(0) };
    static FONT_READS: Cell<usize> = const { Cell::new(0) };
    static KEY_COMPARISONS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_key_comparison() {
    KEY_COMPARISONS.with(|n| n.set(n.get() + 1));
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
    assert_eq!(fonts.state().matches.len(), 64);
    // The newest entry is the worst case for a front-to-back scan.
    KEY_COMPARISONS.with(|n| n.set(0));
    fonts.match_cluster(&query, clusters.last().unwrap());
    let compared = KEY_COMPARISONS.with(Cell::get);
    assert!(
        compared <= 2,
        "{compared} key comparisons for one cache hit"
    );
    assert_eq!(fonts.state().matches.len(), 64);
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
                fonts.match_scripted(&base, script, cluster),
                fonts.match_cluster(&cloned, cluster),
                "{script:?} {cluster:?}"
            );
        }
    }
    // Six distinct (script, cluster) keys are cached once each, and the
    // equivalent cloned-query lookups above hit the same entries.
    assert_eq!(fonts.state().matches.len(), 6);
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
    let hit_cost = |c: &str| {
        KEY_COMPARISONS.with(|n| n.set(0));
        fonts.match_cluster(&query, c);
        KEY_COMPARISONS.with(Cell::get)
    };
    fonts.match_cluster(&query, "A");
    fonts.match_cluster(&query, "B");
    fonts.match_cluster(&query, "A"); // A is now most recent
    fonts.match_cluster(&query, "C"); // evicts B, not A
    assert!(
        hit_cost("A") > 0,
        "A must still be cached: a hit compares its key"
    );
    // A miss on B compares no cached key against an equal entry; re-inserting
    // it must not have kept a stale copy.
    assert_eq!(fonts.state().matches.len(), 2);
    fonts.match_cluster(&query, "B"); // evicts C (A was just touched)
    let a_after = hit_cost("A");
    assert!(a_after > 0);
    assert_eq!(fonts.state().matches.len(), 2);
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
