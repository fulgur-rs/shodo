//! Fixed-font remaining ancestor-work probe; build and fresh layout are separate.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{BoxDecorationBreak, FontFamily, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn builder(depth: usize, mode: &str, first: bool, limits: &Limits) -> ParagraphBuilder {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
    style.first_line = first.then(|| shodo::style::InlineStyle {
        font_size: 20.0,
        ..style.root.clone()
    });
    let mut b = ParagraphBuilder::new(&style, limits);
    b.with_offset_mapping(true);
    for i in 0..depth {
        let mut inline = style.root.clone();
        inline.box_decoration_break = if mode == "clone" || mode == "mixed" && i % 2 == 0 {
            BoxDecorationBreak::Clone
        } else {
            BoxDecorationBreak::Slice
        };
        let mut edges = InlineEdges::default();
        edges.margin.inline_start = -0.125;
        edges.padding.inline_start = 0.25;
        edges.margin.inline_end = -0.125;
        edges.padding.inline_end = 0.375;
        b.open_inline(NodeId(i as u64 + 10), &inline, edges);
    }
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 7,
        },
        &"abcd".repeat(256),
    );
    for _ in 0..depth {
        b.close_inline();
    }
    b
}

fn measured<T>(f: impl FnOnce() -> T) -> (T, Value) {
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOC.begin().unwrap();
    #[cfg(not(feature = "allocation-counting"))]
    let start = std::time::Instant::now();
    let value = std::hint::black_box(f());
    #[cfg(feature = "allocation-counting")]
    let counts = scope.finish();
    #[cfg(feature = "allocation-counting")]
    let result = json!({"allocation": counts});
    #[cfg(not(feature = "allocation-counting"))]
    let result = {
        let ns = start.elapsed().as_nanos() as u64;
        json!({"ns": ns})
    };
    (value, result)
}

fn main() {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for depth in [1, 64, 256] {
        for mode in ["slice", "clone", "mixed"] {
            for first in [false, true]
                .into_iter()
                .filter(|first| !first || depth == 64)
            {
                let bounded = Limits {
                    max_shaped_glyphs: Some(64),
                    ..limits.clone()
                };
                let rejected = builder(depth, mode, first, &bounded)
                    .build(&mut LayoutContext::new(), &fonts.collection)
                    .unwrap_err();
                for sample in 0..11 {
                    let b = builder(depth, mode, first, &limits);
                    let mut build_cx = LayoutContext::new();
                    let (p, build) =
                        measured(|| b.build(&mut build_cx, &fonts.collection).unwrap());
                    let mut cx = LayoutContext::new();
                    let (lines, layout) = measured(|| {
                        p.break_all(
                            &mut cx,
                            &Default::default(),
                            1_000_000.0,
                            &AtomicSizes::EMPTY,
                        )
                    });
                    assert_eq!(lines.len(), 1);
                    let public = lines.iter().map(snapshot::line).collect::<Vec<_>>();
                    let digest =
                        format!("{:x}", Sha256::digest(serde_json::to_vec(&public).unwrap()));
                    println!(
                        "{}",
                        json!({"depth":depth,"mode":mode,"first_line":first,"sample":sample,"build":build,"layout":layout,"digest":digest,"build_warnings":format!("{:?}",p.warnings()),"layout_warnings":format!("{:?}",cx.take_warnings()),"rejected":format!("{rejected:?}")})
                    );
                }
            }
        }
    }
}
