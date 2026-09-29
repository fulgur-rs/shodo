//! Original whole-layout samples, independent time and allocation builds.
#[cfg(feature = "allocation-counting")]
#[path = "../../bench/src/allocator.rs"]
mod allocator;
mod caller;
mod core;
mod isolated;
mod paged;
mod scope;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: allocator::CountingAllocator<std::alloc::System> =
    allocator::CountingAllocator::new(std::alloc::System);
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo_raikiri_integration::{
    BaselineLayout, CandidateBlock, WptFonts, WptResources, WptScreenDocument,
    layout_candidate_screen_page,
};
use std::{error::Error, path::Path};

enum Page<'a> {
    Native(BaselineLayout<'a>),
    Candidate(Vec<CandidateBlock>),
}

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}

fn fetch_web_faces(
    doc: &WptScreenDocument,
    resources: &WptResources,
) -> Result<(), Box<dyn Error>> {
    let tree = raikiri_html::build_rule_tree(&doc.parsed);
    for (family, face) in tree.font_faces().iter() {
        require(
            family.eq_ignore_ascii_case("Ahem")
                && face.style == raikiri_style::FontFaceStyle::Normal
                && face.weight == raikiri_style::FontFaceWeight::Normal
                && face.stretch.0.as_str() == "normal"
                && face.unicode_range.is_empty()
                && face.src.len() == 1
                && matches!(&face.src[0],raikiri_style::FontFaceSource::Url{url,..} if url.as_str()=="/fonts/Ahem.ttf"),
            "unrepresented original web-face registration",
        )?;
        resources.load_url("http://web-platform.test/fonts/Ahem.ttf")?;
    }
    Ok(())
}

fn layout<'a>(
    engine: &str,
    doc: &'a WptScreenDocument,
    fonts: &WptFonts,
    limits: &Limits,
) -> Result<Page<'a>, Box<dyn Error>> {
    match engine {
        "native" => {
            let mut page = raikiri_traits::PageBox::new();
            page.width = 800.0;
            page.height = 600.0;
            Ok(Page::Native(BaselineLayout::measure_screen(
                doc,
                page,
                fonts.baseline.clone(),
            )?))
        }
        "candidate" => Ok(Page::Candidate(layout_candidate_screen_page(
            doc,
            &fonts.candidate,
            [800, 600],
            limits,
        )?)),
        _ => Err("unknown measured engine".into()),
    }
}

