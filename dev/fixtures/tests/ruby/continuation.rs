//! Public accepted output, immutable continuation and context-retry contracts.
use super::*;
use shodo::node::OutOfFlowKind;
use shodo::style::{LineBreak, TextTransform};
use shodo::{Fragment, Line};

fn visible(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(*c, '\u{2066}'..='\u{2069}' | '\u{202a}'..='\u{202e}'))
        .collect()
}

fn line_text(line: &Line) -> String {
    visible(&line.text()[line.text_range()])
}

fn pair(base: &str, reading: &str, size: f32, node: u64) -> Ruby {
    let base_style = InlineStyle {
        line_break: LineBreak::Anywhere,
        ..style(24.0)
    };
    let reading_style = InlineStyle {
        line_break: LineBreak::Anywhere,
        ..style(size)
    };
    Ruby::new(
        vec![RubyBase {
            node: NodeId(node),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(node),
                    offset: 40,
                },
                base,
                &base_style,
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(node + 1),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(node + 1),
                        offset: 70,
                    },
                    reading,
                    &reading_style,
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

fn build(pair: Ruby) -> Paragraph {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style(24.0), pair);
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
}

fn reading_text(line: &Line, level: usize) -> String {
    line.ruby_annotations()
        .filter(|a| a.level() == level)
        .map(|a| visible(&a.paragraph().text()[a.text_range()]))
        .collect()
}

fn reading_glyphs(line: &Line, level: usize) -> Vec<u32> {
    line.ruby_annotations()
        .filter(|a| a.level() == level)
        .flat_map(|a| {
            a.line()
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .flat_map(|r| r.glyphs().map(|g| g.id))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn signature(
    line: &Line,
) -> (
    shodo::BreakToken,
    String,
    Vec<(usize, String, Vec<u32>)>,
    f32,
) {
    let mut readings = Vec::new();
    for a in line.ruby_annotations() {
        let ids = a
            .line()
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect();
        readings.push((
            a.level(),
            visible(&a.paragraph().text()[a.text_range()]),
            ids,
        ));
    }
    (
        line.break_token(),
        line_text(line),
        readings,
        line.inline_size(),
    )
}

#[test]
fn short_pair_retains_actual_annotation_output_and_font_owners() {
    let p = super::short_pair();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        12.0,
        &AtomicSizes::EMPTY,
    );
    let line = &lines[0];
    assert_eq!(line_text(line), "日");
    assert_eq!(reading_text(line, 0), "にほん");
    assert_eq!(reading_glyphs(line, 0), [208, 224, 248]);
    assert_eq!(
        line.fragments()
            .filter(|f| matches!(f, Fragment::RubyAnnotation(_)))
            .count(),
        1
    );
    let a = line.ruby_annotations().next().unwrap();
    assert_eq!(a.container(), NodeId(8));
    assert_eq!(a.base_nodes(), [NodeId(10)]);
    assert_eq!(a.node(), Some(NodeId(20)));
    assert_eq!(visible(&line.text()[a.base_text_range()]), "日");
    assert_eq!(a.text_range(), a.line().text_range());
    assert_eq!(a.visibility(), RubyVisibility::Visible);
    let t = a.transform();
    assert_eq!(a.origin(), (t.inline_offset, t.block_offset));
    assert_eq!(
        (
            t.inline_inline,
            t.inline_block,
            t.block_inline,
            t.block_block
        ),
        (1.0, 0.0, 0.0, 1.0)
    );
    let cjk = shodo_fixtures::font("cjk").unwrap().bytes;
    for f in a.line().fragments() {
        if let Fragment::GlyphRun(run) = f {
            assert_eq!(run.font_size(), 12.0);
            assert_eq!(run.font_data().unwrap().data.as_ref(), cjk);
        }
    }
}

#[test]
fn long_pair_wraps_together() {
    let p = build(pair("日本語日本語", "にほんごにほんご", 12.0, 10));
    for width in [1000.0, 48.0, 0.0] {
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        assert!(
            !lines.is_empty() && lines.len() <= 6,
            "bounded progress at width{width}"
        );
        assert_eq!(
            lines.iter().map(line_text).collect::<String>(),
            "日本語日本語"
        );
        assert_eq!(
            lines.iter().map(|l| reading_text(l, 0)).collect::<String>(),
            "にほんごにほんご"
        );
        assert_eq!(
            lines.iter().flat_map(|l| reading_glyphs(l, 0)).count(),
            8,
            "each reading glyph emitted once"
        );
        if width == 48.0 {
            assert_eq!(lines.len(), 3);
            assert!(lines.iter().all(|l| l.inline_size() <= 48.0));
        }
        for line in &lines {
            assert!(!line_text(line).is_empty());
            assert!(!reading_text(line, 0).is_empty());
        }
    }
}

#[test]
fn multi_level_continuation_retry() {
    let base = RubyBase {
        node: NodeId(10),
        content: RubyContent::text(
            TextSource::Dom {
                node: NodeId(10),
                offset: 40,
            },
            "日本語日本語",
            &InlineStyle {
                line_break: LineBreak::Anywhere,
                ..style(24.0)
            },
            &Limits::default(),
        ),
        align: RubyAlign::default(),
    };
    let levels = ["にほんごにほんご", "かなかな"]
        .iter()
        .enumerate()
        .map(|(i, text)| RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20 + i as u64),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20 + i as u64),
                        offset: 70,
                    },
                    text,
                    &InlineStyle {
                        line_break: LineBreak::Anywhere,
                        ..style(12.0)
                    },
                    &Limits::default(),
                ),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle {
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        })
        .collect();
    let p = build(Ruby::new(vec![base], levels).unwrap());
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut bases = String::new();
    let mut readings = [String::new(), String::new()];
    for _ in 0..8 {
        let mut too_short = LineConstraint::new(48.0);
        too_short.max_block_size = Some(0.0);
        assert!(matches!(
            p.next_line(
                &mut cx,
                token,
                &LineOptions::default(),
                &too_short,
                &AtomicSizes::EMPTY
            ),
            LineResult::BlockSizeExceeded { .. }
        ));
        // A speculative wide retry must not consume any lane state.
        let _ = p.next_line(
            &mut cx,
            token,
            &LineOptions::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        );
        let LineResult::Line(warm) = p.next_line(
            &mut cx,
            token,
            &LineOptions::default(),
            &LineConstraint::new(48.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("warm continuation");
        };
        let LineResult::Line(cold) = p.next_line(
            &mut LayoutContext::new(),
            token,
            &LineOptions::default(),
            &LineConstraint::new(48.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("cold continuation");
        };
        assert_eq!(signature(&warm), signature(&cold));
        bases.push_str(&line_text(&warm));
        for (level, reading) in readings.iter_mut().enumerate() {
            reading.push_str(&reading_text(&warm, level));
        }
        assert_ne!(warm.break_token(), token);
        token = warm.break_token();
        if warm.is_last() {
            break;
        }
    }
    assert_eq!(bases, "日本語日本語");
    assert_eq!(readings, ["にほんごにほんご", "かなかな"]);
    assert!(matches!(
        p.next_line(
            &mut cx,
            token,
            &LineOptions::default(),
            &LineConstraint::new(48.0),
            &AtomicSizes::EMPTY
        ),
        LineResult::Done
    ));
}

#[test]
fn first_line_ruby_source_cuts() {
    let latin = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        line_break: LineBreak::Anywhere,
        ..style(24.0)
    };
    let mut base = ParagraphBuilder::new(
        &ParagraphStyle {
            root: latin.clone(),
            first_line: Some(InlineStyle {
                text_transform: TextTransform::Uppercase,
                ..latin
            }),
            ..Default::default()
        },
        &Limits::default(),
    );
    base.push_text(
        TextSource::Dom {
            node: NodeId(10),
            offset: 40,
        },
        "ßa",
    );
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            first_line: Some(style(24.0)),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 70,
        },
        "にほん",
    );
    let p = build(
        Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::from_builder(base),
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
        .unwrap(),
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        50.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "SS");
    assert_eq!(line_text(&lines[1]), "a");
    assert_eq!(
        lines.iter().map(|l| reading_text(l, 0)).collect::<String>(),
        "にほん"
    );
    for (index, line) in lines.iter().enumerate() {
        for a in line.ruby_annotations() {
            for f in a.line().fragments() {
                if let Fragment::GlyphRun(run) = f {
                    assert_eq!(run.font_size(), if index == 0 { 24.0 } else { 12.0 });
                }
            }
        }
    }
    assert!(
        lines[0]
            .offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(10), 42)
            .is_some()
    );
    assert!(
        lines[1]
            .offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(10), 42)
            .is_some()
    );
}

