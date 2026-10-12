//! Fixed-face multi-run input for auxiliary emphasis metrics measurements.
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle, TextEmphasis,
    TextEmphasisPosition, TextEmphasisShape, VerticalAlign,
};
use shodo::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
};

pub struct Input {
    pub paragraph: Paragraph,
}
impl Input {
    pub fn line(&self, cx: &mut LayoutContext) -> Line {
        let LineResult::Line(line) = self.paragraph.next_line(
            cx,
            self.paragraph.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(1_000_000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected accepted line")
        };
        line
    }
}
pub fn input(runs: usize, marked: bool) -> Input {
    let limits = Limits::unlimited();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        font_size: 10.0,
        line_height: LineHeight::Px(12.0),
        text_emphasis: marked.then_some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position: TextEmphasisPosition::OverRight,
        }),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    for n in 0..runs {
        b.open_inline(
            NodeId((2 * n) as u64),
            &InlineStyle {
                vertical_align: VerticalAlign::Length(if n % 2 == 0 { 2.0 } else { -3.0 }),
                ..style.root.clone()
            },
            Default::default(),
        );
        b.push_text(
            TextSource::Generated {
                node: NodeId((2 * n + 1) as u64),
            },
            "a",
        );
        b.close_inline();
    }
    let paragraph = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(paragraph.warnings().is_empty());
    Input { paragraph }
}
