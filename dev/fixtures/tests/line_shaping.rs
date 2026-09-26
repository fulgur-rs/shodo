use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, OverflowWrap, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, Paragraph, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn build(text: &str, style: InlineStyle) -> Paragraph {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        root: style,
        direction: Direction::Ltr,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
    );
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
}
fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}
fn direct_width(text: &str) -> f32 {
    let font = harfrust::FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    let mut b = harfrust::UnicodeBuffer::new();
    b.push_str(text);
    b.guess_segment_properties();
    shaper
        .shape(b, Default::default())
        .glyph_positions()
        .iter()
        .map(|p| p.x_advance as f32 * 16.0 / shaper.units_per_em() as f32)
        .sum()
}
fn glyph_ids(line: &Line) -> Vec<u32> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .flat_map(|r| r.glyphs().map(|g| g.id))
        .collect()
}

#[test]
fn emergency_break_only_when_needed() {
    let style = InlineStyle {
        overflow_wrap: OverflowWrap::Anywhere,
        ..Default::default()
    };
    let p = build("WWWW", style.clone());
    let actual = lines(&p, direct_width("WW") + 0.1);
    assert_eq!(
        actual.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
        vec![0..2, 2..4]
    );
    assert_eq!(actual[0].break_reason(), shodo::BreakReason::Emergency);
    assert_eq!(
        lines(
            &build("WWWW", InlineStyle::default()),
            direct_width("WW") + 0.1
        )
        .len(),
        1
    );
    let p = build("abc def", style);
    let actual = lines(&p, direct_width("abc d") + 0.1);
    assert_eq!(
        actual[0].text_range(),
        0..4,
        "normal space opportunity precedes emergency"
    );
}

#[test]
fn break_word_vs_anywhere_min_content() {
    let normal = build("WWWW", InlineStyle::default());
    let word = build(
        "WWWW",
        InlineStyle {
            overflow_wrap: OverflowWrap::BreakWord,
            ..Default::default()
        },
    );
    let anywhere = build(
        "WWWW",
        InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
    );
    let intrinsic = |p: &Paragraph| {
        p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &Default::default(),
            &Default::default(),
        )
    };
    assert!((intrinsic(&word).min_content - intrinsic(&normal).min_content).abs() < 0.02);
    assert!(intrinsic(&anywhere).min_content < intrinsic(&word).min_content);
    assert_eq!(lines(&word, direct_width("WW") + 0.1).len(), 2);
}

#[test]
fn soft_hyphen_only_when_taken() {
    use skrifa::MetadataProvider;
    let p = build("ab\u{ad}cd", InlineStyle::default());
    let font = skrifa::FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let hyphen = font.charmap().map('-').unwrap().to_u32();
    let unbroken = lines(&p, 1000.0);
    assert!(!glyph_ids(&unbroken[0]).contains(&hyphen));
    let broken = lines(&p, direct_width("ab-") + 0.1);
    assert_eq!(
        broken.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
        vec![0..4, 4..6]
    );
    assert!(glyph_ids(&broken[0]).contains(&hyphen));
    assert!(!glyph_ids(&broken[1]).contains(&hyphen));
}

#[test]
fn overlay_changed_glyph_count_public_iterators() {
    let p = build(
        "ffi",
        InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
    );
    assert_eq!(glyph_ids(&lines(&p, 1000.0)[0]).len(), 1);
    let broken = lines(&p, direct_width("f") + 0.1);
    assert_eq!(
        broken.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
        vec![0..1, 1..2, 2..3]
    );
    for (at, line) in broken.iter().enumerate() {
        for r in line.fragments().filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        }) {
            assert_eq!(r.glyphs().len(), 1);
            assert_eq!(r.glyphs().get(0), r.glyphs().next());
            assert!(r.glyphs().get(1).is_none());
            assert_eq!(r.clusters().len(), 1);
            assert!(r.glyphs().all(|g| g.cluster == at as u32 && g.id != 0));
            assert!(r.font_data().is_some());
        }
    }
    assert_eq!(
        glyph_ids(&lines(&p, 1000.0)[0]).len(),
        1,
        "paragraph remains shared and unchanged"
    );
}

