//! Build-only probe for scoped text-combine width-probe reuse in ruby bases.
//! `sample` times builds in an uninstrumented binary; `alloc` needs the
//! `allocation-counting` feature and reports allocations of one build.
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

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOC: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

const REPEATS: usize = 64;
const INLINE_SIZE: f32 = 96.0;
/// Nested ruby levels and innermost TCY groups of `deep-nested-multi`.
const DEEP_DEPTH: usize = 250;
const DEEP_GROUPS: usize = 400;

#[derive(Clone, Copy)]
enum Case {
    BaseTwoRl,
    BaseThreeLrRtl,
    BaseFourRlMulti,
    BaseTwoLrRtlMulti,
    BaseFallback,
    PlainTcy,
    DeepNestedMulti,
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
            "deep-nested-multi" => Self::DeepNestedMulti,
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
            Self::DeepNestedMulti => "deep-nested-multi",
        }
    }

    fn shape(self) -> Shape {
        let (text, mode, direction, multiple_styles) = match self {
            Self::BaseTwoRl | Self::BaseFallback | Self::PlainTcy | Self::DeepNestedMulti => {
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

fn ruby(base: RubyContent, annotation_style: &InlineStyle, limits: &Limits) -> Ruby {
    let annotation = RubyContent::text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 30,
        },
        "日",
        annotation_style,
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

/// `DEEP_DEPTH` nested ruby bases; the innermost holds `DEEP_GROUPS` TCY
/// "1日2日" elements styled Latin-then-CJK, each after a plain Latin "x".
/// Latin lacks "日", so every group has four shape inputs owned by the
/// deepest base scope.
fn deep_builder(limits: &Limits) -> ParagraphBuilder {
    let cjk_family = shodo_fixtures::FONTS[1].family;
    let latin_family = shodo_fixtures::FONTS[0].family;
    let cjk_style = style(cjk_family, Direction::Ltr);
    let mixed_style = InlineStyle {
        font_families: vec![
            FontFamily::Named(latin_family.into()),
            FontFamily::Named(cjk_family.into()),
        ],
        ..cjk_style.clone()
    };
    let plain_style = InlineStyle {
        text_combine_upright: TextCombineUpright::None,
        ..style(latin_family, Direction::Ltr)
    };
    let paragraph_style = ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        direction: Direction::Ltr,
        root: cjk_style.clone(),
        ..Default::default()
    };
    let base_limits = Limits::default();
    let mut content = ParagraphBuilder::new(&paragraph_style, &base_limits);
    for group in 0..DEEP_GROUPS as u64 {
        let separator = NodeId(100_000 + 2 * group);
        let text = NodeId(100_001 + 2 * group);
        content
            .open_inline(separator, &plain_style, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: separator,
                    offset: 0,
                },
                "x",
            )
            .close_inline()
            .open_inline(text, &mixed_style, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: text,
                    offset: 0,
                },
                "1日2日",
            )
            .close_inline();
    }
    for level in (0..DEEP_DEPTH).rev() {
        let ruby = ruby(RubyContent::from_builder(content), &cjk_style, limits);
        content = ParagraphBuilder::new(
            &paragraph_style,
            if level == 0 { limits } else { &base_limits },
        );
        content.push_ruby(NodeId(1), &cjk_style, ruby);
    }
    content
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    if matches!(case, Case::DeepNestedMulti) {
        return deep_builder(limits);
    }
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

    let ruby = ruby(
        RubyContent::from_builder(base_builder),
        &style(cjk_family, shape.direction),
        limits,
    );
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    builder.push_ruby(NodeId(1), &cjk_style, ruby);
    builder
}

fn paragraph_warnings(paragraph: &Paragraph) -> Vec<String> {
    paragraph
        .warnings()
        .iter()
        .map(|warning| format!("{:?}:{}", warning.kind, warning.message))
        .collect()
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
    let build_warnings = paragraph_warnings(paragraph);
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
    match args.next().as_deref() {
        Some("sample") => sample(args),
        Some("alloc") => alloc(args),
        _ => panic!("use: sample <case> <label> <index> <builds> | alloc <case> <label> <builds>"),
    }
}

/// Allocation calls, requested bytes and peak extra live bytes of each of
/// `builds` single builds, after a warm-up build. Builders and contexts are
/// prepared outside the counting scope.
#[cfg(feature = "allocation-counting")]
fn alloc(mut args: impl Iterator<Item = String>) {
    let case = Case::parse(&args.next().expect("case name"));
    let label = args.next().expect("baseline or candidate label");
    let builds: usize = args.next().expect("build count").parse().unwrap();
    assert!(builds > 0);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    builder(case, &limits)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut counts = Vec::with_capacity(builds);
    let mut digest = None;
    for _ in 0..builds {
        let builder = builder(case, &limits);
        let mut context = LayoutContext::new();
        let scope = ALLOC.begin().unwrap();
        let paragraph = black_box(builder.build(&mut context, &fonts.collection).unwrap());
        let count = scope.finish();
        counts.push(json!({
            "calls": count.calls,
            "allocated_bytes": count.allocated_bytes,
            "peak_extra_bytes": count.peak_extra_bytes,
            "retained_bytes": count.net_bytes,
        }));
        let signature = output_signature(&paragraph, &mut context);
        assert!(digest.get_or_insert(signature.0.clone()) == &signature.0);
    }
    println!(
        "{}",
        json!({
            "case": case.name(),
            "label": label,
            "builds": builds,
            "allocations": counts,
            "output_sha256": digest,
        })
    );
}

#[cfg(not(feature = "allocation-counting"))]
fn alloc(_: impl Iterator<Item = String>) {
    panic!("alloc requires --features allocation-counting");
}

fn sample(mut args: impl Iterator<Item = String>) {
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

    // `break_all` on 250 nested ruby levels takes seconds, so timing runs of
    // `deep-nested-multi` may skip the output digest; smoke runs keep it.
    let (paragraph, context) = outputs.first_mut().expect("at least one output");
    let (output_sha256, warning_sha256, build_warnings, layout_warnings) =
        if std::env::var_os("TCY_PROBE_SKIP_DIGEST").is_some() {
            (None, None, paragraph_warnings(paragraph), None)
        } else {
            let (output, warning, build, layout) = output_signature(paragraph, context);
            (Some(output), Some(warning), build, Some(layout))
        };
    assert!(
        outputs
            .iter()
            .all(|(paragraph, _)| paragraph_warnings(paragraph) == build_warnings)
    );

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
