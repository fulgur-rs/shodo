use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    BoxDecorationBreak, FontFamily, HangingPunctuation, InlineStyle, LineOptions, ParagraphStyle,
    TextAlign, TextJustify, UnicodeBidi,
};
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
    let mut cases: Vec<_> = [
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
    .collect();
    cases.push(repeated_boxes_case(fonts, limits));
    cases.push(continued_rtl_box_case(fonts, limits));
    cases
}

fn fragment_style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_size: 16.0,
            font_families: ["latin", "cjk", "arabic"]
                .into_iter()
                .map(|id| FontFamily::Named(shodo_fixtures::font(id).unwrap().family.into()))
                .collect(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn repeated_boxes_case(fonts: &FixtureFonts, limits: &Limits) -> Case {
    let style = fragment_style();
    let mut rtl = style.root.clone();
    rtl.direction = shodo::geometry::Direction::Rtl;
    rtl.unicode_bidi = UnicodeBidi::BidiOverride;
    let mut builder = ParagraphBuilder::new(&style, limits);
    for index in 0..32 {
        builder.push_text(
            TextSource::Generated {
                node: NodeId(index * 4),
            },
            "Library ",
        );
        builder.open_inline(NodeId(index * 4 + 1), &rtl, InlineEdges::default());
        builder.push_text(
            TextSource::Generated {
                node: NodeId(index * 4 + 2),
            },
            "مرحبا",
        );
        builder.close_inline();
        builder.push_text(
            TextSource::Generated {
                node: NodeId(index * 4 + 3),
            },
            " 日本語。 ",
        );
    }
    let paragraph = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .expect("inline-box fixture builds");
    Case {
        name: "no-continuation",
        paragraph,
        width: 20_000.0,
        options: LineOptions::default(),
    }
}

fn continued_rtl_box_case(fonts: &FixtureFonts, limits: &Limits) -> Case {
    let mut style = fragment_style();
    style.root.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
    let mut rtl = style.root.clone();
    rtl.direction = shodo::geometry::Direction::Rtl;
    rtl.unicode_bidi = UnicodeBidi::BidiOverride;
    rtl.box_decoration_break = BoxDecorationBreak::Clone;
    let edges = InlineEdges {
        padding: Sides {
            inline_start: 1.0,
            inline_end: 1.0,
            ..Default::default()
        },
        border: Sides {
            inline_start: 0.5,
            inline_end: 0.5,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, limits);
    for (node, text) in [(101, "f"), (102, "f"), (103, "i")] {
        builder.push_text(TextSource::Generated { node: NodeId(node) }, text);
    }
    builder.open_inline(NodeId(100), &rtl, edges);
    builder.push_text(
        TextSource::Generated { node: NodeId(104) },
        &" VAVAVAVA العربية،".repeat(12),
    );
    builder.close_inline();
    builder.push_text(TextSource::Generated { node: NodeId(105) }, "、 ");
    let paragraph = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .expect("continued RTL inline-box fixture builds");
    Case {
        name: "rtl-continuation-hanging",
        paragraph,
        width: 100.0,
        options: LineOptions {
            hanging_punctuation: HangingPunctuation {
                force_end: true,
                last: true,
                ..Default::default()
            },
            ..Default::default()
        },
    }
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
