//! Fixed-font accepted-line workloads shared by the probe and allocation test.
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, LineHeight, LineOptions, ParagraphStyle, VerticalAlign,
};
use shodo::{
    AtomicSizes, LayoutContext, Line, LineConstraint, LineResult, Paragraph, ParagraphBuilder,
};

pub struct Input {
    pub paragraph: Paragraph,
    pub constraint: LineConstraint<'static>,
}

impl Input {
    pub fn line(&self, cx: &mut LayoutContext) -> Line {
        let LineResult::Line(line) = self.paragraph.next_line(
            cx,
            self.paragraph.start_token(),
            &LineOptions::default(),
            &self.constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected an accepted line")
        };
        line
    }
}

pub fn input(runs: usize, reshape: bool) -> Input {
    let limits = Limits::unlimited();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        font_size: 10.0,
        line_height: LineHeight::Px(12.0),
        ..Default::default()
    };
    let style = ParagraphStyle {
        root,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    for n in 0..runs {
        builder.open_inline(
            NodeId((2 * n) as u64),
            &InlineStyle {
                vertical_align: VerticalAlign::Length(if n % 2 == 0 { 2.0 } else { -3.0 }),
                ..style.root.clone()
            },
            Default::default(),
        );
        builder.push_text(
            TextSource::Generated {
                node: NodeId((2 * n + 1) as u64),
            },
            "a",
        );
        builder.close_inline();
    }
    if reshape {
        builder.push_text(
            TextSource::Generated {
                node: NodeId((2 * runs + 1) as u64),
            },
            "hy\u{ad}phenation",
        );
    }
    let paragraph = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(paragraph.warnings().is_empty());
    let mut input = Input {
        paragraph,
        constraint: LineConstraint::new(1_000_000.0),
    };
    if reshape {
        let full = input.line(&mut LayoutContext::new());
        let prefix: f32 = full
            .fragments()
            .filter_map(|f| match f {
                shodo::Fragment::GlyphRun(run)
                    if run.node().is_some_and(|n| n.0 < (2 * runs) as u64) =>
                {
                    Some(run.inline_size())
                }
                _ => None,
            })
            .sum();
        // Fit the prefix plus "hy-", but not the rest of the last word.
        input.constraint = LineConstraint::new(prefix + 16.0);
    }
    input
}
