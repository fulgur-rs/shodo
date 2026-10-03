//! Suppressed-warning format probe (shodo-zb0.10).
//! Builds paragraphs with many invalid sanitize inputs under tight warning caps
//! and reports warning equivalence plus allocation/timing.
//! Run without features for timing, then with `allocation-counting` and `alloc`.
use shodo::font::{FontCollection, FontOptions};
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle};
use shodo::{LayoutContext, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const ITEMS: usize = 2000;

fn bad_edges() -> InlineEdges {
    InlineEdges {
        margin: Sides {
            inline_start: f32::NAN,     // NonFinite
            inline_end: 2.0e7,          // Saturated (>1e7)
            block_start: f32::INFINITY, // NonFinite
            block_end: 0.0,
        },
        border: Sides {
            inline_start: -4.0,   // Negative (border may not be negative)
            inline_end: f32::NAN, // NonFinite
            block_start: 0.0,
            block_end: 0.0,
        },
        padding: Sides {
            inline_start: f32::NEG_INFINITY, // NonFinite
            inline_end: -1.0,                // Negative
            block_start: 0.0,
            block_end: 0.0,
        },
    }
}

fn build_paragraph(limits: &Limits) -> (shodo::Paragraph, Vec<shodo::limits::Warning>) {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let style = ParagraphStyle::default();
    let mut builder = ParagraphBuilder::new(&style, limits);
    let span = InlineStyle::default();
    for i in 0..ITEMS {
        let node = NodeId(i as u64 + 1);
        builder
            .open_inline(node, &span, bad_edges())
            .push_text(TextSource::Generated { node }, "a")
            .close_inline();
    }
    let mut cx = LayoutContext::new();
    let paragraph = builder.build(&mut cx, &fonts).expect("build within limits");
    let warnings = paragraph.warnings().to_vec();
    (paragraph, warnings)
}

fn summarize(limits: &Limits, label: &str) {
    // Warmup build outside allocation scope.
    let (para, warnings) = build_paragraph(limits);
    let suppressed = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::Suppressed)
        .count();
    let non_finite = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::NonFiniteInput)
        .count();
    let saturated = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::Saturated)
        .count();
    let negative = warnings
        .iter()
        .filter(|w| w.kind == WarningKind::NegativeInput)
        .count();
    println!(
        "[{label}] warnings={} suppressed={} non_finite={} saturated={} negative={} text_len={}",
        warnings.len(),
        suppressed,
        non_finite,
        saturated,
        negative,
        para.text().len()
    );
    for (i, w) in warnings.iter().take(12).enumerate() {
        println!("  w[{i}] {:?}: {}", w.kind, w.message);
    }

    // Timed rebuilds.
    let iters = 5;
    let start = Instant::now();
    for _ in 0..iters {
        black_box(build_paragraph(black_box(limits)));
    }
    let elapsed = start.elapsed();
    println!(
        "[{label}] {iters} builds in {elapsed:?} ({:?}/build)",
        elapsed / iters
    );

    #[cfg(feature = "allocation-counting")]
    {
        let scope = ALLOC.begin().unwrap();
        black_box(build_paragraph(black_box(limits)));
        let counts = scope.finish();
        println!(
            "[{label}] alloc calls={} bytes={} peak_extra={} live={}",
            counts.calls, counts.allocated_bytes, counts.peak_extra_bytes, counts.live_bytes
        );
    }
}

fn main() {
    let zero = Limits {
        max_warnings: Some(0),
        ..Limits::default()
    };
    let small = Limits {
        max_warnings: Some(5),
        ..Limits::default()
    };
    let def = Limits::default();
    summarize(&zero, "max_warnings=0");
    summarize(&small, "max_warnings=5");
    summarize(&def, "max_warnings=default(1024)");
}
