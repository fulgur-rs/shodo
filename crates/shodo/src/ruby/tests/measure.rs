use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::ruby::measure::{SpanWidth, column_widths};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, LineBreak, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, Paragraph, ParagraphBuilder};

fn width(value: f32) -> LayoutUnit {
    LayoutUnit::from_f32_round(value, &mut Saturation::default())
}

#[test]
fn spanning_widths_expand_shortest_spans_first() {
    // [10,10,10] → [20,10,10] → [30,20,10] → [40,30,20]:
    // each span adds its excess equally, preserving earlier column maxima.
    // Starting with the longest span would incorrectly produce30/30/30.
    let spans = [
        SpanWidth {
            columns: 0..3,
            width: width(90.0),
        },
        SpanWidth {
            columns: 0..2,
            width: width(50.0),
        },
        SpanWidth {
            columns: 0..1,
            width: width(20.0),
        },
    ];
    let result = column_widths(&[width(10.0); 3], &spans, &mut Saturation::default());
    assert_eq!(
        result.iter().map(|w| w.to_f32()).collect::<Vec<_>>(),
        [40.0, 30.0, 20.0]
    );
}

#[test]
fn narrow_annotations_do_not_reduce_actual_base_advances() {
    let spans = [SpanWidth {
        columns: 0..2,
        width: width(12.0),
    }];
    let result = column_widths(&[width(24.0); 2], &spans, &mut Saturation::default());
    assert_eq!(result, [width(24.0); 2]);
}

#[test]
fn span_expansion_preserves_the_exact_fixed_point_width() {
    let spans = [SpanWidth {
        columns: 0..3,
        width: LayoutUnit::from_raw(7),
    }];
    let result = column_widths(
        &[LayoutUnit::from_raw(2); 3],
        &spans,
        &mut Saturation::default(),
    );
    assert_eq!(
        result.iter().map(|w| w.raw()).collect::<Vec<_>>(),
        [3, 2, 2]
    );
}

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        line_break: LineBreak::Anywhere,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}

fn ruby(base: RubyContent, reading: &str) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base,
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20),
                        offset: 70,
                    },
                    reading,
                    &style(12.0),
                    &Limits::default(),
                ),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap()
}

fn base(text: &str) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 40,
        },
        text,
        &style(24.0),
        &Limits::default(),
    )
}

fn fonts() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn build(ruby: Ruby) -> Paragraph {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style(24.0), ruby);
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

#[test]
fn candidate_reserves_actual_annotation_width_without_formatting_child_lines() {
    let p = build(ruby(base("日"), "にほん"));
    let measure = crate::ruby::measure::candidate(
        &p.data,
        0,
        p.data.units.len(),
        &AtomicSizes::EMPTY,
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    );
    assert_eq!(
        measure.adjustment.to_f32(),
        12.0,
        "24px base becomes a36px pair"
    );
    assert_eq!(measure.fragments.len(), 1);
    let fragment = &measure.fragments[0];
    assert_eq!(
        fragment
            .columns
            .iter()
            .map(|w| w.to_f32())
            .collect::<Vec<_>>(),
        [36.0]
    );
    assert_eq!(fragment.lanes[0].width.to_f32(), 36.0);
    assert_eq!(
        fragment.lanes[0].units,
        0..p.data.ruby.containers[0].lanes[0]
            .paragraph
            .data
            .units
            .len()
    );
}

#[test]
fn candidate_ranges_follow_the_prepared_parallel_cursors() {
    let p = build(ruby(base("日本語日本語"), "にほんごにほんご"));
    let cuts = &p.data.ruby.containers[0].cuts;
    let measure = crate::ruby::measure::candidate(
        &p.data,
        cuts[0].unit,
        cuts[2].unit,
        &AtomicSizes::EMPTY,
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    );
    assert_eq!(measure.adjustment, LayoutUnit::ZERO);
    let lane = &measure.fragments[0].lanes[0];
    assert_eq!(lane.units, cuts[0].lanes[0]..cuts[2].lanes[0]);
    assert_eq!(
        lane.width.to_f32(),
        36.0,
        "two bases pair with the nearest three reading characters"
    );
    let child = &p.data.ruby.containers[0].lanes[0].paragraph;
    let text: String = child.data.units[lane.units.clone()]
        .iter()
        .filter(|u| matches!(u.kind, crate::analysis::units::UnitKind::Cluster { .. }))
        .map(|u| &child.text()[u.text.start as usize..u.text.end as usize])
        .collect();
    assert_eq!(text, "にほん");
}

#[test]
fn nested_candidate_expansion_is_charged_once_inside_its_outer_base() {
    let mut inner = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    inner.push_ruby(NodeId(18), &style(24.0), ruby(base("日"), "にほん"));
    let p = build(ruby(RubyContent::from_builder(inner), "かな"));
    let measure = crate::ruby::measure::candidate(
        &p.data,
        0,
        p.data.units.len(),
        &AtomicSizes::EMPTY,
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    );
    assert_eq!(measure.fragments.len(), 2);
    assert_eq!(measure.adjustment.to_f32(), 12.0);
    assert!(
        measure
            .fragments
            .iter()
            .all(|f| f.columns.iter().map(|w| w.to_f32()).sum::<f32>() == 36.0)
    );
}

#[test]
fn accepted_line_and_intrinsics_reserve_the_same_actual_lane_width() {
    let p = build(ruby(base("日"), "にほん"));
    let options = Default::default();
    let mut cx = LayoutContext::new();
    let crate::LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &options,
        &crate::LineConstraint::new(30.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("the unbreakable pair must overflow intact")
    };
    assert_eq!(line.inline_size(), 36.0);
    let intrinsic = p.intrinsic_sizes(&mut cx, &options, &Default::default());
    assert_eq!(intrinsic.min_content, 36.0);
    assert_eq!(intrinsic.max_content, 36.0);
}

#[test]
fn line_ends_at_the_paired_cut_without_consuming_the_next_base_open() {
    let mut pair = ruby(base("日"), "に");
    pair.bases.push(RubyBase {
        node: NodeId(11),
        content: base("本"),
        align: RubyAlign::default(),
    });
    pair.levels[0].annotations[0].span = RubySpan::Auto;
    let mut second = pair.levels[0].annotations[0].clone();
    second.node = NodeId(21);
    second.content = RubyContent::text(
        TextSource::Generated { node: NodeId(21) },
        "ほん",
        &style(12.0),
        &Limits::default(),
    );
    pair.levels[0].annotations.push(second);
    let p = build(Ruby::new(pair.bases, pair.levels).unwrap());
    let boundary = p.data.ruby.containers[0].cuts[1].unit;
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(24.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("first column")
    };
    assert_eq!(line.units.end as usize, boundary);
    assert_eq!(line.inline_size(), 24.0);
}

