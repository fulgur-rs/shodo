mod common;

use common::{first_line, glyphs};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::{BaselineKind, Direction, PhysicalConverter, PhysicalSize, WritingMode};
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, FontFeature, FontKerning, InlineStyle, LineOptions, ParagraphStyle,
    TextCombineUpright, TextOrientation, TextSpacingTrim, VerticalAlign,
};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn style(mode: WritingMode, orientation: TextOrientation) -> ParagraphStyle {
    ParagraphStyle {
        writing_mode: mode,
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 16.0,
            lang: Some("ja".into()),
            text_orientation: orientation,
            text_spacing_trim: TextSpacingTrim::SpaceAll,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn paragraph(style: &ParagraphStyle, text: &str) -> Paragraph {
    let limits = Limits::default();
    build_paragraph(style, &limits, |builder| {
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
    })
}

fn build_paragraph(
    style: &ParagraphStyle,
    limits: &Limits,
    add: impl FnOnce(&mut ParagraphBuilder),
) -> Paragraph {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(style, limits);
    add(&mut builder);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

#[test]
fn vertical_shaping_uses_actual_vert_substitutions() {
    // FontTools inspected this pinned GSUB: 「114→672, 」115→673, 、103→651.
    // vmtx records advance 1000 for all three at UPEM1000, hence 16px.
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let p = paragraph(&style(mode, TextOrientation::Mixed), "「」、");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let glyphs = glyphs(&line);
        assert_eq!(
            glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![672, 673, 651]
        );
        assert_eq!(
            glyphs.iter().map(|g| g.advance).collect::<Vec<_>>(),
            vec![16.0; 3]
        );
    }
}

#[test]
fn vertical_glyph_origin_uses_vorg_once() {
    // CFF VORG is 880 units and horizontal advance is 1000. Harfrust's
    // vertical origin is (500,880), giving x_offset=-500/y_offset=-880.
    // Quantization is 1/64px: 880*16/1000 → 14.078125.
    for (mode, block_offset) in [
        (WritingMode::VerticalRl, 8.0),
        (WritingMode::VerticalLr, -8.0),
    ] {
        let p = paragraph(&style(mode, TextOrientation::Upright), "水水");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let glyphs = glyphs(&line);
        assert_eq!(glyphs.len(), 2);
        assert_eq!(glyphs[0].inline_position, 14.078125);
        assert_eq!(glyphs[1].inline_position, 30.078125);
        assert_eq!(glyphs[0].block_offset, block_offset);
        assert_eq!(glyphs[1].block_offset, block_offset);
    }
}

#[test]
fn sideways_writing_keeps_horizontal_substitutions_and_advances() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let p = paragraph(&style(mode, TextOrientation::Upright), "「」、");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let glyphs = glyphs(&line);
        assert_eq!(
            glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![114, 115, 103]
        );
        assert_eq!(
            glyphs.iter().map(|g| g.advance).collect::<Vec<_>>(),
            vec![16.0; 3]
        );
    }
}

