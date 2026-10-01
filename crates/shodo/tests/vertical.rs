mod common;

use common::{first_line, glyphs};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::{BaselineKind, Direction, PhysicalConverter, PhysicalSize, WritingMode};
use shodo::hit::{LineLayout, TextPosition};
use shodo::limits::Limits;
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use shodo::style::{
    FontFamily, FontFeature, FontKerning, InlineStyle, LineOptions, ParagraphStyle, TextAutospace,
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
fn upright_latin_letters_and_digits_do_not_trigger_vertical_autospace() {
    for text in ["国X国", "国1国"] {
        for (mode, orientation, should_space) in [
            (WritingMode::VerticalRl, TextOrientation::Upright, false),
            (WritingMode::VerticalLr, TextOrientation::Upright, false),
            (WritingMode::VerticalRl, TextOrientation::Mixed, true),
            (WritingMode::VerticalRl, TextOrientation::Sideways, true),
            (WritingMode::HorizontalTb, TextOrientation::Upright, true),
            (WritingMode::SidewaysRl, TextOrientation::Upright, true),
        ] {
            let width = |autospace| {
                let mut input = style(mode, orientation);
                input.root.font_size = 20.0;
                input.root.text_autospace = autospace;
                first_line(
                    &paragraph(&input, text),
                    300.,
                    &LineOptions::default(),
                    &AtomicSizes::EMPTY,
                )
                .inline_size()
            };
            let normal = width(TextAutospace::Normal);
            let no_autospace = width(TextAutospace::NoAutospace);
            if should_space {
                assert!(normal > no_autospace, "{text:?} {mode:?} {orientation:?}");
            } else {
                assert_eq!(normal, no_autospace, "{text:?} {mode:?} {orientation:?}");
                assert_eq!(normal, 60.0, "{text:?} {mode:?} {orientation:?}");
            }
        }
    }
}

#[test]
fn upright_inline_orientation_controls_autospace_at_both_edges() {
    for (root_orientation, inline_orientation, should_space) in [
        (TextOrientation::Mixed, TextOrientation::Upright, false),
        (TextOrientation::Upright, TextOrientation::Mixed, true),
    ] {
        let width = |autospace| {
            let limits = Limits::default();
            let mut root = style(WritingMode::VerticalRl, root_orientation);
            root.root.font_size = 20.0;
            root.root.text_autospace = autospace;
            let mut inline = root.root.clone();
            inline.text_orientation = inline_orientation;
            let fonts = FontCollection::with_options(
                &limits,
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            fonts
                .register_face(
                    include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
                    0,
                    FontFaceDescriptor {
                        family: "Shodo Fixture CJK".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut builder = ParagraphBuilder::new(&root, &limits);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "国");
            builder.open_inline(NodeId(2), &inline, InlineEdges::default());
            builder.push_text(TextSource::Generated { node: NodeId(3) }, "X");
            builder.close_inline();
            builder.push_text(TextSource::Generated { node: NodeId(4) }, "国");
            let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            first_line(
                &paragraph,
                300.,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            )
            .inline_size()
        };
        let normal = width(TextAutospace::Normal);
        let no_autospace = width(TextAutospace::NoAutospace);
        if should_space {
            assert!(normal > no_autospace);
        } else {
            assert_eq!(normal, no_autospace);
        }
    }
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
fn physical_origin_places_outline_in_container_for_every_inline_flow() {
    let container = PhysicalSize {
        width: 100.0,
        height: 80.0,
    };
    for (mode, direction, expected_inline_axis) in [
        (WritingMode::HorizontalTb, Direction::Ltr, 7.0),
        (WritingMode::HorizontalTb, Direction::Rtl, 77.0),
        (WritingMode::VerticalRl, Direction::Ltr, 7.0),
        (WritingMode::VerticalRl, Direction::Rtl, 57.0),
        (WritingMode::VerticalLr, Direction::Ltr, 7.0),
        (WritingMode::VerticalLr, Direction::Rtl, 57.0),
        (WritingMode::SidewaysRl, Direction::Ltr, 7.0),
        (WritingMode::SidewaysRl, Direction::Rtl, 57.0),
        (WritingMode::SidewaysLr, Direction::Ltr, 73.0),
        (WritingMode::SidewaysLr, Direction::Rtl, 23.0),
    ] {
        let mut input = style(mode, TextOrientation::Sideways);
        input.direction = direction;
        input.root.direction = direction;
        let p = paragraph(&input, "水");
        let mut constraint = LineConstraint::new(60.0);
        constraint.inline_start_offset = 7.0;
        constraint.block_offset = 20.0;
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &LineOptions::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected line")
        };
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        let glyph = run.glyphs().next().unwrap();
        assert_eq!(glyph.inline_position, 7.0);
        let block = 20.0 + run.baseline() + glyph.block_offset;
        let expected = match mode {
            WritingMode::HorizontalTb => (expected_inline_axis, block),
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                (100.0 - block, expected_inline_axis)
            }
            WritingMode::VerticalLr | WritingMode::SidewaysLr => (block, expected_inline_axis),
        };
        assert_eq!(
            run.physical_origin(0, container),
            Some(expected),
            "{mode:?}/{direction:?}"
        );
        assert_eq!(run.physical_origin(run.glyphs().len(), container), None);
        assert_eq!(run.physical_origin(usize::MAX, container), None);
    }
}

#[test]
fn physical_origin_uses_upright_vertical_used_direction() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut input = style(mode, TextOrientation::Upright);
        input.direction = Direction::Rtl;
        input.root.direction = Direction::Rtl;
        let p = paragraph(&input, "水");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.used_direction(), Direction::Ltr);
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        let glyph = run.glyphs().next().unwrap();
        let origin = run
            .physical_origin(
                0,
                PhysicalSize {
                    width: 100.0,
                    height: 80.0,
                },
            )
            .unwrap();
        assert_eq!(origin.1, glyph.inline_position, "{mode:?}");
    }
}

#[test]
fn physical_origin_keeps_rtl_tracking_outside_the_natural_advance_cell() {
    let mut input = style(WritingMode::HorizontalTb, TextOrientation::Mixed);
    input.direction = Direction::Rtl;
    input.root.direction = Direction::Rtl;
    input.root.letter_spacing = 6.0;
    let p = paragraph(&input, "水水");
    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
    let run = line
        .fragments()
        .find_map(|f| match f {
            Fragment::GlyphRun(run) => Some(run),
            _ => None,
        })
        .unwrap();
    // The fixture's water glyph has a 16px natural advance. Tracking affects
    // layout cells, but moving the outline origin by that extra space shifts ink.
    assert!(run.glyphs().any(|g| g.advance > 16.0));
    for (index, glyph) in run.glyphs().enumerate() {
        let expected = (
            100.0 - glyph.inline_position - 16.0,
            run.baseline() + glyph.block_offset,
        );
        assert_eq!(
            run.physical_origin(
                index,
                PhysicalSize {
                    width: 100.0,
                    height: 80.0
                }
            ),
            Some(expected)
        );
    }
}

