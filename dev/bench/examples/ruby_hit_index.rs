//! Fixed-font ruby hit index probe; paragraph layout stays outside query scopes.
use serde_json::{Value, json};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{NodeId, TextSource};
use shodo::ruby::{
    Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, Line, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        value,
        style,
        limits,
    )
}

fn lines(count: usize) -> Vec<Line> {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Ruby Hit Fixture".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = InlineStyle {
        font_size: 20.0,
        font_families: vec![FontFamily::Named("Shodo Ruby Hit Fixture".into())],
        ..Default::default()
    };
    let reading = InlineStyle {
        font_size: 8.0,
        ..root.clone()
    };
    let bases = (0..count)
        .map(|i| RubyBase {
            node: NodeId(100 + i as u64),
            content: text(1_000 + i as u64, "a", &root, &limits),
            align: Default::default(),
        })
        .collect();
    let annotations = (0..count)
        .map(|i| RubyAnnotation {
            node: NodeId(2_000 + i as u64),
            content: text(3_000 + i as u64, "b", &reading, &limits),
            span: RubySpan::Columns(i..i + 1),
            visibility: RubyVisibility::Visible,
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
    let style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.with_offset_mapping(true);
    builder.push_ruby(NodeId(9_999), &root, ruby);
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, &fonts).unwrap();
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        1_000_000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].ruby_annotations().len(), count);
    lines
}

fn body_point(line: &Line) -> (f32, f32) {
    let layout = LineLayout::new(std::slice::from_ref(line));
    let caret = layout
        .caret(TextPosition {
            line: 0,
            offset: line.text_range().start as u32,
            affinity: Affinity::Downstream,
        })
        .unwrap();
    (
        line.inline_size() / 2.0,
        caret.rect.block_start + caret.rect.block_size / 2.0,
    )
}

fn ruby_point(line: &Line) -> (f32, f32) {
    let annotation = line.ruby_annotations().next().unwrap();
    let point = body_point(annotation.line());
    let transform = annotation.transform();
    (
        transform.inline_inline * point.0
            + transform.inline_block * point.1
            + transform.inline_offset,
        transform.block_inline * point.0
            + transform.block_block * point.1
            + transform.block_offset
            + line.block_offset(),
    )
}

fn time_samples(iterations: usize, mut query: impl FnMut()) -> Vec<u64> {
    (0..7)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iterations {
                query();
            }
            start.elapsed().as_nanos() as u64
        })
        .collect()
}

fn median(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[cfg(feature = "allocation-counting")]
fn allocation_cost(query: impl FnOnce()) -> Value {
    let scope = ALLOC.begin().unwrap();
    query();
    json!({ "allocation": scope.finish() })
}

#[cfg(not(feature = "allocation-counting"))]
fn allocation_cost(_: impl FnOnce()) -> Value {
    Value::Null
}

fn main() {
    let mut rows = Vec::new();
    for count in [16, 64, 256, 1024] {
        let lines = lines(count);
        let build_iterations = 16;
        let build_samples = time_samples(build_iterations, || {
            let layout = LineLayout::new(&lines);
            black_box(&layout);
        });
        #[cfg(feature = "allocation-counting")]
        let (layout, build_allocation) = {
            let scope = ALLOC.begin().unwrap();
            let layout = LineLayout::new(&lines);
            let allocation = json!(scope.finish());
            (layout, allocation)
        };
        #[cfg(not(feature = "allocation-counting"))]
        let (layout, build_allocation) = (LineLayout::new(&lines), Value::Null);
        let miss = (1_000_000.0, 1_000_000.0);
        let ruby = ruby_point(&lines[0]);
        let body = body_point(&lines[0]);
        assert!(layout.hit_test_ruby(ruby.0, ruby.1).is_some());
        assert!(layout.hit_test_ruby(miss.0, miss.1).is_none());
        assert!(layout.hit_test(body.0, body.1).unwrap().inside);
        assert!(!layout.hit_test(miss.0, miss.1).unwrap().inside);

        let iterations = 2_000;
        let ruby_hit = time_samples(iterations, || {
            black_box(layout.hit_test_ruby(black_box(ruby.0), black_box(ruby.1)));
        });
        let ruby_miss = time_samples(iterations, || {
            black_box(layout.hit_test_ruby(black_box(miss.0), black_box(miss.1)));
        });
        let body_hit = time_samples(iterations, || {
            black_box(layout.hit_test(black_box(body.0), black_box(body.1)));
        });
        let main_miss = time_samples(iterations, || {
            black_box(layout.hit_test(black_box(miss.0), black_box(miss.1)));
        });
        let ruby_hit_alloc = allocation_cost(|| {
            for _ in 0..iterations {
                black_box(layout.hit_test_ruby(ruby.0, ruby.1));
            }
        });
        let ruby_miss_alloc = allocation_cost(|| {
            for _ in 0..iterations {
                black_box(layout.hit_test_ruby(miss.0, miss.1));
            }
        });
        let body_hit_alloc = allocation_cost(|| {
            for _ in 0..iterations {
                black_box(layout.hit_test(body.0, body.1));
            }
        });
        rows.push(json!({
            "annotations": count,
            "layout_build_iterations_per_sample": build_iterations,
            "iterations_per_sample": iterations,
            "layout_build_ns_samples": build_samples,
            "layout_build_ns_median": median(&build_samples) / build_iterations as u64,
            "ruby_hit_ns_samples": ruby_hit,
            "ruby_hit_ns_per_query_median": median(&ruby_hit) / iterations as u64,
            "ruby_miss_ns_samples": ruby_miss,
            "ruby_miss_ns_per_query_median": median(&ruby_miss) / iterations as u64,
            "body_hit_ns_samples": body_hit,
            "body_hit_ns_per_query_median": median(&body_hit) / iterations as u64,
            "main_miss_ns_samples": main_miss,
            "main_miss_ns_per_query_median": median(&main_miss) / iterations as u64,
            "ruby_hit_allocation": ruby_hit_alloc,
            "ruby_miss_allocation": ruby_miss_alloc,
            "body_hit_allocation": body_hit_alloc,
            "layout_build_allocation": build_allocation,
        }));
    }
    println!("{}", serde_json::to_string(&rows).unwrap());
}
