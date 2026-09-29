//! Reproducible public-API probe for the generic partial-line scan cache.
//! Run in both pinned revisions with identical source and fixture font bytes.
use serde_json::{Value, json};
use shodo::font::FontQuery;
use shodo::limits::Limits;
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, RichText};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUP: usize = 8;
const SAMPLES: usize = 21;
const REPEATS: usize = 4;

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    text: &'static str,
    initial_width: f32,
    retries: [f32; 4],
}

fn cases() -> Vec<Case> {
    // Leaked strings keep the workload definition outside the measured scope.
    let long = Box::leak("alpha beta gamma delta ".repeat(64).into_boxed_str());
    vec![
        Case {
            name: "short",
            text: "alpha beta gamma delta epsilon",
            initial_width: 10000.,
            retries: [512., 320., 192., 96.],
        },
        Case {
            name: "long",
            text: long,
            initial_width: 10000.,
            retries: [512., 320., 192., 96.],
        },
        Case {
            name: "ligature",
            text: "ffi office ffi office ffi office",
            initial_width: 10000.,
            retries: [512., 320., 192., 96.],
        },
        // This exact input at 48 px retains an owned selected-SHY overlay in
        // the first ordinary scan (see line/cache.rs's ownership test).
        Case {
            name: "owned_shy_overlay",
            text: "ab\u{ad}cdef\u{ad}ghijkl\u{ad}mn",
            initial_width: 48.,
            retries: [44., 40., 36., 32.],
        },
    ]
}

