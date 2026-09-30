//! Representative raikiri caller: resolved first-line inputs and source links.
#[allow(dead_code)] // Shared caller has separate fixture and WPT entry points.
#[path = "support/raikiri_contracts.rs"]
mod caller;

const CSS: &str = "#root{font-family:'Shodo Fixture Latin';font-size:16px;color:black}#link{color:blue;text-decoration-line:underline;text-decoration-color:lime;text-decoration-thickness:2px}";
const FIRST: &str = "#root::first-line{font-size:32px;color:red}";
const BODY: &str = "<span id=owner>f</span><a id=link href='/target'>f</a><span>i</span>";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "raikiri-contracts".into());
    let directory = std::path::Path::new(&directory);
    let input = caller::resolve_html(
        &format!("<style>{CSS}{FIRST}</style><div id=root>{BODY}<br><a href='/later'>ß</a></div>"),
        "root",
    )?;
    let fonts = shodo_fixtures::load_fonts(&Default::default())?;
    let output = caller::layout(&input, &fonts.collection, 400.0)?;
    let (image, count) = caller::paint(&output)?;
    let lines: Vec<_> = output
        .lines
        .iter()
        .map(|line| {
            let runs: Vec<_> = line
                .fragments()
                .filter_map(|fragment| match fragment {
                    shodo::Fragment::GlyphRun(run) => Some(serde_json::json!({
                        "owner": run.node().map(|id| id.0),
                        "text": run.text_range(), "font_size": run.font_size(),
                        "color": run.paint_style().color,
                        "glyph_ids": run.glyphs().map(|g| g.id).collect::<Vec<_>>()
                    })),
                    _ => None,
                })
                .collect();
            serde_json::json!({"text": &line.text()[line.text_range()], "runs": runs})
        })
        .collect();
    let links: Vec<_> = output.links.iter().map(|r| {
        let hit = output.link_at(r.rect.inline_start + r.rect.inline_size * 0.5,
            r.rect.block_start + r.rect.block_size * 0.5);
        serde_json::json!({"line": r.line, "text_node": r.node.0,
            "link_element": r.link.element.0, "href": r.link.href,
            "dom": r.dom, "text": r.text, "mapping_kind": format!("{:?}", r.kind),
            "logical_rect": [r.rect.inline_start, r.rect.block_start, r.rect.inline_size, r.rect.block_size],
            "midpoint_hit": hit.map(|link| &link.href)})
    }).collect();
    std::fs::create_dir_all(directory)?;
    image.save_png(directory.join("caller.png"))?;
    std::fs::write(
        directory.join("caller.json"),
        serde_json::to_string_pretty(
            &serde_json::json!({"first_line_supplier": "raikiri cascade_with_first_line; real CSS ::first-line",
            "png_margin": 10, "drawn_glyphs": count, "lines": lines, "links": links}),
        )? + "\n",
    )?;
    println!(
        "{}: {} lines, {} source link regions, {count} drawn glyphs",
        directory.display(),
        output.lines.len(),
        output.links.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BODY, CSS, FIRST, caller};
    use shodo::{Fragment, mapping::MappingKind};

    fn input(body: &str, extra: &str, first: Option<&str>) -> caller::ResolvedInput {
        caller::resolve_html(
            &format!(
                "<style>{CSS}{extra}{}</style><div id=root>{body}</div>",
                first.unwrap_or_default()
            ),
            "root",
        )
        .unwrap()
    }

    fn first_run(line: &shodo::Line) -> shodo::GlyphRunView<'_> {
        line.fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap()
    }

    fn pixel(image: &tiny_skia::Pixmap, x: usize, y: usize) -> [u8; 4] {
        image.data()[4 * (y * image.width() as usize + x)..][..4]
            .try_into()
            .unwrap()
    }

    #[test]
    fn real_css_first_line_reaches_glyph_paint_and_mapping() {
        let input = input(BODY, "", Some(FIRST));
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.lines.len(), 1);
        let run = first_run(&output.lines[0]);
        assert_eq!(run.font_size(), 32.0);
        assert_eq!(run.font(), fonts.ids[0]);
        assert_eq!(run.paint_style().color, [255, 0, 0, 255]);
        assert_eq!(run.text_range(), 0..3);
        assert_eq!(run.glyphs().len(), 1);
        let (image, count) = caller::paint(&output).unwrap();
        assert_eq!(count, 1, "shared glyph must be drawn once");
        // Independent Latin hhea ascent1069/UPEM1000, post position-100:
        // 32px baseline34.203125, underline+3.2, canvas margin10 => y47.
        // Raw GDEF ffi carets315/631: middle source spans x20.08..30.192.
        assert_eq!(pixel(&image, 24, 47), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, 12, 47), [255; 4]);
        assert_eq!(pixel(&image, 33, 47), [255; 4]);
    }

    #[test]
    fn middle_link_uses_mapping_units_instead_of_the_glyph_owner_range() {
        let input = input(BODY, "", Some(FIRST));
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.links.len(), 1);
        let region = &output.links[0];
        assert_eq!(region.link.href, "/target");
        assert_eq!(first_run(&output.lines[0]).glyphs().next().unwrap().id, 367);
        assert_eq!(region.dom, 0..1);
        assert_eq!(region.text, 1..2);
        assert_ne!(
            region.node, region.link.element,
            "text ID differs from element ID"
        );
        assert_ne!(Some(region.node), first_run(&output.lines[0]).node());
        assert!((region.rect.inline_start - 10.08).abs() < 1.0 / 64.0);
        assert!((region.rect.inline_size - 10.112).abs() < 1.0 / 64.0);
        let hit = output
            .link_at(
                region.rect.inline_start + region.rect.inline_size * 0.5,
                region.rect.block_start + region.rect.block_size * 0.5,
            )
            .unwrap();
        assert_eq!(hit.href, "/target");
        assert!(output.link_at(1.0, region.rect.block_start + 1.0).is_none());
    }

    #[test]
    fn explicit_equal_child_and_inherited_child_keep_distinct_first_line_inputs() {
        let input = input(
            "<span style='font-size:16px'>a</span><span>b</span>",
            "",
            Some(FIRST),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        let sizes: Vec<_> = output.lines[0]
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r.font_size()),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, [16.0, 32.0]);
    }

    #[test]
    fn every_accepted_line_uses_its_own_transformed_mapping() {
        let input = input(
            "<a href='/first'>ß</a><br><a href='/later'>ß</a>",
            "a{color:inherit}",
            Some("#root::first-line{font-size:32px;color:red;text-transform:uppercase}"),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.lines.len(), 2);
        let text = |line: &shodo::Line| {
            line.text()[line.text_range()]
                .trim_end_matches('\n')
                .to_owned()
        };
        assert_eq!(text(&output.lines[0]), "SS");
        assert_eq!(text(&output.lines[1]), "ß");
        assert_eq!(first_run(&output.lines[0]).font_size(), 32.0);
        assert_eq!(first_run(&output.lines[1]).font_size(), 16.0);
        assert_eq!(
            first_run(&output.lines[0]).paint_style().color,
            [255, 0, 0, 255]
        );
        assert_eq!(
            first_run(&output.lines[1]).paint_style().color,
            [0, 0, 0, 255]
        );
        assert_eq!(output.links.len(), 2);
        assert_eq!(output.links[0].line, 0);
        assert_eq!(output.links[0].kind, MappingKind::Expanded);
        assert_eq!(output.links[0].dom, 0..2);
        assert_eq!(output.links[0].text, 0..2);
        assert_eq!(output.links[0].link.href, "/first");
        assert_eq!(output.links[1].line, 1);
        assert_eq!(output.links[1].kind, MappingKind::Identity);
        assert_eq!(output.links[1].link.href, "/later");
        assert!(
            output.links[1].rect.block_start
                >= output.links[0].rect.block_start + output.links[0].rect.block_size
        );
    }

    #[test]
    fn one_text_nodes_source_ranges_are_clipped_to_each_line() {
        let input = input("<a href='/split'>a\nb</a>", "#root{white-space:pre}", None);
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.lines.len(), 2);
        assert_eq!(output.links.len(), 2);
        assert_eq!(output.links[1].dom, 2..3);
        assert_eq!(output.links[1].text, 2..3);
        assert_eq!(output.links[0].node, output.links[1].node);
    }

    #[test]
    fn collapsed_link_space_has_no_phantom_click_region() {
        let input = input(
            "<span>a </span><a href='/collapsed'> </a><span>b</span>",
            "",
            None,
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert!(output.links.is_empty());
        assert!(
            output.lines[0]
                .offset_mapping()
                .unwrap()
                .units()
                .iter()
                .any(|u| u.kind == MappingKind::Collapsed)
        );
    }

    #[test]
    fn nested_link_children_retain_distinct_sources_and_inherited_underline() {
        let input = input(
            "<span>f</span><a id=link href='/nested'><span>f</span><span>i</span></a>",
            "",
            Some(FIRST),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.links.len(), 2);
        assert_ne!(output.links[0].node, output.links[1].node);
        assert_eq!(output.links[0].link, output.links[1].link);
        assert_eq!(output.links[0].text, 1..2);
        assert_eq!(output.links[1].text, 2..3);
        let (image, count) = caller::paint(&output).unwrap();
        assert_eq!(count, 1);
        assert_eq!(pixel(&image, 24, 47), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, 33, 47), [0, 255, 0, 255]);
        assert_eq!(pixel(&image, 12, 47), [255; 4]);
    }

    #[test]
    fn empty_first_line_leaves_following_text_normal() {
        let input = input(
            "<br><a href='/later'>ß</a>",
            "a{color:inherit}",
            Some("#root::first-line{font-size:32px;color:red;text-transform:uppercase}"),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(output.lines.len(), 2);
        assert_eq!(
            output.lines[1].text()[output.lines[1].text_range()].trim(),
            "ß"
        );
        assert_eq!(first_run(&output.lines[1]).font_size(), 16.0);
        assert_eq!(
            first_run(&output.lines[1]).paint_style().color,
            [0, 0, 0, 255]
        );
    }

    #[test]
    fn wrapped_single_text_node_preserves_later_source_ranges() {
        let input = input(
            "<a href='/wrap'>ß a b c d</a>",
            "a{color:inherit}",
            Some("#root::first-line{font-size:32px;color:red;text-transform:uppercase}"),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 50.0).unwrap();
        assert!(output.lines.len() > 1);
        assert_eq!(first_run(&output.lines[0]).font_size(), 32.0);
        for line in &output.lines[1..] {
            assert_eq!(first_run(line).font_size(), 16.0);
            assert_eq!(first_run(line).paint_style().color, [0, 0, 0, 255]);
        }
        assert!(output.links.iter().all(|r| r.node == output.links[0].node));
        assert_eq!(output.links[0].kind, MappingKind::Expanded);
        assert!(
            output
                .links
                .iter()
                .filter(|r| r.line > 0)
                .all(|r| r.kind == MappingKind::Identity)
        );
    }

    #[test]
    fn first_line_box_declarations_do_not_change_structure() {
        let input = input(
            BODY,
            "",
            Some("#root::first-line{display:none;font-size:32px}#link::first-line{display:block}"),
        );
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = caller::layout(&input, &fonts.collection, 400.0).unwrap();
        assert_eq!(first_run(&output.lines[0]).font_size(), 32.0);
    }

    #[test]
    fn unsupported_first_line_values_are_rejected() {
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        for css in [
            "opacity:.5",
            "background-color:red",
            "font-style:italic",
            "text-transform:full-width",
        ] {
            let input = input(
                "<span>a</span>",
                "",
                Some(&format!("#root::first-line{{{css}}}")),
            );
            assert!(
                caller::layout(&input, &fonts.collection, 400.0).is_err(),
                "{css}"
            );
        }
    }
}
