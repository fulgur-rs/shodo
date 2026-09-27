#[path = "../examples/support/snapshot_cases.rs"]
mod snapshot_cases;
use serde_json::Value;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().unwrap().as_slice()
}

#[test]
fn fixed_matrix_paints_real_glyphs_on_the_declared_canvas() {
    let ids = snapshot_cases::case_ids();
    assert_eq!(ids.len(), 21);
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        21
    );
    for id in &ids {
        let rendered = snapshot_cases::render(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(rendered.id, *id);
        assert_eq!(
            (rendered.image.width(), rendered.image.height()),
            (512, 1024)
        );
        assert!(rendered.glyph_count > 0, "{id}");
        assert_eq!(
            array(&rendered.geometry, "glyphs").len(),
            rendered.glyph_count
        );
        assert!(
            rendered
                .image
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| *p != [255, 255, 255, 255])
        );
        if let Some(directory) = std::env::var_os("SHODO_SNAPSHOT_REVIEW_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            rendered
                .image
                .save_png(directory.join(format!("{id}.png")))
                .unwrap();
            std::fs::write(
                directory.join(format!("{id}.geometry.json")),
                serde_json::to_vec_pretty(&rendered.geometry).unwrap(),
            )
            .unwrap();
        }
    }
    assert!(snapshot_cases::render("unknown").is_err());
    assert_eq!(
        snapshot_cases::conditions()["canvas"],
        serde_json::json!([512, 1024])
    );
}

#[test]
fn separate_font_collections_produce_identical_pixels_and_geometry() {
    for id in ["mixed-scripts", "shared-ffi-color", "float-pages"] {
        let first = snapshot_cases::render(id).unwrap();
        let second = snapshot_cases::render(id).unwrap();
        assert_eq!(first.image.data(), second.image.data(), "{id}");
        assert_eq!(first.geometry, second.geometry, "{id}");
        assert_eq!(first.settings, second.settings, "{id}");
    }
}

#[test]
fn shared_ffi_is_one_red_owner_glyph_with_a_separate_blue_source_annotation() {
    let r = snapshot_cases::render("shared-ffi-color").unwrap();
    assert_eq!(r.glyph_count, 1);
    assert_eq!(r.geometry["glyphs"][0]["owner"], 1);
    let annotations = array(&r.geometry, "annotations");
    assert_eq!(annotations.len(), 1);
    assert!(annotations[0]["width"].as_f64().unwrap() > 0.0);
    let pixels = r.image.data().as_chunks::<4>().0.iter().collect::<Vec<_>>();
    assert!(pixels.iter().filter(|p| p[0] > p[2]).count() > 20);
    assert!(pixels.iter().filter(|p| p[2] > p[0]).count() > 2);
}

#[test]
fn nested_atomic_records_real_baseline_and_paints_the_second_line_rectangle() {
    let r = snapshot_cases::render("nested-atomic-baseline").unwrap();
    let a = &r.geometry["atomics"][0];
    assert_eq!(a["line"], 1);
    assert_eq!(a["width"], 20.0);
    assert_eq!(a["height"], 20.0);
    let baseline = a["baseline"].as_f64().unwrap();
    let top = a["block_start_in_line"].as_f64().unwrap();
    assert!((baseline - top - 16.0).abs() < 1.0 / 32.0);
    assert_eq!(array(&r.geometry, "inline_boxes").len(), 4);
    let x = (10.0 + a["inline_start"].as_f64().unwrap() + 10.0) as usize;
    let y = (10.0 + a["block_start"].as_f64().unwrap() + 10.0) as usize;
    let pixel = 4 * (y * 512 + x);
    assert_eq!(&r.image.data()[pixel..pixel + 4], &[0, 128, 0, 255]);
}

#[test]
fn preserved_tabs_and_indent_use_the_declared_caller_options() {
    let tabs = snapshot_cases::render("preserved-tabs").unwrap();
    assert_eq!(tabs.settings["text"], "One  two\tthree\nFour five.");
    assert_eq!(tabs.settings["tab_px"], 16.0);
    let lines = array(&tabs.geometry, "lines");
    assert!(lines.len() >= 3);
    assert_eq!(lines[0]["start"], 0);
    for pair in lines.windows(2) {
        assert_eq!(pair[0]["end"], pair[1]["start"]);
    }
    assert_eq!(lines.last().unwrap()["end"], 25);
    let indent = snapshot_cases::render("indent-baseline").unwrap();
    assert_eq!(indent.geometry["glyphs"][0]["inline_position"], 10.0);
}

