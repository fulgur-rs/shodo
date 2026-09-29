#![allow(dead_code)]
use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{LineOptions, ParagraphStyle};
use shodo::{
    AtomicSizes, Fragment, Glyph, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

pub fn style() -> ParagraphStyle {
    let mut s = ParagraphStyle::default();
    s.root.font_size = 10.0;
    s
}
pub fn build(style: &ParagraphStyle, input: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(style, &Limits::default());
    input(&mut b);
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        ),
    )
    .unwrap()
}
pub fn paragraph(text: &str) -> Paragraph {
    build(&style(), |b| {
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
    })
}
pub fn first_line(p: &Paragraph, width: f32, options: &LineOptions, atomics: &AtomicSizes) -> Line {
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        options,
        &LineConstraint::new(width),
        atomics,
    ) else {
        panic!("expected line")
    };
    line
}
pub fn glyphs(line: &Line) -> Vec<Glyph> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r.glyphs())
            } else {
                None
            }
        })
        .flatten()
        .collect()
}
