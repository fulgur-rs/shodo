//! Real-font ruby geometry: literal advances, positions and neighbor clearance.
use shodo::geometry::WritingMode;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineHeight, ParagraphStyle};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, Line, LineConstraint, LineResult,
    ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel,
    RubyOverhang, RubyPosition, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_fixtures::load_fonts;

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}
fn content(node: u64, text: &str, size: f32) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 40,
        },
        text,
        &style(size),
        &Limits::default(),
    )
}
fn pair(base: &str, reading: &str, base_align: RubyAlign, ruby_style: RubyStyle) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: content(10, base, 24.0),
            align: base_align,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: content(20, reading, 12.0),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: ruby_style,
        }],
    )
    .unwrap()
}
fn layout(builder: ParagraphBuilder, atomics: &AtomicSizes) -> Line {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        atomics,
    ) else {
        panic!("real ruby line")
    };
    line
}
fn glyph_positions(line: &Line) -> Vec<f32> {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().map(|g| g.inline_position))
        .collect()
}

#[test]
fn all_ruby_alignments_keep_actual_base_and_reading_advances() {
    for (align, base_positions, reading_positions) in [
        (RubyAlign::Start, vec![0.0, 24.0], vec![0.0, 12.0]),
        (RubyAlign::Center, vec![12.0, 36.0], vec![24.0, 36.0]),
        (RubyAlign::SpaceBetween, vec![0.0, 48.0], vec![0.0, 60.0]),
        (RubyAlign::SpaceAround, vec![6.0, 42.0], vec![12.0, 48.0]),
    ] {
        let ruby_style = RubyStyle {
            align,
            overhang: RubyOverhang::None,
            ..Default::default()
        };
        // Six12px reading glyphs reserve72px for two24px base glyphs.
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair("日本", "にほんにほん", align, ruby_style),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 72.0);
        assert_eq!(glyph_positions(&line), base_positions);
        let carets = shodo::hit::LineLayout::new(std::slice::from_ref(&line));
        let source = line.text().find("日本").unwrap() as u32;
        for (offset, want) in [source, source + 3].into_iter().zip(&base_positions) {
            let caret = carets
                .caret(shodo::hit::TextPosition {
                    line: 0,
                    offset,
                    affinity: shodo::mapping::Affinity::Downstream,
                })
                .unwrap();
            assert_eq!(
                caret.rect.inline_start, *want,
                "{align:?} actual base caret"
            );
        }

        let end_caret = carets
            .caret(shodo::hit::TextPosition {
                line: 0,
                offset: source + 6,
                affinity: shodo::mapping::Affinity::Upstream,
            })
            .unwrap();
        assert_eq!(
            end_caret.rect.inline_start,
            base_positions[1] + 24.0,
            "{align:?} base end"
        );

        for run in line.fragments().filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        }) {
            assert!(run.clusters().all(|c| c.shaping_advance == 24.0));
            assert_eq!(run.font_size(), 24.0);
        }
        // Three24px base glyphs reserve72px for two12px reading glyphs.
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair("日本語", "にほ", align, ruby_style),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        let reading = line.ruby_annotations().next().unwrap();
        assert_eq!(reading.line().inline_size(), 72.0);
        assert_eq!(glyph_positions(reading.line()), reading_positions);
        let carets = shodo::hit::LineLayout::new(std::slice::from_ref(reading.line()));
        let source = reading.line().text().find("にほ").unwrap() as u32;
        for (offset, affinity, want) in [
            (
                source,
                shodo::mapping::Affinity::Downstream,
                reading_positions[0],
            ),
            (
                source + 3,
                shodo::mapping::Affinity::Downstream,
                reading_positions[1],
            ),
            (
                source + 6,
                shodo::mapping::Affinity::Upstream,
                reading_positions[1] + 12.0,
            ),
        ] {
            let caret = carets
                .caret(shodo::hit::TextPosition {
                    line: 0,
                    offset,
                    affinity,
                })
                .unwrap();
            assert_eq!(
                caret.rect.inline_start, want,
                "{align:?} retained reading caret"
            );
        }
        for run in reading.line().fragments().filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        }) {
            assert!(run.clusters().all(|c| c.shaping_advance == 12.0));
            assert_eq!(run.font_size(), 12.0);
        }
    }
}