#[test]
fn sideways_lr_outline_origin_stays_inside_its_advance_cell() {
    for direction in [Direction::Ltr, Direction::Rtl] {
        let mut style = style(WritingMode::SidewaysLr, TextOrientation::Mixed);
        style.direction = direction;
        style.root.direction = direction;
        let p = paragraph(&style, "F");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let run = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        let glyph = run.glyphs().next().unwrap();
        let converter = PhysicalConverter::new(
            WritingMode::SidewaysLr,
            direction,
            PhysicalSize {
                width: 100.0,
                height: 80.0,
            },
        );
        let (inline, block) = run.glyph_origin(0).unwrap();
        let origin = converter.point(inline, block);
        // CCW font x advances physically upward. In LTR the cell begins at
        // its bottom; in RTL its bottom is one advance after its logical start.
        let expected_y = if direction == Direction::Ltr {
            80.0 - glyph.inline_position
        } else {
            glyph.inline_position + glyph.advance
        };
        assert_eq!(origin.1, expected_y, "{direction:?}");
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
            include_bytes!("../../../dev/fixtures/assets/fonts/arabic.ttf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let latin = fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
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
    // Latin hhea/OS2 extents 1069/293 units at UPEM1000 and size20.
    // Centering gives 27.24px, rounded outward to 27.25px.
    assert_eq!(line.block_size(), 27.25);
    assert_eq!(runs[1].baseline() - runs[0].baseline(), 7.765625);
}

#[test]
fn mixed_sideways_baseline_and_selection_share_the_central_axis() {
    for (mode, sign, selection_start) in [
        (WritingMode::VerticalRl, 1.0, -0.013125),
        (WritingMode::VerticalLr, -1.0, 0.017),
    ] {
        let mut s = style(mode, TextOrientation::Mixed);
        s.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let p = paragraph(&s, "水A");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let runs = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect::<Vec<_>>();
        // CJK hhea ascent/descent are1160/288 units; (18.56-4.608)/2
        // is6.976px, quantized to6.96875. vhea is symmetric8/8.
        assert_eq!(
            runs[1].baseline() - runs[0].baseline(),
            sign * 6.96875,
            "{mode:?}"
        );
        assert_eq!(line.block_size(), 23.171875);
        assert_eq!(runs[0].baseline(), line.baseline(BaselineKind::Central));
        let lines = [line];
        let rects = LineLayout::new(&lines).selection_rects(
            TextPosition {
                line: 0,
                offset: 3,
                affinity: Affinity::Downstream,
            },
            TextPosition {
                line: 0,
                offset: 4,
                affinity: Affinity::Upstream,
            },
        );
        assert_eq!(rects.len(), 1);
        // Baseline and shift round independently; line width rounds outward.
        // RL:11.578125+6.96875-18.56. LR:11.59375-6.96875-4.608.
        assert!(
            (rects[0].block_start - selection_start).abs() < 1e-5,
            "{mode:?} {rects:?}"
        );
        assert!(
            (rects[0].block_size - 23.168).abs() < 1.0 / 64.0,
            "{mode:?} {rects:?}"
        );
    }
}

#[test]
fn mixed_and_sideways_inline_boundaries_convert_the_dominant_baseline_once() {
    for (mode, sign) in [
        (WritingMode::VerticalRl, 1.0),
        (WritingMode::VerticalLr, -1.0),
    ] {
        for root_orientation in [TextOrientation::Mixed, TextOrientation::Sideways] {
            let root = style(mode, root_orientation);
            let mut child = root.root.clone();
            child.text_orientation = if root_orientation == TextOrientation::Mixed {
                TextOrientation::Sideways
            } else {
                TextOrientation::Mixed
            };
            let p = build_paragraph(&root, &Limits::default(), |b| {
                b.push_text(
                    TextSource::Generated { node: NodeId(1) },
                    if root_orientation == TextOrientation::Mixed {
                        "水"
                    } else {
                        "A"
                    },
                );
                b.open_inline(NodeId(2), &child, InlineEdges::default());
                b.push_text(
                    TextSource::Generated { node: NodeId(3) },
                    if root_orientation == TextOrientation::Mixed {
                        "A"
                    } else {
                        "水A"
                    },
                );
                b.close_inline();
            });
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            let runs = line
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if root_orientation == TextOrientation::Mixed {
                assert_eq!(
                    runs[1].baseline() - runs[0].baseline(),
                    sign * 6.96875,
                    "{mode:?}"
                );
            } else {
                assert_eq!(
                    runs[1].baseline() - runs[0].baseline(),
                    -sign * 6.96875,
                    "{mode:?}"
                );
                assert_eq!(runs[2].baseline(), runs[0].baseline());
            }
        }
    }
}

#[test]
fn mixed_sideways_text_edge_alignment_uses_converted_extents() {
    for (mode, sign) in [
        (WritingMode::VerticalRl, 1.0),
        (WritingMode::VerticalLr, -1.0),
    ] {
        for (alignment, displacement) in [
            (VerticalAlign::TextTop, 10.5625),
            (VerticalAlign::TextBottom, 3.390625),
        ] {
            let root = style(mode, TextOrientation::Mixed);
            let mut child = root.root.clone();
            child.text_orientation = TextOrientation::Sideways;
            child.vertical_align = alignment;
            let p = build_paragraph(&root, &Limits::default(), |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "水")
                    .open_inline(NodeId(2), &child, InlineEdges::default())
                    .push_text(TextSource::Generated { node: NodeId(3) }, "A")
                    .close_inline();
            });
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            let runs = line
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .collect::<Vec<_>>();
            // TextTop uses18.56-8=10.56, TextBottom8-4.608=3.392.
            assert_eq!(
                runs[1].baseline() - runs[0].baseline(),
                sign * displacement,
                "{mode:?}/{alignment:?}"
            );
        }
    }
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
    let mut bytes = include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec();
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
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
fn combine_all_reverts_fullwidth_for_multiple_characters() {
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
        // Author input and transformed input use the same narrow forms.
        // A single typographic unit keeps its fullwidth form.
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
        assert_eq!(glyphs(&authored)[0].id, glyphs(&plain)[0].id);
        combined.root.text_transform = TextTransform::FullWidth;
        let single = first_line(
            &paragraph(&combined, "1"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(glyphs(&single)[0].id, 694);
    }
}

#[test]
fn combine_all_authored_fullwidth_preserves_source_owners_and_byte_cuts() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.lang = None;
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.font_features = vec![FontFeature {
            tag: *b"hwid",
            value: 0,
        }];
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let p = build_paragraph(&combined, &Limits::default(), |b| {
            b.push_text(
                TextSource::Dom {
                    node: NodeId(8),
                    offset: 7,
                },
                "１",
            )
            .push_text(
                TextSource::Dom {
                    node: NodeId(9),
                    offset: 9,
                },
                "２",
            );
        });
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.text(), "１２");
        assert_eq!(line.text_range(), 0..6);
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| (g.id, g.cluster))
                .collect::<Vec<_>>(),
            vec![(827, 0), (828, 3)]
        );
        let runs = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            runs.iter().map(|r| r.node()).collect::<Vec<_>>(),
            vec![Some(NodeId(8)), Some(NodeId(9))]
        );
        for run in &runs {
            assert!((run.glyph_transform().block_x.abs() - 0.90140843).abs() < 1e-6);
        }
        let mapping = line.offset_mapping().unwrap();
        for (text, node, offset) in [(0, NodeId(8), 7), (3, NodeId(9), 9)] {
            assert_eq!(
                mapping.text_to_dom(text, Affinity::Downstream),
                Some(shodo::mapping::TextOrigin::Dom { node, offset })
            );
        }
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
        let mut bytes = include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec();
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
fn combine_all_reports_candidates_rejected_across_an_inherited_box_boundary() {
    use shodo::limits::WarningKind;
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut root = style(mode, TextOrientation::Mixed);
        root.root.text_combine_upright = TextCombineUpright::All;
        let rejected = build_paragraph(&root, &Limits::default(), |builder| {
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "12");
            builder.open_inline(NodeId(2), &root.root, InlineEdges::default());
            builder.push_text(TextSource::Generated { node: NodeId(3) }, "34");
            builder.close_inline();
        });
        let messages: Vec<_> = rejected
            .warnings()
            .iter()
            .filter(|warning| warning.message.contains("text-combine-upright"))
            .collect();
        // One diagnostic per rejected candidate, each naming processed-text bytes.
        assert_eq!(messages.len(), 2, "{mode:?}: {:?}", rejected.warnings());
        assert!(
            messages
                .iter()
                .all(|warning| warning.kind == WarningKind::Unsupported)
        );
        assert!(
            messages[0].message.contains("0..2"),
            "{}",
            messages[0].message
        );
        assert!(
            messages[1].message.contains("2..4"),
            "{}",
            messages[1].message
        );

        // Separate `all` ancestors compose normally and stay silent.
        let combined = root.root.clone();
        root.root.text_combine_upright = TextCombineUpright::None;
        let composed = build_paragraph(&root, &Limits::default(), |builder| {
            for (node, text) in [(NodeId(1), "12"), (NodeId(2), "34")] {
                builder.open_inline(node, &combined, InlineEdges::default());
                builder.push_text(TextSource::Generated { node }, text);
                builder.close_inline();
            }
        });
        assert!(
            composed
                .warnings()
                .iter()
                .all(|warning| !warning.message.contains("text-combine-upright")),
            "{:?}",
            composed.warnings()
        );

        // Horizontal text never composes, so nothing was rejected.
        let horizontal = style(WritingMode::HorizontalTb, TextOrientation::Mixed);
        let mut horizontal = horizontal;
        horizontal.root.text_combine_upright = TextCombineUpright::All;
        let plain = paragraph(&horizontal, "12");
        assert!(plain.warnings().is_empty());
    }
}

