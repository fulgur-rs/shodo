use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::ruby::{
    Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo::style::{BoxDecorationBreak, FontFamily, InlineStyle, ParagraphStyle};
use shodo::{LayoutContext, ParagraphBuilder};

fn text(node: u64, value: &str, style: &InlineStyle, limits: &Limits) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 7,
        },
        value,
        style,
        limits,
    )
}

fn wrapped_base(column: usize, depth: usize, style: &InlineStyle, limits: &Limits) -> RubyContent {
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, limits);
    builder.with_offset_mapping(true);
    for level in 0..depth {
        let inline = InlineStyle {
            box_decoration_break: if level % 2 == 0 {
                BoxDecorationBreak::Clone
            } else {
                BoxDecorationBreak::Slice
            },
            ..style.clone()
        };
        let mut edges = InlineEdges::default();
        edges.margin.inline_start = -0.125;
        edges.padding.inline_start = 0.25;
        edges.margin.inline_end = -0.125;
        edges.padding.inline_end = 0.375;
        builder.open_inline(
            NodeId(10_000 + column as u64 * 256 + level as u64),
            &inline,
            edges,
        );
    }
    builder.push_text(
        TextSource::Dom {
            node: NodeId(20_000 + column as u64),
            offset: 7,
        },
        "日",
    );
    for _ in 0..depth {
        builder.close_inline();
    }
    RubyContent::from_builder(builder)
}

pub fn paragraph_for_measurement(columns: usize, depth: usize) -> shodo::Paragraph {
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(shodo_fixtures::FONTS[1].family.into())],
        ..Default::default()
    };
    let reading = InlineStyle {
        font_size: 8.0,
        ..root.clone()
    };
    let ruby = Ruby::new(
        (0..columns)
            .map(|column| RubyBase {
                node: NodeId(100 + column as u64),
                content: wrapped_base(column, depth, &root, &limits),
                align: Default::default(),
            })
            .collect(),
        vec![RubyLevel {
            annotations: (0..columns)
                .map(|column| RubyAnnotation {
                    node: NodeId(1_000 + column as u64),
                    content: text(2_000 + column as u64, "に", &reading, &limits),
                    span: RubySpan::Columns(column..column + 1),
                    visibility: RubyVisibility::Visible,
                })
                .collect(),
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.with_offset_mapping(true);
    builder.push_ruby(NodeId(9_999), &root, ruby);
    let mut context = LayoutContext::new();
    builder.build(&mut context, &fonts.collection).unwrap()
}