#[test]
fn vertical_ruby_positions_use_line_over_and_line_under() {
    for (mode, position, block_origin, baseline) in [
        (WritingMode::VerticalRl, RubyPosition::Over, 0.0, 24.0),
        (WritingMode::VerticalRl, RubyPosition::Under, 24.0, 12.0),
        (
            WritingMode::VerticalRl,
            RubyPosition::InterCharacter,
            0.0,
            24.0,
        ),
        (WritingMode::VerticalLr, RubyPosition::Over, 24.0, 12.0),
        (WritingMode::VerticalLr, RubyPosition::Under, 0.0, 24.0),
        (
            WritingMode::VerticalLr,
            RubyPosition::InterCharacter,
            24.0,
            12.0,
        ),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair(
                "日",
                "に",
                RubyAlign::default(),
                RubyStyle {
                    position,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.block_size(), 36.0, "{mode:?}/{position:?}");
        assert_eq!(line.metrics().baseline, baseline);
        let reading = line.ruby_annotations().next().unwrap();
        assert_eq!(reading.origin().1, block_origin);
        let t = reading.transform();
        assert_eq!(
            (
                t.inline_inline,
                t.inline_block,
                t.block_inline,
                t.block_block
            ),
            (1.0, 0.0, 0.0, 1.0)
        );
        assert_eq!(reading.line().block_size(), 12.0);
    }
}

#[test]
fn over_and_under_stacks_share_available_leading_without_overlap_on_repeat() {
    let base_style = style(20.0);
    let container_style = InlineStyle {
        line_height: LineHeight::Px(50.0),
        ..base_style.clone()
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: content(10, "日", 20.0),
            align: RubyAlign::default(),
        }],
        [RubyPosition::Over, RubyPosition::Under]
            .into_iter()
            .enumerate()
            .map(|(i, position)| RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20 + i as u64),
                    content: content(20 + i as u64, "に", 10.0),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    position,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            })
            .collect(),
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: container_style.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_ruby(NodeId(8), &container_style, ruby);
    let line = layout(b, &AtomicSizes::EMPTY);
    // Actual pinned content B28.96875 plus O14.484375/U14.484375.
    assert_eq!(line.block_size(), 57.9375);
    let over = line.ruby_annotations().find(|a| a.level() == 0).unwrap();
    let under = line.ruby_annotations().find(|a| a.level() == 1).unwrap();
    assert_eq!(over.origin().1, 0.0);
    assert_eq!(under.origin().1, 43.453125);
    assert_eq!(
        under.origin().1 + under.line().block_size(),
        line.block_size() + over.origin().1,
        "identical repeated line stacks touch without overlapping"
    );
}

#[test]
fn safe_overhang_uses_plain_neighbors_and_rejects_blockers_and_line_edges() {
    for (neighbor, overhang, want) in [
        ("plain", RubyOverhang::None, 84.0),
        ("atomic", RubyOverhang::Auto, 84.0),
        ("ruby", RubyOverhang::Auto, 84.0),
        ("tall", RubyOverhang::Auto, 132.0),
        ("edge", RubyOverhang::Auto, 36.0),
        ("plain", RubyOverhang::Auto, 72.0),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        let mut atomics = AtomicSizes::new();
        let neighbor_ruby = pair(
            "本",
            "ほ",
            RubyAlign::default(),
            RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        );
        for (index, node) in [(0, 1), (1, 2)] {
            match neighbor {
                "plain" => {
                    b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
                }
                "atomic" => {
                    b.push_atomic(NodeId(node), &style(24.0), Default::default());
                    atomics.insert(
                        NodeId(node),
                        AtomicSize {
                            inline_size: 24.0,
                            block_size: 24.0,
                            ..Default::default()
                        },
                    );
                }
                "ruby" => {
                    b.push_ruby(NodeId(100 + node), &style(24.0), neighbor_ruby.clone());
                }
                "tall" => {
                    b.open_inline(NodeId(200 + node), &style(48.0), Default::default());
                    b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
                    b.close_inline();
                }
                "edge" => {}
                _ => unreachable!(),
            }
            if index == 0 {
                b.push_ruby(
                    NodeId(8),
                    &style(24.0),
                    pair(
                        "日",
                        "にほん",
                        RubyAlign::default(),
                        RubyStyle {
                            overhang,
                            ..Default::default()
                        },
                    ),
                );
            }
        }
        let line = layout(b, &atomics);
        assert_eq!(line.inline_size(), want, "{neighbor}/{overhang:?}");
        if neighbor == "plain" && overhang == RubyOverhang::Auto {
            let reading = line
                .ruby_annotations()
                .find(|a| a.container() == NodeId(8))
                .unwrap();
            assert_eq!(
                reading.origin().0,
                18.0,
                "half-ic6px may hang over each24px plain neighbor"
            );
            assert_eq!(reading.line().inline_size(), 36.0);
            assert_eq!(glyph_positions(&line), [0.0, 24.0, 48.0]);
        }
    }
}

