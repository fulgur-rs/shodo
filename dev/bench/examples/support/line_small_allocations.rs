use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, LineOptions, ParagraphStyle, TextAlign, TextJustify};
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};
use shodo_bench::Run;
use shodo_fixtures::FixtureFonts;

pub struct Case {
    pub name: &'static str,
    paragraph: Paragraph,
    width: f32,
    options: LineOptions,
}

pub fn cases(fonts: &FixtureFonts, limits: &Limits) -> Vec<Case> {
    [
        ("uniform-ltr", "japanese-long", TextJustify::InterCharacter),
        ("uniform-rtl", "arabic-long", TextJustify::InterWord),
        ("mixed-levels", "mixed-scripts", TextJustify::InterCharacter),
    ]
    .into_iter()
    .map(|(name, fixture_id, text_justify)| {
        let fixture = shodo_fixtures::case(fixture_id).expect("fixed fixture case");
        let families = fixture
            .font_ids
            .iter()
            .map(|id| FontFamily::Named(shodo_fixtures::font(id).unwrap().family.into()))
            .collect();
        let style = ParagraphStyle {
            direction: fixture.direction,
            root: shodo::style::InlineStyle {
                direction: fixture.direction,
                lang: fixture.lang.clone(),
                font_size: fixture.font_size,
                font_families: families,
                ..Default::default()
            },
            ..Default::default()
        };
        let repetitions = if fixture_id == "mixed-scripts" { 24 } else { 2 };
        let source = fixture.text.repeat(repetitions);
        let mut builder = ParagraphBuilder::new(&style, limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, &source);
        let paragraph = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .expect("fixed fixture paragraph builds");
        Case {
            name,
            paragraph,
            width: fixture.width,
            options: LineOptions {
                text_align: TextAlign::JustifyAll,
                text_justify,
                ..Default::default()
            },
        }
    })
    .collect()
}

pub fn layout(case: &Case) -> Run {
    let mut cx = LayoutContext::new();
    let mut token = case.paragraph.start_token();
    let mut run = Run::default();
    for _ in 0..4096 {
        match case.paragraph.next_line(
            &mut cx,
            token,
            &case.options,
            &LineConstraint::new(case.width),
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Line(line) => {
                assert_ne!(line.break_token(), token, "{} made no progress", case.name);
                token = line.break_token();
                run.lines.push(line);
            }
            LineResult::Done => return run,
            other => panic!("{} returned unexpected result {other:?}", case.name),
        }
    }
    panic!("{} exceeded the bounded line count", case.name)
}

pub fn digest(run: &Run, fonts: &FixtureFonts) -> String {
    shodo_bench::digest(run, fonts).unwrap().sha256
}
