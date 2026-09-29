//! Real page APIs on original inputs; the candidate flow consumer is provisional.
use crate::{caller::Input, core, fetch_web_faces, require, scope};
use raikiri_traits::Dom;
use serde_json::{Value, json};
use shodo::LayoutContext;
use shodo_raikiri_integration::{
    CandidatePageInputs, DocumentProjection, FlowDriver, FlowFragmentSize, FlowPagination,
    FlowState, FlowTrace, PreparedIfc, WptResources, WptScreenDocument, candidate_page_inputs,
    snapshot_candidate,
};
use std::{collections::BTreeMap, error::Error};

struct CandidateRoot {
    root: usize,
    width: f32,
    prepared: PreparedIfc,
    pagination: FlowPagination,
}
enum Output {
    Candidate(Vec<CandidateRoot>, LayoutContext),
    Native(Vec<raikiri_traits::PageFragment>),
}
struct Page {
    document: WptScreenDocument,
    resources: WptResources,
    output: Option<Output>,
}

fn preflight(page: &Page, inputs: &CandidatePageInputs) -> Result<(), Box<dyn Error>> {
    let initial = raikiri_style::ComputedValues::initial();
    require(
        page.document.cascade.page.size().is_none()
            && page.document.cascade.page.margin_boxes().is_empty()
            && page
                .document
                .cascade
                .page_values
                .iter()
                .all(|p| *p == raikiri_style::property::PageValue::Auto),
        "provisional pagination does not represent page styles/names",
    )?;
    for cv in &page.document.cascade.computed {
        require(
            cv.break_before == initial.break_before
                && cv.break_after == initial.break_after
                && cv.break_inside == initial.break_inside,
            "provisional pagination does not represent forced/avoided box breaks",
        )?;
    }
    let mut page_box = raikiri_traits::PageBox::new();
    page_box.width = 800.0;
    page_box.height = 128.0;
    let geometry = raikiri_dom::resolve_page_fragment_geometry(&page.document.cascade, page_box, 0);
    require(
        geometry.content_box.x == 0.0
            && geometry.content_box.y == 0.0
            && geometry.content_box.width == 800.0
            && geometry.content_box.height == 128.0,
        "provisional pagination does not represent page margins/insets",
    )?;
    for block in &inputs.blocks {
        let edges = block.geometry.edges;
        require(
            edges.padding == shodo::node::Sides::default()
                && edges.border == shodo::node::Sides::default()
                && edges.margin.block_start >= 0.0
                && edges.margin.block_end >= 0.0,
            "provisional pagination does not represent leaf padding/borders/negative margins",
        )?;
        require(
            block.geometry.content_inline_size.is_finite()
                && block.geometry.content_inline_size > 0.0,
            "provisional pagination needs positive original content width",
        )?;
    }
    require(
        inputs.body_edges.margin.block_start.is_finite()
            && inputs.body_edges.margin.block_start >= 0.0
            && inputs.body_edges.margin.block_end >= 0.0,
        "provisional pagination does not represent negative body margins",
    )
}

fn relayout(page: &mut Page, input: &Input<'_>, height: u32) -> Result<(), Box<dyn Error>> {
    if input.engine == "native" {
        let mut page_box = raikiri_traits::PageBox::new();
        page_box.width = 800.0;
        page_box.height = height as f32;
        let fragments = raikiri_dom::layout_page_fragments(
            &mut page.document.parsed.dom,
            &page.document.cascade,
            page_box,
            input.fonts.baseline.clone(),
        )
        .map_err(|e| format!("actual native pagination failed: {e:?}"))?;
        page.output = Some(Output::Native(fragments));
    } else {
        // Resolve the original CSS again inside the actual caller, not by
        // timing a clone of an already prepared paragraph or static output.
        let inputs = candidate_page_inputs(&page.document, &input.fonts.candidate, [800, height])?;
        let projection = DocumentProjection::new_screen(&page.document);
        let mut context = LayoutContext::new();
        let mut roots = Vec::new();
        let mut fragment = 0;
        let mut offset = 0.0;
        let mut pending_margin = inputs.body_edges.margin.block_start;
        for block in inputs.blocks {
            let width = block.geometry.content_inline_size;
            let prepared = projection.build_ifc_in_block(
                block.node,
                &mut context,
                &input.fonts.candidate,
                input.limits,
                block.geometry,
            )?;
            let cv = &page.document.cascade.computed[block.node];
            let orphans = usize::try_from(cv.orphans)?;
            let widows = usize::try_from(cv.widows)?;
            let mut checkpoint = FlowState::new(prepared.paragraph.start_token(), width)?;
            checkpoint.fragment = fragment;
            checkpoint.block_offset =
                offset + pending_margin.max(block.geometry.edges.margin.block_start);
            let driver = FlowDriver::new(
                &prepared.paragraph,
                prepared.line_options(width),
                &prepared.atomics,
                &[],
            )?;
            let pagination = driver.paginate(
                &mut context,
                &checkpoint,
                &[FlowFragmentSize {
                    inline_size: width,
                    block_size: height as f32,
                }],
                orphans,
                widows,
            )?;
            fragment = pagination.state.fragment;
            offset = pagination.state.block_offset;
            pending_margin = block.geometry.edges.margin.block_end;
            roots.push(CandidateRoot {
                root: block.node,
                width,
                prepared,
                pagination,
            });
        }
        page.output = Some(Output::Candidate(roots, context));
    }
    Ok(())
}