#[test]
fn overhang_agrees_with_intrinsics_plans_and_width_retries_on_the_actual_line() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        pair("日", "にほん", RubyAlign::default(), RubyStyle::default()),
    );
    b.push_text(TextSource::Generated { node: NodeId(2) }, "日");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(intrinsic.max_content, 72.0);
    assert_eq!(
        intrinsic.min_content, 36.0,
        "an isolated ruby has no neighboring text to overhang"
    );
    let mut cx = LayoutContext::new();
    for (width, want, count, reading_origin) in [
        (70.0, 54.0, 2, 18.0),
        (72.0, 72.0, 3, 18.0),
        (70.0, 54.0, 2, 18.0),
    ] {
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("retry actual ruby edge")
        };
        assert_eq!(line.inline_size(), want);
        assert_eq!(glyph_positions(&line).len(), count);
        let reading = line.ruby_annotations().next().unwrap();
        assert_eq!(reading.origin().0, reading_origin);
        assert_eq!(reading.line().inline_size(), 36.0);
    }
    for wrap in [
        shodo::style::TextWrapStyle::Balance,
        shodo::style::TextWrapStyle::Pretty,
    ] {
        let options = shodo::style::LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(&mut cx, &options, 72.0, &AtomicSizes::EMPTY);
        let mut constraint = LineConstraint::new(72.0);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned actual neighbors")
        };
        assert_eq!(line.inline_size(), 72.0);
        assert_eq!(glyph_positions(&line), [0.0, 24.0, 48.0]);
        assert!(matches!(
            p.next_line(
                &mut cx,
                line.break_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY
            ),
            LineResult::Done
        ));
    }
}

#[test]
fn overhang_clearance_uses_the_annotation_side_after_neighbor_displacement() {
    for (position, shift, want) in [
        (RubyPosition::Over, 24.0, 84.0),
        (RubyPosition::Under, -24.0, 84.0),
        (RubyPosition::Over, -24.0, 72.0),
        (RubyPosition::Under, 24.0, 72.0),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        for (i, node) in [(0, 1), (1, 2)] {
            b.open_inline(
                NodeId(100 + node),
                &InlineStyle {
                    vertical_align: shodo::style::VerticalAlign::Length(shift),
                    ..style(24.0)
                },
                Default::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
            b.close_inline();
            if i == 0 {
                b.push_ruby(
                    NodeId(8),
                    &style(24.0),
                    pair(
                        "日",
                        "にほん",
                        RubyAlign::default(),
                        RubyStyle {
                            position,
                            ..Default::default()
                        },
                    ),
                );
            }
        }
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(
            line.inline_size(),
            want,
            "{position:?}, displaced neighbor {shift}"
        );
    }
}

#[test]
fn overhang_clearance_includes_neighbor_block_padding_and_borders() {
    for (position, start, end, want) in [
        (RubyPosition::Over, 20.0, 0.0, 84.0),
        (RubyPosition::Under, 0.0, 20.0, 84.0),
        (RubyPosition::Over, 0.0, 20.0, 72.0),
        (RubyPosition::Under, 20.0, 0.0, 72.0),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        for (i, node) in [(0, 1), (1, 2)] {
            // The outer box has no inline edge. Its block edge still occupies
            // the annotation track, even through a transparent inner box.
            b.open_inline(
                NodeId(100 + node),
                &style(24.0),
                shodo::node::InlineEdges {
                    padding: shodo::node::Sides {
                        block_start: start,
                        ..Default::default()
                    },
                    border: shodo::node::Sides {
                        block_end: end,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            b.open_inline(NodeId(200 + node), &style(24.0), Default::default());
            b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
            b.close_inline();
            b.close_inline();
            if i == 0 {
                b.push_ruby(
                    NodeId(8),
                    &style(24.0),
                    pair(
                        "日",
                        "にほん",
                        RubyAlign::default(),
                        RubyStyle {
                            position,
                            ..Default::default()
                        },
                    ),
                );
            }
        }
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(
            line.inline_size(),
            want,
            "{position:?}, block edges {start}/{end}"
        );
    }
}

#[test]
fn alternate_levels_stack_on_literal_physical_sides_in_both_vertical_modes() {
    for (mode, position, origins, physical_x) in [
        (
            WritingMode::VerticalRl,
            RubyPosition::Alternate,
            [12.0, 48.0, 0.0],
            [36.0, 0.0, 48.0],
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::Alternate,
            [36.0, 0.0, 48.0],
            [36.0, 0.0, 48.0],
        ),
        (
            WritingMode::VerticalRl,
            RubyPosition::AlternateUnder,
            [36.0, 0.0, 48.0],
            [12.0, 48.0, 0.0],
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::AlternateUnder,
            [12.0, 48.0, 0.0],
            [12.0, 48.0, 0.0],
        ),
    ] {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: content(10, "日", 24.0),
                align: RubyAlign::default(),
            }],
            ["に", "ほ", "ん"]
                .into_iter()
                .enumerate()
                .map(|(i, reading)| RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(20 + i as u64),
                        content: content(20 + i as u64, reading, 12.0),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle {
                        position,
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                })
                .collect(),
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), ruby);
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.block_size(), 60.0);
        let converter = shodo::geometry::PhysicalConverter::new(
            mode,
            shodo::geometry::Direction::Ltr,
            shodo::geometry::PhysicalSize {
                width: 60.0,
                height: line.inline_size(),
            },
        );
        for (i, annotation) in line.ruby_annotations().enumerate() {
            assert_eq!(
                annotation.origin().1,
                origins[i],
                "{mode:?}/{position:?}/level{i}"
            );
            let rect = converter.rect(shodo::geometry::LogicalRect {
                inline_start: annotation.origin().0,
                block_start: annotation.origin().1,
                inline_size: annotation.line().inline_size(),
                block_size: annotation.line().block_size(),
            });
            assert_eq!(
                rect.x, physical_x[i],
                "physical {mode:?}/{position:?}/level{i}"
            );
        }
    }
}