#[test]
fn real_long_ruby_candidates_use_indexed_costs_instead_of_rescanning_prefixes() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let p = build(ruby(
            base(&"日本語".repeat(count)),
            &"にほんご".repeat(count),
        ));
        let mut cx = LayoutContext::new();
        for cut in &p.data.ruby.containers[0].cuts[1..] {
            let m = crate::ruby::measure::candidate(
                &p.data,
                0,
                cut.unit,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert_eq!(m.adjustment, LayoutUnit::ZERO);
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "doubling actual prepared input rescanned prefixes: {visits:?}"
    );
}

#[test]
fn ancestor_cloned_edges_do_not_hide_the_annotation_width_deficit() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(
        NodeId(100),
        &InlineStyle {
            box_decoration_break: crate::style::BoxDecorationBreak::Clone,
            ..style(24.0)
        },
        crate::node::InlineEdges {
            padding: crate::node::Sides {
                inline_start: 10.0,
                inline_end: 10.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    b.push_ruby(NodeId(8), &style(24.0), ruby(base("日"), "にほん"));
    b.close_inline();
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("cloned parent")
    };
    assert_eq!(
        line.inline_size(),
        56.0,
        "36px paired content and two10px parent edges"
    );
}

fn glyph_positions(line: &crate::Line) -> Vec<f32> {
    line.fragments()
        .filter_map(|f| match f {
            crate::Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().map(|g| g.inline_position))
        .collect()
}

#[test]
fn default_alignment_centers_the_base_and_spaces_the_reading_without_scaling() {
    let p = build(ruby(base("日"), "にほん"));
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("short ruby")
    };
    assert_eq!(glyph_positions(&line), [6.0]);
    let run = line
        .fragments()
        .find_map(|f| match f {
            crate::Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(run.clusters().next().unwrap().shaping_advance, 24.0);
    let p = build(ruby(base("日本"), "にほん"));
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("wide base")
    };
    let annotation = line.ruby_annotations().next().unwrap();
    let positions: Vec<_> = glyph_positions(annotation.line())
        .into_iter()
        .map(|p| p + annotation.origin().0)
        .collect();
    assert_eq!(
        positions,
        [2.0, 18.0, 34.0],
        "three12px units use equal4px gaps and2px edge gaps in48px"
    );
    assert!(
        annotation
            .line()
            .fragments()
            .filter_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .all(|r| r.font_size() == 12.0)
    );
}

#[test]
fn real_dynamic_ruby_prefix_costs_include_query_work_and_remain_bounded() {
    for tabs in [false, true] {
        let mut visits = Vec::new();
        for count in [64, 128] {
            let base_style = InlineStyle {
                white_space_collapse: crate::style::WhiteSpaceCollapse::Preserve,
                tab_size: crate::style::TabSize::Px(48.0),
                ..style(24.0)
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: base_style.clone(),
                    ..Default::default()
                },
                &Limits::default(),
            );
            let mut atomics = AtomicSizes::new();
            for i in 0..count {
                b.push_text(
                    TextSource::Generated { node: NodeId(10) },
                    if tabs { "日\t" } else { "日" },
                );
                if !tabs {
                    let node = NodeId(100 + i as u64);
                    b.push_atomic(node, &base_style, crate::node::InlineEdges::default());
                    atomics.insert(
                        node,
                        crate::AtomicSize {
                            inline_size: 18.0,
                            block_size: 24.0,
                            ..Default::default()
                        },
                    );
                }
            }
            let p = build(ruby(RubyContent::from_builder(b), &"にほん".repeat(count)));
            let mut cx = LayoutContext::new();
            for cut in &p.data.ruby.containers[0].cuts[1..] {
                crate::ruby::measure::candidate(
                    &p.data,
                    0,
                    cut.unit,
                    &atomics,
                    &mut cx,
                    &mut Saturation::default(),
                );
            }
            assert!(cx.take_warnings().is_empty());
            visits.push(cx.ruby_measure_visits);
        }
        assert!(
            visits[1] <= visits[0] * 3,
            "tabs{tabs}: unit and range-node query visits{visits:?}"
        );
    }
}

#[test]
fn merged_readings_share_one_alignment_group_and_keep_each_source_pair() {
    for (first, second, expected) in [
        ("にほん", "ご", vec![0.0, 12.0, 24.0, 36.0]),
        ("にほ", "ん", vec![2.0, 18.0, 34.0]),
    ] {
        let pair = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(10),
                    content: base("日"),
                    align: RubyAlign::default(),
                },
                RubyBase {
                    node: NodeId(11),
                    content: base("本"),
                    align: RubyAlign::default(),
                },
            ],
            vec![RubyLevel {
                annotations: [(NodeId(20), first), (NodeId(21), second)]
                    .into_iter()
                    .map(|(node, text)| RubyAnnotation {
                        node,
                        content: RubyContent::text(
                            TextSource::Dom { node, offset: 70 },
                            text,
                            &style(12.0),
                            &Limits::default(),
                        ),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    })
                    .collect(),
                style: RubyStyle {
                    merge: RubyMerge::Merge,
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let p = build(pair);
        let crate::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("merged ruby")
        };
        assert_eq!(line.inline_size(), 48.0);
        let annotations: Vec<_> = line.ruby_annotations().collect();
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].base_nodes(), [NodeId(10)]);
        assert_eq!(annotations[1].base_nodes(), [NodeId(11)]);
        let positions: Vec<_> = annotations
            .iter()
            .flat_map(|a| {
                glyph_positions(a.line())
                    .into_iter()
                    .map(|p| p + a.origin().0)
            })
            .collect();
        assert_eq!(
            positions, expected,
            "accepted source-paired fragments concatenate within one48px group"
        );
        let narrow = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            24.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(narrow.len(), 2);
        assert_eq!(
            narrow[0].ruby_annotations().next().unwrap().base_nodes(),
            [NodeId(10)]
        );
        assert_eq!(
            narrow[1].ruby_annotations().next().unwrap().base_nodes(),
            [NodeId(11)]
        );
    }
}

#[test]
fn reused_source_nodes_keep_the_geometry_of_each_actual_base_box() {
    let pair = ruby(base("日"), "にほん");
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style(24.0), pair.clone());
    b.push_ruby(NodeId(9), &style(24.0), pair);
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("reused ruby input")
    };
    assert_eq!(line.inline_size(), 72.0);
    assert_eq!(glyph_positions(&line), [6.0, 42.0]);
    assert_eq!(
        line.ruby_annotations()
            .find(|a| a.container() == NodeId(8))
            .unwrap()
            .origin()
            .0,
        0.0
    );
    assert_eq!(
        line.ruby_annotations()
            .find(|a| a.container() == NodeId(9))
            .unwrap()
            .origin()
            .0,
        36.0
    );
}

