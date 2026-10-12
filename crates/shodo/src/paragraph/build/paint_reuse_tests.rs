use super::*;
use crate::font::{FontFaceDescriptor, FontOptions};
use crate::node::TextSource;

struct ReferenceGuard(bool);

impl ReferenceGuard {
    fn new(reference: bool) -> Self {
        Self(paint_reuse::FORCE_REFERENCE.with(|flag| flag.replace(reference)))
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

fn fonts() -> FontCollection {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
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
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if tag != *b"GSUB" {
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
    }
    tables.push((*b"GSUB", gsub));
    tables.sort_by_key(|(tag, _)| *tag);
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
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