#[test]
fn overhang_checks_the_visual_neighbor_inside_an_opposing_bidi_box() {
    for (bidi, origin) in [
        (shodo::style::UnicodeBidi::BidiOverride, 0.0),
        (shodo::style::UnicodeBidi::IsolateOverride, 72.0),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.open_inline(
            NodeId(100),
            &InlineStyle {
                direction: shodo::geometry::Direction::Rtl,
                unicode_bidi: bidi,
                ..style(24.0)
            },
            Default::default(),
        );
        b.open_inline(NodeId(101), &style(48.0), Default::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        b.close_inline();
        b.push_text(TextSource::Generated { node: NodeId(2) }, "本");
        b.close_inline();
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair("語", "にほん", RubyAlign::default(), RubyStyle::default()),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(
            line.inline_size(),
            108.0,
            "{bidi:?}: the48px glyph is the actual visual neighbor, so no6px hang is safe"
        );
        let reading = line.ruby_annotations().next().unwrap();
        // An override's embedding also resolves the following neutral isolate;
        // isolate-override keeps the ruby after the independent neighboring box.
        assert_eq!(reading.origin().0, origin, "{bidi:?}");
    }
}

#[test]
fn nested_vertical_ruby_metrics_and_block_size_retries_use_all_annotation_levels() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (mode, inner_origin, outer_origin) in [
        (WritingMode::VerticalRl, 12.0, 0.0),
        (WritingMode::VerticalLr, 24.0, 36.0),
    ] {
        let mut base = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        base.push_ruby(
            NodeId(81),
            &style(24.0),
            pair(
                "日",
                "に",
                RubyAlign::default(),
                RubyStyle {
                    position: RubyPosition::Over,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::from_builder(base),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(30),
                    content: content(30, "ほ", 12.0),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    position: RubyPosition::Over,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), ruby);
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let mut cx = LayoutContext::new();
        let mut constraint = LineConstraint::new(1000.0);
        constraint.max_block_size = Some(47.0);
        for _ in 0..2 {
            let LineResult::BlockSizeExceeded { needed_block_size } = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("both levels must fit before accepting")
            };
            assert_eq!(needed_block_size, 48.0);
        }
        constraint.max_block_size = Some(48.0);
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("retry exact nested height")
        };
        assert_eq!(line.block_size(), 48.0);
        assert_eq!(line.ruby_annotations().len(), 2);
        let inner = line
            .ruby_annotations()
            .find(|a| a.container() == NodeId(81))
            .unwrap();
        let outer = line
            .ruby_annotations()
            .find(|a| a.container() == NodeId(8))
            .unwrap();
        assert_eq!(inner.origin().1, inner_origin);
        assert_eq!(outer.origin().1, outer_origin);
        assert_eq!(inner.line().block_size(), 12.0);
        assert_eq!(outer.line().block_size(), 12.0);
        let overflow = line.overflow_rect();
        for annotation in [inner, outer] {
            let child = annotation.line().overflow_rect();
            assert!(overflow.block_start <= annotation.origin().1 + child.block_start);
            assert!(
                overflow.block_start + overflow.block_size
                    >= annotation.origin().1 + child.block_start + child.block_size
            );
        }
        assert!(matches!(
            p.next_line(
                &mut cx,
                line.break_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY
            ),
            LineResult::Done
        ));
    }
}

