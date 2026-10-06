#![cfg(feature = "allocation-counting")]

use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::style::{FontFamily, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, RichText};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn contiguous_selection_allocation_stays_bounded_as_text_grows() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    style.root.font_size = 16.0;
    let mut cx = LayoutContext::new();
    for length in [1024, 4096, 16384] {
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
        let layout = LineLayout::new(&lines);
        let start = TextPosition {
            line: 0,
            offset: 0,
            affinity: Affinity::Downstream,
        };
        for offset in [1, length as u32] {
            let end = TextPosition {
                offset,
                affinity: Affinity::Upstream,
                ..start
            };
            let left = layout.caret(start).unwrap().rect;
            let right = layout.caret(end).unwrap().rect;
            let expected = shodo::geometry::LogicalRect {
                inline_size: right.inline_start - left.inline_start,
                ..left
            };
            for (a, b) in [(start, end), (end, start)] {
                let scope = ALLOCATOR.begin().unwrap();
                let rects = black_box(layout.selection_rects(a, b));
                let counts = scope.finish();
                assert_eq!(rects, [expected]);
                assert!(
                    counts.allocated_bytes <= 512,
                    "one contiguous rectangle must need bounded working storage, \
                     length={length} selected={offset}: {counts:?}"
                );
            }
        }
    }
}