#[test]
fn combine_rejection_warning_is_not_duplicated_by_first_line_build() {
    use shodo::limits::WarningKind;
    let mut root = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    root.root.text_combine_upright = TextCombineUpright::All;
    let mut first = root.root.clone();
    first.font_size = 24.0;
    root.first_line = Some(first);
    let limits = Limits {
        max_warnings: Some(2),
        ..Default::default()
    };
    let paragraph = build_paragraph(&root, &limits, |builder| {
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "12");
        builder.open_inline(NodeId(2), &root.root, InlineEdges::default());
        builder.push_text(TextSource::Generated { node: NodeId(3) }, "34");
        builder.close_inline();
    });
    let warnings = paragraph.warnings();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().all(|w| w.kind == WarningKind::Unsupported));
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
            include_bytes!("../../../dev/fixtures/assets/fonts/arabic.ttf").to_vec(),
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
                                // Writing Modes 4 §9.1.2 treats the outer square
                                // as upright (strong LTR), even between Arabic
                                // neighbors. Internal shaping keeps computed RTL.
                                let expected_level = u8::from(
                                    direction == Direction::Rtl
                                        && orientation != TextOrientation::Upright,
                                ) * 2;
                                assert_eq!(
                                    run.bidi_level(),
                                    expected_level,
                                    "{mode:?}/{direction:?}/{orientation:?}/{surrounded}"
                                );
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
fn combine_all_centers_its_square_on_parent_text_edges_before_alignment() {
    let mut bytes = include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let vhea = (0..count)
        .find_map(|index| {
            let record = 12 + index * 16;
            (&bytes[record..record + 4] == b"vhea").then(|| {
                u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize
            })
        })
        .unwrap();
    // At 16px the parent text-over/under extents are 9.6/6.4px.
    // The composition's square still occupies 0..16, centered at 8px.
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
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for text in ["12", "\t\t"] {
            for alignment in [0.0, 2.0] {
                let mut root = style(mode, TextOrientation::Upright);
                root.root.font_families = vec![FontFamily::Named("Asymmetric CJK".into())];
                root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
                let mut child = root.root.clone();
                child.text_combine_upright = TextCombineUpright::All;
                child.vertical_align = VerticalAlign::Length(alignment);
                let mut builder = ParagraphBuilder::new(&root, &limits);
                builder.open_inline(NodeId(1), &child, InlineEdges::default());
                builder.push_text(TextSource::Generated { node: NodeId(2) }, text);
                builder.close_inline();
                let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
                assert!(
                    (line.block_size() - (16.0 + alignment)).abs() < 1.0 / 64.0,
                    "{mode:?}/{text:?}/{alignment}: {}",
                    line.block_size()
                );
                let square = line.text_combinations().next().unwrap().square;
                let start = if mode == WritingMode::VerticalLr {
                    alignment
                } else {
                    0.0
                };
                assert!(
                    (square.block_start - start).abs() < 1.0 / 64.0,
                    "{mode:?}/{text:?}/{alignment}: {square:?}"
                );
                assert_eq!(square.block_size, 16.0);
                for fragment in line.fragments() {
                    if let Fragment::GlyphRun(run) = fragment {
                        assert!((run.baseline() - (start + 8.0)).abs() < 1.0 / 64.0);
                    }
                }
            }
        }
    }
}

