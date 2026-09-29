use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{LineHeight, LineOptions, ParagraphStyle, TextAlign, TextAlignLast};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

#[test]
fn line_height_only_boundaries_preserve_real_kerning_and_ligatures() {
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    for (left, right) in [("A", "V"), ("f", "fi")] {
        let style = ParagraphStyle::default();
        let mut child = style.root.clone();
        child.line_height = LineHeight::Px(30.0);
        let mut split = ParagraphBuilder::new(&style, &Limits::default());
        split
            .push_text(TextSource::Generated { node: NodeId(1) }, left)
            .open_inline(NodeId(2), &child, Default::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, right)
            .close_inline();
        let mut whole = ParagraphBuilder::new(&style, &Limits::default());
        whole.push_text(
            TextSource::Generated { node: NodeId(1) },
            &format!("{left}{right}"),
        );
        let actual = split
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            );
        let expected = whole
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            );
        let glyphs = |line: &shodo::Line| {
            line.fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .flat_map(|r| r.glyphs().map(|g| (g.id, g.advance.to_bits())))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            glyphs(&actual[0]),
            glyphs(&expected[0]),
            "{left}|{right}: line-height does not change shaping"
        );
        assert_eq!(actual[0].inline_size(), expected[0].inline_size());
        assert!(
            actual[0].block_size() >= 30.0,
            "child retains its line-height"
        );
        assert_eq!(actual[0].text_range(), 0..left.len() + right.len());
    }
}

#[test]
fn opposite_plaintext_direction_centers_and_justifies_inside_available_width() {
    use shodo::geometry::Direction;
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    for (direction, text) in [(Direction::Ltr, "א ב"), (Direction::Rtl, "a b")] {
        let style = ParagraphStyle {
            direction,
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        for align in [TextAlign::Justify, TextAlign::Center, TextAlign::JustifyAll] {
            let options = LineOptions {
                text_align: align,
                text_align_last: if align == TextAlign::Justify {
                    TextAlignLast::Justify
                } else {
                    TextAlignLast::Auto
                },
                ..Default::default()
            };
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &options,
                100.0,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(lines.len(), 1);
            let start = lines[0]
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .flat_map(|r| r.glyphs().map(|g| g.inline_position))
                .fold(f32::INFINITY, f32::min);
            let expected = if align == TextAlign::Center {
                (100.0 - lines[0].inline_size()) / 2.0
            } else {
                0.0
            };
            assert!(
                (start - expected).abs() <= 1.0 / 64.0,
                "{direction:?}, {align:?}: start {start}, expected {expected}"
            );
            if align != TextAlign::Center {
                assert_eq!(lines[0].inline_size(), 100.0);
            }
        }
    }
}
