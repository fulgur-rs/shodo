//! shodo-ogn: intrinsic row accumulator eviction, with fixed CJK fonts.
//! `digest|time|alloc|loop <case> [groups=64] [iterations=5]`.
//! Preparation is outside intrinsic timing/allocation scopes. Each call uses
//! a fresh context; timing excludes context destruction. Allocation output
//! separates retained context, shrink_to(0), and final context destruction.
use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicIntrinsic, AtomicIntrinsics, FloatClear, FloatIntrinsic, IntrinsicSizes, LayoutContext,
    Paragraph, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel,
    RubyOverhang, RubySpan, RubyStyle, RubyVisibility,
};
use std::{hint::black_box, time::Instant};

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const CASES: &[&str] = &[
    "eviction",
    "one-word",
    "one-ruby",
    "plain",
    "forced",
    "first-line",
    "atomics",
    "warning0",
    "warning2",
    "budget0",
    "budget1",
    "default-budget",
];

fn text(builder: &mut ParagraphBuilder, id: u64, value: &str) {
    builder.push_text(TextSource::Generated { node: NodeId(id) }, value);
}

fn prepare(case: &str, groups: usize) -> (Paragraph, AtomicIntrinsics) {
    let limits = Limits {
        max_ruby_line_work: match case {
            "budget0" => Some(0),
            "budget1" => Some(1),
            "default-budget" => Limits::default().max_ruby_line_work,
            _ => None,
        },
        max_warnings: match case {
            "warning0" => Some(0),
            "warning2" => Some(2),
            _ => Limits::default().max_warnings,
        },
        ..Default::default()
    };
    let fixture = shodo_fixtures::font("cjk").unwrap();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts.register(fixture.bytes.to_vec()).unwrap();
    let style = InlineStyle {
        font_size: 24.0,
        font_families: vec![FontFamily::Named(fixture.family.into())],
        ..Default::default()
    };
    let annotation_style = InlineStyle {
        font_size: 12.0,
        ..style.clone()
    };
    let paragraph = ParagraphStyle {
        root: style.clone(),
        first_line: (case == "first-line").then(|| InlineStyle {
            font_size: 30.0,
            ..style.clone()
        }),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph, &limits);
    let mut inputs = AtomicIntrinsics::new();
    if case == "atomics" {
        builder.push_atomic(NodeId(99), &style, Default::default());
        inputs.insert_atomic(
            NodeId(99),
            AtomicIntrinsic {
                min_content: 4.0,
                max_content: 8.0,
            },
        );
    }
    if case.starts_with("warning") {
        for node in [97, 98, 99] {
            builder.push_atomic(NodeId(node), &style, Default::default());
        }
    }
    for group in 0..groups as u64 {
        for word in 0..if case == "one-word" { 1 } else { 2 } {
            for position in 0..if case == "one-ruby" { 1 } else { 2 } {
                let id = group * 4 + word * 2 + position;
                if case == "plain" {
                    text(&mut builder, 300_000 + id, "12");
                    continue;
                }
                let ruby = Ruby::new(
                    vec![RubyBase {
                        node: NodeId(1000 + id),
                        content: RubyContent::text(
                            TextSource::Generated {
                                node: NodeId(300_000 + id),
                            },
                            "12",
                            &style,
                            &limits,
                        ),
                        align: RubyAlign::default(),
                    }],
                    vec![RubyLevel {
                        annotations: vec![RubyAnnotation {
                            node: NodeId(2000 + id),
                            content: RubyContent::text(
                                TextSource::Generated {
                                    node: NodeId(400_000 + id),
                                },
                                "日日日",
                                &annotation_style,
                                &limits,
                            ),
                            span: RubySpan::Auto,
                            visibility: RubyVisibility::Visible,
                        }],
                        style: RubyStyle {
                            overhang: RubyOverhang::None,
                            ..Default::default()
                        },
                    }],
                )
                .unwrap();
                builder.push_ruby(NodeId(1000 + id), &style, ruby);
            }
            text(&mut builder, 500_000 + group * 2 + word, " ");
        }
        let node = NodeId(900_000 + group);
        builder.push_out_of_flow(node, OutOfFlowKind::Float);
        inputs.insert_float(
            node,
            FloatIntrinsic {
                min_content: 1.0,
                max_content: 1.0,
                side: Default::default(),
                clear: FloatClear::Both,
            },
        );
        if case == "forced" && group % 8 == 7 {
            builder.push_forced_break(NodeId(600_000 + group));
        }
    }
    (
        builder.build(&mut LayoutContext::new(), &fonts).unwrap(),
        inputs,
    )
}