fn signature(line: &Line) -> Value {
    let glyphs: Vec<_> = line
        .fragments()
        .flat_map(|fragment| match fragment {
            shodo::Fragment::GlyphRun(run) => run
                .glyphs()
                .map(|glyph| {
                    json!([
                        glyph.id,
                        glyph.cluster,
                        glyph.inline_position.to_bits(),
                        glyph.block_offset.to_bits(),
                        glyph.advance.to_bits()
                    ])
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    json!({
        "range": [line.text_range().start, line.text_range().end],
        "reason": format!("{:?}", line.break_reason()),
        "continuation": format!("{:?}", line.break_token()),
        "geometry": [line.inline_size().to_bits(), line.block_size().to_bits()],
        "glyphs": glyphs,
    })
}

fn signature_with_continuation(
    paragraph: &shodo::Paragraph,
    line_value: &Line,
    width: f32,
) -> Value {
    let mut fresh = LayoutContext::new();
    let next = match line(paragraph, &mut fresh, line_value.break_token(), width, None) {
        LineResult::Line(next) => signature(&next),
        LineResult::Done => Value::Null,
        result => panic!("unexpected continuation: {result:?}"),
    };
    json!({"line": signature(line_value), "next": next})
}

fn line(
    paragraph: &shodo::Paragraph,
    cx: &mut LayoutContext,
    token: shodo::BreakToken,
    width: f32,
    height: Option<f32>,
) -> LineResult {
    let mut constraint = LineConstraint::new(width);
    constraint.max_block_size = height;
    paragraph.next_line(
        cx,
        token,
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    )
}

fn run(
    paragraph: &shodo::Paragraph,
    case: Case,
    operation: &str,
    inspect: bool,
) -> (LayoutContext, Vec<Value>) {
    let mut cx = LayoutContext::new();
    let mut signatures = Vec::new();
    let start = paragraph.start_token();
    match operation {
        "continuous" => {
            let mut token = start;
            loop {
                match line(paragraph, &mut cx, token, 96., None) {
                    LineResult::Line(result) => {
                        if inspect {
                            signatures.push(signature_with_continuation(paragraph, &result, 96.));
                        }
                        let next = result.break_token();
                        assert_ne!(next, token, "line must progress");
                        token = next;
                    }
                    LineResult::Done => break,
                    result => panic!("unexpected continuous result: {result:?}"),
                }
            }
        }
        "height_retry" => {
            assert!(matches!(
                line(paragraph, &mut cx, start, case.initial_width, Some(0.)),
                LineResult::BlockSizeExceeded { .. }
            ));
            let LineResult::Line(result) =
                line(paragraph, &mut cx, start, case.initial_width, None)
            else {
                panic!("height retry must yield line")
            };
            if inspect {
                signatures.push(signature_with_continuation(
                    paragraph,
                    &result,
                    case.initial_width,
                ));
            }
        }
        "shrink_1" | "shrink_4" => {
            let LineResult::Line(first) = line(paragraph, &mut cx, start, case.initial_width, None)
            else {
                panic!("initial width must yield line")
            };
            if inspect {
                signatures.push(signature_with_continuation(
                    paragraph,
                    &first,
                    case.initial_width,
                ));
            }
            let count = if operation == "shrink_1" { 1 } else { 4 };
            for &width in &case.retries[..count] {
                let LineResult::Line(result) = line(paragraph, &mut cx, start, width, None) else {
                    panic!("shrink retry at {width} must yield line")
                };
                if inspect {
                    signatures.push(signature_with_continuation(paragraph, &result, width));
                }
            }
        }
        _ => unreachable!(),
    }
    (cx, signatures)
}

fn main() {
    let mode = std::env::args().nth(1).expect("time or alloc");
    let case_filter = std::env::var("SHODO_SCAN_CASE").ok();
    let operation_filter = std::env::var("SHODO_SCAN_OPERATION").ok();
    let repeats = std::env::var("SHODO_SCAN_REPEATS")
        .ok()
        .map(|value| value.parse::<usize>().expect("positive repeat count"))
        .unwrap_or(REPEATS);
    assert!(repeats > 0);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    // Resolve the exact intended font before all measurements.
    let query = FontQuery {
        families: style.root.font_families.clone(),
        ..Default::default()
    };
    assert!(
        fonts
            .collection
            .resolve_ch(&query, style.root.font_size)
            .id
            .is_some()
    );
    let operations = ["continuous", "height_retry", "shrink_1", "shrink_4"];
    let mut results = serde_json::Map::new();
    for case in cases() {
        let paragraph = RichText::with_limits(&style, &limits)
            .push(case.text, &style.root)
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        // Build every paragraph to keep opaque BreakToken IDs stable in the
        // captured signatures even when profiling just one case.
        if case_filter.as_deref().is_some_and(|name| name != case.name) {
            continue;
        }
        for operation in operations {
            if operation_filter
                .as_deref()
                .is_some_and(|name| name != operation)
            {
                continue;
            }
            let key = format!("{}:{operation}", case.name);
            let (_, signatures) = run(&paragraph, case, operation, true);
            for _ in 0..WARMUP {
                black_box(run(&paragraph, case, operation, false));
            }
            let samples: Vec<Value> = match mode.as_str() {
                "time" => (0..SAMPLES)
                    .map(|_| {
                        let start = Instant::now();
                        for _ in 0..repeats {
                            black_box(run(&paragraph, case, operation, false));
                        }
                        json!(start.elapsed().as_nanos() as f64 / repeats as f64)
                    })
                    .collect(),
                #[cfg(feature = "allocation-counting")]
                "alloc" => (0..SAMPLES)
                    .map(|_| {
                        let scope = ALLOCATOR.begin().unwrap();
                        let (cx, _) = run(&paragraph, case, operation, false);
                        black_box(&cx);
                        let counts = scope.finish();
                        drop(cx);
                        json!(counts)
                    })
                    .collect(),
                _ => panic!("mode must be time or alloc; alloc requires allocation-counting"),
            };
            results.insert(key, json!({"signatures":signatures,"samples":samples}));
        }
    }
    println!(
        "{}",
        json!({"mode":mode,"warmup":WARMUP,"samples":SAMPLES,"repeats":repeats,"results":results})
    );
}
