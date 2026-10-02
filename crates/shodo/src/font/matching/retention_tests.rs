use super::*;

fn controlled_catalog() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        super::super::FontOptions {
            system_fonts: false,
            match_cache_entries: 0,
            ..Default::default()
        },
    );
    fonts.set_fallback_families(*b"Latn", None, vec![]);
    fonts
}

fn install_catalog_face(fonts: &FontCollection, family: &str, width: u16) {
    // Populate the platform catalog without explicitly registering a shodo
    // face. This exercises native loading with fixed bytes, not host fonts.
    let data = super::super::browser_tests::test_font(family, &['a'], width);
    assert!(
        !fonts
            .state()
            .native
            .register_fonts(super::super::Blob::from(data), None)
            .is_empty()
    );
}

#[test]
fn last_resort_does_not_depend_on_native_materialization_order() {
    for order in [["Native A", "Native B"], ["Native B", "Native A"]] {
        let fonts = controlled_catalog();
        let registered = fonts
            .register(super::super::browser_tests::test_font(
                "Bundled",
                &['a'],
                500,
            ))
            .unwrap();
        install_catalog_face(&fonts, "Native A", 600);
        install_catalog_face(&fonts, "Native B", 700);
        let fallback = FontQuery {
            families: vec![],
            ..Default::default()
        };
        assert_eq!(fonts.match_cluster(&fallback, "a").unwrap().id, registered);
        for name in order {
            let query = FontQuery {
                families: vec![FontFamily::Named(name.into())],
                ..Default::default()
            };
            assert_ne!(fonts.match_cluster(&query, "a").unwrap().id, registered);
            assert_eq!(fonts.match_cluster(&fallback, "a").unwrap().id, registered);
            let document = FontCollection::for_document(&fonts, &Limits::default());
            assert_eq!(
                document.match_cluster(&fallback, "a").unwrap().id,
                registered
            );
        }
    }
}

#[test]
fn equal_native_sources_keep_the_same_face_after_materialization() {
    let fonts = controlled_catalog();
    install_catalog_face(&fonts, "Duplicate", 600);
    install_catalog_face(&fonts, "Duplicate", 700);
    assert_eq!(fonts.native_candidates("Duplicate").len(), 2);
    let query = FontQuery {
        families: vec![FontFamily::Named("Duplicate".into())],
        ..Default::default()
    };
    let first = fonts.match_cluster(&query, "a").unwrap().id;
    for _ in 0..4 {
        assert_eq!(fonts.match_cluster(&query, "a").unwrap().id, first);
    }
}

#[test]
fn native_font_family_name_is_available_without_a_face_descriptor() {
    let fonts = controlled_catalog();
    install_catalog_face(&fonts, "Native Family", 600);
    let query = FontQuery {
        families: vec![FontFamily::Named("Native Family".into())],
        ..Default::default()
    };
    let id = fonts.match_cluster(&query, "a").unwrap().id;

    assert!(fonts.face_descriptor(id).is_none());
    assert_eq!(fonts.family_name(id).as_deref(), Some("Native Family"));
}