#[test]
fn vertical_author_features_override_the_defaults() {
    for (features, expected) in [
        (
            vec![FontFeature {
                tag: *b"vert",
                value: 0,
            }],
            vec![114, 115, 103],
        ),
        (
            vec![FontFeature {
                tag: *b"vrt2",
                value: 1,
            }],
            vec![672, 673, 651],
        ),
    ] {
        let mut style = style(WritingMode::VerticalRl, TextOrientation::Upright);
        style.root.font_features = features;
        let p = paragraph(&style, "「」、");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(
            glyphs(&line).iter().map(|g| g.id).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn physical_converter_maps_points_and_vectors_without_mirroring_outline() {
    let size = PhysicalSize {
        width: 100.0,
        height: 80.0,
    };
    for (mode, direction, origin, inline, block) in [
        (
            WritingMode::HorizontalTb,
            Direction::Ltr,
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
        ),
        (
            WritingMode::HorizontalTb,
            Direction::Rtl,
            (100.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
        ),
        (
            WritingMode::VerticalRl,
            Direction::Ltr,
            (100.0, 0.0),
            (0.0, 1.0),
            (-1.0, 0.0),
        ),
        (
            WritingMode::VerticalRl,
            Direction::Rtl,
            (100.0, 80.0),
            (0.0, -1.0),
            (-1.0, 0.0),
        ),
        (
            WritingMode::VerticalLr,
            Direction::Ltr,
            (0.0, 0.0),
            (0.0, 1.0),
            (1.0, 0.0),
        ),
        (
            WritingMode::VerticalLr,
            Direction::Rtl,
            (0.0, 80.0),
            (0.0, -1.0),
            (1.0, 0.0),
        ),
        (
            WritingMode::SidewaysRl,
            Direction::Ltr,
            (100.0, 0.0),
            (0.0, 1.0),
            (-1.0, 0.0),
        ),
        (
            WritingMode::SidewaysRl,
            Direction::Rtl,
            (100.0, 80.0),
            (0.0, -1.0),
            (-1.0, 0.0),
        ),
        (
            WritingMode::SidewaysLr,
            Direction::Ltr,
            (0.0, 80.0),
            (0.0, -1.0),
            (1.0, 0.0),
        ),
        (
            WritingMode::SidewaysLr,
            Direction::Rtl,
            (0.0, 0.0),
            (0.0, 1.0),
            (1.0, 0.0),
        ),
    ] {
        let converter = PhysicalConverter::new(mode, direction, size);
        assert_eq!(converter.point(0.0, 0.0), origin, "{mode:?}/{direction:?}");
        assert_eq!(converter.vector(1.0, 0.0), inline, "{mode:?}/{direction:?}");
        assert_eq!(converter.vector(0.0, 1.0), block, "{mode:?}/{direction:?}");
        assert_eq!(converter.logical_point(origin.0, origin.1), (0.0, 0.0));
    }
}

#[test]
fn public_vertical_glyph_matrix_keeps_font_outline_upright() {
    // Size has already been applied to outline path. Its local x/y axes are
    // right/down. The converter and run matrix together must keep both axes.
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut style = style(mode, TextOrientation::Upright);
            style.direction = direction;
            let p = paragraph(&style, "水");
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            let run = line
                .fragments()
                .find_map(|f| match f {
                    Fragment::GlyphRun(run) => Some(run),
                    _ => None,
                })
                .unwrap();
            assert_eq!(run.orientation(), shodo::GlyphOrientation::Upright);
            let t = run.glyph_transform();
            let converter = PhysicalConverter::new(
                mode,
                line.used_direction(),
                PhysicalSize {
                    width: 100.0,
                    height: 80.0,
                },
            );
            assert_eq!(converter.vector(t.inline_x, t.block_x), (1.0, 0.0));
            assert_eq!(converter.vector(t.inline_y, t.block_y), (0.0, 1.0));
            let glyph = run.glyphs().next().unwrap();
            let (inline, block) = run.glyph_origin(0).unwrap();
            assert_eq!(block, run.baseline() + glyph.block_offset);
            if line.used_direction() == Direction::Rtl {
                assert_eq!(inline, glyph.inline_position + 16.0);
            } else {
                assert_eq!(inline, glyph.inline_position);
            }
        }
    }
}

#[test]
fn public_glyph_transform_has_literal_physical_axes_in_every_writing_mode() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        for orientation in [
            TextOrientation::Mixed,
            TextOrientation::Upright,
            TextOrientation::Sideways,
        ] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let mut style = style(mode, orientation);
                style.direction = direction;
                let paragraph = paragraph(&style, "A");
                let line = first_line(
                    &paragraph,
                    100.0,
                    &LineOptions::default(),
                    &AtomicSizes::EMPTY,
                );
                let run = line
                    .fragments()
                    .find_map(|fragment| match fragment {
                        Fragment::GlyphRun(run) => Some(run),
                        _ => None,
                    })
                    .unwrap();
                let expected = match (mode, orientation) {
                    (WritingMode::HorizontalTb, _) => shodo::GlyphOrientation::Horizontal,
                    (
                        WritingMode::VerticalRl | WritingMode::VerticalLr,
                        TextOrientation::Upright,
                    ) => shodo::GlyphOrientation::Upright,
                    (WritingMode::SidewaysLr, _) => {
                        shodo::GlyphOrientation::SidewaysCounterClockwise
                    }
                    _ => shodo::GlyphOrientation::SidewaysClockwise,
                };
                assert_eq!(
                    run.orientation(),
                    expected,
                    "{mode:?}/{orientation:?}/{direction:?}"
                );
                let transform = run.glyph_transform();
                let converter = PhysicalConverter::new(
                    mode,
                    line.used_direction(),
                    PhysicalSize {
                        width: 100.0,
                        height: 80.0,
                    },
                );
                let axes = match expected {
                    shodo::GlyphOrientation::Horizontal
                    | shodo::GlyphOrientation::Upright
                    | shodo::GlyphOrientation::Combined => ((1.0, 0.0), (0.0, 1.0)),
                    shodo::GlyphOrientation::SidewaysClockwise => ((0.0, 1.0), (-1.0, 0.0)),
                    shodo::GlyphOrientation::SidewaysCounterClockwise => ((0.0, -1.0), (1.0, 0.0)),
                };
                assert_eq!(
                    converter.vector(transform.inline_x, transform.block_x),
                    axes.0,
                    "x axis {mode:?}/{orientation:?}/{direction:?}",
                );
                assert_eq!(
                    converter.vector(transform.inline_y, transform.block_y),
                    axes.1,
                    "y axis {mode:?}/{orientation:?}/{direction:?}",
                );
            }
        }
    }
}

#[test]
fn upright_vertical_text_uses_ltr_for_bidi_and_keeps_logical_source_order() {
    // The computed RTL value stays in the caller's style, but CSS upright
    // makes the used inline direction LTR and treats Hebrew as strong LTR.
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut style = style(mode, TextOrientation::Upright);
        style.direction = Direction::Rtl;
        let p = paragraph(&style, "אב");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.used_direction(), Direction::Ltr);
        let glyphs = glyphs(&line);
        assert_eq!(
            glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert!(glyphs[0].inline_position < glyphs[1].inline_position);
    }
}

#[test]
fn upright_arabic_isolated_forms_differ_from_mixed_joined_forms() {
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
            include_bytes!("../dev/fixtures/assets/fonts/arabic.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let ids = |orientation, text| {
        let mut style = style(WritingMode::VerticalRl, orientation);
        style.root.font_families = vec![FontFamily::Named("Shodo Fixture Arabic".into())];
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let line = first_line(
            &paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        glyphs(&line)
            .iter()
            .map(|glyph| glyph.id)
            .collect::<Vec<_>>()
    };
    let isolated = ids(TextOrientation::Upright, "ب");
    assert_eq!(isolated.len(), 1);
    assert_eq!(ids(TextOrientation::Upright, "بب"), vec![isolated[0]; 2]);
    assert_ne!(ids(TextOrientation::Mixed, "بب"), vec![isolated[0]; 2]);
}

#[test]
fn missing_font_keeps_upright_orientation_and_one_em_advances() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut style = style(mode, TextOrientation::Upright);
        style.root.font_families = vec![FontFamily::Named("Definitely absent".into())];
        style.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "水A");
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let line = first_line(
            &paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let runs: Vec<_> = line
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .collect();
        assert!(
            runs.iter()
                .all(|run| run.orientation() == shodo::GlyphOrientation::Upright)
        );
        let glyphs = glyphs(&line);
        assert_eq!(
            glyphs.iter().map(|glyph| glyph.id).collect::<Vec<_>>(),
            vec![0, 0]
        );
        assert_eq!(
            glyphs.iter().map(|glyph| glyph.advance).collect::<Vec<_>>(),
            vec![16.0, 16.0]
        );
    }
}

