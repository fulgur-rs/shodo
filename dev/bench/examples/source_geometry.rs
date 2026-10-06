//! Repeated source-geometry measurements with preparation and hashing excluded.
//! Usage: source_geometry OPERATION CASE SCALE ITERATIONS
//! Operations: paint, index, selection, combined, build.
//! Cases use the existing workload names; plain uses SCALE Latin characters.
//! Build measurements include input insertion; all operations include output drop.
//! Use separate builds with/without allocation-counting for allocations/timing.

use serde_json::json;
use sha2::{Digest as _, Sha256};
use shodo::LayoutContext;
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo_bench::{Operation, Workload, digest, layout};
use std::hint::black_box;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        4,
        "usage: source_geometry OPERATION CASE SCALE ITERATIONS"
    );
    let operation = args[0].as_str();
    assert!(matches!(
        operation,
        "paint" | "index" | "selection" | "combined" | "build"
    ));
    let scale: usize = args[2].parse().unwrap();
    let iterations: usize = args[3].parse().unwrap();
    assert!(iterations > 0);
    let mut work = if args[1] == "plain" {
        assert!(scale > 0 && scale <= 16384);
        let mut work = Workload::named("latin-short", 1).unwrap();
        work.id = "plain".into();
        work.scale = scale;
        work.text = "a".repeat(scale);
        work.width = 1_000_000.0;
        work
    } else {
        Workload::named(&args[1], scale).unwrap()
    };
    // Keep the full CJK corpus's actual punctuation and font selection intact.
    if args[1] == "japanese-long" {
        work.width = 1_000_000.0;
    }
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut cx = LayoutContext::new();
    let paragraphs = work.build(&mut cx, &fonts, &limits).unwrap();
    assert!(paragraphs.iter().all(|p| p.warnings().is_empty()));
    let run = layout(
        &work,
        &paragraphs,
        &mut cx,
        &fonts,
        &limits,
        Operation::AllLines,
    )
    .unwrap();
    assert!(cx.take_warnings().is_empty());
    let geometry = digest(&run, &fonts).unwrap();
    let index = LineLayout::new(&run.lines);
    let endpoints: Vec<_> = run
        .lines
        .iter()
        .enumerate()
        .map(|(line, value)| {
            let range = value.text_range();
            (
                TextPosition {
                    line,
                    offset: range.start as u32,
                    affinity: Affinity::Downstream,
                },
                TextPosition {
                    line,
                    offset: range.end as u32,
                    affinity: Affinity::Upstream,
                },
            )
        })
        .collect();
    let selection = || {
        endpoints
            .iter()
            .map(|&(start, end)| index.selection_rects(start, end))
            .collect::<Vec<_>>()
    };
    let paint = || {
        run.lines
            .iter()
            .map(|line| line.paint_spans())
            .collect::<Vec<_>>()
    };
    let selection_hash = format!(
        "{:x}",
        Sha256::digest(format!("{:?}", selection()).as_bytes())
    );
    let paint_hash = format!("{:x}", Sha256::digest(format!("{:?}", paint()).as_bytes()));
    let mut execute = || match operation {
        "paint" => {
            black_box(paint());
        }
        "index" => {
            black_box(LineLayout::new(black_box(&run.lines)));
        }
        "selection" => {
            for &(start, end) in &endpoints {
                black_box(index.selection_rects(black_box(start), black_box(end)));
            }
        }
        "combined" => {
            black_box(paint());
            black_box(LineLayout::new(black_box(&run.lines)));
        }
        "build" => {
            black_box(work.build(&mut cx, &fonts, &limits).unwrap());
        }
        _ => unreachable!(),
    };
    for _ in 0..8 {
        execute();
    }
    #[cfg(feature = "allocation-counting")]
    let measure = {
        let scope = ALLOC.begin().unwrap();
        for _ in 0..iterations {
            execute();
        }
        let counts = scope.finish();
        json!({"allocation": counts})
    };
    #[cfg(not(feature = "allocation-counting"))]
    let measure = {
        let begin = std::time::Instant::now();
        for _ in 0..iterations {
            execute();
        }
        let ns = begin.elapsed().as_nanos();
        json!({"ns": ns})
    };
    assert_eq!(
        selection_hash,
        format!(
            "{:x}",
            Sha256::digest(format!("{:?}", selection()).as_bytes())
        )
    );
    assert_eq!(
        paint_hash,
        format!("{:x}", Sha256::digest(format!("{:?}", paint()).as_bytes()))
    );
    assert!(cx.take_warnings().is_empty());
    println!(
        "{}",
        json!({"operation":operation,"case":work.id,"scale":scale,"iterations":iterations,"measure":measure,"geometry":geometry,"selection_sha256":selection_hash,"paint_sha256":paint_hash})
    );
}
