//! Fixed-font probe for LineLayout construction with long ruby base stop ranges.
use serde_json::{Value, json};
use shodo::hit::LineLayout;
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

#[path = "support/ruby_base_caret_fixture.rs"]
mod fixture;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn time_samples(lines: &[shodo::Line], iterations: usize, samples: usize) -> Vec<u64> {
    (0..samples)
        .map(|_| {
            let mut total = 0;
            for _ in 0..iterations {
                let start = Instant::now();
                let layout = LineLayout::new(black_box(lines));
                black_box(&layout);
                total += start.elapsed().as_nanos() as u64;
                drop(layout);
            }
            total
        })
        .collect()
}

#[cfg(feature = "allocation-counting")]
fn allocation_counts(lines: &[shodo::Line]) -> Value {
    let scope = ALLOCATOR.begin().unwrap();
    let layout = LineLayout::new(lines);
    black_box(&layout);
    json!(scope.finish())
}

#[cfg(not(feature = "allocation-counting"))]
fn allocation_counts(_: &[shodo::Line]) -> Value {
    Value::Null
}

fn main() {
    let samples = env_usize("SHODO_RUBY_BASE_CARET_SAMPLES", 9);
    let iterations = env_usize("SHODO_RUBY_BASE_CARET_ITERATIONS", 32);
    let base_chars = 128;
    let cases = [(1, false), (4, false), (8, false), (4, true)];
    let reverse = std::env::var_os("SHODO_RUBY_BASE_CARET_REVERSE").is_some();
    let cases: Vec<_> = if reverse {
        cases.into_iter().rev().collect()
    } else {
        cases.into_iter().collect()
    };

    let mut rows = Vec::with_capacity(cases.len());
    for (depth, hide_top) in cases {
        let lines = fixture::nested(depth, base_chars, hide_top);
        let visible_depth = fixture::visible_depth(&lines);
        let mut warm = LineLayout::new(&lines);
        black_box(&mut warm);
        drop(warm);

        let build_samples = time_samples(&lines, iterations, samples);
        rows.push(json!({
            "requested_depth": depth,
            "visible_depth": visible_depth,
            "hidden_top": hide_top,
            "base_chars": base_chars,
            "layout_build_iterations_per_sample": iterations,
            "layout_build_ns_samples": build_samples,
            "layout_build_ns_per_layout_median": median(&build_samples) / iterations as u64,
            "layout_build_allocation": allocation_counts(&lines),
        }));
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
