use shodo::limits::{LimitKind, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, FontFeature, InlineStyle, OverflowWrap, ParagraphStyle, TextTransform,
};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder,
};
use shodo_fixtures::{FONTS, load_fonts};

fn glyphs(line: &Line) -> Vec<(u32, f32)> {
    line.fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs().map(move |g| (g.id, r.font_size())))
        .collect()
}

#[test]
fn first_line_features_transform_and_fonts() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (transform, family, features) in [
        (
            TextTransform::Uppercase,
            0,
            vec![FontFeature {
                tag: *b"kern",
                value: 0,
            }],
        ),
        (
            TextTransform::None,
            0,
            vec![FontFeature {
                tag: *b"liga",
                value: 0,
            }],
        ),
        (TextTransform::None, 1, Vec::new()),
    ] {
        let first = InlineStyle {
            font_size: 32.0,
            text_transform: transform,
            font_families: vec![FontFamily::Named(FONTS[family].family.into())],
            font_features: features,
            ..Default::default()
        };
        let style = ParagraphStyle {
            first_line: Some(first.clone()),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi AV 水");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let actual = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            10000.0,
            &AtomicSizes::EMPTY,
        );
        let mut reference = ParagraphBuilder::new(
            &ParagraphStyle {
                root: first,
                ..Default::default()
            },
            &Limits::default(),
        );
        reference.push_text(TextSource::Generated { node: NodeId(1) }, "ffi AV 水");
        let expected = reference
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                10000.0,
                &AtomicSizes::EMPTY,
            );
        assert_eq!(actual.len(), 1);
        assert_eq!(
            glyphs(&actual[0]),
            glyphs(&expected[0]),
            "transform {transform:?}, face {family}"
        );
        assert_eq!(actual[0].inline_size(), expected[0].inline_size());
        assert_eq!(p.text(), "ffi AV 水", "normal set stays unchanged");
    }
}

#[test]
fn first_line_then_normal_token_correspondence() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for mapping in [false, true] {
        for dom in [false, true] {
            let mut style = ParagraphStyle::default();
            style.root.text_transform = TextTransform::FullWidth;
            style.root.overflow_wrap = OverflowWrap::Anywhere;
            let mut first = style.root.clone();
            first.text_transform = TextTransform::Uppercase;
            style.first_line = Some(first);
            let mut b = ParagraphBuilder::new(&style, &Limits::default());
            b.with_offset_mapping(mapping).push_text(
                if dom {
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 100,
                    }
                } else {
                    TextSource::Generated { node: NodeId(1) }
                },
                "ab cd",
            );
            let p = b
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap();
            assert_eq!(p.text(), "ａｂ　ｃｄ");
            let mut cx = LayoutContext::new();
            let constraint = LineConstraint::new(12.0);
            let LineResult::Line(line) = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("first line")
            };
            assert_eq!(line.text_range(), 0..1);
            let LineResult::Line(next) = p.next_line(
                &mut cx,
                line.break_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("normal continuation")
            };
            assert_eq!(next.text_range().start, 3, "mapping={mapping}, DOM={dom}");
            assert_eq!(next.text_range().end, 6);
            let LineResult::Line(retry) = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("retry")
            };
            assert_eq!(glyphs(&retry), glyphs(&line));
            assert_eq!(retry.break_token(), line.break_token());
        }
    }
}

#[test]
fn block_in_inline_disables_first_line() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_size: 32.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    for leading_block in [false, true] {
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        if !leading_block {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        }
        b.push_block_in_inline(NodeId(2))
            .push_text(TextSource::Generated { node: NodeId(3) }, "b");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        if !leading_block {
            assert!(glyphs(&lines[0]).iter().all(|(_, size)| *size == 32.0));
        }
        assert!(
            glyphs(lines.last().unwrap())
                .iter()
                .all(|(_, size)| *size == 16.0)
        );
    }
}