#[test]
fn cached_emergency_break_matches_cold_retry() {
    use shodo::node::OutOfFlowKind;
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "WW")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "WWWW");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut warm = LayoutContext::new();
    let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
        &mut warm,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    for width in [80.0, 45.0, 30.0, 15.0] {
        let mut c = LineConstraint::new(width);
        c.floats_placed_through = Some(float_cursor);
        let LineResult::Line(actual) = p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let LineResult::Line(expected) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(actual.text_range(), expected.text_range());
        assert_eq!(glyph_ids(&actual), glyph_ids(&expected));
        assert_eq!(actual.break_reason(), expected.break_reason());
    }
}

#[test]
fn hyphen_width_selection_none_intrinsics_and_plans_agree() {
    use shodo::style::{Hyphens, TextWrapStyle};
    use shodo::{LineConstraint, LineResult};
    let p = build("a ab\u{ad}cd", InlineStyle::default());
    let width = direct_width("a ab") + 0.1;
    assert_eq!(
        lines(&p, width)[0].text_range(),
        0..2,
        "hyphen advance prevents taking the later candidate"
    );
    let disabled = build(
        "ab\u{ad}cd",
        InlineStyle {
            hyphens: Hyphens::None,
            ..Default::default()
        },
    );
    assert_eq!(lines(&disabled, direct_width("ab-") + 0.1).len(), 1);
    let p = build("ab\u{ad}cd", InlineStyle::default());
    let measured = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert!((measured.min_content - direct_width("ab-").max(direct_width("cd"))).abs() < 0.04);
    assert!((measured.max_content - direct_width("abcd")).abs() < 0.04);
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let width = direct_width("ab-") + 0.1;
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            width,
            &AtomicSizes::EMPTY,
        );
        let mut c = LineConstraint::new(width);
        c.break_plan = Some(&plan);
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(line.text_range(), 0..4);
        assert!((line.inline_size() - direct_width("ab-")).abs() < 0.04);
    }
}

#[test]
fn hyphen_fallback_font_metadata_survives_collection_drop() {
    use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use shodo::style::FontFamily;
    use shodo::{LineConstraint, LineResult};
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let letters = fonts
        .register_face(
            FONTS[0].bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Letters".into(),
                unicode_ranges: vec![(97, 122), (173, 173)],
                ..Default::default()
            },
        )
        .unwrap();
    let dash = fonts
        .register_face(
            FONTS[0].bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Dash".into(),
                unicode_ranges: vec![(45, 45)],
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![
                FontFamily::Named("Letters".into()),
                FontFamily::Named("Dash".into()),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "ab\u{ad}cd",
    );
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let LineResult::Line(line) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(direct_width("ab-") + 0.1),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    drop(p);
    drop(fonts);
    let runs = line
        .fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        runs.iter().map(|r| r.font()).collect::<Vec<_>>(),
        vec![letters, dash]
    );
    assert_eq!(runs[1].text_range(), 2..4);
    assert_eq!(runs[1].glyphs().next().unwrap().cluster, 2);
    assert!(runs.iter().all(|r| r.font_data().is_some()));
}

#[test]
fn real_font_soft_break_precedes_transparent_float() {
    use shodo::node::OutOfFlowKind;
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "cd");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let result = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(direct_width("ab-") + 0.1),
        &AtomicSizes::EMPTY,
    );
    let LineResult::Line(line) = result else {
        panic!("{result:?}")
    };
    assert_eq!(line.text_range(), 0..4);
    assert!((line.inline_size() - direct_width("ab-")).abs() < 0.04);
}

#[test]
fn too_narrow_manual_hyphen_overflows_first_fragment() {
    let p = build("ab\u{ad}cd", InlineStyle::default());
    let actual = lines(&p, direct_width("ab") + 0.1);
    assert_eq!(actual[0].text_range(), 0..4);
    assert!((actual[0].inline_size() - direct_width("ab-")).abs() < 0.04);
}

#[test]
fn cached_hyphen_retry_preserves_break_glyphs_and_width() {
    use shodo::node::OutOfFlowKind;
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "cd");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut warm = LayoutContext::new();
    let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
        &mut warm,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    for width in [direct_width("ab-") + 0.1, direct_width("ab") + 0.1, 1.0] {
        let mut c = LineConstraint::new(width);
        c.floats_placed_through = Some(float_cursor);
        let LineResult::Line(actual) = p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let LineResult::Line(expected) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(actual.text_range(), expected.text_range());
        assert_eq!(glyph_ids(&actual), glyph_ids(&expected));
        assert_eq!(actual.inline_size(), expected.inline_size());
    }
}

