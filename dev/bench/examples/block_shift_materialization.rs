//! Accepted-line timing and allocation probe. Usage: <plain|reshape> <runs> <iterations> <warmup> <samples>.
#[path = "support/block_shift_input.rs"]
mod input;
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::{Fragment, LayoutContext};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        5,
        "use: <plain|reshape> <runs> <iterations> <warmup> <samples>"
    );
    assert!(["plain", "reshape"].contains(&args[0].as_str()));
    let reshape = args[0] == "reshape";
    let runs: usize = args[1].parse().unwrap();
    let iterations: usize = args[2].parse().unwrap();
    let warmup: usize = args[3].parse().unwrap();
    let samples: usize = args[4].parse().unwrap();
    assert!(runs > 0 && iterations > 0 && samples > 0);
    let input = input::input(runs, reshape);
    let mut cx = LayoutContext::new();
    let line = input.line(&mut cx);
    let accepted_runs = line
        .fragments()
        .filter(|f| matches!(f, Fragment::GlyphRun(_)))
        .count();
    assert_eq!(accepted_runs, runs + usize::from(reshape));
    assert_eq!(line.is_last(), !reshape);
    if reshape {
        assert!(line.text()[line.text_range()].ends_with('\u{ad}'));
    }
    let output = snapshot::line(&line);
    let warnings = cx.take_warnings();
    assert!(warnings.is_empty());
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    drop(line);
    for _ in 0..warmup {
        drop(black_box(input.line(&mut cx)));
    }
    let mut elapsed_ns = Vec::with_capacity(samples);
    let mut allocations = Vec::new();
    let mut consumed = 0u64;
    for _ in 0..samples {
        #[cfg(feature = "allocation-counting")]
        let scope = ALLOC.begin().unwrap();
        let started = Instant::now();
        for _ in 0..iterations {
            let line = black_box(input.line(&mut cx));
            consumed = consumed.wrapping_add(u64::from(line.inline_size().to_bits()));
            drop(line);
        }
        let elapsed = started.elapsed().as_nanos();
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
        json!({"case":args[0],"runs":runs,"accepted_runs":accepted_runs,"fragments":output["fragments"].as_array().unwrap().len(),"text_bytes":input.paragraph.text().len(),"width":input.constraint.available_inline_size,"iterations":iterations,"warmup":warmup,"samples":samples,"elapsed_ns":elapsed_ns,"allocations":allocations,"digest":digest,"consumed":consumed})
    );
}