#[test]
fn nested_overhang_clearance_uses_the_complete_inner_ruby_area() {
    for (mode, position, want) in [
        (WritingMode::VerticalRl, RubyPosition::Under, 144.0),
        (WritingMode::VerticalLr, RubyPosition::Under, 144.0),
        (WritingMode::VerticalRl, RubyPosition::Over, 132.0),
        (WritingMode::VerticalLr, RubyPosition::Over, 132.0),
    ] {
        let mut base = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        base.push_ruby(
            NodeId(81),
            &style(24.0),
            pair(
                "日",
                "に",
                RubyAlign::default(),
                RubyStyle {
                    position: RubyPosition::Over,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::from_builder(base),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(30),
                    content: content(30, "ほほほほほほ", 12.0),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    position,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        for (i, node) in [(0, 1), (1, 2)] {
            b.open_inline(NodeId(100 + node), &style(36.0), Default::default());
            b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
            b.close_inline();
            if i == 0 {
                b.push_ruby(NodeId(8), &style(24.0), ruby.clone());
            }
        }
        let line = layout(b, &AtomicSizes::EMPTY);
        // The inner12px Over track clears the36px neighboring font content
        // for an outer Over annotation. It provides no extra Under clearance.
        assert_eq!(line.inline_size(), want, "{mode:?}/{position:?}");
        assert_eq!(line.ruby_annotations().len(), 2);
    }
}

#[test]
fn top_bottom_neighbors_use_the_selected_line_clearance_for_auto_overhang() {
    use shodo::style::VerticalAlign;
    // A60px strut moves a24px Top neighbor toward line-over and a Bottom
    // neighbor toward line-under. Only the opposite annotation side clears.
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for (alignment, position, want) in [
            (VerticalAlign::Top, RubyPosition::Over, 84.0),
            (VerticalAlign::Top, RubyPosition::Under, 72.0),
            (VerticalAlign::Bottom, RubyPosition::Over, 72.0),
            (VerticalAlign::Bottom, RubyPosition::Under, 84.0),
        ] {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    writing_mode: mode,
                    root: InlineStyle {
                        line_height: LineHeight::Px(60.0),
                        ..style(24.0)
                    },
                    ..Default::default()
                },
                &Limits::default(),
            );
            for (i, node) in [(0, 1), (1, 2)] {
                b.open_inline(
                    NodeId(100 + node),
                    &InlineStyle {
                        vertical_align: alignment,
                        ..style(24.0)
                    },
                    Default::default(),
                );
                b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
                b.close_inline();
                if i == 0 {
                    b.push_ruby(
                        NodeId(8),
                        &style(24.0),
                        pair(
                            "日",
                            "にほん",
                            RubyAlign::default(),
                            RubyStyle {
                                position,
                                ..Default::default()
                            },
                        ),
                    );
                }
            }
            let line = layout(b, &AtomicSizes::EMPTY);
            assert_eq!(
                line.inline_size(),
                want,
                "{mode:?}/{alignment:?}/{position:?}"
            );
            if want == 72.0 {
                let reading = line.ruby_annotations().next().unwrap();
                assert_eq!(reading.origin().0, 18.0);
                assert_eq!(reading.line().inline_size(), 36.0);
            }
        }
    }
}

fn rtl_content(node: u64, text: &str, size: f32, override_rtl: bool) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 40,
        },
        text,
        &InlineStyle {
            direction: shodo::geometry::Direction::Rtl,
            unicode_bidi: if override_rtl {
                shodo::style::UnicodeBidi::BidiOverride
            } else {
                shodo::style::UnicodeBidi::Normal
            },
            ..style(size)
        },
        &Limits::default(),
    )
}

#[test]
fn rtl_columns_hang_only_over_the_neighbor_at_their_actual_visual_edge() {
    use shodo::geometry::Direction;
    for (override_rtl, plain_on_right, want, origin) in [
        (false, false, 102.0, 18.0),
        (false, true, 108.0, 24.0),
        (true, false, 108.0, 48.0),
        (true, true, 102.0, 48.0),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        let mut atomics = AtomicSizes::new();
        for (side, node) in [(0, 1), (1, 2)] {
            let plain = if side == 0 {
                !plain_on_right
            } else {
                plain_on_right
            };
            if plain {
                b.push_text(TextSource::Generated { node: NodeId(node) }, "日");
            } else {
                b.push_atomic(NodeId(node), &style(24.0), Default::default());
                atomics.insert(
                    NodeId(node),
                    AtomicSize {
                        inline_size: 24.0,
                        block_size: 24.0,
                        ..Default::default()
                    },
                );
            }
            if side == 0 {
                let ruby = Ruby::new(
                    vec![
                        RubyBase {
                            node: NodeId(10),
                            content: rtl_content(10, "日", 24.0, override_rtl),
                            align: RubyAlign::default(),
                        },
                        RubyBase {
                            node: NodeId(11),
                            content: rtl_content(11, "本", 24.0, override_rtl),
                            align: RubyAlign::default(),
                        },
                    ],
                    vec![RubyLevel {
                        annotations: vec![
                            RubyAnnotation {
                                node: NodeId(20),
                                content: content(20, "にほん", 12.0),
                                span: RubySpan::Auto,
                                visibility: RubyVisibility::Visible,
                            },
                            RubyAnnotation {
                                node: NodeId(21),
                                content: content(21, "ほ", 12.0),
                                span: RubySpan::Auto,
                                visibility: RubyVisibility::Visible,
                            },
                        ],
                        style: RubyStyle::default(),
                    }],
                )
                .unwrap();
                b.push_ruby(
                    NodeId(8),
                    &InlineStyle {
                        direction: Direction::Rtl,
                        unicode_bidi: if override_rtl {
                            shodo::style::UnicodeBidi::BidiOverride
                        } else {
                            shodo::style::UnicodeBidi::Normal
                        },
                        ..style(24.0)
                    },
                    ruby,
                );
            }
        }
        let line = layout(b, &atomics);
        assert_eq!(line.inline_size(), want, "plain_on_right={plain_on_right}");
        let reading = line
            .ruby_annotations()
            .find(|a| a.node() == Some(NodeId(20)))
            .unwrap();
        assert_eq!(
            reading.origin().0,
            origin,
            "annotation must start at its actual visual column edge"
        );
        assert_eq!(reading.line().inline_size(), 36.0);
    }
}

