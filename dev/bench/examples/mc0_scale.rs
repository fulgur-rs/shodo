//! shodo-mc0 probe: build and line-measurement time of the remaining
//! adversarial ruby shapes, on either side of the ruby container caps.
//!
//! `sample <case> <size> <op> <label>` prints one JSON line with the build
//! time, the time of one `op` (`break` = `break_all`, `intrinsic` =
//! `intrinsic_sizes`) on a fresh context, the output and warning digests.
//!
//! Cases (fixed CJK font `FONTS[1]`, annotation "日"):
//! - `siblings`: `size` rubies over "12" in a row (one unbreakable line).
//! - `churn`: `siblings` inside a top-aligned span, each ruby followed by a
//!   "日" larger than every earlier one, so the line profile changes at
//!   every step (shodo-2j6 "profile churn").
//! - `ordinary`: `size` breakable groups of "日日" and a ruby over "日日".
//! - `plain`: ruby-free control, "日" × `4 * size`.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle, VerticalAlign};
use shodo::{
    AtomicIntrinsic, AtomicIntrinsics, AtomicSizes, LayoutContext, ParagraphBuilder, Ruby,
    RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
    RubyVisibility,
};
use std::time::Instant;

const INLINE_SIZE: f32 = 96.0;

fn style() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        ..Default::default()
    }
}

