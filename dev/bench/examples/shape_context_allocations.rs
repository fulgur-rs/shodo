//! Fixed workload for ShapeItem context and owned edge-window allocations.
use serde_json::json;
use shodo::limits::Limits;
use shodo_fixtures::load_fonts;
use std::hint::black_box;
use std::time::Instant;

#[path = "support/shape_context_allocations.rs"]
mod workload;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn main() {
    let mode_arg = std::env::args().nth(1).unwrap_or_else(|| "--time".into());
    let mode = mode_arg.strip_prefix("--").unwrap_or(&mode_arg);
    assert!(matches!(mode, "time" | "alloc"));
    #[cfg(not(feature = "allocation-counting"))]
    assert_ne!(mode, "alloc", "alloc mode requires allocation-counting");

    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let workloads = workload::Workloads::new(&fonts, &limits);
    let context = workloads.build_context_paragraph(&fonts);
    let warm = workload::layout(&context, 50_000.0, &shodo::style::LineOptions::default());
    assert_eq!(
        warm.lines.len(),
        1,
        "context workload should stay on one line"
    );
    let context_digest = workload::digest(&warm, &fonts);
    drop(warm);

    let edge_warm = workloads.layout_edge();
    assert!(edge_warm.lines.len() > 1, "edge workload should wrap");
    let edge_digest = workload::digest(&edge_warm, &fonts);

    for (case, expected) in [
        ("context-build", context_digest),
        ("edge-window", edge_digest),
    ] {
        let median_ns = if mode == "time" {
            let mut samples = Vec::with_capacity(7);
            for _ in 0..7 {
                let started = Instant::now();
                for _ in 0..4 {
                    match case {
                        "context-build" => {
                            black_box(workloads.build_context_paragraph(&fonts));
                        }
                        _ => {
                            black_box(workloads.layout_edge());
                        }
                    }
                }
                samples.push(started.elapsed().as_nanos() as u64 / 4);
            }
            samples.sort_unstable();
            Some(samples[samples.len() / 2])
        } else {
            None
        };

        #[cfg(feature = "allocation-counting")]
        let allocation = {
            let (counts, run) = match case {
                "context-build" => {
                    let scope = ALLOC.begin().unwrap();
                    let paragraph = workloads.build_context_paragraph(&fonts);
                    let counts = scope.finish();
                    let run = workload::layout(
                        &paragraph,
                        50_000.0,
                        &shodo::style::LineOptions::default(),
                    );
                    (counts, run)
                }
                _ => {
                    let scope = ALLOC.begin().unwrap();
                    let run = workloads.layout_edge();
                    let counts = scope.finish();
                    (counts, run)
                }
            };
            assert_eq!(
                workload::digest(&run, &fonts),
                expected,
                "{case} output changed"
            );
            json!(counts)
        };
        #[cfg(not(feature = "allocation-counting"))]
        let allocation = serde_json::Value::Null;

        println!(
            "{}",
            json!({
                "case": case,
                "context_runs": workload::CONTEXT_RUNS,
                "mode": mode,
                "median_ns_per_run": median_ns,
                "allocation": allocation,
                "output_sha256": expected,
            })
        );
    }
}