#[test]
fn rtl_spanning_and_merged_readings_follow_actual_column_geometry() {
    for (override_rtl, merge, first_origin, second_origin) in [
        (false, shodo::RubyMerge::Separate, 24.0, 0.0),
        (false, shodo::RubyMerge::Merge, 24.0, 60.0),
        (true, shodo::RubyMerge::Separate, 24.0, 0.0),
        (true, shodo::RubyMerge::Merge, 60.0, 24.0),
    ] {
        let annotations = if merge == shodo::RubyMerge::Separate {
            vec![RubyAnnotation {
                node: NodeId(20),
                content: content(20, "にほんにほん", 12.0),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }]
        } else {
            vec![
                RubyAnnotation {
                    node: NodeId(20),
                    content: content(20, "にほん", 12.0),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                },
                RubyAnnotation {
                    node: NodeId(21),
                    content: content(21, "ほにん", 12.0),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                },
            ]
        };
        let ruby = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(10),
                    content: rtl_content(10, "日", 24.0, override_rtl),
                    align: RubyAlign::default(),
                },
                RubyBase {
                    node: NodeId(11),
                    content: rtl_content(11, "本", 24.0, override_rtl),
                    align: RubyAlign::default(),
                },
            ],
            vec![RubyLevel {
                annotations,
                style: RubyStyle {
                    merge,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        b.push_ruby(
            NodeId(8),
            &InlineStyle {
                direction: shodo::geometry::Direction::Rtl,
                unicode_bidi: if override_rtl {
                    shodo::style::UnicodeBidi::BidiOverride
                } else {
                    shodo::style::UnicodeBidi::Normal
                },
                ..style(24.0)
            },
            ruby,
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 96.0);
        let first = line
            .ruby_annotations()
            .find(|a| a.node() == Some(NodeId(20)))
            .unwrap();
        assert_eq!(first.origin().0, first_origin, "{merge:?}");
        if merge == shodo::RubyMerge::Merge {
            let second = line
                .ruby_annotations()
                .find(|a| a.node() == Some(NodeId(21)))
                .unwrap();
            assert_eq!(second.origin().0, second_origin);
            assert_eq!(first.base_nodes(), [NodeId(10)]);
            assert_eq!(second.base_nodes(), [NodeId(11)]);
        }
    }
}

#[test]
fn single_typographic_unit_centers_for_space_between_without_changing_advances() {
    for (base, reading, parent_positions, reading_positions) in [
        ("日", "にほん", vec![6.0], vec![0.0, 12.0, 24.0]),
        ("日本語", "に", vec![0.0, 24.0, 48.0], vec![30.0]),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair(
                base,
                reading,
                RubyAlign::SpaceBetween,
                RubyStyle {
                    align: RubyAlign::SpaceBetween,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(glyph_positions(&line), parent_positions);
        assert_eq!(
            glyph_positions(line.ruby_annotations().next().unwrap().line()),
            reading_positions
        );
        assert!(
            line.fragments()
                .filter_map(|f| if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                })
                .all(|r| r.clusters().all(|c| c.shaping_advance == 24.0))
        );
    }
}

#[test]
fn vertical_align_displaces_base_and_reading_together_with_actual_line_advances() {
    use shodo::style::VerticalAlign;
    for (mode, position, shift, advance, origin, baseline, displacement) in [
        (
            WritingMode::VerticalRl,
            RubyPosition::Over,
            8.0,
            44.0,
            0.0,
            32.0,
            -8.0,
        ),
        (
            WritingMode::VerticalRl,
            RubyPosition::Over,
            -8.0,
            36.0,
            0.0,
            16.0,
            8.0,
        ),
        (
            WritingMode::VerticalRl,
            RubyPosition::Under,
            8.0,
            36.0,
            24.0,
            20.0,
            -8.0,
        ),
        (
            WritingMode::VerticalRl,
            RubyPosition::Under,
            -8.0,
            44.0,
            32.0,
            12.0,
            8.0,
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::Over,
            8.0,
            44.0,
            32.0,
            12.0,
            8.0,
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::Over,
            -8.0,
            36.0,
            24.0,
            20.0,
            -8.0,
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::Under,
            8.0,
            36.0,
            0.0,
            16.0,
            8.0,
        ),
        (
            WritingMode::VerticalLr,
            RubyPosition::Under,
            -8.0,
            44.0,
            0.0,
            32.0,
            -8.0,
        ),
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &InlineStyle {
                vertical_align: VerticalAlign::Length(shift),
                ..style(24.0)
            },
            pair(
                "日",
                "に",
                RubyAlign::default(),
                RubyStyle {
                    position,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.block_size(), advance, "{mode:?}/{position:?}/{shift}");
        assert_eq!(line.metrics().baseline, baseline);
        assert_eq!(line.ruby_annotations().next().unwrap().origin().1, origin);
        let run = line
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(run.baseline() - line.metrics().baseline, displacement);
    }
}

#[test]
fn hidden_readings_keep_size_and_trim_preserves_visible_annotation_clearance() {
    use shodo::style::{LineOptions, TextBoxTrim};
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (visibility, width, height, count) in [
        (RubyVisibility::Visible, 36.0, 36.0, 1),
        (RubyVisibility::Hidden, 36.0, 36.0, 1),
        (RubyVisibility::Collapse, 24.0, 24.0, 0),
    ] {
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: content(10, "日", 24.0),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: content(20, "にほん", 12.0),
                    span: RubySpan::All,
                    visibility,
                }],
                style: RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: WritingMode::VerticalRl,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), ruby);
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        for trim in [
            TextBoxTrim::None,
            TextBoxTrim::TrimStart,
            TextBoxTrim::TrimEnd,
            TextBoxTrim::TrimBoth,
        ] {
            let options = LineOptions {
                text_box_trim: trim,
                ..Default::default()
            };
            let mut cx = LayoutContext::new();
            let LineResult::Line(line) = p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &LineConstraint::new(1000.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("visibility/trim ruby")
            };
            assert_eq!(line.inline_size(), width, "{visibility:?}/{trim:?}");
            assert_eq!(line.block_size(), height, "{visibility:?}/{trim:?}");
            assert_eq!(line.ruby_annotations().count(), count);
            if count != 0 {
                assert_eq!(
                    line.ruby_annotations().next().unwrap().visibility(),
                    visibility
                );
            }
        }
    }
}