#[test]
fn anonymous_base_reserves_its_reading_width_before_plain_text_and_a_float() {
    let pair = Ruby::new(
        Vec::new(),
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20),
                        offset: 70,
                    },
                    "にほん",
                    &style(12.0),
                    &Limits::default(),
                ),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    for float in [false, true] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), pair.clone());
        if float {
            b.push_out_of_flow(NodeId(40), crate::node::OutOfFlowKind::Float);
        }
        b.push_text(TextSource::Generated { node: NodeId(30) }, "日");
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let mut constraint = crate::LineConstraint::new(1000.0);
        let mut cx = LayoutContext::new();
        if float {
            let crate::LineResult::FloatEncountered {
                inline_position,
                float_cursor,
                ..
            } = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            )
            else {
                panic!("float after baseless annotation")
            };
            assert_eq!(inline_position, 36.0);
            constraint.floats_placed_through = Some(float_cursor);
        }
        let crate::LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("baseless annotation")
        };
        assert_eq!(line.inline_size(), 60.0);
        assert_eq!(glyph_positions(&line), [36.0]);
        let a = line.ruby_annotations().next().unwrap();
        assert!(a.base_nodes().is_empty());
        assert_eq!(a.node(), Some(NodeId(20)));
    }
}

fn latin_pair(first: &str, second: &str, overflow: crate::style::OverflowWrap) -> Paragraph {
    latin_pair_reading(first, second, overflow, "に")
}

fn latin_pair_reading(
    first: &str,
    second: &str,
    overflow: crate::style::OverflowWrap,
    reading: &str,
) -> Paragraph {
    let fonts = fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let latin = InlineStyle {
        font_size: 12.0,
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        overflow_wrap: overflow,
        ..Default::default()
    };
    let pair = Ruby::new(
        [(NodeId(10), first), (NodeId(11), second)]
            .into_iter()
            .map(|(node, text)| RubyBase {
                node,
                content: RubyContent::text(
                    TextSource::Dom { node, offset: 40 },
                    text,
                    &latin,
                    &Limits::default(),
                ),
                align: RubyAlign::Start,
            })
            .collect(),
        vec![RubyLevel {
            annotations: [(NodeId(20), reading), (NodeId(21), "ほ")]
                .into_iter()
                .map(|(node, text)| RubyAnnotation {
                    node,
                    content: RubyContent::text(
                        TextSource::Dom { node, offset: 70 },
                        text,
                        &style(6.0),
                        &Limits::default(),
                    ),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                })
                .collect(),
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    builder.push_ruby(NodeId(8), &latin, pair);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn emergency_base_boundary_keeps_its_reason_and_min_content_after_cached_retry() {
    let p = latin_pair("WW", "WW", crate::style::OverflowWrap::Anywhere);
    let mut warm = LayoutContext::new();
    assert!(matches!(
        p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY
        ),
        crate::LineResult::Line(_)
    ));
    for cx in [&mut warm, &mut LayoutContext::new()] {
        let crate::LineResult::Line(line) = p.next_line(
            cx,
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(24.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("emergency base boundary")
        };
        assert_eq!(
            p.text()[line.text_range()]
                .chars()
                .filter(char::is_ascii)
                .collect::<String>(),
            "WW"
        );
        assert_eq!(line.break_reason(), crate::BreakReason::Emergency);
    }
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert!(
        intrinsic.min_content < 24.0,
        "the moved Anywhere cut still limits min-content: {intrinsic:?}"
    );
    assert!(intrinsic.max_content > 40.0);
    for wrap in [
        crate::style::TextWrapStyle::Auto,
        crate::style::TextWrapStyle::Balance,
        crate::style::TextWrapStyle::Pretty,
    ] {
        let options = crate::style::LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            24.0,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = crate::LineConstraint::new(24.0);
        constraint.break_plan = Some(&plan);
        let crate::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned emergency")
        };
        assert_eq!(
            p.text()[line.text_range()]
                .chars()
                .filter(char::is_ascii)
                .collect::<String>(),
            "WW"
        );
        assert_eq!(line.break_reason(), crate::BreakReason::Emergency);
        assert_eq!(
            line.ruby_annotations().next().unwrap().base_nodes(),
            [NodeId(10)]
        );
    }
}

#[test]
fn manual_hyphen_at_a_base_boundary_is_shaped_and_survives_cached_retry() {
    let p = latin_pair("ab\u{ad}", "cd", crate::style::OverflowWrap::Normal);
    let mut warm = LayoutContext::new();
    assert!(matches!(
        p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY
        ),
        crate::LineResult::Line(_)
    ));
    for cx in [&mut warm, &mut LayoutContext::new()] {
        let crate::LineResult::Line(line) = p.next_line(
            cx,
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(19.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("manual hyphen boundary")
        };
        assert_eq!(
            p.text()[line.text_range()]
                .chars()
                .filter(|c| c.is_ascii() || *c == '\u{ad}')
                .collect::<String>(),
            "ab\u{ad}"
        );
        let ids: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect();
        // Pinned Latin charmap: '-' is glyph 16, U+2010 is absent.
        assert!(ids.contains(&16), "taken discretionary hyphen: {ids:?}");
        assert!(line.inline_size() > 17.0 && line.inline_size() < 19.0);
    }
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert!(
        intrinsic.min_content < 19.0,
        "manual break min-content: {intrinsic:?}"
    );
    assert!(intrinsic.max_content > 26.0);
    for wrap in [
        crate::style::TextWrapStyle::Auto,
        crate::style::TextWrapStyle::Balance,
        crate::style::TextWrapStyle::Pretty,
    ] {
        let options = crate::style::LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            19.0,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = crate::LineConstraint::new(19.0);
        constraint.break_plan = Some(&plan);
        let crate::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned hyphen")
        };
        assert_eq!(
            p.text()[line.text_range()]
                .chars()
                .filter(|c| c.is_ascii() || *c == '\u{ad}')
                .collect::<String>(),
            "ab\u{ad}"
        );
        let ids: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect();
        assert!(ids.contains(&16));
        assert_eq!(
            line.ruby_annotations().next().unwrap().base_nodes(),
            [NodeId(10)]
        );
    }
}

#[test]
fn long_manual_hyphen_candidates_keep_actual_range_costs_bounded() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let p = latin_pair_reading(
            &"ab\u{ad}".repeat(count),
            "cd",
            crate::style::OverflowWrap::Normal,
            &"に".repeat(count),
        );
        let mut cx = LayoutContext::new();
        let mut hyphens = 0;
        for cut in &p.data.ruby.containers[0].cuts[1..] {
            if cut.class == crate::analysis::units::BreakClass::Hyphen {
                crate::ruby::measure::candidate(
                    &p.data,
                    0,
                    cut.unit,
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut Saturation::default(),
                );
                hyphens += 1;
            }
        }
        assert!(
            hyphens > count / 2,
            "exercise actual paired hyphen candidates"
        );
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "actual hyphen range/query visits: {visits:?}"
    );
}

