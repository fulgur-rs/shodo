use super::*;
use crate::font::{FontCollection, FontOptions, FontSource};
use crate::limits::{LimitExceeded, LimitKind, Limits};

fn collection(limits: &Limits) -> FontCollection {
    FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}

fn font() -> Vec<u8> {
    crate::font::browser_tests::test_font("Web", &['a', 'b'], 600)
}

fn descriptor(ranges: usize) -> FontFaceDescriptor {
    FontFaceDescriptor {
        family: "Web".into(),
        unicode_ranges: vec![(0x61, 0x62); ranges],
        ..Default::default()
    }
}

#[test]
fn direct_registration_rejects_descriptor_payload_before_retaining_the_face() {
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(64),
        ..Default::default()
    });
    let before = fonts.generation();
    let result = fonts.register_face(font(), 0, descriptor(8));
    assert!(
        matches!(
            result,
            Err(FontError::Limit(LimitExceeded {
                kind: LimitKind::FontDescriptorBytes,
                limit: 64,
                actual: 134,
            }))
        ),
        "oversized descriptor was accepted: {result:?}"
    );
    assert_eq!(fonts.generation(), before);
    assert_eq!(fonts.state().descriptors.iter().flatten().count(), 0);
}

#[test]
fn source_preflight_rejects_descriptor_payload_before_validation_or_attempts() {
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(64),
        ..Default::default()
    });
    let before = fonts.generation();
    let mut input = descriptor(8);
    input.unicode_ranges[7] = (2, 1);
    let result = fonts.register_sources(input, vec![FontSource::Data(font(), 0)]);
    assert!(
        matches!(
            result,
            Err(FontError::Limit(LimitExceeded {
                kind: LimitKind::FontDescriptorBytes,
                limit: 64,
                actual: 134,
            }))
        ),
        "range validation/attempt preceded admission: {result:?}"
    );
    assert_eq!(fonts.generation(), before);
    assert_eq!(fonts.state().descriptors.iter().flatten().count(), 0);
}

#[test]
fn descriptor_payloads_share_the_layer_budget_without_partial_registration() {
    let fonts = collection(&Limits {
        max_layer_descriptor_bytes: Some(128),
        ..Default::default()
    });
    let first = fonts.register_face(font(), 0, descriptor(4)).unwrap();
    let before = fonts.generation();
    let bytes = fonts.state().blob_bytes;
    let result = fonts.register_face(font(), 0, descriptor(4));
    assert!(
        matches!(
            result,
            Err(FontError::Limit(LimitExceeded {
                kind: LimitKind::LayerDescriptorBytes,
                limit: 128,
                actual: 140,
            }))
        ),
        "descriptor layer budget was not enforced: {result:?}"
    );
    assert_eq!(fonts.generation(), before);
    assert_eq!(fonts.state().blob_bytes, bytes);
    assert_eq!(fonts.state().descriptors.iter().flatten().count(), 1);
    assert_eq!(fonts.face_descriptor(first).unwrap(), descriptor(4));
}

#[test]
fn range_coverage_work_does_not_scan_the_raw_descriptor_per_scalar() {
    let fonts = collection(&Limits::unlimited());
    let mut input = descriptor(0);
    input.unicode_ranges = (0..4096)
        .map(|i| (0x1000 + i * 2, 0x1000 + i * 2))
        .collect();
    input.unicode_ranges.push((0x61, 0x62));
    let id = fonts.register_face(font(), 0, input).unwrap();
    let query = crate::font::FontQuery {
        families: vec![crate::style::FontFamily::Named("Web".into())],
        ..Default::default()
    };
    RANGE_PROBES.with(|count| count.set(0));
    assert_eq!(fonts.match_cluster(&query, "a").unwrap().id, id);
    let probes = RANGE_PROBES.with(std::cell::Cell::get);
    assert!(probes <= 16, "one scalar visited {probes} raw ranges");
}

#[test]
fn exact_descriptor_boundaries_and_explicit_opt_outs_preserve_valid_registration() {
    for cap in [Some(0), Some(21), Some(22), Some(23), None] {
        let fonts = collection(&Limits {
            max_font_descriptor_bytes: cap,
            ..Default::default()
        });
        let result = fonts.register_face(font(), 0, descriptor(1));
        if cap.is_some_and(|cap| cap < 22) {
            assert_eq!(
                result.unwrap_err(),
                FontError::Limit(LimitExceeded {
                    kind: LimitKind::FontDescriptorBytes,
                    limit: cap.unwrap(),
                    actual: 22,
                })
            );
            assert_eq!(fonts.state().descriptor_bytes, 0);
        } else {
            let id = result.unwrap();
            assert_eq!(fonts.face_descriptor(id).unwrap(), descriptor(1));
            assert_eq!(fonts.state().descriptor_bytes, 22);
        }
    }
    let mut large = descriptor(8193);
    let fonts = collection(&Limits::default());
    assert!(matches!(
        fonts.register_face(font(), 0, large.clone()),
        Err(FontError::Limit(LimitExceeded {
            kind: LimitKind::FontDescriptorBytes,
            ..
        }))
    ));
    let fonts = collection(&Limits::unlimited());
    let id = fonts.register_face(font(), 0, large.clone()).unwrap();
    assert_eq!(fonts.face_descriptor(id).unwrap(), large);
    // Raw duplicates must not be merged before admission to evade the cap.
    large.unicode_ranges.fill((0x61, 0x62));
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(22),
        ..Default::default()
    });
    assert!(matches!(
        fonts.register_face(font(), 0, large),
        Err(FontError::Limit(_))
    ));
}