#[test]
fn float_retry_keeps_ruby_lanes() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        super::ruby(RubyVisibility::Visible),
    )
    .push_out_of_flow(NodeId(50), OutOfFlowKind::Float)
    .push_ruby(
        NodeId(60),
        &style(24.0),
        pair("日本語日本語", "にほんごにほんご", 12.0, 61),
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut cx = LayoutContext::new();
    let LineResult::FloatEncountered {
        float_cursor,
        line_start,
        inline_position,
        ..
    } = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("float encountered after actual ruby");
    };
    assert_eq!(
        inline_position, 36.0,
        "float placement sees the annotation's width"
    );
    assert_eq!(line_start, p.start_token());
    let mut constraint = LineConstraint::new(48.0);
    constraint.floats_placed_through = Some(float_cursor);
    constraint.max_block_size = Some(0.0);
    assert!(matches!(
        p.next_line(
            &mut cx,
            line_start,
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY
        ),
        LineResult::BlockSizeExceeded { .. }
    ));
    constraint.max_block_size = None;
    let LineResult::Line(warm) = p.next_line(
        &mut cx,
        line_start,
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("placed float retry");
    };
    let LineResult::Line(cold) = p.next_line(
        &mut LayoutContext::new(),
        line_start,
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("fresh placed float retry");
    };
    assert_eq!(signature(&warm), signature(&cold));
    constraint.floats_placed_through = None;
    // At48px the next paired base cannot fit, so the accepted cut precedes
    // this anchor. Withdrawing it does not make it part of the earlier line.
    let LineResult::Line(narrow_withdrawn) = p.next_line(
        &mut cx,
        line_start,
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("anchor remains beyond the selected narrow cut")
    };
    assert_eq!(signature(&warm), signature(&narrow_withdrawn));
    constraint.available_inline_size = 1000.0;
    assert!(matches!(
        p.next_line(
            &mut cx,
            line_start,
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY
        ),
        LineResult::FloatEncountered { .. }
    ));
    constraint.available_inline_size = 48.0;
    constraint.floats_placed_through = Some(float_cursor);
    let LineResult::Line(reintroduced) = p.next_line(
        &mut cx,
        line_start,
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("reintroduced float");
    };
    assert_eq!(signature(&warm), signature(&reintroduced));
}