#[test]
fn horizontal_inter_character_reserves_thickness_and_maps_vertical_coordinates() {
    for (reading, child_height, parent_height, positions) in [
        // CJK hhea1160/-288 at24px: content bounds34.75px at1/64px,
        // normal line height rounds up to34.765625px.
        // Glyph origins include the pinned VORG880:12px*880/1000 rounds
        // to10.5625px, independently of the assigned inline alignment gaps.
        ("に", 34.75, 34.765625, vec![21.9375]),
        ("にほん", 36.0, 36.0, vec![10.5625, 22.5625, 34.5625]),
    ] {
        let pair = ruby(base("日"), reading);
        let mut levels = pair.levels;
        levels[0].style.position = RubyPosition::InterCharacter;
        let mut builder = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        builder.push_ruby(
            NodeId(8),
            &style(24.0),
            Ruby::new(pair.bases, levels).unwrap(),
        );
        let p = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let mut warm = LayoutContext::new();
        for cx in [&mut warm, &mut LayoutContext::new()] {
            let crate::LineResult::Line(line) = p.next_line(
                cx,
                p.start_token(),
                &Default::default(),
                &crate::LineConstraint::new(30.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!("upright inter-character pair")
            };
            assert_eq!(
                line.inline_size(),
                36.0,
                "24px base +12px vertical annotation thickness"
            );
            assert_eq!(
                glyph_positions(&line),
                [0.0],
                "the extra column is beside the base"
            );
            assert_eq!(line.block_size(), parent_height);
            let annotation = line.ruby_annotations().next().unwrap();
            let transform = annotation.transform();
            assert_eq!(
                (
                    transform.inline_inline,
                    transform.inline_block,
                    transform.block_inline,
                    transform.block_block
                ),
                (0.0, -1.0, 1.0, 0.0)
            );
            assert_eq!(annotation.origin(), (36.0, 0.0));
            assert_eq!(annotation.line().inline_size(), child_height);
            assert_eq!(annotation.line().block_size(), 12.0);
            assert_eq!(glyph_positions(annotation.line()), positions);
            assert_eq!(annotation.base_nodes(), [NodeId(10)]);
            assert_eq!(annotation.node(), Some(NodeId(20)));
        }
        let intrinsic = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &Default::default(),
        );
        assert_eq!((intrinsic.min_content, intrinsic.max_content), (36.0, 36.0));
    }
}

fn inter_character_content(content: RubyContent) -> Paragraph {
    let pair = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base("日"),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content,
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                position: RubyPosition::InterCharacter,
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    builder.push_ruby(NodeId(8), &style(24.0), pair);
    builder.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

#[test]
fn inter_character_thickness_contains_actual_nested_annotations() {
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    let nested = Ruby::new(
        vec![RubyBase {
            node: NodeId(31),
            content: RubyContent::text(
                TextSource::Generated { node: NodeId(31) },
                "に",
                &style(12.0),
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(32),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(32) },
                    "ほ",
                    &style(6.0),
                    &Limits::default(),
                ),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    reading.push_ruby(NodeId(30), &style(12.0), nested);
    let p = inter_character_content(RubyContent::from_builder(reading));
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("nested upright reading")
    };
    assert_eq!(
        line.inline_size(),
        42.0,
        "24px base plus12px vertical reading and6px nested annotation"
    );
    let annotation = line.ruby_annotations().next().unwrap();
    assert_eq!(annotation.line().block_size(), 18.0);
    assert_eq!(annotation.line().ruby_annotations().count(), 1);
    assert_eq!(annotation.origin().0, 42.0);
}

#[test]
fn inter_character_thickness_tracks_actual_atomic_block_size_after_revision() {
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_atomic(NodeId(44), &style(12.0), Default::default());
    let p = inter_character_content(RubyContent::from_builder(reading));
    let mut cx = LayoutContext::new();
    let mut atomics = AtomicSizes::new();
    for (thickness, width) in [(30.0, 54.0), (42.0, 66.0)] {
        atomics.insert(
            NodeId(44),
            crate::AtomicSize {
                inline_size: 18.0,
                block_size: thickness,
                baseline: Some(thickness / 2.0),
                ..Default::default()
            },
        );
        let crate::LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &atomics,
        ) else {
            panic!("atomic upright reading")
        };
        assert_eq!(line.inline_size(), width);
        assert_eq!(
            line.ruby_annotations().next().unwrap().line().block_size(),
            thickness
        );
    }
}

#[test]
fn ruby_uses_all_available_leading_before_increasing_line_advance() {
    // The pinned hhea1160/-288 at20/10px gives fixed-point B28.96875/O14.484375.
    // H40 adds only3.453125px; H50 has sufficient combined leading even though
    // the annotation still extends above its own line's box.
    for (height, advance, annotation_top) in [
        (40.0, 43.453125, -5.515625),
        (50.0, 50.0, -3.96875),
        (60.0, 60.0, 1.03125),
    ] {
        let base_style = style(20.0);
        let container_style = InlineStyle {
            line_height: crate::style::LineHeight::Px(height),
            ..base_style.clone()
        };
        let pair = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(10) },
                    "日",
                    &base_style,
                    &Limits::default(),
                ),
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(20) },
                        "にほん",
                        &style(10.0),
                        &Limits::default(),
                    ),
                    span: RubySpan::Auto,
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
                root: container_style.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &container_style, pair);
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let crate::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("ruby with leading")
        };
        assert_eq!(
            line.block_size(),
            advance,
            "actual CJK font content with H{height}"
        );
        assert_eq!(
            line.ruby_annotations().next().unwrap().origin().1,
            annotation_top
        );
    }
}

#[test]
fn outer_base_alignment_moves_a_nested_ruby_as_one_group() {
    let nested = Ruby::new(
        vec![RubyBase {
            node: NodeId(31),
            content: base("日本"),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(32),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(32) },
                    "に",
                    &style(12.0),
                    &Limits::default(),
                ),
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
    let mut contents = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    contents.push_ruby(NodeId(30), &style(24.0), nested);
    let p = build(ruby(RubyContent::from_builder(contents), "にほんにほん"));
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("nested base")
    };
    assert_eq!(line.inline_size(), 72.0);
    assert_eq!(
        glyph_positions(&line),
        [12.0, 36.0],
        "outer spacing belongs around the nested ruby, preserving its inner glyph gap"
    );
    let inner = line
        .ruby_annotations()
        .find(|a| a.container() == NodeId(30))
        .unwrap();
    assert_eq!(
        inner.origin().0,
        12.0,
        "the nested wrapper and reading move with both base glyphs"
    );
    assert_eq!(inner.line().inline_size(), 48.0);
    assert_eq!(glyph_positions(inner.line()), [18.0]);
}