fn ruby(base: RubyContent, reading: &str, limits: &Limits) -> Ruby {
    let annotation = RubyContent::text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 30,
        },
        reading,
        &style(),
        limits,
    );
    Ruby::new(
        vec![RubyBase {
            node: NodeId(2),
            content: base,
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(3),
                content: annotation,
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap()
}

fn text(b: &mut ParagraphBuilder, node: u64, text: &str) {
    b.push_text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        text,
    );
}

fn digit_ruby(b: &mut ParagraphBuilder, i: u64, paragraph: &ParagraphStyle, limits: &Limits) {
    let mut base = ParagraphBuilder::new(paragraph, limits);
    text(&mut base, 1_000_000 + i, "12");
    b.push_ruby(
        NodeId(2_000_000 + i),
        &style(),
        ruby(RubyContent::from_builder(base), "日", limits),
    );
}

fn builder(case: &str, size: usize, limits: &Limits) -> ParagraphBuilder {
    let paragraph = ParagraphStyle {
        root: style(),
        ..Default::default()
    };
    let mut top = ParagraphBuilder::new(&paragraph, limits);
    match case {
        "siblings" => {
            for i in 0..size as u64 {
                digit_ruby(&mut top, i, &paragraph, limits);
            }
        }
        "churn" | "churnplain" => {
            top.open_inline(
                NodeId(7),
                &InlineStyle {
                    vertical_align: VerticalAlign::Top,
                    ..style()
                },
                Default::default(),
            );
            for i in 0..size as u64 {
                if case == "churn" {
                    digit_ruby(&mut top, i, &paragraph, limits);
                } else {
                    text(&mut top, 1_000_000 + i, "12");
                }
                top.open_inline(
                    NodeId(3_000_000 + i),
                    &InlineStyle {
                        font_size: 16.0 + i as f32,
                        ..style()
                    },
                    Default::default(),
                );
                text(&mut top, 4_000_000 + i, "日");
                top.close_inline();
            }
            top.close_inline();
        }
        "ordinary" => {
            for i in 0..size as u64 {
                text(&mut top, 5_000_000 + i, "日日");
                let mut base = ParagraphBuilder::new(&paragraph, limits);
                text(&mut base, 6_000_000 + i, "日日");
                top.push_ruby(
                    NodeId(7_000_000 + i),
                    &style(),
                    ruby(RubyContent::from_builder(base), "日", limits),
                );
            }
        }
        "plain" => text(&mut top, 1, &"日".repeat(4 * size)),
        "pairs" => {
            // One ruby with `size` base/annotation pairs.
            let bases = (0..size as u64)
                .map(|i| {
                    let mut base = ParagraphBuilder::new(&paragraph, limits);
                    text(&mut base, 1_000_000 + i, "日");
                    RubyBase {
                        node: NodeId(2_000_000 + i),
                        content: RubyContent::from_builder(base),
                        align: RubyAlign::default(),
                    }
                })
                .collect();
            let annotations = (0..size as u64)
                .map(|i| RubyAnnotation {
                    node: NodeId(3_000_000 + i),
                    content: RubyContent::text(
                        TextSource::Dom {
                            node: NodeId(3_000_000 + i),
                            offset: 0,
                        },
                        "日日",
                        &style(),
                        limits,
                    ),
                    span: RubySpan::Auto,
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
            top.push_ruby(NodeId(9), &style(), ruby);
        }
        "breaks" | "breaksatomic" => {
            if case == "breaksatomic" {
                // One atomic inline: intrinsic sizes then size atomics for
                // the min and the max content separately.
                top.push_atomic(NodeId(9_000_000), &style(), Default::default());
            }
            // A ruby, then `size` forced breaks after short text: intrinsic
            // sizes alternate the min/max atomic revisions at every break.
            digit_ruby(&mut top, 0, &paragraph, limits);
            for i in 0..size as u64 {
                text(&mut top, 5_000_000 + i, "日日");
                digit_ruby(&mut top, i + 1, &paragraph, limits);
                top.push_forced_break(NodeId(8_000_000 + i));
            }
        }
        other => panic!("unknown case {other}"),
    }
    top
}

fn warnings(list: &[shodo::limits::Warning]) -> Vec<String> {
    list.iter()
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect()
}

fn sample(mut args: impl Iterator<Item = String>) {
    let case = args.next().expect("case");
    let size: usize = args.next().expect("size").parse().unwrap();
    let op = args.next().expect("break or intrinsic");
    let label = args.next().expect("label");
    let inline_size: f32 = args.next().map_or(INLINE_SIZE, |w| w.parse().unwrap());
    #[allow(unused_mut)]
    let mut limits = Limits::default();
    // Candidate only: `MC0_FACTOR` (a number or `none`) overrides
    // `max_ruby_line_work`. Remove this block to build the baseline.
    if let Ok(factor) = std::env::var("MC0_FACTOR") {
        limits.max_ruby_line_work = factor.parse().ok();
    }
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let start = Instant::now();
    let paragraph = builder(&case, size, &limits)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let build_ns = start.elapsed().as_nanos();
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let start = Instant::now();
    let op_ns;
    let (output, lines) = match op.as_str() {
        "break" => {
            let lines = paragraph.break_all(&mut cx, &options, inline_size, &AtomicSizes::EMPTY);
            op_ns = start.elapsed().as_nanos();
            let n = lines.len();
            (
                serde_json::to_vec(&lines.iter().map(snapshot::line).collect::<Vec<_>>()).unwrap(),
                n,
            )
        }
        "intrinsic" => {
            let mut inputs = AtomicIntrinsics::new();
            inputs.insert_atomic(
                NodeId(9_000_000),
                AtomicIntrinsic {
                    min_content: 10.0,
                    max_content: 20.0,
                },
            );
            let sizes = paragraph.intrinsic_sizes(&mut cx, &options, &inputs);
            op_ns = start.elapsed().as_nanos();
            (
                format!("{:?} {:?}", sizes.min_content, sizes.max_content).into_bytes(),
                0,
            )
        }
        other => panic!("unknown op {other}"),
    };
    let layout = warnings(&cx.take_warnings());
    println!(
        "{}",
        json!({
            "issue": "shodo-mc0",
            "case": case,
            "size": size,
            "op": op,
            "inline_size": inline_size,
            "label": label,
            "build_ns": build_ns,
            "op_ns": op_ns,
            "lines": lines,
            "output_sha256": format!("{:x}", Sha256::digest(&output)),
            "warning_sha256": format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&json!({
                        "build": warnings(paragraph.warnings()),
                        "layout": layout,
                    }))
                    .unwrap()
                )
            ),
            "build_warnings": warnings(paragraph.warnings()),
            "layout_warnings": layout,
        })
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("sample") => sample(args),
        _ => panic!("use: sample <case> <size> <break|intrinsic> <label>"),
    }
}
