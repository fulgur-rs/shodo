//! Fixed-fixture alignment and fragment workload for SHODO-56.
use serde_json::json;
use shodo::limits::Limits;
use shodo_fixtures::load_fonts;
use std::hint::black_box;
use std::time::Instant;

#[path = "support/line_small_allocations.rs"]
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
    let cases = workload::cases(&fonts, &limits);
    for case in &cases {
        let warm = workload::layout(case);
        assert!(
            warm.lines.len() > 1,
            "{} should exercise several lines",
            case.name
        );
        let expected = workload::digest(&warm, &fonts);
        let line_count = warm.lines.len();
        drop(warm);

        let median_ns = if mode == "time" {
            let mut samples = Vec::with_capacity(7);
            for _ in 0..7 {
                let started = Instant::now();
                for _ in 0..8 {
                    black_box(workload::layout(black_box(case)));
                }
                samples.push(started.elapsed().as_nanos() as u64 / 8);
            }
            samples.sort_unstable();
            Some(samples[samples.len() / 2])
        } else {
            None
        };

        #[cfg(feature = "allocation-counting")]
        let allocation = {
            let scope = ALLOC.begin().unwrap();
            let run = workload::layout(case);
            let counts = scope.finish();
            let digest = workload::digest(&run, &fonts);
            assert_eq!(digest, expected, "{} output changed", case.name);
            json!(counts)
        };
        #[cfg(not(feature = "allocation-counting"))]
        let allocation = serde_json::Value::Null;

        println!(
            "{}",
            json!({
                "case": case.name,
                "mode": mode,
                "lines": line_count,
                "median_ns_per_layout": median_ns,
                "allocation": allocation,
                "output_sha256": expected,
            })
        );
    }
}
