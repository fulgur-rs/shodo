#![cfg(feature = "allocation-counting")]

use shodo::limits::Limits;
use shodo_bench::allocator::CountingAllocator;
use shodo_fixtures::load_fonts;
use std::alloc::System;

#[path = "../examples/support/line_small_allocations.rs"]
mod workload;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn fixture_output_is_stable_and_uniform_bidi_allocations_drop() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    // Re-measured with SHODO-56 reverted and the decoration-work budget in
    // place. See docs/records/2026-10-27-clone-decoration-allocation-oracles.md.
    let expected = [
        (
            "uniform-ltr",
            "60973ddd5bc9955622709ea7d85227f42d88ef07932cdf04fc2e8bb6b0a8b942",
            1_220_941_u64,
        ),
        (
            "uniform-rtl",
            "5395dda87a8461994bad8891551f3636d8a91432061c1bdd9516824f901481ab",
            1_484_625_u64,
        ),
        (
            "mixed-levels",
            "a753376f7efdbb188b12d2eae75377e3599ff7e1655f8c7df0faa39aafc28ff6",
            809_933_u64,
        ),
        (
            "no-continuation",
            "6f8d280d7c370c8754622059aeed3ffb91044ef8e4211ea7db48971d4f5c423e",
            312_952_u64,
        ),
        (
            "rtl-continuation-hanging",
            "2ec7341a6d49469043168727634b2c71ce3551e248a45cb861f5043da3dc6fa0",
            1_705_521_u64,
        ),
    ];

    for (case, (name, digest, pre_change_bytes)) in
        workload::cases(&fonts, &limits).iter().zip(expected)
    {
        assert_eq!(case.name, name);
        let scope = ALLOCATOR.begin().unwrap();
        let run = workload::layout(case);
        let counts = scope.finish();
        assert_eq!(workload::digest(&run, &fonts), digest, "{name}");
        if matches!(name, "mixed-levels" | "rtl-continuation-hanging") {
            assert!(
                counts.allocated_bytes <= pre_change_bytes,
                "{name} should not exceed pre-change {pre_change_bytes} bytes: {counts:?}"
            );
        } else {
            assert!(
                counts.allocated_bytes < pre_change_bytes,
                "{name} should allocate less than pre-change {pre_change_bytes} bytes: {counts:?}"
            );
        }
    }
}