fn output(page: &Page<'_>, roots: &[usize]) -> Result<Value, Box<dyn Error>> {
    let blocks: Vec<_> = match page {
        Page::Native(baseline) => roots.iter().map(|&root| {
            let geometry = baseline.block_geometry(root)?;
            Ok(json!({"node":root,"border_size":[geometry.border_inline_size,geometry.border_block_size],
                "content_width":geometry.content_inline_size}))
        }).collect::<Result<_,Box<dyn Error>>>()?,
        Page::Candidate(blocks) => blocks.iter().map(|b| json!({"node":b.node,
            "border_origin":b.border_origin,"border_size":[b.geometry.border_inline_size,b.geometry.border_block_size],
            "content_width":b.geometry.content_inline_size,"content_height":b.content_height,"line_count":b.line_count})).collect(),
    };
    Ok(json!(blocks))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    require(
        args.len() == 7 || args.len() == 8,
        "usage: measurement-probe MODE ENGINE WPT SELECTION ID OUTPUT [layout|pipeline|pipeline-check|isolated|isolated-check|pagination|pagination-check]",
    )?;
    let instrumented = cfg!(feature = "allocation-counting");
    require(
        args[1] == if instrumented { "memory" } else { "time" },
        "mode differs from the actual compiled instrumentation",
    )?;
    let engine = &args[2];
    let operation = args.get(7).map(String::as_str).unwrap_or("layout");
    require(
        matches!(
            operation,
            "layout"
                | "pipeline"
                | "pipeline-check"
                | "isolated"
                | "isolated-check"
                | "pagination"
                | "pagination-check"
        ),
        "unknown operation",
    )?;
    require(
        matches!(engine.as_str(), "native" | "candidate"),
        "unknown engine",
    )?;
    let selected: Value = serde_json::from_slice(&std::fs::read(&args[4])?)?;
    require(
        selected["wpt_revision"] == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e",
        "original selection WPT pin differs",
    )?;
    let case = selected["cases"]
        .as_array()
        .ok_or("missing cases")?
        .iter()
        .find(|c| c["id"] == args[5])
        .ok_or("original selected case missing")?;
    require(
        case["candidate_page"]["classification"] == "candidate-static-leaf-block-page",
        "original selected candidate page is unsupported",
    )?;
    let roots: Vec<_> = case["candidate_page"]["blocks"]
        .as_array()
        .ok_or("original blocks missing")?
        .iter()
        .map(|b| {
            b["node"]
                .as_u64()
                .map(|n| n as usize)
                .ok_or("original root missing")
        })
        .collect::<Result<_, _>>()?;
    require(!roots.is_empty(), "empty measured root set")?;
    let wpt = Path::new(&args[3]);
    let pin = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(wpt)
        .output()?;
    require(
        pin.status.success()
            && std::str::from_utf8(&pin.stdout)?.trim()
                == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e",
        "actual WPT checkout pin differs",
    )?;
    let resources = WptResources::new(wpt)?;
    let doc = resources.parse_screen(&args[5])?;
    fetch_web_faces(&doc, &resources)?;
    require(
        serde_json::to_value(resources.records()?)? == case["resources"],
        "original resource bytes/order differ",
    )?;
    let warnings: Vec<_> = doc
        .parsed
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .collect();
    require(
        json!(warnings) == case["parse_warnings"],
        "original parser warnings differ",
    )?;
    let limits = Limits::default();
    let (fonts, font_setup) = scope::measured(|| WptFonts::load(&wpt.join("fonts"), &limits))?;
    let font_hashes: Vec<_> = fonts
        .bytes
        .iter()
        .map(|b| format!("{:x}", Sha256::digest(b)))
        .collect();
    require(
        json!(font_hashes) == case["font_sha256"],
        "original font bytes/order differ",
    )?;
    let text_nodes = (0..doc.parsed.dom.node_count())
        .filter(|&node| {
            doc.parsed
                .dom
                .get_node(node)
                .is_some_and(|n| n.text_content().is_some())
        })
        .count();
    require(
        !instrumented || engine != "native" || text_nodes < 32,
        "native memory scope cannot prove the required single-threaded preshape job bound",
    )?;
    if matches!(operation, "pagination" | "pagination-check") {
        let mut report = paged::measure(
            caller::Input {
                wpt,
                id: &args[5],
                engine,
                case,
                fonts: &fonts,
                limits: &limits,
                roots: &roots,
            },
            operation == "pagination-check",
        )?;
        let (_, release_font_registry) = scope::measured(|| {
            drop(fonts);
            Ok(())
        })?;
        report["font_setup"] = font_setup;
        report["release_font_registry"] = release_font_registry;
        report["font_setup_boundary"] = json!(
            "common original WptFonts setup creates both registries/88font byte copies; final registry release includes shared lazy caches"
        );
        std::fs::write(&args[6], serde_json::to_vec_pretty(&report)?)?;
        return Ok(());
    }
    if matches!(operation, "pipeline" | "pipeline-check") {
        let mut report = caller::measure_pipeline(
            caller::Input {
                wpt,
                id: &args[5],
                engine,
                case,
                fonts: &fonts,
                limits: &limits,
                roots: &roots,
            },
            operation == "pipeline-check",
        )?;
        let (_, release_font_registry) = scope::measured(|| {
            drop(fonts);
            Ok(())
        })?;
        report["font_setup"] = font_setup;
        report["release_font_registry"] = release_font_registry;
        report["font_setup_boundary"] = json!(
            "common original WptFonts setup creates both native/candidate registries and retains original88font bytes; outside layout/shape windows; final registry release includes lazy shared caches"
        );
        std::fs::write(&args[6], serde_json::to_vec_pretty(&report)?)?;
        return Ok(());
    }
    if matches!(operation, "isolated" | "isolated-check") {
        let mut report = isolated::measure(
            caller::Input {
                wpt,
                id: &args[5],
                engine,
                case,
                fonts: &fonts,
                limits: &limits,
                roots: &roots,
            },
            &doc,
            operation == "isolated-check",
        )?;
        let (_, release_font_registry) = scope::measured(|| {
            drop(fonts);
            Ok(())
        })?;
        report["font_setup"] = font_setup;
        report["release_font_registry"] = release_font_registry;
        report["font_setup_boundary"] = json!(
            "common original WptFonts setup creates both native/candidate registries and retains original88font bytes; outside layout/shape windows; final registry release includes lazy shared caches"
        );
        std::fs::write(&args[6], serde_json::to_vec_pretty(&report)?)?;
        return Ok(());
    }
    let mut samples = Vec::new();
    let mut expected = None;
    for index in 0..10 {
        // The public native caller consumes its FontContext. Allocate that
        // owned input inside the same window in which it will be released.
        let (page, measurement) = scope::measured(|| layout(engine, &doc, &fonts, &limits))?;
        let actual = output(&page, &roots)?;
        if let Some(previous) = &expected {
            require(previous == &actual, "cold/warm actual output changed")?;
        }
        if engine == "candidate" {
            require(
                actual == case["candidate_page"]["blocks"],
                "original candidate block geometry changed",
            )?;
        }
        expected = Some(actual);
        let (_, release) = scope::measured(|| {
            drop(page);
            Ok(())
        })?;
        samples.push(
            json!({"state":if index==0{"first-call-in-process"}else{"warm-process"},
            "layout":measurement,"release_output":release}),
        );
    }
    let (_, release_font_registry) = scope::measured(|| {
        drop(fonts);
        Ok(())
    })?;
    let report = json!({"schema":1,"mode":args[1],"instrumented":instrumented,"engine":engine,
        "font_setup":font_setup,"release_font_registry":release_font_registry,
        "font_setup_boundary":"common original WptFonts setup creates both native/candidate registries and retains original88font bytes; outside layout windows; final registry release includes lazy shared caches",
        "id":args[5],"operation":"whole-layout-initial","viewport_css_px":[800,600],
        "scope":"parsed original screen input and configured font registry are outside layout; actual public native layout or original candidate layout-only pass; raster/serialization excluded",
        "retained_owner":if engine=="native"{"native laid-out DOM clone and atomic map"}else{"candidate block geometry plus lazy configured FontCollection caches; paragraph/line/context released inside caller"},
        "retained_memory_semantics":"counts.net_bytes is the call's requested heap change, including changes to shared configured font caches; release_output releases only the page output, retaining fonts/caches for subsequent warm calls",
        "preconditioning":"input parsed/fonts configured once; native FontContext cloned inside every layout window; both callers create fresh layout contexts; first-call and warm-process are not claimed as reused native LayoutContext",
        "samples":samples,"output":expected,"resources":resources.records()?,"parse_warnings":warnings,
        "font_sha256":font_hashes,
        "full_comparison_complete":false});
    std::fs::write(&args[6], serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