#[test]
fn opposite_annotation_direction_maps_its_actual_inline_axis_into_the_parent() {
    use shodo::geometry::{Direction, PhysicalConverter, PhysicalSize};
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        for (parent_direction, child_direction, scale, physical_step) in [
            (Direction::Ltr, Direction::Ltr, 1.0, 12.0),
            (Direction::Rtl, Direction::Rtl, 1.0, -12.0),
            (Direction::Rtl, Direction::Ltr, -1.0, 12.0),
            (Direction::Ltr, Direction::Rtl, -1.0, -12.0),
        ] {
            let mut reading = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction: child_direction,
                    root: InlineStyle {
                        direction: child_direction,
                        ..style(12.0)
                    },
                    ..Default::default()
                },
                &Limits::default(),
            );
            reading.push_text(
                TextSource::Dom {
                    node: NodeId(20),
                    offset: 40,
                },
                "にほん",
            );
            let ruby = Ruby::new(
                vec![RubyBase {
                    node: NodeId(10),
                    content: content(10, "日", 24.0),
                    align: RubyAlign::default(),
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(20),
                        content: RubyContent::from_builder(reading),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle {
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                }],
            )
            .unwrap();
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction: parent_direction,
                    writing_mode: mode,
                    root: InlineStyle {
                        direction: parent_direction,
                        ..style(24.0)
                    },
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_ruby(
                NodeId(8),
                &InlineStyle {
                    direction: parent_direction,
                    ..style(24.0)
                },
                ruby,
            );
            let line = layout(b, &AtomicSizes::EMPTY);
            let reading = line.ruby_annotations().next().unwrap();
            assert_eq!(reading.line().used_direction(), child_direction);
            let t = reading.transform();
            assert_eq!(
                t.inline_inline, scale,
                "{mode:?}/{parent_direction:?}/{child_direction:?}"
            );
            assert_eq!(
                (t.inline_block, t.block_inline, t.block_block),
                (0.0, 0.0, 1.0)
            );
            let converter = PhysicalConverter::new(
                mode,
                parent_direction,
                PhysicalSize {
                    width: 100.0,
                    height: 100.0,
                },
            );
            let (x, y) = converter.vector(t.inline_inline * 12.0, t.block_inline * 12.0);
            if mode == WritingMode::HorizontalTb {
                assert_eq!((x, y), (physical_step, 0.0));
            } else {
                assert_eq!((x, y), (0.0, physical_step));
            }
            let edge0 = t.inline_offset;
            let edge1 = t.inline_offset + t.inline_inline * reading.line().inline_size();
            assert_eq!(edge0.min(edge1), 0.0);
            assert_eq!(edge0.max(edge1), 36.0);
        }
    }
}

