use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, OverflowWrap, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};
use shodo_bench::Run;
use shodo_fixtures::FixtureFonts;

pub const RUNS: usize = 64;

pub struct Case {
    pub name: &'static str,
    text: String,
    style: ParagraphStyle,
}

pub fn cases() -> Vec<Case> {
    let font = FontFamily::Named(
        shodo_fixtures::font("latin")
            .expect("fixed Latin fixture")
            .family
            .into(),
    );
    [
        ("fi-splits", "fi".repeat(RUNS), OverflowWrap::Anywhere),
        ("ffi-splits", "ffi".repeat(RUNS), OverflowWrap::Anywhere),
        (
            "unsplit-control",
            "office ".repeat(RUNS),
            OverflowWrap::Normal,
        ),
    ]
    .into_iter()
    .map(|(name, text, overflow_wrap)| {
        let root = InlineStyle {
            font_families: vec![font.clone()],
            font_size: 16.0,
            lang: Some("en".into()),
            overflow_wrap,
            ..Default::default()
        };
        Case {
            name,
            text,
            style: ParagraphStyle {
                root,
                ..Default::default()
            },
        }
    })
    .collect()
}

pub fn build(case: &Case, limits: &Limits, fonts: &FixtureFonts) -> Paragraph {
    let mut builder = ParagraphBuilder::new(&case.style, limits);
    builder.push_text(
        TextSource::Generated {
            node: NodeId(8_560_000),
        },
        &case.text,
    );
    builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .expect("fixed reshape allocation workload builds")
}

pub fn layout(paragraph: &Paragraph) -> Run {
    let mut cx = LayoutContext::new();
    let constraint = LineConstraint::new(100_000.0);
    let mut token = paragraph.start_token();
    let mut run = Run::default();
    loop {
        match paragraph.next_line(
            &mut cx,
            token,
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                assert_ne!(
                    line.break_token(),
                    token,
                    "reshape workload made no progress"
                );
                token = line.break_token();
                run.lines.push(line);
            }
            LineResult::Done => return run,
            other => panic!("unexpected reshape workload result: {other:?}"),
        }
    }
}

pub fn digest(run: &Run, fonts: &FixtureFonts) -> String {
    shodo_bench::digest(run, fonts).unwrap().sha256
}
