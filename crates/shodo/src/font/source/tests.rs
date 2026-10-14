use super::*;
use crate::font::{FontOptions, browser_tests::test_font};
use crate::limits::Limits;

fn shared() -> FontCollection {
    with_limits(&Limits::default())
}

fn with_limits(limits: &Limits) -> FontCollection {
    FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}

fn limit_error(result: Result<crate::font::FontId, FontError>, kind: LimitKind) {
    assert!(
        matches!(result, Err(FontError::Limit(error)) if error.kind == kind),
        "{result:?}"
    );
}

fn descriptor() -> FontFaceDescriptor {
    FontFaceDescriptor {
        family: "Alias".into(),
        ..Default::default()
    }
}

fn missing(fonts: &FontCollection, name: &str) {
    assert!(matches!(
        fonts.register_sources(descriptor(), vec![FontSource::Local(name.into())]),
        Err(FontError::Malformed(_))
    ));
}

#[test]
fn distinct_local_misses_share_name_metadata_across_documents() {
    let root = shared();
    root.register(test_font("Bundled", &['a'], 500)).unwrap();
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    missing(&root, "missing-first");
    let first_reads = LOCAL_NAME_FACE_READS.with(std::cell::Cell::get);
    assert!(first_reads > 0, "fixture must exercise name parsing");
    for n in 0..80 {
        let document = FontCollection::for_document(&root, &Limits::default());
        missing(&document.clone(), &format!("missing-{n}"));
    }
    assert_eq!(
        LOCAL_NAME_FACE_READS.with(std::cell::Cell::get),
        first_reads,
        "distinct missing names reparse the shared catalog"
    );
    assert_eq!(root.generation(), 1);
}

#[test]
fn native_local_misses_do_not_reload_catalog_sources() {
    let root = shared();
    // Installed fonts are represented by catalog entries without root faces.
    root.state()
        .native
        .register_fonts(peniko::Blob::from(test_font("Catalog", &['a'], 500)), None);
    LOCAL_CATALOG_LOADS.with(|calls| calls.set(0));
    missing(&root, "missing-first");
    let first_loads = LOCAL_CATALOG_LOADS.with(std::cell::Cell::get);
    assert!(first_loads > 0, "fixture must load a catalog source");
    for n in 0..80 {
        let document = FontCollection::for_document(&root, &Limits::default());
        missing(&document, &format!("missing-{n}"));
    }
    assert_eq!(
        LOCAL_CATALOG_LOADS.with(std::cell::Cell::get),
        first_loads,
        "distinct missing names reload the native catalog"
    );
    assert_eq!(root.generation(), 0);
}

#[test]
fn source_count_is_checked_before_even_a_successful_first_source() {
    let fonts = with_limits(&Limits {
        max_font_sources: Some(1),
        ..Default::default()
    });
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    limit_error(
        fonts.register_sources(
            descriptor(),
            vec![
                FontSource::Data(test_font("Web", &['a'], 500), 0),
                FontSource::Local("absent".into()),
            ],
        ),
        LimitKind::FontSources,
    );
    assert_eq!(fonts.generation(), 0);
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), 0);
    fonts
        .register_sources(
            descriptor(),
            vec![FontSource::Data(test_font("Web", &['a'], 500), 0)],
        )
        .unwrap();
    assert_eq!(fonts.generation(), 1);
}

#[test]
fn local_name_bytes_include_the_unvisited_suffix_and_exact_boundary() {
    let fonts = with_limits(&Limits {
        max_font_source_name_bytes: Some(3),
        ..Default::default()
    });
    missing(&fonts, "abc");
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    limit_error(
        fonts.register_sources(
            descriptor(),
            vec![
                FontSource::Data(test_font("Web", &['a'], 500), 0),
                FontSource::Local("ab".into()),
                FontSource::Local("cd".into()),
            ],
        ),
        LimitKind::FontSourceNameBytes,
    );
    assert_eq!(fonts.generation(), 0);
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), 0);
    // Count UTF-8 bytes rather than scalar values.
    limit_error(
        fonts.register_sources(descriptor(), vec![FontSource::Local("水a".into())]),
        LimitKind::FontSourceNameBytes,
    );
}

