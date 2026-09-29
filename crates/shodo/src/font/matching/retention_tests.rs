use super::*;
fn reloaded(source: SourceId) -> Candidate {
    let blob = super::super::Blob::from(super::super::browser_tests::test_font(
        "Native",
        &['a'],
        600,
    ));
    let info = FontInfo::from_source(SourceInfo::new(source, SourceKind::Memory(blob.clone())), 0)
        .unwrap();
    Candidate {
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
        .best_match(vec![reloaded(source)], &FontQuery::default(), "a", true)
        .unwrap();
    let blob = fonts.state().faces[1].data.id();
    for _ in 0..3 {
        let next = fonts
            .best_match(vec![reloaded(source)], &FontQuery::default(), "a", true)
            .unwrap();
        assert_eq!(next.id, first.id);
        assert_eq!(fonts.state().faces.len(), 2);
        assert_eq!(fonts.state().faces[1].data.id(), blob);
    }
    assert!(fonts.state().blob_bytes > 0);
}
#[test]
fn platform_retention_obeys_face_and_blob_limits() {
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
        assert!(
            fonts
                .best_match(
                    vec![reloaded(SourceId::new())],
                    &FontQuery::default(),
                    "a",
                    true
                )
                .is_none()
        );
        assert_eq!(fonts.state().faces.len(), 1);
        assert_eq!(fonts.state().blob_bytes, 0);
    }
}
