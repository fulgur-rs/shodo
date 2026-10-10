use shodo::{LayoutContext, limits::Limits};
use shodo_bench::{Operation, Workload, digest, layout, workloads};
use shodo_fixtures::load_fonts;
use std::error::Error;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--describe"] {
        // This is the timing matrix consumed by tools/bench/run.py. Ad hoc
        // cold/memory probes remain addressable directly by ID and scale.
        let definitions: Vec<_> = workloads().iter().map(Workload::settings).collect();
        println!("{}", serde_json::to_string(&definitions)?);
        return Ok(());
    }
    if args.len() != 3 {
        return Err("usage: shodo-probe --cold|--memory ID SCALE".into());
    }
    let mode = args[0].as_str();
    if (mode == "--cold" && cfg!(feature = "allocation-counting"))
        || (mode == "--memory" && !cfg!(feature = "allocation-counting"))
    {
        return Err("cold timing and allocator measurements require separate builds".into());
    }
    if !["--cold", "--memory"].contains(&mode) {
        return Err("unknown probe mode".into());
    }
    let workload = workload(&args[1], args[2].parse()?)?;
    let limits = Limits::default();
    #[cfg(not(feature = "allocation-counting"))]
    let report = cold(&workload, &limits)?;
    #[cfg(feature = "allocation-counting")]
    let report = memory(&workload, &limits)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn workload(id: &str, scale: usize) -> Result<Workload, Box<dyn Error>> {
    if id != "itemize-graphemes" {
        return Ok(Workload::named(id, scale)?);
    }
    if ![1_024, 4_096, 16_384].contains(&scale) {
        return Err("itemize-graphemes size must be 1024, 4096 or 16384".into());
    }
    let mut workload = Workload::named("latin-short", 1)?;
    workload.id = id.into();
    workload.scale = scale;
    workload.text = "a".repeat(scale);
    Ok(workload)
}