#[test]
fn index_byte_failure_is_shared_sticky_and_blocks_data_fallback() {
    let root = with_limits(&Limits {
        max_local_font_index_bytes: Some(0),
        ..Default::default()
    });
    root.state()
        .native
        .register_fonts(peniko::Blob::from(test_font("Catalog", &['a'], 500)), None);
    LOCAL_CATALOG_LOADS.with(|calls| calls.set(0));
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    for n in 0..80 {
        let doc = FontCollection::for_document(&root, &Limits::unlimited());
        limit_error(
            doc.register_sources(
                descriptor(),
                vec![
                    FontSource::Local(format!("missing-{n}")),
                    FontSource::Data(test_font("Web", &['a'], 500), 0),
                ],
            ),
            LimitKind::LocalFontIndexBytes,
        );
        assert_eq!(doc.generation(), 0);
        assert!(doc.state().faces.is_empty());
        assert_eq!(doc.state().blob_bytes, 0);
    }
    assert_eq!(
        LOCAL_NAME_FACE_READS.with(std::cell::Cell::get),
        1,
        "failed index was restarted"
    );
    assert_eq!(
        LOCAL_CATALOG_LOADS.with(std::cell::Cell::get),
        0,
        "byte admission occurred after a source load"
    );
}

#[test]
fn construction_work_failure_keeps_known_names_without_restarting() {
    // Stub visit 1 + A visit 1 + four records scanned twice 8 + full-name
    // UTF-16 bytes 18 = 28. The subsequent PostScript record exceeds the cap.
    let root = with_limits(&Limits {
        max_local_font_lookup_work: Some(28),
        ..Default::default()
    });
    let native = root.register(test_font("A", &['a'], 500)).unwrap();
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    for n in 0..80 {
        let doc = FontCollection::for_document(&root, &Limits::unlimited());
        limit_error(
            doc.register_sources(
                descriptor(),
                vec![FontSource::Local(format!("missing-{n}"))],
            ),
            LimitKind::LocalFontLookupWork,
        );
    }
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), 2);
    let doc = FontCollection::for_document(&root, &Limits::default());
    let alias = doc
        .register_sources(descriptor(), vec![FontSource::Local("A Regular".into())])
        .unwrap();
    assert_eq!(
        doc.font_data(alias).unwrap().data.id(),
        root.font_data(native).unwrap().data.id()
    );
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), 2);
}

#[test]
fn root_registration_after_a_miss_adds_names_without_rebuilding_native_index() {
    let root = shared();
    missing(&root, "Added-Regular");
    LOCAL_CATALOG_LOADS.with(|calls| calls.set(0));
    let native = root.register(test_font("Added", &['a'], 500)).unwrap();
    let doc = FontCollection::for_document(&root, &Limits::default());
    let alias = doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("Added-Regular".into())],
        )
        .unwrap();
    assert_eq!(
        doc.font_data(alias).unwrap().data.id(),
        root.font_data(native).unwrap().data.id()
    );
    assert_eq!(LOCAL_CATALOG_LOADS.with(std::cell::Cell::get), 0);
}

#[test]
fn registration_cannot_publish_faces_that_poison_an_active_local_index() {
    let root = with_limits(&Limits {
        max_local_font_lookup_work: Some(128),
        ..Default::default()
    });
    root.state()
        .native
        .register_fonts(peniko::Blob::from(test_font("A", &['a'], 500)), None);
    let doc = FontCollection::for_document(&root, &Limits::default());
    let first = doc
        .register_sources(descriptor(), vec![FontSource::Local("A Regular".into())])
        .unwrap();
    let oversized_postscript = "Added".repeat(100);
    let rows = [
        (1, 0x0409, "Added"),
        (4, 0x0409, "A Regular"),
        (6, 0x0409, oversized_postscript.as_str()),
    ];
    limit_error(
        root.register(named_font(&rows)),
        LimitKind::LocalFontLookupWork,
    );
    assert_eq!(root.generation(), 0);
    assert_eq!(root.state().faces.len(), 1);
    let second = doc
        .register_sources(descriptor(), vec![FontSource::Local("A Regular".into())])
        .unwrap();
    assert_eq!(
        doc.font_data(first).unwrap().data.id(),
        doc.font_data(second).unwrap().data.id()
    );
}

