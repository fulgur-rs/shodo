//! Measures warm paragraph construction and repeated full line layout for one
//! fixed-font workload. Copy this same source to both historical revisions.

use std::hint::black_box;
use std::time::Instant;

fn measure_samples<T, E>(
    warmup: usize,
    samples: usize,
    mut operation: impl FnMut() -> Result<T, E>,
) -> Result<Vec<u128>, E> {
    for _ in 0..warmup {
        black_box(operation()?);
    }
    let mut elapsed = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let output = operation()?;
        let duration = start.elapsed().as_nanos();
        black_box(output);
        elapsed.push(duration);
    }
    Ok(elapsed)
}

fn run(
    case: &str,
    scale: usize,
    warmup: usize,
    samples: usize,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use serde_json::json;
    use shodo::{LayoutContext, limits::Limits, style::LineOptions};
    use shodo_bench::{Run, Workload, digest};
    use shodo_fixtures::load_fonts;

    let limits = Limits::default();
    let fonts = load_fonts(&limits)?;
    let workload = Workload::named(case, scale)?;
    if workload.paragraph_count() != 1 {
        return Err("measurement expects exactly one paragraph".into());
    }
    let mut context = LayoutContext::new();
    let build_ns = measure_samples(warmup, samples, || {
        workload.build(&mut context, &fonts, &limits)
    })?;
    let paragraphs = workload.build(&mut context, &fonts, &limits)?;
    let options = LineOptions::default();
    let lines = paragraphs[0].break_all(&mut context, &options, workload.width, workload.atomics());
    let line_semantics: Vec<_> = lines
        .iter()
        .map(|line| {
            let range = line.text_range();
            json!({"start": range.start, "end": range.end, "reason": format!("{:?}", line.break_reason())})
        })
        .collect();
    let output = digest(
        &Run {
            lines,
            ..Default::default()
        },
        &fonts,
    )?;
    let break_all_ns = measure_samples(warmup, samples, || {
        Ok::<_, shodo_bench::BenchError>(paragraphs[0].break_all(
            &mut context,
            &options,
            workload.width,
            workload.atomics(),
        ))
    })?;
    Ok(json!({
        "schema": 1,
        "case": case,
        "scale": scale,
        "width": workload.width,
        "source_bytes": workload.text.len(),
        "warmup": warmup,
        "samples": samples,
        "operations": {"build": "Workload::build", "break_all": "Paragraph::break_all with default options and fixed width"},
        "build_ns": build_ns,
        "break_all_ns": break_all_ns,
        "digest": output,
        "line_semantics": line_semantics,
    }))
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        eprintln!("usage: spacing_summary_elapsed CASE SCALE WARMUP SAMPLES");
        std::process::exit(2);
    }
    let result = (|| {
        let scale = args[1].parse()?;
        let warmup = args[2].parse()?;
        let samples = args[3].parse()?;
        run(&args[0], scale, warmup, samples)
    })();
    match result {
        Ok(report) => println!("{}", serde_json::to_string(&report).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{measure_samples, run};

    #[test]
    fn every_warmup_and_sample_executes_the_operation() {
        let mut calls = 0;
        let samples = measure_samples(2, 3, || {
            calls += 1;
            Ok::<_, ()>(calls)
        })
        .unwrap();
        assert_eq!(calls, 5);
        assert_eq!(samples.len(), 3);
    }

    #[test]
    fn report_keeps_samples_and_line_semantics() {
        let report = run("latin-short", 1, 1, 2).unwrap();
        assert_eq!(report["build_ns"].as_array().unwrap().len(), 2);
        assert_eq!(report["break_all_ns"].as_array().unwrap().len(), 2);
        assert_eq!(
            report["line_semantics"].as_array().unwrap().last().unwrap()["reason"],
            "End"
        );
        assert_eq!(report["digest"]["glyphs"].as_u64().unwrap() > 0, true);
    }
}