#[test]
fn first_line_aggregate_text_glyph_limits() {
    for (limits, kind) in [
        (
            Limits {
                max_text_bytes: Some(7),
                ..Default::default()
            },
            LimitKind::TextBytes,
        ),
        (
            Limits {
                max_shaped_glyphs: Some(7),
                ..Default::default()
            },
            LimitKind::ShapedGlyphs,
        ),
        (
            Limits {
                max_items: Some(1),
                ..Default::default()
            },
            LimitKind::Items,
        ),
        (
            Limits {
                max_styles: Some(1),
                ..Default::default()
            },
            LimitKind::Styles,
        ),
    ] {
        let fonts = load_fonts(&limits).unwrap();
        let style = ParagraphStyle {
            first_line: Some(InlineStyle::default()),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
        assert_eq!(
            b.build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap_err()
                .kind,
            kind
        );
    }
}

#[test]
fn first_line_continues_inside_a_normal_shared_ligature() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut first = style.root.clone();
    first.text_transform = TextTransform::Uppercase;
    style.first_line = Some(first);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.with_offset_mapping(false)
        .push_text(TextSource::Generated { node: NodeId(1) }, "ffiX");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        10.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        actual.iter().map(Line::text_range).collect::<Vec<_>>(),
        vec![0..1, 1..3, 3..4]
    );
    let mut reference = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    reference.push_text(TextSource::Generated { node: NodeId(1) }, "fi");
    let expected = reference
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            10.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(glyphs(&actual[1]), glyphs(&expected[0]));
    assert_eq!(actual[1].inline_size(), expected[0].inline_size());
}

#[test]
fn first_line_inherits_root_differences_and_preserves_explicit_inline_style() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let first = InlineStyle {
        font_size: 32.0,
        text_transform: TextTransform::Uppercase,
        ..Default::default()
    };
    let style = ParagraphStyle {
        first_line: Some(first),
        ..Default::default()
    };
    let explicit = InlineStyle {
        font_size: 10.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.open_inline(NodeId(1), &style.root, Default::default())
        .push_text(TextSource::Generated { node: NodeId(2) }, "a")
        .close_inline()
        .open_inline(NodeId(3), &explicit, Default::default())
        .push_text(TextSource::Generated { node: NodeId(4) }, "b")
        .close_inline();
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    let runs: Vec<_> = actual[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some((r.node(), r.font_size())),
            _ => None,
        })
        .collect();
    assert_eq!(runs, vec![(Some(NodeId(2)), 32.0), (Some(NodeId(4)), 10.0)]);
}

#[test]
fn first_line_expanded_scalar_is_consumed_once_before_normal_continuation() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut first = style.root.clone();
    first.text_transform = TextTransform::Uppercase;
    style.first_line = Some(first);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.with_offset_mapping(false)
        .push_text(TextSource::Generated { node: NodeId(1) }, "ßa");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        10.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        actual.iter().map(Line::text_range).collect::<Vec<_>>(),
        vec![0..2, 2..3]
    );
    assert_eq!(
        glyphs(&actual[0]).len(),
        2,
        "SS expansion stays indivisible"
    );
    assert_eq!(glyphs(&actual[1]).len(), 1);
}

#[test]
fn first_line_exposes_actual_text_and_mapping_for_its_source_ranges() {
    use shodo::mapping::{Affinity, TextOrigin};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut first = style.root.clone();
    first.text_transform = TextTransform::Uppercase;
    style.first_line = Some(first);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 100,
        },
        "ßa",
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        10.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(actual[0].text(), "SSA");
    assert_eq!(&actual[0].text()[actual[0].text_range()], "SS");
    assert_eq!(
        actual[0]
            .offset_mapping()
            .unwrap()
            .text_to_dom(2, Affinity::Upstream),
        Some(TextOrigin::Dom {
            node: NodeId(1),
            offset: 102
        })
    );
    assert_eq!(actual[1].text(), "ßa");
    assert_eq!(&actual[1].text()[actual[1].text_range()], "a");
    assert_eq!(
        actual[1]
            .offset_mapping()
            .unwrap()
            .text_to_dom(2, Affinity::Downstream),
        Some(TextOrigin::Dom {
            node: NodeId(1),
            offset: 102
        })
    );
}