#[test]
fn added_registered_names_take_precedence_over_indexed_native_names() {
    let root = shared();
    root.state()
        .native
        .register_fonts(peniko::Blob::from(test_font("A", &['a'], 500)), None);
    missing(&root, "absent");
    let registered = root.register(test_font("A", &['a'], 700)).unwrap();
    let doc = FontCollection::for_document(&root, &Limits::default());
    let alias = doc
        .register_sources(descriptor(), vec![FontSource::Local("A Regular".into())])
        .unwrap();
    assert_eq!(
        doc.font_data(alias).unwrap().data.id(),
        root.font_data(registered).unwrap().data.id()
    );
}

#[test]
fn css_faces_do_not_consume_the_installed_name_index_work_budget() {
    let root = with_limits(&Limits {
        max_local_font_lookup_work: Some(128),
        ..Default::default()
    });
    root.state()
        .native
        .register_fonts(peniko::Blob::from(test_font("A", &['a'], 500)), None);
    missing(&root, "absent");
    for _ in 0..100 {
        root.register_face(test_font("Web", &['a'], 500), 0, descriptor())
            .unwrap();
    }
    let doc = FontCollection::for_document(&root, &Limits::default());
    doc.register_sources(descriptor(), vec![FontSource::Local("A Regular".into())])
        .unwrap();
    missing(&doc, "Web Regular");
}

fn collection_bytes(first: Vec<u8>, second: Vec<u8>) -> Vec<u8> {
    let offsets = [20usize, 20 + first.len()];
    let mut collection = b"ttcf\0\x01\0\0\0\0\0\x02".to_vec();
    for offset in offsets {
        collection.extend_from_slice(&(offset as u32).to_be_bytes());
    }
    for (mut face, offset) in [(first, offsets[0]), (second, offsets[1])] {
        let tables = u16::from_be_bytes([face[4], face[5]]) as usize;
        for table in 0..tables {
            let at = 12 + table * 16 + 8;
            let value = u32::from_be_bytes(face[at..at + 4].try_into().unwrap());
            face[at..at + 4].copy_from_slice(&(value + offset as u32).to_be_bytes());
        }
        collection.extend(face);
    }
    collection
}

#[test]
fn active_index_admits_all_collection_faces_or_none() {
    let root = with_limits(&Limits {
        max_local_font_lookup_work: Some(128),
        ..Default::default()
    });
    missing(&root, "absent");
    let oversized = "Long".repeat(100);
    let bytes = collection_bytes(
        test_font("First", &['a'], 500),
        named_font(&[
            (1, 0x0409, "Second"),
            (4, 0x0409, "Second Regular"),
            (6, 0x0409, oversized.as_str()),
        ]),
    );
    limit_error(root.register(bytes), LimitKind::LocalFontLookupWork);
    assert_eq!(root.state().faces.len(), 1);
    assert_eq!(root.state().blob_bytes, 0);
    assert_eq!(root.generation(), 0);
    missing(&root, "First Regular");
    missing(&root, "Second Regular");
    let first = root.register(test_font("First", &['a'], 500)).unwrap();
    let doc = FontCollection::for_document(&root, &Limits::default());
    let alias = doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("First Regular".into())],
        )
        .unwrap();
    assert_eq!(
        doc.font_data(alias).unwrap().data.id(),
        root.font_data(first).unwrap().data.id()
    );
}

