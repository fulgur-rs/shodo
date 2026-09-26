use std::collections::HashSet;

use sha2::{Digest, Sha256};
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::style::LineOptions;
use shodo::{AtomicSizes, LayoutContext};
use shodo_fixtures::{FONTS, case, cases, font, load_fonts};
use skrifa::{FontRef, MetadataProvider};

#[test]
fn manifest_bytes_and_selected_faces_are_fixed() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../assets/manifest.json")).unwrap();
    for fixture in FONTS {
        let entry = manifest["fonts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == fixture.id)
            .unwrap();
        assert_eq!(fixture.face_index, 0);
        assert_eq!(
            format!("{:x}", Sha256::digest(fixture.bytes)),
            fixture.sha256
        );
        assert_eq!(entry["sha256"], fixture.sha256);
        assert_eq!(entry["family"], fixture.family);
        assert_eq!(entry["size"], fixture.bytes.len());
        assert_eq!(entry["face_index"], fixture.face_index);
        assert!(FontRef::from_index(fixture.bytes, fixture.face_index).is_ok());
        assert!(font(fixture.id).is_some());
    }
    assert!(font("not-a-font").is_none());
}

#[test]
fn configured_font_chains_cover_every_visible_sample_character() {
    for case in cases() {
        let fonts: Vec<_> = case
            .font_ids
            .iter()
            .map(|id| {
                let fixture = font(id).unwrap();
                FontRef::from_index(fixture.bytes, fixture.face_index).unwrap()
            })
            .collect();
        for ch in case
            .text
            .chars()
            .filter(|ch| !matches!(ch, '\n' | '\t' | '\u{ad}'))
        {
            assert!(
                fonts
                    .iter()
                    .any(|f| f.charmap().map(ch).is_some_and(|id| id.to_u32() != 0)),
                "{} missing U+{:04X}",
                case.id,
                ch as u32
            );
        }
    }
}

#[test]
fn corpus_ids_and_configuration_are_stable() {
    let all = cases();
    assert_eq!(all.len(), 12);
    let ids: HashSet<_> = all.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), all.len());
    assert_eq!(case("arabic-short").unwrap().direction, Direction::Rtl);
    assert_eq!(case("japanese-short").unwrap().lang.as_deref(), Some("ja"));
    assert!(case("unknown").is_none());
    for c in all {
        assert!(c.width.is_finite() && c.width > 0. && c.font_size > 0.);
    }
}

#[test]
fn public_font_and_layout_consumers_use_the_same_fixture_set() {
    let limits = Limits::default();
    let fixtures = load_fonts(&limits).unwrap();
    for (fixture, id) in FONTS.iter().zip(fixtures.ids) {
        let bytes = fixtures.collection.font_data(id).unwrap();
        assert_eq!(bytes.data.as_ref(), fixture.bytes);
        assert_eq!(
            fixtures.collection.face_descriptor(id).unwrap().family,
            fixture.family
        );
        let metrics = fixtures
            .collection
            .metrics_with_coords(id, 16., &[])
            .unwrap();
        assert!(metrics.ascent > 0. && metrics.descent > 0.);
        assert!(fixtures.collection.shaper_data(id).is_some());
    }
    let mut cx = LayoutContext::new();
    for c in cases() {
        let paragraph = c.build(&mut cx, &fixtures, &limits).unwrap();
        assert!(!paragraph.text().is_empty());
        let lines = paragraph.break_all(
            &mut cx,
            &LineOptions::default(),
            c.width,
            &AtomicSizes::new(),
        );
        assert!(!lines.is_empty(), "{}", c.id);
    }
}

#[test]
fn arabic_subset_preserves_contextual_substitution_and_mark_placement() {
    let fixtures = load_fonts(&Limits::default()).unwrap();
    let data = fixtures.collection.font_data(fixtures.ids[2]).unwrap();
    let font = harfrust::FontRef::from_index(data.data.as_ref(), data.index).unwrap();
    let shared = fixtures.collection.shaper_data(fixtures.ids[2]).unwrap();
    let shaper = shared.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    let text = "السَّلَامُ";
    buffer.push_str(text);
    buffer.set_direction(harfrust::Direction::RightToLeft);
    buffer.set_script(harfrust::script::ARABIC);
    buffer.set_language("ar".parse().unwrap());
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    assert!(!shaped.glyph_infos().is_empty());
    assert!(shaped.glyph_infos().iter().all(|g| g.glyph_id != 0));
    let nominal: HashSet<_> = skrifa::FontRef::from_index(data.data.as_ref(), data.index)
        .unwrap()
        .charmap()
        .mappings()
        .map(|(_, glyph)| glyph.to_u32())
        .collect();
    assert!(
        shaped
            .glyph_infos()
            .iter()
            .any(|glyph| !nominal.contains(&glyph.glyph_id)),
        "contextual forms were removed"
    );
    assert!(
        shaped
            .glyph_positions()
            .iter()
            .any(|position| position.x_advance == 0
                && (position.x_offset != 0 || position.y_offset != 0)),
        "mark attachment was removed"
    );
    assert!(
        shaped
            .glyph_infos()
            .windows(2)
            .all(|g| g[0].cluster >= g[1].cluster)
    );
}