#[test]
fn combine_all_square_is_measured_with_one_em_internal_line_height() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for text in ["12", "\t\t"] {
            for boxed in [false, true] {
                for alignment in [
                    VerticalAlign::Baseline,
                    VerticalAlign::Top,
                    VerticalAlign::Bottom,
                ] {
                    let mut root = style(mode, TextOrientation::Mixed);
                    root.root.line_height = shodo::style::LineHeight::Px(0.0);
                    root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
                    let mut child = root.root.clone();
                    child.text_combine_upright = TextCombineUpright::All;
                    child.vertical_align = alignment;
                    if !boxed {
                        root.root = child.clone();
                    }
                    let p = build_paragraph(&root, &Limits::default(), |builder| {
                        if boxed {
                            builder.open_inline(NodeId(1), &child, InlineEdges::default());
                        }
                        builder.push_text(TextSource::Generated { node: NodeId(2) }, text);
                        if boxed {
                            builder.close_inline();
                        }
                    });
                    let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
                    assert_eq!(
                        line.block_size(),
                        16.0,
                        "{mode:?}/{text:?}/{boxed}/{alignment:?}"
                    );
                    let square = line.text_combinations().next().unwrap().square;
                    assert_eq!(
                        square.block_start, 0.0,
                        "{mode:?}/{text:?}/{boxed}/{alignment:?}"
                    );
                    assert_eq!(square.block_size, 16.0);
                }
            }
        }
    }
}

#[test]
fn combine_all_is_atomic_for_intrinsics_and_emergency_breaks() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        combined.root.overflow_wrap = shodo::style::OverflowWrap::Anywhere;
        combined.root.word_break = shodo::style::WordBreak::BreakAll;
        let p = paragraph(&combined, "12345");
        let sizes = p.intrinsic_sizes(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &shodo::AtomicIntrinsics::EMPTY,
        );
        assert_eq!((sizes.min_content, sizes.max_content), (16.0, 16.0));
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            1.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text_range(), 0..5);
        assert_eq!(lines[0].inline_size(), 16.0);
        assert_eq!(glyphs(&lines[0]).len(), 5);
    }
}

#[test]
fn combine_all_keeps_whole_box_breaks_source_paint_and_planned_cached_results() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut root = style(mode, TextOrientation::Mixed);
            root.direction = direction;
            root.root.direction = direction;
            root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let mut combined = root.root.clone();
            combined.text_combine_upright = TextCombineUpright::All;
            let p = build_paragraph(&root, &Limits::default(), |builder| {
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
                    "12",
                );
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(4),
                        offset: 0,
                    },
                    "34",
                );
                builder.close_inline();
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(5),
                        offset: 0,
                    },
                    "水",
                );
            });
            let options = LineOptions::default();
            let sizes = p.intrinsic_sizes(
                &mut LayoutContext::new(),
                &options,
                &shodo::AtomicIntrinsics::EMPTY,
            );
            assert_eq!(
                (sizes.min_content, sizes.max_content),
                (16.0, 48.0),
                "{mode:?}/{direction:?}"
            );
            let plan = p.plan_breaks(
                &mut LayoutContext::new(),
                &options,
                16.0,
                &AtomicSizes::EMPTY,
            );
            let collect = |cx: &mut LayoutContext, planned| {
                let mut token = p.start_token();
                let mut constraint = LineConstraint::new(16.0);
                constraint.break_plan = planned;
                let mut output = Vec::new();
                loop {
                    match p.next_line(cx, token, &options, &constraint, &AtomicSizes::EMPTY) {
                        LineResult::Line(line) => {
                            token = line.break_token();
                            let runs: Vec<_> = line
                                .fragments()
                                .filter_map(|fragment| match fragment {
                                    Fragment::GlyphRun(run) => Some((
                                        run.orientation(),
                                        run.glyph_transform(),
                                        run.node(),
                                        run.inline_start(),
                                        (0..run.glyphs().len())
                                            .map(|g| run.glyph_origin(g).unwrap())
                                            .collect::<Vec<_>>(),
                                    )),
                                    _ => None,
                                })
                                .collect();
                            if line.text_range() == (3..7) {
                                let positions: Vec<_> = runs
                                    .iter()
                                    .flat_map(|run| run.4.iter().map(|origin| origin.0))
                                    .collect();
                                assert_eq!(positions.len(), 4);
                                assert!(
                                    positions.iter().all(|x| (*x - positions[0]).abs() < 0.001),
                                    "{mode:?}/{direction:?}: {runs:?}"
                                );
                            }
                            output.push((
                                line.text_range(),
                                line.inline_size(),
                                glyphs(&line),
                                runs,
                            ));
                        }
                        LineResult::Done => break,
                        other => panic!("{other:?}"),
                    }
                }
                output
            };
            let fresh = collect(&mut LayoutContext::new(), None);
            assert_eq!(
                fresh.iter().map(|line| line.0.clone()).collect::<Vec<_>>(),
                vec![0..3, 3..7, 7..10]
            );
            let mut cx = LayoutContext::new();
            assert_eq!(collect(&mut cx, None), fresh);
            assert_eq!(collect(&mut cx, None), fresh);
            assert_eq!(collect(&mut LayoutContext::new(), Some(&plan)), fresh);
        }
    }
}

