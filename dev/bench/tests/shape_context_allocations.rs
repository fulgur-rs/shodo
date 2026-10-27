#![cfg(feature = "allocation-counting")]

use shodo::limits::Limits;
use shodo::style::LineOptions;
use shodo_bench::allocator::CountingAllocator;
use shodo_fixtures::load_fonts;
use std::alloc::System;

#[path = "../examples/support/shape_context_allocations.rs"]
mod workload;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn context_and_owned_edge_outputs_are_stable_with_lower_allocations() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let workloads = workload::Workloads::new(&fonts, &limits);

    // Match the standalone measurement's warm font/cache state. A cold build
    // includes dependency initialization that its warm baseline does not cover.
    // See docs/records/2026-10-26-shodo-56-allocation-oracles.md.
    let warm = workloads.build_context_paragraph(&fonts);
    let warm_run = workload::layout(&warm, 50_000.0, &LineOptions::default());
    let warm_digest = workload::digest(&warm_run, &fonts);
    drop(warm_run);
    drop(warm);

    let scope = ALLOCATOR.begin().unwrap();
    let paragraph = workloads.build_context_paragraph(&fonts);
    let build_counts = scope.finish();
    let context_run = workload::layout(&paragraph, 50_000.0, &LineOptions::default());
    assert_eq!(context_run.lines.len(), 1);
    assert_eq!(workload::digest(&context_run, &fonts), warm_digest);
    assert_eq!(
        workload::digest(&context_run, &fonts),
        "32d61f314336e6eecc86c34a2be2c9d5cc0775a9ede93cd959581acba055a1fd",
        "context-heavy output changed",
    );
    let (context_pre_change_bytes, edge_pre_change_bytes) = if cfg!(debug_assertions) {
        (582_636, 1_226_867)
    } else {
        (582_588, 1_226_867)
    };
    let one_context_per_item_budget = context_pre_change_bytes - workload::CONTEXT_RUNS as u64 * 20;
    assert!(
        build_counts.allocated_bytes <= one_context_per_item_budget,
        "context-heavy build should save at least one 20-byte context allocation per item; budget={one_context_per_item_budget}, counts={build_counts:?}",
    );

    let edge_warm = workloads.layout_edge();
    let edge_warm_digest = workload::digest(&edge_warm, &fonts);
    drop(edge_warm);
    let scope = ALLOCATOR.begin().unwrap();
    let edge_run = workloads.layout_edge();
    let edge_counts = scope.finish();
    assert!(edge_run.lines.len() > 1);
    assert_eq!(workload::digest(&edge_run, &fonts), edge_warm_digest);
    assert_eq!(
        workload::digest(&edge_run, &fonts),
        "027f72000eb0f61cdfc42caa7c911844e63d8efbfffabe88f0a5cec5f2c0eca7",
        "soft-hyphen edge output changed",
    );
    assert!(
        edge_counts.allocated_bytes < edge_pre_change_bytes,
        "owned edge-window should allocate less than profile baseline {edge_pre_change_bytes} bytes: {edge_counts:?}",
    );
}
