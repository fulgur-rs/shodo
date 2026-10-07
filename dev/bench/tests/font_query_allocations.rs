#![cfg(feature = "allocation-counting")]

use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use shodo_bench::allocator::CountingAllocator;

#[global_allocator]
static ALLOC: CountingAllocator<std::alloc::System> = CountingAllocator::new(std::alloc::System);

#[test]
fn paint_only_styles_do_not_copy_a_large_font_query_payload_per_style() {
    let limits = Limits::unlimited();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    // Keep the query below the match cache's payload cap so this test isolates
    // style/query preparation rather than intentional cache bypass.
    let payload_bytes = 3 * 1024;
    let family = "x".repeat(payload_bytes);
    let id = fonts
        .register_face(
            shodo_fixtures::font("latin").unwrap().bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: family.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(family)],
            lang: Some("EN-us".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let mut warm = ParagraphBuilder::new(&style, &limits);
    warm.push_text(TextSource::Generated { node: NodeId(100) }, "a");
    drop(warm.build(&mut cx, &fonts).unwrap());
    let mut builder = ParagraphBuilder::new(&style, &limits);
    for i in 0..32 {
        let mut inline = style.root.clone();
        inline.paint.color = [i as u8, 17, 23, 255];
        let node = NodeId(i);
        builder
            .open_inline(node, &inline, InlineEdges::default())
            .push_text(TextSource::Generated { node }, "a")
            .close_inline();
    }
    let analysis = builder.analyze().unwrap();
    let scope = ALLOC.begin().unwrap();
    let paragraph = analysis.shape(&mut cx, &fonts).unwrap();
    let counts = scope.finish();
    assert_eq!(paragraph.text(), "a".repeat(32));
    assert!(paragraph.warnings().is_empty());
    let lines = paragraph.break_all(
        &mut cx,
        &LineOptions::default(),
        100_000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert!(cx.take_warnings().is_empty());
    let spans = lines[0].paint_spans();
    assert_eq!(spans.len(), 32);
    for (i, span) in spans.iter().enumerate() {
        assert_eq!(span.style.color, [i as u8, 17, 23, 255]);
    }
    assert!(lines[0].fragments().all(|fragment| match fragment {
        shodo::Fragment::GlyphRun(run) => run.font() == id,
        _ => true,
    }));
    // Include all shaping work, but leave room for shared queries and metrics.
    // Two transient payload copies per style alone would exceed this ceiling.
    assert!(
        counts.allocated_bytes < (48 * payload_bytes) as u64,
        "shared font queries allocated {} bytes for a {} byte payload",
        counts.allocated_bytes,
        payload_bytes,
    );
}
