//! Text preparation and retained-shape operations without complete box layout.
use crate::{caller::Input, core, require, scope};
use raikiri_traits::Dom;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{LayoutContext, Line};
use shodo_raikiri_integration::{
    CandidateBlock, DocumentProjection, PreparedIfc, WptScreenDocument,
    layout_candidate_screen_page, snapshot_candidate,
};
use std::{collections::BTreeMap, error::Error};

struct Candidate {
    context: LayoutContext,
    ifcs: Vec<(usize, f32, PreparedIfc)>,
    lines: Vec<Vec<Line>>,
}
struct Native {
    document: raikiri_dom::Document,
    // Only original text children of the compared leaf BFCs are rebroken.
    nodes: Vec<(usize, usize, f32)>,
    layouts: Vec<parley::Layout<()>>,
    outside_paired_roots: usize,
}
enum Prepared {
    Candidate(Candidate),
    Native(Native),
}

fn prepare(
    input: &Input<'_>,
    document: &WptScreenDocument,
    blocks: &[CandidateBlock],
) -> Result<Prepared, Box<dyn Error>> {
    if input.engine == "candidate" {
        let projection = DocumentProjection::new_screen(document);
        let mut context = LayoutContext::new();
        let mut ifcs = Vec::new();
        let mut lines = Vec::new();
        for block in blocks {
            let p = projection.build_ifc_in_block(
                block.node,
                &mut context,
                &input.fonts.candidate,
                input.limits,
                block.geometry,
            )?;
            let width = block.geometry.content_inline_size;
            lines.push(p.paragraph.break_all(
                &mut context,
                &p.line_options(width),
                width,
                &p.atomics,
            ));
            ifcs.push((block.node, width, p));
        }
        Ok(Prepared::Candidate(Candidate {
            context,
            ifcs,
            lines,
        }))
    } else {
        let mut owned = document.parsed.dom.clone();
        // This public boundary does real text/style preparation and shaping.
        // It also emits initial lines; it is not a clone of shaped output.
        raikiri_dom::relayout_text_for_width(
            &mut owned,
            &document.cascade,
            800.0,
            800.0,
            input.fonts.baseline.clone(),
        );
        let mut bases = BTreeMap::new();
        for block in blocks {
            for child in document
                .parsed
                .dom
                .child_ids(raikiri_traits::NodeId::new(block.node as u64))
            {
                let node = child.0 as usize;
                if document
                    .parsed
                    .dom
                    .get_node(node)
                    .is_some_and(|n| n.text_content().is_some())
                {
                    bases.insert(node, (block.node, block.geometry.content_inline_size));
                }
            }
        }
        let mut nodes = Vec::new();
        let mut outside_paired_roots = 0;
        for node in 0..owned.node_count() {
            let Some(layout) = owned.get_node(node).and_then(|n| n.text_layout()) else {
                continue;
            };
            if layout.lines().len() == 0 {
                continue;
            }
            if let Some(&(root, width)) = bases.get(&node) {
                nodes.push((node, root, width));
            } else {
                outside_paired_roots += 1;
            }
        }
        require(
            !nodes.is_empty(),
            "isolated native has no retained paired text layouts",
        )?;
        Ok(Prepared::Native(Native {
            document: owned,
            nodes,
            layouts: Vec::new(),
            outside_paired_roots,
        }))
    }
}

fn rebreak(
    prepared: &mut Prepared,
    multiplier: f32,
    retry: bool,
) -> Result<(usize, usize), Box<dyn Error>> {
    let mut rejected = (0, 0);
    match prepared {
        Prepared::Candidate(p) => {
            for (index, (_, base, ifc)) in p.ifcs.iter().enumerate() {
                let width = base * multiplier;
                p.lines[index] = if retry {
                    let (lines, count) = core::candidate_retry(ifc, &mut p.context, width)?;
                    rejected.0 += count;
                    lines
                } else {
                    ifc.paragraph.break_all(
                        &mut p.context,
                        &ifc.line_options(width),
                        width,
                        &ifc.atomics,
                    )
                };
            }
        }
        Prepared::Native(p) => {
            for (index, &(_, _, base)) in p.nodes.iter().enumerate() {
                let layout = p
                    .layouts
                    .get_mut(index)
                    .ok_or("native mutable reuse owner missing")?;
                if retry {
                    let (engine, caller) = core::native_retry(layout, base * multiplier)?;
                    rejected.0 += engine;
                    rejected.1 += caller;
                } else {
                    layout.break_all_lines(Some(base * multiplier));
                }
            }
        }
    }
    Ok(rejected)
}