#[test]
fn latin_font_without_vertical_tables_synthesizes_ttb_advance_and_central_baseline() {
    // The pinned Latin face has UPEM=1000, A hmtx advance=639, no vhea/vmtx.
    // Harfrust synthesizes TTB advance from OS/2 typo ascent/descent 1069/−293.
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
            include_bytes!("../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for (orientation, expected_advance) in [
        (TextOrientation::Upright, 21.796875),
        (TextOrientation::Sideways, 10.21875),
    ] {
        let mut style = style(WritingMode::VerticalRl, orientation);
        style.root.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "A");
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let line = first_line(
            &paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let run = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.vertical_metrics(), None);
        assert_eq!(glyphs(&line)[0].advance, expected_advance);
        if orientation == TextOrientation::Upright {
            assert_eq!(line.block_size(), 16.0);
            assert_eq!(line.baseline(BaselineKind::Central), 8.0);
        }
    }
}

#[test]
fn mixed_vertical_line_keeps_distinct_font_instances_and_metrics() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let cjk = fonts
        .register_face(
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let latin = fonts
        .register_face(
            include_bytes!("../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    style.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
    let mut child = style.root.clone();
    child.font_families = vec![FontFamily::Named("Shodo Fixture Latin".into())];
    child.font_size = 20.0;
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder
        .push_text(TextSource::Generated { node: NodeId(1) }, "水")
        .open_inline(NodeId(2), &child, InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(3) }, "A")
        .close_inline();
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let line = first_line(
        &paragraph,
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    let runs = line
        .fragments()
        .filter_map(|fragment| match fragment {
            Fragment::GlyphRun(run) => Some(run),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].font(), cjk);
    assert_eq!(runs[1].font(), latin);
    assert_eq!(runs[0].font_size(), 16.0);
    assert_eq!(runs[1].font_size(), 20.0);
    assert_eq!(runs[0].orientation(), shodo::GlyphOrientation::Upright);
    assert_eq!(
        runs[1].orientation(),
        shodo::GlyphOrientation::SidewaysClockwise
    );
    assert_eq!(
        glyphs(&line).iter().map(|g| g.advance).collect::<Vec<_>>(),
        vec![16.0, 12.78125]
    );
    assert!(line.block_size() >= 29.375);
}

#[test]
fn vertical_planned_and_cached_lines_keep_glyphs_and_orientation() {
    let paragraph = paragraph(
        &style(WritingMode::VerticalRl, TextOrientation::Mixed),
        "水 A 水 A 水 A",
    );
    let options = LineOptions::default();
    let width = 40.0;
    let plan = paragraph.plan_breaks(
        &mut LayoutContext::new(),
        &options,
        width,
        &AtomicSizes::EMPTY,
    );
    let collect = |cx: &mut LayoutContext, planned| {
        let mut constraint = LineConstraint::new(width);
        constraint.break_plan = planned;
        let mut token = paragraph.start_token();
        let mut output = Vec::new();
        loop {
            match paragraph.next_line(cx, token, &options, &constraint, &AtomicSizes::EMPTY) {
                LineResult::Line(line) => {
                    token = line.break_token();
                    let orientations = line
                        .fragments()
                        .filter_map(|fragment| match fragment {
                            Fragment::GlyphRun(run) => {
                                Some((run.orientation(), run.glyph_transform()))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    output.push((line.text_range(), glyphs(&line), orientations));
                }
                LineResult::Done => break,
                other => panic!("{other:?}"),
            }
        }
        output
    };
    let fresh = collect(&mut LayoutContext::new(), None);
    assert!(fresh.len() > 1);
    let mut cached = LayoutContext::new();
    assert_eq!(collect(&mut cached, None), fresh);
    assert_eq!(collect(&mut cached, None), fresh);
    assert_eq!(collect(&mut LayoutContext::new(), Some(&plan)), fresh);
}

#[test]
fn vertical_float_and_height_retry_replay_the_same_glyphs() {
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
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder
        .push_text(TextSource::Generated { node: NodeId(1) }, "水")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "A水");
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let mut constraint = LineConstraint::new(100.0);
    let mut report = None;
    for _ in 0..2 {
        let LineResult::FloatEncountered {
            node,
            line_start,
            float_cursor,
            ..
        } = paragraph.next_line(
            &mut cx,
            paragraph.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        )
        else {
            panic!("expected the same float report before placement")
        };
        assert_eq!(node, NodeId(2));
        assert_eq!(line_start, paragraph.start_token());
        if let Some(previous) = report {
            assert_eq!(float_cursor, previous);
        }
        report = Some(float_cursor);
    }
    constraint.floats_placed_through = report;
    constraint.max_block_size = Some(1.0);
    assert!(matches!(
        paragraph.next_line(
            &mut cx,
            paragraph.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ),
        LineResult::BlockSizeExceeded { .. }
    ));
    constraint.max_block_size = None;
    let get = |cx: &mut LayoutContext| {
        let LineResult::Line(line) = paragraph.next_line(
            cx,
            paragraph.start_token(),
            &options,
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected replayed vertical line")
        };
        let orientations = line
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run.orientation()),
                _ => None,
            })
            .collect::<Vec<_>>();
        (line.text_range(), glyphs(&line), orientations)
    };
    let first = get(&mut cx);
    assert_eq!(first, get(&mut cx));
    assert_eq!(first, get(&mut LayoutContext::new()));
}

#[test]
fn vertical_first_line_font_change_does_not_leak_into_following_lines() {
    let mut style = style(WritingMode::VerticalRl, TextOrientation::Upright);
    let mut first = style.root.clone();
    first.font_size = 24.0;
    style.first_line = Some(first);
    let paragraph = paragraph(&style, "水水水");
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let constraint = LineConstraint::new(30.0);
    let LineResult::Line(first) = paragraph.next_line(
        &mut cx,
        paragraph.start_token(),
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected first vertical line")
    };
    assert_eq!(glyphs(&first)[0].advance, 24.0);
    assert_eq!(first.baseline(BaselineKind::Central), 12.0);
    let LineResult::Line(second) = paragraph.next_line(
        &mut cx,
        first.break_token(),
        &options,
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected second vertical line")
    };
    assert_eq!(glyphs(&second)[0].advance, 16.0);
    assert_eq!(second.baseline(BaselineKind::Central), 8.0);
    assert!(second.fragments().all(|fragment| match fragment {
        Fragment::GlyphRun(run) => run.orientation() == shodo::GlyphOrientation::Upright,
        _ => true,
    }));
}

#[test]
fn small_vertical_shaping_windows_preserve_glyphs_and_orientation() {
    let style = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    let text = "水水A\u{0301}水水";
    let normal = paragraph(&style, text);
    let small_limits = Limits {
        max_shaping_run_bytes: Some(3),
        ..Limits::default()
    };
    let fonts = FontCollection::with_options(
        &small_limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &small_limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    let limited = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let snapshot = |paragraph: &Paragraph| {
        let line = first_line(
            paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let orientations = line
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some((run.orientation(), run.glyphs().count())),
                _ => None,
            })
            .flat_map(|(orientation, count)| std::iter::repeat_n(orientation, count))
            .collect::<Vec<_>>();
        (glyphs(&line), orientations)
    };
    assert_eq!(snapshot(&normal), snapshot(&limited));
    let glyph_limits = Limits {
        max_shaped_glyphs: Some(2),
        max_shaping_run_bytes: Some(3),
        ..Limits::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &glyph_limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    let error = builder
        .build(&mut LayoutContext::new(), &fonts)
        .unwrap_err();
    assert_eq!(error.kind, shodo::limits::LimitKind::ShapedGlyphs);
}

#[test]
fn vertical_lr_uses_right_line_over_with_asymmetric_vhea() {
    let mut bytes = include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let vhea = (0..count)
        .find_map(|index| {
            let record = 12 + index * 16;
            (&bytes[record..record + 4] == b"vhea").then(|| {
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize
            })
        })
        .unwrap();
    bytes[vhea + 4..vhea + 6].copy_from_slice(&600i16.to_be_bytes());
    bytes[vhea + 6..vhea + 8].copy_from_slice(&(-400i16).to_be_bytes());
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
            bytes,
            0,
            FontFaceDescriptor {
                family: "Asymmetric CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for (mode, expected_baseline, expected_over, expected_under) in [
        (WritingMode::VerticalRl, 9.59375, 0.0, 16.0),
        (WritingMode::VerticalLr, 6.40625, 16.0, 0.0),
    ] {
        let mut style = style(mode, TextOrientation::Upright);
        style.root.font_families = vec![FontFamily::Named("Asymmetric CJK".into())];
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "水");
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let line = first_line(
            &paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let run = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        assert_eq!(line.block_size(), 16.0);
        assert_eq!(run.baseline(), expected_baseline, "{mode:?}");
        assert_eq!(line.baseline(BaselineKind::Central), 8.0, "{mode:?}");
        let metrics = line.metrics();
        assert!((metrics.text_over - expected_over).abs() < 1.0 / 64.0);
        assert!((metrics.text_under - expected_under).abs() < 1.0 / 64.0);
        let lines = [line];
        let selection = LineLayout::new(&lines).selection_rects(
            TextPosition {
                line: 0,
                offset: 0,
                affinity: Affinity::Downstream,
            },
            TextPosition {
                line: 0,
                offset: "水".len() as u32,
                affinity: Affinity::Upstream,
            },
        );
        assert_eq!(selection.len(), 1);
        assert!(selection[0].block_start.abs() < 1.0 / 64.0, "{mode:?}");
        assert!(
            (selection[0].block_size - 16.0).abs() < 1.0 / 64.0,
            "{mode:?}"
        );
    }
}

#[test]
fn vertical_line_over_and_under_align_inline_boxes_in_both_column_directions() {
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
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for align in [VerticalAlign::Top, VerticalAlign::Bottom] {
            let style = style(mode, TextOrientation::Upright);
            let mut child = style.root.clone();
            child.font_size = 8.0;
            child.vertical_align = align;
            let mut builder = ParagraphBuilder::new(&style, &limits);
            builder
                .push_text(TextSource::Generated { node: NodeId(1) }, "水")
                .open_inline(NodeId(2), &child, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "水")
                .close_inline();
            let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            let line = first_line(
                &paragraph,
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            let child_run = line
                .fragments()
                .find_map(|fragment| match fragment {
                    Fragment::GlyphRun(run) if run.node() == Some(NodeId(3)) => Some(run),
                    _ => None,
                })
                .unwrap();
            let metrics = child_run.vertical_metrics().unwrap();
            let over = if mode == WritingMode::VerticalLr {
                child_run.baseline() + metrics.ascent
            } else {
                child_run.baseline() - metrics.ascent
            };
            let under = if mode == WritingMode::VerticalLr {
                child_run.baseline() - metrics.descent
            } else {
                child_run.baseline() + metrics.descent
            };
            let expected_over = if mode == WritingMode::VerticalLr {
                line.block_size()
            } else {
                0.0
            };
            let expected_under = if mode == WritingMode::VerticalLr {
                0.0
            } else {
                line.block_size()
            };
            if align == VerticalAlign::Top {
                assert!((over - expected_over).abs() < 1.0 / 64.0, "{mode:?}");
            } else {
                assert!((under - expected_under).abs() < 1.0 / 64.0, "{mode:?}");
            }
        }
    }
}

#[test]
fn vertical_text_edges_align_to_parent_vertical_font_metrics() {
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
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for align in [VerticalAlign::TextTop, VerticalAlign::TextBottom] {
            let style = style(mode, TextOrientation::Upright);
            let mut child = style.root.clone();
            child.font_size = 8.0;
            child.vertical_align = align;
            let mut builder = ParagraphBuilder::new(&style, &limits);
            builder
                .push_text(TextSource::Generated { node: NodeId(1) }, "水")
                .open_inline(NodeId(2), &child, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "水")
                .close_inline();
            let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            let line = first_line(
                &paragraph,
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            let child_run = line
                .fragments()
                .find_map(|fragment| match fragment {
                    Fragment::GlyphRun(run) if run.node() == Some(NodeId(3)) => Some(run),
                    _ => None,
                })
                .unwrap();
            let v = child_run.vertical_metrics().unwrap();
            let over = if mode == WritingMode::VerticalLr {
                child_run.baseline() + v.ascent
            } else {
                child_run.baseline() - v.ascent
            };
            let under = if mode == WritingMode::VerticalLr {
                child_run.baseline() - v.descent
            } else {
                child_run.baseline() + v.descent
            };
            assert_eq!(line.block_size(), 16.0, "{mode:?}/{align:?}");
            if align == VerticalAlign::TextTop {
                assert_eq!(
                    over,
                    if mode == WritingMode::VerticalLr {
                        16.0
                    } else {
                        0.0
                    }
                );
            } else {
                assert_eq!(
                    under,
                    if mode == WritingMode::VerticalLr {
                        0.0
                    } else {
                        16.0
                    }
                );
            }
        }
    }
}

#[test]
fn vertical_atomic_without_baseline_uses_margin_box_centre() {
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
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let style = style(mode, TextOrientation::Upright);
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder
            .push_text(TextSource::Generated { node: NodeId(1) }, "水")
            .push_atomic(NodeId(2), &style.root, InlineEdges::default());
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(
            paragraph.required_baseline(NodeId(2)),
            Some(BaselineKind::Central)
        );
        let mut sizes = AtomicSizes::new();
        sizes.insert(
            NodeId(2),
            AtomicSize {
                inline_size: 10.0,
                block_size: 20.0,
                ..Default::default()
            },
        );
        let line = first_line(&paragraph, 100.0, &LineOptions::default(), &sizes);
        let atomic = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::Atomic(atomic) => Some(atomic),
                _ => None,
            })
            .unwrap();
        assert_eq!(line.block_size(), 20.0);
        assert_eq!(atomic.baseline, 10.0);
        assert_eq!(atomic.margin_rect.block_start, 0.0);
        assert_eq!(atomic.margin_rect.block_size, 20.0);
    }
}

