//! Retained representative raikiri caller checks and diagnostic entry point.
#[allow(dead_code)]
#[path = "support/raikiri_contracts.rs"]
mod caller;
#[path = "support/caller_measure.rs"]
mod measure;
#[allow(dead_code)]
#[path = "support/retained_caller.rs"]
mod retained;
#[allow(dead_code)]
#[path = "support/caller_snapshot.rs"]
mod snapshot;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    measure::run().map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{caller, retained::RetainedCaller};
    use shodo::Fragment;
    const CSS: &str =
        "#root{font-family:'Shodo Fixture Latin';font-size:16px}#root::first-line{font-size:32px}";
    const BODY: &str = "<span>f</span><a href='/target'>f</a><span>i</span>";
    fn input(body: &str, css: &str) -> caller::ResolvedInput {
        caller::resolve_html(
            &format!("<style>{css}</style><div id=root>{body}</div>"),
            "root",
        )
        .unwrap()
    }
    fn ignore_slice_offsets(lines: &mut [serde_json::Value]) {
        for line in lines {
            for fragment in line["fragments"].as_array_mut().unwrap() {
                fragment.as_object_mut().unwrap().remove("slice_offset");
            }
        }
    }
    #[test]
    fn unchanged_inputs_retain_prepared_paragraph_after_literal_glyph_and_link_guards() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let mut s = RetainedCaller::new(
            input(BODY, CSS),
            fonts.collection.clone(),
            None,
            caller::FontPolicy::FixtureLatin,
        );
        let out = s.layout_width(400.0).unwrap();
        assert_eq!(out.lines.len(), 1);
        assert_eq!(out.lines[0].text(), "ffi");
        let run = out.lines[0]
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(run.font_size(), 32.0);
        assert_eq!(run.glyphs().map(|g| g.id).collect::<Vec<_>>(), [367]);
        assert_eq!(out.links.len(), 1);
        assert_eq!(out.links[0].text, 1..2);
        assert!((out.links[0].rect.inline_size - 10.112).abs() < 1.0 / 64.0);
        let id = s.paragraph_id().unwrap();
        s.layout_width(80.0).unwrap();
        assert_eq!(
            s.paragraph_id(),
            Some(id),
            "width-only changes must retain prepared paragraph"
        );
    }
    #[test]
    fn width_sequence_matches_fresh_first_line_transform_and_source_links() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let body =
            "<a href='/one'>Straße ffi </a><span>abc def ghi</span><br><a href='/two'>later</a>";
        let css = format!("{CSS}#root{{text-transform:uppercase}}");
        let oracle = input(body, &css);
        let mut s = RetainedCaller::new(
            input(body, &css),
            fonts.collection.clone(),
            None,
            caller::FontPolicy::FixtureLatin,
        );
        for width in [400.0, 50.0, 120.0, 400.0] {
            let out = s.layout_width(width).unwrap();
            let fresh = caller::layout(&oracle, &fonts.collection, width).unwrap();
            assert_eq!(
                super::snapshot::output(&out),
                super::snapshot::output(&fresh)
            );
            assert!(!out.links.is_empty());
        }
    }
    #[test]
    fn input_content_normal_style_and_first_line_replacements_invalidate_old_tokens() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let mut s = RetainedCaller::new(
            input(BODY, CSS),
            fonts.collection.clone(),
            None,
            caller::FontPolicy::FixtureLatin,
        );
        s.layout_width(400.0).unwrap();
        for (body, css) in [
            ("<a href='/new'>hello</a>", CSS),
            (
                BODY,
                "#root{font-family:'Shodo Fixture Latin';font-size:24px;color:blue}",
            ),
            (
                BODY,
                "#root{font-family:'Shodo Fixture Latin';font-size:16px}#root::first-line{font-size:40px}",
            ),
        ] {
            let old = s.start_token().unwrap();
            let id = s.paragraph_id().unwrap();
            s.replace_input(input(body, css));
            let out = s.layout_width(400.0).unwrap();
            let fresh = caller::layout(&input(body, css), &fonts.collection, 400.0).unwrap();
            assert_eq!(
                super::snapshot::output(&out),
                super::snapshot::output(&fresh)
            );
            assert_ne!(s.paragraph_id(), Some(id));
            assert!(matches!(
                s.next_line(old, &shodo::LineConstraint::new(400.0))
                    .unwrap(),
                shodo::LineResult::InvalidToken
            ));
        }
    }
    #[test]
    fn registration_in_either_layer_rebuilds_and_invalidates_old_tokens() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let doc = shodo::font::FontCollection::for_document(&fonts.collection, &Default::default());
        let mut s = RetainedCaller::new(
            input(BODY, CSS),
            fonts.collection.clone(),
            Some(doc.clone()),
            caller::FontPolicy::FixtureLatin,
        );
        s.layout_width(400.0).unwrap();
        for layer in [&fonts.collection, &doc] {
            let id = s.paragraph_id().unwrap();
            let old = s.start_token().unwrap();
            let generation = layer.generation();
            layer
                .register(shodo_fixtures::font("latin").unwrap().bytes.to_vec())
                .unwrap();
            assert_eq!(layer.generation(), generation + 1);
            let out = s.layout_width(400.0).unwrap();
            let fresh = caller::layout(&input(BODY, CSS), &doc, 400.0).unwrap();
            assert_eq!(
                super::snapshot::output(&out),
                super::snapshot::output(&fresh)
            );
            assert_ne!(s.paragraph_id(), Some(id), "font generation changed");
            assert!(matches!(
                s.next_line(old, &shodo::LineConstraint::new(400.0))
                    .unwrap(),
                shodo::LineResult::InvalidToken
            ));
        }
    }
    #[test]
    fn equal_generation_different_font_collection_must_rebuild() {
        let a = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let b = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        assert_eq!(a.collection.generation(), b.collection.generation());
        assert_ne!(
            a.collection.layer_handle().id(),
            b.collection.layer_handle().id()
        );
        let mut s = RetainedCaller::new(
            input(BODY, CSS),
            a.collection,
            None,
            caller::FontPolicy::FixtureLatin,
        );
        s.layout_width(400.0).unwrap();
        let id = s.paragraph_id().unwrap();
        s.replace_fonts(b.collection.clone(), None);
        let out = s.layout_width(400.0).unwrap();
        let fresh = caller::layout(&input(BODY, CSS), &b.collection, 400.0).unwrap();
        assert_eq!(
            super::snapshot::output(&out),
            super::snapshot::output(&fresh)
        );
        assert_ne!(s.paragraph_id(), Some(id));
    }
    #[test]
    fn height_changes_retry_same_token_and_keep_prepared_first_line_data() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let body = "<a href='/first'>ffi</a><br><a href='/later'>abc</a><br>def";
        let mut s = RetainedCaller::new(
            input(body, CSS),
            fonts.collection.clone(),
            None,
            caller::FontPolicy::FixtureLatin,
        );
        let flat = s.layout_width(400.0).unwrap();
        assert_eq!(flat.lines.len(), 3);
        assert!(flat.lines[0].block_size() > 25.0);
        let id = s.paragraph_id().unwrap();
        let page = s.layout_height(400.0, 25.0).unwrap();
        assert_eq!(
            page.pages.len(),
            3,
            "height must change actual page boundaries"
        );
        assert_eq!(page.oversize_lines, 1);
        assert!(page.height_retries > 0);
        assert!(page.pages.iter().all(|p| p.lines.len() == 1));
        let mut expected = super::snapshot::output(&flat)["lines"]
            .as_array()
            .unwrap()
            .clone();
        let mut actual = page
            .pages
            .iter()
            .flat_map(|p| {
                super::snapshot::output(p)["lines"]
                    .as_array()
                    .unwrap()
                    .clone()
            })
            .collect::<Vec<_>>();
        for l in expected.iter_mut().chain(actual.iter_mut()) {
            let line = l.as_object_mut().unwrap();
            line.remove("offset");
        }
        ignore_slice_offsets(&mut expected);
        ignore_slice_offsets(&mut actual);
        assert_eq!(
            actual, expected,
            "accepted lines match independent fresh-width content/geometry"
        );
        let repeat = s.layout_height(400.0, 25.0).unwrap();
        assert_eq!(
            page.page_tokens, repeat.page_tokens,
            "same paragraph/page cuts retain exact tokens"
        );
        assert_eq!(s.paragraph_id(), Some(id));
        let tall = s.layout_height(400.0, 100.0).unwrap();
        assert_eq!(tall.pages.len(), 1);
        assert_eq!(tall.oversize_lines, 0);
        let mut tall_output = super::snapshot::output(&tall.pages[0]);
        let mut flat_output = super::snapshot::output(&flat);
        ignore_slice_offsets(tall_output["lines"].as_array_mut().unwrap());
        ignore_slice_offsets(flat_output["lines"].as_array_mut().unwrap());
        assert_eq!(
            tall_output, flat_output,
            "streamed page lines intentionally omit the break_all slice coordinates"
        );
        let zero = s.layout_height(400.0, 0.0).unwrap();
        assert_eq!(zero.pages.len(), 3);
        assert_eq!(zero.oversize_lines, 3);
        assert_eq!(s.paragraph_id(), Some(id));
    }
    #[test]
    fn invalid_replacement_never_exposes_old_preparation_and_can_recover() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let mut s = RetainedCaller::new(
            input(BODY, CSS),
            fonts.collection.clone(),
            None,
            caller::FontPolicy::FixtureLatin,
        );
        s.layout_width(400.0).unwrap();
        let old = s.start_token().unwrap();
        s.replace_input(input(
            "<span style='position:absolute'>ffi</span>",
            "#root{font-family:'Shodo Fixture Latin';font-size:16px}",
        ));
        assert!(s.layout_width(400.0).is_err());
        assert_eq!(s.paragraph_id(), None);
        s.replace_input(input(BODY, CSS));
        let out = s.layout_width(400.0).unwrap();
        assert_eq!(out.lines[0].text(), "ffi");
        assert!(matches!(
            s.next_line(old, &shodo::LineConstraint::new(400.0))
                .unwrap(),
            shodo::LineResult::InvalidToken
        ));
    }
}