#[test]
fn first_line_break_plans_use_the_alternate_cursor_for_expanded_text() {
    use shodo::style::{LineOptions, TextWrapStyle};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut first = style.root.clone();
    first.text_transform = TextTransform::Uppercase;
    style.first_line = Some(first);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ßa abc def");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &options, 20.0, &AtomicSizes::EMPTY);
        let mut constraint = LineConstraint::new(20.0);
        constraint.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("first planned line")
        };
        assert_eq!(&line.text()[line.text_range()], "SS", "{wrap:?}");
        let LineResult::Line(next) = p.next_line(
            &mut cx,
            line.break_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("planned continuation")
        };
        assert_eq!(next.text_range().start, 2, "{wrap:?}");
        assert!(line.inline_size() <= 20.0);
    }
}

#[test]
fn first_line_float_retry_keeps_actual_width_and_normal_source_cursor() {
    use shodo::node::OutOfFlowKind;
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut style = ParagraphStyle::default();
    style.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut first = style.root.clone();
    first.text_transform = TextTransform::Uppercase;
    style.first_line = Some(first);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ß")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "ffi tail");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut cx = LayoutContext::new();
    let LineResult::FloatEncountered {
        line_start,
        inline_position,
        float_cursor,
        ..
    } = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("wide float retry")
    };
    assert_eq!(line_start, p.start_token());
    let mut reference = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    reference.push_text(TextSource::Generated { node: NodeId(1) }, "SS");
    let expected = reference
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(inline_position, expected[0].inline_size());
    let mut constraint = LineConstraint::new(20.0);
    constraint.floats_placed_through = Some(float_cursor);
    let LineResult::Line(warm) = p.next_line(
        &mut cx,
        line_start,
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("warm first")
    };
    let LineResult::Line(cold) = p.next_line(
        &mut LayoutContext::new(),
        line_start,
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("cold first")
    };
    assert_eq!(warm.text_range(), cold.text_range());
    assert_eq!(glyphs(&warm), glyphs(&cold));
    assert_eq!(warm.break_token(), cold.break_token());
    assert_eq!(glyphs(&warm).len(), 2);
    assert_eq!(warm.displaced_floats(), &[(NodeId(2), float_cursor)]);
    constraint.floats_placed_through = float_cursor.before();
    let LineResult::FloatEncountered {
        line_start: normal_start,
        inline_position,
        ..
    } = p.next_line(
        &mut cx,
        warm.break_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("withdrawn float on the normal continuation")
    };
    assert_eq!(normal_start, warm.break_token());
    assert_eq!(inline_position, 0.0);
    constraint.floats_placed_through = Some(float_cursor);
    let LineResult::Line(next) = p.next_line(
        &mut cx,
        normal_start,
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("normal continuation")
    };
    assert_eq!(
        next.text_range(),
        2..9,
        "processed float bytes are part of this line"
    );
    let runs: Vec<_> = next
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].text_range(),
        5..9,
        "glyph source excludes the marker and includes the hanging space"
    );
    assert_eq!(runs[0].clusters().next().unwrap().text_range, 5..8);
    let mut reference = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    reference.push_text(TextSource::Generated { node: NodeId(3) }, "ffi ");
    let expected = reference
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            20.0,
            &AtomicSizes::EMPTY,
        );
    assert_eq!(
        glyphs(&next),
        glyphs(&expected[0]),
        "normal ffi and space each emitted once"
    );
    assert_eq!(next.inline_size(), expected[0].inline_size());
}

#[test]
fn forced_break_does_not_restart_first_line_style() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_size: 32.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
        .push_forced_break(NodeId(2))
        .push_text(TextSource::Generated { node: NodeId(3) }, "b");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(actual.len(), 2);
    assert_eq!(glyphs(&actual[0])[0].1, 32.0);
    assert_eq!(glyphs(&actual[1])[0].1, 16.0);
}