fn start(input: &Input<'_>, height: u32) -> Result<Page, Box<dyn Error>> {
    let resources = WptResources::new(input.wpt)?;
    let document = resources.parse_screen(input.id)?;
    fetch_web_faces(&document, &resources)?;
    let mut page = Page {
        document,
        resources,
        output: None,
    };
    relayout(&mut page, input, height)?;
    Ok(page)
}

fn output(page: &Page, roots: &[usize]) -> Result<Value, Box<dyn Error>> {
    let mut partition = Vec::new();
    let mut snapshots = Vec::new();
    let mut widths = Vec::new();
    match page
        .output
        .as_ref()
        .ok_or("missing actual pagination output")?
    {
        Output::Candidate(blocks, _context) => {
            let mut heights = 0;
            let mut lookaheads = 0;
            let mut page_count = 1;
            for block in blocks {
                let mut source_end = 0;
                let mut line_start = 0;
                let mut lines = Vec::new();
                for fragment in &block.pagination.fragments {
                    let line_end = line_start + fragment.lines.len();
                    partition.push(json!({"root":block.root,"page":fragment.index,
                        "line_start":line_start,"line_end":line_end}));
                    let mut snapshot = snapshot_candidate(&block.prepared, &fragment.lines)?;
                    for (line, row) in fragment.lines.iter().zip(&mut snapshot.lines) {
                        require(
                            line.text_range().start == source_end,
                            "paged candidate source gap/overlap",
                        )?;
                        source_end = line.text_range().end;
                        for run in &mut row.runs {
                            for glyph in &mut run.glyphs {
                                glyph.block_position -= line.block_offset();
                            }
                        }
                    }
                    lines.extend(snapshot.lines);
                    line_start = line_end;
                    page_count = page_count.max(fragment.index + 1);
                }
                require(
                    source_end == block.prepared.paragraph.text().len(),
                    "paged candidate unconsumed source",
                )?;
                heights += block
                    .pagination
                    .trace
                    .iter()
                    .filter(|e| matches!(e, FlowTrace::HeightRejected))
                    .count();
                lookaheads += block
                    .pagination
                    .trace
                    .iter()
                    .filter(|e| matches!(e, FlowTrace::LookaheadRejected))
                    .count();
                snapshots.push(
                    json!({"root":block.root,"processed_text":block.prepared.paragraph.text(),
                    "processed_source_bytes":source_end,"lines":lines}),
                );
                widths.push(json!({"root":block.root,"content_width":block.width}));
            }
            Ok(
                json!({"partition":partition,"snapshots":snapshots,"root_widths":widths,
                "page_count":page_count,"source_partition_complete":true,
                "actual_height_rejections":heights,"actual_lookahead_rejections":lookaheads,
                "retry_trace_visible":true}),
            )
        }
        Output::Native(pages) => {
            let mut nodes = BTreeMap::new();
            for &root in roots {
                let node = page
                    .document
                    .parsed
                    .dom
                    .get_node(root)
                    .ok_or("missing paged native root")?;
                let b = node.unrounded_layout;
                widths.push(json!({"root":root,"content_width":
                    (b.size.width-b.border.left-b.border.right-b.padding.left-b.padding.right).max(0.0)}));
                for child in page
                    .document
                    .parsed
                    .dom
                    .child_ids(raikiri_traits::NodeId::new(root as u64))
                {
                    let id = child.0 as usize;
                    let node = page
                        .document
                        .parsed
                        .dom
                        .get_node(id)
                        .ok_or("missing original native text")?;
                    if node.text_content().is_none_or(|t| t.trim().is_empty()) {
                        continue;
                    }
                    let layout = node
                        .text_layout()
                        .ok_or("paged native source has no retained shape")?;
                    require(
                        layout.lines().len() > 0,
                        "paged native nonempty source has no lines",
                    )?;
                    nodes.insert(id, (root, layout.lines().len(), 0_usize));
                    snapshots.push(
                        json!({"root":root,"text_node":id,"lines":core::native_output(layout)}),
                    );
                }
            }
            // Keep actual text-node ranges. A merged IFC line count is not
            // invented for native paragraphs containing multiple text nodes.
            for fragment in pages {
                for item in &fragment.items {
                    let id = item.node_id.0 as usize;
                    let Some((root, total, end)) = nodes.get_mut(&id) else {
                        continue;
                    };
                    let Some(range) = item.line_range else {
                        continue;
                    };
                    require(
                        !item.is_repeat
                            && range.start as usize == *end
                            && range.end as usize <= *total,
                        "paged native line partition gap/overlap",
                    )?;
                    *end = range.end as usize;
                    partition.push(
                        json!({"root":root,"text_node":id,"page":fragment.page_index,
                        "line_start":range.start,"line_end":range.end}),
                    );
                }
            }
            require(
                !nodes.is_empty() && nodes.values().all(|(_, total, end)| total == end),
                "paged native fragment projection leaves source lines unassigned",
            )?;
            Ok(
                json!({"partition":partition,"snapshots":snapshots,"root_widths":widths,
                "page_count":pages.len(),"source_partition_complete":true,
                "retry_trace_visible":false,"actual_height_rejections":null,
                "actual_lookahead_rejections":null}),
            )
        }
    }
}

