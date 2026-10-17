#![cfg(feature = "allocation-counting")]

use shodo::limits::Limits;
use shodo_bench::allocator::CountingAllocator;
use shodo_fixtures::load_fonts;
use std::alloc::System;

#[path = "../examples/support/punctuation_metrics.rs"]
mod workload;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn repeated_cjk_punctuation_has_stable_geometry_and_bounded_build_allocations() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let cases = workload::cases();
    let expected = [
        (
            "repeated-opening",
            "0b6273a00e17c89d417a3a4c9b77fa16fdfd2ddc2958dd1ba884c21dd5025559",
            360_720_u64,
            360_768_u64,
        ),
        (
            "repeated-closing-bracket",
            "e7262294ea05306467939781ad10b656828672d2e4a39f0bdd89cfe659e2f365",
            357_424_u64,
            357_472_u64,
        ),
    ];

    for (case, (name, digest, release_bytes, debug_bytes)) in cases.iter().zip(expected) {
        let pre_change_bytes = if cfg!(debug_assertions) {
            debug_bytes
        } else {
            release_bytes
        };
        assert_eq!(case.name, name);
        assert_eq!(workload::run_count(), 64);
        let warm = workload::build(case, &limits, &fonts);
        let warm_run = workload::layout(&warm);
        assert_eq!(
            workload::digest(&warm_run, &fonts),
            digest,
            "{name} warm output"
        );
        drop(warm_run);
        drop(warm);
        let scope = ALLOCATOR.begin().unwrap();
        let paragraph = workload::build(case, &limits, &fonts);
        let counts = scope.finish();
        let run = workload::layout(&paragraph);
        assert_eq!(run.lines.len(), 1);
        assert_eq!(
            workload::digest(&run, &fonts),
            digest,
            "{name} geometry changed"
        );
        assert!(
            counts.allocated_bytes <= pre_change_bytes,
            "{name} should not exceed pre-change build allocation {pre_change_bytes}: {counts:?}",
        );
    }
}