fn output(prepared: &Prepared) -> Result<Value, Box<dyn Error>> {
    let rows: Vec<_> = match prepared {
        Prepared::Candidate(p) => p
            .ifcs
            .iter()
            .zip(&p.lines)
            .map(|((root, _, ifc), lines)| {
                let mut end = 0;
                for line in lines {
                    require(
                        line.text_range().start == end,
                        "isolated candidate source gap",
                    )?;
                    end = line.text_range().end;
                }
                require(
                    end == ifc.paragraph.text().len(),
                    "isolated candidate leaves unconsumed source",
                )?;
                Ok(json!({"root":root,"processed_source_bytes":end,
                "snapshot":snapshot_candidate(ifc,lines)?}))
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
        Prepared::Native(p) => p
            .nodes
            .iter()
            .enumerate()
            .map(|(index, &(node, root, _))| {
                let layout = if p.layouts.is_empty() {
                    p.document.get_node(node).and_then(|n| n.text_layout())
                } else {
                    p.layouts.get(index)
                }
                .ok_or("native retained output disappeared")?;
                Ok(json!({"root":root,"text_node":node,"snapshot":core::native_output(layout)}))
            })
            .collect::<Result<_, Box<dyn Error>>>()?,
    };
    Ok(json!(rows))
}

fn digest(value: &Value) -> Result<String, Box<dyn Error>> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

fn setup_mutable_reuse(prepared: &mut Prepared) -> Result<(), Box<dyn Error>> {
    if let Prepared::Native(p) = prepared {
        p.layouts = p
            .nodes
            .iter()
            .map(|&(node, _, _)| {
                p.document
                    .get_node(node)
                    .and_then(|n| n.text_layout())
                    .cloned()
                    .ok_or("native initial shape disappeared")
            })
            .collect::<Result<_, _>>()?;
    }
    Ok(())
}

pub fn measure(
    input: Input<'_>,
    document: &WptScreenDocument,
    verify_fresh: bool,
) -> Result<Value, Box<dyn Error>> {
    // Caller geometry is established outside isolated engine scopes. Running
    // this pass preconditions font caches but no prepared shapes are reused.
    let (blocks, geometry_preconditioning) = scope::measured(|| {
        Ok(layout_candidate_screen_page(
            document,
            &input.fonts.candidate,
            [800, 600],
            input.limits,
        )?)
    })?;
    require(
        blocks.iter().map(|b| b.node).collect::<Vec<_>>() == input.roots,
        "isolated caller root provenance changed",
    )?;
    let mut samples = Vec::new();
    let mut expected = None;
    let mut outside = 0;
    for index in 0..10 {
        let (mut prepared, initial) = scope::measured(|| prepare(&input, document, &blocks))?;
        if let Prepared::Native(p) = &prepared {
            outside = p.outside_paired_roots;
        }
        let initial_output = output(&prepared)?;
        if let Some(previous) = &expected {
            require(
                previous == &initial_output,
                "isolated initial cold/warm output changed",
            )?;
        }
        expected = Some(initial_output);
        let (_, reuse_owner_setup) = scope::measured(|| setup_mutable_reuse(&mut prepared))?;
        let mut widths = Vec::new();
        for multiplier in [1.0, 0.5, 1.5, 1.0] {
            let (_, normal) = scope::measured(|| rebreak(&mut prepared, multiplier, false))?;
            let ordinary = output(&prepared)?;
            if verify_fresh {
                // Correctness-only operation: construct actual fresh shapes,
                // not copies of the measured retained-shape owner.
                let mut fresh = prepare(&input, document, &blocks)?;
                setup_mutable_reuse(&mut fresh)?;
                rebreak(&mut fresh, multiplier, false)?;
                require(
                    output(&fresh)? == ordinary,
                    "isolated retained-shape width output differs from fresh real preparation",
                )?;
            }
            let ((engine_rejections, caller_rejections), retried) =
                scope::measured(|| rebreak(&mut prepared, multiplier, true))?;
            require(
                engine_rejections + caller_rejections > 0,
                "isolated retry did not reject an actual line",
            )?;
            let actual = output(&prepared)?;
            require(
                actual == ordinary,
                "isolated restored retry changed complete source/glyph output",
            )?;
            widths.push(
                json!({"root_width_multiplier":multiplier,"reused_shape_width":normal,
                "height_rejected_retry":retried,"engine_height_rejections":engine_rejections,
                "caller_height_rejections":caller_rejections,"output_sha256":digest(&actual)?,
                "output":actual}),
            );
        }
        let (_, released) = scope::measured(|| {
            drop(prepared);
            Ok(())
        })?;
        samples.push(
            json!({"state":if index==0{"first-call-in-process"}else{"warm-process"},
            "initial_text_pipeline":initial,"mutable_reuse_owner_setup":reuse_owner_setup,
            "reused_widths_and_retries":widths,"release_prepared_owner":released}),
        );
    }
    let root_widths: Vec<_> = blocks
        .iter()
        .map(|b| json!({"root":b.node,"width":b.geometry.content_inline_size}))
        .collect();
    let (_, release_geometry) = scope::measured(|| {
        drop(blocks);
        Ok(())
    })?;
    Ok(
        json!({"schema":1,"operation":"isolated-text-pipeline-width-reuse-and-height-retry",
        "geometry_preconditioning":geometry_preconditioning,"release_geometry":release_geometry,
        "id":input.id,"engine":input.engine,"mode":if cfg!(feature="allocation-counting"){"memory"}else{"time"},
        "instrumented":cfg!(feature="allocation-counting"),"viewport_css_px":[800,600],
        "correctness_only":verify_fresh,"fresh_shape_outputs_verified":verify_fresh,
        "root_width_multipliers":[1.0,0.5,1.5,1.0],"initial_output":expected,"samples":samples,
        "root_content_width_css_px":root_widths,
        "scope":"initial: original resolved DOM/cascade to actual text preparation/shapes/lines, no Taffy full box layout; reused widths and height retries: original paired leaf IFCs/text-node shapes, with same root content widths",
        "preconditioning":"original parse/fonts/audit and candidate caller geometry pass precede first isolated call; actual initial shaping creates fresh contexts/owners, never clones shaped output; native consumed FontContext clone is inside preparation",
        "retained_owner":if input.engine=="native"{"unshaped DOM clone populated by real native text preparation, all original layouts, plus separately measured mutable copies of paired retained shapes; internal native context released"}else{"candidate prepared IFCs, context caches and accepted lines"},
        "native_retained_text_layouts_outside_paired_leaf_roots":outside,
        "initial_boundary_difference":"native public text pipeline prepares all original DOM text and includes immutable-input DOM cloning; candidate projects the compared leaf IFCs; initial cost comparison must retain this difference and is not pure per-run Parley shaping",
        "mutable_reuse_owner_setup":"native public DOM has no mutable text-layout accessor, so actual shaped layouts are cloned in this separately recorded scope; candidate retains original prepared IFCs/context; setup copies are not initial shaping or width-rebreak cost",
        "retry_boundary":"each positive line is actually rejected at remaining height zero and replayed from the same token/breaker checkpoint; this isolated protocol is not complete-page pagination",
        "resources":input.case["resources"],"parse_warnings":input.case["parse_warnings"],
        "font_sha256":input.case["font_sha256"],"full_comparison_complete":false}),
    )
}
