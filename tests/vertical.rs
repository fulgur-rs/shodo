mod common;

use common::{first_line, glyphs};
use shodo::font::{FontCollection, FontFaceDescriptor, FontOptions};
use shodo::geometry::{BaselineKind, Direction, PhysicalConverter, PhysicalSize, WritingMode};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, FontFeature, FontKerning, InlineStyle, LineOptions, ParagraphStyle,
    TextOrientation, TextSpacingTrim,
};
use shodo::{AtomicSizes, Fragment, LayoutContext, Paragraph, ParagraphBuilder};

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
    let mut builder = ParagraphBuilder::new(style, &limits);
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
    );
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
                direction,
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
            if direction == Direction::Rtl {
                assert_eq!(inline, glyph.inline_position + 16.0);
            } else {
                assert_eq!(inline, glyph.inline_position);
            }
        }
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
