//! Presence-only descriptor lookup through real public font matching.
//! Run identical source on both revisions; registration and warmup are excluded.
//! `font_descriptor_lookup CASE ITERATIONS WARMUP SAMPLES`
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery};
use shodo::limits::Limits;
use shodo::style::FontFamily;
use std::{hint::black_box, time::Instant};

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

#[inline(never)]
fn run(fonts: &FontCollection, query: &FontQuery, iterations: usize, repeat: bool) -> u64 {
    let mut digest = 0u64;
    for i in 0..iterations {
        let cluster = if repeat || i % 2 == 0 { "a" } else { "b" };
        let found = black_box(fonts.match_cluster(black_box(query), black_box(cluster))).unwrap();
        // The fixed static fixture produces no axes or synthesis. Check the
        // result on every iteration so a faster failed match is not a result.
        assert!(found.variations.is_empty());
        assert!(!found.embolden);
        assert!(found.skew.is_none());
        digest = digest.rotate_left(1) ^ u64::from(found.id.index());
        black_box(found);
    }
    digest
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: font_descriptor_lookup CASE ITERATIONS WARMUP SAMPLES".into());
    }
    let case = &args[0];
    let (faces, ranges, cap, repeat) = match case.as_str() {
        "prepared-small" => (1, 0, 1, false),
        "prepared-heavy" => (16, 128, 1, false),
        "raw-static" => (1, 128, 0, false),
        "cache-hit" => (16, 128, 256, true),
        _ => return Err("unknown descriptor workload".into()),
    };
    let iterations: usize = args[1].parse()?;
    let warmup: usize = args[2].parse()?;
    let samples: usize = args[3].parse()?;
    if iterations == 0 || samples == 0 {
        return Err("iterations and samples must be positive".into());
    }
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    );
    let bytes = shodo_fixtures::font("latin")
        .ok_or("latin fixture is missing")?
        .bytes
        .to_vec();
    let mut expected = None;
    for _ in 0..faces {
        expected = Some(fonts.register_face(
            bytes.clone(),
            0,
            FontFaceDescriptor {
                family: "Descriptor".into(),
                unicode_ranges: vec![(0, 0x10ffff); ranges],
                ..Default::default()
            },
        )?);
    }
    let query = FontQuery {
        families: vec![FontFamily::Named("Descriptor".into())],
        ..Default::default()
    };
    for cluster in ["a", "b", "a", "b"] {
        assert_eq!(
            fonts.match_cluster(&query, cluster).unwrap().id,
            expected.unwrap()
        );
    }
    for _ in 0..warmup {
        black_box(run(&fonts, &query, iterations, repeat));
    }
    let mut elapsed = Vec::with_capacity(samples);
    let mut digests = Vec::with_capacity(samples);
    #[cfg(feature = "allocation-counting")]
    let mut allocations = Vec::with_capacity(samples);
    for _ in 0..samples {
        #[cfg(feature = "allocation-counting")]
        let scope = ALLOC.begin()?;
        let start = Instant::now();
        let digest = run(&fonts, &query, iterations, repeat);
        let ns = start.elapsed().as_nanos();
        #[cfg(feature = "allocation-counting")]
        let counts = scope.finish();
        elapsed.push(ns);
        digests.push(digest);
        #[cfg(feature = "allocation-counting")]
        allocations.push(counts);
    }
    let report = serde_json::json!({
        "schema": 1, "case": case, "faces": faces, "ranges_per_descriptor": ranges,
        "match_cache_entries": cap, "iterations": iterations, "warmup": warmup,
        "samples": samples, "selected_face_index": expected.unwrap().index(),
        "elapsed_ns": elapsed, "selection_digests": digests,
        "allocation_counting": cfg!(feature = "allocation-counting"),
    });
    #[cfg(feature = "allocation-counting")]
    let mut report = report;
    #[cfg(feature = "allocation-counting")]
    {
        report["allocations"] = serde_json::to_value(allocations)?;
    }
    println!("{report}");
    Ok(())
}
