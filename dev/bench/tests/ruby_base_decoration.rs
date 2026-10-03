#![cfg(feature = "allocation-counting")]

use shodo::LayoutContext;
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[path = "../examples/support/ruby_base_decoration_fixture.rs"]
mod fixture;

#[test]
fn deep_multicolumn_ruby_stays_under_ancestor_allocation_budget() {
    let paragraph = fixture::paragraph_for_measurement(16, 64);
    let mut warm_context = LayoutContext::new();
    let warm = paragraph.break_all(
        &mut warm_context,
        &Default::default(),
        1_000_000.0,
        &shodo::AtomicSizes::EMPTY,
    );
    assert_eq!(warm.len(), 1);
    drop(warm);

    let mut context = LayoutContext::new();
    let scope = ALLOCATOR.begin().unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        1_000_000.0,
        &shodo::AtomicSizes::EMPTY,
    );
    black_box(&lines);
    let counts = scope.finish();

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].ruby_annotations().len(), 16);
    assert!(
        counts.calls < 130_000,
        "temporary ancestor vectors regressed: {counts:?}"
    );
    assert!(
        counts.allocated_bytes < 17_500_000,
        "temporary ancestor vector bytes regressed: {counts:?}"
    );
}
