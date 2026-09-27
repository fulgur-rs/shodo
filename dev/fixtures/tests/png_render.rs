#[path = "../examples/support/glyph_paint.rs"]
mod glyph_paint;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSize, AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

#[test]
fn atomic_border_rectangle_is_painted_with_accepted_line_block_offset() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 32.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    )
    .push_forced_break(NodeId(88))
    .push_atomic(NodeId(9), &style, Default::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(2),
            offset: 0,
        },
        "b",
    );
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(9),
        AtomicSize {
            inline_size: 30.0,
            block_size: 20.0,
            baseline: Some(20.0),
            margins: Default::default(),
        },
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &atomics,
    );
    assert_eq!(lines.len(), 2);
    assert!(lines[1].block_offset() > 0.0);
    let atomic = lines[1]
        .fragments()
        .find_map(|f| match f {
            Fragment::Atomic(a) => Some(a),
            _ => None,
        })
        .unwrap();
    assert_eq!(atomic.border_rect.inline_size, 30.0);
    assert_eq!(atomic.border_rect.block_size, 20.0);
    let (image, count) = glyph_paint::try_paint(
        &lines,
        |owner| {
            if owner == NodeId(9) {
                [0, 128, 0, 255]
            } else {
                [0, 0, 0, 255]
            }
        },
        &[],
    )
    .unwrap();
    assert_eq!(count, 2);
    let x = (10.0 + atomic.border_rect.inline_start + 15.0) as usize;
    let y = (10.0 + lines[1].block_offset() + atomic.border_rect.block_start + 10.0) as usize;
    let index = 4 * (y * image.width() as usize + x);
    assert_eq!(&image.data()[index..index + 4], &[0, 128, 0, 255]);
    assert!(
        image
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
    );
}

#[test]
fn unsupported_synthetic_weight_is_reported_instead_of_silently_ignored() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_weight: 700.0,
        font_size: 32.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style,
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &AtomicSizes::EMPTY,
    );
    assert!(
        lines[0]
            .fragments()
            .any(|f| matches!(f,Fragment::GlyphRun(r) if r.embolden()))
    );
    assert!(matches!(
        glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]),
        Err(glyph_paint::PaintError::UnsupportedSynthesis)
    ));
}

#[test]
fn fallback_stub_is_reported_instead_of_drawing_invented_outlines() {
    let limits = Default::default();
    let fonts = shodo::font::FontCollection::with_options(
        &limits,
        shodo::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    );
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]).map(|(_, count)| count),
        Err(glyph_paint::PaintError::MissingOutline)
    );
}