#[test]
fn native_file_choice_does_not_depend_on_lazy_source_id_assignment() {
    let dir = std::env::temp_dir().join(format!(
        "shodo-native-order-{}-{}",
        std::process::id(),
        SourceId::new().to_u64()
    ));
    std::fs::create_dir(&dir).unwrap();
    let a_path = std::sync::Arc::<std::path::Path>::from(dir.join("a.ttf"));
    let b_path = std::sync::Arc::<std::path::Path>::from(dir.join("b.ttf"));
    let a_bytes = super::super::browser_tests::test_font("Duplicate", &['a'], 600);
    let b_bytes = super::super::browser_tests::test_font("Duplicate", &['a'], 700);
    std::fs::write(&a_path, &a_bytes).unwrap();
    std::fs::write(&b_path, &b_bytes).unwrap();
    let query = FontQuery::default();
    let candidate = |path: std::sync::Arc<std::path::Path>, source: SourceId| {
        let info =
            FontInfo::from_source(SourceInfo::new(source, SourceKind::Path(path)), 0).unwrap();
        let data = FontData::new(info.load(None).unwrap(), 0);
        let mut candidate = reloaded(source);
        candidate.data = data;
        candidate.info = info;
        candidate
    };
    for reverse in [false, true] {
        // A previous family lookup can allocate B's source before A's source,
        // e.g. when another family shares B's TTC in fontconfig/DirectWrite.
        let mut ids = [SourceId::new(), SourceId::new()];
        if reverse {
            ids.reverse();
        }
        let fonts = controlled_catalog();
        let selected = fonts
            .best_match(
                vec![
                    candidate(a_path.clone(), ids[0]),
                    candidate(b_path.clone(), ids[1]),
                ],
                &query,
                &mut FontCluster::new("a"),
                true,
            )
            .unwrap();
        assert!(
            fonts.font_data(selected.id).unwrap().data.as_ref() == b_bytes,
            "lazy source ID assignment changed the selected file"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn reloaded(source: SourceId) -> Candidate {
    let blob = super::super::Blob::from(super::super::browser_tests::test_font(
        "Native",
        &['a'],
        600,
    ));
    let info = FontInfo::from_source(SourceInfo::new(source, SourceKind::Memory(blob.clone())), 0)
        .unwrap();
    Candidate {
        order_key: (source.to_u64(), 0),
        id: FontId {
            layer: 0,
            index: u32::MAX,
        },
        data: FontData::new(blob, 0),
        descriptor: intrinsic_descriptor(&info, "Native".into()),
        info,
        color: false,
    }
}
#[test]
fn reloaded_platform_face_keeps_identity_and_blob() {
    let fonts = FontCollection::new(&crate::limits::Limits::default());
    let source = SourceId::new();
    let first = fonts
        .best_match(
            vec![reloaded(source)],
            &FontQuery::default(),
            &mut FontCluster::new("a"),
            true,
        )
        .unwrap();
    let blob = fonts.state().faces[1].data.id();
    for _ in 0..3 {
        let next = fonts
            .best_match(
                vec![reloaded(source)],
                &FontQuery::default(),
                &mut FontCluster::new("a"),
                true,
            )
            .unwrap();
        assert_eq!(next.id, first.id);
        assert_eq!(fonts.state().faces.len(), 2);
        assert_eq!(fonts.state().faces[1].data.id(), blob);
    }
    assert_eq!(fonts.state().blob_bytes, 0);
}
#[test]
fn platform_retention_does_not_consume_registration_limits() {
    for limits in [
        crate::limits::Limits {
            max_faces_per_layer: Some(1),
            ..Default::default()
        },
        crate::limits::Limits {
            max_layer_blob_bytes: Some(0),
            ..Default::default()
        },
    ] {
        let fonts = FontCollection::new(&limits);
        for _ in 0..257 {
            assert!(
                fonts
                    .best_match(
                        vec![reloaded(SourceId::new())],
                        &FontQuery::default(),
                        &mut FontCluster::new("a"),
                        true
                    )
                    .is_some()
            );
        }
        assert_eq!(fonts.state().faces.len(), 258);
        assert_eq!(fonts.state().blob_bytes, 0);
    }
}

#[test]
fn explicit_face_cap_is_unchanged_after_native_queries() {
    let limits = Limits {
        max_faces_per_layer: Some(2),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        super::super::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .best_match(
            vec![reloaded(SourceId::new())],
            &FontQuery::default(),
            &mut FontCluster::new("a"),
            true,
        )
        .unwrap();
    let bytes = super::super::browser_tests::test_font("Registered", &['a'], 500);
    fonts.register(bytes.clone()).unwrap();
    assert!(
        matches!(fonts.register_face(bytes, 0, FontFaceDescriptor { family: "Explicit".into(), ..Default::default() }),
        Err(super::super::FontError::Limit(err)) if err.kind == LimitKind::FacesPerLayer)
    );
}

#[test]
fn explicit_local_aliases_charge_a_native_blob_once() {
    let bytes = super::super::browser_tests::test_font("Native", &['a'], 600);
    let limits = Limits {
        max_faces_per_layer: Some(4),
        max_layer_blob_bytes: Some(bytes.len() as u64),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        super::super::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .best_match(
            vec![reloaded(SourceId::new())],
            &FontQuery::default(),
            &mut FontCluster::new("a"),
            true,
        )
        .unwrap();
    assert_eq!(fonts.state().blob_bytes, 0);
    for family in ["First", "Second"] {
        fonts
            .register_sources(
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
                vec![super::super::FontSource::Local("Native-Regular".into())],
            )
            .unwrap();
    }
    assert_eq!(fonts.state().blob_bytes, bytes.len() as u64);
    assert!(
        matches!(fonts.register_face(bytes, 0, FontFaceDescriptor { family: "Explicit".into(), ..Default::default() }),
        Err(super::super::FontError::Limit(err)) if err.kind == LimitKind::LayerBlobBytes)
    );
}

#[test]
fn native_blob_reuse_cannot_bypass_explicit_registration_budget() {
    let fonts = FontCollection::with_options(
        &Limits {
            max_layer_blob_bytes: Some(0),
            ..Default::default()
        },
        super::super::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .best_match(
            vec![reloaded(SourceId::new())],
            &FontQuery::default(),
            &mut FontCluster::new("a"),
            true,
        )
        .unwrap();
    let before = fonts.generation();
    assert!(
        matches!(fonts.register_sources(FontFaceDescriptor { family: "Explicit".into(), ..Default::default() },
        vec![super::super::FontSource::Local("Native-Regular".into())]),
        Err(super::super::FontError::Limit(err)) if err.kind == LimitKind::LayerBlobBytes)
    );
    assert_eq!(fonts.generation(), before);
    assert_eq!(fonts.state().faces.len(), 2);
}

#[test]
fn native_faces_still_obey_individual_font_validation_limits() {
    let candidate = reloaded(SourceId::new());
    let fonts = FontCollection::with_options(
        &Limits {
            max_font_blob_bytes: Some(candidate.data.data.len() as u64 - 1),
            ..Default::default()
        },
        super::super::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    assert!(
        fonts
            .best_match(
                vec![candidate],
                &FontQuery::default(),
                &mut FontCluster::new("a"),
                true
            )
            .is_none()
    );
    assert_eq!(fonts.state().faces.len(), 1);
}

#[test]
fn stale_invalid_platform_axes_are_rejected_even_when_loaded_bytes_are_valid() {
    for values in [[900, 400, 100], [100, 99, 900], [100, 901, 900]] {
        let fonts = FontCollection::new(&Limits::default());
        let source = SourceId::new();
        let mut candidate = reloaded(source);
        // Platform metadata can outlive the file from which it was read.
        // The newly loaded bytes in candidate.data are a valid static font.
        let stale = super::super::Blob::from(super::super::browser_tests::font_with_axes(&[(
            *b"wght", values,
        )]));
        candidate.info =
            FontInfo::from_source(SourceInfo::new(source, SourceKind::Memory(stale)), 0).unwrap();
        assert!(
            fonts
                .best_match(
                    vec![candidate],
                    &FontQuery::default(),
                    &mut FontCluster::new("a"),
                    true
                )
                .is_none()
        );
        assert_eq!(fonts.state().faces.len(), 1);
        assert_eq!(fonts.state().blob_bytes, 0);
    }
}
