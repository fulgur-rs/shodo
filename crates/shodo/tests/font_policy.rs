use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;

#[test]
fn document_policy_follows_the_shared_root_across_forks_and_clones() {
    let limits = Limits::default();
    for (system_fonts, bundled_only) in [(false, true), (true, false)] {
        let shared = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts,
                ..Default::default()
            },
        );
        let document = FontCollection::for_document(&shared, &limits);
        let sibling = FontCollection::for_document(&document, &limits);
        for fonts in [shared.clone(), document, sibling] {
            assert_eq!(fonts.is_bundled_only(), bundled_only);
        }
    }
}

#[test]
fn default_policy_follows_the_system_fonts_feature() {
    let fonts = FontCollection::new(&Limits::default());
    assert_eq!(fonts.is_bundled_only(), !cfg!(feature = "system-fonts"));
}

#[test]
fn whole_pixel_metrics_follow_the_shared_root_and_round_every_face() {
    let limits = Limits::default();
    let font = include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec();
    // The Latin fixture has hhea extents 1069/293 units at UPEM 1000, so at
    // 10px its ascent is 10.69px and its descent 2.93px.
    for (whole_pixel_metrics, expected) in [(false, (10.69, 2.93)), (true, (11.0, 3.0))] {
        let shared = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                whole_pixel_metrics,
                ..Default::default()
            },
        );
        let document = FontCollection::for_document(&shared, &limits);
        let id = document
            .register_face(
                font.clone(),
                0,
                shodo::font::FontFaceDescriptor {
                    family: "Fixture".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let metrics = document.metrics(id, 10.0);
        assert!(
            (metrics.ascent - expected.0).abs() < 1e-3
                && (metrics.descent - expected.1).abs() < 1e-3,
            "{whole_pixel_metrics}: {metrics:?}"
        );
    }
}