#[test]
fn combine_all_scales_marks_and_keeps_fallback_fonts_on_one_horizontal_baseline() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        (
            "Shodo Fixture Latin",
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").as_slice(),
        ),
        (
            "Shodo Fixture CJK",
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").as_slice(),
        ),
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
    let make = |mode, bounded: bool| {
        let mut root = style(mode, TextOrientation::Mixed);
        root.root.font_families = vec![
            FontFamily::Named("Shodo Fixture Latin".into()),
            FontFamily::Named("Shodo Fixture CJK".into()),
        ];
        root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        root.root.text_combine_upright = if mode == WritingMode::HorizontalTb {
            TextCombineUpright::None
        } else {
            TextCombineUpright::All
        };
        let limits = Limits {
            max_shaping_run_bytes: bounded.then_some(3),
            ..Limits::default()
        };
        let mut builder = ParagraphBuilder::new(&root, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "q",
        );
        builder.push_text(
            TextSource::Dom {
                node: NodeId(2),
                offset: 0,
            },
            "\u{301}水",
        );
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY)
    };
    let plain = make(WritingMode::HorizontalTb, false);
    let plain_glyphs = glyphs(&plain);
    assert_eq!(plain_glyphs.len(), 3);
    assert!(plain_glyphs.iter().all(|glyph| glyph.id != 0));
    assert!(plain_glyphs.iter().any(|glyph| glyph.advance == 0.0));
    let natural: f32 = plain_glyphs.iter().map(|glyph| glyph.advance).sum();
    let scale = (16.0 / natural).min(1.0);
    let center = (16.0 - natural * scale) / 2.0;
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let line = make(mode, false);
        let actual = glyphs(&line);
        let runs: Vec<_> = line
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 2);
        assert_ne!(runs[0].font(), runs[1].font());
        let sign = if mode == WritingMode::VerticalRl {
            -1.0
        } else {
            1.0
        };
        let baseline = actual[0].inline_position;
        for (actual, expected) in actual.iter().zip(&plain_glyphs) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.advance, expected.advance);
            assert!(
                (actual.block_offset - sign * (center + expected.inline_position * scale - 8.0))
                    .abs()
                    < 0.001
            );
            assert!((actual.inline_position - baseline - expected.block_offset).abs() < 0.001);
        }
        assert_eq!(glyphs(&make(mode, true)), actual);
    }
}

#[test]
fn combine_all_missing_font_and_small_windows_keep_natural_advances() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let limits = Limits {
            max_shaping_run_bytes: Some(1),
            ..Limits::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let mut builder = ParagraphBuilder::new(&combined, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "12",
        );
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 16.0);
        assert_eq!(
            glyphs(&line)
                .iter()
                .map(|g| (g.id, g.advance))
                .collect::<Vec<_>>(),
            vec![(0, 16.0), (0, 16.0)]
        );
        for fragment in line.fragments() {
            if let Fragment::GlyphRun(run) = fragment {
                assert_eq!(run.orientation(), shodo::GlyphOrientation::Combined);
                assert_eq!(run.glyph_transform().block_x.abs(), 0.5);
            }
        }
    }
}

#[test]
fn combine_all_first_line_uses_its_own_square_without_leaking_to_later_lines() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut root = style(mode, TextOrientation::Mixed);
        root.root.word_break = shodo::style::WordBreak::BreakAll;
        root.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let mut first = root.root.clone();
        first.font_size = 24.0;
        root.first_line = Some(first);
        let mut combined = root.root.clone();
        combined.text_combine_upright = TextCombineUpright::All;
        let p = build_paragraph(&root, &Limits::default(), |builder| {
            for (node, text) in [(NodeId(1), "12"), (NodeId(2), "34"), (NodeId(3), "56")] {
                builder.open_inline(node, &combined, InlineEdges::default());
                builder.push_text(TextSource::Dom { node, offset: 0 }, text);
                builder.close_inline();
            }
        });
        let options = LineOptions::default();
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &options,
            24.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.text_range())
                .collect::<Vec<_>>(),
            vec![0..2, 2..4, 4..6]
        );
        for (line, em) in lines.iter().zip([24.0, 16.0, 16.0]) {
            assert_eq!(line.inline_size(), em);
            assert_eq!(line.baseline(BaselineKind::Central), em / 2.0);
            for fragment in line.fragments() {
                if let Fragment::GlyphRun(run) = fragment {
                    assert_eq!(run.orientation(), shodo::GlyphOrientation::Combined);
                    assert_eq!(run.font_size(), em);
                    assert_eq!(run.glyph_transform().block_x.abs(), 1.0);
                    assert_eq!(
                        run.glyphs().map(|g| g.advance).collect::<Vec<_>>(),
                        vec![em / 2.0; 2]
                    );
                }
            }
        }
        let plan = p.plan_breaks(
            &mut LayoutContext::new(),
            &options,
            24.0,
            &AtomicSizes::EMPTY,
        );
        let mut constraint = LineConstraint::new(24.0);
        constraint.break_plan = Some(&plan);
        let mut token = p.start_token();
        let mut cx = LayoutContext::new();
        for expected in &lines {
            let LineResult::Line(line) =
                p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
            else {
                panic!("planned first-line TCY");
            };
            token = line.break_token();
            assert_eq!(glyphs(&line), glyphs(expected));
            assert_eq!(line.text_range(), expected.text_range());
            assert_eq!(line.inline_size(), expected.inline_size());
        }
    }
}

#[test]
fn combine_all_float_and_height_retry_keep_square_origin_and_font_ownership() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = style(mode, TextOrientation::Mixed);
        combined.root.text_combine_upright = TextCombineUpright::All;
        combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        let p = build_paragraph(&combined, &Limits::default(), |builder| {
            builder.push_out_of_flow(NodeId(1), OutOfFlowKind::Float);
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(2),
                    offset: 0,
                },
                "12",
            );
        });
        let mut cx = LayoutContext::new();
        let options = LineOptions::default();
        let mut constraint = LineConstraint::new(16.0);
        constraint.inline_start_offset = 10.0;
        let mut cursor = None;
        for _ in 0..2 {
            let LineResult::FloatEncountered {
                node,
                line_start,
                float_cursor,
                ..
            } = p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            )
            else {
                panic!("float replay");
            };
            assert_eq!(node, NodeId(1));
            assert_eq!(line_start, p.start_token());
            if let Some(previous) = cursor {
                assert_eq!(float_cursor, previous);
            }
            cursor = Some(float_cursor);
        }
        constraint.floats_placed_through = cursor;
        constraint.max_block_size = Some(1.0);
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY
            ),
            LineResult::BlockSizeExceeded { .. }
        ));
        constraint.max_block_size = None;
        let get = |cx: &mut LayoutContext| {
            let LineResult::Line(line) = p.next_line(
                cx,
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("float accepted TCY");
            };
            line
        };
        let line = get(&mut cx);
        assert_eq!(line.inline_size(), 16.0);
        assert_eq!(glyphs(&line), glyphs(&get(&mut cx)));
        assert_eq!(glyphs(&line), glyphs(&get(&mut LayoutContext::new())));
        drop(p);
        let run = line
            .fragments()
            .find_map(|fragment| match fragment {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.inline_start(), 10.0);
        assert_eq!(run.orientation(), shodo::GlyphOrientation::Combined);
        assert!(run.font_data().is_some());
        assert_eq!(run.node(), Some(NodeId(2)));
    }
}

