//! Fixed-face geometry matrix for auxiliary emphasis measurements.
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle, TextCombineUpright,
    TextEmphasis, TextEmphasisPosition, TextEmphasisShape, VerticalAlign,
};
use shodo::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder, Ruby,
    RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyVisibility,
};
pub fn accepted(
    mode: WritingMode,
    marked: bool,
    grouped: bool,
    combined: bool,
    ruby: bool,
) -> Line {
    let limits = Limits::unlimited();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![
            FontFamily::Named("Shodo Fixture Latin".into()),
            FontFamily::Named("Shodo Fixture CJK".into()),
        ],
        font_size: 16.0,
        line_height: LineHeight::Px(18.0),
        text_emphasis: marked.then_some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        }),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root,
        writing_mode: mode,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    for (n, size, align) in [
        (0, 31.0, VerticalAlign::Top),
        (1, 11.0, VerticalAlign::Bottom),
        (2, 22.0, VerticalAlign::Length(2.125)),
    ] {
        let child = InlineStyle {
            font_size: size,
            line_height: LineHeight::Px(size + 3.0),
            vertical_align: if grouped {
                align
            } else {
                VerticalAlign::Length(2.125)
            },
            text_combine_upright: if combined {
                TextCombineUpright::All
            } else {
                TextCombineUpright::None
            },
            ..style.root.clone()
        };
        b.open_inline(NodeId(10 + n), &child, Default::default());
        b.push_text(
            TextSource::Generated {
                node: NodeId(20 + n),
            },
            "12",
        );
        b.close_inline();
    }
    if ruby {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(30),
                align: Default::default(),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(31) },
                    "日",
                    &style.root,
                    &limits,
                ),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(32),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(33) },
                        "日日",
                        &InlineStyle {
                            font_size: 9.0,
                            ..style.root.clone()
                        },
                        &limits,
                    ),
                }],
                style: Default::default(),
            }],
        )
        .unwrap();
        b.push_ruby(NodeId(34), &style.root, ruby);
    }
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(p.warnings().is_empty());
    let mut cx = LayoutContext::new();
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert!(cx.take_warnings().is_empty());
    line
}
