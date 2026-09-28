//! Mutations caught: losing source paint at shaping, splitting GSUB for paint,
//! selecting the wrong shared owner, dropping first-line paint or leaking NaN.
use shodo::geometry::Direction;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, PaintStyle, ParagraphStyle, TextDecoration};
use shodo::{
    AtomicSizes, Fragment, GlyphRunView, LayoutContext, Line, Paragraph, ParagraphBuilder, RichText,
};
use shodo_fixtures::{FONTS, load_fonts};

const RED: [u8; 4] = [200, 10, 20, 128];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 160, 0, 255];
fn style(font: usize, color: [u8; 4]) -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[font].family.into())],
        paint: PaintStyle {
            color,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}
fn runs(line: &Line) -> Vec<GlyphRunView<'_>> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .collect()
}
fn glyphs(lines: &[Line]) -> Vec<(u32, u32, f32, f32)> {
    lines
        .iter()
        .flat_map(runs)
        .flat_map(|r| r.glyphs())
        .map(|g| (g.id, g.cluster, g.inline_position, g.advance))
        .collect()
}

#[test]
fn default_and_explicit_paint_survive_both_builders() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style(0, RED);
    s.paint.underline = Some(TextDecoration {
        color: Some(BLUE),
        offset: Some(3.0),
        thickness: Some(2.0),
    });
    s.paint.strikethrough = Some(TextDecoration {
        color: None,
        offset: Some(-4.0),
        thickness: Some(1.0),
    });
    let root = ParagraphStyle::default();
    let p = RichText::new(&root)
        .push("a", &s)
        .push("b", &InlineStyle::default())
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    let rs = runs(&ls[0]);
    assert_eq!(rs[0].paint_style().color, RED);
    assert_eq!(
        rs[0].paint_style().underline,
        Some(TextDecoration {
            color: Some(BLUE),
            offset: Some(3.0),
            thickness: Some(2.0)
        })
    );
    assert_eq!(
        rs[0].paint_style().strikethrough,
        Some(TextDecoration {
            color: None,
            offset: Some(-4.0),
            thickness: Some(1.0)
        })
    );
    assert_eq!(rs.last().unwrap().paint_style().color, [0, 0, 0, 255]);
    assert_eq!(rs.last().unwrap().paint_style().underline, None);
    assert_eq!(rs.last().unwrap().paint_style().strikethrough, None);
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.open_inline(NodeId(7), &s, Default::default())
        .push_text(TextSource::Generated { node: NodeId(99) }, "a")
        .close_inline();
    let dom = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let domlines = lines(&dom, 1000.0);
    assert_eq!(runs(&domlines[0])[0].node(), Some(NodeId(99)));
    assert_eq!(runs(&domlines[0])[0].paint_style(), rs[0].paint_style());
}

#[test]
fn paint_boundaries_preserve_ffi_owner_and_arabic_joining() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (font, parts, dir) in [
        (0, vec!["f", "f", "i"], Direction::Ltr),
        (2, vec!["س", "ل", "ام"], Direction::Rtl),
    ] {
        let mut a = style(font, RED);
        a.direction = dir;
        a.paint.underline = Some(TextDecoration::default());
        let mut b = a.clone();
        b.paint.color = BLUE;
        b.paint.underline = None;
        b.paint.strikethrough = Some(TextDecoration {
            color: Some(GREEN),
            ..Default::default()
        });
        let root = ParagraphStyle {
            direction: dir,
            root: a.clone(),
            ..Default::default()
        };
        let mut rt = RichText::new(&root);
        for (i, t) in parts.iter().enumerate() {
            rt = rt.push(t, if i == 0 { &a } else { &b });
        }
        let split = rt
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let one = RichText::new(&root)
            .push(&parts.concat(), &a)
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let split = lines(&split, 1000.0);
        let one = lines(&one, 1000.0);
        assert_eq!(
            glyphs(&split),
            glyphs(&one),
            "paint must not split shaping: {parts:?}"
        );
        if font == 0 {
            let rs = runs(&split[0]);
            assert_eq!(rs.len(), 1);
            assert_eq!(rs[0].glyphs().len(), 1);
            assert_eq!(rs[0].text_range(), 0..3);
            assert_eq!(rs[0].node(), Some(NodeId(0)));
            assert_eq!(rs[0].paint_style().color, RED);
            assert!(rs[0].paint_style().underline.is_some());
        }
    }
}

#[test]
fn first_line_retains_resolved_and_legacy_paint() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let normal = style(0, RED);
    let mut first = normal.clone();
    first.paint.color = GREEN;
    first.paint.underline = Some(TextDecoration::default());
    let root = ParagraphStyle {
        root: normal.clone(),
        first_line: Some(first),
        ..Default::default()
    };
    for resolved in [false, true] {
        let mut blue = normal.clone();
        blue.paint.color = BLUE;
        blue.paint.strikethrough = Some(TextDecoration::default());
        let rt = if resolved {
            RichText::new(&root).push_with_first_line("one two three four", &normal, &blue)
        } else {
            RichText::new(&root).push("one two three four", &normal)
        };
        let p = rt
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let ls = lines(&p, 60.0);
        assert!(ls.len() >= 2);
        for r in runs(&ls[0]) {
            assert_eq!(r.paint_style().color, if resolved { BLUE } else { GREEN });
            assert_eq!(r.paint_style().underline.is_some(), !resolved);
            assert_eq!(r.paint_style().strikethrough.is_some(), resolved);
        }
        for r in ls.iter().skip(1).flat_map(runs) {
            assert_eq!(r.paint_style(), &normal.paint);
        }
    }
}

#[test]
fn invalid_decoration_lengths_warn_and_normalize() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut s = style(0, RED);
    s.paint.underline = Some(TextDecoration {
        color: None,
        offset: Some(f32::NAN),
        thickness: Some(f32::INFINITY),
    });
    s.paint.strikethrough = Some(TextDecoration {
        color: None,
        offset: Some(2e7),
        thickness: Some(-1.0),
    });
    let p = RichText::new(&ParagraphStyle::default())
        .push("a", &s)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NonFiniteInput)
    );
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NegativeInput)
    );
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::Saturated)
    );
    let ls = lines(&p, 1000.0);
    let rs = runs(&ls[0]);
    let paint = rs[0].paint_style();
    assert_eq!(paint.underline, Some(TextDecoration::default()));
    assert_eq!(
        paint.strikethrough,
        Some(TextDecoration {
            color: None,
            offset: Some(1e7),
            thickness: Some(0.0)
        })
    );
}
