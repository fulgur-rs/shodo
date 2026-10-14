use super::*;
use crate::font::FontFaceDescriptor;
use crate::node::TextSource;

struct ReferenceGuard(bool);

impl ReferenceGuard {
    fn new(reference: bool) -> Self {
        Self(paint_reuse::FORCE_REFERENCE.with(|flag| flag.replace(reference)))
    }
}

struct IntrinsicReferenceGuard(bool);

impl IntrinsicReferenceGuard {
    fn new(reference: bool) -> Self {
        Self(crate::line::intrinsic::FORCE_TWO_PASSES.with(|flag| flag.replace(reference)))
    }
}

impl Drop for IntrinsicReferenceGuard {
    fn drop(&mut self) {
        crate::line::intrinsic::FORCE_TWO_PASSES.with(|flag| flag.set(self.0));
    }
}

impl Drop for ReferenceGuard {
    fn drop(&mut self) {
        paint_reuse::FORCE_REFERENCE.with(|flag| flag.set(self.0));
    }
}

fn input(style: &ParagraphStyle, limits: &Limits) -> ParagraphBuilder {
    let mut builder = ParagraphBuilder::new(style, limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 7,
        },
        "of",
    );
    let mut child = style.root.clone();
    child.font_weight = 600.0;
    let mut first = child.clone();
    first.paint.color = [255, 0, 0, 255];
    builder.open_inline_with_first_line(NodeId(2), &child, &first, Default::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(3),
            offset: 4,
        },
        "fice  Straße\nＡ ｶﾞ مرحبا",
    );
    builder.close_inline();
    builder
}

fn prepared_snapshot(data: &PreparedBuild) -> String {
    format!(
        "{:?}",
        (
            (&data.style, &data.limits, data.glyph_budget, &data.styles),
            &data.processed.text,
            &data.processed.items,
            &data.processed.mapping,
            &data.processed.indivisible,
            data.processed
                .source_spans
                .iter()
                .map(|s| (&s.old, &s.new, s.kind))
                .collect::<Vec<_>>(),
            data.processed
                .width_origins
                .iter()
                .map(|s| (&s.text, &s.before_width))
                .collect::<Vec<_>>(),
            (
                &data.breaks.graphemes,
                &data.breaks.typographic_starts,
                &data.breaks.authored_bidi_controls,
                &data.breaks.caret_cuts,
                &data.breaks.opportunities
            ),
            (
                &data.bidi.levels,
                data.bidi
                    .paragraphs
                    .iter()
                    .map(|p| (&p.text, p.base_level, p.inline_level))
                    .collect::<Vec<_>>()
            ),
            (&data.combine_spans, data.used_direction),
        )
    )
}

fn analysis_snapshot(
    result: Result<ParagraphAnalysis, LimitExceeded>,
) -> Result<String, LimitExceeded> {
    result.map(|analysis| {
        let state = analysis.state;
        format!(
            "{:?}",
            (
                prepared_snapshot(&state.normal),
                state.alternate.as_ref().map(prepared_snapshot),
                state.source_cuts,
                state.warnings,
                state.alternate_warnings,
            )
        )
    })
}

fn paint_style() -> ParagraphStyle {
    let mut style = ParagraphStyle::default();
    let mut first = style.root.clone();
    first.paint.color = [0, 0, 255, 255];
    style.first_line = Some(first);
    style
}

#[test]
fn mapping_budget_matches_replay_at_clone_admission_boundaries() {
    for transform in [
        crate::style::TextTransform::Uppercase,
        crate::style::TextTransform::FullWidth,
    ] {
        let mut style = paint_style();
        style.root.text_transform = transform;
        style.first_line.as_mut().unwrap().text_transform = transform;
        style.root.text_combine_upright = crate::style::TextCombineUpright::All;
        style.first_line.as_mut().unwrap().text_combine_upright =
            crate::style::TextCombineUpright::All;
        let unlimited = Limits {
            max_mapping_bytes: None,
            ..Default::default()
        };
        let analysis = input(&style, &unlimited).analyze().unwrap();
        assert!(analysis.state.alternate_reuses_analysis);
        let one = analysis.state.normal.processed.mapping_budget.used;
        let both = one * 2;
        for cap in [0, 1, one - 1, one, one + 1, both - 1, both, both + 1] {
            let limits = Limits {
                max_mapping_bytes: Some(cap),
                ..Default::default()
            };
            let candidate = analysis_snapshot(input(&style, &limits).analyze());
            let reference = {
                let _guard = ReferenceGuard::new(true);
                analysis_snapshot(input(&style, &limits).analyze())
            };
            assert_eq!(candidate, reference, "transform={transform:?}, cap={cap}");
            assert_eq!(candidate.is_ok(), cap >= both);
        }
    }
}

