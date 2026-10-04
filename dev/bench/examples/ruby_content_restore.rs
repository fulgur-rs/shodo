//! Fixed-font build-only probe for restored Ruby annotation snapshots.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    LayoutContext, ParagraphBuilder, Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel,
    RubySpan,
};

const ANNOTATIONS: usize = 16;
const REPEATS: usize = 32;

#[derive(Clone, Copy)]
enum Shape {
    Length,
    Depth,
    Styles,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Self::Length => "input_length",
            Self::Depth => "nested_depth",
            Self::Styles => "style_count",
        }
    }
}

fn source(node: u64) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset: 0,
    }
}

fn reading(shape: Shape, size: usize, root: &InlineStyle, limits: &Limits) -> RubyContent {
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: root.clone(),
            ..ParagraphStyle::default()
        },
        limits,
    );
    match shape {
        Shape::Length => {
            for index in 0..size {
                builder
                    .open_inline(NodeId(1_000 + index as u64), root, InlineEdges::default())
                    .close_inline();
            }
        }
        Shape::Depth => {
            for index in 0..size {
                builder.open_inline(NodeId(2_000 + index as u64), root, InlineEdges::default());
            }
            for _ in 0..size {
                builder.close_inline();
            }
        }
        Shape::Styles => {
            for index in 0..size {
                let style = InlineStyle {
                    font_size: root.font_size + index as f32 + 1.0,
                    ..root.clone()
                };
                builder
                    .open_inline(NodeId(3_000 + index as u64), &style, InlineEdges::default())
                    .close_inline();
            }
        }
    }
    builder.push_text(source(4_000), "a");
    RubyContent::from_builder(builder)
}

fn fixture(shape: Shape, size: usize, limits: &Limits) -> ParagraphBuilder {
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        font_size: 12.0,
        ..InlineStyle::default()
    };
    let content = reading(shape, size, &root, limits);
    let bases = (0..ANNOTATIONS)
        .map(|index| RubyBase {
            node: NodeId(10_000 + index as u64),
            content: RubyContent::text(source(20_000 + index as u64), "日", &root, limits),
            align: Default::default(),
        })
        .collect();
    let annotations = (0..ANNOTATIONS)
        .map(|index| RubyAnnotation {
            node: NodeId(30_000 + index as u64),
            content: content.clone(),
            span: RubySpan::Columns(index..index + 1),
            visibility: Default::default(),
        })
        .collect();
    let ruby = Ruby::new(
        bases,
        vec![RubyLevel {
            annotations,
            style: Default::default(),
        }],
    )
    .unwrap();
    let style = ParagraphStyle {
        root: root.clone(),
        ..ParagraphStyle::default()
    };
    let mut builder = ParagraphBuilder::new(&style, limits);
    builder.push_ruby(NodeId(9_000), &root, ruby);
    builder
}

fn main() {
    let sample: usize = std::env::args()
        .nth(1)
        .expect("provide an integer sample index")
        .parse()
        .expect("sample index must be an integer");
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let cases = [
        (Shape::Length, 8),
        (Shape::Length, 128),
        (Shape::Length, 512),
        (Shape::Depth, 1),
        (Shape::Depth, 8),
        (Shape::Depth, 32),
        (Shape::Styles, 1),
        (Shape::Styles, 8),
        (Shape::Styles, 32),
    ];

    for (shape, size) in cases {
        let warmup = fixture(shape, size, &limits);
        warmup
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();

        let builds: Vec<_> = (0..REPEATS)
            .map(|_| (fixture(shape, size, &limits), LayoutContext::new()))
            .collect();
        let start = std::time::Instant::now();
        let mut outputs = Vec::with_capacity(REPEATS);
        for (builder, mut context) in builds {
            let paragraph =
                std::hint::black_box(builder.build(&mut context, &fonts.collection).unwrap());
            outputs.push((paragraph, context));
        }
        let elapsed_ns = start.elapsed().as_nanos();

        let build_warnings: Vec<_> = outputs
            .iter()
            .map(|(paragraph, _)| format!("{:?}", paragraph.warnings()))
            .collect();
        assert!(build_warnings.windows(2).all(|pair| pair[0] == pair[1]));
        let build_warnings = build_warnings.first().cloned().unwrap_or_default();
        let mut output = Vec::new();
        let mut layout_warnings = Vec::new();
        for (paragraph, context) in &mut outputs {
            let lines = paragraph.break_all(
                context,
                &Default::default(),
                160.0,
                &shodo::AtomicSizes::EMPTY,
            );
            output.push(lines.iter().map(snapshot::line).collect::<Vec<_>>());
            layout_warnings.push(format!("{:?}", context.take_warnings()));
        }
        assert!(layout_warnings.windows(2).all(|pair| pair[0] == pair[1]));
        let layout_warnings = layout_warnings.first().cloned().unwrap_or_default();
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
        let raw_items = 2 * size + 1;
        let styles = if matches!(shape, Shape::Styles) {
            size + 1
        } else {
            1
        };
        println!(
            "{}",
            serde_json::json!({
                "shape": shape.name(),
                "size": size,
                "sample": sample,
                "raw_items_per_restored_input": raw_items,
                "style_entries_per_restored_input": styles,
                "restored_inputs_per_build": ANNOTATIONS,
                "ns_per_build": elapsed_ns / REPEATS as u128,
                "output_sha256": digest,
                "build_warnings": build_warnings,
                "layout_warnings": layout_warnings,
            })
        );
    }
}
