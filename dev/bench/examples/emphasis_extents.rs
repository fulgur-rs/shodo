//! Usage: <marked|control> <runs> <iterations> <warmup> <samples>.
#[path = "support/emphasis_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::{Fragment, LayoutContext};
use std::{hint::black_box, time::Instant};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        5,
        "use: <marked|control> <runs> <iterations> <warmup> <samples>"
    );
    assert!(["marked", "control"].contains(&args[0].as_str()));
    let marked = args[0] == "marked";
    let runs: usize = args[1].parse().unwrap();
    let iterations: usize = args[2].parse().unwrap();
    let warmup: usize = args[3].parse().unwrap();
    let samples: usize = args[4].parse().unwrap();
    assert!(runs > 0 && iterations > 0 && samples > 0);
    let input = input::input(runs, marked);
    let mut cx = LayoutContext::new();
    let line = input.line(&mut cx);
    let marks: Vec<_> = line
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(format!("{:?}", r.emphasis_mark())),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), runs);
    for f in line.fragments() {
        if let Fragment::GlyphRun(r) = f {
            assert_eq!(r.emphasis_mark().is_some(), marked);
        }
    }
    let output = json!({"line":snapshot::line(&line),"emphasis":marks});
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    assert!(cx.take_warnings().is_empty());
    drop(line);
    for _ in 0..warmup {
        drop(black_box(input.line(&mut cx)));
    }
    let mut elapsed_ns = Vec::with_capacity(samples);
    let mut allocations = Vec::with_capacity(samples);
    let mut consumed = 0u64;
    for _ in 0..samples {
        #[cfg(feature = "allocation-counting")]
        let scope = ALLOC.begin().unwrap();
        let start = Instant::now();
        for _ in 0..iterations {
            let l = black_box(input.line(&mut cx));
            consumed = consumed.wrapping_add(u64::from(l.block_size().to_bits()));
            drop(l);
        }
        let elapsed = start.elapsed().as_nanos();
        #[cfg(feature = "allocation-counting")]
        let counts = scope.finish();
        elapsed_ns.push(elapsed);
        #[cfg(feature = "allocation-counting")]
        allocations.push(counts);
        #[cfg(not(feature = "allocation-counting"))]
        allocations.push(json!(null));
    }
    assert!(cx.take_warnings().is_empty());
    println!(
        "{}",
        json!({"case":args[0],"runs":runs,"iterations":iterations,"warmup":warmup,"samples":samples,"elapsed_ns":elapsed_ns,"allocations":allocations,"digest":digest,"consumed":consumed})
    );
}