#[test]
fn materialized_collection_face_keeps_its_nonzero_index() {
    let root = shared();
    root.state().native.register_fonts(
        peniko::Blob::from(collection_bytes(
            test_font("First", &['a'], 500),
            test_font("Second", &['b'], 700),
        )),
        None,
    );
    missing(&root, "absent");
    let query = crate::font::FontQuery {
        families: vec![crate::style::FontFamily::Named("Second".into())],
        ..Default::default()
    };
    let matched = root.match_cluster(&query, "b").unwrap();
    assert_eq!(root.font_data(matched.id).unwrap().index, 1);
    let doc = FontCollection::for_document(&root, &Limits::default());
    let alias = doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("Second Regular".into())],
        )
        .unwrap();
    assert_eq!(doc.font_data(alias).unwrap().index, 1);
    assert_eq!(
        doc.font_data(alias).unwrap().data.id(),
        root.font_data(matched.id).unwrap().data.id()
    );
}

#[test]
fn registered_full_names_preserve_unicode_string_lowercasing() {
    for (family, request) in [
        ("İstanbul", "i\u{307}stanbul regular"),
        ("ΣΟΣ", "σος regular"),
    ] {
        let root = shared();
        let native = root.register(test_font(family, &['a'], 500)).unwrap();
        let doc = FontCollection::for_document(&root, &Limits::default());
        let alias = doc
            .register_sources(descriptor(), vec![FontSource::Local(request.into())])
            .unwrap();
        assert_eq!(
            doc.font_data(alias).unwrap().data.id(),
            root.font_data(native).unwrap().data.id()
        );
    }
}

struct NativeFile {
    directory: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl NativeFile {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "shodo-local-index-{}-{}",
            std::process::id(),
            fontique::SourceId::new().to_u64()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("font.ttf");
        std::fs::write(&path, test_font("Installed", &['a'], 500)).unwrap();
        Self { directory, path }
    }

    fn collection(&self, age: u64) -> FontCollection {
        let root = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                source_cache_max_age: age,
                ..Default::default()
            },
        );
        root.state().native.load_fonts_from_paths([&self.path]);
        root
    }
}

impl Drop for NativeFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[test]
fn catalog_index_releases_path_blobs_without_source_cache_eviction() {
    let file = NativeFile::new();
    let root = file.collection(8);
    LOCAL_INDEX_BLOBS.with(|blobs| blobs.borrow_mut().clear());
    missing(&root, "absent");
    LOCAL_INDEX_BLOBS.with(|blobs| {
        let blobs = blobs.borrow();
        assert_eq!(blobs.len(), 1, "fixture must index a path source");
        assert!(
            blobs[0].upgrade().is_none(),
            "name index retained a mapped font blob"
        );
    });
}

#[test]
fn local_aliases_share_document_blob_after_source_cache_eviction() {
    let file = NativeFile::new();
    let root = file.collection(0);
    let doc = FontCollection::for_document(&root, &Limits::default());
    let a = doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("Installed Regular".into())],
        )
        .unwrap();
    // Independently populate and evict the matching cache. Local aliases must
    // keep using the document's retained source identity after eviction.
    let cached = {
        let mut state = root.state();
        let family = state.native.family_by_name("Installed").unwrap();
        let blob = family.fonts()[0]
            .load(Some(&mut state.source_cache))
            .unwrap();
        blob.downgrade()
    };
    assert!(
        cached.upgrade().is_some(),
        "fixture must populate the cache"
    );
    root.state().source_cache.prune(0, true);
    assert!(
        cached.upgrade().is_none(),
        "fixture must evict the cached blob"
    );
    missing(&root, "absent-after-eviction");
    let b = doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("Installed-Regular".into())],
        )
        .unwrap();
    assert_eq!(
        doc.font_data(a).unwrap().data.id(),
        doc.font_data(b).unwrap().data.id()
    );
    assert_eq!(
        doc.state().blob_bytes,
        std::fs::metadata(&file.path).unwrap().len()
    );
    assert_eq!(doc.generation(), 2);
}

