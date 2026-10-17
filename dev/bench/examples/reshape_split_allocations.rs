//! Fixed Latin ligature workload for split-cluster identity allocations.
use serde_json::json;
use shodo::limits::Limits;
use shodo_fixtures::load_fonts;
use std::hint::black_box;
use std::time::Instant;

#[path = "support/reshape_split_allocations.rs"]
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
    for case in workload::cases() {
        let warm = workload::build(&case, &limits, &fonts);
        let warm_run = workload::layout(&warm);
        assert_eq!(
            warm_run.lines.len(),
            1,
            "{} should stay on one line",
            case.name
        );
        let expected = workload::digest(&warm_run, &fonts);
        drop(warm_run);
        drop(warm);

        let median_ns = if mode == "time" {
            let mut samples = Vec::with_capacity(7);
            for _ in 0..7 {
                let started = Instant::now();
                for _ in 0..4 {
                    black_box(workload::build(&case, &limits, &fonts));
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
            let scope = ALLOC.begin().unwrap();
            let paragraph = workload::build(&case, &limits, &fonts);
            let counts = scope.finish();
            let run = workload::layout(&paragraph);
            assert_eq!(
                workload::digest(&run, &fonts),
                expected,
                "{} output changed",
                case.name
            );
            json!(counts)
        };
        #[cfg(not(feature = "allocation-counting"))]
        let allocation = serde_json::Value::Null;

        println!(
            "{}",
            json!({
                "case": case.name,
                "runs": workload::RUNS,
                "mode": mode,
                "median_ns_per_build": median_ns,
                "allocation": allocation,
                "output_sha256": expected,
            })
        );
    }
}