#[test]
fn first_line_intrinsic_widths_follow_the_actual_unbreakable_word() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for size in [8.0, 32.0] {
        let first = InlineStyle {
            font_size: size,
            ..Default::default()
        };
        let style = ParagraphStyle {
            first_line: Some(first.clone()),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "WWWW");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let actual = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &Default::default(),
        );
        let mut reference = ParagraphBuilder::new(
            &ParagraphStyle {
                root: first,
                ..Default::default()
            },
            &Limits::default(),
        );
        reference.push_text(TextSource::Generated { node: NodeId(1) }, "WWWW");
        let expected = reference
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .intrinsic_sizes(
                &mut LayoutContext::new(),
                &Default::default(),
                &Default::default(),
            );
        assert_eq!(
            (actual.min_content, actual.max_content),
            (expected.min_content, expected.max_content),
            "first-line {size}px"
        );
    }
}

#[test]
fn first_line_intrinsics_use_normal_words_after_the_first_soft_break() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for size in [8.0, 32.0] {
        let first = InlineStyle {
            font_size: size,
            ..Default::default()
        };
        let style = ParagraphStyle {
            first_line: Some(first.clone()),
            ..Default::default()
        };
        let measure = |text: &str, root: InlineStyle| {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root,
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            b.build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap()
                .intrinsic_sizes(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    &Default::default(),
                )
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a WWWW");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let actual = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &Default::default(),
        );
        let min = measure("a", first.clone())
            .min_content
            .max(measure("WWWW", InlineStyle::default()).min_content);
        let max = measure("a WWWW", first).max_content.max(min);
        assert_eq!(
            (actual.min_content, actual.max_content),
            (min, max),
            "first-line {size}px"
        );
    }
}

#[test]
fn first_line_intrinsics_stop_at_forced_and_block_boundaries() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_size: 8.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    for forced in [false, true] {
        for leading in [false, true] {
            let mut b = ParagraphBuilder::new(&style, &Limits::default());
            if !leading {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            }
            if forced {
                b.push_forced_break(NodeId(2));
            } else {
                b.push_block_in_inline(NodeId(2));
            }
            b.push_text(TextSource::Generated { node: NodeId(3) }, "WWWW");
            let p = b
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap();
            let actual = p.intrinsic_sizes(
                &mut LayoutContext::new(),
                &Default::default(),
                &Default::default(),
            );
            let mut reference =
                ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
            reference.push_text(TextSource::Generated { node: NodeId(3) }, "WWWW");
            let expected = reference
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap()
                .intrinsic_sizes(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    &Default::default(),
                );
            assert_eq!(actual, expected, "forced={forced}, leading={leading}");
        }
    }
}

#[test]
fn first_line_intrinsic_manual_hyphen_uses_joint_shaping_before_normal_tail() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let first = InlineStyle {
        font_size: 32.0,
        ..Default::default()
    };
    let style = ParagraphStyle {
        first_line: Some(first.clone()),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "T\u{ad}iii");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    let mut reference = ParagraphBuilder::new(
        &ParagraphStyle {
            root: first,
            ..Default::default()
        },
        &Limits::default(),
    );
    reference.push_text(TextSource::Generated { node: NodeId(1) }, "T-");
    let expected = reference
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &Default::default(),
        );
    assert_eq!(actual.min_content, expected.max_content);
}

