//! Single-threaded caller windows; serialization/oracles stay outside scopes.
use super::{caller, retained, snapshot};
use serde_json::{Value, json};
use shodo::{AtomicSizes, LayoutContext, font::FontCollection, limits::Warning};
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[path = "../../../bench/src/allocator.rs"]
mod allocator;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: allocator::CountingAllocator<std::alloc::System> =
    allocator::CountingAllocator::new(std::alloc::System);

// The disposable trace overlay replaces these two hooks. Shipping timing
// builds execute no shaper counters; the archive includes the exact overlay.
fn trace_reset() {}
fn trace_read() -> Option<[u64; 4]> {
    None
}
fn measured<T>(f: impl FnOnce() -> Result<T, String>) -> Result<(T, Value), String> {
    trace_reset();
    #[cfg(feature = "allocation-counting")]
    let allocation = ALLOC.begin().map_err(|e| e.to_string())?;
    let start = Instant::now();
    let result = f()?;
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let counts = Some(allocation.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let counts: Option<Value> = None;
    let trace = trace_read();
    Ok((result, json!({"ns":ns,"allocation":counts,"shape":trace})))
}

enum Accepted {
    Width(caller::Output, Vec<Warning>),
    Height(retained::PagedOutput, Vec<Warning>),
}
impl Accepted {
    fn payload(&self) -> Value {
        let (mut pages, warnings, extra) = match self {
            Self::Width(output, warnings) => (vec![snapshot::output(output)], warnings, None),
            Self::Height(output, warnings) => (
                output.pages.iter().map(snapshot::output).collect(),
                warnings,
                Some((output.height_retries, output.oversize_lines)),
            ),
        };
        // Lossless text interning keeps whole processed source strings once,
        // rather than repeating them for every line/page in long inputs.
        let mut texts = Vec::<Value>::new();
        let mut mappings = Vec::<Value>::new();
        for page in &mut pages {
            let table = page.as_object_mut().unwrap().remove("mappings").unwrap();
            let translated = table
                .as_array()
                .unwrap()
                .iter()
                .map(|value| {
                    if let Some(i) = mappings.iter().position(|m| m == value) {
                        i
                    } else {
                        mappings.push(value.clone());
                        mappings.len() - 1
                    }
                })
                .collect::<Vec<_>>();
            for line in page["lines"].as_array_mut().unwrap() {
                if let Some(i) = line["mapping_index"].as_u64() {
                    line["mapping_index"] = json!(translated[i as usize]);
                }
            }

            let text = page.as_object_mut().unwrap().remove("text").unwrap();
            let index = if let Some(i) = texts.iter().position(|t| t == &text) {
                i
            } else {
                texts.push(text);
                texts.len() - 1
            };
            page["text_index"] = json!(index);
        }
        let mut result = json!({"texts":texts,"mappings":mappings,"pages":pages,"warnings":format!("{warnings:?}")});
        if let Some((retries, oversize)) = extra {
            result["height_retries"] = json!(retries);
            result["oversize_lines"] = json!(oversize);
        }
        result
    }
    fn tokens(&self) -> Vec<String> {
        match self {
            Self::Width(o, _) => o
                .lines
                .iter()
                .map(|l| format!("{:?}", l.break_token()))
                .collect(),
            Self::Height(o, _) => o.page_tokens.iter().map(|t| format!("{t:?}")).collect(),
        }
    }
}

fn execute(
    input: &caller::ResolvedInput,
    fonts: &FontCollection,
    context: &mut LayoutContext,
    session: &mut Option<retained::RetainedCaller>,
    reused_context: bool,
    width: f32,
    height: Option<f32>,
) -> Result<Accepted, String> {
    if let Some(session) = session {
        let output = match height {
            Some(h) => Accepted::Height(session.layout_height(width, h)?, session.take_warnings()),
            None => Accepted::Width(session.layout_width(width)?, session.take_warnings()),
        };
        let mut output = output;
        match &mut output {
            Accepted::Width(_, warnings) | Accepted::Height(_, warnings) => {
                let mut build = session.paragraph_warnings().to_vec();
                build.append(warnings);
                *warnings = build;
            }
        }
        return Ok(output);
    }
    let mut fresh = LayoutContext::new();
    let context = if reused_context { context } else { &mut fresh };
    let p = caller::prepare(input, context, fonts, caller::FontPolicy::FixtureLatin)?;
    let mut warnings = p.paragraph.warnings().to_vec();
    let output = if let Some(h) = height {
        let o = retained::paginate(&p, context, width, h)?;
        warnings.extend(context.take_warnings());
        Accepted::Height(o, warnings)
    } else {
        let lines = p
            .paragraph
            .break_all(context, &Default::default(), width, &AtomicSizes::EMPTY);
        let o = p.output(lines)?;
        warnings.extend(context.take_warnings());
        Accepted::Width(o, warnings)
    };
    Ok(output)
}

pub fn inputs() -> Vec<(String, String, String)> {
    let mut result = Vec::new();
    let base = "#root{font-family:'Shodo Fixture Latin';font-size:16px}";
    for repeats in [1, 128] {
        for (name, part, css) in [
            ("plain", "ffi abc def ghi ", base.to_string()),
            (
                "links",
                "<span>f</span><a href='/target'>f</a><span>i abc </span>",
                base.to_string(),
            ),
            (
                "styles",
                "<span style='font-size:18px'>ffi </span><a href='/abc' style='font-size:20px'>abc </a><span>def </span>",
                base.to_string(),
            ),
            (
                "first-line",
                "<a href='/first'>Straße ffi abc </a><span>def ghi </span>",
                format!(
                    "{base}#root{{text-transform:uppercase}}#root::first-line{{font-size:32px;color:red}}"
                ),
            ),
        ] {
            result.push((format!("{name}/{repeats}"), part.repeat(repeats), css));
        }
    }
    result
}
fn resolve(body: &str, css: &str) -> Result<caller::ResolvedInput, String> {
    caller::resolve_html(
        &format!("<style>{css}</style><div id=root>{body}</div>"),
        "root",
    )
}

pub fn run() -> Result<(), String> {
    let fonts = shodo_fixtures::load_fonts(&Default::default()).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let paths = if std::env::args().any(|a| a == "--reverse-path-order") {
        ["retained", "reused-context", "fresh"]
    } else {
        ["fresh", "reused-context", "retained"]
    };

    for (id, body, css) in inputs() {
        let input = resolve(&body, &css)?;
        // Font resources and a full fresh caller are preconditioned outside
        // the windows. This is not process startup or cold filesystem time.
        drop(caller::layout(&input, &fonts.collection, 400.0)?);
        for height_mode in [false, true] {
            let dimensions = if height_mode {
                [25.0, 100.0, 0.0, 100.0]
            } else {
                [400.0, 80.0, 160.0, 400.0]
            };
            // Correctness/preconditioning oracle outside every measured window.
            let mut expected = Vec::new();
            for dimension in dimensions {
                let mut oracle_context = LayoutContext::new();
                let output = execute(
                    &input,
                    &fonts.collection,
                    &mut oracle_context,
                    &mut None,
                    false,
                    if height_mode { 160.0 } else { dimension },
                    height_mode.then_some(dimension),
                )?;
                expected.push(output.payload());
            }
            for path in paths {
                let mut context = LayoutContext::new();
                let mut session = if path == "retained" {
                    Some(retained::RetainedCaller::new(
                        resolve(&body, &css)?,
                        fonts.collection.clone(),
                        None,
                        caller::FontPolicy::FixtureLatin,
                    ))
                } else {
                    None
                };
                let (initial, setup) = measured(|| {
                    execute(
                        &input,
                        &fonts.collection,
                        &mut context,
                        &mut session,
                        path == "reused-context",
                        400.0,
                        None,
                    )
                })?;
                let (_, setup_output_release) = measured(|| {
                    drop(initial);
                    Ok(())
                })?;
                for (step, dimension) in if height_mode {
                    [25.0, 100.0, 0.0, 100.0]
                } else {
                    [400.0, 80.0, 160.0, 400.0]
                }
                .into_iter()
                .enumerate()
                {
                    let width = if height_mode { 160.0 } else { dimension };
                    let height = height_mode.then_some(dimension);
                    for _ in 0..4 {
                        drop(execute(
                            &input,
                            &fonts.collection,
                            &mut context,
                            &mut session,
                            path == "reused-context",
                            width,
                            height,
                        )?);
                    }
                    let mut signature = None;
                    let mut samples = Vec::new();
                    for _ in 0..10 {
                        let (output, mut sample) = measured(|| {
                            execute(
                                &input,
                                &fonts.collection,
                                &mut context,
                                &mut session,
                                path == "reused-context",
                                width,
                                height,
                            )
                        })?;
                        let payload = output.payload();
                        if let Some(previous) = &signature {
                            if previous != &payload {
                                return Err(format!("sample output changed: {id}/{path}/{step}"));
                            }
                        } else {
                            signature = Some(payload);
                        }
                        sample["tokens_debug"] = json!(output.tokens());
                        samples.push(sample);
                        drop(output);
                    }
                    let signature = signature.unwrap();
                    if expected[step] != signature {
                        return Err(format!("fresh caller output differs: {id}/{path}/{step}"));
                    }
                    rows.push(json!({"id":format!("{id}/{}/{step}/{path}",if height_mode{"height"}else{"width"}),"case":id,"html_body":body,"css":css,"path":path,"operation":if height_mode{"height"}else{"width"},"width":width,"height":height,"step":step,"signature":signature,"samples":samples,"setup":setup,"setup_output_release":setup_output_release}));
                }
                // Includes the session's pre-parsed immutable input, allocated
                // before setup; therefore this is not the inverse of setup net.
                let (_, release) = measured(|| {
                    drop(session);
                    drop(context);
                    Ok(())
                })?;
                for row in rows.iter_mut().rev().take(4) {
                    row["release_owner"] = release.clone();
                }
            }
        }
    }
    let output = json!({"schema":1,"issue":"shodo-sbp.14","instrumented":cfg!(feature="allocation-counting"),"traced":trace_read().is_some(),"path_order":paths,"scope":"single-thread caller preparation/break/index/link; parsed/cascaded input and font registration, warmups, payload/serialization and output drops outside operation windows; initial setup and owner/output release reported separately","retention":"setup net plus setup output release is incremental caller scratch/prepared retention; final owner release also frees retained session parsed input allocated before setup; warm operation net includes live accepted output and changes to existing caches; requested bytes are not RSS","rows":rows});
    println!(
        "{}",
        serde_json::to_string(&output).map_err(|e| e.to_string())?
    );
    Ok(())
}