#[test]
fn missing_font_discretionary_hyphen_has_no_unbroken_advance() {
    let limits = Limits::default();
    let fonts = shodo::font::FontCollection::with_options(
        &limits,
        shodo::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}cd");
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let unbroken = lines(&p, 1000.0);
    assert_eq!(unbroken[0].inline_size(), 64.0);
    assert_eq!(
        glyph_ids(&unbroken[0]).len(),
        4,
        "hidden SHY must not draw .notdef"
    );
    let broken = lines(&p, 48.1);
    assert_eq!(broken[0].text_range(), 0..4);
    assert_eq!(broken[0].inline_size(), 48.0);
    assert_eq!(broken[1].inline_size(), 32.0);
}

#[test]
fn cached_hyphen_costs_across_fonts_and_closing_boxes_match_cold_layout() {
    use shodo::node::{InlineEdges, OutOfFlowKind};
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    for (i, size) in [100.0, 1.0, 64.0, 2.0, 32.0, 3.0].iter().enumerate() {
        let style = InlineStyle {
            font_size: *size,
            ..Default::default()
        };
        b.open_inline(NodeId(i as u64 + 1), &style, InlineEdges::default())
            .push_text(
                TextSource::Generated {
                    node: NodeId(i as u64 + 1),
                },
                "a\u{ad}",
            )
            .close_inline();
    }
    b.push_out_of_flow(NodeId(20), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(21) }, "zz");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut warm = LayoutContext::new();
    let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
        &mut warm,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    for width in (1..=240).rev().chain([1000, 120, 60, 1]) {
        let mut c = LineConstraint::new(width as f32);
        c.floats_placed_through = Some(float_cursor);
        let LineResult::Line(actual) = p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let LineResult::Line(expected) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(actual.text_range(), expected.text_range(), "width={width}");
        assert_eq!(
            actual.inline_size(),
            expected.inline_size(),
            "width={width}"
        );
        assert_eq!(glyph_ids(&actual), glyph_ids(&expected), "width={width}");
        assert_eq!(
            actual.break_reason(),
            expected.break_reason(),
            "width={width}"
        );
    }
}

#[test]
fn pretty_considers_an_earlier_manual_hyphen_to_balance_raggedness() {
    use shodo::style::TextWrapStyle;
    let p = build("aa\u{ad}bb\u{ad}cc\u{ad}dd", InlineStyle::default());
    let width = direct_width("aabbcc-") + 0.1;
    let options = LineOptions {
        text_wrap_style: TextWrapStyle::Pretty,
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
    let mut c = shodo::LineConstraint::new(width);
    c.break_plan = Some(&plan);
    let shodo::LineResult::Line(line) =
        p.next_line(&mut cx, p.start_token(), &options, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    assert_eq!(line.text_range(), 0..8);
    assert!((line.inline_size() - direct_width("aabb-")).abs() < 0.04);
}

#[test]
fn unavailable_hyphen_window_keeps_word_whole_in_greedy_intrinsics_and_plans() {
    use shodo::style::TextWrapStyle;
    use shodo::{LineConstraint, LineResult};
    let limits = Limits {
        max_reshape_window_bytes: Some(0),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let text = "aa\u{ad}bb cc\u{ad}dd ee\u{ad}ff";
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert!(
        (intrinsic.min_content
            - direct_width("ccdd")
                .max(direct_width("aabb"))
                .max(direct_width("eeff")))
        .abs()
            < 0.04
    );
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        for width in [1.0, 24.0, 48.0, 64.0, 80.0] {
            let mut cx = LayoutContext::new();
            let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
            let mut c = LineConstraint::new(width);
            c.break_plan = Some(&plan);
            let mut token = p.start_token();
            loop {
                match p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY) {
                    LineResult::Line(line) => {
                        let end = line.text_range().end;
                        assert!(
                            end == text.len() || text.as_bytes()[end - 1] == b' ',
                            "unavailable manual break taken at {end}, {wrap:?}, width={width}"
                        );
                        token = line.break_token();
                    }
                    LineResult::Done => break,
                    other => panic!("{other:?}"),
                }
            }
        }
    }
}

#[test]
fn terminal_soft_hyphen_before_transparent_float_is_not_a_taken_break() {
    use shodo::node::{InlineEdges, OutOfFlowKind};
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(NodeId(10), &InlineStyle::default(), InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}")
        .close_inline()
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(1.0);
    let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("terminal SHY must not exclude the transparent float")
    };
    c.floats_placed_through = Some(float_cursor);
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(line.break_reason(), shodo::BreakReason::End);
    assert_eq!(glyph_ids(&line).len(), 2);
    assert!((line.inline_size() - direct_width("ab")).abs() < 0.04);
}

