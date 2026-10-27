#![cfg(feature = "allocation-counting")]

use shodo::limits::Limits;
use shodo_bench::allocator::CountingAllocator;
use shodo_fixtures::load_fonts;
use std::alloc::System;

#[path = "../examples/support/reshape_split_allocations.rs"]
mod workload;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn ligature_split_outputs_stay_stable_with_lower_build_allocations() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    // Re-measured with SHODO-56 reverted and the decoration-work budget in
    // place. The split cases still save 1,632 bytes and 59 allocations.
    let expected = [
        (
            "fi-splits",
            "1a539e47a86309e87725d86e4b200a0e6e712524da3e3df0f2d9ebb8ab8ba580",
            634_368_u64,
            634_344_u64,
        ),
        (
            "ffi-splits",
            "5fb7d0ae69ca8f5d89c1b9b38b27e54b82eec61dacb4a175fb04e175a2f78711",
            1_194_688_u64,
            1_194_664_u64,
        ),
        (
            "unsplit-control",
            "bfd516e533fa85e7e49872556f95aabb3cdf3e9be5bc14dbe738aecb0e584be6",
            534_694_u64,
            534_670_u64,
        ),
    ];

    for (case, (name, digest, debug_baseline, release_baseline)) in
        workload::cases().iter().zip(expected)
    {
        assert_eq!(case.name, name);
        let baseline = if cfg!(debug_assertions) {
            debug_baseline
        } else {
            release_baseline
        };
        let warm = workload::build(case, &limits, &fonts);
        let warm_run = workload::layout(&warm);
        assert_eq!(warm_run.lines.len(), 1, "{name} should stay on one line");
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
        assert_eq!(run.lines.len(), 1, "{name} should stay on one line");
        assert_eq!(
            workload::digest(&run, &fonts),
            digest,
            "{name} geometry changed"
        );
        if name == "unsplit-control" {
            assert!(
                counts.allocated_bytes <= baseline,
                "unsplit control should not regress its pre-change allocation {baseline}: {counts:?}",
            );
        } else {
            assert!(
                counts.allocated_bytes < baseline,
                "{name} should allocate less than pre-change build {baseline}: {counts:?}",
            );
        }
    }
}
