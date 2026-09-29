//! Fresh actual library construction, with exclusive preparation/library phases.
#[cfg(feature = "allocation-counting")]
#[path = "../../bench/src/allocator.rs"]
mod allocator;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: allocator::CountingAllocator<std::alloc::System> =
    allocator::CountingAllocator::new(std::alloc::System);
mod scope;
// Share the real native serializer; retry helpers belong to the other probe.
#[allow(dead_code)]
mod core;
use raikiri_traits::Dom;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{LayoutContext, Line, limits::Limits};
use shodo_benchmark_observer::Event;
use shodo_raikiri_integration::{
    CandidateBlockInput, DocumentProjection, PreparedIfc, WptFonts, WptResources,
    WptScreenDocument, candidate_page_inputs, snapshot_candidate,
};
use std::{cell::RefCell, collections::BTreeMap, error::Error, path::Path, rc::Rc};

fn require(ok: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if ok {
        Ok(())
    } else {
        Err(message.to_owned().into())
    }
}
struct Active {
    tag: Value,
    #[cfg(feature = "allocation-counting")]
    counter: allocator::Scope<'static>,
    #[cfg(not(feature = "allocation-counting"))]
    start: std::time::Instant,
}
impl Active {
    fn new(tag: Value) -> Self {
        Self {
            tag,
            #[cfg(feature = "allocation-counting")]
            counter: ALLOCATOR
                .begin()
                .expect("exclusive library phase overlaps allocator scope"),
            #[cfg(not(feature = "allocation-counting"))]
            start: std::time::Instant::now(),
        }
    }
    fn finish(self) -> Value {
        #[cfg(feature = "allocation-counting")]
        let measurement = {
            let counts = self.counter.finish();
            json!({"counts":counts})
        };
        #[cfg(not(feature = "allocation-counting"))]
        let measurement = {
            let elapsed = self.start.elapsed().as_nanos();
            json!({"duration_ns":u64::try_from(elapsed).expect("duration overflow")})
        };
        let mut tag = self.tag;
        tag["measurement"] = measurement;
        tag
    }
}
struct Profile {
    active: Option<Active>,
    rows: Vec<Value>,
    pairs: BTreeMap<usize, usize>,
    engine: String,
}
impl Profile {
    fn stop(&mut self) {
        let active = self.active.take().expect("missing exclusive phase");
        // Finish before serializing/storing phase metadata. No observer JSON
        // allocation or source digest work enters the library or prep windows.
        self.rows.push(active.finish());
    }
    fn prep(&mut self) {
        self.active = Some(Active::new(json!({"kind":"preparation"})));
    }
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start { engine, node, text } => {
                assert_eq!(engine, self.engine, "wrong actual library observer");
                assert_eq!(
                    self.active.as_ref().unwrap().tag["kind"],
                    "preparation",
                    "nested/unobserved library construction"
                );
                self.stop();
                let tag = json!({"kind":"library","node":node,"paired_root":self.pairs.get(&node),
                    "input_bytes":text.len(),"input_sha256":format!("{:x}",Sha256::digest(text.as_bytes()))});
                self.active = Some(Active::new(tag));
            }
            Event::End => {
                assert_eq!(
                    self.active.as_ref().unwrap().tag["kind"],
                    "library",
                    "library end without start"
                );
                self.stop();
                self.prep();
            }
        }
    }
}
enum Prepared {
    Candidate(LayoutContext, Vec<(usize, PreparedIfc, Vec<Line>)>),
    Native(raikiri_dom::Document),
}

