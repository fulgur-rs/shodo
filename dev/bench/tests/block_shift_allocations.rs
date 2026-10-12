#![cfg(feature = "allocation-counting")]

#[path = "../examples/support/block_shift_input.rs"]
mod input;

use shodo::{Fragment, LayoutContext};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOC: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn accepted_lines_do_not_allocate_discarded_shift_buffers() {
    // Reintroducing initial per-fragment zeros exceeds the plain budget;
    // maintaining the reshape copy exceeds the second budget. Paragraph,
    // fonts and warmed scan caches are outside the scope; the accepted line
    // keeps its final metrics and is dropped inside it.
    for (reshape, budget) in [(false, 276_000), (true, 895_000)] {
        let input = input::input(512, reshape);
        let mut cx = LayoutContext::new();
        for _ in 0..3 {
            drop(black_box(input.line(&mut cx)));
        }
        let scope = ALLOC.begin().unwrap();
        let line = black_box(input.line(&mut cx));
        let runs = line
            .fragments()
            .filter(|f| matches!(f, Fragment::GlyphRun(_)))
            .count();
        assert_eq!(runs, 512 + usize::from(reshape));
        assert_eq!(line.is_last(), !reshape);
        drop(line);
        let counts = scope.finish();
        assert!(cx.take_warnings().is_empty());
        assert!(
            counts.allocated_bytes <= budget,
            "reshape={reshape}: {counts:?}; budget={budget}"
        );
    }
}
