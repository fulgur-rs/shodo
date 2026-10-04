//! shodo-d77 probe: `break_all` time for nested ruby depth, sibling ruby
//! count, ordinary breakable ruby text and a ruby-free control.
//!
//! `sample <case> <size> <label> <index> <reps>` prints one JSON line. The
//! paragraph, its output digest and the contexts are prepared outside the
//! timed loop; only `reps` calls of `break_all` are timed.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use std::hint::black_box;
use std::time::Instant;

const INLINE_SIZE: f32 = 96.0;

#[derive(Clone, Copy)]
enum Case {
    /// `size` rubies nested in each other's single base around "日".
    Nested,
    /// As `Nested`, but the innermost content is "日\t日". Any tab disables
    /// the through memo (shodo-b7d), so this case stays quadratic.
    NestedTab,
    /// `size` sibling rubies with base "12": one unbreakable line.
    Siblings,
    /// `size` breakable groups of text "日日" and a ruby over "日日".
    Ordinary,
    /// Ruby-free control: "日" repeated `4 * size` times.
    Plain,
}

impl Case {
    fn parse(name: &str) -> Self {
        match name {
            "nested" => Self::Nested,
            "nestedtab" => Self::NestedTab,
            "siblings" => Self::Siblings,
            "ordinary" => Self::Ordinary,
            "plain" => Self::Plain,
            other => panic!("unknown case {other}"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Nested => "nested",
            Self::NestedTab => "nestedtab",
            Self::Siblings => "siblings",
            Self::Ordinary => "ordinary",
            Self::Plain => "plain",
        }
    }
}

fn style() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        ..Default::default()
    }
}

/// The fixed CJK font has "日" and digits; other kana/kanji may be missing,
/// which the geometry snapshot rejects.
fn ruby(base: RubyContent, limits: &Limits) -> Ruby {
    let annotation = RubyContent::text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 30,
        },
        "日",
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

fn builder(case: Case, size: usize, limits: &Limits) -> ParagraphBuilder {
    let paragraph_style = ParagraphStyle {
        root: style(),
        ..Default::default()
    };
    let root = style();
    let text = |b: &mut ParagraphBuilder, node: u64, text: &str| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(node),
                offset: 0,
            },
            text,
        );
    };
    match case {
        Case::Nested | Case::NestedTab => {
            let mut content = ParagraphBuilder::new(&paragraph_style, limits);
            text(
                &mut content,
                1,
                if matches!(case, Case::NestedTab) {
                    "日\t日"
                } else {
                    "日"
                },
            );
            for _ in 0..size {
                let r = ruby(RubyContent::from_builder(content), limits);
                content = ParagraphBuilder::new(&paragraph_style, limits);
                content.push_ruby(NodeId(4), &root, r);
            }
            content
        }
        Case::Siblings => {
            let mut top = ParagraphBuilder::new(&paragraph_style, limits);
            for i in 0..size as u64 {
                let mut base = ParagraphBuilder::new(&paragraph_style, limits);
                text(&mut base, 500_000 + i, "12");
                top.push_ruby(
                    NodeId(600_000 + i),
                    &root,
                    ruby(RubyContent::from_builder(base), limits),
                );
            }
            top
        }
        Case::Ordinary => {
            let mut top = ParagraphBuilder::new(&paragraph_style, limits);
            for i in 0..size as u64 {
                text(&mut top, 700_000 + 2 * i, "日日");
                let mut base = ParagraphBuilder::new(&paragraph_style, limits);
                text(&mut base, 700_001 + 2 * i, "日日");
                top.push_ruby(
                    NodeId(800_000 + i),
                    &root,
                    ruby(RubyContent::from_builder(base), limits),
                );
            }
            top
        }
        Case::Plain => {
            let mut top = ParagraphBuilder::new(&paragraph_style, limits);
            text(&mut top, 1, &"日".repeat(4 * size));
            top
        }
    }
}

fn warnings(list: &[shodo::limits::Warning]) -> Vec<String> {
    list.iter()
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect()
}

fn signature(paragraph: &Paragraph) -> (String, String, Vec<String>, usize) {
    let mut context = LayoutContext::new();
    let lines = paragraph.break_all(
        &mut context,
        &LineOptions::default(),
        INLINE_SIZE,
        &AtomicSizes::EMPTY,
    );
    let output = lines.iter().map(snapshot::line).collect::<Vec<_>>();
    let layout = warnings(&context.take_warnings());
    let output_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
    let warning_sha256 = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&json!({
                "build": warnings(paragraph.warnings()),
                "layout": layout,
            }))
            .unwrap()
        )
    );
    (output_sha256, warning_sha256, layout, lines.len())
}

fn sample(mut args: impl Iterator<Item = String>) {
    let case = Case::parse(&args.next().expect("case"));
    let size: usize = args.next().expect("size").parse().unwrap();
    let label = args.next().expect("baseline or candidate label");
    let sample: usize = args.next().expect("sample index").parse().unwrap();
    let reps: usize = args.next().expect("repetitions").parse().unwrap();
    assert!(reps > 0);
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let paragraph = builder(case, size, &limits)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let (output_sha256, warning_sha256, layout_warnings, lines) = signature(&paragraph);
    let contexts: Vec<_> = (0..reps).map(|_| LayoutContext::new()).collect();
    let options = LineOptions::default();
    let start = Instant::now();
    for mut context in contexts {
        black_box(paragraph.break_all(&mut context, &options, INLINE_SIZE, &AtomicSizes::EMPTY));
    }
    let elapsed_ns = start.elapsed().as_nanos();
    println!(
        "{}",
        json!({
            "issue": "shodo-d77",
            "case": case.name(),
            "size": size,
            "label": label,
            "sample": sample,
            "reps": reps,
            "break_ns_total": elapsed_ns,
            "break_ns_per_call": elapsed_ns / reps as u128,
            "lines": lines,
            "output_sha256": output_sha256,
            "warning_sha256": warning_sha256,
            "build_warnings": warnings(paragraph.warnings()),
            "layout_warnings": layout_warnings,
        })
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("sample") => sample(args),
        _ => panic!("use: sample <case> <size> <label> <index> <reps>"),
    }
}
