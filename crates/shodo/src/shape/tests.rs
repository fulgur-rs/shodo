use super::*;
use crate::font::FontCollection;
use crate::geometry::LayoutUnit;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;

#[test]
fn ordered_windows_do_not_allocate_glyph_indices() {
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle};

    for (text, bytes, mode) in [
        (
            "a",
            crate::test_support::fonts::LATIN,
            WritingMode::HorizontalTb,
        ),
        (
            "日",
            crate::test_support::fonts::CJK,
            WritingMode::HorizontalTb,
        ),
        (
            "日",
            crate::test_support::fonts::CJK,
            WritingMode::VerticalRl,
        ),
    ] {
        for budget in [None, Some(16)] {
            let limits = Limits {
                max_shaping_run_bytes: budget,
                ..Limits::default()
            };
            let fonts = FontCollection::with_options(
                &limits,
                crate::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            fonts
                .register_face(
                    bytes.to_vec(),
                    0,
                    crate::font::FontFaceDescriptor {
                        family: "Ordered".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let style = ParagraphStyle {
                root: InlineStyle {
                    font_families: vec![FontFamily::Named("Ordered".into())],
                    ..Default::default()
                },
                writing_mode: mode,
                ..Default::default()
            };
            let mut builder = crate::ParagraphBuilder::new(&style, &limits);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, &text.repeat(64));
            GLYPH_ORDER_ALLOCATIONS.with(|count| count.set(0));
            HARFRUST_SHAPE_CALLS.with(|count| count.set(0));
            let mut cx = crate::LayoutContext::new();
            let paragraph = builder.build(&mut cx, &fonts).unwrap();
            assert_eq!(paragraph.data.glyphs.len(), 64);
            assert!(paragraph.warnings().is_empty());
            assert!(cx.take_warnings().is_empty());
            assert_eq!(
                paragraph.data.glyphs.cluster,
                (0..64).map(|i| i * text.len() as u32).collect::<Vec<_>>()
            );
            if budget.is_some() {
                assert!(HARFRUST_SHAPE_CALLS.with(std::cell::Cell::get) > 1);
            }
            assert_eq!(
                GLYPH_ORDER_ALLOCATIONS.with(std::cell::Cell::get),
                0,
                "text={text}, mode={mode:?}, budget={budget:?}"
            );
        }
    }
}

#[test]
fn glyph_order_uses_corrected_clusters_and_preserves_ties() {
    type Case<'a> = (&'a [u32], u32, Option<&'a [usize]>);
    let cases: &[Case<'_>] = &[
        (&[], 0, None),
        (&[4], 0, None),
        (&[0, 0, 2, 2, 4], 0, None),
        // An inversion removed by the leading-control correction needs no sort.
        (&[2, 0, 1], 2, None),
        (&[4, 2, 2, 0, 0], 0, Some(&[3, 4, 1, 2, 0])),
        (&[0, 4, 2, 2], 0, Some(&[0, 2, 3, 1])),
        // The correction also changes which glyphs must retain their tie order.
        (&[4, 0, 3, 0, 4], 3, Some(&[1, 2, 3, 0, 4])),
    ];
    for &(clusters, leading_end, expected) in cases {
        let infos: Vec<_> = clusters
            .iter()
            .map(|&cluster| {
                let mut info = harfrust::GlyphInfo::default();
                info.cluster = cluster;
                info
            })
            .collect();
        assert_eq!(
            glyph_order(&infos, leading_end).as_deref(),
            expected,
            "clusters={clusters:?}, leading_end={leading_end}"
        );
    }
}

#[derive(Clone, Copy)]
struct WidthProbeCase {
    name: &'static str,
    text: &'static str,
    mode: WritingMode,
    direction: crate::geometry::Direction,
    multiple_styles: bool,
    expected_width_feature: Option<[u8; 4]>,
}

const WIDTH_PROBE_CASES: [WidthProbeCase; 4] = [
    WidthProbeCase {
        name: "two-vertical-rl-ltr",
        text: "12",
        mode: WritingMode::VerticalRl,
        direction: crate::geometry::Direction::Ltr,
        multiple_styles: false,
        expected_width_feature: Some(*b"hwid"),
    },
    WidthProbeCase {
        name: "three-vertical-lr-rtl",
        text: "123",
        mode: WritingMode::VerticalLr,
        direction: crate::geometry::Direction::Rtl,
        multiple_styles: false,
        expected_width_feature: None,
    },
    WidthProbeCase {
        name: "four-vertical-rl-multiple-fonts",
        text: "1234",
        mode: WritingMode::VerticalRl,
        direction: crate::geometry::Direction::Ltr,
        multiple_styles: true,
        expected_width_feature: None,
    },
    WidthProbeCase {
        name: "two-vertical-lr-rtl-multiple-fonts",
        text: "12",
        mode: WritingMode::VerticalLr,
        direction: crate::geometry::Direction::Rtl,
        multiple_styles: true,
        expected_width_feature: Some(*b"hwid"),
    },
];

struct CombinedWidthProbeModeGuard;

impl CombinedWidthProbeModeGuard {
    fn new(mode: crate::shape::CombinedWidthProbeMode) -> Self {
        crate::shape::set_combined_width_probe_mode(mode);
        Self
    }
}

impl Drop for CombinedWidthProbeModeGuard {
    fn drop(&mut self) {
        crate::shape::set_combined_width_probe_mode(
            crate::shape::CombinedWidthProbeMode::ScopedReuse,
        );
    }
}

fn width_probe_fonts(limits: &Limits) -> FontCollection {
    let fonts = FontCollection::with_options(
        limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        ("Width CJK", crate::test_support::fonts::CJK),
        ("Width Latin", crate::test_support::fonts::LATIN),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fonts
}

fn width_probe_style(
    family: &str,
    direction: crate::geometry::Direction,
) -> crate::style::InlineStyle {
    crate::style::InlineStyle {
        font_families: vec![crate::style::FontFamily::Named(family.into())],
        direction,
        text_combine_upright: crate::style::TextCombineUpright::All,
        text_autospace: crate::style::TextAutospace::NoAutospace,
        ..Default::default()
    }
}

fn width_probe_builder(
    case: WidthProbeCase,
    base_glyph_limit: Option<u64>,
    base_shaping_run_bytes: Option<u64>,
    repeats: usize,
    limits: &Limits,
    first_line: bool,
) -> crate::ParagraphBuilder {
    let base_limits = Limits {
        max_shaped_glyphs: base_glyph_limit,
        max_shaping_run_bytes: base_shaping_run_bytes,
        ..Limits::default()
    };
    let cjk_style = width_probe_style("Width CJK", case.direction);
    let latin_style = crate::style::InlineStyle {
        font_size: 19.0,
        ..width_probe_style("Width Latin", case.direction)
    };
    let mut plain_latin_style = latin_style.clone();
    plain_latin_style.text_combine_upright = crate::style::TextCombineUpright::None;
    let mut base_builder = crate::ParagraphBuilder::new(
        &crate::style::ParagraphStyle {
            writing_mode: case.mode,
            direction: case.direction,
            root: cjk_style.clone(),
            ..Default::default()
        },
        &base_limits,
    );
    for repeat in 0..repeats {
        if repeat > 0 {
            let separator = crate::node::NodeId(3000 + repeat as u64);
            base_builder
                .open_inline(
                    separator,
                    &plain_latin_style,
                    crate::node::InlineEdges::default(),
                )
                .push_text(
                    crate::node::TextSource::Dom {
                        node: separator,
                        offset: 0,
                    },
                    "x",
                )
                .close_inline();
        }
        let wrapper = crate::node::NodeId(100 + repeat as u64);
        let first = crate::node::NodeId(1000 + repeat as u64);
        base_builder
            .open_inline(wrapper, &cjk_style, crate::node::InlineEdges::default())
            .push_text(
                crate::node::TextSource::Dom {
                    node: first,
                    offset: 10,
                },
                case.text,
            )
            .close_inline();
    }
    if case.multiple_styles {
        let node = crate::node::NodeId(9000);
        base_builder
            .open_inline(
                node,
                &plain_latin_style,
                crate::node::InlineEdges::default(),
            )
            .push_text(crate::node::TextSource::Dom { node, offset: 0 }, "x")
            .close_inline();
    }
    let base = crate::RubyContent::from_builder(base_builder);
    let annotation_style = width_probe_style("Width CJK", case.direction);
    let annotation = crate::RubyContent::text(
        crate::node::TextSource::Dom {
            node: crate::node::NodeId(3),
            offset: 30,
        },
        "注",
        &annotation_style,
        limits,
    );
    let ruby = crate::Ruby::new(
        vec![crate::RubyBase {
            node: crate::node::NodeId(2),
            content: base,
            align: crate::RubyAlign::default(),
        }],
        vec![crate::RubyLevel {
            annotations: vec![crate::RubyAnnotation {
                node: crate::node::NodeId(3),
                content: annotation,
                span: crate::RubySpan::Auto,
                visibility: crate::RubyVisibility::Visible,
            }],
            style: crate::RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = crate::ParagraphBuilder::new(
        &crate::style::ParagraphStyle {
            writing_mode: case.mode,
            direction: case.direction,
            root: cjk_style.clone(),
            first_line: first_line.then(|| crate::style::InlineStyle {
                font_size: 24.0,
                ..cjk_style.clone()
            }),
            ..Default::default()
        },
        limits,
    );
    builder.push_ruby(crate::node::NodeId(1), &cjk_style, ruby);
    builder
}

fn build_width_probe_case(
    case: WidthProbeCase,
    mode: crate::shape::CombinedWidthProbeMode,
    base_glyph_limit: Option<u64>,
    base_shaping_run_bytes: Option<u64>,
    repeats: usize,
) -> Result<crate::Paragraph, LimitExceeded> {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    let builder = width_probe_builder(
        case,
        base_glyph_limit,
        base_shaping_run_bytes,
        repeats,
        &limits,
        false,
    );
    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.set(0));
    COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| count.set(0));
    COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.set(0));
    COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.set(0));
    let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
    builder.build(&mut crate::LayoutContext::new(), &fonts)
}

fn build_width_probe_with(
    case: WidthProbeCase,
    mode: crate::shape::CombinedWidthProbeMode,
    base_glyph_limit: Option<u64>,
    base_shaping_run_bytes: Option<u64>,
    paragraph: &Limits,
    first_line: bool,
    repeats: usize,
) -> Result<crate::Paragraph, LimitExceeded> {
    let fonts = width_probe_fonts(paragraph);
    let builder = width_probe_builder(
        case,
        base_glyph_limit,
        base_shaping_run_bytes,
        repeats,
        paragraph,
        first_line,
    );
    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
    builder.build(&mut crate::LayoutContext::new(), &fonts)
}

fn width_probe_outcome(
    result: Result<crate::Paragraph, LimitExceeded>,
) -> Result<String, (LimitKind, u64, u64)> {
    result
        .map(|paragraph| width_probe_snapshot(&paragraph))
        .map_err(|error| (error.kind, error.limit, error.actual))
}

fn width_probe_snapshot(paragraph: &crate::Paragraph) -> String {
    let glyphs = &paragraph.data.glyphs;
    let geometry = &paragraph.data.combine_geometry;
    let shape_items = paragraph
        .data
        .shape_items
        .iter()
        .map(|item| {
            (
                item.segment,
                item.scalars
                    .iter()
                    .map(|scalar| {
                        (
                            scalar.c,
                            scalar.offset,
                            scalar.end,
                            scalar.item,
                            scalar.grapheme_start,
                        )
                    })
                    .collect::<Vec<_>>(),
                item.end,
                item.style,
                item.level,
                item.script,
                item.font
                    .as_ref()
                    .map(|font| (font.id.index(), &font.variations, font.embolden, font.skew)),
                item.orientation,
                item.combine,
                item.width_feature,
                &item.before,
                &item.after,
            )
        })
        .collect::<Vec<_>>();
    let runs = paragraph
        .data
        .runs
        .iter()
        .map(|run| {
            (
                run.glyphs.clone(),
                run.text.clone(),
                run.item,
                run.orientation,
                run.font.index(),
                run.font_size.to_bits(),
                format!("{:?}", run.instance),
            )
        })
        .collect::<Vec<_>>();
    let paints = geometry
        .glyphs
        .iter()
        .map(|paint| paint.map(|p| (p.span, p.x, p.from, p.to, p.extra)))
        .collect::<Vec<_>>();
    let tabs = geometry
        .tabs
        .iter()
        .map(|(range, p)| (range.clone(), (p.span, p.x, p.from, p.to, p.extra)))
        .collect::<Vec<_>>();
    let source_spans = paragraph
        .data
        .source_spans
        .iter()
        .map(|span| (span.old.clone(), span.new.clone(), span.kind))
        .collect::<Vec<_>>();
    format!(
        "{:?}",
        (
            paragraph.text(),
            paragraph.offset_mapping(),
            &paragraph.warnings(),
            shape_items,
            &paragraph.data.items,
            &paragraph.data.units,
            &paragraph.data.boxes,
            &paragraph.data.combine_spans,
            source_spans,
            (
                &glyphs.id,
                &glyphs.advance,
                &glyphs.pen,
                &glyphs.offset_inline,
                &glyphs.offset_block,
                &glyphs.cluster,
                &glyphs.flags,
                &glyphs.spacing,
                &glyphs.leading,
            ),
            runs,
            (&geometry.scales, &geometry.baselines, paints, tabs,),
        )
    )
}

#[test]
fn combined_width_trials_reuse_selected_result_for_two_shaper_calls() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
            text_combine_upright: crate::style::TextCombineUpright::All,
            text_autospace: crate::style::TextAutospace::NoAutospace,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "12",
    );

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.set(0));
    COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.set(0));
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();

    assert!(
        paragraph
            .data
            .shape_items
            .iter()
            .all(|item| item.width_feature == Some(*b"hwid"))
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 2);
    assert_eq!(
        COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get()),
        0
    );
    assert!(COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.get()) > 0);
}

