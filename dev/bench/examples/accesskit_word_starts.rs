//! Fixed-font AccessKit update probe; build/layout and output hashing are untimed.
//! Run twice: default allocator for time, allocation-counting for memory.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::accessibility::AccessibleLayout;
use shodo::accessibility::accesskit::{AccessKitAdapter, NodeSemantics, types};
use shodo::geometry::PhysicalRect;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);
fn export(layout: &AccessibleLayout<'_>) -> types::TreeUpdate {
    let mut adapter = AccessKitAdapter::new(types::NodeId(1));
    let mut id = 1;
    adapter
        .update(
            layout,
            types::Node::new(types::Role::Document),
            PhysicalRect {
                x: 0.0,
                y: 0.0,
                width: 1_000_000.0,
                height: 100.0,
            },
            None,
            |_| NodeSemantics::default(),
            || {
                id += 1;
                types::NodeId(id)
            },
        )
        .unwrap()
}
fn main() {
    let limits = Default::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for (count, separate) in [
        (512, false),
        (4096, false),
        (16384, false),
        (256, true),
        (1024, true),
        (4096, true),
    ] {
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())];
        let mut builder = ParagraphBuilder::new(&style, &limits);
        if separate {
            for i in 0..count {
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(i as u64 + 1),
                        offset: 0,
                    },
                    "a ",
                );
            }
        } else {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(1),
                    offset: 0,
                },
                &"a ".repeat(count),
            );
        }
        let paragraph = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let lines = paragraph.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1_000_000.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1);
        for line in &lines {
            for fragment in line.fragments() {
                if let shodo::Fragment::GlyphRun(run) = fragment {
                    assert!(run.glyphs().all(|g| g.id != 0));
                }
            }
        }
        let layout = AccessibleLayout::new(&lines);
        assert_eq!(layout.lines()[0].word_starts.len(), count);
        let hash = |out: &types::TreeUpdate| {
            format!("{:x}", Sha256::digest(format!("{out:?}").as_bytes()))
        };
        let expected = hash(&export(&layout));
        for sample in 0..10 {
            #[cfg(feature = "allocation-counting")]
            let scope = ALLOC.begin().unwrap();
            #[cfg(not(feature = "allocation-counting"))]
            let start = std::time::Instant::now();
            let out = std::hint::black_box(export(std::hint::black_box(&layout)));
            #[cfg(feature = "allocation-counting")]
            let counts = scope.finish();
            #[cfg(not(feature = "allocation-counting"))]
            let elapsed = start.elapsed().as_nanos();
            #[cfg(feature = "allocation-counting")]
            let measurement = json!({"allocation": counts});
            #[cfg(not(feature = "allocation-counting"))]
            let measurement = json!({"ns": elapsed});
            assert_eq!(hash(&out), expected);
            println!(
                "{}",
                json!({"count":count, "separate":separate, "sample":sample, "digest":expected, "measurement":measurement})
            );
        }
    }
}
