//! Public matching: thrash|warm|local CAP KEYS THREADS ITERATIONS WARMUP SAMPLES [STYLES].
//! Fixed faces, queries and threads are prepared outside sample timing.
use sha2::{Digest, Sha256};
use shodo::{
    font::{FontCollection, FontOptions, FontQuery},
    limits::Limits,
    style::{FontFamily, FontStyle},
};
use std::{
    hint::black_box,
    sync::{Arc, Barrier},
    time::Instant,
};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const TEXT: &str = "日本語組版句読点空白調整文章条件読みやすい同じで比べます。縦書き横幅文字表示";
fn matches(fonts: &FontCollection, queries: &[FontQuery], clusters: &[String], n: usize) -> u64 {
    let mut digest = 0u64;
    for i in 0..n {
        let k = i % queries.len();
        let result =
            black_box(fonts.match_cluster(black_box(&queries[k]), black_box(&clusters[k])));
        if let Some(f) = result {
            digest = digest.wrapping_add(u64::from(f.id.index()) + 1);
            digest = digest.wrapping_add(u64::from(f.embolden) * 3);
            digest = digest.wrapping_add(u64::from(f.skew.is_some()) * 7);
            assert!(f.variations.is_empty());
        } else {
            digest = digest.wrapping_add(19);
        }
    }
    black_box(digest)
}
fn main() {
    let a: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        (7..=8).contains(&a.len()),
        "thrash|warm|local CAP KEYS THREADS ITERATIONS WARMUP SAMPLES [STYLES]"
    );
    assert!(["thrash", "warm", "local"].contains(&a[0].as_str()));
    let cap: usize = a[1].parse().unwrap();
    let keys: usize = a[2].parse().unwrap();
    let threads: usize = a[3].parse().unwrap();
    let n: usize = a[4].parse().unwrap();
    let warmup: usize = a[5].parse().unwrap();
    let samples: usize = a[6].parse().unwrap();
    let styles: usize = a.get(7).map_or(keys, |s| s.parse().unwrap());
    assert!(keys > 0 && threads > 0 && n > 0 && samples > 0);
    assert!(styles > 0 && styles <= keys);
    if a[0] == "thrash" {
        assert!(keys > cap);
    }
    if a[0] == "warm" {
        assert!(keys <= cap);
    }
    if a[0] == "local" {
        assert_eq!(keys, 1);
    }
    #[cfg(feature = "allocation-counting")]
    assert_eq!(threads, 1, "allocation scope requires one thread");
    let fonts = FontCollection::with_options(
        &Limits::unlimited(),
        FontOptions {
            system_fonts: false,
            match_cache_entries: cap,
            ..Default::default()
        },
    );
    for name in ["latin", "cjk"] {
        fonts
            .register(shodo_fixtures::font(name).unwrap().bytes.to_vec())
            .unwrap();
    }
    let mut chars: Vec<char> = Vec::new();
    for c in TEXT.chars() {
        if !chars.contains(&c) {
            chars.push(c);
        }
    }
    let needed = keys.div_ceil(styles);
    for scalar in 0x5000..=0x9fff {
        if chars.len() >= needed {
            break;
        }
        let c = char::from_u32(scalar).unwrap();
        if !chars.contains(&c) {
            chars.push(c);
        }
    }
    assert!(chars.len() >= needed);
    let queries: Vec<_> = (0..keys)
        .map(|i| FontQuery {
            families: vec![
                FontFamily::Named("Shodo Fixture CJK".into()),
                FontFamily::Named("Shodo Fixture Latin".into()),
            ],
            weight: 100. + ((i % styles) % 128) as f32 * 6.,
            language: Some(format!("ja-{}", (i % styles) / 128)),
            style: if (i % styles).is_multiple_of(3) {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            script: *b"Hani",
            ..Default::default()
        })
        .collect();
    let clusters: Vec<_> = (0..keys).map(|i| chars[i / styles].to_string()).collect();
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    let selected: Vec<_> = queries
        .iter()
        .zip(&clusters)
        .map(|(q, c)| {
            fonts
                .match_cluster(q, c)
                .map(|f| (f.id.index(), f.variations, f.embolden, f.skew))
        })
        .collect();
    #[cfg(feature = "allocation-counting")]
    let initial = scope.finish();
    let selection_sha256 = format!("{:x}", Sha256::digest(format!("{selected:?}").as_bytes()));
    let matched_keys = selected.iter().filter(|s| s.is_some()).count();
    let expected = matches(&fonts, &queries, &clusters, n);
    let mut elapsed = Vec::with_capacity(samples);
    let mut digests = Vec::with_capacity(samples);
    let mut allocations = Vec::with_capacity(samples);
    let mut latencies = Vec::with_capacity(samples);
    if threads == 1 {
        for _ in 0..warmup {
            matches(&fonts, &queries, &clusters, n);
        }
        for _ in 0..samples {
            #[cfg(feature = "allocation-counting")]
            let scope = ALLOC.begin().unwrap();
            let start = Instant::now();
            let digest = matches(&fonts, &queries, &clusters, n);
            let ns = start.elapsed().as_nanos();
            #[cfg(feature = "allocation-counting")]
            let counts = scope.finish();
            assert_eq!(digest, expected);
            elapsed.push(ns);
            digests.push(digest);
            latencies.push(vec![ns]);
            #[cfg(feature = "allocation-counting")]
            allocations.push(serde_json::to_value(counts).unwrap());
            #[cfg(not(feature = "allocation-counting"))]
            allocations.push(serde_json::json!(null));
        }
    } else {
        let barrier = Arc::new(Barrier::new(threads + 1));
        let results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(threads);
            for _ in 0..threads {
                let barrier = barrier.clone();
                let fonts = &fonts;
                let queries = &queries;
                let clusters = &clusters;
                handles.push(scope.spawn(move || {
                    matches(fonts, queries, clusters, keys);
                    for _ in 0..warmup {
                        matches(fonts, queries, clusters, n);
                    }
                    let mut rows = Vec::with_capacity(samples);
                    for _ in 0..samples {
                        barrier.wait(); // all workers are ready, including after warmup
                        barrier.wait();
                        let start = Instant::now();
                        let digest = matches(fonts, queries, clusters, n);
                        let ns = start.elapsed().as_nanos();
                        rows.push((ns, digest));
                        barrier.wait();
                    }
                    rows
                }));
            }
            for _ in 0..samples {
                barrier.wait(); // exclude preparation and warmup from timing
                let start = Instant::now();
                barrier.wait();
                barrier.wait();
                elapsed.push(start.elapsed().as_nanos());
            }
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        for sample in 0..samples {
            let row: Vec<_> = results.iter().map(|r| r[sample].0).collect();
            assert!(
                row.iter().all(|ns| *ns <= elapsed[sample]),
                "aggregate timer must encompass every worker"
            );
            for r in &results {
                assert_eq!(r[sample].1, expected);
            }
            latencies.push(row);
            digests.push(expected);
            allocations.push(serde_json::json!(null));
        }
    }
    let report = serde_json::json!({"case":a[0],"cap":cap,"keys":keys,"styles":styles,"matched_keys":matched_keys,"threads":threads,"iterations_per_thread":n,"samples":samples,"warmup":warmup,"elapsed_ns":elapsed,"worker_elapsed_ns":latencies,"selection_digests":digests,"selection_sha256":selection_sha256,"allocations":allocations});
    #[cfg(feature = "allocation-counting")]
    let mut report = report;
    #[cfg(feature = "allocation-counting")]
    {
        report["initial_cache_and_selection"] = serde_json::to_value(initial).unwrap();
    }
    println!("{}", report);
}