#[test]
fn rtl_inter_character_stays_on_the_physical_right_of_its_base() {
    let rtl = crate::geometry::Direction::Rtl;
    let base_style = InlineStyle {
        direction: rtl,
        ..style(24.0)
    };
    let reading_style = InlineStyle {
        direction: rtl,
        ..style(12.0)
    };
    let pair = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Generated { node: NodeId(10) },
                "日",
                &base_style,
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(20) },
                    "に",
                    &reading_style,
                    &Limits::default(),
                ),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                position: RubyPosition::InterCharacter,
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            direction: rtl,
            root: base_style.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    b.push_ruby(NodeId(8), &base_style, pair);
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let crate::LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("RTL upright reading")
    };
    assert_eq!(line.inline_size(), 36.0);
    assert_eq!(glyph_positions(&line), [12.0]);
    let reading = line.ruby_annotations().next().unwrap();
    assert_eq!(reading.origin(), (0.0, 0.0));
    let t = reading.transform();
    assert_eq!(
        (
            t.inline_inline,
            t.inline_block,
            t.block_inline,
            t.block_block
        ),
        (0.0, 1.0, 1.0, 0.0)
    );
    assert_eq!(
        reading.line().used_direction(),
        crate::geometry::Direction::Ltr
    );
}

#[test]
fn interlinear_span_counts_only_internal_inter_character_columns() {
    for (cross_span, parent_width, base_positions, cross_origin) in [
        (RubySpan::Columns(0..1), 72.0, vec![3.0, 45.0], 42.0),
        (RubySpan::All, 84.0, vec![6.0, 42.0], 84.0),
    ] {
        let pair = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(10),
                    content: base("日"),
                    align: RubyAlign::default(),
                },
                RubyBase {
                    node: NodeId(11),
                    content: base("本"),
                    align: RubyAlign::default(),
                },
            ],
            vec![
                RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(20),
                        content: RubyContent::text(
                            TextSource::Generated { node: NodeId(20) },
                            "に",
                            &style(12.0),
                            &Limits::default(),
                        ),
                        span: cross_span,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle {
                        position: RubyPosition::InterCharacter,
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                },
                RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(21),
                        content: RubyContent::text(
                            TextSource::Generated { node: NodeId(21) },
                            "にほんにほん",
                            &style(12.0),
                            &Limits::default(),
                        ),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle {
                        position: RubyPosition::Over,
                        overhang: RubyOverhang::None,
                        ..Default::default()
                    },
                },
            ],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), pair);
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let crate::LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("cross/interlinear span")
        };
        assert_eq!(line.inline_size(), parent_width);
        assert_eq!(glyph_positions(&line), base_positions);
        let cross = line.ruby_annotations().find(|a| a.level() == 0).unwrap();
        assert_eq!(cross.origin().0, cross_origin);
        let interlinear = line.ruby_annotations().find(|a| a.level() == 1).unwrap();
        assert_eq!(
            interlinear.line().inline_size(),
            72.0,
            "interlinear width includes the internal cross slot but excludes the outside slot"
        );
    }
}

#[test]
fn repeated_inter_character_fit_probes_reuse_actual_child_block_metrics() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let mut pair = ruby(base(&"日".repeat(count)), &"に".repeat(count * 2));
        pair.levels[0].style.position = RubyPosition::InterCharacter;
        let p = build(pair);
        let mut cx = LayoutContext::new();
        for end in 1..=p.data.units.len() {
            crate::ruby::measure::candidate(
                &p.data,
                0,
                end,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "actual range/metric/query visits after doubling real parent and child text: {visits:?}"
    );
}

#[test]
fn pretty_alternate_cuts_keep_the_next_ruby_base_open_on_its_line() {
    let bases = (0..5)
        .map(|i| RubyBase {
            node: NodeId(10 + i),
            content: base("日"),
            align: RubyAlign::default(),
        })
        .collect();
    let annotations = (0..5)
        .map(|i| RubyAnnotation {
            node: NodeId(20 + i),
            content: RubyContent::text(
                TextSource::Generated {
                    node: NodeId(20 + i),
                },
                "に",
                &style(12.0),
                &Limits::default(),
            ),
            span: RubySpan::Auto,
            visibility: RubyVisibility::Visible,
        })
        .collect();
    let p = build(
        Ruby::new(
            bases,
            vec![RubyLevel {
                annotations,
                style: RubyStyle {
                    overhang: RubyOverhang::None,
                    ..Default::default()
                },
            }],
        )
        .unwrap(),
    );
    let options = crate::style::LineOptions {
        text_wrap_style: crate::style::TextWrapStyle::Pretty,
        ..Default::default()
    };
    for width in [50.0, 60.0, 72.0, 90.0] {
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
        let mut token = p.start_token();
        let mut readings = Vec::new();
        let mut bases = 0;
        loop {
            let mut constraint = crate::LineConstraint::new(width);
            constraint.break_plan = Some(&plan);
            match p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY) {
                crate::LineResult::Line(line) => {
                    assert!(
                        line.inline_size() <= width,
                        "planned columns fit the width at {width}"
                    );
                    bases += glyph_positions(&line).len();
                    readings.extend(line.ruby_annotations().map(|a| a.node().unwrap()));
                    assert_ne!(line.break_token(), token);
                    token = line.break_token();
                }
                crate::LineResult::Done => break,
                _ => panic!("prepared planned columns"),
            }
        }
        assert_eq!(bases, 5);
        assert_eq!(
            readings,
            [NodeId(20), NodeId(21), NodeId(22), NodeId(23), NodeId(24)]
        );
    }
}

#[test]
fn repeated_auto_overhang_probes_share_actual_neighbor_geometry() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let fonts = fonts();
        let mut builder = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        let mut input = ruby(base(&"日".repeat(count)), &"にほん".repeat(count));
        input.levels[0].style.overhang = RubyOverhang::Auto;
        builder.push_ruby(NodeId(8), &style(24.0), input);
        builder.push_text(TextSource::Generated { node: NodeId(2) }, "日");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let mut cx = LayoutContext::new();
        for cut in &p.data.ruby.containers[0].cuts[1..] {
            let measure = crate::ruby::measure::candidate(
                &p.data,
                0,
                cut.unit,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert!(
                measure
                    .fragments
                    .iter()
                    .any(|f| f.lanes.iter().any(|l| l.overhang.0 > LayoutUnit::ZERO))
            );
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "repeated actual Auto allowances rescanned prefixes: {visits:?}"
    );
}

#[test]
fn overhang_index_does_not_saturate_from_an_unselected_future_atomic() {
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "日");
    let mut input = ruby(base("日"), "にほん");
    input.levels[0].style.overhang = RubyOverhang::Auto;
    builder.push_ruby(NodeId(8), &style(24.0), input);
    builder.push_atomic(NodeId(99), &style(24.0), Default::default());
    let p = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(99),
        crate::AtomicSize {
            inline_size: 1.0e9,
            block_size: 1.0e9,
            baseline: Some(-1.0e9),
            margins: crate::node::Sides {
                block_start: 1.0e9,
                block_end: 1.0e9,
                ..Default::default()
            },
        },
    );
    let mut sat = Saturation::default();
    let measure = crate::ruby::measure::candidate(
        &p.data,
        0,
        p.data.ruby.containers[0].units.end,
        &atomics,
        &mut LayoutContext::new(),
        &mut sat,
    );
    assert_eq!(measure.fragments[0].lanes[0].overhang.0.to_f32(), 6.0);
    assert_eq!(measure.adjustment.to_f32(), 6.0);
    assert!(
        sat.is_clean(),
        "unselected atomic contaminated the selected fragment: {sat:?}"
    );
}

