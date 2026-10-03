#![cfg(feature = "allocation-counting")]

use shodo::Line;
use shodo::hit::LineLayout;
use shodo_bench::allocator::{AllocationCounts, CountingAllocator};
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[path = "../examples/support/ruby_base_caret_fixture.rs"]
mod fixture;

fn construction_allocations(lines: &[Line]) -> AllocationCounts {
    let scope = ALLOCATOR.begin().unwrap();
    for _ in 0..8 {
        black_box(LineLayout::new(black_box(lines)));
    }
    scope.finish()
}

#[test]
fn hidden_long_base_does_not_allocate_caret_copies() {
    let base_chars = 256;
    let hidden = fixture::nested(4, base_chars, true);
    assert_eq!(hidden[0].text().matches('A').count(), base_chars);

    let hidden_allocations = construction_allocations(&hidden);

    assert_eq!(
        hidden_allocations.calls,
        8 * 47,
        "each hidden long-base layout should avoid its one old Vec allocation: {hidden_allocations:?}"
    );
    assert!(hidden_allocations.allocated_bytes < 2_012_352);
}
