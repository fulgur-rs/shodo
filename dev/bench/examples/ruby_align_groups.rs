//! Fixed-font merged ruby alignment probe for group counting and gap storage.
//! Time and allocation runs share the exact input builder and geometry digest.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::geometry::{Direction, WritingMode};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::ruby::{
    Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubyMerge, RubyPosition,
    RubySpan, RubyStyle, RubyVisibility,
};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, TextCombineUpright};
use shodo::{AtomicSizes, LayoutContext, Line, Paragraph, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

#[derive(Clone, Copy)]
enum ContentKind {
    Plain,
    SharedCluster,
    Tcy,
    Nested,
    Bidi,
}

impl ContentKind {
    fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::SharedCluster => "shared-cluster",
            Self::Tcy => "tcy",
            Self::Nested => "nested-ruby",
            Self::Bidi => "bidi",
        }
    }

    fn reading_family(self) -> &'static str {
        match self {
            Self::SharedCluster => "Shodo Fixture Latin",
            Self::Tcy => "Shodo Fixture Latin",
            Self::Bidi => "Shodo Fixture Arabic",
            Self::Plain | Self::Nested => "Shodo Fixture CJK",
        }
    }

    fn direction(self) -> Direction {
        if matches!(self, Self::Bidi) {
            Direction::Rtl
        } else {
            Direction::Ltr
        }
    }

    fn writing_mode(self) -> WritingMode {
        if matches!(self, Self::Tcy) {
            WritingMode::VerticalRl
        } else {
            WritingMode::HorizontalTb
        }
    }
}

fn inline_style(size: f32, family: &str) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named(family.into())],
        ..Default::default()
    }
}

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            direction: style.direction,
            root: style.clone(),
            ..Default::default()
        },
        limits,
    );
    builder.with_offset_mapping(true);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 11,
        },
        value,
    );
    RubyContent::from_builder(builder)
}

fn content(kind: ContentKind, node: u64, style: &InlineStyle, limits: &Limits) -> RubyContent {
    match kind {
        ContentKind::Plain => text(node, "にほ", style, limits),
        ContentKind::SharedCluster => {
            let mut builder = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style.clone(),
                    ..Default::default()
                },
                limits,
            );
            builder.with_offset_mapping(true);
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(node),
                    offset: 11,
                },
                "e",
            );
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(node + 1),
                    offset: 30,
                },
                "\u{301}",
            );
            RubyContent::from_builder(builder)
        }
        ContentKind::Tcy => {
            let mut tcy_style = style.clone();
            tcy_style.text_combine_upright = TextCombineUpright::All;
            let mut builder = ParagraphBuilder::new(
                &ParagraphStyle {
                    writing_mode: WritingMode::VerticalRl,
                    root: tcy_style.clone(),
                    ..Default::default()
                },
                limits,
            );
            builder.with_offset_mapping(true);
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(node),
                    offset: 11,
                },
                "12",
            );
            RubyContent::from_builder(builder)
        }
        ContentKind::Nested => {
            let nested = Ruby::new(
                vec![RubyBase {
                    node: NodeId(node + 2),
                    content: text(node + 3, "に", style, limits),
                    align: RubyAlign::default(),
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(node + 4),
                        content: text(
                            node + 5,
                            "ni",
                            &inline_style(6.0, "Shodo Fixture Latin"),
                            limits,
                        ),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            let mut builder = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style.clone(),
                    ..Default::default()
                },
                limits,
            );
            builder.with_offset_mapping(true);
            builder.push_ruby(NodeId(node + 6), style, nested);
            RubyContent::from_builder(builder)
        }
        ContentKind::Bidi => text(node, "سلامa3", style, limits),
    }
}