#[test]
fn annotation_line_height_is_ignored_while_actual_descendant_font_size_is_retained() {
    for (mode, advance, child_height) in [
        (WritingMode::HorizontalTb, 60.828125, 26.078125),
        (WritingMode::VerticalRl, 42.0, 18.0),
        (WritingMode::VerticalLr, 42.0, 18.0),
    ] {
        let mut reading = ParagraphBuilder::new(
            &ParagraphStyle {
                root: InlineStyle {
                    line_height: LineHeight::Px(120.0),
                    ..style(12.0)
                },
                ..Default::default()
            },
            &Limits::default(),
        );
        reading.open_inline(
            NodeId(22),
            &InlineStyle {
                line_height: LineHeight::Px(400.0),
                ..style(18.0)
            },
            Default::default(),
        );
        reading.push_text(
            TextSource::Dom {
                node: NodeId(20),
                offset: 40,
            },
            "に",
        );
        reading.close_inline();
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: content(10, "日", 24.0),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: RubyContent::from_builder(reading),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), ruby);
        let line = layout(b, &AtomicSizes::EMPTY);
        let reading = line.ruby_annotations().next().unwrap();
        assert_eq!(line.block_size(), advance, "{mode:?}");
        assert_eq!(reading.line().block_size(), child_height);
        let run = reading
            .line()
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(run.font_size(), 18.0);
        assert!(run.clusters().all(|c| c.shaping_advance == 18.0));
    }
}

#[test]
fn inter_character_spanning_reading_uses_the_actual_rightmost_bidi_column() {
    for (override_rtl, first, second) in [(false, 0.0, 24.0), (true, 48.0, 0.0)] {
        let ruby = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(10),
                    content: rtl_content(10, "日", 24.0, override_rtl),
                    align: RubyAlign::Start,
                },
                RubyBase {
                    node: NodeId(11),
                    content: rtl_content(11, "本", 48.0, override_rtl),
                    align: RubyAlign::Start,
                },
            ],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: content(20, "にほん", 12.0),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle {
                    align: RubyAlign::Start,
                    position: RubyPosition::InterCharacter,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &InlineStyle {
                direction: shodo::geometry::Direction::Rtl,
                unicode_bidi: if override_rtl {
                    shodo::style::UnicodeBidi::BidiOverride
                } else {
                    shodo::style::UnicodeBidi::Normal
                },
                ..style(24.0)
            },
            ruby,
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 84.0);
        let positions: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(run) => Some((
                    run.text_range(),
                    run.glyphs().next().unwrap().inline_position,
                )),
                _ => None,
            })
            .collect();
        for (text, expected) in [("日", first), ("本", second)] {
            let offset = line.text().find(text).unwrap();
            let actual = positions
                .iter()
                .find(|(range, _)| range.contains(&offset))
                .unwrap()
                .1;
            assert_eq!(
                actual, expected,
                "override={override_rtl} base={text}: cross slot belongs after the visual right edge"
            );
        }
        let reading = line.ruby_annotations().next().unwrap();
        assert_eq!(
            reading.origin().0,
            84.0,
            "override={override_rtl}: reading lies in x72..84 beside both bases"
        );
        assert_eq!(reading.line().block_size(), 12.0);
        assert_eq!(reading.transform().inline_block, -1.0);
    }
}

#[test]
fn sideways_modes_retain_actual_alphabetic_font_content_metrics() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &style(24.0),
            pair(
                "日",
                "にほん",
                RubyAlign::Start,
                RubyStyle {
                    position: RubyPosition::Over,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            ),
        );
        let line = layout(b, &AtomicSizes::EMPTY);
        assert_eq!(
            line.block_size(),
            52.140625,
            "{mode:?} actual hhea content at24px/12px"
        );
        assert_eq!(
            line.ruby_annotations().next().unwrap().line().block_size(),
            17.390625
        );
    }
}
