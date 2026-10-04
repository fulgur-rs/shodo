//! Build-only probe for empty text-combine span membership passes.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle, TextCombineUpright};
use shodo::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Case {
    PlainShort,
    PlainLong,
    TcyMixed,
    TcyHorizontalDisabled,
    TcyRejected,
    FirstLine,
    Empty,
}

impl Case {
    fn parse(name: &str) -> Self {
        match name {
            "plain-short" => Self::PlainShort,
            "plain-long" => Self::PlainLong,
            "tcy-mixed" => Self::TcyMixed,
            "tcy-horizontal-disabled" => Self::TcyHorizontalDisabled,
            "tcy-rejected" => Self::TcyRejected,
            "first-line" => Self::FirstLine,
            "empty" => Self::Empty,
            _ => panic!("unknown case: {name}"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::PlainShort => "plain-short",
            Self::PlainLong => "plain-long",
            Self::TcyMixed => "tcy-mixed",
            Self::TcyHorizontalDisabled => "tcy-horizontal-disabled",
            Self::TcyRejected => "tcy-rejected",
            Self::FirstLine => "first-line",
            Self::Empty => "empty",
        }
    }

    fn writing_mode(self) -> WritingMode {
        match self {
            Self::TcyMixed | Self::TcyRejected => WritingMode::VerticalRl,
            _ => WritingMode::HorizontalTb,
        }
    }

    fn inline_size(self) -> f32 {
        match self {
            Self::PlainShort | Self::PlainLong | Self::FirstLine => 48.0,
            Self::TcyMixed | Self::TcyHorizontalDisabled | Self::TcyRejected | Self::Empty => 32.0,
        }
    }
}

fn root_style() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[0].family.into())],
        font_size: 16.0,
        ..Default::default()
    }
}

fn source(node: u64) -> TextSource {
    TextSource::Generated { node: NodeId(node) }
}

fn builder(case: Case, limits: &Limits) -> ParagraphBuilder {
    let mut root = root_style();
    match case {
        Case::TcyHorizontalDisabled | Case::TcyRejected => {
            root.text_combine_upright = TextCombineUpright::All;
        }
        _ => {}
    }
    let mut paragraph_style = ParagraphStyle {
        root: root.clone(),
        writing_mode: case.writing_mode(),
        ..Default::default()
    };
    if matches!(case, Case::FirstLine) {
        paragraph_style.first_line = Some(InlineStyle {
            font_size: 24.0,
            ..root.clone()
        });
    }
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);

    match case {
        Case::PlainShort => {
            builder.push_text(source(1), "a b c d e f");
        }
        Case::PlainLong | Case::FirstLine => {
            let repeats = if matches!(case, Case::PlainLong) {
                128
            } else {
                32
            };
            let text = "a b c d ".repeat(repeats);
            builder.push_text(source(1), &text);
        }
        Case::TcyMixed => {
            let mut combined = root.clone();
            combined.text_combine_upright = TextCombineUpright::All;
            for index in 0..8 {
                builder.push_text(source(10 + index * 5), "a");
                let first = NodeId(11 + index * 5);
                builder
                    .open_inline(first, &combined, InlineEdges::default())
                    .push_text(TextSource::Generated { node: first }, "12")
                    .close_inline();
                builder.push_text(source(12 + index * 5), "b");
                let second = NodeId(13 + index * 5);
                builder
                    .open_inline(second, &combined, InlineEdges::default())
                    .push_text(TextSource::Generated { node: second }, "34")
                    .close_inline();
                builder.push_text(source(14 + index * 5), "c ");
            }
        }
        Case::TcyHorizontalDisabled => {
            let text = "1234 ".repeat(64);
            builder.push_text(source(1), &text);
        }
        Case::TcyRejected => {
            builder.push_text(source(1), "12");
            builder.open_inline(NodeId(2), &root, InlineEdges::default());
            builder.push_text(source(3), "34");
            builder.close_inline();
        }
        Case::Empty => {}
    }
    builder
}

fn output_signature(
    paragraph: &Paragraph,
    context: &mut LayoutContext,
    case: Case,
) -> (String, String, Vec<String>, Vec<String>) {
    let lines = paragraph.break_all(
        context,
        &LineOptions::default(),
        case.inline_size(),
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
        output_signature(paragraph, context, case);
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