#[test]
fn actual_annotation_block_profiles_do_not_rescan_long_prefixes() {
    let mut visits = Vec::new();
    for count in [64, 128] {
        let p = build(ruby(base(&"日".repeat(count)), &"にほん".repeat(count)));
        let container = &p.data.ruby.containers[0];
        let child = &container.lanes[0].paragraph.data;
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
        "actual child block profiles rescanned prefixes: {visits:?}"
    );
}

#[test]
fn indexed_annotation_heights_match_retained_lines_for_clipped_alignment_groups() {
    use crate::style::{LineHeight, VerticalAlign};
    for mode in [
        crate::geometry::WritingMode::HorizontalTb,
        crate::geometry::WritingMode::VerticalRl,
        crate::geometry::WritingMode::VerticalLr,
    ] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                writing_mode: mode,
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        for (i, alignment) in [VerticalAlign::Top, VerticalAlign::Bottom]
            .into_iter()
            .enumerate()
        {
            b.open_inline(
                NodeId(100 + i as u64),
                &InlineStyle {
                    vertical_align: alignment,
                    ..style(48.0)
                },
                Default::default(),
            );
            b.open_inline(
                NodeId(200 + i as u64),
                &InlineStyle {
                    vertical_align: VerticalAlign::Length(-3.0),
                    ..style(8.0)
                },
                Default::default(),
            );
            b.push_text(
                TextSource::Generated {
                    node: NodeId(2 + i as u64),
                },
                "日本",
            );
            b.close_inline();
            b.close_inline();
        }
        b.push_atomic(
            NodeId(99),
            &InlineStyle {
                vertical_align: VerticalAlign::Length(5.0),
                ..style(24.0)
            },
            Default::default(),
        );
        b.open_inline(
            NodeId(300),
            &InlineStyle {
                line_height: LineHeight::Px(80.0),
                vertical_align: VerticalAlign::Length(-3.0),
                ..style(24.0)
            },
            Default::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(5) }, "語");
        b.close_inline();
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let mut atomics = AtomicSizes::new();
        atomics.insert(
            NodeId(99),
            crate::AtomicSize {
                inline_size: 34.0,
                block_size: 27.0,
                baseline: Some(9.0),
                ..Default::default()
            },
        );
        let owners: Vec<_> = p
            .data
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                matches!(
                    u.kind,
                    crate::analysis::units::UnitKind::Cluster { .. }
                        | crate::analysis::units::UnitKind::Atomic { .. }
                )
            })
            .map(|(i, _)| i)
            .collect();
        let mut cx = LayoutContext::new();
        cx.ruby_ranges.begin(&p.data, &atomics);
        for (at, start) in owners.iter().enumerate() {
            for end in owners[at..].iter().map(|i| i + 1) {
                let range = *start..end;
                let indexed = crate::line::range::block_size(
                    &p.data,
                    range.clone(),
                    &Default::default(),
                    &atomics,
                    &mut cx,
                    &mut Saturation::default(),
                );
                let actual = p.ruby_line(
                    &mut LayoutContext::new(),
                    range.clone(),
                    1000.0,
                    &atomics,
                    crate::ruby::align::AnnotationAlign::Policy(RubyAlign::Start),
                );
                assert_eq!(indexed.to_f32(), actual.block_size(), "{mode:?}/{range:?}");
            }
        }
    }
}

#[test]
fn nested_annotation_profiles_keep_actual_metrics_and_bounded_probes() {
    let mut visits = Vec::new();
    for count in [1024, 2048] {
        let mut reading = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        reading.push_ruby(
            NodeId(81),
            &style(24.0),
            ruby(base(&"日".repeat(count)), &"にほん".repeat(count)),
        );
        let outer = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: base(&"日本語".repeat(count)),
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
        let p = build(outer);
        let container = &p.data.ruby.containers[0];
        assert!(
            container.cuts.len() >= count / 2,
            "nested fixture needs many actual cuts: {}",
            container.cuts.len()
        );
        let reading = &container.lanes[0].paragraph;
        let full = reading.ruby_line(
            &mut LayoutContext::new(),
            0..reading.data.units.len(),
            100000.0,
            &AtomicSizes::EMPTY,
            crate::ruby::align::AnnotationAlign::Policy(RubyAlign::Start),
        );
        assert_eq!(full.block_size(), 52.140625);
        let mut cx = LayoutContext::new();
        for cut in &container.cuts[1..] {
            let measure = crate::ruby::measure::candidate(
                &p.data,
                0,
                cut.unit,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert_eq!(measure.fragments[0].lanes[0].block_size.to_f32(), 52.140625);
        }
        visits.push(cx.ruby_measure_visits);
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "nested actual row profiles rescanned prefixes: {visits:?}"
    );
}

#[test]
fn top_bottom_nested_annotation_probes_do_not_rescan_prefixes() {
    use crate::style::VerticalAlign;
    for alignment in [VerticalAlign::Top, VerticalAlign::Bottom] {
        let mut visits = Vec::new();
        for count in [1024, 2048] {
            let mut reading = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style(24.0),
                    ..Default::default()
                },
                &Limits::default(),
            );
            reading.open_inline(
                NodeId(80),
                &InlineStyle {
                    vertical_align: alignment,
                    ..style(24.0)
                },
                Default::default(),
            );
            reading.push_ruby(
                NodeId(81),
                &style(24.0),
                ruby(base(&"日".repeat(count)), &"にほん".repeat(count)),
            );
            reading.close_inline();
            let outer = Ruby::new(
                vec![RubyBase {
                    node: NodeId(10),
                    content: base(&"日本語".repeat(count)),
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
            let p = build(outer);
            let container = &p.data.ruby.containers[0];
            assert!(container.cuts.len() >= count / 2);
            let reading = &container.lanes[0].paragraph;
            let full = reading.ruby_line(
                &mut LayoutContext::new(),
                0..reading.data.units.len(),
                100000.0,
                &AtomicSizes::EMPTY,
                crate::ruby::align::AnnotationAlign::Policy(RubyAlign::Start),
            );
            assert_eq!(full.block_size(), 52.140625);
            let mut cx = LayoutContext::new();
            for cut in &container.cuts[1..] {
                let measure = crate::ruby::measure::candidate(
                    &p.data,
                    0,
                    cut.unit,
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut Saturation::default(),
                );
                assert_eq!(
                    measure.fragments[0].lanes[0].block_size.to_f32(),
                    52.140625,
                    "{alignment:?}"
                );
            }
            visits.push(cx.ruby_measure_visits);
        }
        assert!(
            visits[1] <= visits[0] * 3,
            "{alignment:?} nested prefix rescans: {visits:?}"
        );
    }
}

#[test]
fn many_structural_columns_and_containers_measure_actual_output_with_bounded_work() {
    for grouped in [true, false] {
        let mut visits = Vec::new();
        for count in if grouped { [256, 512] } else { [512, 1024] } {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style(24.0),
                    ..Default::default()
                },
                &Limits::default(),
            );
            if grouped {
                let bases = (0..count)
                    .map(|i| RubyBase {
                        node: NodeId(1000 + i as u64),
                        content: base("日"),
                        align: RubyAlign::Start,
                    })
                    .collect();
                let annotations = (0..count)
                    .map(|i| RubyAnnotation {
                        node: NodeId(3000 + i as u64),
                        content: RubyContent::text(
                            TextSource::Generated {
                                node: NodeId(3000 + i as u64),
                            },
                            "にほん",
                            &style(12.0),
                            &Limits::default(),
                        ),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    })
                    .collect();
                b.push_ruby(
                    NodeId(8),
                    &style(24.0),
                    Ruby::new(
                        bases,
                        vec![RubyLevel {
                            annotations,
                            style: RubyStyle {
                                overhang: RubyOverhang::None,
                                ..Default::default()
                            },
                        }],
                    )
                    .unwrap(),
                );
            } else {
                for i in 0..count {
                    b.push_ruby(
                        NodeId(1000 + i as u64),
                        &style(24.0),
                        ruby(base("日"), "にほん"),
                    );
                }
            }
            let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
            let mut cx = LayoutContext::new();
            let measured = crate::ruby::measure::candidate(
                &p.data,
                0,
                p.data.units.len(),
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default(),
            );
            assert_eq!(measured.adjustment.to_f32(), count as f32 * 12.0);
            assert_eq!(measured.fragments.len(), if grouped { 1 } else { count });
            assert_eq!(
                measured
                    .fragments
                    .iter()
                    .map(|f| f.bases.len())
                    .sum::<usize>(),
                count
            );
            visits.push(cx.ruby_measure_visits);
        }
        assert!(
            visits[1] <= visits[0] * 3,
            "grouped={grouped}: materializing twice the real structural output did superlinear work {visits:?}"
        );
    }
}