#[test]
fn soft_hyphen_before_a_mandatory_boundary_stays_hidden() {
    use shodo::node::{InlineEdges, OutOfFlowKind};
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    for block in [false, true] {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.open_inline(NodeId(10), &InlineStyle::default(), InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}")
            .close_inline()
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Absolute);
        if block {
            b.push_block_in_inline(NodeId(3));
        } else {
            b.push_forced_break(NodeId(3));
        }
        b.push_text(TextSource::Generated { node: NodeId(4) }, "cd");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(
            line.break_reason(),
            if block {
                shodo::BreakReason::BlockInInline
            } else {
                shodo::BreakReason::Forced
            }
        );
        assert_eq!(glyph_ids(&line).len(), 2);
        assert!((line.inline_size() - direct_width("ab")).abs() < 0.04);
    }
}

#[test]
fn ligature_prefix_suffix_widths_and_clusters_match_real_shapes() {
    use shodo::style::WordBreak;
    for style in [
        InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
        InlineStyle {
            word_break: WordBreak::BreakAll,
            ..Default::default()
        },
    ] {
        let p = build("ffi", style);
        let wide = lines(&p, 1000.0);
        assert_eq!(glyph_ids(&wide[0]).len(), 1);
        let cluster = wide[0]
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => r.clusters().next(),
                _ => None,
            })
            .unwrap();
        assert_eq!(cluster.text_range, 0..3);
        let actual = lines(&p, direct_width("ff") + 0.1);
        assert_eq!(
            actual.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
            vec![0..2, 2..3]
        );
        for (line, text, range) in [(&actual[0], "ff", 0..2), (&actual[1], "i", 2..3)] {
            assert!((line.inline_size() - direct_width(text)).abs() < 0.04);
            let run = line
                .fragments()
                .find_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .unwrap();
            assert_eq!(run.text_range(), range);
            assert_eq!(run.clusters().next().unwrap().text_range, range);
            assert_eq!(run.glyphs().len(), 1);
        }
    }
}

#[test]
fn cross_node_ligature_continuations_use_actual_source_owner() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    for (node, text) in [(NodeId(1), "f"), (NodeId(2), "f"), (NodeId(3), "i")] {
        b.push_text(TextSource::Dom { node, offset: 0 }, text);
    }
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let wide = lines(&p, 1000.0);
    assert_eq!(glyph_ids(&wide[0]).len(), 1);
    for (index, line) in lines(&p, direct_width("f") + 0.1).iter().enumerate() {
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.node(), Some(NodeId(index as u64 + 1)));
        assert_eq!(run.text_range(), index..index + 1);
        assert!(
            (line.inline_size() - direct_width(if index < 2 { "f" } else { "i" })).abs() < 0.04
        );
    }
}

#[test]
fn cached_ligature_interior_retry_matches_cold_source_and_width() {
    use shodo::node::OutOfFlowKind;
    use shodo::{LineConstraint, LineResult};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut warm = LayoutContext::new();
    let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
        &mut warm,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    for width in [
        1000.0,
        direct_width("ff") + 0.1,
        direct_width("f") + 0.1,
        1.0,
    ] {
        let mut c = LineConstraint::new(width);
        c.floats_placed_through = Some(float_cursor);
        let LineResult::Line(actual) = p.next_line(
            &mut warm,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let LineResult::Line(expected) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert_eq!(actual.text_range(), expected.text_range());
        assert_eq!(actual.inline_size(), expected.inline_size());
        assert_eq!(glyph_ids(&actual), glyph_ids(&expected));
        assert_eq!(actual.break_reason(), expected.break_reason());
    }
}

#[test]
fn ligature_slices_intrinsics_and_planned_lines_agree() {
    use shodo::style::TextWrapStyle;
    use shodo::{LineConstraint, LineResult};
    let p = build(
        "ffi",
        InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
    );
    let intrinsic = p.intrinsic_sizes(
        &mut LayoutContext::new(),
        &Default::default(),
        &Default::default(),
    );
    assert!((intrinsic.min_content - direct_width("f").max(direct_width("i"))).abs() < 0.04);
    assert!((intrinsic.max_content - direct_width("ffi")).abs() < 0.04);
    for wrap in [
        TextWrapStyle::Auto,
        TextWrapStyle::Balance,
        TextWrapStyle::Pretty,
    ] {
        let options = LineOptions {
            text_wrap_style: wrap,
            ..Default::default()
        };
        let width = direct_width("f") + 0.1;
        let mut cx = LayoutContext::new();
        let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
        let mut c = LineConstraint::new(width);
        c.break_plan = Some(&plan);
        let mut token = p.start_token();
        for (start, text) in [(0, "f"), (1, "f"), (2, "i")] {
            let LineResult::Line(line) =
                p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY)
            else {
                panic!()
            };
            assert_eq!(line.text_range(), start..start + 1);
            assert!((line.inline_size() - direct_width(text)).abs() < 0.04);
            assert_eq!(glyph_ids(&line).len(), 1);
            token = line.break_token();
        }
        assert!(matches!(
            p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY),
            LineResult::Done
        ));
    }
}