#[test]
fn combined_width_reuse_preserves_global_glyph_limit_failure() {
    let limits = Limits {
        max_shaped_glyphs: Some(3),
        ..Limits::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = crate::style::InlineStyle {
        font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
        ..Default::default()
    };
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: root.clone(),
        ..Default::default()
    };
    let mut combined = root;
    combined.text_combine_upright = crate::style::TextCombineUpright::All;
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    for node in [1, 2] {
        let node = crate::node::NodeId(node);
        builder
            .open_inline(node, &combined, crate::node::InlineEdges::default())
            .push_text(crate::node::TextSource::Generated { node }, "12")
            .close_inline();
    }

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let error = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap_err();
    assert_eq!(
        (error.kind, error.limit, error.actual),
        (LimitKind::ShapedGlyphs, 3, 4)
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 5);
}

#[test]
fn ruby_base_glyph_limits_fall_back_to_the_scoped_final_shape() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let base_style = crate::style::InlineStyle {
        font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
        text_combine_upright: crate::style::TextCombineUpright::All,
        ..Default::default()
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: base_style.clone(),
        ..Default::default()
    };
    let base = crate::RubyContent::text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(2),
        },
        "12",
        &base_style,
        &Limits {
            max_shaped_glyphs: Some(1),
            ..Limits::default()
        },
    );
    let annotation = crate::RubyContent::text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(3),
        },
        "あ",
        &base_style,
        &limits,
    );
    let ruby = crate::Ruby::new(
        vec![crate::RubyBase {
            node: crate::node::NodeId(2),
            content: base,
            align: crate::RubyAlign::default(),
        }],
        vec![crate::RubyLevel {
            annotations: vec![crate::RubyAnnotation {
                node: crate::node::NodeId(3),
                content: annotation,
                span: crate::RubySpan::Auto,
                visibility: crate::RubyVisibility::Visible,
            }],
            style: crate::RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = crate::ParagraphBuilder::new(&paragraph_style, &limits);
    builder.push_ruby(crate::node::NodeId(1), &base_style, ruby);

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let error = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap_err();
    assert_eq!((error.kind, error.limit), (LimitKind::ShapedGlyphs, 1));
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 3);
}

#[test]
fn ruby_base_combined_width_probe_borrows_the_feature_override_view() {
    let paragraph = build_width_probe_case(
        WIDTH_PROBE_CASES[0],
        crate::shape::CombinedWidthProbeMode::FeatureView,
        None,
        None,
        1,
    )
    .unwrap();

    assert!(
        paragraph
            .data
            .shape_items
            .iter()
            .all(|item| item.width_feature == Some(*b"hwid"))
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 3);
    assert_eq!(
        COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.get()),
        0
    );
    assert_eq!(
        COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get()),
        0,
        "probe should borrow ShapeItem metadata while overriding only the width feature"
    );
}

#[test]
fn feature_override_view_matches_clone_reference_for_scoped_tcy_cases() {
    for case in WIDTH_PROBE_CASES {
        let reference = build_width_probe_case(
            case,
            crate::shape::CombinedWidthProbeMode::CloneReference,
            None,
            None,
            1,
        )
        .unwrap_or_else(|error| panic!("{} clone reference: {error:?}", case.name));
        let reference_calls = HARFRUST_SHAPE_CALLS.with(|calls| calls.get());
        let reference_clone_count = COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.get());
        let reference_clone_scalars = COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| count.get());
        let reference_clone_bytes = COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get());
        let reference_trial_bytes = COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.get());
        let selected_features: Vec<_> = reference
            .data
            .shape_items
            .iter()
            .filter(|item| item.combine.is_some())
            .map(|item| item.width_feature)
            .collect();
        assert!(!selected_features.is_empty(), "no TCY items: {}", case.name);
        assert!(
            selected_features
                .iter()
                .all(|feature| *feature == case.expected_width_feature),
            "width feature selection: {}",
            case.name
        );
        let view = build_width_probe_case(
            case,
            crate::shape::CombinedWidthProbeMode::FeatureView,
            None,
            None,
            1,
        )
        .unwrap_or_else(|error| panic!("{} feature view: {error:?}", case.name));

        assert_eq!(
            width_probe_snapshot(&reference),
            width_probe_snapshot(&view),
            "mapping, warning, glyph, source and geometry output: {}",
            case.name
        );
        assert!(reference_clone_count > 0, "no probe group: {}", case.name);
        assert!(
            reference_clone_scalars > 0,
            "no scalar clone: {}",
            case.name
        );
        assert!(
            reference_clone_bytes > 0,
            "no cloned storage: {}",
            case.name
        );
        assert_eq!(
            HARFRUST_SHAPE_CALLS.with(|calls| calls.get()),
            reference_calls
        );
        assert_eq!(
            COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.get()),
            0,
            "feature view cloned a group: {}",
            case.name
        );
        assert_eq!(
            COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| count.get()),
            0,
            "feature view cloned scalars: {}",
            case.name
        );
        assert_eq!(
            COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get()),
            0,
            "feature view cloned storage: {}",
            case.name
        );
        assert_eq!(
            COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.get()),
            reference_trial_bytes,
            "trial retention changed: {}",
            case.name
        );
    }
}

#[test]
fn feature_override_view_preserves_scoped_glyph_limit_failure() {
    let case = WIDTH_PROBE_CASES[0];
    let mut outcomes = Vec::new();
    for mode in [
        crate::shape::CombinedWidthProbeMode::CloneReference,
        crate::shape::CombinedWidthProbeMode::FeatureView,
    ] {
        let error = build_width_probe_case(case, mode, Some(1), None, 1).unwrap_err();
        assert_eq!(error.kind, LimitKind::ShapedGlyphs);
        outcomes.push((
            error.kind,
            error.limit,
            error.actual,
            HARFRUST_SHAPE_CALLS.with(|calls| calls.get()),
        ));
        assert_eq!(
            crate::shape::combined_width_probe_mode_for_test(),
            crate::shape::CombinedWidthProbeMode::ScopedReuse,
            "probe mode must not leak into tests reusing this worker thread"
        );
    }
    assert_eq!(outcomes[0], outcomes[1]);
}

#[test]
fn feature_override_view_preserves_scoped_shaping_warnings() {
    let case = WIDTH_PROBE_CASES[0];
    let reference = build_width_probe_case(
        case,
        crate::shape::CombinedWidthProbeMode::CloneReference,
        None,
        Some(0),
        1,
    )
    .unwrap();
    assert!(
        !reference.warnings().is_empty(),
        "the scoped shaping-run limit should exercise warning output"
    );
    let view = build_width_probe_case(
        case,
        crate::shape::CombinedWidthProbeMode::FeatureView,
        None,
        Some(0),
        1,
    )
    .unwrap();
    assert_eq!(
        width_probe_snapshot(&reference),
        width_probe_snapshot(&view)
    );
}

#[test]
fn scoped_output_reuse_saves_final_shapes_and_keeps_local_glyph_caps() {
    let limits = Limits::default();
    for case in WIDTH_PROBE_CASES {
        let reference = build_width_probe_with(
            case,
            crate::shape::CombinedWidthProbeMode::CloneReference,
            None,
            None,
            &limits,
            false,
            2,
        )
        .unwrap();
        let reference_calls = HARFRUST_SHAPE_CALLS.with(|calls| calls.get());
        let reused = build_width_probe_with(
            case,
            crate::shape::CombinedWidthProbeMode::ScopedReuse,
            None,
            None,
            &limits,
            false,
            2,
        )
        .unwrap();
        let reused_calls = HARFRUST_SHAPE_CALLS.with(|calls| calls.get());
        assert_eq!(
            width_probe_snapshot(&reference),
            width_probe_snapshot(&reused),
            "{}",
            case.name
        );
        assert!(
            reused_calls < reference_calls,
            "{}: {reused_calls} >= {reference_calls}",
            case.name
        );
    }
    let case = WIDTH_PROBE_CASES[0];
    let reference_calls = {
        build_width_probe_with(
            case,
            crate::shape::CombinedWidthProbeMode::CloneReference,
            None,
            None,
            &limits,
            false,
            1,
        )
        .unwrap();
        HARFRUST_SHAPE_CALLS.with(|calls| calls.get())
    };
    build_width_probe_with(
        case,
        crate::shape::CombinedWidthProbeMode::ScopedReuse,
        None,
        None,
        &limits,
        false,
        1,
    )
    .unwrap();
    assert_eq!(
        HARFRUST_SHAPE_CALLS.with(|calls| calls.get()) + 1,
        reference_calls
    );

    for mode in [
        crate::shape::CombinedWidthProbeMode::CloneReference,
        crate::shape::CombinedWidthProbeMode::ScopedReuse,
    ] {
        let error =
            build_width_probe_with(case, mode, Some(1), None, &limits, false, 1).unwrap_err();
        assert_eq!(
            (error.kind, error.limit),
            (LimitKind::ShapedGlyphs, 1),
            "{mode:?}"
        );
    }
}

#[test]
fn scoped_output_reuse_matches_reference_across_base_and_paragraph_limits() {
    for case in WIDTH_PROBE_CASES {
        for repeats in [1, 2] {
            for base_glyphs in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(6)] {
                for base_run_bytes in [None, Some(0), Some(1), Some(2), Some(4)] {
                    for paragraph_glyphs in [None, Some(1), Some(3), Some(5)] {
                        let paragraph = Limits {
                            max_shaped_glyphs: paragraph_glyphs,
                            ..Limits::default()
                        };
                        let run = |mode| {
                            width_probe_outcome(build_width_probe_with(
                                case,
                                mode,
                                base_glyphs,
                                base_run_bytes,
                                &paragraph,
                                false,
                                repeats,
                            ))
                        };
                        assert_eq!(
                            run(crate::shape::CombinedWidthProbeMode::CloneReference),
                            run(crate::shape::CombinedWidthProbeMode::ScopedReuse),
                            "{} repeats={repeats} base_glyphs={base_glyphs:?} base_run_bytes={base_run_bytes:?} paragraph_glyphs={paragraph_glyphs:?}",
                            case.name
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn scoped_output_reuse_matches_reference_for_first_line_cumulative_base_glyphs() {
    let mut outcomes = Vec::new();
    for case in WIDTH_PROBE_CASES {
        for repeats in [1, 2] {
            for base_glyphs in [
                None,
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(8),
            ] {
                for paragraph_glyphs in [None, Some(2), Some(4), Some(6), Some(8)] {
                    let paragraph = Limits {
                        max_shaped_glyphs: paragraph_glyphs,
                        ..Limits::default()
                    };
                    let run = |mode| {
                        width_probe_outcome(build_width_probe_with(
                            case,
                            mode,
                            base_glyphs,
                            None,
                            &paragraph,
                            true,
                            repeats,
                        ))
                    };
                    let reference = run(crate::shape::CombinedWidthProbeMode::CloneReference);
                    assert_eq!(
                        reference,
                        run(crate::shape::CombinedWidthProbeMode::ScopedReuse),
                        "{} repeats={repeats} base_glyphs={base_glyphs:?} paragraph_glyphs={paragraph_glyphs:?}",
                        case.name
                    );
                    outcomes.push(reference.is_ok());
                }
            }
        }
    }
    assert!(outcomes.contains(&true) && outcomes.contains(&false));
}

#[test]
fn scoped_output_reuse_matches_reference_under_exhausted_warning_caps() {
    for case in WIDTH_PROBE_CASES {
        for max_warnings in [Some(0), Some(1), Some(2)] {
            for base_run_bytes in [None, Some(0), Some(1)] {
                let paragraph = Limits {
                    max_warnings,
                    max_shaping_run_bytes: Some(1),
                    ..Limits::default()
                };
                let run = |mode| {
                    width_probe_outcome(build_width_probe_with(
                        case,
                        mode,
                        None,
                        base_run_bytes,
                        &paragraph,
                        false,
                        2,
                    ))
                };
                assert_eq!(
                    run(crate::shape::CombinedWidthProbeMode::CloneReference),
                    run(crate::shape::CombinedWidthProbeMode::ScopedReuse),
                    "{} max_warnings={max_warnings:?} base_run_bytes={base_run_bytes:?}",
                    case.name
                );
            }
        }
    }
}

/// Builds `<ruby><rb outer>34<ruby><rb inner>CONTENT</rb>..</ruby></rb>..</ruby>`
/// where `fill_inner` pushes the content of the innermost base.
fn nested_width_probe_builder_with(
    outer_glyphs: Option<u64>,
    inner_glyphs: Option<u64>,
    limits: &Limits,
    fill_inner: impl FnOnce(&mut crate::ParagraphBuilder, &crate::style::InlineStyle),
) -> crate::ParagraphBuilder {
    let style = width_probe_style("Width CJK", crate::geometry::Direction::Ltr);
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: style.clone(),
        ..Default::default()
    };
    let ruby = |node: u64, content: crate::RubyContent| {
        crate::Ruby::new(
            vec![crate::RubyBase {
                node: crate::node::NodeId(node),
                content,
                align: crate::RubyAlign::default(),
            }],
            vec![crate::RubyLevel {
                annotations: vec![crate::RubyAnnotation {
                    node: crate::node::NodeId(node + 1),
                    content: crate::RubyContent::text(
                        crate::node::TextSource::Generated {
                            node: crate::node::NodeId(node + 1),
                        },
                        "注",
                        &style,
                        limits,
                    ),
                    span: crate::RubySpan::Auto,
                    visibility: crate::RubyVisibility::Visible,
                }],
                style: crate::RubyStyle::default(),
            }],
        )
        .unwrap()
    };
    let inner_limits = Limits {
        max_shaped_glyphs: inner_glyphs,
        ..Limits::default()
    };
    let mut inner = crate::ParagraphBuilder::new(&paragraph_style, &inner_limits);
    fill_inner(&mut inner, &style);
    let outer_limits = Limits {
        max_shaped_glyphs: outer_glyphs,
        ..Limits::default()
    };
    let mut outer = crate::ParagraphBuilder::new(&paragraph_style, &outer_limits);
    outer.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(10),
        },
        "34",
    );
    outer.push_ruby(
        crate::node::NodeId(11),
        &style,
        ruby(12, crate::RubyContent::from_builder(inner)),
    );
    let mut builder = crate::ParagraphBuilder::new(&paragraph_style, limits);
    builder.push_ruby(
        crate::node::NodeId(1),
        &style,
        ruby(2, crate::RubyContent::from_builder(outer)),
    );
    builder
}

fn nested_width_probe_builder(
    outer_glyphs: Option<u64>,
    inner_glyphs: Option<u64>,
    limits: &Limits,
) -> crate::ParagraphBuilder {
    nested_width_probe_builder_with(outer_glyphs, inner_glyphs, limits, |inner, style| {
        // A leading non-TCY "x" puts the inner group after other base content;
        // `base_leading_width_probe_builder` covers a group at the base start.
        let plain = crate::style::InlineStyle {
            text_combine_upright: crate::style::TextCombineUpright::None,
            ..style.clone()
        };
        inner
            .open_inline(
                crate::node::NodeId(19),
                &plain,
                crate::node::InlineEdges::default(),
            )
            .push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(19),
                    offset: 0,
                },
                "x",
            )
            .close_inline();
        inner.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(20),
            },
            "12",
        );
    })
}