#[test]
fn sequential_lines_do_not_rescan_every_independent_ruby_container() {
    // Whole-paragraph measurement cannot detect a full scan on every short line.
    let mut visits = Vec::new();
    for count in [512, 1024] {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        for i in 0..count {
            b.push_ruby(
                NodeId(1000 + i as u64),
                &style(24.0),
                ruby(base("日"), "にほん"),
            );
        }
        crate::ruby::index::take_visits();
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let mut cx = LayoutContext::new();
        let lines = p.break_all(&mut cx, &Default::default(), 360.0, &AtomicSizes::EMPTY);
        let mut base_glyphs = 0;
        let mut reading_glyphs = 0;
        for line in &lines {
            for fragment in line.fragments() {
                if let crate::Fragment::GlyphRun(run) = fragment {
                    base_glyphs += run.glyphs().len();
                    assert!(run.glyphs().all(|g| g.id != 0));
                }
            }
            for annotation in line.ruby_annotations() {
                for fragment in annotation.line().fragments() {
                    if let crate::Fragment::GlyphRun(run) = fragment {
                        reading_glyphs += run.glyphs().len();
                        assert!(run.glyphs().all(|g| g.id != 0));
                    }
                }
            }
        }
        assert_eq!((base_glyphs, reading_glyphs), (count, count * 3));
        assert!(
            lines.len() >= count / 12,
            "fixed width produces real continuations"
        );
        assert!(cx.take_warnings().is_empty());
        visits.push(cx.ruby_measure_visits + crate::ruby::index::take_visits());
    }
    assert!(
        visits[1] <= visits[0] * 3,
        "twice the independent ruby input must not cause quadratic sequential candidate work: {visits:?}"
    );
}

#[test]
fn clipped_nested_bases_keep_every_completed_fragment_at_the_same_source_start() {
    let mut inner = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    inner.push_ruby(
        NodeId(81),
        &style(24.0),
        ruby(base("日日日日日日"), &"に".repeat(18)),
    );
    let mut middle = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    middle.push_ruby(
        NodeId(82),
        &style(24.0),
        ruby(RubyContent::from_builder(inner), &"ほ".repeat(30)),
    );
    let p = build(ruby(RubyContent::from_builder(middle), &"ん".repeat(36)));
    let outer = &p.data.ruby.containers[0];
    let cut = outer
        .cuts
        .iter()
        .find(|cut| {
            p.data.units[cut.unit..outer.units.end]
                .iter()
                .filter(|u| matches!(u.kind, crate::analysis::units::UnitKind::Cluster { .. }))
                .count()
                == 3
        })
        .expect("real halfway coordinated cut");
    let mut cx = LayoutContext::new();
    let m = crate::ruby::measure::candidate(
        &p.data,
        cut.unit,
        outer.units.end,
        &AtomicSizes::EMPTY,
        &mut cx,
        &mut Saturation::default(),
    );
    assert_eq!(m.fragments.len(), 3);
    assert!(
        m.fragments.iter().all(|f| f.units.start == cut.unit),
        "all three selected ancestors share the clipped cursor"
    );
    // Remaining base3*24=72; inner9*12=108; middle15*12=180;
    // outer18*12=216. Each ancestor adds only its own additional width.
    assert_eq!(m.adjustment.to_f32(), 144.0);
}

#[test]
fn accepted_ruby_continuations_reuse_root_and_child_indexes_without_prefix_rescans() {
    for nested in [false, true] {
        let mut visits = Vec::new();
        for count in [64, 128] {
            let pair = if nested {
                let mut child = ParagraphBuilder::new(
                    &ParagraphStyle {
                        root: style(24.0),
                        ..Default::default()
                    },
                    &Limits::default(),
                );
                child.push_ruby(
                    NodeId(80),
                    &style(24.0),
                    ruby(base(&"本".repeat(count)), &"にほん".repeat(count)),
                );
                let mut pair = ruby(base(&"日".repeat(count)), "");
                pair.levels[0].annotations[0].content = RubyContent::from_builder(child);
                pair
            } else {
                ruby(base(&"日".repeat(count)), &"にほん".repeat(count))
            };
            let p = build(pair);
            let mut cx = LayoutContext::new();
            let lines = p.break_all(&mut cx, &Default::default(), 36.0, &AtomicSizes::EMPTY);
            assert_eq!(
                lines.len(),
                count,
                "nested={nested}: one real paired unit per accepted line"
            );
            assert!(lines.iter().all(|line| line.inline_size() == 36.0));
            let mut glyphs = 0;
            for a in lines.iter().flat_map(|line| line.ruby_annotations()) {
                let child = if nested {
                    a.line().ruby_annotations().next().unwrap().line()
                } else {
                    a.line()
                };
                glyphs += child
                    .fragments()
                    .filter_map(|f| {
                        if let crate::Fragment::GlyphRun(r) = f {
                            Some(r.glyphs().len())
                        } else {
                            None
                        }
                    })
                    .sum::<usize>();
            }
            assert_eq!(glyphs, count * 3);
            visits.push(cx.ruby_measure_visits);
        }
        assert!(
            visits[1] <= visits[0] * 3,
            "nested={nested}: actual accepted parent/child continuation rebuilt complete indexes {visits:?}"
        );
    }
}