#[test]
fn vertical_grapheme_shared_across_nodes_keeps_one_orientation_and_owner() {
    let style = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    let unsplit = paragraph(&style, "A\u{0301}");
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
            include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder
        .push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "A",
        )
        .push_text(
            TextSource::Dom {
                node: NodeId(2),
                offset: 0,
            },
            "\u{0301}",
        );
    let split = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let snapshot = |paragraph: &Paragraph| {
        let line = first_line(
            paragraph,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let runs = line
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some((run.node(), run.orientation())),
                _ => None,
            })
            .collect::<Vec<_>>();
        (glyphs(&line), runs)
    };
    let single = snapshot(&unsplit);
    let divided = snapshot(&split);
    assert_eq!(single.0, divided.0);
    assert_eq!(
        divided.1,
        vec![(Some(NodeId(1)), shodo::GlyphOrientation::SidewaysClockwise)]
    );
}

#[test]
fn combine_all_digit_sequences_take_one_em_in_vertical_modes() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for text in ["1", "12", "123", "1234", "12345"] {
            let mut style = style(mode, TextOrientation::Mixed);
            style.root.text_combine_upright = TextCombineUpright::All;
            style.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let paragraph = paragraph(&style, text);
            let line = first_line(
                &paragraph,
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            assert_eq!(line.inline_size(), 16.0, "{mode:?}/{text}");
            assert_eq!(glyphs(&line).len(), text.len(), "{mode:?}/{text}");
            assert_eq!(line.baseline(BaselineKind::Central), 8.0, "{mode:?}/{text}");
        }
    }
}