/// One text-combine-upright element whose text needs two fonts (Latin digit, then a
/// CJK-only scalar), so the single combine group produces several shape inputs and
/// several shaper charges.
fn nested_multi_input_width_probe_builder(
    outer_glyphs: Option<u64>,
    inner_glyphs: Option<u64>,
    limits: &Limits,
) -> crate::ParagraphBuilder {
    nested_width_probe_builder_with(outer_glyphs, inner_glyphs, limits, |inner, cjk| {
        let mixed = crate::style::InlineStyle {
            font_families: vec![
                crate::style::FontFamily::Named("Width Latin".into()),
                crate::style::FontFamily::Named("Width CJK".into()),
            ],
            ..cjk.clone()
        };
        // A leading plain "x" spends inner-base glyphs before the group, which the
        // cumulative glyph-limit sweep below relies on.
        let plain = crate::style::InlineStyle {
            text_combine_upright: crate::style::TextCombineUpright::None,
            ..cjk.clone()
        };
        inner
            .open_inline(
                crate::node::NodeId(20),
                &plain,
                crate::node::InlineEdges::default(),
            )
            .push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(20),
                    offset: 0,
                },
                "x",
            )
            .close_inline()
            .open_inline(
                crate::node::NodeId(21),
                &mixed,
                crate::node::InlineEdges::default(),
            )
            .push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(21),
                    offset: 0,
                },
                "1日",
            )
            .close_inline();
    })
}

/// The inner base starts with its combine group, right after the isolate controls
/// of both ruby boundaries.
fn base_leading_width_probe_builder(
    outer_glyphs: Option<u64>,
    inner_glyphs: Option<u64>,
    limits: &Limits,
) -> crate::ParagraphBuilder {
    nested_width_probe_builder_with(outer_glyphs, inner_glyphs, limits, |inner, _| {
        inner.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(20),
            },
            "12",
        );
    })
}

#[test]
fn base_leading_combined_group_selects_a_width_feature() {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    let build = |outer, inner, mode| {
        let builder = base_leading_width_probe_builder(outer, inner, &limits);
        let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
        builder.build(&mut crate::LayoutContext::new(), &fonts)
    };
    let mut snapshots = Vec::new();
    for mode in [
        crate::shape::CombinedWidthProbeMode::CloneReference,
        crate::shape::CombinedWidthProbeMode::ScopedReuse,
    ] {
        let paragraph = build(None, None, mode).unwrap();
        let group: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .filter(|item| item.scalars.iter().any(|s| matches!(s.c, '1' | '2')))
            .map(|item| {
                (
                    item.scalars
                        .iter()
                        .map(|s| (s.c, s.grapheme_start))
                        .collect::<Vec<_>>(),
                    item.width_feature,
                )
            })
            .collect();
        assert_eq!(
            group,
            [(vec![('1', true), ('2', true)], Some(*b"hwid"))],
            "{mode:?}"
        );
        snapshots.push(width_probe_snapshot(&paragraph));
    }
    assert_eq!(snapshots[0], snapshots[1]);

    let mut outcomes = Vec::new();
    for outer in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(6)] {
        for inner in [None, Some(0), Some(1), Some(2), Some(4)] {
            let reference = width_probe_outcome(build(
                outer,
                inner,
                crate::shape::CombinedWidthProbeMode::CloneReference,
            ));
            assert_eq!(
                reference,
                width_probe_outcome(build(
                    outer,
                    inner,
                    crate::shape::CombinedWidthProbeMode::ScopedReuse
                )),
                "outer={outer:?} inner={inner:?}"
            );
            outcomes.push(reference.is_ok());
        }
    }
    assert!(outcomes.contains(&true) && outcomes.contains(&false));
}

/// Ruby-free: a combine group right after an out-of-flow placeholder.
#[test]
fn combined_group_after_out_of_flow_selects_a_width_feature() {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    let combined = width_probe_style("Width CJK", crate::geometry::Direction::Ltr);
    let plain = crate::style::InlineStyle {
        text_combine_upright: crate::style::TextCombineUpright::None,
        ..combined.clone()
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: plain,
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&paragraph_style, &limits);
    builder
        .push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "あ",
        )
        .push_out_of_flow(crate::node::NodeId(2), crate::node::OutOfFlowKind::Absolute)
        .open_inline(
            crate::node::NodeId(3),
            &combined,
            crate::node::InlineEdges::default(),
        )
        .push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(4),
            },
            "12",
        )
        .close_inline();
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let group: Vec<_> = paragraph
        .data
        .shape_items
        .iter()
        .filter(|item| item.combine.is_some())
        .map(|item| item.width_feature)
        .collect();
    assert_eq!(group, [Some(*b"hwid")]);
}

/// A grapheme right after a transparent gap is a legal run-byte split point, so
/// the budget never has to split inside a grapheme there.
/// An authored bidi control is its own shaping cluster even though it starts no
/// paragraph grapheme, so the run-byte budget may still split right before it.
#[test]
fn run_byte_budget_splits_before_an_authored_bidi_control() {
    let limits = Limits {
        max_shaping_run_bytes: Some(2),
        ..Limits::default()
    };
    let fonts = width_probe_fonts(&limits);
    let style = crate::style::ParagraphStyle {
        root: width_probe_style("Width Latin", crate::geometry::Direction::Ltr),
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "ab\u{200e}",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.shape_items.len(), 1);
    let flags: Vec<_> = paragraph.data.shape_items[0]
        .scalars
        .iter()
        .map(|s| (s.c, s.grapheme_start))
        .collect();
    assert_eq!(flags, [('a', true), ('b', true), ('\u{200e}', false)]);
    assert!(
        paragraph
            .warnings()
            .iter()
            .all(|warning| !warning.message.contains("giant grapheme")),
        "{:?}",
        paragraph.warnings()
    );
}

#[test]
fn run_byte_budget_splits_after_a_transparent_gap() {
    let limits = Limits {
        max_shaping_run_bytes: Some(1),
        ..Limits::default()
    };
    let fonts = width_probe_fonts(&limits);
    let style = crate::style::ParagraphStyle {
        root: width_probe_style("Width Latin", crate::geometry::Direction::Ltr),
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder
        .push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        )
        .push_out_of_flow(crate::node::NodeId(2), crate::node::OutOfFlowKind::Absolute)
        .push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(3),
            },
            "b",
        );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.shape_items.len(), 1);
    assert!(
        paragraph
            .warnings()
            .iter()
            .all(|warning| !warning.message.contains("giant grapheme")),
        "{:?}",
        paragraph.warnings()
    );
}

#[test]
fn scoped_output_reuse_matches_reference_for_nested_base_scopes() {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    // Both the outer "34" group and the inner "12" group must be width-probed, so
    // reuse saves a final shape for each of them.
    let calls = |mode| {
        let builder = nested_width_probe_builder(None, None, &limits);
        HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
        let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        (paragraph, HARFRUST_SHAPE_CALLS.with(|calls| calls.get()))
    };
    let (reference_paragraph, reference_calls) =
        calls(crate::shape::CombinedWidthProbeMode::CloneReference);
    let (reused_paragraph, reused_calls) = calls(crate::shape::CombinedWidthProbeMode::ScopedReuse);
    assert_eq!(
        width_probe_snapshot(&reference_paragraph),
        width_probe_snapshot(&reused_paragraph)
    );
    let mut combine_ids: Vec<u32> = reused_paragraph
        .data
        .shape_items
        .iter()
        .filter_map(|item| item.combine)
        .collect();
    combine_ids.dedup();
    assert!(combine_ids.len() >= 2, "{combine_ids:?}");
    assert!(
        reference_calls >= reused_calls + 2,
        "reference={reference_calls} reused={reused_calls}"
    );

    let mut outcomes = Vec::new();
    for outer in [None, Some(0), Some(1), Some(2), Some(3), Some(4), Some(6)] {
        for inner in [None, Some(0), Some(1), Some(2), Some(4)] {
            let run = |mode| {
                let builder = nested_width_probe_builder(outer, inner, &limits);
                let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
                width_probe_outcome(builder.build(&mut crate::LayoutContext::new(), &fonts))
            };
            let reference = run(crate::shape::CombinedWidthProbeMode::CloneReference);
            assert_eq!(
                reference,
                run(crate::shape::CombinedWidthProbeMode::ScopedReuse),
                "outer={outer:?} inner={inner:?}"
            );
            outcomes.push(reference.is_ok());
        }
    }
    assert!(outcomes.contains(&true) && outcomes.contains(&false));
}

#[test]
fn scoped_output_reuse_matches_reference_for_multi_input_groups_in_nested_bases() {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    let build = |outer, inner, mode| {
        let builder = nested_multi_input_width_probe_builder(outer, inner, &limits);
        HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
        let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
        builder.build(&mut crate::LayoutContext::new(), &fonts)
    };

    // The combine group must span several shape inputs, and reuse must save several calls.
    let reference_paragraph = build(
        None,
        None,
        crate::shape::CombinedWidthProbeMode::CloneReference,
    )
    .unwrap();
    let reference_calls = HARFRUST_SHAPE_CALLS.with(|calls| calls.get());
    let reused_paragraph = build(
        None,
        None,
        crate::shape::CombinedWidthProbeMode::ScopedReuse,
    )
    .unwrap();
    let reused_calls = HARFRUST_SHAPE_CALLS.with(|calls| calls.get());
    assert_eq!(
        width_probe_snapshot(&reference_paragraph),
        width_probe_snapshot(&reused_paragraph)
    );
    let data = &reused_paragraph.data;
    // The probed group is the combine id that owns several shape items; "34" in the
    // outer base is a separate single-item group.
    // Its id is derived from the item holding the leading "1" rather than hard-coded.
    let group_id = data
        .shape_items
        .iter()
        .find(|item| item.scalars.first().is_some_and(|scalar| scalar.c == '1'))
        .and_then(|item| item.combine)
        .expect("the '1' item belongs to a combine group");
    let group_items: Vec<usize> = data
        .shape_items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.combine == Some(group_id) && item.font.is_some())
        .map(|(index, _)| index)
        .collect();
    // Glyphs per shape item: runs whose source text covers the item's scalars.
    let group_glyphs: Vec<u64> = group_items
        .iter()
        .map(|&index| {
            let first = data.shape_items[index].scalars[0].offset;
            data.runs
                .iter()
                .filter(|run| run.text.start <= first && first < run.text.end)
                .map(|run| run.glyphs.len() as u64)
                .sum()
        })
        .collect();
    assert!(group_items.len() >= 2, "{group_items:?}");
    assert!(
        reference_calls >= reused_calls + 2,
        "reference={reference_calls} reused={reused_calls}"
    );
    // The inner base spends `prefix` glyphs on its leading plain "x" before the group,
    // so a limit in [prefix + largest, prefix + total) admits each charge on its own
    // but not their cumulative sum. The sweep proves the reused path reaches and
    // reproduces that cumulative failure boundary. It cannot tell a cumulative
    // preflight from a per-charge one, because the commit replay fails identically;
    // the preflight's cumulative semantics are pinned by the unit tests in
    // ruby/base_budget.rs (`shaped_glyph_preflight_accumulates_shared_ancestor_charges`).
    let prefix: u64 = data
        .shape_items
        .iter()
        .filter(|item| item.scalars.first().is_some_and(|scalar| scalar.c == 'x'))
        .map(|item| {
            let first = item.scalars[0].offset;
            data.runs
                .iter()
                .filter(|run| run.text.start <= first && first < run.text.end)
                .map(|run| run.glyphs.len() as u64)
                .sum::<u64>()
        })
        .sum();
    let group_total: u64 = group_glyphs.iter().sum();
    let largest = *group_glyphs.iter().max().unwrap();
    assert!(prefix > 0 && group_total > largest);

    let values = [
        None,
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(4),
        Some(5),
        Some(6),
    ];
    let mut ok = 0;
    let mut err = 0;
    let mut cumulative_boundary = false;
    for outer in values {
        for inner in values {
            let reference = width_probe_outcome(build(
                outer,
                inner,
                crate::shape::CombinedWidthProbeMode::CloneReference,
            ));
            assert_eq!(
                reference,
                width_probe_outcome(build(
                    outer,
                    inner,
                    crate::shape::CombinedWidthProbeMode::ScopedReuse
                )),
                "outer={outer:?} inner={inner:?}"
            );
            match &reference {
                Ok(_) => ok += 1,
                Err((kind, limit, _)) => {
                    err += 1;
                    if *kind == LimitKind::ShapedGlyphs
                        && *limit >= prefix + largest
                        && *limit < prefix + group_total
                    {
                        cumulative_boundary = true;
                    }
                }
            }
        }
    }
    assert!(ok > 0 && err > 0);
    assert!(cumulative_boundary);
}

