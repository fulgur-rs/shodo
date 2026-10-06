//! Quirks-mode line height (`ParagraphStyle::line_height_quirk`) in ruby
//! annotation paragraphs and the metric index.
use super::measure_tests::{base, fonts, ruby, style};
use crate::geometry::Saturation;
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{InlineStyle, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};

#[test]
fn quirk_annotation_block_profiles_do_not_rescan_long_prefixes() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                line_height_quirk: true,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            ruby(base(&"日".repeat(count)), &"にほん ".repeat(count)),
        );
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let container = &p.data.ruby.containers[0];
        let child = &container.lanes[0].paragraph.data;
        assert!(child.style.line_height_quirk);
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        for cut in &container.cuts[1..] {
            let height = crate::line::range::block_size(
                child,
                0..cut.lanes[0],
                &Default::default(),
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert_eq!(height.to_f32(), 17.390625);
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "quirk child block profiles rescanned prefixes: {visits:?}"
    );
}

#[test]
fn annotation_paragraphs_inherit_line_height_quirk() {
    for quirk in [false, true] {
        // Nested: the annotation itself carries ruby.
        let mut inner = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(12.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        inner.push_ruby(NodeId(30), &style(12.0), ruby(base("に"), "ni"));
        let mut outer = ruby(base("日"), "x");
        outer.levels[0].annotations[0].content = RubyContent::from_builder(inner);
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                line_height_quirk: quirk,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), outer);
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let lane = &p.data.ruby.containers[0].lanes[0].paragraph.data;
        assert_eq!(lane.style.line_height_quirk, quirk);
        let nested = &lane.ruby.containers[0].lanes[0].paragraph.data;
        assert_eq!(nested.style.line_height_quirk, quirk);
    }
}

fn quirk_paragraph(quirk: bool, build_content: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    quirk_paragraph_in(
        crate::geometry::WritingMode::HorizontalTb,
        quirk,
        build_content,
    )
}

fn quirk_paragraph_in(
    writing_mode: crate::geometry::WritingMode,
    quirk: bool,
    build_content: impl FnOnce(&mut ParagraphBuilder),
) -> Paragraph {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode,
            root: InlineStyle {
                line_height: crate::style::LineHeight::Px(30.0),
                ..style(24.0)
            },
            line_height_quirk: quirk,
            ..Default::default()
        },
        &Limits::default(),
    );
    build_content(&mut b);
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

/// Compare indexed block sizes with retained lines over every range a line
/// can hold, returning how many ranges were compared.
fn assert_quirk_parity(p: &Paragraph, atomics: &AtomicSizes) -> usize {
    use crate::analysis::units::UnitKind;
    let n = p.data.units.len();
    let mut cx = LayoutContext::new();
    cx.ruby_ranges.begin(&p.data, atomics);
    let mut compared = 0;
    for start in 0..n {
        if !matches!(
            p.data.units[start].kind,
            UnitKind::Cluster { .. }
                | UnitKind::Atomic { .. }
                | UnitKind::Open { .. }
                | UnitKind::Close { .. }
        ) {
            continue;
        }
        for end in start + 1..=n {
            let range = start..end;
            // A ruby container is one column for line breaking.
            if p.data.ruby.containers.iter().any(|c| {
                (c.units.start < start && start < c.units.end)
                    || (c.units.start < end && end < c.units.end)
            }) {
                continue;
            }
            let actual = p.ruby_line(
                &mut LayoutContext::new(),
                range.clone(),
                10000.0,
                atomics,
                crate::ruby::align::AnnotationAlign::Policy(RubyAlign::Start),
            );
            // A forced break inside the range ends the retained line early.
            if actual.units != (start as u32..end as u32) {
                continue;
            }
            let mut sat = Saturation::default();
            let ruby =
                crate::ruby::measure::candidate(&p.data, start, end, atomics, &mut cx, &mut sat);
            let indexed = crate::line::range::block_size(
                &p.data,
                range.clone(),
                &ruby,
                atomics,
                &mut cx,
                &mut sat,
            );
            assert_eq!(indexed.to_f32(), actual.block_size(), "{range:?}");
            // With no ruby annotation expansion the scalar profile also
            // exposes the baseline, including a zero-height pending line.
            if p.data.ruby.containers.is_empty() {
                let scalar = crate::line::metric_index::measure(
                    &p.data,
                    range.clone(),
                    atomics,
                    &mut cx,
                    &mut sat,
                );
                assert_eq!(
                    scalar.baseline.to_f32(),
                    actual.baseline(crate::geometry::BaselineKind::Alphabetic),
                    "baseline {range:?}"
                );
            }
            compared += 1;
        }
    }
    compared
}