#[test]
fn changed_path_source_never_registers_a_stale_local_name() {
    let file = NativeFile::new();
    let root = file.collection(0);
    missing(&root, "absent");
    let new_bytes = test_font("Replaced", &['a'], 700);
    let replacement = file.directory.join("replacement.ttf");
    std::fs::write(&replacement, &new_bytes).unwrap();
    std::fs::rename(&replacement, &file.path).unwrap();
    let doc = FontCollection::for_document(&root, &Limits::default());
    missing(&doc, "Installed Regular");
    missing(&doc, "Replaced Regular");
    assert_eq!(doc.generation(), 0);
    let refreshed = file.collection(0);
    let refreshed_doc = FontCollection::for_document(&refreshed, &Limits::default());
    let alias = refreshed_doc
        .register_sources(
            descriptor(),
            vec![FontSource::Local("Replaced Regular".into())],
        )
        .unwrap();
    assert_eq!(
        refreshed_doc.font_data(alias).unwrap().data.as_ref(),
        new_bytes.as_slice()
    );
}

#[test]
fn unavailable_or_renamed_native_candidate_uses_the_next_exact_name() {
    for rename in [false, true] {
        let files = [NativeFile::new(), NativeFile::new()];
        let bytes: Vec<_> = ["First", "Second"]
            .into_iter()
            .map(|family| {
                named_font(&[
                    (1, 0x0409, family),
                    (4, 0x0409, "Shared Regular"),
                    (6, 0x0409, "Shared-Regular"),
                ])
            })
            .collect();
        for (file, bytes) in files.iter().zip(&bytes) {
            std::fs::write(&file.path, bytes).unwrap();
        }
        let root = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                source_cache_max_age: 0,
                ..Default::default()
            },
        );
        root.state()
            .native
            .load_fonts_from_paths(files.iter().map(|file| &file.path));
        missing(&root, "absent");
        let doc = FontCollection::for_document(&root, &Limits::default());
        let first = doc
            .register_sources(
                descriptor(),
                vec![FontSource::Local("Shared Regular".into())],
            )
            .unwrap();
        let first_bytes = doc.font_data(first).unwrap().data;
        let chosen = bytes
            .iter()
            .position(|bytes| bytes.as_slice() == first_bytes.as_ref())
            .unwrap();
        if rename {
            let replacement = files[chosen].directory.join("replacement.ttf");
            std::fs::write(&replacement, test_font("Renamed", &['a'], 700)).unwrap();
            std::fs::rename(replacement, &files[chosen].path).unwrap();
        } else {
            std::fs::remove_file(&files[chosen].path).unwrap();
        }
        let next_doc = FontCollection::for_document(&root, &Limits::default());
        for name in ["Shared Regular", "Shared-Regular"] {
            let alias = next_doc
                .register_sources(descriptor(), vec![FontSource::Local(name.into())])
                .unwrap();
            assert_eq!(
                next_doc.font_data(alias).unwrap().data.as_ref(),
                bytes[1 - chosen].as_slice()
            );
        }
    }
}

#[test]
fn selected_native_verification_bounds_name_decoding_like_the_index() {
    let file = NativeFile::new();
    let root = file.collection(0);
    missing(&root, "absent");
    // The root index decoded the same records under its own limit. The
    // caller's limit bounds the scratch of re-reading a path face's names.
    let doc = FontCollection::for_document(
        &root,
        &Limits {
            max_local_font_index_bytes: Some(64),
            ..Default::default()
        },
    );
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    limit_error(
        doc.register_sources(
            descriptor(),
            vec![
                FontSource::Local("Installed Regular".into()),
                FontSource::Data(test_font("Web", &['a'], 500), 0),
            ],
        ),
        LimitKind::LocalFontIndexBytes,
    );
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), 1);
    assert_eq!(doc.generation(), 0);
    let roomy = FontCollection::for_document(&root, &Limits::default());
    assert!(
        roomy
            .register_sources(
                descriptor(),
                vec![FontSource::Local("Installed Regular".into())]
            )
            .is_ok()
    );
}

