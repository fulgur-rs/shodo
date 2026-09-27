use shodo::{LayoutContext, LineConstraint, LineResult};
use shodo_fixtures::{browser, load_fonts};

#[test]
fn utf16_positions_reject_surrogate_interiors_and_recover_source_bytes() {
    let text = "A𠮷é";
    for (units, bytes) in [(0, 0), (1, 1), (3, 5), (4, 7)] {
        assert_eq!(browser::utf16_to_utf8(text, units), Some(bytes));
        assert_eq!(browser::utf8_to_utf16(text, bytes), Some(units));
    }
    assert_eq!(browser::utf16_to_utf8(text, 2), None);
    assert_eq!(browser::utf16_to_utf8(text, 5), None);
    assert_eq!(browser::utf8_to_utf16(text, 2), None);
}

#[test]
fn materialized_priority_inputs_build_and_map_nested_and_atomic_sources() {
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let cases = browser::cases();
    assert!(cases.len() >= 54);
    for id in [
        "nested-inline",
        "color-ffi",
        "arabic-wrap",
        "pre-wrap-tab",
        "japanese-punctuation",
        "atomic-baseline",
        "supplementary",
    ] {
        let case = cases.iter().find(|c| c.id == id).unwrap();
        let built = browser::build(case, &mut cx, &fonts).unwrap();
        let LineResult::Line(line) = built.paragraph.next_line(
            &mut cx,
            built.paragraph.start_token(),
            &Default::default(),
            &LineConstraint::new(10000.0),
            &built.atomics,
        ) else {
            panic!("{id}: no actual line")
        };
        if id != "pre-wrap-tab" {
            assert_eq!(
                browser::source_end(case, &line).unwrap(),
                case.text().len(),
                "{id}"
            );
        }
        if id == "color-ffi" {
            let glyphs: usize = line
                .fragments()
                .filter_map(|f| {
                    if let shodo::Fragment::GlyphRun(r) = f {
                        Some(r.glyphs().len())
                    } else {
                        None
                    }
                })
                .sum();
            assert!(
                glyphs < case.text().chars().count(),
                "actual shared ligature"
            );
        }
    }
}
