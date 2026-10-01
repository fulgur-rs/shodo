use crate::font::{FontCollection, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::output::clone_probe;
use crate::style::{FontFamily, LineOptions, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};
use std::cell::RefCell;
use std::rc::Rc;

fn paragraph() -> Paragraph {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Noto Sans".into())];
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.with_offset_mapping(true);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 7,
        },
        "ffi abc def ghi jkl mno",
    );
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn internal_break_all_does_not_clone_lines() {
    let p = paragraph();
    clone_probe::reset();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        30.,
        &AtomicSizes::EMPTY,
    );
    assert!(lines.len() > 3);
    assert_eq!(
        clone_probe::count(),
        0,
        "unused owned previous Lines must not be copied"
    );
    assert!(
        lines
            .iter()
            .flat_map(|l| l.fragments())
            .filter_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs())
            .all(|g| g.id != 0)
    );
}

#[test]
fn grapheme_break_all_does_not_clone_lines() {
    let p = paragraph();
    for limit in [0, 1, 3] {
        clone_probe::reset();
        let lines = p.break_all_with_grapheme_limit(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            30.,
            limit,
            &AtomicSizes::EMPTY,
        );
        assert!(lines.len() > 3);
        assert_eq!(
            clone_probe::count(),
            0,
            "grapheme-limited internal path still copies Lines"
        );
        assert_eq!(lines.last().unwrap().text_range().end, p.text().len());
    }
}

fn signature(l: &crate::Line) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        l.text_range(),
        l.break_token(),
        l.metrics(),
        l.block_offset().to_bits(),
        l.offset_mapping(),
        l.fragments().collect::<Vec<_>>(),
        l.fragments()
            .filter_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(
                    r.glyphs()
                        .map(|g| (
                            g.id,
                            g.cluster,
                            g.advance.to_bits(),
                            g.inline_position.to_bits(),
                            g.block_offset.to_bits()
                        ))
                        .collect::<Vec<_>>()
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
    )
}

#[test]
fn public_iterator_keeps_previous_line() {
    let p = paragraph();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let capture = Rc::clone(&observed);
    let mut cx = LayoutContext::new();
    clone_probe::reset();
    let lines = p
        .lines(
            &mut cx,
            p.start_token(),
            &LineOptions::default(),
            move |previous, offset| {
                if let Some(LineResult::Line(l)) = previous {
                    capture.borrow_mut().push(signature(l));
                }
                let mut c = LineConstraint::new(30.);
                c.block_offset = offset;
                c
            },
            &AtomicSizes::EMPTY,
        )
        .filter_map(|r| match r {
            LineResult::Line(l) => Some(l),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(lines.len() > 3);
    assert_eq!(clone_probe::count(), lines.len());
    assert_eq!(
        *observed.borrow(),
        lines.iter().map(signature).collect::<Vec<_>>()
    );
    let copied = lines[0].clone();
    assert_eq!(clone_probe::count(), lines.len() + 1);
    assert_eq!(signature(&copied), signature(&lines[0]));
}
