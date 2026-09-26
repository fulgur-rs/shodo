mod common;
use common::*;
use shodo::style::{LineOptions, TextAlign, TextAlignLast, TextJustify};
use shodo::{AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult};

#[test]
fn alignment_and_indent_use_available_space() {
    let p = paragraph("ab");
    for (align, expected) in [
        (TextAlign::Start, 0.0),
        (TextAlign::End, 80.0),
        (TextAlign::Center, 40.0),
    ] {
        let o = LineOptions {
            text_align: align,
            ..LineOptions::default()
        };
        assert_eq!(
            glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[0].inline_position,
            expected
        );
    }
    let mut o = LineOptions {
        text_align: TextAlign::Center,
        ..LineOptions::default()
    };
    o.text_indent.length = 10.0;
    let mut c = LineConstraint::new(80.0);
    c.inline_start_offset = 10.0;
    let LineResult::Line(l) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &o,
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(glyphs(&l)[0].inline_position, 45.0);
}

#[test]
fn justification_excludes_hanging_spaces_and_preserves_other_lines() {
    let p = paragraph("a b c");
    let o = LineOptions {
        text_align: TextAlign::Justify,
        ..LineOptions::default()
    };
    let plain = first_line(&p, 40.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    let justified = first_line(&p, 40.0, &o, &AtomicSizes::EMPTY);
    assert_eq!(glyphs(&justified)[2].inline_position, 30.0);
    assert_eq!(glyphs(&plain)[2].inline_position, 20.0);
    assert_eq!(justified.inline_size(), 40.0);
    assert_eq!(glyphs(&justified)[3].advance, 10.0);
}

#[test]
fn last_line_auto_and_explicit_justify() {
    let p = paragraph("a b");
    for (align, last, expected) in [
        (TextAlign::Justify, TextAlignLast::Auto, 20.0),
        (TextAlign::JustifyAll, TextAlignLast::Auto, 90.0),
        (TextAlign::Start, TextAlignLast::Justify, 90.0),
    ] {
        let o = LineOptions {
            text_align: align,
            text_align_last: last,
            ..LineOptions::default()
        };
        assert_eq!(
            glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[2].inline_position,
            expected
        );
    }
}

#[test]
fn cluster_advances_and_random_access_share_adjusted_positions() {
    let p = paragraph("a\u{301}b");
    let o = LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::InterCharacter,
        ..LineOptions::default()
    };
    let l = first_line(&p, 40.0, &o, &AtomicSizes::EMPTY);
    let Fragment::GlyphRun(r) = l.fragments().next().unwrap() else {
        panic!()
    };
    assert_eq!(r.glyphs().get(1), r.glyphs().nth(1));
    let clusters: Vec<_> = r.clusters().collect();
    assert_eq!(clusters.len(), 2);
    assert_eq!(clusters[0].text_range, 0..3);
    assert_eq!(clusters[0].advance, 30.0);
    assert_eq!(clusters[0].shaping_advance, 10.0);
    assert_eq!(glyphs(&l)[1].inline_position, 25.0);
}

#[test]
fn rtl_physical_alignment_and_no_justification() {
    let mut root = style();
    root.direction = shodo::geometry::Direction::Rtl;
    let p = build(&root, |b| {
        b.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "אב",
        );
    });
    for (a, x) in [
        (TextAlign::Start, 0.0),
        (TextAlign::End, 80.0),
        (TextAlign::Left, 80.0),
        (TextAlign::Right, 0.0),
    ] {
        let o = LineOptions {
            text_align: a,
            ..LineOptions::default()
        };
        let l = first_line(&p, 100.0, &o, &AtomicSizes::EMPTY);
        assert_eq!(glyphs(&l)[0].inline_position, x);
    }
    let p = paragraph("a b");
    let o = LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::None,
        ..LineOptions::default()
    };
    assert_eq!(
        glyphs(&first_line(&p, 100.0, &o, &AtomicSizes::EMPTY))[2].inline_position,
        20.0
    );
}
