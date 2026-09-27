use criterion::{BatchSize, BenchmarkId, Criterion};
use shodo::{LayoutContext, limits::Limits};
use shodo_bench::{
    Operation,
    checked::{CheckedBuild, CheckedRun},
    digest, layout, workloads,
};
use std::{hint::black_box, path::Path, time::Duration};
fn main() {
    assert!(
        !cfg!(feature = "allocation-counting"),
        "time benchmarks require a build without allocation-counting"
    );
    let selected = std::env::var("SHODO_BENCH_CASE").ok();
    let work: Vec<_> = workloads()
        .into_iter()
        .filter(|w| selected.as_ref().is_none_or(|id| *id == w.id))
        .collect();
    assert!(!work.is_empty(), "unknown/empty selected workload");
    let mut c = Criterion::default();
    if std::env::var("SHODO_BENCH_QUICK").as_deref() == Ok("1") {
        c = c
            .sample_size(10)
            .warm_up_time(Duration::from_millis(100))
            .measurement_time(Duration::from_millis(200));
    }
    if let Ok(path) = std::env::var("SHODO_BENCH_OUTPUT") {
        c = c.output_directory(Path::new(&path));
    }
    c = c.configure_from_args();
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut evidence = Vec::new();
    for w in &work {
        let mut cx = LayoutContext::new();
        let ps = w.build(&mut cx, &fonts, &limits).unwrap();
        let expected = digest(
            &layout(w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap(),
            &fonts,
        )
        .unwrap();
        let mut row = serde_json::json!({"settings":w.settings(),"operations":{"build":expected}});
        let mut group = c.benchmark_group(&w.id);
        group.bench_function(BenchmarkId::new("build", w.scale), |b| {
            b.iter_batched(
                || (),
                |_| {
                    CheckedBuild::new(
                        black_box(w.build(&mut cx, &fonts, &limits).unwrap()),
                        w,
                        &fonts,
                        &limits,
                        &expected,
                    )
                },
                BatchSize::LargeInput,
            )
        });
        for (name, operation) in [
            ("next_line", Operation::FirstLine),
            ("all_lines", Operation::AllLines),
            ("intrinsic", Operation::Intrinsic),
            ("reuse_widths", Operation::ReuseWidths),
            ("rebuild_widths", Operation::RebuildWidths),
            ("page_retry", Operation::PageRetry),
        ] {
            let expected = digest(
                &layout(w, &ps, &mut cx, &fonts, &limits, operation).unwrap(),
                &fonts,
            )
            .unwrap();
            row["operations"][name] = serde_json::to_value(&expected).unwrap();
            group.bench_function(BenchmarkId::new(name, w.scale), |b| {
                b.iter_batched(
                    || (),
                    |_| {
                        CheckedRun::new(
                            black_box(layout(w, &ps, &mut cx, &fonts, &limits, operation).unwrap()),
                            &fonts,
                            &expected,
                        )
                    },
                    BatchSize::LargeInput,
                )
            });
        }
        group.finish();
        evidence.push(row);
    }
    c.final_summary();
    if let Ok(path) = std::env::var("SHODO_BENCH_OUTPUT") {
        let path = Path::new(&path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(
            path.join("digests.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
}