#[test]
fn combine_all_does_not_change_horizontal_or_sideways_modes() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::SidewaysRl,
        WritingMode::SidewaysLr,
    ] {
        let normal = paragraph(&style(mode, TextOrientation::Mixed), "1234");
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        let combined = paragraph(&combined, "1234");
        let ordinary = first_line(&normal, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let unchanged = first_line(
            &combined,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(glyphs(&ordinary), glyphs(&unchanged), "{mode:?}");
        assert_eq!(ordinary.inline_size(), unchanged.inline_size(), "{mode:?}");
    }
}

#[test]
fn combine_all_keeps_horizontal_glyphs_and_centers_their_paint_axes() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for text in ["1", "12", "1234", "12345"] {
            let mut combined = style(mode, TextOrientation::Mixed);
            combined.root.text_combine_upright = TextCombineUpright::All;
            combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let line = first_line(
                &paragraph(&combined, text),
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            let run = match line.fragment(0).unwrap() {
                Fragment::GlyphRun(run) => run,
                _ => panic!("combined glyph run"),
            };
            let natural: f32 = run.clusters().map(|cluster| cluster.shaping_advance).sum();
            let scale = (16.0 / natural).min(1.0);
            assert_eq!(run.orientation(), shodo::GlyphOrientation::Combined);
            let converter = PhysicalConverter::new(
                mode,
                line.used_direction(),
                PhysicalSize {
                    width: 100.0,
                    height: 100.0,
                },
            );
            let transform = run.glyph_transform();
            let x = converter.vector(transform.inline_x, transform.block_x);
            assert!(
                (x.0 - scale).abs() < 0.001,
                "{mode:?}/{text}: {x:?}/{scale}"
            );
            assert_eq!(x.1, 0.0);
            assert_eq!(
                converter.vector(transform.inline_y, transform.block_y),
                (0.0, 1.0)
            );
            let origins: Vec<_> = (0..run.glyphs().len())
                .map(|i| run.glyph_origin(i).unwrap())
                .collect();
            assert!(
                origins
                    .iter()
                    .all(|origin| (origin.0 - origins[0].0).abs() < 0.001),
                "horizontal baseline {origins:?}"
            );
            let sign = if mode == WritingMode::VerticalRl {
                -1.0
            } else {
                1.0
            };
            let expected =
                line.metrics().baseline + sign * (((16.0 - natural * scale) / 2.0) - 8.0);
            assert!(
                (origins[0].1 - expected).abs() < 0.001,
                "center {mode:?}/{text}: {origins:?}/{expected}"
            );
        }
    }
}

