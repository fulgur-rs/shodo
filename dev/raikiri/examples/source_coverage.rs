//! Reproduce original incomplete-source IFCs without adopting the S4 spike.
#[path = "support/source_coverage.rs"]
mod coverage;
#[path = "support/raikiri_style_diffs.rs"]
mod diagnostic;
#[path = "support/source_fonts.rs"]
mod fonts;
#[path = "support/offline_wpt.rs"]
mod offline;
#[path = "support/source_replay.rs"]
mod replay;

use raikiri_traits::{Dom, NodeId as DomId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const RAIKIRI_PIN: &str = "ab7e619a8f321f03de8b8c8b9342954868e044c8";

fn targets(case: &Value) -> Vec<&Value> {
    case["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["classification"] == "incomplete-source")
        .collect()
}

fn child_source(input: &offline::ScreenInput, root: usize) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let node = input
            .parsed
            .dom
            .get_node(id)
            .ok_or("block owner no longer exists")?;
        let cv = input
            .cascade
            .computed
            .get(id)
            .ok_or("missing block-owner style")?;
        result.push(json!({"node":id,"tag":node.tag_name(),"kind":format!("{:?}",node.kind()),
            "text":node.text_content(),"in_document":node.is_in_document(),"display":format!("{:?}",cv.display),
            "float":format!("{:?}",cv.float),"position":format!("{:?}",cv.position)}));
        let children: Vec<_> = input.parsed.dom.child_ids(DomId(id as u64)).collect();
        stack.extend(children.into_iter().rev().map(|n| n.0 as usize));
    }
    Ok(result)
}