#[test]
fn combine_all_compression_does_not_bypass_the_real_glyph_budget() {
    let mut input = style(WritingMode::VerticalRl, TextOrientation::Mixed);
    input.root.text_combine_upright = TextCombineUpright::All;
    let limits = Limits {
        max_shaped_glyphs: Some(1),
        ..Default::default()
    };
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            include_bytes!("../../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut b = ParagraphBuilder::new(&input, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "12",
    );
    let error = b.build(&mut LayoutContext::new(), &fonts).unwrap_err();
    assert_eq!(error.kind, shodo::limits::LimitKind::ShapedGlyphs);
    assert_eq!(error.limit, 1);
}

#[test]
fn combine_all_reports_saturation_of_internal_spacing_and_tabs() {
    use shodo::style::{TabSize, WhiteSpaceCollapse};
    for (text, tab, word) in [
        ("1 2", TabSize::Px(20.0), f32::MAX),
        ("1\t2", TabSize::Px(f32::MAX), 0.0),
    ] {
        let mut input = style(WritingMode::VerticalRl, TextOrientation::Mixed);
        input.root.text_combine_upright = TextCombineUpright::All;
        input.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        input.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        input.root.word_spacing = word;
        input.root.tab_size = tab;
        let p = paragraph(&input, text);
        assert!(
            p.warnings()
                .iter()
                .any(|w| w.kind == shodo::limits::WarningKind::Saturated),
            "internal geometry must retain lossy-conversion warnings for {text:?}"
        );
        let line = first_line(&p, 16.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        assert_eq!(line.inline_size(), 16.0);
        assert!(
            glyphs(&line)
                .iter()
                .all(|g| g.block_offset.is_finite() && g.inline_position.is_finite())
        );
    }
}

#[test]
fn combine_all_adjusted_fonts_and_space_tabs_keep_original_shaping_metrics() {
    use shodo::style::{FontMetricKind, FontSizeAdjust, TabSize, WhiteSpaceCollapse};
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
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
            0,
            FontFaceDescriptor {
                family: "Adjusted".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut horizontal = ParagraphStyle::default();
    horizontal.root.font_families = vec![FontFamily::Named("Adjusted".into())];
    horizontal.root.font_size = 16.0;
    horizontal.root.font_size_adjust = Some(FontSizeAdjust {
        metric: FontMetricKind::ExHeight,
        value: 1.0,
    });
    horizontal.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
    horizontal.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    horizontal.root.tab_size = TabSize::Spaces(4.0);
    horizontal.root.word_spacing = 2.0;
    let make = |input: &ParagraphStyle, text| {
        let mut b = ParagraphBuilder::new(input, &limits);
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            text,
        );
        b.build(&mut LayoutContext::new(), &fonts).unwrap()
    };
    for text in ["12345", "1\t2"] {
        let plain = first_line(
            &make(&horizontal, text),
            1000.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let expected = glyphs(&plain);
        let expected_size = match plain.fragment(0).unwrap() {
            Fragment::GlyphRun(run) => run.font_size(),
            _ => panic!("font"),
        };
        assert!(
            expected_size > 16.0,
            "fixture must actually adjust font size"
        );
        for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            let mut combined = horizontal.clone();
            combined.writing_mode = mode;
            combined.root.text_combine_upright = TextCombineUpright::All;
            combined.root.letter_spacing = 10.0;
            let line = first_line(
                &make(&combined, text),
                1.0,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
            );
            assert_eq!(
                line.inline_size(),
                16.0,
                "the CSS em is unchanged by adjusted shaping size"
            );
            assert_eq!(
                line.text_combinations().next().unwrap().square.block_size,
                16.0
            );
            let scale = 16.0 / plain.inline_size();
            let sign = if mode == WritingMode::VerticalRl {
                -1.0
            } else {
                1.0
            };
            let actual = glyphs(&line);
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(&expected) {
                assert_eq!((actual.id, actual.advance), (expected.id, expected.advance));
                assert!(
                    (actual.block_offset - sign * (expected.inline_position * scale - 8.0)).abs()
                        < 0.02
                );
            }
            for run in line.fragments().filter_map(|f| match f {
                Fragment::GlyphRun(run) => Some(run),
                _ => None,
            }) {
                assert_eq!(run.font_size(), expected_size);
                assert!((run.glyph_transform().block_x.abs() - scale).abs() < 0.001);
            }
        }
    }
}

#[test]
fn combine_all_mixed_bidi_reorders_only_its_internal_horizontal_sources() {
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for (family, bytes) in [
        (
            "Shodo Fixture Latin",
            include_bytes!("../../../dev/fixtures/assets/fonts/latin.ttf").as_slice(),
        ),
        (
            "Shodo Fixture Arabic",
            include_bytes!("../../../dev/fixtures/assets/fonts/arabic.ttf").as_slice(),
        ),
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
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for orientation in [
                TextOrientation::Mixed,
                TextOrientation::Upright,
                TextOrientation::Sideways,
            ] {
                let mut input = style(mode, orientation);
                input.direction = direction;
                input.root.direction = direction;
                input.root.font_families = vec![
                    FontFamily::Named("Shodo Fixture Latin".into()),
                    FontFamily::Named("Shodo Fixture Arabic".into()),
                ];
                input.root.text_combine_upright = TextCombineUpright::All;
                input.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
                let mut builder = ParagraphBuilder::new(&input, &limits);
                for (node, text) in [(1, "a"), (2, "بب"), (3, "12"), (4, "z")] {
                    builder.push_text(
                        TextSource::Dom {
                            node: NodeId(node),
                            offset: 0,
                        },
                        text,
                    );
                }
                let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                let line = first_line(&p, 1.0, &LineOptions::default(), &AtomicSizes::EMPTY);
                assert_eq!(line.inline_size(), 16.0);
                assert_eq!(line.text_combinations().count(), 1);
                assert_eq!(line.text_range(), 0..8);
                let sign = if mode == WritingMode::VerticalRl {
                    -1.0
                } else {
                    1.0
                };
                let mut internal: Vec<_> = glyphs(&line)
                    .iter()
                    .map(|g| (sign * g.block_offset, g.cluster))
                    .collect();
                internal.sort_by(|a, b| a.0.total_cmp(&b.0));
                // UAX#9 visual LTR order of a + two Arabic letters +12 +z.
                // Byte positions preserve both 2-byte Arabic scalars. Under
                // RTL, I2 gives both AN digits and L z level2, so12z keeps
                // its order inside the reversed level1 paragraph (L2).
                let expected = if direction == Direction::Ltr {
                    vec![0, 5, 6, 3, 1, 7]
                } else {
                    vec![5, 6, 7, 3, 1, 0]
                };
                assert_eq!(
                    internal.iter().map(|v| v.1).collect::<Vec<_>>(),
                    expected,
                    "{mode:?}/{direction:?}/{orientation:?}"
                );
                let layout = LineLayout::new(std::slice::from_ref(&line));
                for offset in [0, 1, 3, 5, 6, 7, 8] {
                    let caret = layout
                        .caret(TextPosition {
                            line: 0,
                            offset,
                            affinity: if offset == 8 {
                                Affinity::Upstream
                            } else {
                                Affinity::Downstream
                            },
                        })
                        .unwrap();
                    assert_eq!((caret.rect.inline_size, caret.rect.block_size), (16.0, 0.0));
                }
            }
        }
    }
}

