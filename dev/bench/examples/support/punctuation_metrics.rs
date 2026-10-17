use sha2::{Digest, Sha256};
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
};
use shodo_bench::Run;
use shodo_fixtures::FixtureFonts;

const RUNS: usize = 64;

struct Span {
    style: InlineStyle,
    text: &'static str,
}

pub struct Case {
    pub name: &'static str,
    style: ParagraphStyle,
    spans: Vec<Span>,
}

pub fn cases() -> Vec<Case> {
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(
            shodo_fixtures::font("cjk")
                .expect("fixed CJK fixture")
                .family
                .into(),
        )],
        font_size: 20.0,
        lang: Some("ja".into()),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    [("repeated-opening", "「"), ("repeated-closing-quote", "」")]
        .into_iter()
        .map(|(name, text)| Case {
            name,
            style: style.clone(),
            spans: (0..RUNS)
                .map(|index| Span {
                    style: InlineStyle {
                        lang: Some(if index % 2 == 0 { "ja" } else { "en" }.into()),
                        ..root.clone()
                    },
                    text,
                })
                .collect(),
        })
        .collect()
}

pub fn build(case: &Case, limits: &Limits, fonts: &FixtureFonts) -> Paragraph {
    let mut builder = ParagraphBuilder::new(&case.style, limits);
    for (index, span) in case.spans.iter().enumerate() {
        let node = NodeId(2_000_000 + index as u64 * 2);
        builder
            .open_inline(NodeId(node.0), &span.style, InlineEdges::default())
            .push_text(TextSource::Generated { node }, span.text)
            .close_inline();
    }
    builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .expect("fixed CJK punctuation workload builds")
}

pub fn layout(paragraph: &Paragraph) -> Run {
    let mut cx = LayoutContext::new();
    let constraint = LineConstraint::new(100_000.0);
    let options = LineOptions::default();
    let mut token = paragraph.start_token();
    let mut run = Run::default();
    loop {
        match paragraph.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY) {
            LineResult::Line(line) => {
                assert_ne!(
                    line.break_token(),
                    token,
                    "punctuation layout made no progress"
                );
                token = line.break_token();
                run.lines.push(line);
            }
            LineResult::Done => return run,
            other => panic!("unexpected punctuation layout result: {other:?}"),
        }
    }
}

fn add_usize(hash: &mut Sha256, value: usize) {
    hash.update((value as u64).to_le_bytes());
}

fn add_f32(hash: &mut Sha256, value: f32) {
    hash.update(value.to_bits().to_le_bytes());
}

pub fn digest(run: &Run, _fonts: &FixtureFonts) -> String {
    let mut hash = Sha256::new();
    add_usize(&mut hash, run.lines.len());
    for line in &run.lines {
        hash.update(line.text().as_bytes());
        add_usize(&mut hash, line.text_range().start);
        add_usize(&mut hash, line.text_range().end);
        for value in [
            line.inline_size(),
            line.block_size(),
            line.block_offset(),
            line.hang_start(),
            line.hang_end(),
        ] {
            add_f32(&mut hash, value);
        }
        for fragment in line.fragments() {
            match fragment {
                Fragment::GlyphRun(glyph_run) => {
                    hash.update(b"glyph-run");
                    add_usize(&mut hash, glyph_run.font().layer() as usize);
                    add_usize(&mut hash, glyph_run.font().index() as usize);
                    add_usize(&mut hash, glyph_run.text_range().start);
                    add_usize(&mut hash, glyph_run.text_range().end);
                    add_f32(&mut hash, glyph_run.font_size());
                    for (index, glyph) in glyph_run.glyphs().enumerate() {
                        add_usize(&mut hash, glyph.id as usize);
                        add_usize(&mut hash, glyph.cluster as usize);
                        add_f32(&mut hash, glyph.inline_position);
                        add_f32(&mut hash, glyph.block_offset);
                        add_f32(&mut hash, glyph.advance);
                        if let Some((x, y)) = glyph_run.glyph_origin(index) {
                            add_f32(&mut hash, x);
                            add_f32(&mut hash, y);
                        }
                    }
                }
                other => {
                    hash.update(format!("{other:?}").as_bytes());
                }
            }
        }
    }
    format!("{:x}", hash.finalize())
}

pub fn run_count() -> usize {
    RUNS
}