fn fonts() -> FontCollection {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    for (family, bytes) in [
        ("Latin", crate::test_support::fonts::LATIN),
        ("CJK", crate::test_support::fonts::CJK),
        ("Arabic", crate::test_support::fonts::ARABIC),
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
    fonts
}

fn font_paint_style() -> ParagraphStyle {
    let mut style = paint_style();
    style.root.font_families = ["Latin", "CJK", "Arabic"]
        .map(|family| crate::style::FontFamily::Named(family.into()))
        .to_vec();
    style.first_line.as_mut().unwrap().font_families = style.root.font_families.clone();
    style
}

fn work_reset() {
    ANALYSIS_PASSES.with(|count| count.set(0));
    crate::analysis::itemize::MATCH_CALLS.with(|count| count.set(0));
    crate::shape::HARFRUST_SHAPE_CALLS.with(|count| count.set(0));
}

fn work_counts() -> (usize, usize, usize) {
    (
        ANALYSIS_PASSES.with(std::cell::Cell::get),
        crate::analysis::itemize::MATCH_CALLS.with(std::cell::Cell::get),
        crate::shape::HARFRUST_SHAPE_CALLS.with(std::cell::Cell::get),
    )
}

#[test]
fn paint_only_build_does_not_repeat_font_matching_or_shaping() {
    let fonts = fonts();
    let mut style = font_paint_style();
    let build = |style: &ParagraphStyle| {
        let mut builder = ParagraphBuilder::new(style, &Limits::default());
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "office affinity first line",
        );
        work_reset();
        builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        work_counts()
    };
    let first_line = build(&style);
    style.first_line = None;
    let normal = build(&style);
    assert!(
        normal.1 > 0 && normal.2 > 0,
        "actual matching and Harfrust work"
    );
    assert_eq!(
        first_line, normal,
        "paint-only first-line must not repeat analysis, matching or shaping"
    );
}

#[test]
fn paint_only_analysis_uses_one_pass() {
    let style = paint_style();
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.with_offset_mapping(true);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "office affinity  first\nline",
    );
    ANALYSIS_PASSES.with(|count| count.set(0));
    let _analysis = builder.analyze().unwrap();
    assert_eq!(
        ANALYSIS_PASSES.with(std::cell::Cell::get),
        1,
        "paint-only first-line must reuse normal analysis"
    );
}

#[test]
fn changed_inherited_float_rejects_without_cloning_style_payloads() {
    let mut style = paint_style();
    style.first_line.as_mut().unwrap().font_size = 22.0;
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "office affinity first line",
    );
    paint_reuse::STYLE_COMPARISON_CLONES.with(|count| count.set(0));
    ANALYSIS_PASSES.with(|count| count.set(0));
    builder.analyze().unwrap();
    assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
    assert_eq!(
        paint_reuse::STYLE_COMPARISON_CLONES.with(std::cell::Cell::get),
        0,
        "an inherited float mismatch must reject before the owned style clone"
    );
}

#[test]
fn reused_analysis_matches_reference_for_identical_transforms_and_style_maps() {
    for transformed in [false, true] {
        for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
            let mut style = paint_style();
            style.writing_mode = mode;
            if transformed {
                style.root.text_transform = crate::style::TextTransform::UppercaseFullWidth;
                style.first_line.as_mut().unwrap().text_transform = style.root.text_transform;
            }
            ANALYSIS_PASSES.with(|count| count.set(0));
            let candidate = analysis_snapshot(input(&style, &Limits::default()).analyze());
            assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 1);
            let reference = {
                let _guard = ReferenceGuard::new(true);
                ANALYSIS_PASSES.with(|count| count.set(0));
                analysis_snapshot(input(&style, &Limits::default()).analyze())
            };
            assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
            assert_eq!(candidate, reference);
        }
    }
}