#[test]
fn combine_all_exposes_one_emphasis_square_without_changing_paint_owners() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut input = style(mode, TextOrientation::Mixed);
            input.direction = direction;
            input.root.direction = direction;
            input.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let mut child = input.root.clone();
            child.text_combine_upright = TextCombineUpright::All;
            let p = build_paragraph(&input, &Limits::default(), |builder| {
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "水",
                );
                builder.open_inline(NodeId(2), &child, InlineEdges::default());
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(3),
                        offset: 0,
                    },
                    "12",
                );
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(4),
                        offset: 0,
                    },
                    "34",
                );
                builder.close_inline();
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(5),
                        offset: 0,
                    },
                    "水",
                );
            });
            let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
            let combinations: Vec<_> = line.text_combinations().collect();
            assert_eq!(
                combinations.len(),
                1,
                "one emphasis target despite two paint owners"
            );
            assert_eq!(combinations[0].text_range, 3..7);
            assert_eq!(
                combinations[0].square,
                shodo::geometry::LogicalRect {
                    inline_start: 16.0,
                    inline_size: 16.0,
                    block_start: 0.0,
                    block_size: 16.0
                }
            );
            let mut owners = Vec::new();
            let mut eligible = 0;
            for fragment in line.fragments() {
                if let Fragment::GlyphRun(run) = fragment {
                    eligible += run
                        .clusters()
                        .filter(|c| !c.flags.emphasis_excluded)
                        .count();
                    if run.orientation() == shodo::GlyphOrientation::Combined {
                        owners.push(run.node().unwrap());
                        assert!(
                            run.clusters().all(|c| c.flags.emphasis_excluded),
                            "internal glyph clusters must not add independent emphasis marks"
                        );
                    }
                }
            }
            owners.sort_by_key(|n| n.0);
            assert_eq!(owners, vec![NodeId(3), NodeId(4)]);
            assert_eq!(
                eligible + combinations.len(),
                3,
                "water, combined text, water"
            );
        }
    }
}

