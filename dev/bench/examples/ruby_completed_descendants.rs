//! Fixed-font nested-ruby measurement; input construction and output hashing
//! are outside the timed and allocation-counted layout scope.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineBreak, ParagraphStyle};
use shodo::{
    AtomicSizes, LayoutContext, ParagraphBuilder, Ruby, RubyAnnotation, RubyBase, RubyContent,
    RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const WARMUPS: usize = 3;
const SAMPLES: usize = 11;

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    base_depth: usize,
    annotation_depth: usize,
    columns: usize,
    annotation_chars: usize,
    inline_width: f32,
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (depth, name) in [
        (0, "base-depth-0"),
        (1, "base-depth-1"),
        (2, "base-depth-2"),
        (4, "base-depth-4"),
    ] {
        cases.push(Case {
            name,
            base_depth: depth,
            annotation_depth: 0,
            columns: 8,
            annotation_chars: 3,
            inline_width: 96.0,
        });
    }
    for (depth, name) in [
        (0, "annotation-depth-0"),
        (1, "annotation-depth-1"),
        (2, "annotation-depth-2"),
        (4, "annotation-depth-4"),
    ] {
        cases.push(Case {
            name,
            base_depth: 0,
            annotation_depth: depth,
            columns: 4,
            annotation_chars: 3,
            inline_width: 96.0,
        });
    }
    for (columns, name) in [
        (1, "columns-1"),
        (4, "columns-4"),
        (16, "columns-16"),
        (64, "columns-64"),
    ] {
        cases.push(Case {
            name,
            base_depth: 2,
            annotation_depth: 0,
            columns,
            annotation_chars: 3,
            inline_width: 96.0,
        });
    }
    for (name, annotation_chars, inline_width) in [
        ("continuation-short", 3, 48.0),
        ("continuation-medium", 18, 72.0),
        ("continuation-long", 48, 96.0),
    ] {
        cases.push(Case {
            name,
            base_depth: 1,
            annotation_depth: 1,
            columns: 8,
            annotation_chars,
            inline_width,
        });
    }
    cases
}

fn style(size: f32, family: &str) -> InlineStyle {
    InlineStyle {
        font_size: size,
        line_break: LineBreak::Anywhere,
        font_families: vec![FontFamily::Named(family.into())],
        ..Default::default()
    }
}

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 3,
        },
        value,
        style,
        limits,
    )
}

fn wrap_ruby(
    content: RubyContent,
    reading: RubyContent,
    id: u64,
    base_style: &InlineStyle,
    limits: &Limits,
) -> RubyContent {
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(id + 1),
            content,
            align: Default::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(id + 2),
                content: reading,
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: base_style.clone(),
            ..Default::default()
        },
        limits,
    );
    builder.push_ruby(NodeId(id), base_style, ruby);
    RubyContent::from_builder(builder)
}

fn nested_content(
    depth: usize,
    chars: usize,
    id: u64,
    base_style: &InlineStyle,
    reading_style: &InlineStyle,
    limits: &Limits,
) -> RubyContent {
    let mut content = text(id + 1, &"日".repeat(chars), base_style, limits);
    for layer in 0..depth {
        let layer_id = id + 10 + layer as u64 * 10;
        let reading = text(layer_id + 2, &"にほん".repeat(chars), reading_style, limits);
        content = wrap_ruby(content, reading, layer_id, base_style, limits);
    }
    content
}

