//! Fixed ch-heavy caller workload for before/after font-unit measurements.
//! Run `cargo run -p shodo-bench --release --example ch_cost -- time` or
//! add `--features allocation-counting` and pass `alloc` for requested bytes.
use shodo::font::{FontCollection, FontQuery};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const SPANS: usize = 96;
const WIDTH: f32 = 800.0;
const WARMUP: usize = 32;
const SAMPLES: usize = 21;
const REPEATS: usize = 8;

struct Spec {
    text: String,
    ch_query: FontQuery,
    ic_query: FontQuery,
    size: f32,
    family: FontFamily,
}

#[derive(Clone, Copy)]
struct Values {
    start: f32,
    end: f32,
    padding: f32,
    spacing: f32,
}

fn specs() -> Vec<Spec> {
    (0..SPANS)
        .map(|i| {
            let family = FontFamily::Named(
                if i % 2 == 0 {
                    "Shodo Fixture Latin"
                } else {
                    "Shodo Fixture CJK"
                }
                .into(),
            );
            let ch_query = FontQuery {
                families: vec![family.clone()],
                ..Default::default()
            };
            let ic_query = FontQuery {
                families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
                script: *b"Hani",
                ..Default::default()
            };
            Spec {
                text: format!("item{i:03} 00 水 "),
                ch_query,
                ic_query,
                size: [16.0, 18.0, 20.0, 22.0][i % 4],
                family,
            }
        })
        .collect()
}

// Exactly three resolve_ch and one resolve_ic per span; these are the same
// style resolutions consumed by `full_layout` below.
fn resolve(fonts: &FontCollection, spec: &Spec) -> Values {
    let ch_start = fonts.resolve_ch(&spec.ch_query, spec.size);
    let ch_end = fonts.resolve_ch(&spec.ch_query, spec.size);
    let ic_padding = fonts.resolve_ic(&spec.ic_query, spec.size);
    let ch_spacing = fonts.resolve_ch(&spec.ch_query, spec.size);
    assert!(ch_start.id.is_some() && ch_end.id.is_some());
    assert!(ic_padding.id.is_some() && ch_spacing.id.is_some());
    Values {
        start: ch_start.advance * 0.5,
        end: ch_end.advance * 0.25,
        padding: ic_padding.advance * 0.25,
        spacing: ch_spacing.advance * 0.125,
    }
}

fn unit_batch(fonts: &FontCollection, specs: &[Spec]) -> f32 {
    specs
        .iter()
        .map(|spec| {
            let v = resolve(fonts, spec);
            v.start + v.end + v.padding + v.spacing
        })
        .sum()
}

fn ch_batch(fonts: &FontCollection, specs: &[Spec]) -> f32 {
    specs
        .iter()
        .map(|spec| {
            (0..3)
                .map(|_| fonts.resolve_ch(&spec.ch_query, spec.size).advance)
                .sum::<f32>()
        })
        .sum()
}

fn ic_batch(fonts: &FontCollection, specs: &[Spec]) -> f32 {
    specs
        .iter()
        .map(|spec| fonts.resolve_ic(&spec.ic_query, spec.size).advance)
        .sum()
}

fn layout(fonts: &FontCollection, specs: &[Spec], prepared: Option<&[Values]>) -> (usize, f64) {
    let limits = Limits::default();
    let ps = ParagraphStyle::default();
    let mut builder = ParagraphBuilder::new(&ps, &limits);
    for (i, spec) in specs.iter().enumerate() {
        let v = prepared.map_or_else(|| resolve(fonts, spec), |values| values[i]);
        let style = InlineStyle {
            font_families: vec![spec.family.clone()],
            font_size: spec.size,
            letter_spacing: v.spacing,
            ..Default::default()
        };
        let edges = InlineEdges {
            margin: Sides {
                inline_start: v.start,
                inline_end: v.end,
                ..Default::default()
            },
            padding: Sides {
                inline_start: v.padding,
                ..Default::default()
            },
            ..Default::default()
        };
        builder.open_inline(NodeId(i as u64 + 1), &style, edges);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(i as u64 + 1),
                offset: 0,
            },
            &spec.text,
        );
        builder.close_inline();
    }
    let mut cx = LayoutContext::new();
    let paragraph = builder.build(&mut cx, fonts).unwrap();
    let lines = paragraph.break_all(&mut cx, &LineOptions::default(), WIDTH, &AtomicSizes::EMPTY);
    let width_sum: f64 = lines.iter().map(|line| line.inline_size() as f64).sum();
    (lines.len(), width_sum)
}

fn run(op: &str, fonts: &FontCollection, specs: &[Spec], prepared: &[Values]) -> f64 {
    match op {
        "ch" => ch_batch(fonts, specs) as f64,
        "ic" => ic_batch(fonts, specs) as f64,
        "units" => unit_batch(fonts, specs) as f64,
        "layout_only" => {
            let (lines, sum) = layout(fonts, specs, Some(prepared));
            lines as f64 + sum
        }
        "full_layout" => {
            let (lines, sum) = layout(fonts, specs, None);
            lines as f64 + sum
        }
        _ => unreachable!(),
    }
}

fn main() {
    let mode = std::env::args().nth(1).expect("time or alloc");
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    let fonts = &fonts.collection;
    let specs = specs();
    let prepared: Vec<_> = specs.iter().map(|spec| resolve(fonts, spec)).collect();
    let (line_count, width_sum) = layout(fonts, &specs, Some(&prepared));
    let full = layout(fonts, &specs, None);
    assert_eq!(line_count, full.0);
    assert_eq!(width_sum, full.1);
    let ops = ["ch", "ic", "units", "layout_only", "full_layout"];
    let mut results = serde_json::Map::new();
    for op in ops {
        for _ in 0..WARMUP {
            black_box(run(op, fonts, &specs, &prepared));
        }
        match mode.as_str() {
            "time" => {
                let samples: Vec<_> = (0..SAMPLES)
                    .map(|_| {
                        let start = Instant::now();
                        for _ in 0..REPEATS {
                            black_box(run(op, fonts, &specs, &prepared));
                        }
                        start.elapsed().as_nanos() as f64 / REPEATS as f64
                    })
                    .collect();
                results.insert(op.into(), serde_json::json!(samples));
            }
            #[cfg(feature = "allocation-counting")]
            "alloc" => {
                let samples: Vec<_> = (0..SAMPLES)
                    .map(|_| {
                        let scope = ALLOCATOR.begin().unwrap();
                        black_box(run(op, fonts, &specs, &prepared));
                        scope.finish()
                    })
                    .collect();
                results.insert(op.into(), serde_json::json!(samples));
            }
            _ => panic!("mode must be time or alloc; alloc requires allocation-counting"),
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "mode":mode,"spans":SPANS,"ch_calls":SPANS*3,"ic_calls":SPANS,
            "viewport_width":WIDTH,"warmup":WARMUP,"samples":SAMPLES,"repeats":REPEATS,
            "line_count":line_count,"line_width_sum":width_sum,"results":results
        })
    );
}