fn many_column_pair(count: usize) -> Ruby {
    let bases = (0..count)
        .map(|i| RubyBase {
            node: NodeId(1000 + i as u64),
            content: base("日"),
            align: RubyAlign::Start,
        })
        .collect();
    let levels = [1, 2, 4]
        .into_iter()
        .enumerate()
        .map(|(level, step)| RubyLevel {
            // Input order is deliberately reversed; preparation normalizes spans.
            annotations: (0..count)
                .step_by(step)
                .rev()
                .map(|i| RubyAnnotation {
                    node: NodeId(2000 + level as u64 * 1000 + i as u64),
                    content: RubyContent::text(
                        TextSource::Generated {
                            node: NodeId(2000 + level as u64 * 1000 + i as u64),
                        },
                        "に",
                        &style(12.0),
                        &Limits::default(),
                    ),
                    span: RubySpan::Columns(i..i + step),
                    visibility: RubyVisibility::Visible,
                })
                .collect(),
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        })
        .collect();
    Ruby::new(bases, levels).unwrap()
}

#[test]
fn short_candidate_visits_only_intersecting_annotation_lanes() {
    for count in [64, 128] {
        let p = build(many_column_pair(count));
        let ruby = &p.data.ruby.containers[0];
        let first = count / 2;
        let mut cx = LayoutContext::new();
        let measure = crate::ruby::measure::candidate(
            &p.data,
            ruby.columns[first].units.start,
            ruby.columns[first + 3].units.end,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        );
        let fragment = &measure.fragments[0];
        assert_eq!(fragment.lanes.len(), 7);
        let nodes: Vec<_> = fragment
            .lanes
            .iter()
            .map(|l| ruby.lanes[l.lane].node.unwrap())
            .collect();
        let expected: Vec<_> = [1, 2, 4]
            .into_iter()
            .enumerate()
            .flat_map(|(level, step)| {
                (first..first + 4)
                    .step_by(step)
                    .map(move |i| NodeId(2000 + level as u64 * 1000 + i as u64))
            })
            .collect();
        assert_eq!(
            nodes, expected,
            "paired cut lookup must retain original global lane IDs"
        );
        assert_eq!(
            cx.ruby_lane_visits, 7,
            "out-of-window annotations must not be visited"
        );
    }
}

#[test]
fn short_candidate_measures_only_selected_base_columns() {
    for count in [64, 128] {
        let p = build(many_column_pair(count));
        let ruby = &p.data.ruby.containers[0];
        let first = count / 2;
        let mut cx = LayoutContext::new();
        let measure = crate::ruby::measure::candidate(
            &p.data,
            ruby.columns[first].units.start,
            ruby.columns[first + 3].units.end,
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        );
        assert_eq!(
            cx.ruby_column_visits, 4,
            "unselected columns must not be measured"
        );
        let fragment = &measure.fragments[0];
        assert_eq!(
            fragment.bases.len(),
            4,
            "candidate arrays must retain only the selected window"
        );
        assert_eq!(
            fragment.bases,
            ruby.columns[first..first + 4]
                .iter()
                .map(|c| c.units.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            fragment
                .columns
                .iter()
                .map(|w| w.to_f32())
                .collect::<Vec<_>>(),
            [24.0; 4]
        );
    }
}

#[test]
fn boundary_only_candidate_keeps_empty_ruby_fallback_geometry() {
    let p = build(many_column_pair(64));
    let ruby = &p.data.ruby.containers[0];
    let measure = crate::ruby::measure::candidate(
        &p.data,
        ruby.units.end - 1,
        ruby.units.end,
        &AtomicSizes::EMPTY,
        &mut LayoutContext::new(),
        &mut Saturation::default(),
    );
    assert_eq!(measure.fragments.len(), 1);
    let fragment = &measure.fragments[0];
    assert!(fragment.bases.iter().all(|b| b.is_empty()));
    assert!(fragment.lanes.is_empty());
    assert_eq!(fragment.adjustment, LayoutUnit::ZERO);
    assert!(!fragment.has_content);
    assert_eq!(fragment.contribution.top, LayoutUnit::ZERO);
    assert_eq!(fragment.contribution.bottom, LayoutUnit::ZERO);
}

#[test]
fn continued_columns_keep_their_own_alignment_nodes_and_source_ranges() {
    let pair = Ruby::new(
        [RubyAlign::Start, RubyAlign::Center, RubyAlign::SpaceAround]
            .into_iter()
            .enumerate()
            .map(|(i, align)| {
                let node = NodeId(100 + i as u64);
                RubyBase {
                    node,
                    align,
                    content: RubyContent::text(
                        TextSource::Dom {
                            node,
                            offset: 40 + i as u32 * 10,
                        },
                        "日",
                        &style(24.0),
                        &Limits::default(),
                    ),
                }
            })
            .collect(),
        vec![RubyLevel {
            annotations: (0..3)
                .map(|i| {
                    let node = NodeId(200 + i);
                    RubyAnnotation {
                        node,
                        content: RubyContent::text(
                            TextSource::Dom { node, offset: 70 },
                            "にほん",
                            &style(12.0),
                            &Limits::default(),
                        ),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    }
                })
                .collect(),
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    let p = build(pair);
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        36.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 3);
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(glyph_positions(line), [if i == 0 { 0.0 } else { 6.0 }]);
        let annotation = line.ruby_annotations().next().unwrap();
        assert_eq!(annotation.base_nodes(), &[NodeId(100 + i as u64)]);
        assert_eq!(annotation.node(), Some(NodeId(200 + i as u64)));
        let text = &p.data.ruby.containers[0].columns[i].text;
        assert_eq!(
            annotation.base_text_range(),
            text.start as usize..text.end as usize
        );
        let node = line.fragments().find_map(|f| match f {
            crate::Fragment::GlyphRun(r) => r.node(),
            _ => None,
        });
        assert_eq!(node, Some(NodeId(100 + i as u64)));
        // Column boundaries can carry generated anchors at the same offset.
        // Check the DOM-owned glyph range directly rather than assigning that
        // ambiguous generated boundary a DOM affinity.
        let glyph_range = line
            .fragments()
            .find_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r.text_range()),
                _ => None,
            })
            .unwrap();
        let mapping = p
            .offset_mapping()
            .unwrap()
            .units()
            .iter()
            .find(|u| u.node == NodeId(100 + i as u64))
            .unwrap();
        assert_eq!(mapping.dom, 40 + i as u32 * 10..43 + i as u32 * 10);
        assert_eq!(
            mapping.text,
            glyph_range.start as u32..glyph_range.end as u32
        );
    }
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