#[inline(never)]
fn intrinsic(p: &Paragraph, inputs: &AtomicIntrinsics, cx: &mut LayoutContext) -> IntrinsicSizes {
    black_box(p.intrinsic_sizes(black_box(cx), &LineOptions::default(), black_box(inputs)))
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(
        (2..=4).contains(&args.len()),
        "digest|time|alloc|loop <case> [groups] [iterations]"
    );
    let mode = args[0].as_str();
    let case = args[1].as_str();
    assert!(CASES.contains(&case));
    let groups: usize = args.get(2).map_or(64, |s| s.parse().unwrap());
    let iterations: usize = args.get(3).map_or(5, |s| s.parse().unwrap());
    assert!(groups > 0 && iterations > 0);
    let (p, inputs) = prepare(case, groups);
    let mut cx = LayoutContext::new();
    let output = match mode {
        "loop" => {
            for _ in 0..iterations {
                let mut context = LayoutContext::new();
                black_box(intrinsic(&p, &inputs, &mut context));
            }
            return;
        }
        "time" => {
            // Warm up without retaining a measured context.
            intrinsic(&p, &inputs, &mut LayoutContext::new());
            let samples: Vec<_> = (0..21)
                .map(|_| {
                    let mut context = LayoutContext::new();
                    let start = Instant::now();
                    let widths = intrinsic(&p, &inputs, &mut context);
                    let ns = start.elapsed().as_nanos() as u64;
                    black_box((widths, context));
                    ns
                })
                .collect();
            let mut sorted = samples.clone();
            sorted.sort_unstable();
            json!({"samples_ns":samples,"median_ns":sorted[sorted.len()/2]})
        }
        "digest" | "alloc" => {
            #[cfg(feature = "allocation-counting")]
            let scope = (mode == "alloc").then(|| ALLOC.begin().unwrap());
            #[cfg(not(feature = "allocation-counting"))]
            assert!(mode != "alloc", "alloc requires allocation-counting");
            let widths = intrinsic(&p, &inputs, &mut cx);
            #[cfg(feature = "allocation-counting")]
            let counts = scope.map(|s| s.finish());
            let measured_warnings = cx.take_warnings();
            let warnings: Vec<_> = p
                .warnings()
                .iter()
                .chain(measured_warnings.iter())
                .map(|w| format!("{w:?}"))
                .collect();
            let bits = [widths.min_content.to_bits(), widths.max_content.to_bits()];
            let public = json!({"width_bits":bits,"warnings":warnings});
            let report = json!({"output":public,"sha256":format!("{:x}",Sha256::digest(public.to_string().as_bytes()))});
            #[cfg(feature = "allocation-counting")]
            let mut report = report;
            #[cfg(feature = "allocation-counting")]
            if let Some(counts) = counts {
                report["allocations"] = serde_json::to_value(counts).unwrap();
                let scope = ALLOC.begin().unwrap();
                cx.shrink_to(0);
                report["shrink"] = serde_json::to_value(scope.finish()).unwrap();
                let scope = ALLOC.begin().unwrap();
                drop(cx);
                drop(measured_warnings);
                report["context_and_warnings_drop"] = serde_json::to_value(scope.finish()).unwrap();
            }
            report
        }
        _ => panic!("unknown mode"),
    };
    println!(
        "{}",
        json!({"case":case,"groups":groups,"context_inline_bytes":std::mem::size_of::<LayoutContext>(),"result":output})
    );
}