#[test]
fn combine_all_preserved_tabs_keep_horizontal_stops_inside_one_square() {
    use shodo::style::{TabSize, WhiteSpaceCollapse};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            for text in ["1\t2", "\t1\t", "\t\t"] {
                let mut combined = style(mode, TextOrientation::Mixed);
                combined.direction = direction;
                combined.root.direction = direction;
                combined.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
                combined.root.text_combine_upright = TextCombineUpright::All;
                combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
                combined.root.tab_size = TabSize::Px(20.0);
                combined.root.letter_spacing = 10.0;
                combined.root.font_features = [*b"hwid", *b"twid", *b"qwid"]
                    .into_iter()
                    .map(|tag| FontFeature { tag, value: 0 })
                    .collect();
                let p = paragraph(&combined, text);
                let intrinsic = p.intrinsic_sizes(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    &shodo::AtomicIntrinsics::EMPTY,
                );
                assert_eq!(
                    (intrinsic.min_content, intrinsic.max_content),
                    (16.0, 16.0),
                    "tabs cannot split the external square: {text:?}"
                );
                let lines = p.break_all(
                    &mut LayoutContext::new(),
                    &LineOptions::default(),
                    1.0,
                    &AtomicSizes::EMPTY,
                );
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0].inline_size(), 16.0);
                assert_eq!(lines[0].text_range(), 0..text.len());
                assert_eq!(lines[0].hang_end(), 0.0);
                let combinations: Vec<_> = lines[0].text_combinations().collect();
                assert_eq!(combinations.len(), 1);
                assert_eq!(combinations[0].text_range, 0..text.len());
                assert_eq!(combinations[0].square.inline_size, 16.0);
                assert_eq!(combinations[0].square.block_size, 16.0);
                let layout = LineLayout::new(&lines);
                for offset in 0..=text.len() {
                    let affinity = if offset == text.len() {
                        Affinity::Upstream
                    } else {
                        Affinity::Downstream
                    };
                    let caret = layout
                        .caret(TextPosition {
                            line: 0,
                            offset: offset as u32,
                            affinity,
                        })
                        .unwrap();
                    assert_eq!(
                        (caret.rect.inline_size, caret.rect.block_size),
                        (16.0, 0.0),
                        "all tab/source cuts follow the horizontal axis inside TCY"
                    );
                }
                // No tracking internally. A tab advances to20px, a second
                // to40px, and the fixture's normal digit advance is8.875px.
                let natural = if text == "1\t2" { 28.875 } else { 40.0 };
                let scale = 16.0 / natural;
                let sign = if mode == WritingMode::VerticalRl {
                    -1.0
                } else {
                    1.0
                };
                let mut pens: Vec<_> = glyphs(&lines[0])
                    .iter()
                    .map(|g| sign * g.block_offset + 8.0)
                    .collect();
                pens.sort_by(f32::total_cmp);
                let expected: Vec<_> = match (text, direction) {
                    ("1\t2", Direction::Ltr) => vec![0.0, 20.0 * scale],
                    ("1\t2", Direction::Rtl) => vec![0.0, 20.0 * scale],
                    ("\t1\t", _) => vec![
                        (if direction == Direction::Ltr {
                            20.0
                        } else {
                            11.125
                        }) * scale,
                    ],
                    _ => vec![],
                };
                assert_eq!(pens.len(), expected.len());
                for (actual, expected) in pens.iter().zip(expected) {
                    assert!(
                        (*actual - expected).abs() < 0.02,
                        "internal tab stop: {mode:?}/{direction:?}/{text:?}: {pens:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn combine_all_keeps_external_spacing_outside_split_sources() {
    use shodo::style::{TextAlign, TextJustify};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for direction in [Direction::Ltr, Direction::Rtl] {
            let mut input = style(mode, TextOrientation::Mixed);
            input.direction = direction;
            input.root.direction = direction;
            input.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            input.root.letter_spacing = 6.0;
            let mut child = input.root.clone();
            child.text_combine_upright = TextCombineUpright::All;
            let p = build_paragraph(&input, &Limits::default(), |builder| {
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(1),
                        offset: 0,
                    },
                    "水",
                );
                builder.open_inline(NodeId(2), &child, InlineEdges::default());
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(3),
                        offset: 0,
                    },
                    "12",
                );
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(4),
                        offset: 0,
                    },
                    "34",
                );
                builder.close_inline();
                builder.push_text(
                    TextSource::Dom {
                        node: NodeId(5),
                        offset: 0,
                    },
                    "水",
                );
            });
            for (options, expected_size, expected_start) in [
                (LineOptions::default(), 60.0, 22.0),
                (
                    LineOptions {
                        text_align: TextAlign::JustifyAll,
                        text_justify: TextJustify::InterCharacter,
                        ..Default::default()
                    },
                    80.0,
                    32.0,
                ),
            ] {
                let line = first_line(&p, 80.0, &options, &AtomicSizes::EMPTY);
                assert_eq!(line.inline_size(), expected_size);
                let runs: Vec<_> = line
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r)
                            if r.orientation() == shodo::GlyphOrientation::Combined =>
                        {
                            Some(r)
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(runs.len(), 2);
                let origins: Vec<_> = runs
                    .iter()
                    .flat_map(|r| (0..r.glyphs().len()).map(|g| r.glyph_origin(g).unwrap().0))
                    .collect();
                assert!(
                    origins.iter().all(|x| (*x - origins[0]).abs() < 0.001),
                    "split TCY must share square origin: {mode:?}/{direction:?}: {origins:?}"
                );
                // hhea ascent/descent 1160/288 at UPEM1000: 8 + (18.56 - 4.608)/2.
                let base = if direction == Direction::Ltr {
                    14.976
                } else {
                    16.0 - 14.976
                };
                assert!(
                    (origins[0] - expected_start - base).abs() < 0.02,
                    "external gap must precede the whole square: {mode:?}/{direction:?}, expected {expected_start}, starts/sizes {:?}, origins {origins:?}",
                    runs.iter()
                        .map(|r| (r.inline_start(), r.inline_size(), r.bidi_level()))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn combine_all_is_one_external_justification_character() {
    use shodo::style::{TextAlign, TextJustify};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for (text, justify) in [
            ("1234", TextJustify::InterCharacter),
            ("1 2", TextJustify::InterWord),
        ] {
            let mut combined = style(mode, TextOrientation::Mixed);
            combined.root.text_combine_upright = TextCombineUpright::All;
            combined.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
            let options = LineOptions {
                text_align: TextAlign::JustifyAll,
                text_justify: justify,
                ..Default::default()
            };
            let p = paragraph(&combined, text);
            let line = first_line(&p, 64.0, &options, &AtomicSizes::EMPTY);
            assert_eq!(
                line.inline_size(),
                16.0,
                "internal characters must not stretch the square: {text}"
            );
            let run = match line.fragment(0).unwrap() {
                Fragment::GlyphRun(run) => run,
                _ => panic!("TCY run"),
            };
            assert_eq!(
                run.inline_start(),
                24.0,
                "no external opportunity: center the 16px square"
            );
            assert_eq!(glyphs(&line).len(), text.chars().count());
        }
    }
}

#[test]
fn combine_all_keeps_horizontal_word_spacing_before_compression() {
    let mut horizontal = style(WritingMode::HorizontalTb, TextOrientation::Mixed);
    horizontal.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
    horizontal.root.word_spacing = 10.0;
    let plain = first_line(
        &paragraph(&horizontal, "1 2"),
        100.0,
        &LineOptions::default(),
        &AtomicSizes::EMPTY,
    );
    let expected = glyphs(&plain);
    let scale = 16.0 / plain.inline_size();
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut combined = horizontal.clone();
        combined.writing_mode = mode;
        combined.root.text_combine_upright = TextCombineUpright::All;
        let line = first_line(
            &paragraph(&combined, "1 2"),
            100.0,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        );
        let run = match line.fragment(0).unwrap() {
            Fragment::GlyphRun(run) => run,
            _ => panic!("word-spaced TCY"),
        };
        assert_eq!(line.inline_size(), 16.0);
        assert!((run.glyph_transform().block_x.abs() - scale).abs() < 0.001);
        let sign = if mode == WritingMode::VerticalRl {
            -1.0
        } else {
            1.0
        };
        for (actual, expected) in glyphs(&line).iter().zip(&expected) {
            assert_eq!(actual.id, expected.id);
            assert_eq!(actual.advance, expected.advance);
            assert!(
                (actual.block_offset - sign * (expected.inline_position * scale - 8.0)).abs()
                    < 0.001
            );
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
fn upright_font_size_adjust_scales_actual_vmtx_and_vhea_together() {
    use shodo::style::{FontMetricKind, FontSizeAdjust};
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let mut input = style(mode, TextOrientation::Upright);
        input.root.text_autospace = shodo::style::TextAutospace::NoAutospace;
        input.root.font_size_adjust = Some(FontSizeAdjust {
            metric: FontMetricKind::IcHeight,
            value: 1.5,
        });
        let p = paragraph(&input, "水");
        let line = first_line(&p, 100.0, &LineOptions::default(), &AtomicSizes::EMPTY);
        let run = match line.fragment(0).unwrap() {
            Fragment::GlyphRun(run) => run,
            _ => panic!("upright run"),
        };
        // The fixture's 水 has vmtx height1000 at UPEM1000. Adjusting
        // ic-height to1.5 scales the actual instance from16 to24px.
        assert_eq!(run.font_size(), 24.0);
        assert_eq!(glyphs(&line)[0].advance, 24.0);
        assert_eq!(line.block_size(), 24.0);
        assert_eq!(run.baseline(), 12.0);
        let metrics = run.vertical_metrics().unwrap();
        assert_eq!((metrics.ascent, metrics.descent), (12.0, 12.0));
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