/// `depth` nested ruby bases (the outermost with `outer_glyphs`, the innermost with
/// `inner_glyphs`); the innermost base holds `groups` text-combine-upright "1日2日"
/// elements, each preceded by a plain "x". Latin lacks "日", so every group spans
/// four shape inputs and four shaper charges owned by the deepest scope.
fn deep_nested_multi_input_builder(
    depth: usize,
    groups: usize,
    outer_glyphs: Option<u64>,
    inner_glyphs: Option<u64>,
    limits: &Limits,
) -> crate::ParagraphBuilder {
    assert!(depth >= 2);
    let cjk = width_probe_style("Width CJK", crate::geometry::Direction::Ltr);
    let mixed = crate::style::InlineStyle {
        font_families: vec![
            crate::style::FontFamily::Named("Width Latin".into()),
            crate::style::FontFamily::Named("Width CJK".into()),
        ],
        ..cjk.clone()
    };
    let plain = crate::style::InlineStyle {
        text_combine_upright: crate::style::TextCombineUpright::None,
        ..cjk.clone()
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: cjk.clone(),
        ..Default::default()
    };
    let base_limits = |glyphs| Limits {
        max_shaped_glyphs: glyphs,
        ..Limits::default()
    };
    let mut content = crate::ParagraphBuilder::new(&paragraph_style, &base_limits(inner_glyphs));
    for group in 0..groups as u64 {
        let node = crate::node::NodeId(100_000 + 2 * group);
        let text = crate::node::NodeId(100_001 + 2 * group);
        content
            .open_inline(node, &plain, crate::node::InlineEdges::default())
            .push_text(crate::node::TextSource::Dom { node, offset: 0 }, "x")
            .close_inline()
            .open_inline(text, &mixed, crate::node::InlineEdges::default())
            .push_text(
                crate::node::TextSource::Dom {
                    node: text,
                    offset: 0,
                },
                "1日2日",
            )
            .close_inline();
    }
    for level in (0..depth as u64).rev() {
        let node = crate::node::NodeId(10 * level + 1);
        let ruby = crate::Ruby::new(
            vec![crate::RubyBase {
                node: crate::node::NodeId(10 * level + 2),
                content: crate::RubyContent::from_builder(content),
                align: crate::RubyAlign::default(),
            }],
            vec![crate::RubyLevel {
                annotations: vec![crate::RubyAnnotation {
                    node: crate::node::NodeId(10 * level + 3),
                    content: crate::RubyContent::text(
                        crate::node::TextSource::Generated {
                            node: crate::node::NodeId(10 * level + 3),
                        },
                        "注",
                        &cjk,
                        limits,
                    ),
                    span: crate::RubySpan::Auto,
                    visibility: crate::RubyVisibility::Visible,
                }],
                style: crate::RubyStyle::default(),
            }],
        )
        .unwrap();
        content = match level {
            0 => crate::ParagraphBuilder::new(&paragraph_style, limits),
            1 => crate::ParagraphBuilder::new(&paragraph_style, &base_limits(outer_glyphs)),
            _ => crate::ParagraphBuilder::new(&paragraph_style, &Limits::default()),
        };
        content.push_ruby(node, &cjk, ruby);
    }
    content
}

#[test]
fn scoped_output_reuse_preflight_stays_linear_in_deep_nested_bases() {
    const DEPTH: usize = 32;
    const GROUPS: usize = 6;
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    let build = |outer, inner, mode| {
        let builder = deep_nested_multi_input_builder(DEPTH, GROUPS, outer, inner, &limits);
        let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
        builder.build(&mut crate::LayoutContext::new(), &fonts)
    };

    crate::ruby::base_budget::PREFLIGHT_OPS.with(|log| log.borrow_mut().clear());
    crate::ruby::base_budget::PREFLIGHT_OPS_ENABLED.with(|enabled| enabled.set(true));
    let reused = build(
        None,
        None,
        crate::shape::CombinedWidthProbeMode::ScopedReuse,
    );
    crate::ruby::base_budget::PREFLIGHT_OPS_ENABLED.with(|enabled| enabled.set(false));
    let reused = reused.unwrap();
    let ops = crate::ruby::base_budget::PREFLIGHT_OPS.with(|log| log.take());
    let reference = build(
        None,
        None,
        crate::shape::CombinedWidthProbeMode::CloneReference,
    )
    .unwrap();
    assert_eq!(
        width_probe_snapshot(&reference),
        width_probe_snapshot(&reused)
    );
    // Every group is probed under the full scope chain with several charges, and
    // each preflight does at most one chain walk per charge plus one.
    let deep: Vec<_> = ops
        .iter()
        .filter(|op| op.depth >= DEPTH && op.charges >= 4)
        .collect();
    assert!(deep.len() >= GROUPS, "{ops:?}");
    for op in &ops {
        assert!(op.depth <= DEPTH, "{op:?}");
        assert!(op.steps <= (op.charges + 1) * (op.depth + 1), "{op:?}");
        // A single owner is summed first: charges + one walk.
        assert!(op.steps <= op.charges + op.depth, "{op:?}");
    }

    let mut outcomes = Vec::new();
    // The innermost base spends 30 glyphs in total and the last group spans 26..=30,
    // so inner limits 28 and 29 fail inside one reused multi-input group.
    for outer in [None, Some(0), Some(40), Some(80)] {
        for inner in [None, Some(3), Some(28), Some(29), Some(30)] {
            let reference = width_probe_outcome(build(
                outer,
                inner,
                crate::shape::CombinedWidthProbeMode::CloneReference,
            ));
            assert_eq!(
                reference,
                width_probe_outcome(build(
                    outer,
                    inner,
                    crate::shape::CombinedWidthProbeMode::ScopedReuse
                )),
                "outer={outer:?} inner={inner:?}"
            );
            outcomes.push(reference.is_ok());
        }
    }
    assert!(outcomes.contains(&true) && outcomes.contains(&false));
}

#[test]
#[ignore = "manual fixed-font timing probe; run with --ignored --nocapture"]
fn combined_width_base_probe_performance_probe() {
    use std::time::{Duration, Instant};

    const REPEATS: usize = 64;
    const WARMUP_DEFAULT: usize = 3;
    const SAMPLES_DEFAULT: usize = 21;

    fn median_ns(samples: &[u128]) -> u128 {
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }

    fn sample(
        case: WidthProbeCase,
        mode: crate::shape::CombinedWidthProbeMode,
        limits: &Limits,
        fonts: &FontCollection,
    ) -> (Duration, String, usize, usize, usize, usize, usize) {
        let builder = width_probe_builder(case, None, None, REPEATS, limits, false);
        let mut cx = crate::LayoutContext::new();
        HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
        COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.set(0));
        COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| count.set(0));
        COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.set(0));
        COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.set(0));
        let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
        let start = Instant::now();
        let paragraph = builder.build(&mut cx, fonts).unwrap();
        let elapsed = start.elapsed();
        std::hint::black_box(&paragraph);
        let snapshot = width_probe_snapshot(&paragraph);
        (
            elapsed,
            snapshot,
            COMBINED_WIDTH_GROUP_CLONE_COUNT.with(|count| count.get()),
            COMBINED_WIDTH_GROUP_CLONE_SCALARS.with(|count| count.get()),
            COMBINED_WIDTH_GROUP_CLONE_BYTES.with(|bytes| bytes.get()),
            HARFRUST_SHAPE_CALLS.with(|calls| calls.get()),
            COMBINED_WIDTH_TRIAL_STORE_BYTES.with(|bytes| bytes.get()),
        )
    }

    let requested = std::env::var("SHODO_TCY_PROBE_VARIANT").unwrap_or_else(|_| "compare".into());
    let warmup_count = std::env::var("SHODO_TCY_PROBE_WARMUP").map_or(WARMUP_DEFAULT, |value| {
        value.parse().expect("numeric warmup count")
    });
    let sample_count = std::env::var("SHODO_TCY_PROBE_SAMPLES").map_or(SAMPLES_DEFAULT, |value| {
        value.parse().expect("numeric sample count")
    });
    let modes: &[crate::shape::CombinedWidthProbeMode] = match requested.as_str() {
        "clone" => &[crate::shape::CombinedWidthProbeMode::CloneReference],
        "view" => &[crate::shape::CombinedWidthProbeMode::FeatureView],
        "reuse" => &[crate::shape::CombinedWidthProbeMode::ScopedReuse],
        "compare" => &[
            crate::shape::CombinedWidthProbeMode::CloneReference,
            crate::shape::CombinedWidthProbeMode::FeatureView,
        ],
        "reuse-compare" => &[
            crate::shape::CombinedWidthProbeMode::CloneReference,
            crate::shape::CombinedWidthProbeMode::ScopedReuse,
        ],
        _ => panic!("SHODO_TCY_PROBE_VARIANT must be clone, view, reuse, compare or reuse-compare"),
    };
    let limits = Limits::default();
    for case in WIDTH_PROBE_CASES {
        let fonts = width_probe_fonts(&limits);
        let mut outputs = Vec::new();
        for &mode in modes {
            let mut last = None;
            for _ in 0..warmup_count {
                last = Some(sample(case, mode, &limits, &fonts));
            }
            if let Some(last) = last {
                outputs.push(last.1);
            }
        }
        if outputs.len() == 2 {
            assert_eq!(outputs[0], outputs[1], "output mismatch: {}", case.name);
        }

        let mut timings = vec![Vec::with_capacity(sample_count); modes.len()];
        let mut metrics = vec![(0usize, 0usize, 0usize, 0usize, 0usize); modes.len()];
        for index in 0..sample_count {
            for variant in 0..modes.len() {
                let mode_index = if index % 2 == 0 {
                    variant
                } else {
                    modes.len() - 1 - variant
                };
                let (elapsed, output, groups, scalars, bytes, calls, trial_bytes) =
                    sample(case, modes[mode_index], &limits, &fonts);
                if let Some(expected) = outputs.get(mode_index) {
                    assert_eq!(output, *expected, "unstable output: {}", case.name);
                }
                timings[mode_index].push(elapsed.as_nanos());
                metrics[mode_index] = (groups, scalars, bytes, calls, trial_bytes);
            }
        }
        for (index, mode) in modes.iter().enumerate() {
            let (groups, scalars, bytes, calls, trial_bytes) = metrics[index];
            eprintln!(
                "tcy_probe case={} mode={mode:?} groups={groups} cloned_scalars={scalars} estimated_clone_bytes={bytes} shaper_calls={calls} trial_store_bytes={trial_bytes} median_ns={}",
                case.name,
                median_ns(&timings[index]),
            );
        }
        if modes.len() == 2 {
            match modes[1] {
                crate::shape::CombinedWidthProbeMode::FeatureView => {
                    assert_eq!(metrics[0].0, REPEATS, "expected one clone per TCY group");
                    assert_eq!(metrics[1].0, 0, "view should not clone TCY groups");
                    assert_eq!(metrics[0].3, metrics[1].3, "shaper calls changed");
                    assert_eq!(metrics[0].4, metrics[1].4, "trial storage changed");
                }
                crate::shape::CombinedWidthProbeMode::ScopedReuse => {
                    assert_eq!(metrics[0].0, REPEATS, "expected one clone per TCY group");
                    assert_eq!(metrics[1].0, 0, "scoped reuse should not clone TCY groups");
                    assert_eq!(
                        metrics[0].3,
                        metrics[1].3 + REPEATS,
                        "shape-call saving changed"
                    );
                }
                crate::shape::CombinedWidthProbeMode::CloneReference => {
                    unreachable!("the reference mode is the comparison baseline")
                }
            }
        }
    }
}

#[test]
fn combined_width_reuse_replays_selected_warnings_through_the_global_cap() {
    let limits = Limits {
        max_shaping_run_bytes: Some(0),
        max_warnings: Some(1),
        ..Limits::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Width CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Width CJK".into())],
            text_combine_upright: crate::style::TextCombineUpright::All,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "12",
    );

    HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(
        paragraph
            .warnings()
            .iter()
            .map(|warning| warning.kind)
            .collect::<Vec<_>>(),
        [
            crate::limits::WarningKind::Unsupported,
            crate::limits::WarningKind::Suppressed,
        ]
    );
    assert_eq!(HARFRUST_SHAPE_CALLS.with(|calls| calls.get()), 4);
}

