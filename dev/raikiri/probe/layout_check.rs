//! Real original-input acceptance test for the disposable layout-only boundary.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo_raikiri_integration::{
    WptFonts, WptResources, candidate_page_inputs, layout_candidate_screen_page,
};
use std::{error::Error, path::Path};

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    require(args.len() == 4, "usage: layout-check WPT SELECTION OUTPUT")?;
    let wpt = Path::new(&args[1]);
    let selected: Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    require(
        selected["wpt_revision"] == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e",
        "wrong WPT pin",
    )?;
    let actual_pin = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(wpt)
        .output()?;
    require(
        actual_pin.status.success()
            && std::str::from_utf8(&actual_pin.stdout)?.trim() == selected["wpt_revision"],
        "actual WPT checkout pin differs",
    )?;
    let cases = selected["cases"].as_array().ok_or("missing cases")?;
    require(!cases.is_empty(), "empty selected page set")?;
    let limits = Limits::default();
    let fonts = WptFonts::load(&wpt.join("fonts"), &limits)?;
    let font_hashes: Vec<_> = fonts
        .bytes
        .iter()
        .map(|b| format!("{:x}", Sha256::digest(b)))
        .collect();
    require(
        font_hashes.len() == 88,
        "wrong original bundled font registry",
    )?;
    let mut verified = Vec::new();
    for expected in cases {
        let id = expected["id"].as_str().ok_or("missing original id")?;
        require(
            expected["candidate_page"]["classification"] == "candidate-static-leaf-block-page",
            "original candidate page is unsupported",
        )?;
        require(
            json!(font_hashes) == expected["font_sha256"],
            "original font bytes/order differ",
        )?;
        let resources = WptResources::new(wpt)?;
        let doc = resources.parse_screen(id)?;
        // Replay the original caller's supported intrinsic Ahem declaration.
        // This fetch is part of provenance validation, outside all measurement.
        let tree = raikiri_html::build_rule_tree(&doc.parsed);
        for (family, face) in tree.font_faces().iter() {
            require(
                family.eq_ignore_ascii_case("Ahem")
                    && face.style == raikiri_style::FontFaceStyle::Normal
                    && face.weight == raikiri_style::FontFaceWeight::Normal
                    && face.stretch.0.as_str() == "normal"
                    && face.unicode_range.is_empty()
                    && face.src.len() == 1
                    && matches!(&face.src[0], raikiri_style::FontFaceSource::Url { url, .. } if url.as_str() == "/fonts/Ahem.ttf"),
                "unrepresented original web-face registration",
            )?;
            resources.load_url("http://web-platform.test/fonts/Ahem.ttf")?;
        }
        require(
            serde_json::to_value(resources.records()?)? == expected["resources"],
            "original resource bytes/order differ",
        )?;
        let warnings: Vec<_> = doc
            .parsed
            .warnings
            .iter()
            .map(|w| format!("{w:?}"))
            .collect();
        require(
            json!(warnings) == expected["parse_warnings"],
            "original parse warnings differ",
        )?;
        let blocks = layout_candidate_screen_page(&doc, &fonts.candidate, [800, 600], &limits)?;
        let page_inputs = candidate_page_inputs(&doc, &fonts.candidate, [800, 600])?;
        require(
            page_inputs.blocks.len() == blocks.len(),
            "page-input root count changed",
        )?;
        for (input, block) in page_inputs.blocks.iter().zip(&blocks) {
            require(
                input.node == block.node
                    && input.geometry.content_inline_size == block.geometry.content_inline_size
                    && input.geometry.border_inline_size == block.geometry.border_inline_size
                    && input.geometry.edges == block.geometry.edges,
                "page-input CSS root/width/edges differ from original static caller",
            )?;
            require(
                page_inputs.body_edges.margin.inline_start
                    + input.geometry.edges.margin.inline_start
                    == block.border_origin[0],
                "page-input body inline origin differs",
            )?;
        }
        let actual: Vec<_> = blocks
            .iter()
            .map(|b| {
                json!({
                    "node": b.node, "border_origin": b.border_origin,
                    "border_size": [b.geometry.border_inline_size, b.geometry.border_block_size],
                    "content_width": b.geometry.content_inline_size,
                    "content_height": b.content_height, "line_count": b.line_count,
                })
            })
            .collect();
        if json!(actual) != expected["candidate_page"]["blocks"] {
            return Err(format!("layout-only extraction changed actual original block output: {id}; actual={actual:?}; expected={}", expected["candidate_page"]["blocks"]).into());
        }
        // Conservative bound for the native preshaper's job count. The pinned
        // caller starts Rayon at 32 jobs; allocator probes require a verified
        // single-threaded operation, rather than assuming every page is small.
        let original_text_nodes = (0..doc.parsed.dom.node_count())
            .filter(|&node| {
                doc.parsed
                    .dom
                    .get_node(node)
                    .is_some_and(|n| n.text_content().is_some())
            })
            .count();
        verified.push(json!({"id": id, "blocks": actual, "resources": resources.records()?, "parse_warnings": warnings,
            "original_text_nodes": original_text_nodes,
            "native_job_count_below_parallel_threshold_by_text_node_bound": original_text_nodes < 32}));
    }
    let report = json!({"schema": 1, "complete": true, "verified_pages": verified.len(),
        "viewport_css_px": [800, 600], "font_sha256": font_hashes,
        "wpt_revision": selected["wpt_revision"], "cases": verified,
        "page_input_css_width_edges_validated": true,
        "candidate_wpt_image_verdicts": 0, "pass_delta": null});
    std::fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "validated original block geometry, resources, warnings and 88 fonts: {} pages",
        verified.len()
    );
    Ok(())
}