fn verify_input(page: &Page, case: &Value) -> Result<(), Box<dyn Error>> {
    require(
        serde_json::to_value(page.resources.records()?)? == case["resources"],
        "pagination original resources differ",
    )?;
    let warnings: Vec<_> = page
        .document
        .parsed
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .collect();
    require(
        json!(warnings) == case["parse_warnings"],
        "pagination original warnings differ",
    )
}

pub fn measure(input: Input<'_>, verify_fresh: bool) -> Result<Value, Box<dyn Error>> {
    // Capability validation belongs outside every timed/allocator scope.
    let (inputs, capability_preconditioning) = scope::measured(|| {
        let resources = WptResources::new(input.wpt)?;
        let document = resources.parse_screen(input.id)?;
        let inputs = candidate_page_inputs(&document, &input.fonts.candidate, [800, 128])?;
        preflight(
            &Page {
                document,
                resources,
                output: None,
            },
            &inputs,
        )?;
        Ok(inputs)
    })?;
    let (_, release_capability_inputs) = scope::measured(|| {
        drop(inputs);
        Ok(())
    })?;
    let mut samples = Vec::new();
    let mut expected = None;
    for index in 0..10 {
        let (mut page, initial) = scope::measured(|| start(&input, 128))?;
        verify_input(&page, input.case)?;
        let original = output(&page, input.roots)?;
        if let Some(previous) = &expected {
            require(previous == &original, "paged cold/warm output changed")?;
        }
        let mut heights = Vec::new();
        for height in [64, 32, 128] {
            let (_, measurement) = scope::measured(|| relayout(&mut page, &input, height))?;
            let actual = output(&page, input.roots)?;
            require(
                actual["snapshots"] == original["snapshots"],
                "paged source/glyph output changed with fragment height",
            )?;
            if verify_fresh {
                let fresh = start(&input, height)?;
                verify_input(&fresh, input.case)?;
                require(
                    output(&fresh, input.roots)? == actual,
                    "paged reentry differs from fresh actual preparation",
                )?;
            }
            if height == 128 {
                require(
                    actual == original,
                    "paged height restoration changed output",
                )?;
            }
            heights.push(
                json!({"viewport_css_px":[800,height],"measurement":measurement,"output":actual}),
            );
        }
        expected = Some(original);
        let (_, release) = scope::measured(|| {
            drop(page);
            Ok(())
        })?;
        samples.push(json!({"state":if index==0{"first-call-in-process"}else{"warm-process"},
            "parse_cascade_paginate":initial,"complete_caller_height_reentry":heights,"release_pipeline_owner":release}));
    }
    Ok(
        json!({"schema":1,"operation":"provisional-complete-caller-pagination","id":input.id,
        "engine":input.engine,"mode":if cfg!(feature="allocation-counting"){"memory"}else{"time"},
        "instrumented":cfg!(feature="allocation-counting"),"viewport_css_px":[800,128],
        "height_sequence_css_px":[128,64,32,128],"correctness_only":verify_fresh,
        "fresh_fragment_outputs_verified":verify_fresh,"samples":samples,"output":expected,
        "capability_preconditioning":capability_preconditioning,
        "release_capability_inputs":release_capability_inputs,
        "capability_boundary":"same original candidate CSS input/capability audit on both engines; separate from page caller costs; accounts shared ch font-cache setup and releases transient audit document/geometry",
        "resources":input.case["resources"],"parse_warnings":input.case["parse_warnings"],
        "font_sha256":input.case["font_sha256"],
        "scope":"initial: actual original offline parse/screen cascade/Ahem preflight and page layout; height reentry: same DOM/cascade, fresh actual native layout_page_fragments or original candidate CSS inputs/projection and preserved FlowDriver.paginate; serialization/raster excluded",
        "candidate_boundary":"new provisional consumer, distinct from the preserved static wrapper which cannot paginate; positive margins collapse between ordinary leaf roots; original body top margin remains present",
        "native_boundary":"actual pinned layout_page_fragments; native paged UA body-top-margin policy differs and oversized IFC widow/orphan semantics are limited; retry traces are not exposed, no fabricated rejection counts",
        "preconditioning":"fonts configured once; input/resource/geometry capability audits precede measurement; first-call-in-process is not cold startup",
        "retained_owner":if input.engine=="candidate"{"parsed/cascaded document/resources plus prepared IFCs, accepted paged lines, full flow checkpoints/traces and LayoutContext; shared configured font caches survive page release"}else{"parsed/cascaded native laid-out DOM/resources and actual PageFragment snapshots"},
        "full_comparison_complete":false}),
    )
}