#[test]
fn combine_all_ignores_internal_letter_spacing() {
    let mut combined = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    combined.root.text_combine_upright = TextCombineUpright::All;
    combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
    let normal = first_line(
        &paragraph(&combined, "1234"),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    combined.root.letter_spacing = 10.0;
    let tracked = first_line(
        &paragraph(&combined, "1234"),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(tracked.inline_size(), 16.0);
    assert_eq!(glyphs(&tracked), glyphs(&normal));
}

#[test]
fn combine_all_reverts_only_applied_fullwidth_for_multiple_characters() {
    use shodo::style::TextTransform;
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        combined.root.font_features = vec![FontFeature {
            tag: *b"hwid",
            value: 0,
        }];
        for text in ["12", "ｶﾞ12"] {
            let plain = first_line(
                &paragraph(&combined, text),
                100.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            let mut fullwidth = combined.clone();
            fullwidth.root.text_transform = TextTransform::FullWidth;
            let p = paragraph(&fullwidth, text);
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(
                glyphs(&line).iter().map(|g| g.id).collect::<Vec<_>>(),
                glyphs(&plain).iter().map(|g| g.id).collect::<Vec<_>>(),
                "{mode:?}/{text}"
            );
            assert_eq!(line.inline_size(), 16.0);
            if text == "12" {
                assert_eq!(line.text(), "１２");
                assert_eq!(
                    glyphs(&line).iter().map(|g| g.cluster).collect::<Vec<_>>(),
                    vec![0, 3]
                );
                assert_eq!(line.text_range(), 0..6);
                assert_eq!(
                    line.offset_mapping()
                        .unwrap()
                        .text_to_dom(3, Affinity::Downstream),
                    Some(shodo::mapping::TextOrigin::Dom {
                        node: NodeId(1),
                        offset: 1
                    })
                );
            }
        }
        // Authored fullwidth characters retain their form; only the transform
        // is reverted. One transformed character also retains its full width.
        let authored = first_line(
            &paragraph(&combined, "１２"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let plain = first_line(
            &paragraph(&combined, "12"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_ne!(glyphs(&authored)[0].id, glyphs(&plain)[0].id);
        combined.root.text_transform = TextTransform::FullWidth;
        let single = first_line(
            &paragraph(&combined, "1"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(glyphs(&single)[0].id, glyphs(&authored)[0].id);
    }
}

#[test]
fn combine_all_uses_width_features_only_with_complete_applicable_coverage() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let line = first_line(
            &paragraph(&combined, "12"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        // The pinned hwid SingleSubst maps gid18→853 and19→854, with500-unit
        // hmtx advances at UPEM1000. Natural advance is16px; no scale needed.
        assert_eq!(
            glyphs(&line).iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![853, 854]
        );
        assert_eq!(
            glyphs(&line).iter().map(|g| g.advance).collect::<Vec<_>>(),
            vec![8.0, 8.0]
        );
        let partial = first_line(
            &paragraph(&combined, "1水"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            glyphs(&partial).iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![18, 618]
        );
        combined.root.font_features = vec![FontFeature {
            tag: *b"hwid",
            value: 0,
        }];
        let disabled = first_line(
            &paragraph(&combined, "12"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            glyphs(&disabled).iter().map(|g| g.id).collect::<Vec<_>>(),
            vec![18, 19]
        );
    }
}

#[test]
fn combine_all_uses_third_and_quarter_width_then_scales_remaining_excess() {
    for (tag, text, expected, scale) in [
        (*b"twid", "123", vec![853, 854, 855], 2.0 / 3.0),
        (*b"qwid", "1234", vec![853, 854, 855, 856], 0.5),
    ] {
        let mut bytes = include_bytes!("../dev/fixtures/assets/fonts/cjk.otf").to_vec();
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let gsub = (0..count)
            .find_map(|i| {
                let at = 12 + i * 16;
                (&bytes[at..at + 4] == b"GSUB").then(|| {
                    u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize
                })
            })
            .unwrap();
        let features =
            gsub + u16::from_be_bytes(bytes[gsub + 6..gsub + 8].try_into().unwrap()) as usize;
        let count = u16::from_be_bytes(bytes[features..features + 2].try_into().unwrap()) as usize;
        let record = (0..count)
            .map(|i| features + 2 + i * 6)
            .find(|at| &bytes[*at..*at + 4] == b"hwid")
            .unwrap();
        bytes[record..record + 4].copy_from_slice(&tag);
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
                bytes,
                0,
                FontFaceDescriptor {
                    family: "Derived Width CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            let mut combined = style(mode, TextOrientation::Mixed);
            combined.root.font_families = vec![FontFamily::Named("Derived Width CJK".into())];
            combined.root.text_combine_upright = TextCombineUpright::All;
            combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let mut builder = ParagraphBuilder::new(&combined, &limits);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
            let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(
                glyphs(&line).iter().map(|g| g.id).collect::<Vec<_>>(),
                expected,
                "{tag:?}/{mode:?}"
            );
            assert_eq!(line.inline_size(), 16.0);
            let run = match line.fragment(0).unwrap() {
                Fragment::GlyphRun(run) => run,
                _ => panic!("combined"),
            };
            assert!((run.glyph_transform().block_x.abs() - scale).abs() < 0.001);
        }
    }
}

#[test]
fn combine_all_trims_collapsed_spaces_at_its_own_box_edges() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut root = style(mode, TextOrientation::Mixed);
        root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let mut combined = root.root.clone();
        combined.text_combine_upright = TextCombineUpright::All;
        let make = |text| {
            build_paragraph(&root, &Limits::default(), |builder| {
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "水",
                );
                builder.open_inline(NodeId(2), &combined, InlineEdges::default());
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(3),
                        offset: 0,
                    },
                    text,
                );
                builder.close_inline();
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(4),
                        offset: 0,
                    },
                    "水",
                );
            })
        };
        let plain = first_line(
            &make("12"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let spaced = first_line(
            &make(" 12 "),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(spaced.text(), "水12水");
        assert_eq!(spaced.inline_size(), 48.0);
        assert_eq!(glyphs(&spaced), glyphs(&plain));
        assert_eq!(
            spaced
                .offset_mapping()
                .unwrap()
                .text_to_dom(3, Affinity::Downstream),
            Some(shodo::mapping::TextOrigin::Dom {
                node: NodeId(3),
                offset: 1
            })
        );
    }
}

#[test]
fn combine_all_ignores_hard_breaks_but_preserves_source_offsets() {
    use shodo::style::WhiteSpaceCollapse;
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        combined.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let p = paragraph(&combined, "12\n34");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.text(), "1234");
        assert_eq!(line.text_range(), 0..4);
        assert_eq!(line.break_reason(), shodo::BreakReason::End);
        assert_eq!(line.inline_size(), 16.0);
        assert_eq!(
            line.offset_mapping()
                .unwrap()
                .text_to_dom(2, Affinity::Downstream),
            Some(shodo::mapping::TextOrigin::Dom {
                node: NodeId(1),
                offset: 3
            })
        );
        let p = build_paragraph(&combined, &Limits::default(), |builder| {
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "12");
            builder.push_forced_break(NodeId(2));
            builder.push_text(TextSource::Generated { node: NodeId(3) }, "34");
        });
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 16.0);
        assert_eq!(line.break_reason(), shodo::BreakReason::End);
        assert_eq!(glyphs(&line).len(), 4);
    }
}

#[test]
fn combine_all_preserved_spaces_do_not_hang_outside_the_square() {
    use shodo::style::WhiteSpaceCollapse;
    let mut combined = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    combined.root.text_combine_upright = TextCombineUpright::All;
    combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
    combined.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let line = first_line(
        &paragraph(&combined, " 12 "),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(line.inline_size(), 16.0);
    assert_eq!(line.hang_end(), 0.0);
    assert_eq!(glyphs(&line).len(), 4);
}

#[test]
fn combine_all_distinguishes_inherited_box_boundaries_from_separate_all_ancestors() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut root = style(mode, TextOrientation::Mixed);
        root.root.text_combine_upright = TextCombineUpright::All;
        root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        for empty in [false, true] {
            let p = build_paragraph(&root, &Limits::default(), |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "12");
                builder.open_inline(NodeId(2), &root.root, InlineEdges::default());
                if !empty {
                    builder.push_text(TextSource::Generated { node: NodeId(3) }, "34");
                }
                builder.close_inline();
                if empty {
                    builder.push_text(TextSource::Generated { node: NodeId(3) }, "34");
                }
            });
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            assert_eq!(glyphs(&line).len(), 4);
            assert!(
                line.fragments()
                    .filter_map(|fragment| match fragment {
                        Fragment::GlyphRun(run) => Some(run.orientation()),
                        _ => None,
                    })
                    .all(|orientation| orientation == shodo::GlyphOrientation::SidewaysClockwise)
            );
        }
        let combined = root.root.clone();
        root.root.text_combine_upright = TextCombineUpright::None;
        let p = build_paragraph(&root, &Limits::default(), |builder| {
            for (node, text) in [(NodeId(1), "12"), (NodeId(2), "34")] {
                builder.open_inline(node, &combined, InlineEdges::default());
                builder.push_text(TextSource::Generated { node }, text);
                builder.close_inline();
            }
        });
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 32.0);
        assert_eq!(glyphs(&line).len(), 4);
        assert!(
            line.fragments()
                .filter_map(|fragment| match fragment {
                    Fragment::GlyphRun(run) => Some(run.orientation()),
                    _ => None,
                })
                .all(|orientation| orientation == shodo::GlyphOrientation::Combined)
        );
    }
}

