mod common;

use common::*;
use shodo::AtomicSizes;
use shodo::geometry::WritingMode;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    LineHeight, LineOptions, TextEmphasis, TextEmphasisPosition, TextEmphasisShape,
};

fn marked(height: f32) -> shodo::style::ParagraphStyle {
    let mut root = style();
    root.root.line_height = LineHeight::Px(height);
    root.root.text_emphasis = Some(TextEmphasis {
        shape: TextEmphasisShape::Dot,
        filled: true,
        position: TextEmphasisPosition::OverRight,
    });
    root
}

#[test]
fn caller_can_borrow_preceding_leading_without_changing_standalone_lines() {
    let root = marked(14.);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
        b.push_forced_break(NodeId(2));
        b.push_text(TextSource::Generated { node: NodeId(3) }, "ab");
    });
    let lines = p.break_all(
        &mut shodo::LayoutContext::new(),
        &LineOptions::default(),
        1000.,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    for line in &lines {
        let m = line.annotation_metrics();
        assert_eq!(
            (
                line.block_size(),
                m.unannotated_block_start,
                m.unannotated_block_end
            ),
            (17., 3., 17.)
        );
        assert_eq!(
            (
                m.overflow_over,
                m.overflow_under,
                m.space_over,
                m.space_under
            ),
            (3., 0., 0., 2.)
        );
    }
    let previous = lines[0].annotation_metrics();
    let next = lines[1].annotation_metrics();
    let borrowed = previous.space_under.min(next.overflow_over);
    assert_eq!(borrowed, 2.);
    assert_eq!(
        lines.iter().map(|l| l.block_size()).sum::<f32>() - borrowed,
        32.
    );
    // The caller can equally receive this spare space from another block.
    assert_eq!((next.overflow_over - 10.).max(0.), 0.);
}

#[test]
fn negative_leading_does_not_expose_font_overflow_as_annotation_or_space() {
    let mut root = marked(4.);
    root.root.text_emphasis = None;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!(
        (m.unannotated_block_start, m.unannotated_block_end),
        (0., 4.)
    );
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (0., 0., 0., 0.)
    );
    root.root.text_emphasis = marked(4.).root.text_emphasis;
    root.root.text_emphasis.as_mut().unwrap().position = TextEmphasisPosition::UnderRight;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!(
        (m.unannotated_block_start, m.unannotated_block_end),
        (0., 4.)
    );
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (0., 8., 0., 0.)
    );
}

#[test]
fn annotation_space_uses_line_relative_sides_in_every_writing_mode() {
    use TextEmphasisPosition as P;
    use WritingMode as W;
    for mode in [
        W::HorizontalTb,
        W::VerticalRl,
        W::VerticalLr,
        W::SidewaysRl,
        W::SidewaysLr,
    ] {
        for position in [P::OverRight, P::OverLeft, P::UnderRight, P::UnderLeft] {
            let mut root = marked(40.);
            root.writing_mode = mode;
            root.root.text_emphasis.as_mut().unwrap().position = position;
            let p = build(&root, |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
            });
            let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
            let m = line.annotation_metrics();
            let over = match mode {
                W::HorizontalTb => matches!(position, P::OverRight | P::OverLeft),
                W::SidewaysLr => matches!(position, P::OverLeft | P::UnderLeft),
                _ => matches!(position, P::OverRight | P::UnderRight),
            };
            assert_eq!(
                (m.unannotated_block_start, m.unannotated_block_end),
                (0., 40.),
                "{mode:?} {position:?}"
            );
            assert_eq!((m.overflow_over, m.overflow_under), (0., 0.));
            assert_eq!(
                (m.space_over, m.space_under),
                if over { (10., 15.) } else { (15., 10.) },
                "{mode:?} {position:?}"
            );
        }
    }
}

#[test]
fn first_line_retries_and_truncation_preserve_annotation_geometry() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(14.);
    root.first_line = Some(marked(14.).root);
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "abcdef");
        b.push_forced_break(NodeId(2));
        b.push_text(TextSource::Generated { node: NodeId(3) }, "abcdef");
    });
    let mut cx = shodo::LayoutContext::new();
    let mut c = shodo::LineConstraint::new(1000.);
    c.max_block_size = Some(16.);
    for _ in 0..2 {
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &c,
                &AtomicSizes::EMPTY
            ),
            shodo::LineResult::BlockSizeExceeded {
                needed_block_size: 17.
            }
        ));
    }
    c.max_block_size = None;
    let shodo::LineResult::Line(mut line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("accepted");
    };
    let fresh = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.annotation_metrics(), fresh.annotation_metrics());
    let saved = line.annotation_metrics();
    let token = line.break_token();
    assert!(line.truncate_with_ellipsis(&mut cx, 20.).is_some());
    assert_eq!(line.annotation_metrics(), saved);
    let shodo::LineResult::Line(second) = p.next_line(
        &mut cx,
        token,
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("continuation");
    };
    let m = second.annotation_metrics();
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (0., 0., 2., 2.)
    );
}