fn paragraph(
    kind: ContentKind,
    merged: bool,
    columns: usize,
    limits: &Limits,
    fonts: &shodo::font::FontCollection,
) -> Paragraph {
    let base_style = inline_style(24.0, "Shodo Fixture CJK");
    let mut reading_style = inline_style(6.0, kind.reading_family());
    reading_style.direction = kind.direction();
    if matches!(kind, ContentKind::Tcy) {
        reading_style.text_combine_upright = TextCombineUpright::All;
    }
    let bases = (0..columns)
        .map(|column| RubyBase {
            node: NodeId(1_000 + column as u64),
            content: text(2_000 + column as u64, "日", &base_style, limits),
            align: RubyAlign::default(),
        })
        .collect();
    let annotations = (0..columns)
        .map(|column| {
            let node = 10_000 + column as u64 * 8;
            RubyAnnotation {
                node: NodeId(node),
                content: content(kind, node + 1, &reading_style, limits),
                span: RubySpan::Columns(column..column + 1),
                visibility: RubyVisibility::Visible,
            }
        })
        .collect();
    let ruby = Ruby::new(
        bases,
        vec![RubyLevel {
            annotations,
            style: RubyStyle {
                align: RubyAlign::SpaceAround,
                merge: if merged {
                    RubyMerge::Merge
                } else {
                    RubyMerge::Separate
                },
                position: RubyPosition::Over,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: kind.writing_mode(),
            direction: kind.direction(),
            root: base_style.clone(),
            ..Default::default()
        },
        limits,
    );
    builder.with_offset_mapping(true);
    builder.push_ruby(NodeId(8), &base_style, ruby);
    builder.build(&mut LayoutContext::new(), fonts).unwrap()
}

fn layout(paragraph: &Paragraph) -> (Vec<Line>, LayoutContext) {
    let mut context = LayoutContext::new();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        100_000.0,
        &AtomicSizes::EMPTY,
    );
    (lines, context)
}

fn normalize_font_layer_id(value: &mut Value) {
    match value {
        Value::String(text) => {
            let marker = "FontId { layer: ";
            let index_marker = ", index:";
            let mut rest = text.as_str();
            let mut normalized = String::with_capacity(text.len());
            while let Some(start) = rest.find(marker) {
                normalized.push_str(&rest[..start + marker.len()]);
                let after_layer = &rest[start + marker.len()..];
                let Some(index) = after_layer.find(index_marker) else {
                    normalized.push_str(&rest[start + marker.len()..]);
                    rest = "";
                    break;
                };
                normalized.push_str("<resource>");
                normalized.push_str(index_marker);
                rest = &after_layer[index + index_marker.len()..];
            }
            normalized.push_str(rest);
            *text = normalized;
        }
        Value::Array(values) => values.iter_mut().for_each(normalize_font_layer_id),
        Value::Object(values) => values.values_mut().for_each(normalize_font_layer_id),
        _ => {}
    }
}

fn snapshot(lines: &[Line]) -> (String, Value) {
    let mut output = Value::Array(lines.iter().map(snapshot::line).collect());
    normalize_font_layer_id(&mut output);
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    (digest, output)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    assert!(matches!(mode.as_str(), "time" | "alloc"));
    #[cfg(not(feature = "allocation-counting"))]
    assert_ne!(mode, "alloc", "alloc mode requires allocation-counting");

    let samples = env_usize("SHODO_RUBY_ALIGN_SAMPLES", 9);
    let iterations = env_usize("SHODO_RUBY_ALIGN_ITERATIONS", 16);
    let columns = env_usize("SHODO_RUBY_ALIGN_COLUMNS", 16);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();

    for kind in [
        ContentKind::Plain,
        ContentKind::SharedCluster,
        ContentKind::Tcy,
        ContentKind::Nested,
        ContentKind::Bidi,
    ] {
        for merged in [false, true] {
            let name = format!(
                "{}-{}",
                kind.name(),
                if merged { "merged" } else { "separate" }
            );
            let paragraph = paragraph(kind, merged, columns, &limits, &fonts.collection);
            let build_warnings = format!("{:?}", paragraph.warnings());
            let (warm, mut warm_context) = layout(&paragraph);
            assert_eq!(warm.len(), 1, "{name}");
            let layout_warnings = format!("{:?}", warm_context.take_warnings());
            let (expected_digest, _) = snapshot(&warm);
            drop(warm);
            drop(warm_context);

            let median_ns = if mode == "time" {
                let mut timing_samples = Vec::with_capacity(samples);
                for _ in 0..samples {
                    let started = Instant::now();
                    for _ in 0..iterations {
                        let (lines, context) = layout(black_box(&paragraph));
                        black_box((&lines, &context));
                    }
                    timing_samples.push(started.elapsed().as_nanos() as u64 / iterations as u64);
                }
                timing_samples.sort_unstable();
                Some(timing_samples[timing_samples.len() / 2])
            } else {
                None
            };

            let (allocation, output_digest) = {
                #[cfg(feature = "allocation-counting")]
                {
                    let scope = ALLOC.begin().unwrap();
                    let (lines, mut context) = layout(&paragraph);
                    let allocation = scope.finish();
                    assert_eq!(format!("{:?}", context.take_warnings()), layout_warnings);
                    let (digest, _) = snapshot(&lines);
                    (json!(allocation), digest)
                }
                #[cfg(not(feature = "allocation-counting"))]
                {
                    let (lines, mut context) = layout(&paragraph);
                    assert_eq!(format!("{:?}", context.take_warnings()), layout_warnings);
                    let (digest, _) = snapshot(&lines);
                    (Value::Null, digest)
                }
            };
            assert_eq!(output_digest, expected_digest, "{name}");
            println!(
                "{}",
                json!({
                    "case": name,
                    "columns": columns,
                    "merged_lanes": if merged { columns } else { 0 },
                    "count_requests_per_layout": if merged { columns } else { 0 },
                    "mode": mode,
                    "iterations_per_sample": iterations,
                    "median_ns_per_layout": median_ns,
                    "allocation": allocation,
                    "output_sha256": output_digest,
                    "build_warnings": build_warnings,
                    "layout_warnings": layout_warnings,
                })
            );
        }
    }
}
