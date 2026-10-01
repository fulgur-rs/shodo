use shodo::font::{FontCollection, FontFaceDescriptor, FontId, FontOptions, FontQuery, FontUnit};
use shodo::limits::Limits;
use shodo::style::{FontFamily, GenericFamily};

type FontKey = (u64, Option<u64>);

fn register(fonts: &FontCollection, family: &str) -> FontId {
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: family.into(),
                ..Default::default()
            },
        )
        .unwrap()
}

// Model a caller's retained selection: refresh it only when the public font
// key changes. A missing shared/document counter leaves a stale face here.
fn reused_ch(
    fonts: &FontCollection,
    query: &FontQuery,
    cached: &mut Option<(FontKey, FontUnit)>,
) -> FontUnit {
    let key = fonts.generations();
    if cached.as_ref().is_none_or(|(old, _)| *old != key) {
        *cached = Some((key, fonts.resolve_ch(query, 16.)));
    }
    cached.as_ref().unwrap().1
}

#[test]
fn document_reuse_key_tracks_shared_policies_and_document_registration() {
    let limits = Limits::default();
    let shared = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let a = register(&shared, "Family A");
    let b = register(&shared, "Family B");
    let document = FontCollection::for_document(&shared, &limits);
    let generic = FontQuery {
        families: vec![FontFamily::Generic(GenericFamily::SansSerif)],
        ..Default::default()
    };
    document.set_generic_families(GenericFamily::SansSerif, vec!["Family A".into()]);
    let mut generic_cache = None;
    assert_eq!(
        reused_ch(&document, &generic, &mut generic_cache).id,
        Some(a)
    );
    let before = document.generations();
    document.set_generic_families(GenericFamily::SansSerif, vec!["Family B".into()]);
    assert_eq!(document.generations(), (before.0 + 1, before.1));
    assert_eq!(document.generation(), 0);
    assert_eq!(
        reused_ch(&document, &generic, &mut generic_cache).id,
        Some(b)
    );

    let fallback = FontQuery {
        families: vec![],
        script: *b"Latn",
        ..Default::default()
    };
    document.set_fallback_families(*b"Latn", None, vec!["Family B".into()]);
    let mut fallback_cache = None;
    assert_eq!(
        reused_ch(&document, &fallback, &mut fallback_cache).id,
        Some(b)
    );
    let before = document.generations();
    document.set_fallback_families(*b"Latn", None, vec!["Family A".into()]);
    assert_eq!(document.generations(), (before.0 + 1, before.1));
    assert_eq!(document.generation(), 0);
    assert_eq!(
        reused_ch(&document, &fallback, &mut fallback_cache).id,
        Some(a)
    );

    let shared_before = shared.generations();
    let before = document.generations();
    let local = register(&document, "Family A");
    assert_eq!(document.generations(), (before.0, Some(1)));
    assert_eq!(shared.generations(), shared_before);
    assert_eq!(
        reused_ch(&document, &fallback, &mut fallback_cache).id,
        Some(local)
    );
}
