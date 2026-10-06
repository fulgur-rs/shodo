#![cfg(feature = "allocation-counting")]

use shodo::hit::LineLayout;
use shodo::limits::Limits;
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, RichText};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn long_plain_source_geometry_avoids_duplicate_cluster_storage() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    style.root.font_size = 16.0;
    let mut cx = LayoutContext::new();
    for length in [1024, 4096] {
        let text = "a".repeat(length);
        let paragraph = RichText::with_limits(&style, &limits)
            .push(&text, &style.root)
            .build(&mut cx, &fonts.collection)
            .unwrap();
        assert!(paragraph.warnings().is_empty());
        let lines = paragraph.break_all(
            &mut cx,
            &LineOptions::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1);
        assert!(cx.take_warnings().is_empty());

        let scope = ALLOCATOR.begin().unwrap();
        let spans = black_box(lines[0].paint_spans());
        let counts = scope.finish();
        assert_eq!(spans.len(), length);
        assert_eq!(spans[0].text_range, 0..1);
        assert_eq!(spans[length - 1].text_range, length - 1..length);
        assert!(
            counts.allocated_bytes <= length as u64 * 700,
            "paint geometry should not duplicate full cluster buffers: {counts:?}"
        );

        let scope = ALLOCATOR.begin().unwrap();
        let layout = black_box(LineLayout::new(&lines));
        let counts = scope.finish();
        assert!(
            layout
                .caret(shodo::hit::TextPosition {
                    line: 0,
                    offset: length as u32,
                    affinity: shodo::mapping::Affinity::Upstream,
                })
                .is_some()
        );
        assert!(
            counts.allocated_bytes <= length as u64 * 700,
            "hit geometry should not duplicate full cluster buffers: {counts:?}"
        );
    }
}
