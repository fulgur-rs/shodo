//! Single-threaded scopes. Parsing, JSON, controls and output drops are separate.
use super::{caller, snapshot};
use serde_json::{Value, json};
use shodo::{AtomicSizes, LayoutContext, hit::LineLayout};
use std::{hint::black_box, time::Instant};
#[cfg(feature = "allocation-counting")]
#[path = "../../../bench/src/allocator.rs"]
mod allocator;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: allocator::CountingAllocator<std::alloc::System> =
    allocator::CountingAllocator::new(std::alloc::System);
// Disposable instrumentation replaces these hooks; adopted timings have no visit counters.
fn trace_reset() {}
fn trace_read() -> Option<Value> {
    None
}
fn measured<T>(f: impl FnOnce() -> T) -> (T, Value) {
    trace_reset();
    #[cfg(feature = "allocation-counting")]
    let allocation = ALLOC.begin().unwrap();
    let start = Instant::now();
    let result = black_box(f());
    let ns = start.elapsed().as_nanos() as u64;
    #[cfg(feature = "allocation-counting")]
    let counts = Some(allocation.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let counts: Option<Value> = None;
    (
        result,
        json!({"ns":ns,"allocation":counts,"trace":trace_read()}),
    )
}
pub fn inputs() -> Vec<(String, String, String)> {
    let mut cases = Vec::new();
    let base = "#root{font-family:'Shodo Fixture Latin';font-size:16px}a{color:blue;text-decoration-line:underline;text-decoration-color:lime;text-decoration-thickness:2px}";
    for repeat in [1, 128] {
        for (name, piece, css) in [
            ("single-node", "abc ffi def ", ""),
            (
                "many-nodes",
                "<span>abc </span><span>ffi </span><span>def </span>",
                "",
            ),
            (
                "many-links",
                "<a href='/one'>abc </a><a href='/two'><span>ffi </span></a><span>def </span>",
                "",
            ),
            (
                "expanded",
                "<a href='/expanded'>Straße ß </a><span>ffi </span>",
                "#root{text-transform:uppercase}",
            ),
            (
                "rtl-runs",
                "<a href='/rtl'>مرحبا </a><span>abc </span>",
                "#root{font-family:'Shodo Fixture Arabic'}",
            ),
            (
                "nowrap",
                "<a href='/nowrap'>abc ffi def </a><span>ghi </span>",
                "#root{text-wrap-mode:nowrap}",
            ),
            (
                "partial-glyph",
                "<span>f</span><a href='/partial'>f</a><span>i </span>",
                "#root::first-line{font-size:32px}",
            ),
            (
                "collapsed-link",
                "<span>abc </span><a href='/empty'> </a><span>def </span>",
                "",
            ),
        ] {
            cases.push((
                format!("{name}/{repeat}"),
                piece.repeat(repeat),
                format!("{base}{css}"),
            ));
        }
    }
    cases
}
pub fn run() -> Result<(), String> {
    let fonts = shodo_fixtures::load_fonts(&Default::default()).map_err(|e| e.to_string())?;
    let mut inputs = inputs();
    if std::env::args().any(|s| s == "--reverse-cases") {
        inputs.reverse();
    }
    let mut rows = Vec::new();
    for (case, body, css) in inputs {
        let input = caller::resolve_html(
            &format!("<style>{css}</style><div id=root>{body}</div>"),
            "root",
        )?;
        for width in [80.0, 400.0] {
            // Warm the same immutable source; fresh context remains the measured caller policy.
            let _ = caller::layout_with_font_policy(
                &input,
                &fonts.collection,
                width,
                caller::FontPolicy::BundledWpt,
            )?;
            let mut signature = None;
            let mut samples = Vec::new();
            for _ in 0..7 {
                let mut context = LayoutContext::new();
                let (prepared, prepare) = measured(|| {
                    caller::prepare(
                        &input,
                        &mut context,
                        &fonts.collection,
                        caller::FontPolicy::BundledWpt,
                    )
                });
                let prepared = prepared?;
                let (lines, line_break) = measured(|| {
                    prepared.paragraph.break_all(
                        &mut context,
                        &Default::default(),
                        width,
                        &AtomicSizes::EMPTY,
                    )
                });
                // This standalone index is a control; never include it in the caller's total.
                let (index, index_control) = measured(|| LineLayout::new(&lines));
                let (_, index_release) = measured(|| drop(index));
                let (output, projection) = measured(|| prepared.output(lines));
                let output = output?;
                let payload = json!({"output":snapshot::output(&output),"paragraph_warnings":format!("{:?}",prepared.paragraph.warnings()),"layout_warnings":format!("{:?}",context.take_warnings())});
                if let Some(previous) = &signature {
                    if previous != &payload {
                        return Err(format!("unstable output {case}/{width}"));
                    }
                } else {
                    signature = Some(payload);
                }
                let (_, output_release) = measured(|| drop(output));
                let (_, prepared_release) = measured(|| drop(prepared));
                let (_, context_release) = measured(|| drop(context));
                samples.push(json!({"prepare":prepare,"break":line_break,"index_control":index_control,"index_release":index_release,"projection":projection,"output_release":output_release,"prepared_release":prepared_release,"context_release":context_release}));
            }
            rows.push(json!({"id":format!("{case}/{width}"),"case":case,"width":width,"signature":signature,"samples":samples}));
        }
    }
    let unsupported = caller::resolve_html(
        "<style>#root{direction:rtl;font-family:'Shodo Fixture Arabic'}</style><div id=root>مرحبا</div>",
        "root",
    )?;
    let rtl_error = caller::layout_with_font_policy(
        &unsupported,
        &fonts.collection,
        80.0,
        caller::FontPolicy::BundledWpt,
    )
    .err()
    .ok_or("unsupported RTL root unexpectedly accepted")?;
    println!("{}",serde_json::to_string(&json!({"issue":"shodo-sbp.15","rows":rows,"rtl_root_rejection":rtl_error,"system_fonts":false,"index_control_is_separate":true})).map_err(|e|e.to_string())?);
    Ok(())
}
