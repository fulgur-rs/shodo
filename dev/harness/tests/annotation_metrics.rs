//! Literal fixed-font geometry for caller-managed annotation spacing.
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, ParagraphStyle, TextEmphasis, TextEmphasisPosition,
    TextEmphasisShape,
};
use shodo::{
    AtomicSizes, LayoutContext, Line, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase,
    RubyContent, RubyLevel, RubyPosition, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_fixtures::{FONTS, load_fonts};

fn style(family: usize, size: f32) -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[family].family.into())],
        font_size: size,
        line_height: LineHeight::Px(size),
        ..Default::default()
    }
}
fn content(node: u64, text: &str, style: &InlineStyle) -> RubyContent {
    RubyContent::text(
        TextSource::Generated { node: NodeId(node) },
        text,
        style,
        &Limits::default(),
    )
}
fn layout(builder: ParagraphBuilder) -> Vec<Line> {
    let fonts = load_fonts(&Limits::default()).unwrap();
    builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.,
            &AtomicSizes::EMPTY,
        )
}
fn mark(position: TextEmphasisPosition) -> Option<TextEmphasis> {
    Some(TextEmphasis {
        shape: TextEmphasisShape::Dot,
        filled: true,
        position,
    })
}

#[test]
fn original_four_pixel_latin_lines_have_fixed_point_geometry_without_spare_leading() {
    let mut inline = style(0, 10.);
    inline.line_height = LineHeight::Px(4.);
    inline.text_emphasis = mark(TextEmphasisPosition::OverRight);
    let root = ParagraphStyle {
        root: inline,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&root, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
    b.push_forced_break(NodeId(2));
    b.push_text(TextSource::Generated { node: NodeId(3) }, "ab");
    let lines = layout(b);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let m = line.annotation_metrics();
        assert_eq!(line.block_size(), 11.8125);
        assert_eq!(
            (m.unannotated_block_start, m.unannotated_block_end),
            (7.8125, 11.828125)
        );
        assert_eq!(
            (
                m.overflow_over,
                m.overflow_under,
                m.space_over,
                m.space_under
            ),
            (7.8125, 0., 0., 0.)
        );
    }
    assert_eq!(lines.iter().map(|l| l.block_size()).sum::<f32>(), 23.625);
    let mut plain = root.clone();
    plain.root.text_emphasis = None;
    let mut b = ParagraphBuilder::new(&plain, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
    let bare = layout(b).remove(0);
    assert_eq!(bare.block_size(), 4.015625);
    let m = lines[0].annotation_metrics();
    assert_eq!(
        m.unannotated_block_end - m.unannotated_block_start,
        bare.block_size()
    );
}

#[test]
fn independently_rounded_bare_box_does_not_expose_space_past_the_accepted_edge() {
    let mut inline = style(0, 10.);
    inline.line_height = LineHeight::Px(14.00390625);
    inline.text_emphasis = mark(TextEmphasisPosition::OverRight);
    let root = ParagraphStyle {
        root: inline,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&root, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
    let line = layout(b).remove(0);
    let m = line.annotation_metrics();
    assert_eq!(line.block_size(), 16.8125);
    // The fixed Latin descent is 2.9375px at 10px. Spare layout space must
    // fit the accepted line even if the independent bare box rounded farther.
    assert_eq!(m.space_under, 0.1875);
    assert!(m.space_under <= line.block_size() - line.metrics().baseline - 2.9375);
}

#[test]
fn hidden_ruby_and_marks_reserve_sides_while_collapsed_ruby_does_not() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for visibility in [
            RubyVisibility::Visible,
            RubyVisibility::Hidden,
            RubyVisibility::Collapse,
        ] {
            for side in [RubyPosition::Over, RubyPosition::Under] {
                let mut base = style(1, 24.);
                base.text_emphasis = mark(TextEmphasisPosition::OverRight);
                let reading = style(1, 12.);
                let ruby = Ruby::new(
                    vec![RubyBase {
                        node: NodeId(10),
                        content: content(10, "日", &base),
                        align: RubyAlign::Start,
                    }],
                    vec![RubyLevel {
                        annotations: vec![RubyAnnotation {
                            node: NodeId(20),
                            content: content(20, "に", &reading),
                            span: RubySpan::All,
                            visibility,
                        }],
                        style: RubyStyle {
                            position: side,
                            overhang: shodo::RubyOverhang::None,
                            ..Default::default()
                        },
                    }],
                )
                .unwrap();
                let root = ParagraphStyle {
                    root: style(1, 24.),
                    writing_mode: mode,
                    ..Default::default()
                };
                let mut b = ParagraphBuilder::new(&root, &Limits::default());
                b.push_ruby(NodeId(8), &style(1, 24.), ruby);
                let lines = layout(b);
                let line = &lines[0];
                let m = line.annotation_metrics();
                let expected = if visibility == RubyVisibility::Collapse {
                    (12., 0.)
                } else if side == RubyPosition::Over {
                    (24., 0.)
                } else {
                    (12., 12.)
                };
                assert_eq!(
                    (m.overflow_over, m.overflow_under),
                    expected,
                    "{mode:?} {visibility:?} {side:?}"
                );
                assert_eq!(m.unannotated_block_end - m.unannotated_block_start, 24.);
                assert_eq!((m.space_over, m.space_under), (0., 0.));
                assert_eq!(line.block_size(), 24. + expected.0 + expected.1);
            }
        }
    }
}