#[test]
fn arabic_and_japanese_snapshots_retain_all_accepted_source() {
    for id in ["arabic-wrap", "japanese-kinsoku"] {
        let r = snapshot_cases::render(id).unwrap();
        let lines = array(&r.geometry, "lines");
        assert!(lines.len() >= 2, "{id}");
        if id == "arabic-wrap" {
            assert_eq!(lines[0]["block_size"], 48.0);
            assert_eq!(lines[1]["block_offset"], 48.0);
        }
        assert_eq!(lines[0]["start"], 0);
        for pair in lines.windows(2) {
            assert_eq!(pair[0]["end"], pair[1]["start"]);
        }
        assert!(r.glyph_count > 3, "{id}");
    }
}

#[test]
fn page_move_carries_remaining_float_height_and_unconsumed_source() {
    let r = snapshot_cases::render("float-pages").unwrap();
    assert_eq!(r.geometry["height_rejections"], 1);
    assert_eq!(r.geometry["rejected_token_preserved"], true);
    let pages = array(&r.geometry, "pages");
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["fragment"], 1);
    assert_eq!(pages[1]["floats"][0]["height"], 30.0);
    assert_eq!(pages[1]["floats"][0]["inline_start"], 80.0);
    let lines = array(&r.geometry, "lines");
    assert!(lines.len() > 1);
    for pair in lines.windows(2) {
        assert_eq!(pair[0]["end"], pair[1]["start"]);
    }
    assert!(pages[1]["panel_offset"].as_f64().unwrap() > 0.0);
}

fn accepted_line(weight: f32, block: f32) -> shodo::Line {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
            font_weight: weight,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let constraint = LineConstraint {
        block_offset: block,
        ..LineConstraint::new(100.0)
    };
    match p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) {
        LineResult::Line(line) => line,
        other => panic!("unexpected line: {other:?}"),
    }
}

#[test]
fn synthetic_output_and_content_outside_canvas_fail_instead_of_being_blessed() {
    let synthetic = accepted_line(700.0, 0.0);
    assert!(snapshot_cases::paint_lines(&[synthetic], &[], |_| [0, 0, 0, 255]).is_err());
    let outside = accepted_line(400.0, 1100.0);
    assert!(snapshot_cases::paint_lines(&[outside], &[], |_| [0, 0, 0, 255]).is_err());
}

fn accepted_large_line(block: f32) -> shodo::Line {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
            font_size: 100.0,
            line_height: shodo::style::LineHeight::Px(1.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "A");
    let paragraph = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let constraint = LineConstraint {
        block_offset: block,
        ..LineConstraint::new(300.0)
    };
    match paragraph.next_line(
        &mut LayoutContext::new(),
        paragraph.start_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) {
        LineResult::Line(line) => line,
        other => panic!("unexpected accepted line: {other:?}"),
    }
}

#[test]
fn real_outline_outside_canvas_fails_even_when_advance_and_line_box_fit() {
    let line = accepted_large_line(0.0);
    assert!(line.inline_size() < 300.0);
    assert!(line.block_size() < 2.0);
    assert!(snapshot_cases::paint_lines(&[line], &[], |_| [0, 0, 0, 255]).is_err());
}

#[test]
fn real_ink_inside_fixed_canvas_is_not_lost_to_a_short_line_box() {
    let line = accepted_large_line(100.0);
    let (image, count) = snapshot_cases::paint_lines(&[line], &[], |_| [0, 0, 0, 255]).unwrap();
    assert_eq!(count, 1);
    // The A baseline is near149px including page/origin, although the accepted
    // one-pixel line box ends near101px. Its lower ink must survive below143px.
    assert!(
        image.data()[143 * 512 * 4..]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| *pixel != [255, 255, 255, 255])
    );
}

#[test]
fn source_annotation_outside_actual_canvas_fails_instead_of_being_clipped() {
    let line = accepted_line(400.0, 0.0);
    let annotation = shodo::geometry::LogicalRect {
        inline_start: 600.0,
        block_start: 0.0,
        inline_size: 20.0,
        block_size: 10.0,
    };
    assert!(snapshot_cases::paint_lines(&[line], &[annotation], |_| [0, 0, 0, 255]).is_err());
}