#[test]
fn original_range_order_and_union_coverage_survive_internal_normalization() {
    for cache in [0, 1, 8] {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: cache,
                ..Default::default()
            },
        );
        let input = FontFaceDescriptor {
            family: "Web".into(),
            unicode_ranges: vec![
                (100, 100),
                (97, 97),
                (98, 98),
                (97, 97),
                (0xd800, 0xdfff),
                (0x10ffff, 0x10ffff),
            ],
            ..Default::default()
        };
        let id = fonts
            .register_face(
                crate::font::browser_tests::test_font("Web", &['a', 'b', 'c', 'd'], 600),
                0,
                input.clone(),
            )
            .unwrap();
        assert_eq!(fonts.face_descriptor(id).unwrap(), input);
        let query = crate::font::FontQuery {
            families: vec![crate::style::FontFamily::Named("Web".into())],
            ..Default::default()
        };
        for text in ["a", "b", "d", "ab", "a", "b"] {
            assert_eq!(fonts.match_cluster(&query, text).unwrap().id, id);
        }
        assert_ne!(fonts.match_cluster(&query, "c").map(|m| m.id), Some(id));
        let long = format!("a{}", "\u{200d}".repeat(4096));
        assert_eq!(fonts.match_cluster(&query, &long).unwrap().id, id);
        assert_eq!(fonts.face_descriptor(id).unwrap(), input);
    }
}

#[test]
fn canonical_coverage_matches_inclusive_overlapping_unions_and_empty_ranges() {
    for ranges in [
        vec![],
        vec![(8, 12), (0, 2), (2, 4), (7, 9), (8, 12), (20, 20)],
    ] {
        let descriptor = RegisteredDescriptor::new(FontFaceDescriptor {
            unicode_ranges: ranges.clone(),
            ..Default::default()
        });
        for scalar in 0..32 {
            let expected = ranges.is_empty()
                || ranges
                    .iter()
                    .any(|&(min, max)| min <= scalar && scalar <= max);
            assert_eq!(
                descriptor.covers(char::from_u32(scalar).unwrap()),
                expected,
                "scalar {scalar}"
            );
        }
        assert_eq!(descriptor.unicode_ranges, ranges);
    }
}

#[test]
fn coverage_keys_compare_union_values_and_verify_fingerprint_collisions() {
    let first = RegisteredDescriptor::new(FontFaceDescriptor {
        unicode_ranges: vec![(8, 12), (0, 4)],
        ..Default::default()
    });
    let equivalent = RegisteredDescriptor::new(FontFaceDescriptor {
        unicode_ranges: vec![(0, 2), (2, 4), (10, 12), (8, 11)],
        ..Default::default()
    });
    assert!(first.same_coverage(&equivalent));
    assert_eq!(first.coverage_hash(), equivalent.coverage_hash());
    let mut distinct = RegisteredDescriptor::new(FontFaceDescriptor {
        unicode_ranges: vec![(5, 7)],
        ..Default::default()
    });
    // Exercise a colliding fingerprint without relying on hash randomness.
    Arc::get_mut(&mut distinct.0).unwrap().coverage_hash = first.coverage_hash();
    assert!(!first.same_coverage(&distinct));
    assert!(first.covers('\u{8}'));
    assert!(!distinct.covers('\u{8}'));
}

#[test]
fn malformed_and_failed_sources_leave_descriptor_allowance_available() {
    let fonts = collection(&Limits {
        max_layer_descriptor_bytes: Some(22),
        ..Default::default()
    });
    let mut invalid = descriptor(1);
    invalid.unicode_ranges[0] = (2, 1);
    assert!(matches!(
        fonts.register_face(font(), 0, invalid),
        Err(FontError::Malformed(_))
    ));
    assert!(matches!(
        fonts.register_face(vec![0; 4], 0, descriptor(1)),
        Err(FontError::Malformed(_))
    ));
    assert_eq!(fonts.state().descriptor_bytes, 0);
    INDEX_BUILDS.with(|count| count.set(0));
    let id = fonts
        .register_sources(
            descriptor(1),
            vec![
                FontSource::Local("Unavailable".into()),
                FontSource::Data(vec![0; 4], 0),
                FontSource::Data(font(), 0),
            ],
        )
        .unwrap();
    assert_eq!(fonts.state().descriptor_bytes, 22);
    assert_eq!(fonts.face_descriptor(id).unwrap(), descriptor(1));
    assert_eq!(INDEX_BUILDS.with(std::cell::Cell::get), 1);
}

