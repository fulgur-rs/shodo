//! Actual parse/cascade/layout and complete-caller width re-entrance.
use crate::{fetch_web_faces, require, scope};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo_raikiri_integration::{
    CandidateBlock, WptFonts, WptResources, WptScreenDocument, layout_candidate_screen_page,
};
use std::{error::Error, path::Path};

pub struct Input<'a> {
    pub wpt: &'a Path,
    pub id: &'a str,
    pub engine: &'a str,
    pub case: &'a Value,
    pub fonts: &'a WptFonts,
    pub limits: &'a Limits,
    pub roots: &'a [usize],
}

struct Page {
    document: WptScreenDocument,
    resources: WptResources,
    blocks: Option<Vec<CandidateBlock>>,
}

fn relayout(page: &mut Page, input: &Input<'_>, width: u32) -> Result<(), Box<dyn Error>> {
    if input.engine == "native" {
        let doc = &mut page.document;
        let text_nodes = (0..doc.parsed.dom.node_count())
            .filter(|&node| {
                doc.parsed
                    .dom
                    .get_node(node)
                    .is_some_and(|n| n.text_content().is_some())
            })
            .count();
        require(
            !cfg!(feature = "allocation-counting") || text_nodes < 32,
            "native memory scope cannot prove the required single-threaded preshape job bound",
        )?;
        let mut geometry = raikiri_traits::PageBox::new();
        geometry.width = width as f32;
        geometry.height = 600.0;
        raikiri_dom::layout_single_page(
            &mut doc.parsed.dom,
            &doc.cascade,
            geometry,
            input.fonts.baseline.clone(),
        )
        .map_err(|e| format!("actual native layout failed: {e:?}"))?;
    } else {
        page.blocks = Some(layout_candidate_screen_page(
            &page.document,
            &input.fonts.candidate,
            [width, 600],
            input.limits,
        )?);
    }
    Ok(())
}

fn start(input: &Input<'_>, width: u32) -> Result<Page, Box<dyn Error>> {
    let resources = WptResources::new(input.wpt)?;
    let document = resources.parse_screen(input.id)?;
    fetch_web_faces(&document, &resources)?;
    let mut page = Page {
        document,
        resources,
        blocks: None,
    };
    relayout(&mut page, input, width)?;
    Ok(page)
}

fn block_output(page: &Page, roots: &[usize]) -> Result<Value, Box<dyn Error>> {
    let blocks: Vec<_> = if let Some(blocks) = &page.blocks {
        blocks
            .iter()
            .map(|b| {
                json!({"node":b.node,
            "border_origin":b.border_origin,
            "border_size":[b.geometry.border_inline_size,b.geometry.border_block_size],
            "content_width":b.geometry.content_inline_size,
            "content_height":b.content_height,"line_count":b.line_count})
            })
            .collect()
    } else {
        roots.iter().map(|&root| {
            let node = page.document.parsed.dom.get_node(root).ok_or("missing actual native root")?;
            let b = &node.unrounded_layout;
            Ok(json!({"node":root,"border_size":[b.size.width,b.size.height],
                "content_width":(b.size.width-b.border.left-b.border.right-b.padding.left-b.padding.right).max(0.0)}))
        }).collect::<Result<_, Box<dyn Error>>>()?
    };
    Ok(json!(blocks))
}

fn verify_input(page: &Page, case: &Value) -> Result<(), Box<dyn Error>> {
    require(
        serde_json::to_value(page.resources.records()?)? == case["resources"],
        "pipeline original resource bytes/order differ",
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
        "pipeline original parser warnings differ",
    )
}

pub fn measure_pipeline(input: Input<'_>, verify_fresh: bool) -> Result<Value, Box<dyn Error>> {
    let mut samples = Vec::new();
    let mut expected = None;
    for index in 0..10 {
        let (mut page, initial) = scope::measured(|| start(&input, 800))?;
        verify_input(&page, input.case)?;
        let original = block_output(&page, input.roots)?;
        if input.engine == "candidate" {
            require(
                original == input.case["candidate_page"]["blocks"],
                "pipeline initial candidate block geometry changed",
            )?;
        }
        if let Some(previous) = &expected {
            require(previous == &original, "pipeline cold/warm output changed")?;
        }
        let mut widths = Vec::new();
        for width in [400, 1200, 800] {
            let (_, measured) = scope::measured(|| relayout(&mut page, &input, width))?;
            let actual = block_output(&page, input.roots)?;
            if verify_fresh {
                // Correctness-only CLI: never used as a timing comparison.
                // Fresh-width preparation is outside every measured window.
                let fresh = start(&input, width)?;
                verify_input(&fresh, input.case)?;
                require(
                    block_output(&fresh, input.roots)? == actual,
                    &format!("complete caller reentry differs from fresh layout at width {width}"),
                )?;
            }
            if width == 800 {
                require(
                    actual == original,
                    "complete caller width restoration changed output",
                )?;
            }
            widths.push(json!({"viewport_css_px":[width,600],"measurement":measured,
                "output_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&actual)?)),
                "output":actual}));
        }
        expected = Some(original);
        let (_, released) = scope::measured(|| {
            drop(page);
            Ok(())
        })?;
        samples.push(
            json!({"state":if index==0{"first-call-in-process"}else{"warm-process"},
            "parse_cascade_layout":initial,"complete_caller_width_reentry":widths,
            "release_pipeline_owner":released}),
        );
    }
    Ok(
        json!({"schema":1,"operation":"parse-cascade-layout-and-width-reentry",
        "id":input.id,"engine":input.engine,"mode":if cfg!(feature="allocation-counting"){"memory"}else{"time"},
        "instrumented":cfg!(feature="allocation-counting"),"viewport_css_px":[800,600],
        "correctness_only":verify_fresh,"fresh_width_outputs_verified":verify_fresh,
        "width_sequence_css_px":[800,400,1200,800],
        "scope":"initial: actual offline resource parse/screen cascade/Ahem face preflight and actual caller layout; width reentry: same owned DOM/cascade, native layout_single_page or original candidate layout-only pass; fresh shapes on both paths; no raster/serialization",
        "preconditioning":"fonts configured once; an unmeasured original parse/cascade/input audit precedes the first pipeline call, so parsing/filesystem caches are preconditioned; native FontContext cloned inside consuming calls; first-call-in-process is not cold-process startup",
        "retained_owner":if input.engine=="native"{"parsed/cascaded document with actual native laid-out DOM and resource trace"}else{"parsed/cascaded document, candidate block geometry and resource trace, plus lazy configured FontCollection caches; paragraphs/lines/context released in caller"},
        "shared_font_cache_memory":"net bytes include lazy configured FontCollection cache growth; release_pipeline_owner releases the document/block output/resource trace while configured fonts and caches remain for warm calls",
        "width_memory_semantics":"net bytes are changes to an existing retained owner, not standalone output size; final release frees the whole pipeline owner",
        "samples":samples,"output":expected,"resources":input.case["resources"],
        "parse_warnings":input.case["parse_warnings"],"font_sha256":input.case["font_sha256"],
        "full_comparison_complete":false}),
    )
}
