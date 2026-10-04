//! Build-only probe for scoped text-combine width-probe reuse in ruby bases.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::geometry::{Direction, WritingMode};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextAutospace, TextCombineUpright,
};
use shodo::{
    AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use std::hint::black_box;
use std::time::Instant;

const REPEATS: usize = 64;
const INLINE_SIZE: f32 = 96.0;

#[derive(Clone, Copy)]
enum Case {
    BaseTwoRl,
    BaseThreeLrRtl,
    BaseFourRlMulti,
    BaseTwoLrRtlMulti,
    BaseFallback,
    PlainTcy,
}

struct Shape {
    text: &'static str,
    mode: WritingMode,
    direction: Direction,
    multiple_styles: bool,
}

impl Case {
    fn parse(name: &str) -> Self {
        match name {
            "base-two-rl" => Self::BaseTwoRl,
            "base-three-lr-rtl" => Self::BaseThreeLrRtl,
            "base-four-rl-multi" => Self::BaseFourRlMulti,
            "base-two-lr-rtl-multi" => Self::BaseTwoLrRtlMulti,
            "base-fallback" => Self::BaseFallback,
            "plain-tcy" => Self::PlainTcy,
            _ => panic!("unknown case: {name}"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::BaseTwoRl => "base-two-rl",
            Self::BaseThreeLrRtl => "base-three-lr-rtl",
            Self::BaseFourRlMulti => "base-four-rl-multi",
            Self::BaseTwoLrRtlMulti => "base-two-lr-rtl-multi",
            Self::BaseFallback => "base-fallback",
            Self::PlainTcy => "plain-tcy",
        }
    }

    fn shape(self) -> Shape {
        let (text, mode, direction, multiple_styles) = match self {
            Self::BaseTwoRl | Self::BaseFallback | Self::PlainTcy => {
                ("12", WritingMode::VerticalRl, Direction::Ltr, false)
            }
            Self::BaseThreeLrRtl => ("123", WritingMode::VerticalLr, Direction::Rtl, false),
            Self::BaseFourRlMulti => ("1234", WritingMode::VerticalRl, Direction::Ltr, true),
            Self::BaseTwoLrRtlMulti => ("12", WritingMode::VerticalLr, Direction::Rtl, true),
        };
        Shape {
            text,
            mode,
            direction,
            multiple_styles,
        }
    }
}

fn style(family: &str, direction: Direction) -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(family.into())],
        direction,
        text_combine_upright: TextCombineUpright::All,
        text_autospace: TextAutospace::NoAutospace,
        ..Default::default()
    }
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let shape = case.shape();
    let cjk_family = shodo_fixtures::FONTS[1].family;
    let latin_family = shodo_fixtures::FONTS[0].family;
    let cjk_style = style(cjk_family, shape.direction);
    let latin_style = InlineStyle {
        font_size: 19.0,
        ..style(latin_family, shape.direction)
    };
    let mut plain_latin_style = latin_style.clone();
    plain_latin_style.text_combine_upright = TextCombineUpright::None;

    let base_limits = Limits {
        max_shaping_run_bytes: matches!(case, Case::BaseFallback).then_some(1),
        ..Limits::default()
    };
    let paragraph_style = ParagraphStyle {
        writing_mode: shape.mode,
        direction: shape.direction,
        root: cjk_style.clone(),
        ..Default::default()
    };
    // The TCY content builder uses `base_limits`; the outer paragraph uses `limits`.
    let mut base_builder = ParagraphBuilder::new(
        &paragraph_style,
        if matches!(case, Case::PlainTcy) {
            limits
        } else {
            &base_limits
        },
    );
    for repeat in 0..REPEATS {
        if repeat > 0 {
            let separator = NodeId(3000 + repeat as u64);
            base_builder
                .open_inline(separator, &plain_latin_style, InlineEdges::default())
                .push_text(
                    TextSource::Dom {
                        node: separator,
                        offset: 0,
                    },
                    "x",
                )
                .close_inline();
        }
        let wrapper = NodeId(100 + repeat as u64);
        let first = NodeId(1000 + repeat as u64);
        base_builder
            .open_inline(wrapper, &cjk_style, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: first,
                    offset: 10,
                },
                shape.text,
            )
            .close_inline();
    }
    if shape.multiple_styles {
        let node = NodeId(9000);
        base_builder
            .open_inline(node, &plain_latin_style, InlineEdges::default())
            .push_text(TextSource::Dom { node, offset: 0 }, "x")
            .close_inline();
    }
    if matches!(case, Case::PlainTcy) {
        return base_builder;
    }

    let base = RubyContent::from_builder(base_builder);
    let annotation = RubyContent::text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 30,
        },
        "日",
        &style(cjk_family, shape.direction),
        limits,
    );
    let ruby = Ruby::new(
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
    .unwrap();
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    builder.push_ruby(NodeId(1), &cjk_style, ruby);
    builder
}

fn output_signature(
    paragraph: &Paragraph,
    context: &mut LayoutContext,
) -> (String, String, Vec<String>, Vec<String>) {
    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        INLINE_SIZE,
        &AtomicSizes::EMPTY,
    );
    let output = lines.iter().map(snapshot::line).collect::<Vec<_>>();
    let build_warnings = paragraph
        .warnings()
        .iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect::<Vec<_>>();
    let layout_warnings = context
        .take_warnings()
        .into_iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect::<Vec<_>>();
    let output_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    let warning_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({
                "build": build_warnings,
                "layout": layout_warnings,
            }))
            .unwrap()
        )
    );
    (
        output_sha256,
        warning_sha256,
        build_warnings,
        layout_warnings,
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    assert_eq!(
        args.next().as_deref(),
        Some("sample"),
        "use: sample <case> <label> <index> <builds>"
    );
    let case = Case::parse(&args.next().expect("case name"));
    let label = args.next().expect("baseline or candidate label");
    let sample: usize = args.next().expect("sample index").parse().unwrap();
    let builds: usize = args.next().expect("build count").parse().unwrap();
    assert!(builds > 0);

    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    builder(case, &limits)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();

    let inputs = (0..builds)
        .map(|_| (builder(case, &limits), LayoutContext::new()))
        .collect::<Vec<_>>();
    let mut outputs = Vec::with_capacity(builds);
    let start = Instant::now();
    for (builder, mut context) in inputs {
        let paragraph = black_box(builder.build(&mut context, &fonts.collection).unwrap());
        outputs.push((paragraph, context));
    }
    let elapsed_ns = start.elapsed().as_nanos();

    let (paragraph, context) = outputs.first_mut().expect("at least one output");
    let (output_sha256, warning_sha256, build_warnings, layout_warnings) =
        output_signature(paragraph, context);
    assert!(outputs.iter().all(|(paragraph, _)| {
        paragraph
            .warnings()
            .iter()
            .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
            .collect::<Vec<_>>()
            == build_warnings
    }));

    println!(
        "{}",
        json!({
            "case": case.name(),
            "label": label,
            "sample": sample,
            "builds": builds,
            "build_ns_total": elapsed_ns,
            "build_ns_per_paragraph": elapsed_ns / builds as u128,
            "output_sha256": output_sha256,
            "warning_sha256": warning_sha256,
            "build_warnings": build_warnings,
            "layout_warnings": layout_warnings,
        })
    );
}
