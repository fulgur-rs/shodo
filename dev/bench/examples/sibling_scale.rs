//! shodo-2j6 probe: `break_all` time for one line of sibling rubies — plain,
//! inside an outer ruby's base, after a top-aligned glyph, in an RTL
//! paragraph — and the breakable `ordinary` and ruby-free `plain` controls.
//!
//! `sample <case> <size> <label> <index> <reps>` prints one JSON line. The
//! paragraph, its output digest and the contexts are prepared outside the
//! timed loop; only `reps` calls of `break_all` are timed.
#[allow(dead_code)]
#[path = "support/geometry_snapshot.rs"]
mod snapshot;

use serde_json::json;
use sha2::{Digest, Sha256};
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle, VerticalAlign};
use shodo::{
    AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use std::hint::black_box;
use std::time::Instant;

/// The unbreakable sibling rows overflow this width as one line.
const INLINE_SIZE: f32 = 96.0;
/// `outer` has break opportunities inside its outer ruby: at this width
/// every candidate fits, so the paragraph is one scanned line.
const WIDE_INLINE_SIZE: f32 = 1.0e7;

#[derive(Clone, Copy)]
enum Case {
    /// `size` sibling rubies over "12": one unbreakable line.
    Siblings,
    /// One outer ruby whose base holds `size` × ("日" + ruby over "12"),
    /// with the reading "日" × `size` (paired cuts inside the base).
    Outer,
    /// A top-aligned 48 px "1" (no break opportunity before the digit
    /// bases, so it shares the line), then `Siblings`.
    Valign,
    /// `Siblings` in an RTL paragraph.
    Rtl,
    /// `size` breakable groups of text "日日" and a ruby over "日日".
    Ordinary,
    /// Ruby-free control: "日" repeated `4 * size` times.
    Plain,
}

impl Case {
    fn parse(name: &str) -> Self {
        match name {
            "siblings" => Self::Siblings,
            "outer" => Self::Outer,
            "valign" => Self::Valign,
            "rtl" => Self::Rtl,
            "ordinary" => Self::Ordinary,
            "plain" => Self::Plain,
            other => panic!("unknown case {other}"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Siblings => "siblings",
            Self::Outer => "outer",
            Self::Valign => "valign",
            Self::Rtl => "rtl",
            Self::Ordinary => "ordinary",
            Self::Plain => "plain",
        }
    }

    fn inline_size(self) -> f32 {
        match self {
            Self::Outer => WIDE_INLINE_SIZE,
            _ => INLINE_SIZE,
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
        direction: if matches!(case, Case::Rtl) {
            Direction::Rtl
        } else {
            Direction::Ltr
        },
        ..Default::default()
    };
    let mut top = ParagraphBuilder::new(&paragraph, limits);
    match case {
        Case::Siblings | Case::Rtl => siblings(&mut top, size, &paragraph, limits),
        Case::Valign => {
            top.open_inline(
                NodeId(7),
                &InlineStyle {
                    vertical_align: VerticalAlign::Top,
                    font_size: 48.0,
                    ..style()
                },
                Default::default(),
            );
            text(&mut top, 8, "1");
            top.close_inline();
            siblings(&mut top, size, &paragraph, limits);
        }
        Case::Outer => {
            let mut base = ParagraphBuilder::new(&paragraph, limits);
            for i in 0..size as u64 {
                text(&mut base, 700_000 + i, "日");
                let mut inner = ParagraphBuilder::new(&paragraph, limits);
                text(&mut inner, 500_000 + i, "12");
                base.push_ruby(
                    NodeId(600_000 + i),
                    &style(),
                    ruby(RubyContent::from_builder(inner), "日", limits),
                );
            }
            top.push_ruby(
                NodeId(9),
                &style(),
                ruby(RubyContent::from_builder(base), &"日".repeat(size), limits),
            );
        }
        Case::Ordinary => {
            for i in 0..size as u64 {
                text(&mut top, 700_000 + 2 * i, "日日");
                let mut base = ParagraphBuilder::new(&paragraph, limits);
                text(&mut base, 700_001 + 2 * i, "日日");
                top.push_ruby(
                    NodeId(800_000 + i),
                    &style(),
                    ruby(RubyContent::from_builder(base), "日", limits),
                );
            }
        }
        Case::Plain => text(&mut top, 1, &"日".repeat(4 * size)),
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
    let inline_size = case.inline_size();
    let (output_sha256, warning_sha256, layout_warnings, lines) =
        signature(&paragraph, inline_size);
    let contexts: Vec<_> = (0..reps).map(|_| LayoutContext::new()).collect();
    let options = LineOptions::default();
    let start = Instant::now();
    for mut context in contexts {
        black_box(paragraph.break_all(&mut context, &options, inline_size, &AtomicSizes::EMPTY));
    }
    let elapsed_ns = start.elapsed().as_nanos();
    println!(
        "{}",
        json!({
            "issue": "shodo-2j6",
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
