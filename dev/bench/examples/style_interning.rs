//! Compare builder recording and complete builds with prepared style inputs.
//! Use separate release builds for `time` and `allocation-counting` / `alloc`.
use serde_json::json;
use sha2::{Digest as _, Sha256};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use shodo_bench::{Run, digest};
use std::hint::black_box;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

struct Input {
    paragraph: ParagraphStyle,
    styles: Vec<InlineStyle>,
    first: Vec<InlineStyle>,
    spans: usize,
}

impl Input {
    fn new(count: usize, spans: usize, first_line: bool) -> Self {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named(
                shodo_fixtures::font("latin").unwrap().family.into(),
            )],
            ..Default::default()
        };
        let styles: Vec<_> = (0..count)
            .map(|i| InlineStyle {
                paint: shodo::style::PaintStyle {
                    color: [i as u8, 17, 23, 255],
                    ..Default::default()
                },
                ..root.clone()
            })
            .collect();
        let first = if first_line {
            styles
                .iter()
                .map(|style| InlineStyle {
                    font_size: 18.0,
                    ..style.clone()
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            paragraph: ParagraphStyle {
                root,
                ..Default::default()
            },
            styles,
            first,
            spans,
        }
    }

    fn record(&self, limits: &Limits) -> ParagraphBuilder {
        let mut builder = ParagraphBuilder::new(&self.paragraph, limits);
        if self.styles.is_empty() {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(0),
                    offset: 0,
                },
                "a",
            );
        } else {
            for i in 0..self.spans {
                let index = i % self.styles.len();
                let node = NodeId(i as u64);
                if self.first.is_empty() {
                    builder.open_inline(node, &self.styles[index], InlineEdges::default());
                } else {
                    builder.open_inline_with_first_line(
                        node,
                        &self.styles[index],
                        &self.first[index],
                        InlineEdges::default(),
                    );
                }
                builder
                    .push_text(TextSource::Dom { node, offset: 0 }, "a")
                    .close_inline();
            }
        }
        assert_eq!(builder.error(), None);
        builder
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    assert_eq!(
        mode,
        if cfg!(feature = "allocation-counting") {
            "alloc"
        } else {
            "time"
        }
    );
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    for (case, input) in [
        ("root", Input::new(0, 0, false)),
        ("consecutive-1x512", Input::new(1, 512, false)),
        ("reuse-16x512", Input::new(16, 512, false)),
        ("unique-64", Input::new(64, 64, false)),
        ("first-line-16x512", Input::new(16, 512, true)),
    ] {
        let mut cx = LayoutContext::new();
        let snapshot = |cx: &mut LayoutContext| {
            let paragraph = input.record(&limits).build(cx, &fonts.collection).unwrap();
            assert!(paragraph.warnings().is_empty());
            let run = Run {
                lines: paragraph.break_all(cx, &LineOptions::default(), 320.0, &AtomicSizes::EMPTY),
                float_reports: 0,
                height_retries: 0,
                intrinsics: Vec::new(),
            };
            assert!(cx.take_warnings().is_empty());
            let geometry = digest(&run, &fonts).unwrap();
            assert_eq!(geometry.synthetic_glyphs, 0);
            let paint = format!(
                "{:x}",
                Sha256::digest(
                    format!(
                        "{:?}",
                        run.lines
                            .iter()
                            .map(|line| line.paint_spans())
                            .collect::<Vec<_>>()
                    )
                    .as_bytes(),
                )
            );
            json!({"geometry": geometry, "paint_sha256": paint})
        };
        let expected = snapshot(&mut cx);
        for op in ["record", "build"] {
            let iterations = if mode == "alloc" {
                1
            } else if case == "root" {
                5_000
            } else {
                50
            };
            let mut execute = || {
                let builder = black_box(&input).record(black_box(&limits));
                if op == "build" {
                    black_box(builder.build(&mut cx, &fonts.collection).unwrap());
                } else {
                    black_box(builder);
                }
            };
            for _ in 0..3 {
                execute();
            }
            for sample in 0..9 {
                #[cfg(feature = "allocation-counting")]
                let measurement = {
                    let scope = ALLOC.begin().unwrap();
                    for _ in 0..iterations {
                        execute();
                    }
                    json!({"allocation": scope.finish()})
                };
                #[cfg(not(feature = "allocation-counting"))]
                let measurement = {
                    let start = std::time::Instant::now();
                    for _ in 0..iterations {
                        execute();
                    }
                    json!({"ns": start.elapsed().as_nanos()})
                };
                println!(
                    "{}",
                    json!({"case": case, "op": op, "mode": mode, "sample": sample,
                    "iterations": iterations, "measurement": measurement, "output": expected})
                );
            }
        }
        assert_eq!(snapshot(&mut cx), expected);
    }
}
