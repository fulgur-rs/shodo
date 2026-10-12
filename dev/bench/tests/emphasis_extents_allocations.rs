#![cfg(feature = "allocation-counting")]
#[path = "../examples/support/emphasis_input.rs"]
mod input;
use shodo::{Fragment, LayoutContext};
use shodo_bench::allocator::CountingAllocator;
use std::{alloc::System, hint::black_box};
#[global_allocator]
static ALLOC: CountingAllocator<System> = CountingAllocator::new(System);
#[test]
fn auxiliary_emphasis_metrics_avoid_discarded_per_fragment_vectors() {
    let input = input::input(512, true);
    let mut cx = LayoutContext::new();
    for _ in 0..3 {
        drop(black_box(input.line(&mut cx)));
    }
    let scope = ALLOC.begin().unwrap();
    let line = black_box(input.line(&mut cx));
    assert_eq!(
        line.fragments()
            .filter(|f| matches!(f, Fragment::GlyphRun(_)))
            .count(),
        512
    );
    for f in line.fragments() {
        if let Fragment::GlyphRun(r) = f {
            assert!(r.emphasis_mark().is_some());
        }
    }
    drop(line);
    let counts = scope.finish();
    assert!(cx.take_warnings().is_empty());
    // Keeping the discarded shift/membership vectors adds 16 KiB for 1024
    // fragments. Leave room for ordinary retained metrics/context work.
    assert!(
        counts.allocated_bytes < 376_000,
        "auxiliary extents allocated {counts:?}"
    );
}