#[test]
fn family_payload_and_source_preflight_share_the_descriptor_limit() {
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(64),
        max_local_font_index_bytes: Some(0),
        ..Default::default()
    });
    let mut input = descriptor(0);
    input.family = "W".repeat(33);
    for sources in [
        vec![],
        vec![FontSource::Local("Unavailable".into())],
        vec![FontSource::Data(font(), 0)],
    ] {
        let result = fonts.register_sources(input.clone(), sources);
        assert_eq!(
            result.unwrap_err(),
            FontError::Limit(LimitExceeded {
                kind: LimitKind::FontDescriptorBytes,
                limit: 64,
                actual: 66,
            })
        );
    }
    assert_eq!(fonts.state().descriptor_bytes, 0);
    assert_eq!(fonts.generation(), 0);
}

#[test]
fn publication_rechecks_layer_allowance_after_descriptor_preparation() {
    let fonts = collection(&Limits {
        max_layer_descriptor_bytes: Some(22),
        ..Default::default()
    });
    let first = fonts.prepare_descriptor(descriptor(1)).unwrap();
    let second = fonts.prepare_descriptor(descriptor(1)).unwrap();
    fonts
        .register_source_blob(crate::font::Blob::from(font()), 0, first, None)
        .unwrap();
    let before = fonts.generation();
    let result = fonts.register_source_blob(crate::font::Blob::from(font()), 0, second, None);
    assert_eq!(
        result.unwrap_err(),
        FontError::Limit(LimitExceeded {
            kind: LimitKind::LayerDescriptorBytes,
            limit: 22,
            actual: 44,
        })
    );
    assert_eq!(fonts.generation(), before);
    assert_eq!(fonts.state().descriptor_bytes, 22);
    assert_eq!(fonts.state().descriptors.iter().flatten().count(), 1);
}

#[test]
fn document_budgets_are_local_and_do_not_readmit_shared_descriptors() {
    let limits = Limits {
        max_layer_descriptor_bytes: Some(22),
        ..Default::default()
    };
    let root = collection(&limits);
    let root_id = root.register_face(font(), 0, descriptor(1)).unwrap();
    let doc = FontCollection::for_document(
        &root,
        &Limits {
            max_font_descriptor_bytes: Some(0),
            max_layer_descriptor_bytes: Some(0),
            ..Default::default()
        },
    );
    let query = crate::font::FontQuery {
        families: vec![crate::style::FontFamily::Named("Web".into())],
        ..Default::default()
    };
    assert_eq!(doc.match_cluster(&query, "a").unwrap().id, root_id);
    assert!(matches!(
        doc.register_face(font(), 0, descriptor(1)),
        Err(FontError::Limit(_))
    ));
    for _ in 0..2 {
        let doc = FontCollection::for_document(&root, &limits);
        doc.register_face(font(), 0, descriptor(1)).unwrap();
        assert_eq!(doc.state().descriptor_bytes, 22);
    }
    assert_eq!(root.state().descriptor_bytes, 22);
}

#[test]
fn caller_spare_capacity_is_not_retained_by_registered_descriptors() {
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(22),
        ..Default::default()
    });
    let mut input = descriptor(1);
    input.unicode_ranges.reserve(4096);
    input.family.reserve(4096);
    let id = fonts.register_face(font(), 0, input).unwrap();
    let state = fonts.state();
    let retained = state.descriptors[id.index as usize].as_ref().unwrap();
    assert_eq!(retained.unicode_ranges.capacity(), 1);
    assert_eq!(retained.family.capacity(), 3);
    assert_eq!(retained.bytes(), 22);
}

#[test]
fn rejected_descriptors_do_not_compile_or_copy_range_payloads() {
    let fonts = collection(&Limits {
        max_font_descriptor_bytes: Some(64),
        ..Default::default()
    });
    for sources in [false, true] {
        INDEX_BUILDS.with(|count| count.set(0));
        let result = if sources {
            fonts.register_sources(descriptor(8), vec![FontSource::Data(font(), 0)])
        } else {
            fonts.register_face(font(), 0, descriptor(8))
        };
        assert!(matches!(result, Err(FontError::Limit(_))));
        assert_eq!(INDEX_BUILDS.with(std::cell::Cell::get), 0);
    }
}