fn inspect_block(
    input: &offline::ScreenInput,
    original: &Value,
    fonts: &fonts::OriginalFonts,
    footprint: &mut BTreeMap<String, BTreeSet<String>>,
) -> Result<Value, String> {
    let root = original["root"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("invalid original root")?;
    let root_node = input
        .parsed
        .dom
        .get_node(root)
        .ok_or("missing original root")?;
    if json!(root_node.tag_name()) != original["tag"] {
        return Err("original root tag changed".into());
    }
    let width = original["measured_content_width"]
        .as_f64()
        .ok_or("missing original measured width")? as f32;
    let mut context = LayoutContext::new();
    let prepared = replay::project(
        input,
        root,
        width,
        &mut context,
        &fonts.collection,
        &shodo::limits::Limits::default(),
    )?;
    let p = &prepared.paragraph;
    let events: Vec<_> = p
        .lines(
            &mut context,
            p.start_token(),
            &prepared.options,
            |_, offset| {
                let mut c = LineConstraint::new(width);
                c.block_offset = offset;
                c
            },
            &AtomicSizes::EMPTY,
        )
        .collect();
    let covered = coverage::audit(p, &events)?;
    let original_lines = original["candidate"]["lines"]
        .as_array()
        .ok_or("missing original accepted lines")?;
    let original_ranges: Vec<_> = original_lines
        .iter()
        .map(|l| l["source_range"].clone())
        .collect();
    if json!(covered.line_ranges) != json!(original_ranges)
        || json!(covered.processed_bytes) != original["processed_source_bytes"]
    {
        return Err(
            "actual paragraph/accepted source ranges differ from the original replay".into(),
        );
    }
    let mut consumed = 0;
    let line_only_complete = covered.line_ranges.iter().all(|r| {
        let contiguous = r.start == consumed;
        consumed = r.end;
        contiguous
    }) && consumed == covered.processed_bytes;
    if line_only_complete || original["processed_source_complete"] != false {
        return Err("original incomplete-source predicate was not reproduced".into());
    }
    let mut lines = Vec::new();
    for event in &events {
        if let LineResult::Line(line) = event {
            let mut runs = Vec::new();
            for fragment in line.fragments() {
                if let shodo::Fragment::GlyphRun(run) = fragment {
                    let font = run.font_data().ok_or("accepted glyph has no font bytes")?;
                    runs.push(json!({"node":run.node().map(|n| n.0),"range":run.text_range(),
                        "font_sha256":format!("{:x}",Sha256::digest(font.data.data())),"font_size":run.font_size()}));
                }
            }
            let old_fonts: Vec<_> = original_lines[lines.len()]["runs"]
                .as_array()
                .ok_or("missing original glyph runs")?
                .iter()
                .map(|r| r["font_sha256"].clone())
                .collect();
            let actual_fonts: Vec<_> = runs.iter().map(|r| r["font_sha256"].clone()).collect();
            if actual_fonts != old_fonts {
                return Err("accepted glyph font trace differs from the original replay".into());
            }
            lines.push(json!({"range":line.text_range(),"width":line.inline_size(),"runs":runs}));
        }
    }
    let mut handoffs = Vec::new();
    for block in &covered.blocks {
        handoffs.push(json!({"node":block.node,"generated_origin_node":block.node,"range":block.range,
            "child_source_nodes":child_source(input,block.node as usize)?,"child_layout_paint_status":"pending-separate-BFC"}));
    }
    // Retain the actual public CSS footprint used by this source projection.
    // This separately proves that no unexplained style was lowered to defaults.
    let mut stack = vec![root];
    let mut nodes = Vec::new();
    while let Some(id) = stack.pop() {
        let node = input
            .parsed
            .dom
            .get_node(id)
            .ok_or("missing original source node")?;
        let cv = &input.cascade.computed[id];
        if !node.is_in_document() || cv.display == raikiri_style::property::DisplayValue::None {
            continue;
        }
        if id != root && covered.blocks.iter().any(|b| b.node == id as u64) {
            continue;
        }
        let profile = if id == root {
            diagnostic::InputProfile::MeasuredBlock
        } else if cv.display == raikiri_style::property::DisplayValue::InlineBlock {
            diagnostic::InputProfile::Atomic
        } else {
            diagnostic::InputProfile::Plain
        };
        let cv = diagnostic::prepare_input(cv, profile);
        let differences =
            diagnostic::public_differences(&cv, &raikiri_style::ComputedValues::initial());
        for d in &differences {
            footprint
                .entry(d.field.clone())
                .or_default()
                .insert(d.value.clone());
        }
        nodes.push(json!({"node":id,"tag":node.tag_name(),"text":node.text_content(),"input_profile":profile.name(),"differences":differences,"original_gate_residual":diagnostic::differences(&cv)}));
        let children: Vec<_> = input.parsed.dom.child_ids(DomId(id as u64)).collect();
        stack.extend(children.into_iter().rev().map(|n| n.0 as usize));
    }
    let mapping: Vec<_> = p
        .offset_mapping()
        .ok_or("processed source mapping is missing")?
        .units()
        .iter()
        .map(|u| json!({"node":u.node.0,"dom":u.dom,"text":u.text,"kind":format!("{:?}",u.kind)}))
        .collect();
    Ok(
        json!({"root":root,"tag":original["tag"],"classification":"diagnostic-false-positive-block-handoff",
        "processed_source_bytes":covered.processed_bytes,"processed_text":p.text(),"accepted_line_ranges":covered.line_ranges,
        "original_accepted_line_ranges":original_ranges,"original_native_line_count":original["baseline"]["lines"].as_array().ok_or("missing original native lines")?.len(),
        "measured_content_width":width,"old_predicate_incomplete":true,"terminal_done":matches!(events.last(),Some(LineResult::Done)),
        "accepted_lines":lines,"block_handoffs":handoffs,"dom_source_mapping":mapping,"input_nodes":nodes,
        "whole_page_layout_paint_verified":false,"wpt_verdict":null}),
    )
}

fn inspect_case(
    wpt: &Path,
    case: &Value,
    fonts: &fonts::OriginalFonts,
    footprint: &mut BTreeMap<String, BTreeSet<String>>,
) -> Result<Value, String> {
    if json!(fonts.hashes) != case["font_sha256"] {
        return Err("original font registry/order changed".into());
    }
    let expected: Vec<offline::ResourceRecord> =
        serde_json::from_value(case["resources"].clone()).map_err(|e| e.to_string())?;
    offline::verify_original_resources(wpt, &expected)?;
    let input = offline::parse_screen(wpt, case["id"].as_str().ok_or("missing case ID")?)?;
    for record in &input.resources {
        if !expected.contains(record) {
            return Err("parser resource trace changed".into());
        }
    }
    let warnings: Vec<_> = input
        .parsed
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .collect();
    if json!(warnings) != case["parse_warnings"] {
        return Err("parser warning trace changed".into());
    }
    if input.cascade.computed.len() != input.parsed.dom.node_count() {
        return Err("cascade does not cover the original DOM".into());
    }
    let mut blocks = Vec::new();
    for block in targets(case) {
        blocks.push(match inspect_block(&input,block,fonts,footprint) {
            Ok(result) => result,
            Err(error) => json!({"root":block["root"],"tag":block["tag"],"classification":"source-replay-error","error":error}),
        });
    }
    Ok(
        json!({"id":case["id"],"blocks":blocks,"original_resources_verified":expected,
        "parser_resource_trace":input.resources,"parse_warnings":warnings,"original_font_sha256":case["font_sha256"]}),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: source_coverage <wpt-root> <original-comparison.json> <output.json>".into(),
        );
    }
    let source = std::fs::read(&args[1])?;
    let original: Value = serde_json::from_slice(&source)?;
    if original["raikiri_revision"] != RAIKIRI_PIN {
        return Err("comparison does not use the compiled raikiri pin".into());
    }
    let cases = original["cases"]
        .as_array()
        .ok_or("missing original cases")?;
    let selected: Vec<_> = cases.iter().filter(|c| !targets(c).is_empty()).collect();
    if selected.is_empty() {
        return Err("no original incomplete-source blocks selected".into());
    }
    let mut ids = BTreeSet::new();
    let expected_blocks: usize = selected.iter().map(|c| targets(c).len()).sum();
    let font_registry = fonts::load(
        &Path::new(&args[0]).join("fonts"),
        &shodo::limits::Limits::default(),
    )?;
    let mut footprint = BTreeMap::new();
    let mut records = Vec::new();
    for case in &selected {
        if !ids.insert(case["id"].as_str().ok_or("missing original case ID")?) {
            return Err("duplicate original case ID".into());
        }
        records.push(
            match inspect_case(Path::new(&args[0]), case, &font_registry, &mut footprint) {
                Ok(report) => report,
                Err(error) => {
                    json!({"id":case["id"],"classification":"source-input-error","error":error})
                }
            },
        );
    }
    let reported: usize = records
        .iter()
        .filter_map(|r| r["blocks"].as_array())
        .flatten()
        .filter(|b| b["classification"] == "diagnostic-false-positive-block-handoff")
        .count();
    let errors: usize = records
        .iter()
        .map(|r| {
            if r["classification"] == "source-input-error" {
                1
            } else {
                r["blocks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["classification"] == "source-replay-error")
                    .count()
            }
        })
        .sum();
    let complete = errors == 0 && reported == expected_blocks;
    let report = json!({"scope":"ordinary original IFC processed-source coverage including explicit child-block handoffs; child layout/paint and whole-page WPT conformance remain unverified",
        "raikiri_revision":RAIKIRI_PIN,"original_comparison_sha256":format!("{:x}",Sha256::digest(&source)),
        "viewport_css_px":original["viewport_css_px"],"expected_documents":selected.len(),"expected_blocks":expected_blocks,
        "reported_blocks":reported,"input_errors":errors,"complete":complete,"cases":records,"input_footprint":footprint,
        "font_registry_sha256":font_registry.hashes,"generic_families":font_registry.generics,
        "candidate_wpt_image_verdicts":0,"pass_delta":null});
    std::fs::write(&args[2], serde_json::to_string_pretty(&report)? + "\n")?;
    println!(
        "{} documents, {reported}/{expected_blocks} source audits, {errors} errors; no WPT verdict",
        selected.len()
    );
    if !complete {
        return Err("source replay incomplete; inspect per-block errors".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::coverage;
    use shodo::{
        AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
    };
    use shodo::{
        limits::Limits,
        node::{NodeId, TextSource},
        style::{FontFamily, LineOptions, ParagraphStyle},
    };

    fn paragraph(text: &str, blocks: &[u64]) -> Paragraph {
        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(2),
                offset: 0,
            },
            text,
        );
        for &id in blocks {
            builder.push_block_in_inline(NodeId(id));
        }
        builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
    }

    fn events(p: &Paragraph) -> Vec<LineResult> {
        p.lines(
            &mut LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            |_, _| LineConstraint::new(500.0),
            &AtomicSizes::EMPTY,
        )
        .collect()
    }

    #[test]
    fn trailing_blocks_cover_generated_ranges_missing_from_line_only_trace() {
        let p = paragraph("ab", &[40, 41]);
        let result = coverage::audit(&p, &events(&p)).unwrap();
        assert_eq!(result.processed_bytes, 8);
        assert_eq!(result.line_ranges.len(), 1);
        assert_eq!(result.line_ranges[0], 0..2);
        assert_eq!(
            result
                .blocks
                .iter()
                .map(|b| (b.node, b.range.clone()))
                .collect::<Vec<_>>(),
            [(40, 2..5), (41, 5..8)]
        );
        assert_eq!(p.text(), "ab\u{2029}\u{2029}");
    }

    #[test]
    fn ordinary_text_only_trace_covers_the_accepted_source() {
        let p = paragraph("abc", &[]);
        let result = coverage::audit(&p, &events(&p)).unwrap();
        assert_eq!(result.processed_bytes, 3);
        assert_eq!(result.line_ranges.len(), 1);
        assert_eq!(result.line_ranges[0], 0..3);
        assert!(result.blocks.is_empty());
    }

    #[test]
    fn block_only_trace_is_explicit_handoff_without_invented_lines() {
        let p = paragraph("", &[40, 41]);
        let result = coverage::audit(&p, &events(&p)).unwrap();
        assert!(result.line_ranges.is_empty());
        assert_eq!(result.processed_bytes, 6);
        assert_eq!(result.blocks.len(), 2);
    }

    #[test]
    fn discarding_block_events_is_uncovered_input_even_after_done() {
        let p = paragraph("ab", &[40]);
        let lossy: Vec<_> = events(&p)
            .into_iter()
            .filter(|r| !matches!(r, LineResult::BlockInInline { .. }))
            .collect();
        assert!(coverage::audit(&p, &lossy).is_err());
    }

    #[test]
    fn a_literal_separator_cannot_be_relabelled_as_a_generated_block() {
        let p = paragraph("\u{2029}", &[]);
        let generated = paragraph("", &[40]);
        assert!(coverage::audit(&p, &events(&generated)).is_err());
    }

    #[test]
    fn wrong_block_identity_cannot_claim_a_generated_separator() {
        let p = paragraph("", &[40]);
        let mut wrong = events(&p);
        for event in &mut wrong {
            if let LineResult::BlockInInline { node, .. } = event {
                *node = NodeId(41);
            }
        }
        assert!(coverage::audit(&p, &wrong).is_err());
    }

    #[test]
    fn missing_visible_text_is_not_an_empty_success() {
        let p = paragraph("abc", &[]);
        assert!(coverage::audit(&p, &[LineResult::Done]).is_err());
    }

    #[test]
    fn covered_lines_without_terminal_done_are_an_incomplete_trace() {
        let p = paragraph("abc", &[]);
        let incomplete: Vec<_> = events(&p)
            .into_iter()
            .filter(|r| !matches!(r, LineResult::Done))
            .collect();
        assert!(coverage::audit(&p, &incomplete).is_err());
    }
    fn dom_input(html: &str) -> super::offline::ScreenInput {
        let parsed = raikiri_html::parse(
            html.as_bytes(),
            &raikiri_html::ParseOptions {
                extra_stylesheets: &[],
                network: None,
                base_url: None,
            },
        )
        .unwrap();
        let cascade = raikiri_html::build_cascaded_with_media_context(
            &parsed,
            &raikiri_style::MediaContext::screen(),
        );
        super::offline::ScreenInput {
            parsed,
            cascade,
            resources: vec![],
        }
    }

    fn element(input: &super::offline::ScreenInput, name: &str) -> usize {
        (0..input.parsed.dom.node_count())
            .find(|&id| input.parsed.dom.get_node(id).unwrap().attribute("id") == Some(name))
            .unwrap()
    }

    #[test]
    fn real_dom_projection_retains_nested_block_owner_without_copying_child_text() {
        let input = dom_input("<div id='root'>ab<span>cd</span><p id='child'>EF</p></div>");
        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let root = element(&input, "root");
        let child = element(&input, "child");
        let prepared = super::replay::project(
            &input,
            root,
            500.0,
            &mut LayoutContext::new(),
            &fonts.collection,
            &limits,
        )
        .unwrap();
        assert_eq!(prepared.paragraph.text(), "abcd\u{2029}");
        let trace: Vec<_> = prepared
            .paragraph
            .lines(
                &mut LayoutContext::new(),
                prepared.paragraph.start_token(),
                &prepared.options,
                |_, _| LineConstraint::new(500.0),
                &AtomicSizes::EMPTY,
            )
            .collect();
        let result = coverage::audit(&prepared.paragraph, &trace).unwrap();
        assert_eq!(result.line_ranges.len(), 1);
        assert_eq!(result.line_ranges[0], 0..4);
        assert_eq!(
            result
                .blocks
                .iter()
                .map(|b| (b.node, b.range.clone()))
                .collect::<Vec<_>>(),
            [(child as u64, 4..7)]
        );
    }

    #[test]
    fn text_after_block_is_a_new_accepted_source_segment() {
        let input = dom_input("<div id='root'>ab<div id='child'>EF</div>cd</div>");
        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        let prepared = super::replay::project(
            &input,
            element(&input, "root"),
            500.0,
            &mut LayoutContext::new(),
            &fonts.collection,
            &limits,
        )
        .unwrap();
        assert_eq!(prepared.paragraph.text(), "ab\u{2029}cd");
        let trace: Vec<_> = prepared
            .paragraph
            .lines(
                &mut LayoutContext::new(),
                prepared.paragraph.start_token(),
                &prepared.options,
                |_, _| LineConstraint::new(500.0),
                &AtomicSizes::EMPTY,
            )
            .collect();
        let result = coverage::audit(&prepared.paragraph, &trace).unwrap();
        assert_eq!(result.line_ranges, [0..2, 5..7]);
        assert_eq!(result.blocks[0].range, 2..5);
    }

    #[test]
    fn unresolved_projection_style_is_rejected_instead_of_silently_reset() {
        let input =
            dom_input("<style>#root{writing-mode:vertical-rl}</style><div id='root'>abc</div>");
        let limits = Limits::default();
        let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
        assert!(
            super::replay::project(
                &input,
                element(&input, "root"),
                500.0,
                &mut LayoutContext::new(),
                &fonts.collection,
                &limits
            )
            .is_err()
        );
    }
}
