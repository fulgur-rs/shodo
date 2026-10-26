//! Append-only timing probe. Run with --release; snapshot and parent setup are
//! outside timing, while output materialization remains inside it.
use std::hint::black_box;
use std::time::Instant;

use shodo::ParagraphBuilder;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId};
use shodo::ruby::{Ruby, RubyBase, RubyContent};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};

fn snapshot(bytes: usize) -> RubyContent {
    let paragraph = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(bytes))],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut source = ParagraphBuilder::new(&paragraph, &Limits::default());
    for i in 0..8 {
        source
            .open_inline(
                NodeId(i),
                &InlineStyle {
                    font_size: 20.0 + i as f32,
                    ..Default::default()
                },
                InlineEdges::default(),
            )
            .close_inline();
    }
    assert_eq!(source.error(), None);
    RubyContent::from_builder(source)
}

fn main() {
    if cfg!(debug_assertions) {
        panic!("run with --release");
    }
    let style = InlineStyle::default();
    for bytes in [16, 4096] {
        for (occurrences, unique) in [(1, 1), (16, 1), (128, 1), (128, 8), (128, 128)] {
            let snapshots: Vec<_> = (0..unique).map(|_| snapshot(bytes)).collect();
            let mut samples = Vec::new();
            for sample in 0..35 {
                let input = Ruby::new(
                    (0..occurrences)
                        .map(|i| RubyBase {
                            node: NodeId(100 + i as u64),
                            content: snapshots[i % unique].clone(),
                            align: Default::default(),
                        })
                        .collect(),
                    vec![],
                )
                .unwrap();
                let mut parent =
                    ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
                let start = Instant::now();
                parent.push_ruby(NodeId(1000), black_box(&style), black_box(input));
                let elapsed = start.elapsed().as_nanos();
                assert_eq!(parent.error(), None);
                black_box(&parent);
                if sample >= 4 {
                    samples.push(elapsed);
                }
            }
            samples.sort_unstable();
            println!(
                "{}",
                serde_json::json!({
                    "occurrences": occurrences, "unique_snapshots": unique,
                    "family_bytes": bytes, "source_styles": 9, "samples": samples.len(),
                    "median_ns": samples[samples.len() / 2],
                    "p25_ns": samples[samples.len() / 4],
                    "p75_ns": samples[samples.len() * 3 / 4],
                })
            );
        }
    }
}
