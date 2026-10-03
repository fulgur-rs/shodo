use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::ruby::{
    Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSizes, LayoutContext, Line, ParagraphBuilder};

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        value,
        style,
        limits,
    )
}

fn nested_ruby(
    depth: usize,
    top_depth: usize,
    hide_top: bool,
    base_chars: usize,
    style: &InlineStyle,
    limits: &Limits,
) -> Ruby {
    let base_char = if depth % 2 == top_depth % 2 { 'A' } else { 'C' };
    let base_text = base_char.to_string().repeat(base_chars);
    let reading = if depth == 1 {
        let reading_text = "B".repeat(base_chars);
        text(20_000 + depth as u64, &reading_text, style, limits)
    } else {
        let paragraph_style = ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
        builder.with_offset_mapping(true);
        builder.push_ruby(
            NodeId(30_000 + depth as u64),
            style,
            nested_ruby(depth - 1, top_depth, false, base_chars, style, limits),
        );
        RubyContent::from_builder(builder)
    };

    Ruby::new(
        vec![RubyBase {
            node: NodeId(10_000 + depth as u64),
            content: text(11_000 + depth as u64, &base_text, style, limits),
            align: Default::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(12_000 + depth as u64),
                content: reading,
                span: RubySpan::Auto,
                visibility: if depth == top_depth && hide_top {
                    RubyVisibility::Hidden
                } else {
                    RubyVisibility::Visible
                },
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap()
}

fn paragraph_style() -> InlineStyle {
    InlineStyle {
        font_size: 20.0,
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        ..Default::default()
    }
}

fn fonts(limits: &Limits) -> FontCollection {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn finish(builder: ParagraphBuilder, fonts: &FontCollection) -> (Vec<Line>, String) {
    let mut context = LayoutContext::new();
    let paragraph = builder.build(&mut context, fonts).unwrap();
    let build_warnings = format!("{:?}", paragraph.warnings());
    let lines = paragraph.break_all(
        &mut context,
        &Default::default(),
        100_000.0,
        &AtomicSizes::EMPTY,
    );
    (
        lines,
        format!("{build_warnings}/{:?}", context.take_warnings()),
    )
}

pub fn nested(depth: usize, base_chars: usize, hide_top: bool) -> Vec<Line> {
    assert!(depth > 0);
    let limits = Limits::default();
    let fonts = fonts(&limits);
    let style = paragraph_style();
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.with_offset_mapping(true);
    builder.push_ruby(
        NodeId(19_999),
        &style,
        nested_ruby(depth, depth, hide_top, base_chars, &style, &limits),
    );
    let (lines, warnings) = finish(builder, &fonts);
    assert_eq!(lines.len(), 1, "fixture text must remain on a single line");
    assert_eq!(
        warnings, "[]/[]",
        "fixture depth={depth}, hide_top={hide_top} should build without warnings"
    );
    assert_eq!(lines[0].ruby_annotations().len(), 1);
    assert_eq!(
        visible_depth(&lines),
        if hide_top { 0 } else { depth },
        "fixture should retain the requested visible nesting depth"
    );
    lines
}

pub fn visible_depth(lines: &[Line]) -> usize {
    lines
        .iter()
        .flat_map(|line| line.ruby_annotations())
        .filter(|annotation| annotation.visibility() == RubyVisibility::Visible)
        .map(|annotation| 1 + visible_depth(std::slice::from_ref(annotation.line())))
        .max()
        .unwrap_or(0)
}