#[test]
fn analysis_fallback_preserves_callbacks_warnings_and_limit_errors() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let style = paint_style();
    let run_callback = |reference| {
        let _guard = ReferenceGuard::new(reference);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut builder = input(&style, &Limits::default());
        builder.with_line_break_override(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
            crate::LineBreakOverride::Allow
        });
        ANALYSIS_PASSES.with(|count| count.set(0));
        let snapshot = analysis_snapshot(builder.analyze()).unwrap();
        assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
        (snapshot, calls.load(Ordering::Relaxed))
    };
    let callback = run_callback(false);
    assert!(callback.1 > 0);
    assert_eq!(callback, run_callback(true));

    for cap in [0, 1, 8] {
        let limits = Limits {
            max_warnings: Some(cap),
            ..Limits::default()
        };
        let mut invalid = style.clone();
        invalid.root.font_size = f32::NAN;
        invalid.first_line.as_mut().unwrap().font_size = f32::NAN;
        ANALYSIS_PASSES.with(|count| count.set(0));
        let candidate = analysis_snapshot(input(&invalid, &limits).analyze()).unwrap();
        assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 2);
        let reference = {
            let _guard = ReferenceGuard::new(true);
            analysis_snapshot(input(&invalid, &limits).analyze()).unwrap()
        };
        assert_eq!(candidate, reference);
    }

    for cap in 0..120 {
        for text_cap in [false, true] {
            let mut limits = Limits::default();
            if text_cap {
                limits.max_text_bytes = Some(cap);
            } else {
                limits.max_items = Some(cap);
            }
            let candidate = analysis_snapshot(input(&style, &limits).analyze());
            let reference = {
                let _guard = ReferenceGuard::new(true);
                analysis_snapshot(input(&style, &limits).analyze())
            };
            assert_eq!(candidate, reference, "cap={cap}, text={text_cap}");
        }
    }
}

fn build_probe(
    builder: ParagraphBuilder,
    fonts: &FontCollection,
    reference: bool,
) -> (Result<Paragraph, LimitExceeded>, (usize, usize, usize)) {
    let _guard = ReferenceGuard::new(reference);
    work_reset();
    let result = builder.build(&mut crate::LayoutContext::new(), fonts);
    (result, work_counts())
}

