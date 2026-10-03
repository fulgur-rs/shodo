//! Fixed-font Ruby column probe for ancestor Vec and membership-scan costs.
#[path = "support/ruby_base_decoration_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{LayoutContext, Line, Paragraph};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn layout(paragraph: &Paragraph) -> (Vec<Line>, LayoutContext) {
    let mut context = LayoutContext::new();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        1_000_000.0,
        &shodo::AtomicSizes::EMPTY,
    );
    (lines, context)
}

fn time_samples(paragraph: &Paragraph, samples: usize, iterations: usize) -> Vec<u64> {
    (0..samples)
        .map(|_| {
            let mut total = 0;
            for _ in 0..iterations {
                let start = Instant::now();
                let (lines, context) = layout(black_box(paragraph));
                total += start.elapsed().as_nanos() as u64;
                black_box((&lines, &context));
                drop((lines, context));
            }
            total
        })
        .collect()
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn normalize_font_layer_id(value: &mut Value) {
    match value {
        Value::String(text) => {
            let marker = "FontId { layer: ";
            let index_marker = ", index:";
            let mut rest = text.as_str();
            let mut normalized = String::with_capacity(text.len());
            while let Some(start) = rest.find(marker) {
                normalized.push_str(&rest[..start + marker.len()]);
                let after_layer = &rest[start + marker.len()..];
                let Some(index) = after_layer.find(index_marker) else {
                    normalized.push_str(&rest[start + marker.len()..]);
                    rest = "";
                    break;
                };
                normalized.push_str("<resource>");
                normalized.push_str(index_marker);
                rest = &after_layer[index + index_marker.len()..];
            }
            normalized.push_str(rest);
            *text = normalized;
        }
        Value::Array(values) => values.iter_mut().for_each(normalize_font_layer_id),
        Value::Object(values) => values.values_mut().for_each(normalize_font_layer_id),
        _ => {}
    }
}

#[cfg(feature = "allocation-counting")]
fn allocation_counts(paragraph: &Paragraph) -> Value {
    let mut context = LayoutContext::new();
    let scope = ALLOC.begin().unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        1_000_000.0,
        &shodo::AtomicSizes::EMPTY,
    );
    black_box(&lines);
    let counts = scope.finish();
    json!(counts)
}

#[cfg(not(feature = "allocation-counting"))]
fn allocation_counts(_: &Paragraph) -> Value {
    Value::Null
}

fn main() {
    let samples = env_usize("SHODO_RUBY_BASE_DECORATION_SAMPLES", 9);
    let iterations = env_usize("SHODO_RUBY_BASE_DECORATION_ITERATIONS", 32);
    let cases = [(8, 32), (16, 64)];
    let reverse = std::env::var_os("SHODO_RUBY_BASE_DECORATION_REVERSE").is_some();
    let cases: Vec<_> = if reverse {
        cases.into_iter().rev().collect()
    } else {
        cases.into_iter().collect()
    };

    let mut rows = Vec::with_capacity(cases.len());
    for &(columns, depth) in &cases {
        // Font loading and all nested input construction happen before scopes.
        let paragraph = fixture::paragraph_for_measurement(columns, depth);
        let (warm, _) = layout(&paragraph);
        assert_eq!(warm.len(), 1);
        drop(warm);

        let timings = time_samples(&paragraph, samples, iterations);
        let median_ns = median(&timings) / iterations as u64;
        let allocations = allocation_counts(&paragraph);
        let (_, mut context) = layout(&paragraph);
        let layout_warnings = format!("{:?}", context.take_warnings());
        rows.push(json!({
            "columns": columns,
            "inline_depth": depth,
            "layout_iterations_per_sample": iterations,
            "layout_ns_samples": timings,
            "layout_ns_per_iteration_median": median_ns,
            "layout_allocation": allocations,
            "build_warnings": format!("{:?}", paragraph.warnings()),
            "layout_warnings": layout_warnings
        }));
    }

    // Snapshot export runs after every timed case, so serialization and its
    // retained JSON cannot warm or slow either side of the timing loop.
    for (index, (columns, depth)) in cases.iter().copied().enumerate() {
        let paragraph = fixture::paragraph_for_measurement(columns, depth);
        let (lines, _) = layout(&paragraph);
        assert_eq!(lines[0].ruby_annotations().len(), columns);
        let mut output = Value::Array(lines.iter().map(snapshot::line).collect());
        normalize_font_layer_id(&mut output);
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
        rows[index]["output_sha256"] = json!(digest);
        rows[index]["output_snapshot"] = output;
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
