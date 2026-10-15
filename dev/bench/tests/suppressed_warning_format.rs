#![cfg(feature = "allocation-counting")]

//! shodo-zb0.10: suppressed warning messages are lazily formatted.
//! Many invalid sanitize inputs under tight caps must keep exact warnings
//! while avoiding a String per dropped warning.

use shodo::font::{FontCollection, FontOptions};
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle};
use shodo::{LayoutContext, ParagraphBuilder};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const ITEMS: usize = 2000;

fn bad_edges() -> InlineEdges {
    InlineEdges {
        margin: Sides {
            inline_start: f32::NAN,
            inline_end: 2.0e7,
            block_start: f32::INFINITY,
            block_end: 0.0,
        },
        border: Sides {
            inline_start: -4.0,
            inline_end: f32::NAN,
            block_start: 0.0,
            block_end: 0.0,
        },
        padding: Sides {
            inline_start: f32::NEG_INFINITY,
            inline_end: -1.0,
            block_start: 0.0,
            block_end: 0.0,
        },
    }
}

fn build(limits: &Limits) -> shodo::Paragraph {
    build_with_edges(limits, bad_edges())
}

fn build_with_edges(limits: &Limits, edges: InlineEdges) -> shodo::Paragraph {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let style = ParagraphStyle::default();
    let span = InlineStyle::default();
    let mut builder = ParagraphBuilder::new(&style, limits);
    for i in 0..ITEMS {
        let node = NodeId(i as u64 + 1);
        builder
            .open_inline(node, &span, edges)
            .push_text(TextSource::Generated { node }, "a")
            .close_inline();
    }
    let mut cx = LayoutContext::new();
    builder.build(&mut cx, &fonts).expect("within limits")
}

#[test]
fn suppressed_warnings_keep_exact_output_without_per_drop_strings() {
    // max_warnings=0: only the suppression marker is retained.
    let zero = Limits {
        max_warnings: Some(0),
        ..Limits::default()
    };
    let para = build(&zero);
    assert_eq!(para.text().len(), ITEMS);
    let warnings = para.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].kind, WarningKind::Suppressed);
    assert_eq!(warnings[0].message, "further warnings suppressed");

    let scope = ALLOCATOR.begin().unwrap();
    black_box(build(black_box(&zero)));
    let zero_counts = scope.finish();
    println!(
        "max_warnings=0: {} calls {} bytes live {}",
        zero_counts.calls, zero_counts.allocated_bytes, zero_counts.live_bytes
    );
    // Same retained geometry and width-index construction as bad_edges after
    // sanitize. Empty fonts can still warn; both zero-cap outputs keep the
    // same single marker. Only the invalid float diagnostics differ.
    let clean_edges = InlineEdges {
        margin: Sides {
            inline_end: 1.0e7,
            ..Default::default()
        },
        ..Default::default()
    };
    let clean = build_with_edges(&zero, clean_edges);
    assert_eq!(clean.text(), para.text());
    assert_eq!(clean.warnings(), para.warnings());
    let scope = ALLOCATOR.begin().unwrap();
    black_box(build_with_edges(black_box(&zero), clean_edges));
    let clean_counts = scope.finish();
    println!(
        "sanitized control: {} calls {} bytes live {}",
        clean_counts.calls, clean_counts.allocated_bytes, clean_counts.live_bytes
    );
    // Seven bad floats x 2000 items = 14000 dropped messages avoided.
    assert!(
        zero_counts.calls < 20000,
        "suppressed build still formats per dropped warning: {:?}",
        zero_counts
    );
    assert!(
        zero_counts.allocated_bytes <= clean_counts.allocated_bytes + 1024,
        "suppressed float diagnostics allocate beyond fixed marker overhead: invalid={zero_counts:?}, control={clean_counts:?}"
    );
    assert!(
        zero_counts.calls <= clean_counts.calls + 16,
        "suppressed float diagnostics allocate per drop: invalid={zero_counts:?}, control={clean_counts:?}"
    );

    // Small cap: first 5 messages in order, then the single marker.
    let small = Limits {
        max_warnings: Some(5),
        ..Limits::default()
    };
    let para = build(&small);
    let warnings = para.warnings();
    assert_eq!(warnings.len(), 6, "{warnings:?}");
    let kinds: Vec<_> = warnings.iter().map(|w| w.kind).collect();
    assert_eq!(
        kinds,
        vec![
            WarningKind::NonFiniteInput,
            WarningKind::Saturated,
            WarningKind::NonFiniteInput,
            WarningKind::NegativeInput,
            WarningKind::NonFiniteInput,
            WarningKind::Suppressed,
        ]
    );
    assert_eq!(warnings[0].message, "non-finite margin replaced with 0");
    assert_eq!(warnings[1].message, "margin clamped to 1e7 px");
    assert_eq!(warnings[5].message, "further warnings suppressed");

    let scope = ALLOCATOR.begin().unwrap();
    black_box(build(black_box(&small)));
    let small_counts = scope.finish();
    println!(
        "max_warnings=5: {} calls {} bytes",
        small_counts.calls, small_counts.allocated_bytes
    );
    assert!(
        small_counts.calls < 20000,
        "small-cap build still formats per dropped warning: {:?}",
        small_counts
    );

    // Default cap: full ordering, counts, and saturation output are unchanged.
    let paragraph = build(&Limits::default());
    let warnings = paragraph.warnings();
    assert_eq!(warnings.len(), 1025, "1024 stored + 1 marker");
    assert_eq!(warnings.last().unwrap().kind, WarningKind::Suppressed);
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
    assert_eq!((non_finite, saturated, negative), (585, 147, 292));
    // Spot-check saturation and negative outputs beyond the first window.
    assert!(
        warnings
            .iter()
            .any(|w| w.message == "margin clamped to 1e7 px")
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.message == "negative padding replaced with 0")
    );
    assert!(
        warnings
            .iter()
            .all(|w| !w.message.is_empty() && w.message.len() < 200)
    );

    let scope = ALLOCATOR.begin().unwrap();
    black_box(build(black_box(&Limits::default())));
    let default_counts = scope.finish();
    println!(
        "default cap: {} calls {} bytes",
        default_counts.calls, default_counts.allocated_bytes
    );
    // Baseline: 31342 calls / 7436505 bytes; ~12976 dropped messages avoided.
    assert!(
        default_counts.calls < 21000,
        "default-cap build still formats per dropped warning: {:?}",
        default_counts
    );
}