fn line_snapshot(line: &crate::Line) -> String {
    let glyphs = line
        .fragments()
        .map(|fragment| match fragment {
            crate::Fragment::GlyphRun(run) => format!(
                "{:?}",
                (
                    (
                        run.style_index(),
                        run.node(),
                        run.source(),
                        run.text_range(),
                        run.font(),
                        run.font_size().to_bits(),
                        run.bidi_level()
                    ),
                    (
                        run.paint_style(),
                        run.normalized_coords(),
                        run.variations(),
                        run.script(),
                        run.language()
                    ),
                    (
                        run.embolden(),
                        run.skew().map(f32::to_bits),
                        run.orientation(),
                        run.glyph_transform(),
                        run.metrics(),
                        run.vertical_metrics()
                    ),
                    (
                        run.inline_start().to_bits(),
                        run.inline_size().to_bits(),
                        run.baseline().to_bits()
                    ),
                    run.clusters().collect::<Vec<_>>(),
                    run.glyphs().collect::<Vec<_>>(),
                    (0..run.glyphs().len())
                        .map(|i| run.glyph_origin(i).map(|(x, y)| [x.to_bits(), y.to_bits()]))
                        .collect::<Vec<_>>(),
                )
            ),
            crate::Fragment::RubyAnnotation(_) => "ruby".into(),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>();
    let ruby = line
        .ruby_annotations()
        .map(|a| {
            (
                a.container(),
                a.node(),
                a.level(),
                a.base_text_range(),
                a.text_range(),
                a.visibility(),
                a.origin(),
                a.transform(),
                line_snapshot(a.line()),
            )
        })
        .collect::<Vec<_>>();
    let token = line.break_token();
    format!(
        "{:?}",
        (
            (
                line.text(),
                line.text_range(),
                line.offset_mapping(),
                (token.unit, token.flags),
                line.break_reason(),
                line.is_last()
            ),
            (
                line.inline_size().to_bits(),
                line.block_size().to_bits(),
                line.block_offset().to_bits(),
                line.hang_start().to_bits(),
                line.hang_end().to_bits(),
                line.metrics(),
                line.overflow_rect()
            ),
            (
                line.writing_mode(),
                line.used_direction(),
                line.displaced_floats(),
                line.text_combinations().collect::<Vec<_>>()
            ),
            glyphs,
            ruby,
        )
    )
}

fn paragraph_snapshot(
    result: Result<Paragraph, LimitExceeded>,
) -> Result<Vec<String>, LimitExceeded> {
    result.map(|paragraph| {
        let data_snapshot = |data: &ParagraphData| {
            format!(
                "{:?}",
                (
                    (&data.text, &data.items, &data.styles, &data.mapping),
                    (&data.units, &data.boxes, &data.runs, &data.shape_items),
                    (
                        &data.glyphs.id,
                        &data.glyphs.advance,
                        &data.glyphs.pen,
                        &data.glyphs.offset_inline,
                        &data.glyphs.offset_block,
                        &data.glyphs.cluster,
                        &data.glyphs.flags
                    ),
                    (
                        &data.clusters,
                        &data.glyph_clusters,
                        &data.selectable_clusters,
                        &data.shaping_barriers
                    ),
                )
            )
        };
        let mut out = vec![
            data_snapshot(&paragraph.data),
            format!("{:?}", paragraph.warnings()),
        ];
        if let Some(first) = &paragraph.data.first_line {
            out.push(data_snapshot(&first.data));
            out.push(format!(
                "{:?}",
                (&first.normal_cursors, &first.alternate_cursors)
            ));
        }
        let options = crate::style::LineOptions::default();
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(&mut cx, &options, &crate::AtomicIntrinsics::EMPTY);
        out.push(format!(
            "{:?}",
            (
                [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
                cx.take_warnings()
            )
        ));
        for width in [0.0, 32.0, 240.0, f32::MAX] {
            let mut token = paragraph.start_token();
            let mut cx = crate::LayoutContext::new();
            for _ in 0..1000 {
                match paragraph.next_line(
                    &mut cx,
                    token,
                    &options,
                    &crate::LineConstraint::new(width),
                    &crate::AtomicSizes::EMPTY,
                ) {
                    crate::LineResult::Line(line) => {
                        assert_ne!(line.break_token(), token, "forward progress");
                        token = line.break_token();
                        out.push(line_snapshot(&line));
                    }
                    crate::LineResult::Done => break,
                    other => panic!("unexpected {other:?}"),
                }
            }
            out.push(format!("{:?}", cx.take_warnings()));
        }
        out
    })
}

#[test]
fn reused_shaping_preserves_first_line_paint_and_full_line_geometry() {
    let fonts = fonts();
    for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
        for transform in [
            crate::style::TextTransform::None,
            crate::style::TextTransform::UppercaseFullWidth,
        ] {
            let mut style = font_paint_style();
            style.writing_mode = mode;
            style.root.text_transform = transform;
            style.first_line.as_mut().unwrap().text_transform = transform;
            let candidate = build_probe(input(&style, &Limits::default()), &fonts, false);
            let reference = build_probe(input(&style, &Limits::default()), &fonts, true);
            assert_eq!(candidate.1.0, 1);
            assert_eq!(reference.1.0, 2);
            assert_eq!(
                paragraph_snapshot(candidate.0),
                paragraph_snapshot(reference.0)
            );
        }
    }
    let style = font_paint_style();
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Generated { node: NodeId(1) },
        "office first affinity line",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let mut cx = crate::LayoutContext::new();
    let lines = paragraph.break_all(
        &mut cx,
        &Default::default(),
        45.0,
        &crate::AtomicSizes::EMPTY,
    );
    assert!(lines.len() > 1);
    for (index, line) in lines.iter().enumerate() {
        for fragment in line.fragments() {
            if let crate::Fragment::GlyphRun(run) = fragment {
                assert_eq!(
                    run.paint_style().color,
                    if index == 0 {
                        [0, 0, 255, 255]
                    } else {
                        [0, 0, 0, 255]
                    }
                );
            }
        }
    }
    // Identical shaping with separate paint owners must retain a ligature
    // crossing their source/box boundary, including first-line cursor cuts.
    let split = || {
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        let mut child = style.root.clone();
        child.paint.color = [0, 128, 0, 255];
        let mut first = child.clone();
        first.paint.color = [255, 0, 0, 255];
        b.push_text(
            TextSource::Dom {
                node: NodeId(40),
                offset: 0,
            },
            "of",
        )
        .open_inline_with_first_line(NodeId(41), &child, &first, Default::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(42),
                offset: 2,
            },
            "fice affinity first line",
        )
        .close_inline();
        b
    };
    let candidate = build_probe(split(), &fonts, false);
    let reference = build_probe(split(), &fonts, true);
    assert_eq!(candidate.1.0, 1);
    assert!(candidate.0.as_ref().unwrap().data.glyphs.len() < "office affinity first line".len());
    assert_eq!(
        paragraph_snapshot(candidate.0),
        paragraph_snapshot(reference.0)
    );

    // Reusing a warm scratch context must not inherit a different font or
    // edge result from the preceding build.
    let warm = |reference| {
        let _guard = ReferenceGuard::new(reference);
        let mut cx = crate::LayoutContext::new();
        let seed = input(&style, &Limits::default())
            .build(&mut cx, &fonts)
            .unwrap();
        seed.break_all(
            &mut cx,
            &Default::default(),
            32.0,
            &crate::AtomicSizes::EMPTY,
        );
        cx.take_warnings();
        let paragraph = split().build(&mut cx, &fonts);
        (paragraph_snapshot(paragraph), cx.take_warnings())
    };
    assert_eq!(warm(false), warm(true));
}

#[test]
fn first_line_paint_reuses_one_intrinsic_measure_pass() {
    let fonts = fonts();
    let style = font_paint_style();
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "123 456 789 987 654\n321 10 0 ".repeat(80).as_str(),
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();

    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        crate::line::intrinsic::INTRINSIC_UNIT_VISITS.with(|visits| visits.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
            crate::line::intrinsic::INTRINSIC_UNIT_VISITS.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 1, "paint-only geometry needs one full walk");
    assert_eq!(reference.2, 2, "reference path retains both walks");
    assert_eq!(
        optimized.3,
        paragraph.data.units.len(),
        "cached eligibility avoids a second intrinsic unit traversal"
    );
    assert_eq!(
        reference.3,
        paragraph.data.units.len() * 2,
        "reference path visits each dataset once per measurement"
    );
}

#[test]
fn first_line_paint_with_unsafe_edges_keeps_reference_walks() {
    let fonts = fonts();
    let style = font_paint_style();
    let limits = Limits {
        max_reshape_window_bytes: Some(0),
        max_warnings: Some(1),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "office affinity first line waffle official",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert!(
        paragraph
            .data
            .units
            .iter()
            .any(|unit| { unit.unsafe_to_break || unit.unsafe_to_concat })
    );
    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "unsafe shaping edges retain both walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_paint_with_tracking_saturation_keeps_warning_suppression() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.root.letter_spacing = 1_000_000.0;
    style.first_line.as_mut().unwrap().letter_spacing = 1_000_000.0;
    let limits = Limits {
        max_warnings: Some(1),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "1234567890".repeat(8).as_str(),
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "tracking saturation keeps both walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_paint_with_negative_advance_keeps_warning_suppression() {
    let fonts = fonts();
    let style = font_paint_style();
    let limits = Limits {
        max_warnings: Some(1),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "1",
    );
    let mut paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let data = std::sync::Arc::get_mut(&mut paragraph.data).unwrap();
    for advance in &mut data.glyphs.advance {
        *advance = crate::geometry::LayoutUnit::from_raw(-2);
    }
    let first = data.first_line.as_mut().unwrap();
    let alternate = std::sync::Arc::get_mut(&mut first.data).unwrap();
    for advance in &mut alternate.glyphs.advance {
        *advance = crate::geometry::LayoutUnit::from_raw(-2);
    }
    // This test mutates the built paragraph to exercise an unsafe geometry
    // that normal callers cannot mutate after publication. Refresh the
    // construction-time eligibility cache to model what the builder sees.
    let intrinsic_single_pass_safe = {
        let first = data.first_line.as_ref().unwrap();
        crate::line::intrinsic::single_pass_safe(data, first, &crate::style::LineOptions::default())
    };
    assert!(!intrinsic_single_pass_safe);
    data.first_line.as_mut().unwrap().intrinsic_single_pass_safe = intrinsic_single_pass_safe;

    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "negative advances keep both warning walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_paint_with_hanging_indent_after_forced_break_keeps_reference_walks() {
    let fonts = fonts();
    let style = font_paint_style();
    let limits = Limits {
        max_warnings: Some(1),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "123\n456",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let options = crate::style::LineOptions {
        text_indent: crate::style::TextIndent {
            length: -1.0e7,
            hanging: true,
            each_line: false,
        },
        ..Default::default()
    };
    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(&mut cx, &options, &crate::AtomicIntrinsics::EMPTY);
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "hanging indent keeps both warning walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_size_with_intermediate_glyph_saturation_keeps_reference_walks() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.first_line.as_mut().unwrap().font_size = 22.0;
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "1",
    );
    let mut paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();

    fn set_saturating_glyph_sequence(data: &mut crate::paragraph::ParagraphData) {
        use crate::analysis::units::UnitKind;
        let UnitKind::Cluster { run, space, .. } = data.units[0].kind else {
            panic!("expected one shaped cluster");
        };
        data.units[0].kind = UnitKind::Cluster {
            run,
            glyphs: 0..3,
            space,
        };
        data.glyphs.advance = [i32::MAX, 64, -64]
            .into_iter()
            .map(crate::geometry::LayoutUnit::from_raw)
            .collect();
    }

    let data = std::sync::Arc::get_mut(&mut paragraph.data).unwrap();
    set_saturating_glyph_sequence(data);
    let first = data.first_line.as_mut().unwrap();
    set_saturating_glyph_sequence(std::sync::Arc::get_mut(&mut first.data).unwrap());

    let measure = |force_reference| {
        let _guard = IntrinsicReferenceGuard::new(force_reference);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure(false);
    let reference = measure(true);
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(
        optimized.2, 2,
        "intermediate saturation keeps reference walks"
    );
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_font_size_change_measures_simple_text_in_one_pass() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.first_line.as_mut().unwrap().font_size = 22.0;
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    let text = "1 22 333\n444 55 6 ".repeat(80);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text.trim_end(),
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let measure = || {
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure();
    let reference = {
        let _guard = IntrinsicReferenceGuard::new(true);
        measure()
    };
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 1, "simple size changes share one unit walk");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_font_size_change_with_letter_spacing_keeps_reference_walks() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.root.letter_spacing = 1.0;
    style.first_line.as_mut().unwrap().letter_spacing = 1.0;
    style.first_line.as_mut().unwrap().font_size = 22.0;
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "1 22 333\n444 55 6",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let measure = || {
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure();
    let reference = {
        let _guard = IntrinsicReferenceGuard::new(true);
        measure()
    };
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "tracking requires the reference walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_font_size_change_with_ligatures_keeps_reference_walks() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.first_line.as_mut().unwrap().font_size = 22.0;
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    let text = "office affinity first line waffle official ".repeat(80);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text.trim_end(),
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
    let mut optimized_cx = crate::LayoutContext::new();
    let sizes = paragraph.intrinsic_sizes(
        &mut optimized_cx,
        &crate::style::LineOptions::default(),
        &crate::AtomicIntrinsics::EMPTY,
    );
    let optimized = (
        [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
        optimized_cx.take_warnings(),
        crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
    );
    let reference = {
        let _guard = IntrinsicReferenceGuard::new(true);
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(
            &mut cx,
            &crate::style::LineOptions::default(),
            &crate::AtomicIntrinsics::EMPTY,
        );
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.2, 2, "edge reshaping keeps both reference walks");
    assert_eq!(reference.2, 2, "reference path retains both walks");
}

#[test]
fn first_line_nonfinite_indent_keeps_warning_and_suppression_order() {
    let fonts = fonts();
    let style = font_paint_style();
    let limits = Limits {
        max_warnings: Some(1),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a short plain paragraph",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let mut options = crate::style::LineOptions::default();
    options.text_indent.length = f32::NAN;
    let measure = || {
        crate::line::intrinsic::MEASURE_PASSES.with(|passes| passes.set(0));
        let mut cx = crate::LayoutContext::new();
        let sizes = paragraph.intrinsic_sizes(&mut cx, &options, &crate::AtomicIntrinsics::EMPTY);
        (
            [sizes.min_content.to_bits(), sizes.max_content.to_bits()],
            cx.take_warnings(),
            crate::line::intrinsic::MEASURE_PASSES.with(std::cell::Cell::get),
        )
    };
    let optimized = measure();
    let reference = {
        let _guard = IntrinsicReferenceGuard::new(true);
        measure()
    };
    assert_eq!(optimized.0, reference.0);
    assert_eq!(optimized.1, reference.1);
    assert_eq!(optimized.1.len(), 2);
    assert_eq!(optimized.2, 2, "invalid indent keeps both warning passes");
}

#[test]
fn shape_fallback_preserves_warnings_saturation_and_glyph_errors() {
    let fonts = fonts();
    let style = font_paint_style();
    for cap in 0..90 {
        let limits = Limits {
            max_shaped_glyphs: Some(cap),
            ..Limits::default()
        };
        let candidate = build_probe(input(&style, &limits), &fonts, false);
        let reference = build_probe(input(&style, &limits), &fonts, true);
        assert_eq!(
            paragraph_snapshot(candidate.0),
            paragraph_snapshot(reference.0),
            "glyph cap {cap}"
        );
    }
    for warning_cap in [0, 1, 8] {
        let limits = Limits {
            max_warnings: Some(warning_cap),
            ..Limits::default()
        };
        let mut malformed = style.clone();
        malformed.first_line.as_mut().unwrap().paint.underline =
            Some(crate::style::TextDecoration {
                offset: Some(f32::NAN),
                thickness: Some(f32::INFINITY),
                ..Default::default()
            });
        let candidate = build_probe(input(&malformed, &limits), &fonts, false);
        let reference = build_probe(input(&malformed, &limits), &fonts, true);
        assert_eq!(
            paragraph_snapshot(candidate.0),
            paragraph_snapshot(reference.0)
        );
        for numeric in [false, true] {
            let limits = Limits {
                max_shaping_run_bytes: Some(1),
                max_warnings: Some(warning_cap),
                ..Limits::default()
            };
            let mut style = style.clone();
            if numeric {
                style.root.font_size_adjust = Some(crate::style::FontSizeAdjust {
                    metric: crate::style::FontMetricKind::ExHeight,
                    value: f32::MAX,
                });
                style.first_line.as_mut().unwrap().font_size_adjust = style.root.font_size_adjust;
            }
            let builder = || {
                let mut b = ParagraphBuilder::new(&style, &limits);
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "a\u{301}\u{301} office",
                );
                b
            };
            let candidate = build_probe(builder(), &fonts, false);
            let reference = build_probe(builder(), &fonts, true);
            assert!(!candidate.0.as_ref().unwrap().warnings().is_empty());
            assert_eq!(candidate.1.1, reference.1.1);
            assert_eq!(candidate.1.2, reference.1.2);
            assert_eq!(
                paragraph_snapshot(candidate.0),
                paragraph_snapshot(reference.0)
            );
        }
    }
}

#[test]
fn inherited_float_bits_and_ruby_use_full_fallback() {
    let fonts = fonts();
    let mut style = font_paint_style();
    style.root.font_variations = vec![crate::style::FontVariation {
        tag: *b"TEST",
        value: 0.0,
    }];
    style.first_line.as_mut().unwrap().font_variations = vec![crate::style::FontVariation {
        tag: *b"TEST",
        value: -0.0,
    }];
    let candidate = build_probe(input(&style, &Limits::default()), &fonts, false);
    let reference = build_probe(input(&style, &Limits::default()), &fonts, true);
    assert_eq!(candidate.1, reference.1);
    assert_eq!(candidate.1.0, 2);
    let first = &candidate
        .0
        .as_ref()
        .unwrap()
        .data
        .first_line
        .as_ref()
        .unwrap()
        .data;
    assert!(first.runs.iter().any(|run| {
        run.instance
            .variations
            .iter()
            .any(|v| v.value.to_bits() == (-0.0f32).to_bits())
    }));
    assert_eq!(
        paragraph_snapshot(candidate.0),
        paragraph_snapshot(reference.0)
    );

    let style = font_paint_style();
    let limits = Limits::default();
    let builder = || {
        use crate::{
            Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
            RubyVisibility,
        };
        let content = |node, text| {
            RubyContent::text(
                TextSource::Dom {
                    node: NodeId(node),
                    offset: 0,
                },
                text,
                &style.root,
                &limits,
            )
        };
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: content(11, "office"),
                align: RubyAlign::Center,
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(12),
                    content: content(13, "first"),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_ruby(NodeId(9), &style.root, ruby);
        b
    };
    let candidate = build_probe(builder(), &fonts, false);
    let reference = build_probe(builder(), &fonts, true);
    assert_eq!(candidate.1, reference.1);
    assert_eq!(
        paragraph_snapshot(candidate.0),
        paragraph_snapshot(reference.0)
    );
}

#[test]
fn combined_width_trial_limits_preserve_reference_failure() {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let glyph = |c| font.charmap().map(c).unwrap().to_u32() as u16;
    let (a, w, z) = (glyph('a'), glyph('W'), glyph('Z'));
    // ccmp expands each 'a' to [a,W]; hwid contracts that pair to Z.
    // Plain trial: 4 glyphs. Selected narrow output: 2 glyphs.
    let mut gsub = Vec::new();
    let put = |out: &mut Vec<u8>, values: &[u16]| {
        for value in values {
            out.extend(value.to_be_bytes());
        }
    };
    put(&mut gsub, &[1, 0, 10, 32, 58, 1]);
    gsub.extend(b"latn");
    put(&mut gsub, &[8, 4, 0, 0, 0xffff, 2, 0, 1, 2]);
    gsub.extend(b"ccmp");
    put(&mut gsub, &[14]);
    gsub.extend(b"hwid");
    put(&mut gsub, &[20, 0, 1, 0, 0, 1, 1]);
    put(&mut gsub, &[2, 6, 34]);
    put(&mut gsub, &[2, 0, 1, 8, 1, 14, 1, 8, 2, a, w, 1, 1, a]);
    put(
        &mut gsub,
        &[4, 0, 1, 8, 1, 18, 1, 8, 1, 4, z, 2, w, 1, 1, a],
    );
    let mut tables: Vec<_> = crate::font::sfnt::read_sfnt_tables(bytes)
        .into_iter()
        .filter(|(tag, _)| tag != b"GSUB")
        .collect();
    tables.push((*b"GSUB", gsub));
    tables.sort_by_key(|(tag, _)| *tag);
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            FontFaceDescriptor {
                family: "Trial".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = paint_style();
    style.writing_mode = WritingMode::VerticalRl;
    style.root.font_families = vec![crate::style::FontFamily::Named("Trial".into())];
    style.root.text_combine_upright = crate::style::TextCombineUpright::All;
    style.first_line.as_mut().unwrap().font_families = style.root.font_families.clone();
    let builder = |limits: &Limits| {
        let mut b = ParagraphBuilder::new(&style, limits);
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "aa",
        );
        b
    };
    let ordinary = build_probe(builder(&Limits::default()), &fonts, true)
        .0
        .unwrap();
    assert_eq!(
        ordinary.data.glyphs.len(),
        2,
        "fixture must select contracted width output"
    );
    assert!(
        ordinary
            .data
            .shape_items
            .iter()
            .all(|item| item.width_feature == Some(*b"hwid"))
    );
    let limits = Limits {
        max_shaped_glyphs: Some(4),
        ..Limits::default()
    };
    let candidate = build_probe(builder(&limits), &fonts, false);
    let reference = build_probe(builder(&limits), &fonts, true);
    assert_eq!(
        reference.0.as_ref().err(),
        Some(&LimitExceeded {
            kind: LimitKind::ShapedGlyphs,
            limit: 4,
            actual: 6
        })
    );
    assert_eq!(
        candidate.0.as_ref().err(),
        reference.0.as_ref().err(),
        "retained glyph count cannot bound unselected trials"
    );
    for cap in 0..12 {
        let limits = Limits {
            max_shaped_glyphs: Some(cap),
            ..Limits::default()
        };
        let candidate = build_probe(builder(&limits), &fonts, false);
        let reference = build_probe(builder(&limits), &fonts, true);
        assert_eq!(
            paragraph_snapshot(candidate.0),
            paragraph_snapshot(reference.0),
            "combined cap {cap}"
        );
    }
    let unlimited = Limits {
        max_shaped_glyphs: None,
        ..Limits::default()
    };
    let candidate = build_probe(builder(&unlimited), &fonts, false);
    let reference = build_probe(builder(&unlimited), &fonts, true);
    assert_eq!(candidate.1.2, 2);
    assert_eq!(reference.1.2, 4);
    assert_eq!(
        paragraph_snapshot(candidate.0),
        paragraph_snapshot(reference.0)
    );
}