#[test]
fn larger_unmarked_font_overflow_is_not_added_to_annotation_overflow() {
    let root = marked(4.);
    let mut large = root.root.clone();
    large.font_size = 20.;
    large.text_emphasis = None;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        b.open_inline(NodeId(2), &large, shodo::node::InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "b")
            .close_inline();
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!(
        (m.unannotated_block_start, m.unannotated_block_end),
        (5., 12.)
    );
    // The large unmarked glyph extends 3px farther than the marks.
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (5., 0., 0., 0.)
    );
}

#[test]
fn root_em_floor_never_discards_displaced_small_marks() {
    let mut root = style();
    root.root.line_height = LineHeight::Px(40.);
    let mut small = marked(5.).root;
    small.font_size = 5.;
    small.vertical_align = shodo::style::VerticalAlign::Length(5.);
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &small, shodo::node::InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "a")
            .close_inline();
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!((m.space_over, m.space_under), (11.5, 15.));
}

#[test]
fn quirk_without_root_strut_keeps_small_annotation_overflow() {
    let mut root = style();
    root.line_height_quirk = true;
    root.root.font_size = 100.;
    let mut small = marked(5.).root;
    small.font_size = 5.;
    let p = build(&root, |b| {
        b.open_inline(NodeId(2), &small, shodo::node::InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, "a")
            .close_inline();
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!(
        (
            line.block_size(),
            m.unannotated_block_start,
            m.unannotated_block_end
        ),
        (7.5, 2.5, 7.5)
    );
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (2.5, 0., 0., 0.)
    );
}

#[test]
fn forced_root_strut_controls_the_bare_box_and_root_em_floor() {
    for forced in [false, true] {
        let mut root = style();
        root.line_height_quirk = true;
        root.force_root_strut = forced;
        root.root.font_size = 100.;
        root.root.line_height = LineHeight::Px(120.);
        let mut small = marked(5.).root;
        small.font_size = 5.;
        let p = build(&root, |b| {
            b.open_inline(NodeId(2), &small, shodo::node::InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "a")
                .close_inline();
        });
        let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
        let m = line.annotation_metrics();
        assert_eq!(
            (
                line.block_size(),
                m.unannotated_block_start,
                m.unannotated_block_end,
                m.overflow_over,
                m.overflow_under,
                m.space_over,
                m.space_under
            ),
            if forced {
                (120., 0., 120., 0., 0., 10., 10.)
            } else {
                (7.5, 2.5, 7.5, 2.5, 0., 0., 0.)
            },
            "forced={forced}"
        );
    }
}

#[test]
fn top_and_bottom_groups_never_offer_their_occupied_leading() {
    use shodo::style::VerticalAlign;
    for align in [VerticalAlign::Top, VerticalAlign::Bottom] {
        let mut root = style();
        root.root.line_height = LineHeight::Px(40.);
        let mut child = marked(5.).root;
        child.font_size = 5.;
        child.vertical_align = align;
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            b.open_inline(NodeId(2), &child, shodo::node::InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "b")
                .close_inline();
        });
        let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
        let m = line.annotation_metrics();
        assert_eq!(
            (m.unannotated_block_start, m.unannotated_block_end),
            (0., 40.)
        );
        assert_eq!((m.overflow_over, m.overflow_under), (0., 0.));
        assert_eq!(
            (m.space_over, m.space_under),
            if align == VerticalAlign::Top {
                (0., 15.)
            } else {
                (15., 0.)
            },
            "{align:?}"
        );
    }
}

#[test]
fn atomic_over_marks_reserve_space_but_under_marks_follow_existing_exclusion() {
    for position in [
        TextEmphasisPosition::OverRight,
        TextEmphasisPosition::UnderRight,
    ] {
        let mut root = style();
        root.root.line_height = LineHeight::Px(40.);
        let mut child = marked(40.).root;
        child.text_emphasis.as_mut().unwrap().position = position;
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            b.push_atomic(NodeId(2), &child, shodo::node::InlineEdges::default());
        });
        let mut sizes = AtomicSizes::new();
        sizes.insert(
            NodeId(2),
            shodo::AtomicSize {
                inline_size: 10.,
                block_size: 8.,
                ..Default::default()
            },
        );
        let line = first_line(&p, 1000., &LineOptions::default(), &sizes);
        let m = line.annotation_metrics();
        assert_eq!((m.overflow_over, m.overflow_under), (0., 0.));
        assert_eq!(
            (m.space_over, m.space_under),
            if position == TextEmphasisPosition::OverRight {
                (10., 15.)
            } else {
                (15., 15.)
            }
        );
    }
}

