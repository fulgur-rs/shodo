//! Emphasis marks reserve line-box extent on their line-relative side.

mod common;

use common::*;
use shodo::AtomicSizes;
use shodo::geometry::{BaselineKind, WritingMode};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    LineHeight, LineOptions, ParagraphStyle, TextEmphasis, TextEmphasisPosition, TextEmphasisShape,
};

fn emphasis(position: TextEmphasisPosition) -> Option<TextEmphasis> {
    Some(TextEmphasis {
        shape: TextEmphasisShape::Sesame,
        filled: true,
        position,
    })
}

/// Block size and alphabetic baseline of one line of `text` in `root`.
fn measure(root: &ParagraphStyle, text: &str) -> (f32, f32) {
    let p = build(root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    });
    let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    (l.block_size(), l.baseline(BaselineKind::Alphabetic))
}

#[test]
fn marks_extend_the_root_line_on_their_side() {
    // The fixture font has 8px ascent and 2px descent at 10px; marks are 5px.
    assert_eq!(measure(&style(), "ab"), (10.0, 8.0));
    let mut root = style();
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    assert_eq!(measure(&root, "ab"), (15.0, 13.0));
    root.root.text_emphasis = emphasis(TextEmphasisPosition::UnderRight);
    assert_eq!(measure(&root, "ab"), (15.0, 8.0));
}

#[test]
fn half_leading_absorbs_marks() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(40.0);
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    assert_eq!(measure(&root, "ab"), (40.0, 23.0));
    // With negative half-leading, both sides fall back to the font extents,
    // and the marked side adds the mark.
    root.root.line_height = LineHeight::Px(4.0);
    assert_eq!(measure(&root, "ab"), (15.0, 13.0));
    root.root.text_emphasis = None;
    assert_eq!(measure(&root, "ab"), (4.0, 5.0));
}

#[test]
fn emphasized_inline_extends_the_line() {
    let root = style();
    let mut child = root.root.clone();
    child.text_emphasis = emphasis(TextEmphasisPosition::OverLeft);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b")
            .close_inline();
    });
    let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(
        (l.block_size(), l.baseline(BaselineKind::Alphabetic)),
        (15.0, 13.0)
    );
    // An empty emphasized inline still sizes the line with its strut.
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .close_inline();
    });
    let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 15.0);
}

#[test]
fn vertical_marks_use_the_left_or_right_keyword() {
    for (mode, position, over) in [
        (
            WritingMode::VerticalRl,
            TextEmphasisPosition::OverRight,
            true,
        ),
        (
            WritingMode::VerticalRl,
            TextEmphasisPosition::UnderRight,
            true,
        ),
        (
            WritingMode::VerticalRl,
            TextEmphasisPosition::OverLeft,
            false,
        ),
        (
            WritingMode::VerticalLr,
            TextEmphasisPosition::UnderRight,
            true,
        ),
        (
            WritingMode::VerticalLr,
            TextEmphasisPosition::UnderLeft,
            false,
        ),
    ] {
        let mut root = style();
        root.writing_mode = mode;
        let (plain_size, plain_baseline) = measure(&root, "ab");
        root.root.text_emphasis = emphasis(position);
        let (size, baseline) = measure(&root, "ab");
        assert_eq!(size, plain_size + 5.0, "{mode:?} {position:?}");
        // Line-over is block-start in vertical-rl and block-end in vertical-lr.
        let shifted = over == (mode == WritingMode::VerticalRl);
        let expected = plain_baseline + if shifted { 5.0 } else { 0.0 };
        assert_eq!(baseline, expected, "{mode:?} {position:?}");
    }
}

#[test]
fn combined_text_reserves_its_marks() {
    let mut root = style();
    root.writing_mode = WritingMode::VerticalRl;
    let mut combined = root.root.clone();
    combined.text_combine_upright = shodo::style::TextCombineUpright::All;
    let line = |combined: &shodo::style::InlineStyle| {
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            b.open_inline(NodeId(2), combined, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "12")
                .close_inline();
        });
        let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(l.text_combinations().len(), 1);
        (l.block_size(), l.baseline(BaselineKind::Central))
    };
    let (plain, plain_baseline) = line(&combined);
    combined.text_emphasis = emphasis(TextEmphasisPosition::UnderRight);
    assert_eq!(line(&combined), (plain + 5.0, plain_baseline + 5.0));
}

#[test]
fn runs_describe_their_marks() {
    let marks = |root: &ParagraphStyle| {
        let p = build(root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
        });
        let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        l.fragments()
            .filter_map(|f| match f {
                shodo::Fragment::GlyphRun(run) => Some(run.emphasis_mark()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let mut root = style();
    assert_eq!(marks(&root), [None]);
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    assert_eq!(
        marks(&root),
        [Some(shodo::EmphasisMark {
            character: '\u{FE45}',
            font_size: 5.0,
            line_over: true,
            offset: 8.0,
        })]
    );
    root.root.text_emphasis = Some(TextEmphasis {
        shape: TextEmphasisShape::Custom('*'),
        filled: false,
        position: TextEmphasisPosition::UnderLeft,
    });
    assert_eq!(
        marks(&root),
        [Some(shodo::EmphasisMark {
            character: '*',
            font_size: 5.0,
            line_over: false,
            offset: 2.0,
        })]
    );
    root.writing_mode = WritingMode::VerticalRl;
    root.root.text_emphasis = Some(TextEmphasis {
        shape: TextEmphasisShape::Circle,
        filled: false,
        position: TextEmphasisPosition::UnderRight,
    });
    let [Some(mark)] = marks(&root)[..] else {
        panic!("one run")
    };
    assert_eq!((mark.character, mark.line_over), ('\u{25CB}', true));
}