fn prepare(
    engine: &str,
    document: &WptScreenDocument,
    blocks: &[CandidateBlockInput],
    fonts: &WptFonts,
    limits: &Limits,
) -> Result<Prepared, Box<dyn Error>> {
    if engine == "native" {
        let mut owned = document.parsed.dom.clone();
        raikiri_dom::relayout_text_for_width(
            &mut owned,
            &document.cascade,
            800.0,
            800.0,
            fonts.baseline.clone(),
        );
        Ok(Prepared::Native(owned))
    } else {
        let projection = DocumentProjection::new_screen(document);
        let mut context = LayoutContext::new();
        let mut rows = Vec::new();
        for block in blocks {
            let p = projection.build_ifc_in_block(
                block.node,
                &mut context,
                &fonts.candidate,
                limits,
                block.geometry,
            )?;
            let lines = p.paragraph.break_all(
                &mut context,
                &p.line_options(block.geometry.content_inline_size),
                block.geometry.content_inline_size,
                &p.atomics,
            );
            rows.push((block.node, p, lines));
        }
        Ok(Prepared::Candidate(context, rows))
    }
}
fn output(
    prepared: &Prepared,
    pairs: &BTreeMap<usize, usize>,
    roots: &[usize],
) -> Result<Value, Box<dyn Error>> {
    let rows=match prepared {
        Prepared::Candidate(_context,ifcs)=>ifcs.iter().map(|(root,p,lines)| {
            let mut end=0;
            for line in lines { require(line.text_range().start==end,"library candidate source gap")?;end=line.text_range().end; }
            require(end==p.paragraph.text().len(),"library candidate source remains unconsumed")?;
            Ok(json!({"root":root,"processed_source_bytes":end,"snapshot":snapshot_candidate(p,lines)?}))
        }).collect::<Result<Vec<_>,Box<dyn Error>>>()?,
        Prepared::Native(document)=>{
            let mut rows=Vec::new();
            for (&node,&root) in pairs {
                let Some(layout)=document.get_node(node).and_then(|n|n.text_layout()) else {continue;};
                if layout.lines().len()==0 {continue;}
                rows.push(json!({"root":root,"text_node":node,"snapshot":core::native_output(layout)}));
            }
            require(roots.iter().all(|root|rows.iter().any(|row|row["root"]==*root)),"library native missing paired root output")?;
            rows
        }
    };
    Ok(json!(rows))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    require(
        args.len() == 7,
        "usage: library-probe MODE ENGINE WPT SELECTION ID OUTPUT",
    )?;
    let memory = cfg!(feature = "allocation-counting");
    require(
        args[1] == if memory { "memory" } else { "time" },
        "library mode differs from actual instrumentation",
    )?;
    let engine = &args[2];
    require(
        matches!(engine.as_str(), "candidate" | "native"),
        "unknown library engine",
    )?;
    let selection: Value = serde_json::from_slice(&std::fs::read(&args[4])?)?;
    require(
        selection["wpt_revision"] == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e",
        "wrong original WPT pin",
    )?;
    let case = selection["cases"]
        .as_array()
        .ok_or("missing selection")?
        .iter()
        .find(|c| c["id"] == args[5])
        .ok_or("case not selected")?;
    require(
        case["candidate_page"]["classification"] == "candidate-static-leaf-block-page",
        "original candidate unsupported",
    )?;
    let wpt = Path::new(&args[3]);
    let pin = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(wpt)
        .output()?;
    require(
        pin.status.success()
            && std::str::from_utf8(&pin.stdout)?.trim()
                == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e",
        "actual WPT pin differs",
    )?;
    let resources = WptResources::new(wpt)?;
    let document = resources.parse_screen(&args[5])?;
    let tree = raikiri_html::build_rule_tree(&document.parsed);
    for (family, face) in tree.font_faces().iter() {
        require(
            family.eq_ignore_ascii_case("Ahem")
                && face.style == raikiri_style::FontFaceStyle::Normal
                && face.weight == raikiri_style::FontFaceWeight::Normal
                && face.stretch.0.as_str() == "normal"
                && face.unicode_range.is_empty()
                && face.src.len() == 1
                && matches!(&face.src[0],raikiri_style::FontFaceSource::Url{url,..} if url.as_str()=="/fonts/Ahem.ttf"),
            "unrepresented original web face",
        )?;
        resources.load_url("http://web-platform.test/fonts/Ahem.ttf")?;
    }
    require(
        serde_json::to_value(resources.records()?)? == case["resources"],
        "original resources differ",
    )?;
    let warnings: Vec<_> = document
        .parsed
        .warnings
        .iter()
        .map(|w| format!("{w:?}"))
        .collect();
    require(
        json!(warnings) == case["parse_warnings"],
        "original warnings differ",
    )?;
    let text_nodes = (0..document.parsed.dom.node_count())
        .filter(|&n| {
            document
                .parsed
                .dom
                .get_node(n)
                .is_some_and(|n| n.text_content().is_some())
        })
        .count();
    require(
        engine != "native" || text_nodes < 32,
        "native library observation requires the proven sequential job bound",
    )?;
    let limits = Limits::default();
    let (fonts, font_setup) = scope::measured(|| WptFonts::load(&wpt.join("fonts"), &limits))?;
    let hashes: Vec<_> = fonts
        .bytes
        .iter()
        .map(|b| format!("{:x}", Sha256::digest(b)))
        .collect();
    require(
        json!(hashes) == case["font_sha256"],
        "original font registry differs",
    )?;
    let (inputs, input_geometry) = scope::measured(|| {
        Ok(candidate_page_inputs(
            &document,
            &fonts.candidate,
            [800, 600],
        )?)
    })?;
    let roots: Vec<_> = inputs.blocks.iter().map(|b| b.node).collect();
    require(
        json!(roots)
            == json!(
                case["candidate_page"]["blocks"]
                    .as_array()
                    .ok_or("missing blocks")?
                    .iter()
                    .map(|b| b["node"].as_u64().unwrap())
                    .collect::<Vec<_>>()
            ),
        "original root IDs differ",
    )?;
    let mut native_pairs = BTreeMap::new();
    for &root in &roots {
        for child in document
            .parsed
            .dom
            .child_ids(raikiri_traits::NodeId::new(root as u64))
        {
            if document
                .parsed
                .dom
                .get_node(child.0 as usize)
                .is_some_and(|n| n.text_content().is_some())
            {
                native_pairs.insert(child.0 as usize, root);
            }
        }
    }
    let pairs = if engine == "candidate" {
        roots.iter().map(|&r| (r, r)).collect()
    } else {
        native_pairs.clone()
    };
    let mut samples = Vec::new();
    let mut expected = None;
    for index in 0..10 {
        let profile = Rc::new(RefCell::new(Profile {
            active: None,
            rows: Vec::new(),
            pairs: pairs.clone(),
            engine: engine.clone(),
        }));
        let callback = profile.clone();
        let hook = shodo_benchmark_observer::install(Box::new(move |event| {
            callback.borrow_mut().event(event)
        }));
        profile.borrow_mut().prep();
        let result = prepare(engine, &document, &inputs.blocks, &fonts, &limits);
        profile.borrow_mut().stop();
        drop(hook);
        let prepared = result?;
        let actual = output(&prepared, &native_pairs, &roots)?;
        if let Some(previous) = &expected {
            require(
                previous == &actual,
                "fresh library cold/warm output differs",
            )?;
        }
        require(
            roots.iter().all(|root| {
                profile
                    .borrow()
                    .rows
                    .iter()
                    .any(|row| row["kind"] == "library" && row["paired_root"] == *root)
            }),
            "actual paired library construction not observed",
        )?;
        let (_, released) = scope::measured(|| {
            drop(prepared);
            Ok(())
        })?;
        let phases = std::mem::take(&mut profile.borrow_mut().rows);
        samples.push(json!({"state":if index==0{"first-call-in-process"}else{"warm-process"},"exclusive_phases":phases,"release_prepared_owner":released,"output":actual}));
        expected = Some(actual);
    }
    let (_, release_input_geometry) = scope::measured(|| {
        drop(inputs);
        Ok(())
    })?;
    let (_, release_font_registry) = scope::measured(|| {
        drop(fonts);
        Ok(())
    })?;
    let report = json!({"schema":1,"operation":"initial-library-construction-phases","id":args[5],"engine":engine,"mode":args[1],"instrumented":memory,
        "viewport_css_px":[800,600],"font_setup":font_setup,"input_geometry":input_geometry,"release_input_geometry":release_input_geometry,"release_font_registry":release_font_registry,
        "font_sha256":hashes,"resources":case["resources"],"parse_warnings":case["parse_warnings"],"samples":samples,"initial_output":expected,
        "scope":"fresh original DOM/style preparation and exclusive observed real library calls; candidate ParagraphBuilder.build; native sequential actual ranged_builder/default-style API/build batch; initial lines occur in preparation after construction",
        "library_input_semantics":"candidate builder owns original raw source and inline tree; native final jobs own caller-processed strings/defaults; byte/source digests preserve this difference, no fabricated common input or pure glyph-shaping ratio",
        "allocation_semantics":"nonoverlapping prep/library windows plus named output/geometry/font release; net within a library call includes output/cache deltas later released in preparation/owner/registry; not standalone equivalent retained-output sizes",
        "observer_overhead":"observer source hashing/JSON/storage happens between windows; thread-local callback dispatch at window boundaries is included, no claim of instruction-level glyph-shaping cost",
        "preconditioning":"original parse/cascade/fonts and original CSS input sizing precede first sample; first-call-in-process is not cold startup; fresh shapes every sample",
        "pure_glyph_shaping_ratio":null,"full_comparison_complete":false});
    std::fs::write(&args[6], serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