#[test]
fn combine_all_isolates_internal_arabic_joining_from_vertical_neighbors() {
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
            include_bytes!("../dev/fixtures/assets/fonts/arabic.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for orientation in [TextOrientation::Mixed, TextOrientation::Upright] {
                let mut root = style(mode, orientation);
                root.direction = direction;
                root.root.direction = direction;
                root.root.font_families = vec![FontFamily::Named("Shodo Fixture Arabic".into())];
                root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
                let mut combined = root.root.clone();
                combined.text_combine_upright = TextCombineUpright::All;
                let make = |surrounded| {
                    let mut builder = ParagraphBuilder::new(&root, &limits);
                    if surrounded {
                        builder.push_text(TextSource::Generated { node: NodeId(1) }, "ب");
                    }
                    builder.open_inline(NodeId(2), &combined, InlineEdges::default());
                    builder.push_text(TextSource::Generated { node: NodeId(3) }, "بب");
                    builder.close_inline();
                    if surrounded {
                        builder.push_text(TextSource::Generated { node: NodeId(4) }, "ب");
                    }
                    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
                    line.fragments()
                        .filter_map(|fragment| match fragment {
                            Fragment::GlyphRun(run)
                                if run.orientation() == shodo::GlyphOrientation::Combined =>
                            {
                                Some(
                                    run.glyphs()
                                        .map(|g| {
                                            (
                                                g.id,
                                                g.inline_position - run.inline_start(),
                                                g.block_offset,
                                            )
                                        })
                                        .collect::<Vec<_>>(),
                                )
                            }
                            _ => None,
                        })
                        .flatten()
                        .collect::<Vec<_>>()
                };
                let actual = make(true);
                let expected = make(false);
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(
                        actual.0, expected.0,
                        "{mode:?}/{direction:?}/{orientation:?}"
                    );
                    assert!(
                        (actual.1 - expected.1).abs() < 0.001
                            && (actual.2 - expected.2).abs() < 0.001
                    );
                }
            }
        }
    }
}

