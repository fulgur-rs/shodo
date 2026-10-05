//! shodo-b7d probe: `break_all` time with preserved tabs (`white-space: pre`,
//! `tab-size: 40px`): nested rubies with and without a tab in the innermost
//! content, sibling rubies with and without a leading tab, and a ruby-free
//! control with a tab every four characters.
//!
//! `sample <case> <size> <label> <index> <reps>` prints one JSON line. The
//! paragraph, its output digest and the contexts are prepared outside the
//! timed loop; only `reps` calls of `break_all` are timed. Each result and
//! context is dropped inside the timed range.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineOptions, ParagraphStyle, TabSize, WhiteSpaceCollapse,
};
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
    /// As `Nested`, innermost "日\t日".
    NestedTab,
    /// `size` sibling rubies over "12": one unbreakable line.
    Siblings,
    /// As `Siblings` after a leading "\t".
    SiblingsTab,
    /// Ruby-free control: "日日日\t" repeated `size` times.
    Plain,
}

impl Case {
    fn parse(name: &str) -> Self {
        match name {
            "nested" => Self::Nested,
            "nestedtab" => Self::NestedTab,
            "siblings" => Self::Siblings,
            "siblingstab" => Self::SiblingsTab,
            "plain" => Self::Plain,
            other => panic!("unknown case {other}"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Nested => "nested",
            Self::NestedTab => "nestedtab",
            Self::Siblings => "siblings",
            Self::SiblingsTab => "siblingstab",
            Self::Plain => "plain",
        }
    }
}

fn style() -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..Default::default()
    }
}

/// The fixed CJK font has "日" and digits; other kana/kanji may be missing,
/// which the geometry snapshot rejects.
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

/// `size` rubies over "12" pushed into `b`.
fn siblings(b: &mut ParagraphBuilder, size: usize, paragraph: &ParagraphStyle, limits: &Limits) {
    for i in 0..size as u64 {
        let mut base = ParagraphBuilder::new(paragraph, limits);
        text(&mut base, 500_000 + i, "12");
        b.push_ruby(
            NodeId(600_000 + i),
            &style(),
            ruby(RubyContent::from_builder(base), "日", limits),
        );
    }
}

fn builder(case: Case, size: usize, limits: &Limits) -> ParagraphBuilder {
    let paragraph = ParagraphStyle {
        root: style(),
        ..Default::default()
    };
    let mut top = ParagraphBuilder::new(&paragraph, limits);
    match case {
        Case::Siblings => siblings(&mut top, size, &paragraph, limits),
        Case::SiblingsTab => {
            text(&mut top, 1, "\t");
            siblings(&mut top, size, &paragraph, limits);
        }
        Case::Nested | Case::NestedTab => {
            let mut content = ParagraphBuilder::new(&paragraph, limits);
            let inner = if matches!(case, Case::NestedTab) {
                "日\t日"
            } else {
                "日"
            };
            text(&mut content, 1, inner);
            for _ in 0..size {
                let r = ruby(RubyContent::from_builder(content), "日", limits);
                content = ParagraphBuilder::new(&paragraph, limits);
                content.push_ruby(NodeId(4), &style(), r);
            }
            top = content;
        }
        Case::Plain => text(&mut top, 1, &"日日日\t".repeat(size)),
    }
    top
}

fn warnings(list: &[shodo::limits::Warning]) -> Vec<String> {
    list.iter()
        .map(|w| format!("{:?}:{}", w.kind, w.message))
        .collect()
}

fn signature(paragraph: &Paragraph, inline_size: f32) -> (String, String, Vec<String>, usize) {
    let mut context = LayoutContext::new();
    let lines = paragraph.break_all(
        &mut context,
        &LineOptions::default(),
        inline_size,
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
    let inline_size = INLINE_SIZE;
    let (output_sha256, warning_sha256, layout_warnings, lines) =
        signature(&paragraph, inline_size);
    let contexts: Vec<_> = (0..reps).map(|_| LayoutContext::new()).collect();
    let options = LineOptions::default();
    // Each result and context is dropped inside the timed range, as in the
    // shodo-2j6 samples; the release works against the faster build.
    let start = Instant::now();
    for mut context in contexts {
        black_box(paragraph.break_all(&mut context, &options, inline_size, &AtomicSizes::EMPTY));
    }
    let elapsed_ns = start.elapsed().as_nanos();
    println!(
        "{}",
        json!({
            "issue": "shodo-b7d",
            "case": case.name(),
            "size": size,
            "inline_size": inline_size,
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