#[test]
fn planned_long_pair_uses_the_same_accepted_measurement() {
    let p = build(pair("日本語日本語", "にほんごにほんご", 12.0, 10));
    let greedy = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        48.0,
        &AtomicSizes::EMPTY,
    );
    for wrap in [TextWrapStyle::Balance, TextWrapStyle::Pretty] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            48.0,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = LineConstraint::new(48.0);
        constraint.break_plan = Some(&plan);
        let mut token = p.start_token();
        let mut actual = Vec::new();
        for _ in 0..8 {
            let LineResult::Line(line) = p.next_line(
                &mut LayoutContext::new(),
                token,
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                break;
            };
            token = line.break_token();
            let last = line.is_last();
            actual.push(line);
            if last {
                break;
            }
        }
        assert_eq!(actual.len(), greedy.len());
        assert_eq!(
            actual.iter().map(line_text).collect::<String>(),
            "日本語日本語"
        );
        assert_eq!(
            actual
                .iter()
                .map(|l| reading_text(l, 0))
                .collect::<String>(),
            "にほんごにほんご"
        );
        assert!(actual.iter().all(|line| line.inline_size() <= 48.0
            && !line.ruby_annotations().collect::<Vec<_>>().is_empty()));
    }
}

#[test]
fn tall_first_line_annotation_retry_keeps_the_original_source_cuts() {
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            first_line: Some(style(96.0)),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 70,
        },
        "にほんごにほんご",
    );
    let p = build(
        Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(10),
                        offset: 40,
                    },
                    "日本語日本語",
                    &InlineStyle {
                        line_break: LineBreak::Anywhere,
                        ..style(24.0)
                    },
                    &Limits::default(),
                ),
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
        .unwrap(),
    );
    let mut cx = LayoutContext::new();
    let mut short = LineConstraint::new(1000.0);
    short.max_block_size = Some(40.0);
    let LineResult::BlockSizeExceeded { needed_block_size } = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &short,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("96px first-line reading must contribute to the line height");
    };
    assert!(needed_block_size > 40.0);
    let LineResult::Line(warm) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(48.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("retry at narrower width");
    };
    let LineResult::Line(cold) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &LineOptions::default(),
        &LineConstraint::new(48.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("same narrow first line in fresh context");
    };
    assert_eq!(signature(&warm), signature(&cold));
    let lines = p.break_all(&mut cx, &LineOptions::default(), 48.0, &AtomicSizes::EMPTY);
    assert_eq!(
        lines.iter().map(line_text).collect::<String>(),
        "日本語日本語"
    );
    assert_eq!(
        lines.iter().map(|l| reading_text(l, 0)).collect::<String>(),
        "にほんごにほんご"
    );
}
