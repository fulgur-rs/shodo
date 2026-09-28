use sha2::{Digest, Sha256};
use shodo::font::{FontPresentation, FontQuery};
use shodo::limits::Limits;
use shodo_fixtures::{EMOJI_FONTS, FONTS, emoji_cases, font, load_emoji_fonts, load_fonts};
use skrifa::{FontRef, MetadataProvider, raw::TableProvider};

#[test]
fn opt_in_fonts_shape_real_color_and_text_faces() {
    // Failure to load the real bytes must never masquerade as fixture coverage.
    let color = font("emoji-color").expect("real color emoji fixture missing");
    let mono = font("emoji-mono").expect("real monochrome emoji fixture missing");
    let limits = Limits::default();
    let fixtures = load_emoji_fonts(&limits).unwrap();
    let fonts = &fixtures.base;
    let ids = fixtures.emoji_ids;
    for fixture in [color, mono] {
        assert_eq!(
            format!("{:x}", Sha256::digest(fixture.bytes)),
            fixture.sha256
        );
        let face = FontRef::new(fixture.bytes).unwrap();
        assert!(face.charmap().map('😀').is_some());
    }
    let color_face = FontRef::new(color.bytes).unwrap();
    assert!(color_face.cbdt().is_ok() && color_face.cblc().is_ok());
    assert!(FontRef::new(mono.bytes).unwrap().glyf().is_ok());
    for (text, presentation, id) in [
        ("😀", FontPresentation::Auto, ids[0]),
        ("☺︎", FontPresentation::Auto, ids[1]),
        ("☺️", FontPresentation::Auto, ids[0]),
        ("😀", FontPresentation::Text, ids[1]),
        ("☺︎", FontPresentation::Emoji, ids[0]),
    ] {
        let matched = fonts
            .collection
            .match_cluster(
                &FontQuery {
                    families: vec![shodo::style::FontFamily::Named(color.family.into())],
                    presentation,
                    ..Default::default()
                },
                text,
            )
            .unwrap();
        assert_eq!(matched.id, id, "{text:?} {presentation:?}");
    }
}

#[test]
fn opt_in_corpus_retains_selected_font_bytes_without_missing_glyphs() {
    let limits = Limits::default();
    let fonts = load_emoji_fonts(&limits).unwrap();
    let mut cx = shodo::LayoutContext::new();
    for case in emoji_cases() {
        let paragraph = case.build(&mut cx, &fonts.base, &limits).unwrap();
        assert!(
            paragraph.warnings().is_empty(),
            "{} {:?}",
            case.id,
            paragraph.warnings()
        );
        let lines = paragraph.break_all(
            &mut cx,
            &shodo::style::LineOptions::default(),
            case.width,
            &shodo::AtomicSizes::new(),
        );
        let mut saw_glyph = false;
        for line in &lines {
            for fragment in line.fragments() {
                let shodo::Fragment::GlyphRun(run) = fragment else {
                    continue;
                };
                let data = run.font_data().unwrap();
                let fixture = FONTS
                    .iter()
                    .chain(EMOJI_FONTS)
                    .find(|font| font.bytes == data.data.as_ref())
                    .expect("accepted font bytes");
                assert_eq!(data.index, fixture.face_index);
                for glyph in run.glyphs() {
                    assert_ne!(glyph.id, 0, "{}", case.id);
                    saw_glyph = true;
                }
            }
        }
        assert!(saw_glyph, "{}", case.id);
    }
}

#[test]
fn base_loader_and_original_registration_order_remain_usable() {
    let base = load_fonts(&Limits::default()).unwrap();
    for (fixture, id) in FONTS.iter().zip(base.ids) {
        assert_eq!(
            base.collection.font_data(id).unwrap().data.as_ref(),
            fixture.bytes
        );
    }
    assert!(
        base.collection
            .match_cluster(&FontQuery::default(), "😀")
            .is_none()
    );
    let emoji = load_emoji_fonts(&Limits::default()).unwrap();
    let selected = emoji
        .base
        .collection
        .match_cluster(&FontQuery::default(), "😀")
        .unwrap();
    assert_eq!(selected.id, emoji.emoji_ids[0]);
}