#[test]
fn split_items_share_retained_author_features() {
    for real in [false, true] {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        if real {
            fonts
                .register_face(
                    crate::test_support::fonts::LATIN.to_vec(),
                    0,
                    crate::font::FontFaceDescriptor {
                        family: "Latin".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                font_features: vec![
                    crate::style::FontFeature {
                        tag: *b"liga",
                        value: 0
                    };
                    256
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        for _ in 0..32 {
            builder.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                "a",
            );
            builder.push_forced_break(crate::node::NodeId(1));
        }
        features::STYLE_FEATURE_BUILDS.with(|count| count.set(0));
        let p = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        assert_eq!(p.data.runs.len(), 32);
        let first = &p.data.runs[0].instance.features;
        assert_eq!(first.len(), 256);
        for run in &p.data.runs {
            assert_eq!(
                run.instance.features.as_ptr(),
                first.as_ptr(),
                "real={real}"
            );
        }
        assert_eq!(features::STYLE_FEATURE_BUILDS.with(|count| count.get()), 1);
        for _ in 0..2 {
            let (_, edge) = shape_window(
                &p.data,
                &p.data.units[0],
                &mut crate::LayoutContext::new(),
                &mut crate::limits::WarningSink::default(),
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(edge[0].instance.features.as_ptr(), first.as_ptr());
        }
        let replacement = Replacement {
            text: 0..1,
            c: 'b',
            font: None,
        };
        let (_, edited) = shape_window_edit(
            &p.data,
            &p.data.units[0],
            None,
            Some(&replacement),
            &mut crate::LayoutContext::new(),
            &mut crate::limits::WarningSink::default(),
            &mut Saturation::default(),
        )
        .unwrap();
        assert_eq!(edited[0].instance.features.as_ptr(), first.as_ptr());
    }
}

#[test]
fn shared_features_preserve_style_orientation_and_width_settings() {
    let p = edge_input_paragraph(false, 1024);
    let mut styles = vec![p.data.styles[p.data.shape_items[0].style as usize].clone(); 2];
    styles[1].font_features.push(crate::style::FontFeature {
        tag: *b"liga",
        value: 1,
    });
    let mut items = Vec::new();
    for _ in 0..2 {
        for (orientation, width, style) in [
            (orientation::RunOrientation::Horizontal, None, 0),
            (orientation::RunOrientation::Upright, None, 0),
            (orientation::RunOrientation::Horizontal, Some(*b"hwid"), 0),
            (orientation::RunOrientation::Horizontal, None, 1),
        ] {
            let mut item = p.data.shape_items[0].clone();
            item.orientation = orientation;
            item.width_feature = width;
            item.style = style;
            items.push(item);
        }
    }
    let (_, runs) = shape_items(
        &mut crate::LayoutContext::new(),
        &items,
        &styles,
        &p.data.fonts,
        WritingMode::HorizontalTb,
        &Limits::default(),
        &mut crate::limits::WarningSink::default(),
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(runs.len(), 8);
    for (i, run) in runs.iter().enumerate() {
        let expected = features::for_item(&styles[items[i].style as usize], &items[i]);
        assert_eq!(
            format!("{:?}", run.instance.features),
            format!("{expected:?}")
        );
        assert_eq!(
            run.instance.features.as_ptr(),
            runs[i % 4].instance.features.as_ptr()
        );
    }
}

#[test]
fn missing_vorg_origin_cache_spans_budget_windows_but_not_inputs() {
    let original = crate::test_support::fonts::CJK;
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    let tables: Vec<_> = (0..count)
        .filter_map(|n| {
            let at = 12 + n * 16;
            let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
            let start = u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
            (&tag != b"VORG").then(|| (tag, original[start..start + len].to_vec()))
        })
        .collect();
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "No VORG".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = crate::style::InlineStyle {
        font_size: 16.0,
        font_families: vec![crate::style::FontFamily::Named("No VORG".into())],
        ..Default::default()
    };
    let style = crate::style::ParagraphStyle {
        writing_mode: crate::geometry::WritingMode::VerticalRl,
        root: root.clone(),
        ..Default::default()
    };
    let larger = crate::style::InlineStyle {
        font_size: 20.0,
        ..root
    };
    let build = |budget| {
        let limits = Limits {
            max_shaping_run_bytes: budget,
            ..Limits::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        let node = crate::node::NodeId(1);
        builder.push_text(crate::node::TextSource::Generated { node }, "水日水日水日");
        builder
            .open_inline(
                crate::node::NodeId(2),
                &larger,
                crate::node::InlineEdges::default(),
            )
            .push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(3),
                },
                "水日水日",
            )
            .close_inline();
        CFF_ORIGIN_DELTA_CALLS.with(|calls| calls.set(0));
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let calls = CFF_ORIGIN_DELTA_CALLS.with(std::cell::Cell::get);
        (paragraph, calls)
    };
    let (whole, whole_calls) = build(None);
    // One window per Han scalar: each input repeats two glyphs over many windows.
    let (split, split_calls) = build(Some(3));
    assert_eq!(whole.data.shape_items.len(), 2);
    // Two distinct glyphs per input; the cache is rebuilt for the second input.
    assert_eq!(whole_calls, 4);
    assert_eq!(split_calls, 4);
    let glyphs = |p: &crate::Paragraph| {
        let g = &p.data.glyphs;
        (
            g.id.clone(),
            g.advance.clone(),
            g.offset_inline.clone(),
            g.offset_block.clone(),
        )
    };
    assert_eq!(glyphs(&whole), glyphs(&split));
    assert!(whole.warnings().is_empty() && split.warnings().is_empty());
}

#[test]
fn missing_vorg_uses_vmtx_top_bearing_for_vertical_origin() {
    // CJK 水 has yMax=838 in this pinned outline and vmtx TSB=42.
    // Remove VORG and change only its TSB to 142: origin becomes 980.
    let original = crate::test_support::fonts::CJK;
    let face = skrifa::FontRef::from_index(original, 0).unwrap();
    let gid = face.charmap().map('水').unwrap().to_u32() as usize;
    let mut tables = Vec::new();
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    for n in 0..count {
        let at = 12 + n * 16;
        let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
        if &tag == b"VORG" {
            continue;
        }
        let start = u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
        let mut data = original[start..start + len].to_vec();
        if &tag == b"vmtx" {
            data[gid * 4 + 2..gid * 4 + 4].copy_from_slice(&142i16.to_be_bytes());
        }
        tables.push((tag, data));
    }
    let bytes = crate::font::sfnt::build_sfnt(&tables);
    let derived = skrifa::FontRef::from_index(&bytes, 0).unwrap();
    assert!(derived.vorg().is_err());
    assert_eq!(
        derived
            .vmtx()
            .unwrap()
            .side_bearing(skrifa::GlyphId::new(gid as u32)),
        Some(142)
    );
    let bounds = derived
        .glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .bounds(skrifa::GlyphId::new(gid as u32))
        .unwrap();
    assert_eq!(bounds.y_max, 838.0);
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let registered = fonts
        .register_face(
            bytes,
            0,
            crate::font::FontFaceDescriptor {
                family: "No VORG".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        writing_mode: crate::geometry::WritingMode::VerticalRl,
        root: crate::style::InlineStyle {
            font_size: 16.0,
            font_families: vec![crate::style::FontFamily::Named("No VORG".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "水",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    assert_eq!(paragraph.data.runs[0].font, registered);
    assert_eq!(paragraph.data.glyphs.id[0], gid as u32);
    assert_eq!(paragraph.data.glyphs.advance[0].to_f32(), 16.0);
    assert_eq!(paragraph.data.glyphs.offset_inline[0].to_f32(), 15.6875);
    assert_eq!(paragraph.data.glyphs.offset_block[0].to_f32(), 8.0);
}

/// Shapes `text` through the missing-font path: the collection has no faces.
fn shape(
    text: &str,
    size: f32,
    limits: &Limits,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    let fonts = FontCollection::with_options(
        limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: size,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        text,
    );
    let paragraph = builder.build(&mut crate::LayoutContext::new(), &fonts)?;
    Ok((paragraph.data.glyphs.clone(), paragraph.data.runs.clone()))
}

#[test]
fn one_em_per_character() {
    let (g, runs) = shape("abc", 10.0, &Limits::default()).unwrap();
    assert_eq!(g.len(), 3);
    let px: Vec<f32> = g.pen.iter().map(|p| p.to_f32()).collect();
    assert_eq!(px, vec![0.0, 10.0, 20.0]);
    assert!(g.advance.iter().all(|a| a.to_f32() == 10.0));
    assert_eq!(g.cluster, vec![0, 1, 2]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, 0..3);
}

#[test]
fn combining_marks_have_zero_advance_and_an_offset() {
    let (g, _) = shape("e\u{301}x", 10.0, &Limits::default()).unwrap();
    assert_eq!(g.advance[1], LayoutUnit::ZERO);
    assert_eq!(g.offset_inline[1].to_f32(), -5.0);
    assert_eq!(g.cluster[1], 1);
    assert_eq!(g.pen[2].to_f32(), 10.0);
}

#[test]
fn pen_positions_restart_before_saturating() {
    // 1e6 px per glyph: 16 glyphs fit under 2^30 units (about 1.68e7 px).
    let text = "a".repeat(40);
    let (g, runs) = shape(&text, 1.0e6, &Limits::default()).unwrap();
    let sizes: Vec<u32> = runs.iter().map(|r| r.glyphs.end - r.glyphs.start).collect();
    assert_eq!(sizes, vec![16, 16, 8]);
    assert_eq!(g.pen[16], LayoutUnit::ZERO);
    // Differences inside a run stay exact.
    assert_eq!((g.pen[15] - g.pen[14]).to_f32(), 1.0e6);
    assert_eq!(runs[1].text, 16..32);
}

#[test]
fn missing_font_glyphs_are_notdef_and_runs_share_one_instance() {
    // ZWJ is default ignorable: a zero-advance glyph without a mark offset.
    let text = format!("{}\u{200d}{}", "a".repeat(20), "b".repeat(19));
    let (g, runs) = shape(&text, 1.0e6, &Limits::default()).unwrap();
    assert!(g.id.iter().all(|&id| id == 0));
    assert_eq!(g.advance[20], LayoutUnit::ZERO);
    assert_eq!(g.offset_inline[20], LayoutUnit::ZERO);
    assert_eq!(g.cluster[21], 23);
    // The zero-advance ZWJ does not use pen space, so the second run takes
    // one extra glyph before the pen limit.
    let sizes: Vec<u32> = runs.iter().map(|r| r.glyphs.end - r.glyphs.start).collect();
    assert_eq!(sizes, vec![16, 17, 7]);
    assert_eq!(
        runs.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
        [0..16, 16..35, 35..42]
    );
    assert!(
        runs.iter()
            .all(|r| Arc::ptr_eq(&r.instance, &runs[0].instance))
    );
}

#[test]
fn glyph_count_limit_is_checked_before_pushing() {
    let limits = Limits {
        max_shaped_glyphs: Some(2),
        ..Limits::default()
    };
    let err = shape("abc", 10.0, &limits).unwrap_err();
    assert_eq!(err.kind, LimitKind::ShapedGlyphs);
}
#[test]
fn arabic_joining_retains_shaper_safety_flags() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::ARABIC.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named(
                "Shodo Fixture Arabic".into(),
            )],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "السلام",
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(p.data.units.iter().any(|u| u.unsafe_to_break));
    assert!(p.data.units.iter().any(|u| u.unsafe_to_concat));
}
#[test]
fn optical_sizing_and_explicit_variations_survive_public_views() {
    let bytes = crate::test_support::fonts::LATIN;
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            bytes[at..at + 4].try_into().unwrap(),
            bytes[offset..offset + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, 2, 20, 0, 8] {
        fvar.extend(field.to_be_bytes());
    }
    for (tag, values) in [(b"wght", [100i32, 400, 900]), (b"opsz", [8, 12, 72])] {
        fvar.extend(tag);
        for value in values {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Variable".into(),
                weight: (100.0, 900.0),
                ..Default::default()
            },
        )
        .unwrap();
    for (optical, explicit, want) in [
        (true, None, vec![1.0, 1.0]),
        (false, None, vec![1.0, 0.0]),
        (true, Some(8.0), vec![1.0, -1.0]),
    ] {
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 72.0,
                font_families: vec![crate::style::FontFamily::Named("Variable".into())],
                font_weight: 900.0,
                font_optical_sizing: optical,
                font_variations: explicit
                    .map(|value| crate::style::FontVariation {
                        tag: *b"opsz",
                        value,
                    })
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        let crate::LineResult::Line(line) = p.next_line(
            &mut crate::LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(1000.0),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let run = line
            .fragments()
            .find_map(|f| {
                if let crate::Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(
            run.normalized_coords()
                .iter()
                .map(|c| c.to_f32())
                .collect::<Vec<_>>(),
            want
        );
        assert!(
            run.variations()
                .iter()
                .any(|v| v.tag == *b"wght" && v.value == 900.0)
        );
    }
}
#[test]
fn real_font_pen_splits_before_prefix_overflow() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Latin".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"W".repeat(32),
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(p.data.runs.len() > 1);
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
    );
    assert_eq!(p.data.glyphs.len(), 32);
}

#[test]
fn expanded_single_cluster_pen_splits_without_new_breaks() {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
    let a = font.charmap().map('a').unwrap().to_u32() as u16;
    let w = font.charmap().map('W').unwrap().to_u32() as u16;
    // One ccmp MultipleSubst expands a single input cluster to32 glyphs.
    let mut gsub = Vec::new();
    for value in [1u16, 0, 10, 30, 44, 1] {
        gsub.extend(value.to_be_bytes());
    }
    gsub.extend(b"latn");
    for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gsub.extend(value.to_be_bytes());
    }
    gsub.extend(b"ccmp");
    for value in [8u16, 0, 1, 0, 1, 4, 2, 0, 1, 8, 1, 74, 1, 8, 32] {
        gsub.extend(value.to_be_bytes());
    }
    for at in 0..32 {
        gsub.extend(if at % 3 == 0 { a } else { w }.to_be_bytes());
    }
    for value in [1u16, 1, a] {
        gsub.extend(value.to_be_bytes());
    }
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
    let bytes = crate::font::sfnt::build_sfnt(&tables);
    let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
    let direct_data = harfrust::ShaperData::new(&font);
    let shaper = direct_data.shaper(&font).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    buffer.push_str("a");
    buffer.guess_segment_properties();
    let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
    assert_eq!(shaped.len(), 32, "direct test substitution must expand");
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Expansion".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Expansion".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "a",
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert_eq!(p.data.glyphs.len(), 32, "test substitution must expand");
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
    );
    assert!(p.data.runs.len() > 1);
    assert!(
        p.data.units[..p.data.units.len() - 1]
            .iter()
            .all(|u| u.break_after == crate::analysis::units::BreakClass::Prohibited)
    );
    // Distinct glyphs and advances expose a missing RTL part reversal, which a
    // uniform expansion cannot catch. Force the retained item's bidi level so
    // the same pinned Latin substitution exercises both shaping directions.
    for rtl in [false, true] {
        let mut items = p.data.shape_items.clone();
        for item in &mut items {
            item.level = u8::from(rtl);
        }
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("a");
        buffer.set_script(harfrust::script::LATIN);
        buffer.set_direction(if rtl {
            harfrust::Direction::RightToLeft
        } else {
            harfrust::Direction::LeftToRight
        });
        let expected = shaper.shape(buffer, harfrust::ShapeOptions::default());
        let mut warnings = crate::limits::WarningSink::default();
        let mut sat = Saturation::default();
        let (glyphs, runs) = shape_items(
            &mut crate::LayoutContext::new(),
            &items,
            &p.data.styles,
            &fonts,
            style.writing_mode,
            &limits,
            &mut warnings,
            &mut sat,
        )
        .unwrap();
        assert!(runs.len() > 1);
        assert!(sat.is_clean(), "storage splits must avoid pen saturation");
        assert!(runs.iter().all(|run| run.text == (0..1)));
        assert!(glyphs.cluster.iter().all(|cluster| *cluster == 0));
        let mut visual_runs: Vec<_> = runs.iter().collect();
        if rtl {
            visual_runs.reverse();
        }
        let actual: Vec<_> = visual_runs
            .into_iter()
            .flat_map(|run| {
                glyphs.id[run.glyphs.start as usize..run.glyphs.end as usize]
                    .iter()
                    .copied()
            })
            .collect();
        assert_eq!(
            actual,
            expected
                .glyph_infos()
                .iter()
                .map(|g| g.glyph_id)
                .collect::<Vec<_>>(),
            "rtl={rtl}: splitting must preserve intra-cluster visual order"
        );
        assert!(
            glyphs
                .pen
                .iter()
                .all(|pen| pen.raw().abs() <= RUN_PEN_LIMIT)
        );
        let warnings = warnings.take();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, crate::limits::WarningKind::Unsupported);
        assert!(
            warnings[0]
                .message
                .contains("glyph cluster exceeds run pen budget")
        );
    }
}

#[test]
fn negative_positioning_advances_obey_run_pen_budget() {
    use skrifa::MetadataProvider;
    let bytes = crate::test_support::fonts::LATIN;
    let glyph = skrifa::FontRef::from_index(bytes, 0)
        .unwrap()
        .charmap()
        .map('W')
        .unwrap()
        .to_u32() as u16;
    let mut gpos = Vec::new();
    for value in [1u16, 0, 10, 30, 44, 1] {
        gpos.extend(value.to_be_bytes());
    }
    gpos.extend(b"latn");
    for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
        gpos.extend(value.to_be_bytes());
    }
    gpos.extend(b"kern");
    for value in [
        8u16,
        0,
        1,
        0,
        1,
        4,
        1,
        0,
        1,
        8,
        1,
        8,
        4,
        (-2000i16) as u16,
        1,
        1,
        glyph,
    ] {
        gpos.extend(value.to_be_bytes());
    }
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        if tag != *b"GPOS" {
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
    }
    tables.push((*b"GPOS", gpos));
    tables.sort_by_key(|(tag, _)| *tag);
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::font::sfnt::build_sfnt(&tables),
            0,
            crate::font::FontFaceDescriptor {
                family: "Negative".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_size: 1e6,
            font_families: vec![crate::style::FontFamily::Named("Negative".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"W".repeat(64),
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert!(
        p.data.glyphs.advance.iter().all(|a| a.raw() < 0),
        "test positioning must make advances negative"
    );
    assert!(
        p.data
            .glyphs
            .pen
            .iter()
            .all(|p| i64::from(p.raw()).abs() <= i64::from(RUN_PEN_LIMIT))
    );
}

#[test]
fn tiny_run_windows_share_one_resolved_instance() {
    for real in [false, true] {
        let limits = Limits {
            max_shaping_run_bytes: Some(1),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        if real {
            fonts
                .register_face(
                    crate::test_support::fonts::LATIN.to_vec(),
                    0,
                    crate::font::FontFaceDescriptor {
                        family: "Latin".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                font_features: vec![crate::style::FontFeature {
                    tag: *b"liga",
                    value: 0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"a".repeat(32),
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert_eq!(p.data.runs.len(), 32);
        assert!(
            p.data
                .runs
                .iter()
                .all(|r| Arc::ptr_eq(&r.instance, &p.data.runs[0].instance)),
            "real={real}"
        );
    }
}

#[test]
fn smaller_run_budget_automatically_releases_retained_scratch() {
    let limits = Limits::unlimited();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Latin".into())],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut cx = crate::LayoutContext::new();
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"a".repeat(4096),
    );
    b.build(&mut cx, &fonts).unwrap();
    assert!(cx.scratch_bytes > 1000);
    let limits = Limits {
        max_shaping_run_bytes: Some(8),
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &limits);
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "a",
    );
    b.build(&mut cx, &fonts).unwrap();
    assert!(cx.scratch_bytes <= 8 * 128);
}

#[test]
fn shaping_windows_borrow_source_scalars() {
    for real in [false, true] {
        for budget in [1, 1024] {
            let limits = Limits {
                max_shaping_run_bytes: Some(budget),
                ..Default::default()
            };
            let fonts = FontCollection::with_options(
                &limits,
                crate::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            if real {
                fonts
                    .register_face(
                        crate::test_support::fonts::LATIN.to_vec(),
                        0,
                        crate::font::FontFaceDescriptor {
                            family: "Latin".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            let style = crate::style::ParagraphStyle {
                root: crate::style::InlineStyle {
                    font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                    font_features: vec![crate::style::FontFeature {
                        tag: *b"liga",
                        value: 0,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut b = crate::ParagraphBuilder::new(&style, &limits);
            b.push_text(
                crate::node::TextSource::Dom {
                    node: crate::node::NodeId(1),
                    offset: 7,
                },
                &"abcd".repeat(8),
            );
            let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
            let original: Vec<_> = p
                .data
                .shape_items
                .iter()
                .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                .collect();
            crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
            let mut warnings = crate::limits::WarningSink::default();
            let (glyphs, runs) = shape_items(
                &mut crate::LayoutContext::new(),
                &p.data.shape_items,
                &p.data.styles,
                &fonts,
                style.writing_mode,
                &limits,
                &mut warnings,
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(glyphs.id, p.data.glyphs.id);
            assert_eq!(glyphs.cluster, p.data.glyphs.cluster);
            assert_eq!(glyphs.advance, p.data.glyphs.advance);
            assert_eq!(glyphs.pen, p.data.glyphs.pen);
            assert_eq!(
                runs.iter()
                    .map(|r| (r.text.clone(), r.item))
                    .collect::<Vec<_>>(),
                p.data
                    .runs
                    .iter()
                    .map(|r| (r.text.clone(), r.item))
                    .collect::<Vec<_>>()
            );
            assert_eq!(runs.len(), if budget == 1 { 32 } else { 1 });
            assert_eq!(warnings.take().is_empty(), real);
            assert_eq!(
                p.data
                    .shape_items
                    .iter()
                    .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                    .collect::<Vec<_>>(),
                original,
                "retained original scalar ownership must be preserved"
            );
            assert_eq!(
                crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
                0,
                "real={real}, budget={budget}: shaping a window must borrow its scalars"
            );
        }
    }
}

#[test]
fn unedited_edge_windows_borrow_source_scalars() {
    // Reintroducing a clipped scalar Vec must fail the actual Clone check;
    // changing the clip/budget must fail offsets and warning/progress checks.
    for real in [false, true] {
        for budget in [1, 1024] {
            let p = edge_input_paragraph(real, budget);
            let ownership: Vec<_> = p
                .data
                .shape_items
                .iter()
                .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                .collect();
            let mut unit = p.data.units[5].clone();
            unit.text = 5..31;
            crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
            let mut warnings = crate::limits::WarningSink::default();
            let (glyphs, runs) = shape_window_budget(
                &p.data,
                &unit,
                None,
                &mut crate::LayoutContext::new(),
                &mut warnings,
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(glyphs.cluster, (5..31).collect::<Vec<_>>());
            assert_eq!(glyphs.id, p.data.glyphs.id[5..31]);
            let expected_runs: Vec<_> = if budget == 1 {
                (5..31).map(|at| at..at + 1).collect()
            } else {
                std::iter::once(5..31).collect()
            };
            assert_eq!(
                runs.iter().map(|run| run.text.clone()).collect::<Vec<_>>(),
                expected_runs
            );
            assert_eq!(
                warnings.take().len(),
                if real {
                    0
                } else if budget == 1 {
                    26
                } else {
                    1
                }
            );
            assert_eq!(
                p.data
                    .shape_items
                    .iter()
                    .map(|item| (item.scalars.as_ptr(), item.scalars.len(), item.end))
                    .collect::<Vec<_>>(),
                ownership
            );
            assert_eq!(
                crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
                0,
                "real={real}, budget={budget}: unedited edge input must borrow the26 source scalars"
            );
        }
    }
}

fn edge_input_paragraph(real: bool, budget: u64) -> crate::Paragraph {
    let limits = Limits {
        max_shaping_run_bytes: Some(budget),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    if real {
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let style = crate::style::ParagraphStyle {
        root: crate::style::InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("Latin".into())],
            font_kerning: crate::style::FontKerning::None,
            font_features: vec![crate::style::FontFeature {
                tag: *b"liga",
                value: 0,
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Dom {
            node: crate::node::NodeId(1),
            offset: 7,
        },
        "abcdefghijklmnopqrstuvwxyz0123456789",
    );
    builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap()
}

#[test]
fn compatible_unedited_edge_keeps_owned_merge_and_same_full_shaping() {
    let mut p = edge_input_paragraph(true, 1024);
    let mut unit = p.data.units[5].clone();
    unit.text = 5..31;
    let mut warnings = crate::limits::WarningSink::default();
    let expected = shape_window_budget(
        &p.data,
        &unit,
        None,
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert!(warnings.take().is_empty());
    let data = Arc::get_mut(&mut p.data).unwrap();
    let first = &mut data.shape_items[0];
    let mut second = first.clone();
    second.scalars = first.scalars.split_off(18);
    second.before = first.scalars[13..].iter().map(|s| s.c).collect();
    first.after = second.scalars[..5].iter().map(|s| s.c).collect();
    first.end = 18;
    data.shape_items.push(second);
    crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
    let actual = shape_window_budget(
        &p.data,
        &unit,
        None,
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(
        crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
        26
    );
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert!(warnings.take().is_empty());
}

#[test]
fn font_substitution_edge_keeps_owned_scalars_and_source_end() {
    let p = edge_input_paragraph(true, 1024);
    let mut unit = p.data.units[5].clone();
    unit.text = 5..31;
    let replacement = Replacement {
        text: 5..6,
        c: '‑',
        font: None,
    };
    let mut warnings = crate::limits::WarningSink::default();
    crate::analysis::itemize::SCALAR_CLONES.with(|count| count.set(0));
    let (glyphs, runs) = shape_window_edit(
        &p.data,
        &unit,
        None,
        Some(&replacement),
        &mut crate::LayoutContext::new(),
        &mut warnings,
        &mut Saturation::default(),
    )
    .unwrap();
    assert_eq!(
        crate::analysis::itemize::SCALAR_CLONES.with(|count| count.get()),
        26
    );
    assert_eq!(glyphs.cluster, (5..31).collect::<Vec<_>>());
    assert_eq!(glyphs.id[0], 0);
    assert_eq!(
        runs.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
        [5..6, 6..31]
    );
    assert_eq!(p.data.shape_items[0].scalars[5].c, 'f');
    assert_eq!(p.data.shape_items[0].scalars[5].end, 6);
    assert_eq!(warnings.take().len(), 1);
}

/// Mirrors one case of `dev/bench/examples/tcy_base_reuse.rs` with the same
/// fixture font files, so the release probe's shaper calls can be counted.
fn tcy_base_reuse_probe_builder(case: &str, limits: &Limits) -> crate::ParagraphBuilder {
    use crate::geometry::Direction;
    use crate::node::{InlineEdges, NodeId, TextSource};
    const REPEATS: u64 = 64;
    if case == "deep-nested-multi" {
        return tcy_base_reuse_probe_deep_builder(limits);
    }
    let (text, mode, direction, multiple_styles) = match case {
        "base-two-rl" | "base-fallback" | "plain-tcy" => {
            ("12", WritingMode::VerticalRl, Direction::Ltr, false)
        }
        "base-three-lr-rtl" => ("123", WritingMode::VerticalLr, Direction::Rtl, false),
        "base-four-rl-multi" => ("1234", WritingMode::VerticalRl, Direction::Ltr, true),
        "base-two-lr-rtl-multi" => ("12", WritingMode::VerticalLr, Direction::Rtl, true),
        _ => panic!("unknown case {case}"),
    };
    let cjk = width_probe_style("Width CJK", direction);
    let plain_latin = crate::style::InlineStyle {
        font_size: 19.0,
        text_combine_upright: crate::style::TextCombineUpright::None,
        ..width_probe_style("Width Latin", direction)
    };
    let base_limits = Limits {
        max_shaping_run_bytes: (case == "base-fallback").then_some(1),
        ..Limits::default()
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: mode,
        direction,
        root: cjk.clone(),
        ..Default::default()
    };
    let plain = case == "plain-tcy";
    let mut base =
        crate::ParagraphBuilder::new(&paragraph_style, if plain { limits } else { &base_limits });
    for repeat in 0..REPEATS {
        if repeat > 0 {
            let separator = NodeId(3000 + repeat);
            base.open_inline(separator, &plain_latin, InlineEdges::default())
                .push_text(
                    TextSource::Dom {
                        node: separator,
                        offset: 0,
                    },
                    "x",
                )
                .close_inline();
        }
        base.open_inline(NodeId(100 + repeat), &cjk, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: NodeId(1000 + repeat),
                    offset: 10,
                },
                text,
            )
            .close_inline();
    }
    if multiple_styles {
        let node = NodeId(9000);
        base.open_inline(node, &plain_latin, InlineEdges::default())
            .push_text(TextSource::Dom { node, offset: 0 }, "x")
            .close_inline();
    }
    if plain {
        return base;
    }
    let mut builder = crate::ParagraphBuilder::new(&paragraph_style, limits);
    builder.push_ruby(
        NodeId(1),
        &cjk,
        tcy_base_reuse_probe_ruby(crate::RubyContent::from_builder(base), &cjk, limits),
    );
    builder
}

fn tcy_base_reuse_probe_ruby(
    base: crate::RubyContent,
    style: &crate::style::InlineStyle,
    limits: &Limits,
) -> crate::Ruby {
    use crate::node::{NodeId, TextSource};
    crate::Ruby::new(
        vec![crate::RubyBase {
            node: NodeId(2),
            content: base,
            align: crate::RubyAlign::default(),
        }],
        vec![crate::RubyLevel {
            annotations: vec![crate::RubyAnnotation {
                node: NodeId(3),
                content: crate::RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(3),
                        offset: 30,
                    },
                    "日",
                    style,
                    limits,
                ),
                span: crate::RubySpan::Auto,
                visibility: crate::RubyVisibility::Visible,
            }],
            style: crate::RubyStyle::default(),
        }],
    )
    .unwrap()
}

/// Mirror of the probe's `deep_builder` (250 levels, 400 groups).
fn tcy_base_reuse_probe_deep_builder(limits: &Limits) -> crate::ParagraphBuilder {
    use crate::geometry::Direction;
    use crate::node::{InlineEdges, NodeId, TextSource};
    const DEPTH: usize = 250;
    const GROUPS: u64 = 400;
    let cjk = width_probe_style("Width CJK", Direction::Ltr);
    let mixed = crate::style::InlineStyle {
        font_families: vec![
            crate::style::FontFamily::Named("Width Latin".into()),
            crate::style::FontFamily::Named("Width CJK".into()),
        ],
        ..cjk.clone()
    };
    let plain = crate::style::InlineStyle {
        text_combine_upright: crate::style::TextCombineUpright::None,
        ..width_probe_style("Width Latin", Direction::Ltr)
    };
    let paragraph_style = crate::style::ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        direction: Direction::Ltr,
        root: cjk.clone(),
        ..Default::default()
    };
    let base_limits = Limits::default();
    let mut content = crate::ParagraphBuilder::new(&paragraph_style, &base_limits);
    for group in 0..GROUPS {
        let separator = NodeId(100_000 + 2 * group);
        let text = NodeId(100_001 + 2 * group);
        content
            .open_inline(separator, &plain, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: separator,
                    offset: 0,
                },
                "x",
            )
            .close_inline()
            .open_inline(text, &mixed, InlineEdges::default())
            .push_text(
                TextSource::Dom {
                    node: text,
                    offset: 0,
                },
                "1日2日",
            )
            .close_inline();
    }
    for level in (0..DEPTH).rev() {
        let ruby =
            tcy_base_reuse_probe_ruby(crate::RubyContent::from_builder(content), &cjk, limits);
        content = crate::ParagraphBuilder::new(
            &paragraph_style,
            if level == 0 { limits } else { &base_limits },
        );
        content.push_ruby(NodeId(1), &cjk, ruby);
    }
    content
}

#[test]
#[ignore = "manual shaper-call count for the tcy_base_reuse release probe cases; run with --ignored --nocapture"]
fn tcy_base_reuse_probe_shaper_calls() {
    let limits = Limits::default();
    let fonts = width_probe_fonts(&limits);
    for case in [
        "base-two-rl",
        "base-three-lr-rtl",
        "base-four-rl-multi",
        "base-two-lr-rtl-multi",
        "base-fallback",
        "plain-tcy",
        "deep-nested-multi",
    ] {
        let run = |mode| {
            let builder = tcy_base_reuse_probe_builder(case, &limits);
            HARFRUST_SHAPE_CALLS.with(|calls| calls.set(0));
            let _mode_guard = CombinedWidthProbeModeGuard::new(mode);
            let paragraph = builder
                .build(&mut crate::LayoutContext::new(), &fonts)
                .unwrap();
            (
                width_probe_snapshot(&paragraph),
                paragraph.data.glyphs.len(),
                HARFRUST_SHAPE_CALLS.with(|calls| calls.get()),
            )
        };
        let (reference, reference_glyphs, reference_calls) =
            run(crate::shape::CombinedWidthProbeMode::CloneReference);
        let (reused, reused_glyphs, reused_calls) =
            run(crate::shape::CombinedWidthProbeMode::ScopedReuse);
        assert_eq!(reference, reused, "{case}");
        assert_eq!(reference_glyphs, reused_glyphs, "{case}");
        println!(
            "{{\"case\":\"{case}\",\"glyphs\":{reused_glyphs},\"reference_shaper_calls\":{reference_calls},\"scoped_reuse_shaper_calls\":{reused_calls}}}"
        );
    }
}

#[test]
fn vertical_punctuation_blanks_follow_the_upright_inline_axis() {
    use crate::font::{FontFaceDescriptor, FontOptions};
    use crate::line::punctuation::PunctuationClass;
    use crate::node::{NodeId, TextSource};
    use crate::style::{
        FontFamily, FontVariantEastAsian, FontVariantEastAsianWidth, InlineStyle, ParagraphStyle,
        TextSpacingTrim,
    };

    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
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
                family: "Vertical blank CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let layout = |trim, width| {
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Vertical blank CJK".into())],
                font_size: 20.0,
                lang: Some("ja".into()),
                text_spacing_trim: trim,
                font_variant_east_asian: FontVariantEastAsian {
                    width,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Generated { node: NodeId(1) },
            "水）（水水（（水（水",
        );
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let crate::LineResult::Line(line) = paragraph.next_line(
            &mut crate::LayoutContext::new(),
            paragraph.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(400.0),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!("expected one line")
        };
        let mut origins = Vec::new();
        for fragment in line.fragments() {
            if let crate::Fragment::GlyphRun(run) = fragment {
                for i in 0..run.glyphs().len() {
                    origins.push(run.inline_start() + run.glyph_origin(i).unwrap().0);
                }
            }
        }
        let blanks: Vec<_> = paragraph
            .data
            .punctuation
            .iter()
            .filter(|p| {
                matches!(
                    p.class,
                    PunctuationClass::Opening | PunctuationClass::Closing
                )
            })
            .map(|p| (p.class, p.left, p.right))
            .collect();
        (
            paragraph.data.glyphs.id.clone(),
            blanks,
            line.inline_size(),
            origins,
        )
    };
    let half = LayoutUnit::from_f32_round(10.0, &mut Saturation::default());

    // The rotated vertical forms keep their ink in one half of the vertical
    // em, so the inline blanks come from y bounds rather than x bounds.
    let (vertical_ids, vertical_blanks, _, _) = layout(TextSpacingTrim::SpaceAll, None);
    assert_eq!(vertical_blanks.len(), 5);
    assert!(
        vertical_blanks
            .iter()
            .all(|&(class, left, right)| match class {
                PunctuationClass::Opening => left == half && right == LayoutUnit::ZERO,
                _ => left == LayoutUnit::ZERO && right == half,
            })
    );

    // `pwid` replaces the parentheses before `vert`, leaving upright
    // proportional glyphs whose ink fills the vertical em. Such glyphs have
    // no blank to trim, so `normal` lays out exactly like `space-all`.
    let proportional = Some(FontVariantEastAsianWidth::ProportionalWidth);
    let normal = layout(TextSpacingTrim::Normal, proportional);
    let space_all = layout(TextSpacingTrim::SpaceAll, proportional);
    assert_ne!(normal.0[1..3], vertical_ids[1..3]);
    assert!(
        normal
            .1
            .iter()
            .all(|&(_, left, right)| left == LayoutUnit::ZERO && right == LayoutUnit::ZERO)
    );
    assert_eq!(normal.2, 200.0);
    assert_eq!(normal, space_all);
}

#[test]
fn vertical_layout_trims_use_upright_blanks() {
    use crate::font::{FontFaceDescriptor, FontOptions};
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle, TextSpacingTrim};

    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
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
                family: "Vertical layout CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    // Returns each line's inline size and every glyph's inline origin.
    let layout = |text: &str, trim, width| {
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Vertical layout CJK".into())],
                font_size: 20.0,
                lang: Some("ja".into()),
                text_spacing_trim: trim,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let mut lines = Vec::new();
        let mut token = paragraph.start_token();
        while let crate::LineResult::Line(line) = paragraph.next_line(
            &mut crate::LayoutContext::new(),
            token,
            &Default::default(),
            &crate::LineConstraint::new(width),
            &crate::AtomicSizes::EMPTY,
        ) {
            let mut origins = Vec::new();
            for fragment in line.fragments() {
                if let crate::Fragment::GlyphRun(run) = fragment {
                    for i in 0..run.glyphs().len() {
                        origins.push(run.inline_start() + run.glyph_origin(i).unwrap().0);
                    }
                }
            }
            lines.push((line.inline_size(), origins));
            token = line.break_token();
        }
        lines
    };
    // The first rotated closing parenthesis loses its trailing half.
    let pair = layout("水））", TextSpacingTrim::Normal, 400.0);
    assert_eq!(pair.len(), 1);
    assert_eq!(pair[0].0, 50.0);
    // A rotated middle dot loses a quarter on each side, so its outline
    // origin moves a quarter toward the line start.
    let middle = layout("水・水", TextSpacingTrim::TrimAll, 400.0);
    let spaced = layout("水・水", TextSpacingTrim::SpaceAll, 400.0);
    assert_eq!(middle[0].0, 50.0);
    assert_eq!(spaced[0].0, 60.0);
    assert_eq!(middle[0].1[1], spaced[0].1[1] - 5.0);

    // `allow-end` sets the rotated closing parenthesis half-width only when
    // the line would otherwise overflow, so it stays on the first line. Its
    // ink is at the inline start, so trimming keeps the outline origin.
    let normal = layout("水水水）", TextSpacingTrim::Normal, 75.0);
    let space_all = layout("水水水）", TextSpacingTrim::SpaceAll, 75.0);
    assert_eq!(normal.len(), 1);
    assert_eq!(normal[0].0, 70.0);
    assert_eq!(normal[0].1[3], space_all[1].1[1] + 40.0);
    assert_eq!(
        space_all.iter().map(|line| line.0).collect::<Vec<_>>(),
        [40.0, 40.0]
    );
}

fn vertical_spacing_fonts(family: &str) -> (Limits, FontCollection) {
    use crate::font::{FontFaceDescriptor, FontOptions};

    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
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
                family: family.into(),
                ..Default::default()
            },
        )
        .unwrap();
    (limits, fonts)
}

#[test]
fn colon_punctuation_class_follows_the_shaped_glyph() {
    use crate::line::punctuation::PunctuationClass as P;
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle, TextSpacingTrim};

    let (limits, fonts) = vertical_spacing_fonts("Colon CJK");
    let punctuation = |mode, lang: &str, text: &str| {
        let style = ParagraphStyle {
            writing_mode: mode,
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Colon CJK".into())],
                font_size: 20.0,
                lang: Some(lang.into()),
                text_spacing_trim: TextSpacingTrim::Normal,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let crate::LineResult::Line(line) = paragraph.next_line(
            &mut crate::LayoutContext::new(),
            paragraph.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(400.0),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!("expected one line")
        };
        (paragraph.data.punctuation[1], line.inline_size())
    };
    let layout = |mode, lang: &str, text: &str| {
        let (punctuation, size) = punctuation(mode, lang, text);
        (punctuation.class, size)
    };

    // Rotated or centered glyphs keep the language convention.
    assert_eq!(
        layout(WritingMode::HorizontalTb, "ja", "水；（水"),
        (P::Middle, 70.0)
    );
    assert_eq!(
        layout(WritingMode::HorizontalTb, "zh-Hans", "水；（水"),
        (P::Closing, 70.0)
    );
    assert_eq!(
        layout(WritingMode::VerticalRl, "ja", "水：（水"),
        (P::Middle, 70.0)
    );
    // Upright vertical colons and semicolons fill more than half of the
    // vertical em, so they are neither closing nor middle punctuation and
    // leave the following bracket fullwidth.
    for (lang, text) in [
        ("ja", "水；（水"),
        ("zh-Hans", "水：（水"),
        ("zh-Hans", "水；（水"),
        ("zh-Hant", "水：（水"),
        ("zh-Hant", "水；（水"),
    ] {
        let (colon, size) = punctuation(WritingMode::VerticalRl, lang, text);
        assert_eq!((colon.class, size), (P::Other, 80.0), "{lang} {text}");
        assert_eq!(
            (colon.left, colon.right),
            (LayoutUnit::ZERO, LayoutUnit::ZERO)
        );
    }
    // The dot convention also comes from the glyph in both flows.
    assert_eq!(
        layout(WritingMode::HorizontalTb, "zh-Hant", "水。（水"),
        (P::Middle, 70.0)
    );
    assert_eq!(
        layout(WritingMode::VerticalRl, "ja", "水。（水"),
        (P::Closing, 70.0)
    );
    assert_eq!(
        layout(WritingMode::VerticalRl, "zh-Hant", "水。（水"),
        (P::Middle, 70.0)
    );
}

#[test]
fn vertical_normal_keeps_wrapped_opening_from_another_inline_fullwidth() {
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle, TextSpacingTrim};

    let (limits, fonts) = vertical_spacing_fonts("Wrapped CJK");
    let root = InlineStyle {
        font_families: vec![FontFamily::Named("Wrapped CJK".into())],
        font_size: 20.0,
        lang: Some("ja".into()),
        text_spacing_trim: TextSpacingTrim::Normal,
        ..Default::default()
    };
    let style = ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        root: root.clone(),
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "水水水）");
    builder.open_inline(NodeId(2), &root, InlineEdges::default());
    builder.push_text(TextSource::Generated { node: NodeId(3) }, "（水");
    builder.close_inline();
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    let mut sizes = Vec::new();
    let mut token = paragraph.start_token();
    while let crate::LineResult::Line(line) = paragraph.next_line(
        &mut crate::LayoutContext::new(),
        token,
        &Default::default(),
        &crate::LineConstraint::new(80.0),
        &crate::AtomicSizes::EMPTY,
    ) {
        sizes.push(line.inline_size());
        token = line.break_token();
    }
    // `normal` does not trim the start of a line, even though the opening
    // bracket would pair with the closing bracket on the previous line.
    assert_eq!(sizes, [80.0, 40.0]);
}

#[test]
fn colon_punctuation_class_follows_synthetic_and_sideways_glyphs() {
    use crate::line::punctuation::PunctuationClass as P;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle, TextOrientation, TextSpacingTrim};

    let (limits, fonts) = vertical_spacing_fonts("Synthetic colon CJK");
    let layout = |orientation, bold_colon: bool| {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Synthetic colon CJK".into())],
            font_size: 20.0,
            lang: Some("ja".into()),
            text_spacing_trim: TextSpacingTrim::Normal,
            text_orientation: orientation,
            ..Default::default()
        };
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            root: root.clone(),
            ..Default::default()
        };
        let mut colon = root.clone();
        if bold_colon {
            colon.font_weight = 700.0;
        }
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.open_inline(NodeId(1), &colon, InlineEdges::default());
        builder.push_text(TextSource::Generated { node: NodeId(2) }, "水；");
        builder.close_inline();
        builder.push_text(TextSource::Generated { node: NodeId(3) }, "（水");
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        let crate::LineResult::Line(line) = paragraph.next_line(
            &mut crate::LayoutContext::new(),
            paragraph.start_token(),
            &Default::default(),
            &crate::LineConstraint::new(400.0),
            &crate::AtomicSizes::EMPTY,
        ) else {
            panic!("expected one line")
        };
        let synthetic = paragraph.data.runs.iter().any(|run| run.instance.embolden);
        (
            paragraph.data.punctuation[1].class,
            line.inline_size(),
            synthetic,
        )
    };

    // Synthetic emboldening keeps the upright semicolon filling the em.
    assert_eq!(layout(TextOrientation::Mixed, true), (P::Other, 80.0, true));
    // Sideways text measures the rotated horizontal glyph instead.
    assert_eq!(
        layout(TextOrientation::Sideways, false),
        (P::Middle, 70.0, false)
    );
}

#[test]
fn repeated_mixed_runs_prepare_each_instance_once_per_shape_call() {
    for text in ["a水b", "a😀b"] {
        let (p, fonts, style) = mixed_instance_paragraph(&text.repeat(16));
        let limits = Limits::default();
        assert_eq!(p.data.shape_items.len(), 33);
        for _ in 0..2 {
            instance::INSTANCE_BUILDS.with(|count| count.set(0));
            instance::COORDINATE_INSTANCE_BUILDS.with(|count| count.set(0));
            let (glyphs, runs) = shape_items(
                &mut crate::LayoutContext::new(),
                &p.data.shape_items,
                &p.data.styles,
                &fonts,
                style.writing_mode,
                &limits,
                &mut crate::limits::WarningSink::default(),
                &mut Saturation::default(),
            )
            .unwrap();
            assert_eq!(glyphs.id, p.data.glyphs.id);
            assert_eq!(glyphs.cluster, p.data.glyphs.cluster);
            assert_eq!(glyphs.advance, p.data.glyphs.advance);
            assert_eq!(glyphs.pen, p.data.glyphs.pen);
            let builds = instance::INSTANCE_BUILDS.with(std::cell::Cell::get);
            assert_eq!(builds, 2);
            let count = instance::COORDINATE_INSTANCE_BUILDS.with(std::cell::Cell::get);
            println!(
                "text={text}, mixed items={}, runs={}, instance builds={builds}, coordinate builds={count}",
                p.data.shape_items.len(),
                runs.len()
            );
            assert_eq!(count, 2, "each separate call must resolve both faces once");
            assert!(Arc::ptr_eq(&runs[0].instance, &runs[2].instance));
            assert_ne!(runs[0].shaping_input, runs[2].shaping_input);
        }
    }
}

fn mixed_instance_paragraph(
    text: &str,
) -> (
    crate::Paragraph,
    FontCollection,
    crate::style::ParagraphStyle,
) {
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        ("Latin", variable_latin(2).as_slice()),
        ("CJK", crate::test_support::fonts::CJK),
        ("Emoji", crate::test_support::fonts::EMOJI_COLOR),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![
                FontFamily::Named("Latin".into()),
                FontFamily::Named("CJK".into()),
                FontFamily::Named("Emoji".into()),
            ],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    let p = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    (p, fonts, style)
}

#[test]
fn mixed_instance_reuse_preserves_distinct_conditions_and_warning_effects() {
    use crate::style::{FontFeature, FontMetricKind, FontSizeAdjust, FontVariation};
    let (p, fonts, _) = mixed_instance_paragraph(&"a水b".repeat(8));
    for condition in 0..18 {
        let mut styles = p.data.styles.clone();
        if condition == 14 {
            styles[0].font_variations = vec![FontVariation {
                tag: *b"TEST",
                value: 0.0,
            }];
        }
        if condition == 15 {
            styles[0].font_size_adjust = Some(FontSizeAdjust {
                metric: FontMetricKind::ExHeight,
                value: 0.0,
            });
        }
        let mut changed = styles[0].clone();
        match condition {
            0 => changed.font_size = 31.0,
            1 => {
                changed.font_size_adjust = Some(FontSizeAdjust {
                    metric: FontMetricKind::ExHeight,
                    value: 0.75,
                })
            }
            2 => {
                changed.font_size_adjust = Some(FontSizeAdjust {
                    metric: FontMetricKind::IcHeight,
                    value: 0.5,
                })
            }
            3 => {
                changed.font_size_adjust = Some(FontSizeAdjust {
                    metric: FontMetricKind::ExHeight,
                    value: f32::MAX,
                })
            }
            4 => {
                changed.font_variations = vec![FontVariation {
                    tag: *b"opsz",
                    value: 21.0,
                }]
            }
            5 => changed.font_optical_sizing = false,
            6 => changed.lang = Some("tr".into()),
            7 => {
                changed.font_features = vec![FontFeature {
                    tag: *b"liga",
                    value: 0,
                }]
            }
            // Paint-only changes should be eligible for sharing.
            8 => changed.paint.color = [17, 23, 29, 255],
            9..=13 => (),
            14 => changed.font_variations[0].value = -0.0,
            15 => changed.font_size_adjust.as_mut().unwrap().value = -0.0,
            16..=17 => (),
            _ => unreachable!(),
        }
        styles.push(changed);
        let mut items = p.data.shape_items.clone();
        let index = styles.len() as u32 - 1;
        for (i, item) in items.iter_mut().enumerate() {
            if condition == 16 {
                Arc::make_mut(item.font.as_mut().unwrap())
                    .variations
                    .push(FontVariation {
                        tag: *b"TEST",
                        value: 0.0,
                    });
            }
            if condition == 17 {
                Arc::make_mut(item.font.as_mut().unwrap()).skew = Some(0.0);
            }
            if i % 3 != 0 {
                continue;
            }
            item.style = index;
            match condition {
                9 => item.script = *b"Zyyy",
                10 => Arc::make_mut(item.font.as_mut().unwrap()).embolden = true,
                11 => Arc::make_mut(item.font.as_mut().unwrap()).skew = Some(12.0),
                12 => Arc::make_mut(item.font.as_mut().unwrap())
                    .variations
                    .push(FontVariation {
                        tag: *b"wght",
                        value: 700.0,
                    }),
                13 => item.width_feature = Some(*b"hwid"),
                16 => {
                    Arc::make_mut(item.font.as_mut().unwrap())
                        .variations
                        .last_mut()
                        .unwrap()
                        .value = -0.0
                }
                17 => Arc::make_mut(item.font.as_mut().unwrap()).skew = Some(-0.0),
                _ => (),
            }
        }
        for max in [None, Some(0), Some(1), Some(3)] {
            for mode in [WritingMode::HorizontalTb, WritingMode::VerticalRl] {
                let limits = Limits {
                    max_warnings: max,
                    ..Limits::default()
                };
                let shape = |bypass| {
                    struct Reset(bool);
                    impl Drop for Reset {
                        fn drop(&mut self) {
                            instance_cache::BYPASS.with(|v| v.set(self.0));
                        }
                    }
                    let _reset = Reset(instance_cache::BYPASS.with(|v| v.replace(bypass)));
                    let mut warnings = crate::limits::WarningSink::new(max);
                    warnings.push(
                        crate::limits::WarningKind::Unsupported,
                        "before instance resolution",
                    );
                    let mut sat = Saturation::default();
                    instance::COORDINATE_INSTANCE_BUILDS.with(|count| count.set(0));
                    let (glyphs, runs) = shape_items(
                        &mut crate::LayoutContext::new(),
                        &items,
                        &styles,
                        &fonts,
                        mode,
                        &limits,
                        &mut warnings,
                        &mut sat,
                    )
                    .unwrap();
                    let count = instance::COORDINATE_INSTANCE_BUILDS.with(std::cell::Cell::get);
                    (
                        glyphs,
                        runs.iter()
                            .map(|run| format!("{run:?}"))
                            .collect::<Vec<_>>(),
                        format!("{:?}", warnings.take()),
                        format!("{sat:?}"),
                        count,
                    )
                };
                let (cached, runs, warnings, sat, count) = shape(false);
                let (reference, reference_runs, reference_warnings, reference_sat, reference_count) =
                    shape(true);
                assert_eq!(
                    cached.id, reference.id,
                    "condition={condition}, max={max:?}, mode={mode:?}"
                );
                assert_eq!(cached.advance, reference.advance);
                assert_eq!(cached.cluster, reference.cluster);
                assert_eq!(cached.pen, reference.pen);
                assert_eq!(cached.offset_inline, reference.offset_inline);
                assert_eq!(cached.offset_block, reference.offset_block);
                assert_eq!(cached.flags, reference.flags);
                assert_eq!(cached.spacing, reference.spacing);
                assert_eq!(cached.leading, reference.leading);
                assert_eq!(runs.len(), reference_runs.len());
                for (run, reference) in runs.iter().zip(&reference_runs) {
                    assert_eq!(
                        run, reference,
                        "condition={condition}, max={max:?}, mode={mode:?}"
                    );
                }
                assert_eq!(warnings, reference_warnings);
                if max.is_none() && condition == 3 {
                    assert!(warnings.contains("adjusted font size clamped to 1e6 px"));
                }
                if max.is_none() && condition == 2 {
                    assert!(warnings.contains("font-size-adjust metric unavailable"));
                }
                assert_eq!(sat, reference_sat);
                if condition == 8 {
                    assert_eq!(count, 2, "paint-only changes share preparation");
                }
                assert!(
                    count < reference_count,
                    "condition={condition}: reuse must actually occur"
                );
            }
        }
    }
}

fn variable_latin(axis_count: u16) -> Vec<u8> {
    let bytes = crate::test_support::fonts::LATIN;
    let mut tables = Vec::new();
    for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
        let at = 12 + n * 16;
        let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((
            bytes[at..at + 4].try_into().unwrap(),
            bytes[offset..offset + len].to_vec(),
        ));
    }
    let mut fvar = Vec::new();
    for field in [1u16, 0, 16, 2, axis_count, 20, 0, 4 + axis_count * 4] {
        fvar.extend(field.to_be_bytes());
    }
    for index in 0..axis_count {
        let (tag, values) = match index {
            0 => (*b"wght", [100i32, 400, 900]),
            1 => (*b"opsz", [8, 12, 72]),
            _ => ((0x70000000 + u32::from(index)).to_be_bytes(), [0, 0, 1]),
        };
        fvar.extend(tag);
        for value in values {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
    }
    tables.push((*b"fvar", fvar));
    crate::font::sfnt::build_sfnt(&tables)
}

#[test]
fn high_axis_default_instance_bypasses_reuse_without_retaining_hidden_coordinate_heap() {
    use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
    let fonts = FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let id = fonts
        .register_face(
            variable_latin(12),
            0,
            crate::font::FontFaceDescriptor {
                family: "Many axes".into(),
                weight: (100.0, 900.0),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![
                FontFamily::Named("Many axes".into()),
                FontFamily::Named("CJK".into()),
                FontFamily::Named("Emoji".into()),
            ],
            font_optical_sizing: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = crate::ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        &"a水".repeat(4),
    );
    let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
    assert_eq!(p.data.shape_items.len(), 8);
    let runs: Vec<_> = p.data.runs.iter().filter(|run| run.font == id).collect();
    assert_eq!(runs.len(), 4);
    assert!(runs.iter().all(|run| run.instance.coords.is_empty()));
    assert!(
        runs.windows(2)
            .all(|runs| !Arc::ptr_eq(&runs[0].instance, &runs[1].instance))
    );
}