#[test]
fn too_small_ligature_slice_window_retains_whole_cluster_and_warns() {
    let limits = Limits {
        max_reshape_window_bytes: Some(1),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let actual = lines(&p, 1.0);
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].text_range(), 0..3);
    assert_eq!(glyph_ids(&actual[0]).len(), 1);
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.message.contains("intra-cluster reshape window exceeded"))
    );
}

#[test]
fn unbroken_ligature_slices_do_not_overwrite_justified_glyph_positions() {
    use shodo::style::{TextAlign, TextJustify};
    let p = build(
        "ffi x",
        InlineStyle {
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
    );
    let options = LineOptions {
        text_align: TextAlign::JustifyAll,
        text_justify: TextJustify::InterCharacter,
        ..Default::default()
    };
    let actual = p.break_all(
        &mut LayoutContext::new(),
        &options,
        100.0,
        &AtomicSizes::EMPTY,
    );
    let glyphs: Vec<_> = actual[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.glyphs())
        .collect();
    assert_eq!(glyphs.len(), 3);
    assert!((glyphs[0].inline_position).abs() < 0.02);
    assert!((glyphs[2].inline_position - (100.0 - direct_width("x"))).abs() < 0.04);
    let spare = 100.0 - direct_width("ffi x");
    assert!((glyphs[1].inline_position - (direct_width("ffi") + spare / 2.0)).abs() < 0.05);
    assert!((glyphs.iter().map(|g| g.advance).sum::<f32>() - 100.0).abs() < 0.02);
}

#[test]
fn rtl_ligature_slices_preserve_graphemes_joining_and_mark_attachments() {
    let p = build(
        "لَا",
        InlineStyle {
            lang: Some("ar".into()),
            overflow_wrap: OverflowWrap::Anywhere,
            ..Default::default()
        },
    );
    let actual = lines(&p, 1.0);
    assert_eq!(
        actual.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
        vec![0..4, 4..6]
    );
    let font = harfrust::FontRef::from_index(FONTS[2].bytes, 0).unwrap();
    let data = harfrust::ShaperData::new(&font);
    let shaper = data.shaper(&font).build();
    let scale = 16.0 / shaper.units_per_em() as f32;
    for (line, text, source, before, after) in
        [(&actual[0], "لَ", 0, "", "ا"), (&actual[1], "ا", 4, "لَ", "")]
    {
        let mut b = harfrust::UnicodeBuffer::new();
        for (offset, c) in text.char_indices() {
            b.add(c, source + offset as u32);
        }
        b.set_pre_context(before);
        b.set_post_context(after);
        b.set_direction(harfrust::Direction::RightToLeft);
        b.set_language(harfrust::Language::new("ar").unwrap());
        b.guess_segment_properties();
        let expected = shaper.shape(b, Default::default());
        let glyphs: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs())
            .collect();
        assert_eq!(glyphs.len(), expected.len());
        let mut pen = 0.0;
        for (glyph, (info, position)) in glyphs.iter().zip(
            expected
                .glyph_infos()
                .iter()
                .zip(expected.glyph_positions()),
        ) {
            assert_eq!((glyph.id, glyph.cluster), (info.glyph_id, info.cluster));
            assert!(
                (glyph.inline_position - (pen + position.x_offset as f32 * scale)).abs() < 0.04
            );
            assert!((glyph.block_offset + position.y_offset as f32 * scale).abs() < 0.04);
            pen += position.x_advance as f32 * scale;
        }
        assert!((line.inline_size() - pen).abs() < 0.04);
    }
}