#[test]
fn first_line_casing_does_not_leave_a_deleted_mark_for_the_normal_line() {
    use shodo::node::OutOfFlowKind;
    let fonts = load_fonts(&Limits::default()).unwrap();
    for mapping in [false, true] {
        let style = ParagraphStyle {
            first_line: Some(InlineStyle {
                text_transform: TextTransform::Uppercase,
                lang: Some("lt".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.with_offset_mapping(mapping)
            .push_text(TextSource::Generated { node: NodeId(1) }, "i")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(3) }, "\u{307}");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let actual = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            0.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            actual.len(),
            1,
            "mapping={mapping}: original grapheme is indivisible"
        );
        assert_eq!(actual[0].text(), "I\u{fffc}");
        assert_eq!(actual[0].text_range(), 0..4);
        assert_eq!(
            glyphs(&actual[0]).len(),
            1,
            "deleted dot is never restored on a normal line"
        );
    }
}

#[test]
fn first_line_composed_kana_consumes_voicing_mark_across_float_once() {
    use shodo::node::OutOfFlowKind;
    let fonts = load_fonts(&Limits::default()).unwrap();
    for mapping in [false, true] {
        let style = ParagraphStyle {
            first_line: Some(InlineStyle {
                text_transform: TextTransform::FullWidth,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.with_offset_mapping(mapping)
            .push_text(TextSource::Generated { node: NodeId(1) }, "ｶ")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(3) }, "ﾞ")
            .push_text(TextSource::Generated { node: NodeId(4) }, "A");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let actual = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            16.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            actual.len(),
            2,
            "mapping={mapping}: composed original grapheme is consumed once"
        );
        assert_eq!(actual[0].text(), "ガ\u{fffc}Ａ");
        assert_eq!(
            actual[0].text_range(),
            0..6,
            "float stays inside the consumed original grapheme"
        );
        assert_eq!(glyphs(&actual[0]).len(), 1);
        assert_eq!(actual[1].text_range(), 9..10);
        assert_eq!(
            glyphs(&actual[1]).len(),
            1,
            "voicing mark is never restored on the normal line"
        );
    }
}

#[test]
fn first_line_shrinking_transform_uses_final_aggregate_text_size() {
    let limits = Limits {
        max_text_bytes: Some(4),
        max_shaped_glyphs: Some(3),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            text_transform: TextTransform::Uppercase,
            lang: Some("lt".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "i\u{307}");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(p.text().len() + actual[0].text().len(), 4);
    assert_eq!(actual[0].text(), "I");
    assert_eq!(actual.len(), 1);
}

#[test]
fn first_line_aggregate_limits_accept_the_exact_retained_boundary() {
    let limits = Limits {
        max_text_bytes: Some(6),
        max_items: Some(2),
        max_styles: Some(2),
        max_shaped_glyphs: Some(4),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        glyphs(&lines[0]).len(),
        3,
        "one normal ligature plus three alternate glyphs fit budget4"
    );
    assert_eq!(p.text().len() + lines[0].text().len(), 6);
    for (limits, kind, expected_limit, actual) in [
        (
            Limits {
                max_text_bytes: Some(5),
                ..Default::default()
            },
            LimitKind::TextBytes,
            5,
            6,
        ),
        (
            Limits {
                max_items: Some(1),
                ..Default::default()
            },
            LimitKind::Items,
            1,
            2,
        ),
        (
            Limits {
                max_styles: Some(1),
                ..Default::default()
            },
            LimitKind::Styles,
            1,
            2,
        ),
        (
            Limits {
                max_shaped_glyphs: Some(3),
                ..Default::default()
            },
            LimitKind::ShapedGlyphs,
            3,
            4,
        ),
    ] {
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
        let error = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap_err();
        assert_eq!(
            (error.kind, error.limit, error.actual),
            (kind, expected_limit, actual)
        );
    }
}

#[test]
fn first_line_plan_validity_compares_sanitized_inputs() {
    use shodo::style::LineOptions;
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a b");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut options = LineOptions::default();
    options.text_indent.length = f32::NAN;
    let mut cx = LayoutContext::new();
    let plan = p.plan_breaks(&mut cx, &options, f32::NAN, &AtomicSizes::EMPTY);
    cx.take_warnings();
    let mut constraint = LineConstraint::new(f32::NAN);
    constraint.break_plan = Some(&plan);
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY
        ),
        LineResult::Line(_)
    ));
    let warnings = cx.take_warnings();
    assert!(
        !warnings
            .iter()
            .any(|w| w.kind == shodo::limits::WarningKind::Unsupported),
        "valid plan must match after input normalization: {warnings:?}"
    );
}

#[test]
fn first_line_intrinsic_break_after_consumed_mark_and_float_uses_normal_tail() {
    use shodo::node::OutOfFlowKind;
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            text_transform: TextTransform::FullWidth,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ｶ")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "ﾞA");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(
        actual.min_content, 16.0,
        "break follows the consumed source grapheme"
    );
    assert_eq!(
        actual.max_content, 32.0,
        "unbroken first line remains full-width"
    );
}