fn builder(case: Case, limits: &Limits, family: &str) -> ParagraphBuilder {
    let root = style(24.0, family);
    let reading_style = style(8.0, family);
    let bases = (0..case.columns)
        .map(|column| {
            let id = 1_000 + column as u64 * 1_000;
            RubyBase {
                node: NodeId(id),
                content: nested_content(case.base_depth, 1, id + 10, &root, &reading_style, limits),
                align: Default::default(),
            }
        })
        .collect();
    let annotations = (0..case.columns)
        .map(|column| {
            let id = 100_000 + column as u64 * 1_000;
            let mut content = text(
                id + 1,
                &"に".repeat(case.annotation_chars),
                &reading_style,
                limits,
            );
            for layer in 0..case.annotation_depth {
                let layer_id = id + 10 + layer as u64 * 10;
                let reading = text(
                    layer_id + 2,
                    &"ほん".repeat(case.annotation_chars),
                    &reading_style,
                    limits,
                );
                content = wrap_ruby(content, reading, layer_id, &reading_style, limits);
            }
            RubyAnnotation {
                node: NodeId(id),
                content,
                span: RubySpan::Columns(column..column + 1),
                visibility: RubyVisibility::Visible,
            }
        })
        .collect();
    let ruby = Ruby::new(
        bases,
        vec![RubyLevel {
            annotations,
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: root.clone(),
            ..Default::default()
        },
        limits,
    );
    builder.push_ruby(NodeId(8), &root, ruby);
    builder
}

fn limit_result(case: Case, family: &str) -> String {
    let limits = Limits {
        max_shaped_glyphs: Some(1),
        ..Default::default()
    };
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    match builder(case, &limits, family).build(&mut LayoutContext::new(), &fonts.collection) {
        Ok(_) => panic!("the constrained workload should exceed its glyph limit"),
        Err(error) => format!("{error:?}"),
    }
}

fn digest(lines: &[shodo::Line]) -> String {
    let output: Value = json!(lines.iter().map(snapshot::line).collect::<Vec<_>>());
    format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()))
}

fn median(values: &mut [u64]) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "time".into());
    #[cfg(not(feature = "allocation-counting"))]
    assert_ne!(
        mode, "alloc",
        "alloc mode requires --features allocation-counting"
    );
    assert!(
        matches!(mode.as_str(), "time" | "alloc"),
        "mode must be time or alloc"
    );

    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let family = shodo_fixtures::FONTS[1].family;
    for case in cases() {
        let rejected = limit_result(case, family);
        let mut expected = None;
        let mut times = Vec::new();
        let mut allocations = Vec::new();
        for iteration in 0..WARMUPS + SAMPLES {
            let input = builder(case, &limits, family);
            let mut cx = LayoutContext::new();
            #[cfg(feature = "allocation-counting")]
            let allocation_scope = (mode == "alloc").then(|| ALLOC.begin().unwrap());
            let started = (mode == "time").then(std::time::Instant::now);
            let paragraph = input.build(&mut cx, &fonts.collection).unwrap();
            let lines = paragraph.break_all(
                &mut cx,
                &Default::default(),
                case.inline_width,
                &AtomicSizes::EMPTY,
            );
            let elapsed = started.map(|start| start.elapsed().as_nanos() as u64);
            #[cfg(feature = "allocation-counting")]
            let allocation = allocation_scope.map(|scope| scope.finish());

            let build_warnings = format!("{:?}", paragraph.warnings());
            let layout_warnings = format!("{:?}", cx.take_warnings());
            let output_digest = digest(&lines);
            let contract = (
                output_digest.clone(),
                build_warnings.clone(),
                layout_warnings.clone(),
            );
            if let Some(expected) = &expected {
                assert_eq!(&contract, expected, "unstable output in {}", case.name);
            } else {
                expected = Some(contract);
            }
            if iteration < WARMUPS {
                continue;
            }
            let sample = iteration - WARMUPS;
            let measurement = if mode == "time" {
                let ns = elapsed.unwrap();
                times.push(ns);
                json!({"time_ns": ns})
            } else {
                #[cfg(feature = "allocation-counting")]
                {
                    let counts = allocation.unwrap();
                    allocations.push(counts.allocated_bytes);
                    json!({"allocation": counts})
                }
                #[cfg(not(feature = "allocation-counting"))]
                unreachable!()
            };
            println!(
                "{}",
                json!({
                    "case": case.name,
                    "base_depth": case.base_depth,
                    "annotation_depth": case.annotation_depth,
                    "columns": case.columns,
                    "annotation_chars": case.annotation_chars,
                    "inline_width": case.inline_width,
                    "line_count": lines.len(),
                    "sample": sample,
                    "output_sha256": output_digest,
                    "build_warnings": build_warnings,
                    "layout_warnings": layout_warnings,
                    "limit_rejection": rejected,
                    "measurement": measurement,
                })
            );
        }
        if mode == "time" {
            let med = median(&mut times);
            eprintln!("{} median_ns={med}", case.name);
        } else {
            let med = median(&mut allocations);
            eprintln!("{} median_allocated_bytes={med}", case.name);
        }
    }
}
