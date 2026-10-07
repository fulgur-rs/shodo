//! Fixed-font line-profile probe: OPERATION CASE LENGTH ITERATIONS WIDTH.
//! Time and allocations include output destruction. Layout input preparation,
//! output digests and JSON formatting are outside the measured scope. Build
//! includes style/string/RichText input preparation. Combined measures paint
//! plus LineLayout construction on already accepted lines; it excludes layout.
//! `spaces` uses breakable text to exercise genuinely multiple accepted lines.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::hit::LineLayout;
use shodo::limits::Limits;
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextEmphasis, TextEmphasisPosition,
    TextEmphasisShape,
};
use shodo::{AtomicSizes, LayoutContext, Paragraph, RichText};
use shodo_bench::{Run, digest};
use std::hint::black_box;
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
fn build(
    case: &str,
    n: usize,
    cx: &mut LayoutContext,
    fonts: &shodo_fixtures::FixtureFonts,
    limits: &Limits,
) -> Paragraph {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    style.root.font_size = 16.0;
    if case == "emphasis" {
        style.root.text_emphasis = Some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        });
    }
    let mut builder = RichText::with_limits(&style, limits);
    if matches!(case, "colors" | "languages") {
        let mut styles: [InlineStyle; 2] = [style.root.clone(), style.root.clone()];
        if case == "colors" {
            styles[1].paint.color = [200, 0, 0, 255];
        } else {
            styles[0].lang = Some("en".into());
            styles[1].lang = Some("fr".into());
        }
        for i in 0..n {
            builder = builder.push("a", &styles[i % 2]);
        }
    } else {
        let text = if case == "spaces" {
            "a ".repeat(n)
        } else {
            "a".repeat(n)
        };
        builder = builder.push(&text, &style.root);
    }
    let paragraph = builder.build(cx, &fonts.collection).unwrap();
    assert!(paragraph.warnings().is_empty());
    paragraph
}
fn lines(p: &Paragraph, cx: &mut LayoutContext, width: f32) -> Run {
    Run {
        lines: p.break_all(cx, &LineOptions::default(), width, &AtomicSizes::EMPTY),
        float_reports: 0,
        height_retries: 0,
        intrinsics: Vec::new(),
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 5, "OPERATION CASE LENGTH ITERATIONS WIDTH");
    let op = args[0].as_str();
    let case = args[1].as_str();
    let n = args[2].parse().unwrap();
    let iterations: usize = args[3].parse().unwrap();
    let width: f32 = args[4].parse().unwrap();
    assert!(matches!(
        case,
        "plain" | "emphasis" | "colors" | "languages" | "spaces"
    ));
    assert!(iterations > 0);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut cx = LayoutContext::new();
    let paragraph = build(case, n, &mut cx, &fonts, &limits);
    let run = lines(&paragraph, &mut cx, width);
    assert!(cx.take_warnings().is_empty());
    let geometry = digest(&run, &fonts).unwrap();
    let paint_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{:?}",
                run.lines
                    .iter()
                    .map(|l| l.paint_spans())
                    .collect::<Vec<_>>()
            )
            .as_bytes()
        )
    );
    let mut execute = || match op {
        "build" => {
            black_box(build(case, n, &mut cx, &fonts, &limits));
        }
        "layout" => {
            black_box(lines(&paragraph, &mut cx, width));
        }
        "paint" => {
            black_box(
                run.lines
                    .iter()
                    .map(|l| l.paint_spans())
                    .collect::<Vec<_>>(),
            );
        }
        "index" => {
            black_box(LineLayout::new(black_box(&run.lines)));
        }
        "combined" => {
            black_box(
                run.lines
                    .iter()
                    .map(|l| l.paint_spans())
                    .collect::<Vec<_>>(),
            );
            black_box(LineLayout::new(black_box(&run.lines)));
        }
        _ => panic!("unknown operation"),
    };
    for _ in 0..3 {
        execute();
    }
    #[cfg(feature = "allocation-counting")]
    let measure = {
        let scope = ALLOC.begin().unwrap();
        for _ in 0..iterations {
            execute();
        }
        let counts = scope.finish();
        json!({"allocation":counts})
    };
    #[cfg(not(feature = "allocation-counting"))]
    let measure = {
        let begin = std::time::Instant::now();
        for _ in 0..iterations {
            execute();
        }
        let ns = begin.elapsed().as_nanos();
        json!({"ns":ns})
    };
    let after = lines(&paragraph, &mut cx, width);
    assert_eq!(geometry, digest(&after, &fonts).unwrap());
    assert!(cx.take_warnings().is_empty());
    println!(
        "{}",
        json!({"op":op,"case":case,"length":n,"width":width,"iterations":iterations,"measure":measure,"geometry":geometry,"paint_sha256":paint_hash})
    );
}
