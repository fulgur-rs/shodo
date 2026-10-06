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
    // Marks overflow a short line box on their side only; the strut does not
    // carry them.
    root.root.line_height = LineHeight::Px(4.0);
    assert_eq!(measure(&root, "ab"), (12.0, 13.0));
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
    // Inline box struts carry no marks, so an empty emphasized inline does
    // not grow the line (Chromium 152).
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        b.open_inline(NodeId(2), &child, InlineEdges::default())
            .close_inline();
    });
    let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(l.block_size(), 10.0);
}

#[test]
fn vertical_marks_use_the_left_or_right_keyword() {
    use TextEmphasisPosition as P;
    use WritingMode as W;
    for (mode, position, over) in [
        (W::VerticalRl, P::OverRight, true),
        (W::VerticalRl, P::UnderRight, true),
        (W::VerticalRl, P::OverLeft, false),
        (W::VerticalLr, P::UnderRight, true),
        (W::VerticalLr, P::UnderLeft, false),
        (W::SidewaysRl, P::OverRight, true),
        (W::SidewaysRl, P::OverLeft, false),
        // Line-over is on the left in sideways-lr.
        (W::SidewaysLr, P::UnderLeft, true),
        (W::SidewaysLr, P::OverRight, false),
    ] {
        let mut root = style();
        root.writing_mode = mode;
        let (plain_size, plain_baseline) = measure(&root, "ab");
        root.root.text_emphasis = emphasis(position);
        let (size, baseline) = measure(&root, "ab");
        assert_eq!(size, plain_size + 5.0, "{mode:?} {position:?}");
        // Line-over is block-start except in vertical-lr.
        let shifted = over == (mode != W::VerticalLr);
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

#[test]
fn marks_sit_outside_the_normalized_em_box() {
    use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use shodo::limits::Limits;
    use shodo::style::FontFamily;
    // The fixture's typographic ascent is 1069 of a 1362 unit em, so its
    // normalized em ascent is below its 1069 unit hhea ascent.
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Fixture".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut root = style();
    root.root.font_families = vec![FontFamily::Named("Fixture".into())];
    let line = |root: &ParagraphStyle| {
        let mut b = shodo::ParagraphBuilder::new(root, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
        let p = b.build(&mut shodo::LayoutContext::new(), &fonts).unwrap();
        let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        (l.block_size(), l.baseline(BaselineKind::Alphabetic))
    };
    let (plain, baseline) = line(&root);
    // Ascent 10.69 trims 2 whole pixels toward the 7.85px em ascent; descent
    // 2.93 is less than a pixel above the 2.15px em descent and stays. Chromium
    // 152 gives 16 and 19 for 14px lines after rounding the metrics.
    let ascent = 10.0 * 1069.0 / 1000.0;
    let descent = 10.0 * 293.0 / 1000.0;
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    let (size, marked_baseline) = line(&root);
    assert!(
        (marked_baseline - (ascent - 2.0 + 5.0)).abs() < 0.02,
        "{marked_baseline}"
    );
    assert!(
        (size - plain - (marked_baseline - baseline)).abs() < 0.02,
        "{size}"
    );
    root.root.text_emphasis = emphasis(TextEmphasisPosition::UnderRight);
    let (size, under_baseline) = line(&root);
    assert_eq!(under_baseline, baseline);
    assert!((size - (baseline + descent + 5.0)).abs() < 0.02, "{size}");
}

#[test]
fn emphasized_atomics_grow_the_line_over_their_margin_box() {
    let root = style();
    let mut marked = root.root.clone();
    marked.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(2),
        shodo::AtomicSize {
            inline_size: 5.0,
            block_size: 20.0,
            ..Default::default()
        },
    );
    let line = |style: &shodo::style::InlineStyle| {
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            b.push_atomic(NodeId(2), style, InlineEdges::default());
        });
        let l = first_line(&p, 1000.0, &LineOptions::default(), &atomics);
        (l.block_size(), l.baseline(BaselineKind::Alphabetic))
    };
    assert_eq!(line(&root.root), (22.0, 20.0));
    assert_eq!(line(&marked), (27.0, 25.0));
    // Chromium 152 never grows a line for under marks of an atomic inline.
    marked.text_emphasis = emphasis(TextEmphasisPosition::UnderRight);
    assert_eq!(line(&marked), (22.0, 20.0));
}

#[test]
fn fallback_runs_trim_toward_their_own_em_box() {
    use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use shodo::limits::Limits;
    use shodo::style::FontFamily;
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (bytes, family) in [
        (
            &include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf")[..],
            "Latin",
        ),
        (
            &include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf")[..],
            "CJK",
        ),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let mut root = style();
    root.root.font_size = 40.0;
    root.root.font_families = vec![
        FontFamily::Named("Latin".into()),
        FontFamily::Named("CJK".into()),
    ];
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    let mut b = shodo::ParagraphBuilder::new(&root, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日本");
    let p = b.build(&mut shodo::LayoutContext::new(), &fonts).unwrap();
    let l = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    // Like Blink: the Latin primary ascent (42.76px) trims 7 whole pixels
    // toward the CJK face's 35.2px em ascent, then the 20px mark.
    let expected = 40.0 * 1.069 - 7.0 + 20.0;
    let baseline = l.baseline(BaselineKind::Alphabetic);
    assert!((baseline - expected).abs() < 0.02, "{baseline}");
}

#[test]
fn overflow_includes_synthetic_emphasis_boxes_but_skips_excluded_characters() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(4.0);
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    let make = |text: &str| {
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, text);
        });
        first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY)
    };
    let line = make("a");
    let bounds = line.overflow_rect();
    assert_eq!(
        bounds,
        shodo::geometry::LogicalRect {
            inline_start: 2.5,
            block_start: 0.0,
            inline_size: 5.0,
            block_size: 5.0
        }
    );
    let excluded = make(".");
    assert_eq!(excluded.overflow_rect(), Default::default());
}

#[test]
fn nominal_mark_overflow_uses_untrimmed_paint_edges_and_combined_square() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(40.0);
    root.root.text_emphasis = emphasis(TextEmphasisPosition::UnderRight);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let line = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.block_size(), 40.0, "leading absorbs the mark");
    assert_eq!(
        line.overflow_rect(),
        shodo::geometry::LogicalRect {
            inline_start: 2.5,
            block_start: 25.0,
            inline_size: 5.0,
            block_size: 5.0
        }
    );
    root.writing_mode = WritingMode::VerticalLr;
    root.root.line_height = LineHeight::Px(10.0);
    root.root.text_combine_upright = shodo::style::TextCombineUpright::All;
    root.root.text_emphasis = emphasis(TextEmphasisPosition::OverRight);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "12");
    });
    let line = first_line(&p, 1000.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    let square = line.text_combinations().next().unwrap().square;
    let bounds = line.overflow_rect();
    assert_eq!(bounds.inline_start, square.inline_start + 2.5);
    assert_eq!(bounds.block_start, square.block_start + square.block_size);
    assert_eq!((bounds.inline_size, bounds.block_size), (5.0, 5.0));
}
