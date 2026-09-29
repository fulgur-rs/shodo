//! Validate real width-reuse and height-retry outputs on original WPT IFCs.
mod core;
use raikiri_traits::Dom;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{LayoutContext, limits::Limits};
use shodo_raikiri_integration::{
    BaselineLayout, DocumentProjection, WptFonts, WptResources, layout_candidate_screen_page,
    snapshot_candidate,
};
use std::{collections::BTreeMap, error::Error, path::Path};

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    require(
        args.len() == 4,
        "usage: core-contract-check WPT SELECTION OUTPUT",
    )?;
    let wpt = Path::new(&args[1]);
    let selected: Value = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let cases = selected["cases"].as_array().ok_or("missing cases")?;
    require(!cases.is_empty(), "empty selected page set")?;
    let limits = Limits::default();
    let fonts = WptFonts::load(&wpt.join("fonts"), &limits)?;
    let mut page = raikiri_traits::PageBox::new();
    page.width = 800.0;
    page.height = 600.0;
    let mut verified = Vec::new();
    let mut gaps = Vec::new();
    let mut candidate_trials = 0;
    let mut native_trials = 0;
    for case in cases {
        let id = case["id"].as_str().ok_or("missing original id")?;
        let resources = WptResources::new(wpt)?;
        let doc = resources.parse_screen(id)?;
        eprintln!("CORE_PHASE native-initial {id}");
        let baseline = BaselineLayout::measure_screen(&doc, page, fonts.baseline.clone())?;
        eprintln!("CORE_PHASE candidate-initial {id}");
        let blocks = layout_candidate_screen_page(&doc, &fonts.candidate, [800, 600], &limits)?;
        let projection = DocumentProjection::new_screen(&doc);
        let mut context = LayoutContext::new();
        let mut text_bases = BTreeMap::new();
        eprintln!("CORE_PHASE candidate-reuse-retry {id}");
        for block in &blocks {
            let base_width = block.geometry.content_inline_size;
            for child in doc
                .parsed
                .dom
                .child_ids(raikiri_traits::NodeId::new(block.node as u64))
            {
                if doc
                    .parsed
                    .dom
                    .get_node(child.0 as usize)
                    .is_some_and(|n| n.text_content().is_some())
                {
                    text_bases.insert(child.0 as usize, (block.node, base_width));
                }
            }
            let prepared = projection.build_ifc_in_block(
                block.node,
                &mut context,
                &fonts.candidate,
                &limits,
                block.geometry,
            )?;
            for width in [base_width, base_width * 0.5, base_width * 1.5] {
                let fresh = prepared.paragraph.break_all(
                    &mut context,
                    &prepared.line_options(width),
                    width,
                    &prepared.atomics,
                );
                let expected = serde_json::to_value(snapshot_candidate(&prepared, &fresh)?)?;
                let (retried, rejected) = core::candidate_retry(&prepared, &mut context, width)?;
                let actual = serde_json::to_value(snapshot_candidate(&prepared, &retried)?)?;
                require(
                    actual == expected,
                    &format!(
                        "candidate retry differs from fresh output: {id} root{} width{width}",
                        block.node
                    ),
                )?;
                require(
                    !retried.is_empty() && rejected > 0,
                    "candidate contract did not exercise an actual rejected line",
                )?;
                let mut end = 0;
                for line in &retried {
                    require(
                        line.text_range().start == end,
                        "candidate retry omits processed source",
                    )?;
                    end = line.text_range().end;
                }
                require(
                    end == prepared.paragraph.text().len(),
                    "candidate retry leaves unconsumed processed source",
                )?;
                candidate_trials += rejected;
                verified.push(json!({"engine": "shodo", "id": id, "root": block.node, "width": width,
                    "processed_source_bytes": end, "accepted_lines": retried.len(), "height_rejections": rejected,
                    "output_sha256": format!("{:x}", Sha256::digest(serde_json::to_vec(&actual)?))}));
            }
        }
        eprintln!("CORE_PHASE native-reuse-retry {id}");
        for node in 0..doc.parsed.dom.node_count() {
            let Some(layout) = baseline
                .document
                .get_node(node)
                .and_then(|n| n.text_layout())
            else {
                continue;
            };
            if layout.lines().len() == 0 {
                continue;
            }
            let Some(&(root, base_width)) = text_bases.get(&node) else {
                gaps.push(json!({"engine": "raikiri-parley", "id": id, "text_node": node,
                    "reason": "native retained text is outside the candidate's compared leaf BFCs"}));
                continue;
            };
            for width in [base_width, base_width * 0.5, base_width * 1.5] {
                let mut fresh = layout.clone();
                fresh.break_all_lines(Some(width));
                let expected = core::native_output(&fresh);
                let mut retried = layout.clone();
                let (engine_rejected, caller_rejected) = core::native_retry(&mut retried, width)
                    .map_err(|error| format!("{id} text-node{node} width{width}: {error}"))?;
                let rejected = engine_rejected + caller_rejected;
                let actual = core::native_output(&retried);
                require(
                    actual == expected,
                    &format!(
                        "native retry differs from fresh output: {id} text-node{node} width{width}"
                    ),
                )?;
                require(
                    rejected > 0,
                    "native contract did not exercise an actual rejected line",
                )?;
                native_trials += rejected;
                verified.push(json!({"engine": "raikiri-parley", "id": id, "root": root, "text_node": node, "width": width,
                    "accepted_lines": retried.lines().len(), "height_rejections": rejected,
                    "engine_height_rejections": engine_rejected, "caller_height_rejections": caller_rejected,
                    "output_sha256": format!("{:x}", Sha256::digest(serde_json::to_vec(&actual)?))}));
            }
        }
    }
    require(
        candidate_trials > 0 && native_trials > 0,
        "missing either real engine retry path",
    )?;
    let report = json!({"schema": 1, "complete": true, "documents": cases.len(),
        "candidate_height_rejections": candidate_trials, "native_height_rejections": native_trials,
        "verified": verified, "unsupported": gaps, "scope": "isolated actual retained-shape width/height-retry protocols; not initial shaping or complete-page pagination"});
    std::fs::write(&args[3], serde_json::to_vec_pretty(&report)?)?;
    println!(
        "real engine retry protocols verified: {} documents, {} operation rows",
        cases.len(),
        verified.len()
    );
    Ok(())
}