#[test]
fn combined_text_reserves_one_square_and_one_mark_on_the_requested_side() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for position in [
            TextEmphasisPosition::OverRight,
            TextEmphasisPosition::OverLeft,
        ] {
            let mut root = marked(40.);
            root.writing_mode = mode;
            root.root.text_combine_upright = shodo::style::TextCombineUpright::All;
            root.root.text_emphasis.as_mut().unwrap().position = position;
            let p = build(&root, |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "123456");
            });
            let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(line.text_combinations().len(), 1);
            let m = line.annotation_metrics();
            assert_eq!((m.overflow_over, m.overflow_under), (0., 0.));
            assert_eq!(
                (m.space_over, m.space_under),
                if position == TextEmphasisPosition::OverRight {
                    (10., 15.)
                } else {
                    (15., 10.)
                },
                "{mode:?} {position:?}"
            );
        }
    }
}

#[test]
fn a_tab_only_combination_still_exposes_its_reserved_mark_geometry() {
    let mut root = marked(4.);
    root.writing_mode = WritingMode::VerticalRl;
    root.root.text_combine_upright = shodo::style::TextCombineUpright::All;
    root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "\t");
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(line.text_combinations().len(), 1);
    let m = line.annotation_metrics();
    assert_eq!(
        (
            line.block_size(),
            m.unannotated_block_start,
            m.unannotated_block_end
        ),
        (15., 5., 15.)
    );
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (5., 0., 0., 0.)
    );
}

#[test]
fn quirk_trimmed_trailing_space_does_not_restore_excluded_annotation_geometry() {
    let mut root = style();
    root.line_height_quirk = true;
    root.root.line_height = LineHeight::Px(40.);
    let plain = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    });
    let expected = first_line(&plain, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let mut large = marked(4.).root;
    large.font_size = 40.;
    let p = build(&root, |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        b.open_inline(NodeId(2), &large, Default::default())
            .push_text(TextSource::Generated { node: NodeId(3) }, " ")
            .close_inline();
    });
    let actual = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    assert_eq!(actual.block_size(), expected.block_size());
    assert_eq!(actual.annotation_metrics(), expected.annotation_metrics());
}

#[test]
fn styled_break_does_not_expand_annotation_metrics_after_text() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for forced in [false, true] {
            let mut root = style();
            root.writing_mode = mode;
            root.line_height_quirk = true;
            root.force_root_strut = forced;
            root.root.line_height = LineHeight::Px(20.);
            let mut child = marked(5.).root;
            child.font_size = 5.;
            let mut br = marked(40.).root;
            br.font_size = 40.;
            let make = |break_style: Option<&shodo::style::InlineStyle>| {
                build(&root, |b| {
                    b.open_inline(NodeId(2), &child, Default::default())
                        .push_text(TextSource::Generated { node: NodeId(3) }, "a");
                    if let Some(style) = break_style {
                        b.push_forced_break_with_style(NodeId(4), style);
                    } else {
                        b.push_forced_break(NodeId(4));
                    }
                    b.close_inline();
                })
            };
            let p = make(Some(&br));
            let actual = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
            br.text_emphasis = None;
            let plain_break = make(Some(&br));
            let expected = first_line(
                &plain_break,
                1000.,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            assert_eq!(actual.annotation_metrics(), expected.annotation_metrics());
            // Changing only the break's font/emphasis must not expand an
            // already credited text parent's geometry. The legacy API is
            // an independent input path retaining that parent's strut.
            let legacy = make(None);
            let expected = first_line(&legacy, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(
                actual.block_size(),
                expected.block_size(),
                "{mode:?}/{forced}"
            );
            assert_eq!(
                actual.annotation_metrics(),
                expected.annotation_metrics(),
                "{mode:?}/{forced}"
            );
        }
    }
}

#[test]
fn annotation_metrics_separate_bare_box_overflow_and_unused_leading() {
    for (mark, expected_over) in [(false, 15.0), (true, 10.0)] {
        let mut root = marked(40.0);
        if !mark {
            root.root.text_emphasis = None;
        }
        let p = build(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
        });
        let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
        let m = line.annotation_metrics();
        assert_eq!(
            (m.unannotated_block_start, m.unannotated_block_end),
            (0., 40.)
        );
        assert_eq!((m.overflow_over, m.overflow_under), (0., 0.));
        assert_eq!((m.space_over, m.space_under), (expected_over, 15.));
    }
    let p = build(&marked(4.0), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ab");
    });
    let line = first_line(&p, 1000., &LineOptions::default(), &AtomicSizes::EMPTY);
    let m = line.annotation_metrics();
    assert_eq!(
        (
            line.block_size(),
            m.unannotated_block_start,
            m.unannotated_block_end
        ),
        (12., 8., 12.)
    );
    assert_eq!(
        (
            m.overflow_over,
            m.overflow_under,
            m.space_over,
            m.space_under
        ),
        (8., 0., 0., 0.)
    );
}