#[test]
fn combine_all_preserves_source_ownership_and_internal_caret_selection() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let p = build_paragraph(&combined, &Limits::default(), |builder| {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(1),
                    offset: 0,
                },
                "12",
            );
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(2),
                    offset: 0,
                },
                "34",
            );
        });
        let lines = vec![first_line(
            &p,
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        )];
        let owners: Vec<_> = lines[0]
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run.node()),
                _ => None,
            })
            .collect();
        assert_eq!(owners, vec![Some(NodeId(1)), Some(NodeId(2))]);
        let layout = LineLayout::new(&lines);
        let position = |offset, affinity| TextPosition {
            line: 0,
            offset,
            affinity,
        };
        let carets: Vec<_> = (0..=4)
            .map(|offset| {
                layout
                    .caret(position(offset, Affinity::Downstream))
                    .unwrap()
            })
            .collect();
        for caret in &carets {
            assert_eq!(caret.rect.inline_start, 0.0);
            assert_eq!(caret.rect.inline_size, 16.0);
            assert_eq!(caret.rect.block_size, 0.0);
        }
        let sign = if mode == WritingMode::VerticalRl {
            -1.0
        } else {
            1.0
        };
        for pair in carets.windows(2) {
            assert!(
                (pair[1].rect.block_start - pair[0].rect.block_start - sign * 4.0).abs() < 0.01
            );
        }
        let hit = layout.hit_test(8.0, carets[2].rect.block_start).unwrap();
        assert_eq!(hit.position.offset, 2);
        let selection = layout.selection_rects(
            position(1, Affinity::Downstream),
            position(3, Affinity::Upstream),
        );
        assert_eq!(selection.len(), 1);
        assert_eq!(selection[0].inline_size, 16.0);
        assert!((selection[0].block_size - 8.0).abs() < 0.01);
        let mapping = lines[0].offset_mapping().unwrap();
        assert_eq!(
            mapping.text_to_dom(2, Affinity::Downstream),
            Some(shodo::mapping::TextOrigin::Dom {
                node: NodeId(2),
                offset: 0
            })
        );
    }
}

#[test]
fn vertical_line_uses_vhea_and_central_baseline() {
    // The pinned CJK face has vhea ascender 500, descender -500, UPEM 1000.
    // At 16px its vertical strut is 8+8, unlike hhea's 1160+288 units.
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let p = paragraph(&style(mode, TextOrientation::Upright), "水");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.block_size(), 16.0);
        assert_eq!(line.baseline(BaselineKind::Central), 8.0);
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.baseline(), 8.0);
        let vertical = run.vertical_metrics().unwrap();
        assert_eq!(
            (vertical.ascent, vertical.descent, vertical.line_gap),
            (8.0, 8.0, 0.0)
        );
    }
}

#[test]
fn vertical_kerning_none_restores_nominal_vmtx_advance() {
    // In this pinned font, vert maps ぃ→cid65159 and ，→cid58979.
    // The pair has vkrn YAdvance=-20 units while each vmtx advance is 1000.
    let mut style = style(WritingMode::VerticalRl, TextOrientation::Upright);
    style.root.font_kerning = FontKerning::Normal;
    let original = paragraph(&style, "ぃ，");
    let original_line = first_line(
        &original,
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    let kerned = glyphs(&original_line)[0].advance;
    assert_ne!(kerned, 16.0, "fixture must exercise real vertical kerning");
    style.root.font_kerning = FontKerning::None;
    let disabled = paragraph(&style, "ぃ，");
    let disabled_line = first_line(
        &disabled,
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    assert_eq!(glyphs(&disabled_line)[0].advance, 16.0);
}
