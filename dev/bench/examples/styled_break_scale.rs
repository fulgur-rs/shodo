//! Public-API probe for shodo-13f. Usage:
//! `<plain|ruby> <control|sparse|dense> <normal|count> <size> <cold|warm> <reps>`.
//! Timing and allocation builds are separate. Fonts, paragraph construction,
//! output/warning hashes, context creation and warm-up are outside the scope.
//! Layout and result drops are included; cold contexts also drop in scope.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
    Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
    RubyVisibility,
};
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

fn style(height: f32) -> InlineStyle {
    InlineStyle {
        font_size: 10.0,
        line_height: LineHeight::Px(height),
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        ..Default::default()
    }
}

fn paragraph(ruby: bool, case: &str, count: usize, limits: &Limits) -> ParagraphBuilder {
    let ps = ParagraphStyle {
        root: style(10.0),
        line_height_quirk: true,
        ..Default::default()
    };
    let mut base = ParagraphBuilder::new(&ps, limits);
    for i in 0..count as u64 {
        base.open_inline(NodeId(1000 + 3 * i), &style(10.0), Default::default());
        if i % 2 == 0 {
            base.push_text(
                TextSource::Generated {
                    node: NodeId(1001 + 3 * i),
                },
                "日",
            );
        }
        if case == "dense" || (case == "sparse" && i == 0) {
            base.push_forced_break_with_style(NodeId(1002 + 3 * i), &style(40.0));
        } else {
            base.push_forced_break(NodeId(1002 + 3 * i));
        }
        base.close_inline();
    }
    if !ruby {
        return base;
    }
    let mut top = ParagraphBuilder::new(&ps, limits);
    top.push_ruby(
        NodeId(1),
        &style(10.0),
        Ruby::new(
            vec![RubyBase {
                node: NodeId(2),
                content: RubyContent::from_builder(base),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(3),
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(3) },
                        &"日".repeat(count / 2 + 1),
                        &style(10.0),
                        limits,
                    ),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap(),
    );
    top
}

fn layout(p: &Paragraph, cx: &mut LayoutContext, count_mode: bool) -> Vec<Line> {
    let mut constraint = LineConstraint::new(96.0);
    if count_mode {
        constraint.max_graphemes = Some(usize::MAX);
    }
    let mut token = p.start_token();
    let mut lines = Vec::new();
    loop {
        match p.next_line(
            cx,
            token,
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                token = line.break_token();
                lines.push(line);
            }
            LineResult::Done => break,
            other => panic!("unexpected layout result: {other:?}"),
        }
    }
    lines
}

fn hash(value: &impl serde::Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

fn warnings(list: &[shodo::limits::Warning]) -> Vec<String> {
    list.iter()
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        args.len(),
        6,
        "use: <plain|ruby> <control|sparse|dense> <normal|count> <size> <cold|warm> <reps>"
    );
    assert!(["plain", "ruby"].contains(&args[0].as_str()));
    assert!(["control", "sparse", "dense"].contains(&args[1].as_str()));
    assert!(["normal", "count"].contains(&args[2].as_str()));
    assert!(["cold", "warm"].contains(&args[4].as_str()));
    let count: usize = args[3].parse().unwrap();
    let reps: usize = args[5].parse().unwrap();
    assert!(count > 0 && reps > 0);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let p = paragraph(args[0] == "ruby", &args[1], count, &limits)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let count_mode = args[2] == "count";
    let mut warm = LayoutContext::new();
    let lines = layout(&p, &mut warm, count_mode);
    let output = hash(&lines.iter().map(snapshot::line).collect::<Vec<_>>());
    let build_warnings = warnings(p.warnings());
    let layout_warnings = warnings(&warm.take_warnings());
    let warning = hash(&json!({"build":build_warnings,"layout":layout_warnings}));
    let line_count = lines.len();
    drop(lines);
    let contexts: Vec<_> = if args[4] == "cold" {
        (0..reps).map(|_| LayoutContext::new()).collect()
    } else {
        Vec::new()
    };
    #[cfg(feature = "allocation-counting")]
    let scope = ALLOCATOR.begin().unwrap();
    let started = Instant::now();
    if args[4] == "cold" {
        for mut cx in contexts {
            black_box(layout(&p, &mut cx, count_mode));
        }
    } else {
        for _ in 0..reps {
            black_box(layout(&p, &mut warm, count_mode));
        }
    }
    let elapsed = started.elapsed().as_nanos();
    #[cfg(feature = "allocation-counting")]
    let allocations = Some(scope.finish());
    #[cfg(not(feature = "allocation-counting"))]
    let allocations: Option<()> = None;
    println!(
        "{}",
        json!({
            "issue":"shodo-13f", "dataset":args[0], "case":args[1], "line_mode":args[2],
            "size":count, "context":args[4], "reps":reps, "lines":line_count,
            "elapsed_ns":if cfg!(feature="allocation-counting") { None } else { Some(elapsed) },
            "allocations":allocations, "output_sha256":output, "warnings_sha256":warning,
            "build_warnings":build_warnings, "layout_warnings":layout_warnings,
        })
    );
}