/// Top/bottom groups, trailing collapsible (and zero-width) spaces, a
/// preserved space behind an inline-end edge, forced breaks with and
/// without parent content, and a ruby column.
fn quirk_fixture(mode: crate::geometry::WritingMode, quirk: bool) -> (Paragraph, AtomicSizes) {
    use crate::style::{LineHeight, VerticalAlign, WhiteSpaceCollapse};
    let tall = |align| InlineStyle {
        line_height: LineHeight::Px(80.0),
        vertical_align: align,
        ..style(24.0)
    };
    let mut atomics = AtomicSizes::new();
    for id in [90, 91, 92] {
        atomics.insert(
            NodeId(id),
            crate::AtomicSize {
                inline_size: 4.0,
                block_size: 4.0,
                ..Default::default()
            },
        );
    }
    let text = |b: &mut ParagraphBuilder, node: u64, s: &str| {
        b.push_text(TextSource::Generated { node: NodeId(node) }, s);
    };
    let p = quirk_paragraph_in(mode, quirk, |b| {
        text(b, 1, "日 ");
        b.open_inline(
            NodeId(100),
            &tall(VerticalAlign::Baseline),
            Default::default(),
        );
        b.push_atomic(NodeId(90), &style(24.0), Default::default());
        b.push_forced_break(NodeId(3));
        text(b, 2, "日 ");
        b.close_inline();
        b.open_inline(NodeId(101), &tall(VerticalAlign::Top), Default::default());
        b.push_atomic(NodeId(91), &style(24.0), Default::default());
        text(b, 4, "本 ");
        b.open_inline(
            NodeId(104),
            &InlineStyle {
                vertical_align: VerticalAlign::Length(-3.0),
                ..tall(VerticalAlign::Baseline)
            },
            Default::default(),
        );
        text(b, 5, "語 ");
        b.close_inline();
        b.close_inline();
        b.open_inline(
            NodeId(102),
            &InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..tall(VerticalAlign::Bottom)
            },
            crate::node::InlineEdges {
                padding: crate::node::Sides {
                    inline_end: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        text(b, 6, "本 ");
        b.close_inline();
        b.open_inline(
            NodeId(103),
            &InlineStyle {
                font_size: 0.0,
                ..tall(VerticalAlign::Baseline)
            },
            Default::default(),
        );
        text(b, 7, " ");
        b.close_inline();
        b.push_atomic(NodeId(92), &style(24.0), Default::default());
        b.open_inline(
            NodeId(105),
            &tall(VerticalAlign::Bottom),
            Default::default(),
        );
        b.push_forced_break(NodeId(9));
        b.close_inline();
        b.push_ruby(NodeId(8), &style(24.0), ruby(base("語"), "ご"));
        text(b, 11, "日 ");
    });
    (p, atomics)
}

#[test]
fn quirk_fixture_ranges_match_retained_lines_without_the_flag() {
    let (p, atomics) = quirk_fixture(crate::geometry::WritingMode::HorizontalTb, false);
    assert!(assert_quirk_parity(&p, &atomics) > 100);
}

#[test]
fn quirk_index_matches_retained_lines() {
    for mode in [
        crate::geometry::WritingMode::HorizontalTb,
        crate::geometry::WritingMode::VerticalRl,
        crate::geometry::WritingMode::VerticalLr,
    ] {
        let (p, atomics) = quirk_fixture(mode, true);
        assert!(assert_quirk_parity(&p, &atomics) > 100, "{mode:?}");
    }
}

#[test]
fn quirk_index_matches_retained_with_hyphenated_edges() {
    // Line edges reshape at soft hyphens, so `removed`/`replacements` run,
    // including next to trailing spaces.
    let p = quirk_paragraph(true, |b| {
        b.open_inline(
            NodeId(100),
            &InlineStyle {
                line_height: crate::style::LineHeight::Px(80.0),
                ..style(24.0)
            },
            Default::default(),
        );
        b.push_text(
            TextSource::Generated { node: NodeId(1) },
            "日\u{ad}本 \u{ad}語\u{ad} ",
        );
        b.close_inline();
        b.push_text(TextSource::Generated { node: NodeId(2) }, "日\u{ad} ");
    });
    assert!(assert_quirk_parity(&p, &AtomicSizes::EMPTY) > 10);
}

#[test]
fn pending_vertical_align_index_matches_retained_ranges() {
    use crate::style::{LineHeight, VerticalAlign};
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(90),
        crate::AtomicSize {
            inline_size: 2.0,
            block_size: 2.0,
            ..Default::default()
        },
    );
    for outer in [
        VerticalAlign::Baseline,
        VerticalAlign::Top,
        VerticalAlign::Bottom,
    ] {
        for inner in [
            VerticalAlign::Baseline,
            VerticalAlign::Top,
            VerticalAlign::Bottom,
            VerticalAlign::TextTop,
            VerticalAlign::TextBottom,
            VerticalAlign::Sub,
            VerticalAlign::Middle,
            VerticalAlign::Length(0.0),
        ] {
            for content in [false, true] {
                let p = quirk_paragraph(true, |b| {
                    let parent = InlineStyle {
                        line_height: LineHeight::Px(60.0),
                        vertical_align: outer,
                        ..style(24.0)
                    };
                    let child = InlineStyle {
                        vertical_align: inner,
                        ..style(24.0)
                    };
                    b.open_inline(NodeId(100), &parent, Default::default());
                    // A baseline ancestor between p and a pending child.
                    b.open_inline(NodeId(101), &style(24.0), Default::default());
                    b.open_inline(NodeId(102), &child, Default::default());
                    if content {
                        b.push_atomic(NodeId(90), &child, Default::default());
                    }
                    b.close_inline();
                    b.close_inline();
                    b.push_forced_break(NodeId(3));
                    b.close_inline();
                    b.push_forced_break(NodeId(4));
                });
                assert!(
                    assert_quirk_parity(&p, &atomics) > 5,
                    "{outer:?}/{inner:?}/{content}"
                );
            }
        }
    }
}

#[test]
fn pending_forced_break_queries_do_not_rescan_descendants() {
    use crate::style::{LineHeight, VerticalAlign};
    let mut visits = Vec::new();
    for count in [64, 128] {
        let p = quirk_paragraph(true, |b| {
            b.open_inline(
                NodeId(100),
                &InlineStyle {
                    line_height: LineHeight::Px(60.0),
                    ..style(24.0)
                },
                Default::default(),
            );
            for i in 0..count {
                b.open_inline(
                    NodeId(200 + i),
                    &InlineStyle {
                        vertical_align: VerticalAlign::Top,
                        ..style(24.0)
                    },
                    Default::default(),
                );
                b.close_inline();
            }
            b.push_forced_break(NodeId(3));
            b.close_inline();
        });
        let end = p.data.units.len() - 1;
        let mut cx = LayoutContext::new();
        for i in 0..count as usize {
            let metrics = crate::line::metric_index::measure(
                &p.data,
                1 + 2 * i..end,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert_eq!(metrics.block_size.to_f32(), 60.0);
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] < 3 * visits[0],
        "pending descendants rescanned: {visits:?}"
    );
}

#[test]
fn emphasis_index_matches_retained_ranges() {
    use crate::style::{
        LineHeight, TextCombineUpright, TextEmphasis, TextEmphasisPosition as P, TextEmphasisShape,
    };
    let mark = |position| {
        Some(TextEmphasis {
            shape: TextEmphasisShape::Dot,
            filled: true,
            position,
        })
    };
    for mode in [
        crate::geometry::WritingMode::HorizontalTb,
        crate::geometry::WritingMode::VerticalRl,
        crate::geometry::WritingMode::VerticalLr,
    ] {
        for quirk in [false, true] {
            // Marks on root text only, then on inline box text and a
            // composition only, so neither hides the other's overflow.
            for root_mark in [true, false] {
                let inner = |position| if root_mark { None } else { mark(position) };
                let mut b = ParagraphBuilder::new(
                    &ParagraphStyle {
                        writing_mode: mode,
                        root: InlineStyle {
                            line_height: LineHeight::Px(20.0),
                            text_emphasis: if root_mark { mark(P::OverRight) } else { None },
                            ..style(24.0)
                        },
                        line_height_quirk: quirk,
                        ..Default::default()
                    },
                    &Limits::default(),
                );
                b.push_text(TextSource::Generated { node: NodeId(1) }, "日 ");
                b.open_inline(
                    NodeId(100),
                    &InlineStyle {
                        line_height: LineHeight::Px(4.0),
                        text_emphasis: inner(P::UnderLeft),
                        ..style(12.0)
                    },
                    Default::default(),
                );
                b.push_text(TextSource::Generated { node: NodeId(2) }, "本 語 ");
                b.close_inline();
                b.push_text(TextSource::Generated { node: NodeId(5) }, "日 ");
                b.open_inline(
                    NodeId(101),
                    &InlineStyle {
                        text_emphasis: inner(P::OverLeft),
                        ..style(40.0)
                    },
                    Default::default(),
                );
                b.close_inline();
                b.push_text(TextSource::Generated { node: NodeId(6) }, "日 ");
                b.open_inline(
                    NodeId(102),
                    &InlineStyle {
                        text_combine_upright: TextCombineUpright::All,
                        text_emphasis: inner(P::UnderRight),
                        ..style(32.0)
                    },
                    Default::default(),
                );
                // One character: the parity walk would cut a longer composition.
                b.push_text(TextSource::Generated { node: NodeId(3) }, "1");
                b.close_inline();
                b.push_text(TextSource::Generated { node: NodeId(4) }, " 日");
                // A forced break that only the root strut sizes.
                b.open_inline(NodeId(103), &style(12.0), Default::default());
                b.close_inline();
                b.push_forced_break(NodeId(7));
                b.push_text(TextSource::Generated { node: NodeId(8) }, "日");
                let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
                assert!(
                    assert_quirk_parity(&p, &AtomicSizes::EMPTY) > 20,
                    "{mode:?} {quirk} {root_mark}"
                );
            }
        }
    }
}