#[cfg(not(feature = "allocation-counting"))]
fn cold(w: &Workload, limits: &Limits) -> Result<serde_json::Value, Box<dyn Error>> {
    use std::time::Instant;
    let begin = Instant::now();
    let mut cx = LayoutContext::new();
    let context = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let fonts = load_fonts(limits)?;
    let fonts_time = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let ps = w.build(&mut cx, &fonts, limits)?;
    let build = begin.elapsed().as_nanos();
    let begin = Instant::now();
    let lines = layout(w, &ps, &mut cx, &fonts, limits, Operation::AllLines)?;
    let all_lines = begin.elapsed().as_nanos();
    let output = digest(&lines, &fonts)?;
    Ok(
        serde_json::json!({"schema":1,"mode":"cold","instrumented":false,"settings":w.settings(),"durations_ns":{"context_init":context,"font_initialization_registration":fonts_time,"build":build,"all_lines":all_lines},"digest":output}),
    )
}
#[cfg(feature = "allocation-counting")]
fn measured<T>(
    f: impl FnOnce() -> Result<T, Box<dyn Error>>,
) -> Result<(T, shodo_bench::allocator::AllocationCounts), Box<dyn Error>> {
    let scope = ALLOCATOR.begin()?;
    let result = f()?;
    let counts = scope.finish();
    Ok((result, counts))
}
#[cfg(feature = "allocation-counting")]
fn memory(w: &Workload, limits: &Limits) -> Result<serde_json::Value, Box<dyn Error>> {
    use serde::Serialize;
    use shodo::style::{LineOptions, TextAlign};
    use shodo_bench::{allocator::AllocationCounts, layout_with_options};
    #[derive(Serialize)]
    struct Row {
        name: &'static str,
        counts: AllocationCounts,
    }
    let mut rows = Vec::with_capacity(20);
    let mut record = |name, counts| rows.push(Row { name, counts });
    let ((mut cx, fonts), counts) = measured(|| Ok((LayoutContext::new(), load_fonts(limits)?)))?;
    record("font_context_init", counts);
    let (ps, counts) = measured(|| Ok(w.build(&mut cx, &fonts, limits)?))?;
    record("build", counts);
    let build_net = counts.net_bytes;
    let default_digest = digest(
        &layout(w, &ps, &mut cx, &fonts, limits, Operation::AllLines)?,
        &fonts,
    )?;
    // Both line modes are independently warmed and their output ownership
    // released before measuring the equivalent retained-output call.
    let plain_options = LineOptions::default();
    drop(layout_with_options(
        w,
        &ps,
        &mut cx,
        &fonts,
        limits,
        Operation::AllLines,
        &plain_options,
    )?);
    let (plain, counts) = measured(|| {
        Ok(layout_with_options(
            w,
            &ps,
            &mut cx,
            &fonts,
            limits,
            Operation::AllLines,
            &plain_options,
        )?)
    })?;
    record("plain_lines", counts);
    let plain_net = counts.net_bytes;
    let plain_digest = digest(&plain, &fonts)?;
    let (_, counts) = measured(|| {
        drop(plain);
        Ok(())
    })?;
    record("release_plain_lines", counts);
    let options = LineOptions {
        text_align: TextAlign::Justify,
        ..Default::default()
    };
    drop(layout_with_options(
        w,
        &ps,
        &mut cx,
        &fonts,
        limits,
        Operation::AllLines,
        &options,
    )?);
    let (justified, counts) = measured(|| {
        Ok(layout_with_options(
            w,
            &ps,
            &mut cx,
            &fonts,
            limits,
            Operation::AllLines,
            &options,
        )?)
    })?;
    record("justify_lines", counts);
    let justify_net = counts.net_bytes;
    let justify_digest = digest(&justified, &fonts)?;
    let (_, counts) = measured(|| {
        drop(justified);
        Ok(())
    })?;
    record("release_justify_lines", counts);
    let mut outputs = Vec::new();
    for (name, operation) in [
        ("reuse_widths", Operation::ReuseWidths),
        ("rebuild_widths", Operation::RebuildWidths),
        ("page_retry", Operation::PageRetry),
        ("intrinsic", Operation::Intrinsic),
    ] {
        drop(layout(w, &ps, &mut cx, &fonts, limits, operation)?);
        let (run, counts) = measured(|| Ok(layout(w, &ps, &mut cx, &fonts, limits, operation)?))?;
        record(name, counts);
        outputs.push((name, digest(&run, &fonts)?));
        let (_, counts) = measured(|| {
            drop(run);
            Ok(())
        })?;
        record(
            match name {
                "reuse_widths" => "release_reuse",
                "rebuild_widths" => "release_rebuild",
                "page_retry" => "release_pages",
                _ => "release_intrinsic",
            },
            counts,
        );
    }
    if outputs[0].1 != outputs[1].1 {
        return Err("reuse and rebuild outputs differ".into());
    }
    let (_, counts) = measured(|| {
        drop(ps);
        Ok(())
    })?;
    record("drop_paragraphs", counts);
    let (_, counts) = measured(|| {
        cx.shrink_to(0);
        Ok(())
    })?;
    record("context_shrink_zero", counts);
    let (_, counts) = measured(|| {
        drop(cx);
        Ok(())
    })?;
    record("drop_context", counts);
    let (_, counts) = measured(|| {
        drop(fonts);
        Ok(())
    })?;
    record("drop_fonts", counts);
    let ratio = |bytes: i128, n: usize| {
        if n == 0 {
            None
        } else {
            Some(bytes as f64 / n as f64)
        }
    };
    Ok(
        serde_json::json!({"schema":1,"mode":"memory","instrumented":true,"settings":w.settings(),"scopes":rows,"digests":{"default":default_digest,"plain":plain_digest,"justify":justify_digest,"reuse":outputs[0].1,"rebuild":outputs[1].1,"pages":outputs[2].1,"intrinsic":outputs[3].1},"normalization":{"paragraph_and_context_net_bytes_per_painted_glyph":ratio(build_net,plain_digest.glyphs),"plain_line_and_context_net_bytes_per_line":ratio(plain_net,plain_digest.lines),"justify_line_and_context_net_bytes_per_line":ratio(justify_net,justify_digest.lines),"justify_incremental_net_bytes":justify_net-plain_net},"scope_notes":"Workload input/configuration and report/digest serialization excluded; context/paragraph/line owners included as named. Default output validated outside allocator scopes; plain and justify line modes independently warmed. Allocator requested blocks only; not RSS, stack, native malloc overhead or in-realloc temporary native storage."}),
    )
}