#[test]
fn selected_native_verification_limit_stops_before_data_fallback() {
    let file = NativeFile::new();
    let root = file.collection(0);
    missing(&root, "absent");
    let doc = FontCollection::for_document(
        &root,
        &Limits {
            max_local_font_lookup_work: Some(0),
            ..Default::default()
        },
    );
    limit_error(
        doc.register_sources(
            descriptor(),
            vec![
                FontSource::Local("Installed Regular".into()),
                FontSource::Data(test_font("Web", &['a'], 500), 0),
            ],
        ),
        LimitKind::LocalFontLookupWork,
    );
    assert_eq!(doc.generation(), 0);
}

fn named_font(records: &[(u16, u16, &str)]) -> Vec<u8> {
    let mut name = vec![0, 0];
    name.extend_from_slice(&(records.len() as u16).to_be_bytes());
    name.extend_from_slice(&((6 + 12 * records.len()) as u16).to_be_bytes());
    let mut strings = Vec::new();
    for &(id, language, value) in records {
        let encoded: Vec<_> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for field in [
            3u16,
            1,
            language,
            id,
            encoded.len() as u16,
            strings.len() as u16,
        ] {
            name.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend(encoded);
    }
    name.extend(strings);
    let bytes = test_font("Records", &['a'], 500);
    let mut tables = crate::font::sfnt::read_sfnt_tables(&bytes);
    for (tag, data) in &mut tables {
        if tag == b"name" {
            *data = name.clone();
        }
    }
    crate::font::sfnt::build_sfnt(&tables)
}

#[test]
fn nonmatching_name_record_ids_are_charged_before_decoding() {
    let root = with_limits(&Limits {
        max_local_font_lookup_work: Some(128),
        ..Default::default()
    });
    let mut rows = vec![(0u16, 0x0409u16, "ignored"); 1000];
    rows.extend([
        (1, 0x0409, "Records"),
        (4, 0x0409, "Records Regular"),
        (6, 0x0409, "Records-Regular"),
    ]);
    root.register(named_font(&rows)).unwrap();
    let doc = FontCollection::for_document(&root, &Limits::default());
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(0));
    limit_error(
        doc.register_sources(
            descriptor(),
            vec![FontSource::Local("Records-Regular".into())],
        ),
        LimitKind::LocalFontLookupWork,
    );
    assert_eq!(doc.generation(), 0);
    let first = LOCAL_NAME_FACE_READS.with(std::cell::Cell::get);
    limit_error(
        doc.register_sources(descriptor(), vec![FontSource::Local("another-name".into())]),
        LimitKind::LocalFontLookupWork,
    );
    assert_eq!(LOCAL_NAME_FACE_READS.with(std::cell::Cell::get), first);
}

#[test]
fn localized_full_names_and_first_registered_face_are_preserved() {
    let root = shared();
    let first = root
        .register(named_font(&[
            (1, 0x0409, "Records"),
            (4, 0x0409, "Records Regular"),
            (4, 0x0411, "名前 レギュラー"),
            (6, 0x0409, "Records-Regular"),
        ]))
        .unwrap();
    root.register(test_font("Records", &['a'], 700)).unwrap();
    for name in ["Records Regular", "Records-Regular", "名前 レギュラー"] {
        let doc = FontCollection::for_document(&root, &Limits::default());
        let alias = doc
            .register_sources(descriptor(), vec![FontSource::Local(name.into())])
            .unwrap();
        assert_eq!(
            doc.font_data(alias).unwrap().data.id(),
            root.font_data(first).unwrap().data.id()
        );
    }
    missing(&root, "Records");
}

#[test]
fn disabled_input_limits_allow_a_long_source_suffix() {
    let root = with_limits(&Limits::unlimited());
    let mut sources = vec![FontSource::Data(test_font("Web", &['a'], 500), 0)];
    sources.extend((0..100).map(|_| FontSource::Local("absent".repeat(1024))));
    root.register_sources(descriptor(), sources).unwrap();
    assert_eq!(root.generation(), 1);
}
