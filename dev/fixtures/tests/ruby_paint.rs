//! Real retained output: missing lanes, wrong source paint, nested displacement,
//! mirrored vertical outlines, intrinsic bitmap tint and clipping regressions.
#[path = "../examples/support/glyph_paint.rs"]
mod glyph_paint;
use shodo::geometry::{PhysicalConverter, PhysicalSize, WritingMode};
use shodo::limits::Limits;
use shodo::node::{NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, PaintStyle, ParagraphStyle, TextDecoration, TextOrientation,
};
use shodo::{
    AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder, Ruby, RubyAlign, RubyAnnotation,
    RubyBase, RubyContent, RubyLevel, RubyOverhang, RubyPosition, RubyStyle, RubyVisibility,
};
use shodo_fixtures::{EMOJI_FONTS, FONTS, load_emoji_fonts, load_fonts};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};
const RED: [u8; 4] = [180, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 180, 255];
const GREEN: [u8; 4] = [0, 140, 0, 255];
fn style(size: f32, family: &str, color: [u8; 4]) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named(family.into())],
        paint: PaintStyle {
            color,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn cjk(size: f32, color: [u8; 4]) -> InlineStyle {
    style(size, FONTS[1].family, color)
}
fn content(node: u64, text: &str, style: &InlineStyle) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 40,
        },
        text,
        style,
        &Limits::default(),
    )
}
fn ruby(
    base: &str,
    reading: RubyContent,
    visibility: RubyVisibility,
    position: RubyPosition,
) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: content(10, base, &cjk(24.0, RED)),
            align: RubyAlign::Center,
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: reading,
                span: shodo::RubySpan::All,
                visibility,
            }],
            style: RubyStyle {
                position,
                overhang: RubyOverhang::None,
                ..Default::default()
            },
        }],
    )
    .unwrap()
}
fn builder(mode: WritingMode) -> ParagraphBuilder {
    ParagraphBuilder::new(
        &ParagraphStyle {
            writing_mode: mode,
            root: cjk(24.0, RED),
            ..Default::default()
        },
        &Limits::default(),
    )
}
fn layout(b: ParagraphBuilder) -> Vec<Line> {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(p.warnings().is_empty(), "{:?}", p.warnings());
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &AtomicSizes::EMPTY,
    )
}
fn dominant(image: &tiny_skia::Pixmap, channel: usize) -> usize {
    image
        .pixels()
        .iter()
        .filter(|p| {
            let rgb = [p.red(), p.green(), p.blue()];
            (0..3).all(|i| i == channel || rgb[channel] > rgb[i])
        })
        .count()
}

#[test]
fn ruby_painter_consumes_retained_lanes_and_source_decorations_once() {
    let mut b = builder(WritingMode::HorizontalTb);
    for (base, reading) in [("日本語", "にほんご"), ("読み", "よみ")] {
        let mut s = cjk(12.0, BLUE);
        s.paint.underline = Some(TextDecoration {
            color: Some(GREEN),
            offset: Some(2.0),
            thickness: Some(1.0),
        });
        b.push_ruby(
            NodeId(8),
            &cjk(24.0, RED),
            ruby(
                base,
                content(20, reading, &s),
                RubyVisibility::Visible,
                RubyPosition::Over,
            ),
        );
    }
    let lines = layout(b);
    // The accepted Lines keep real font bytes after Paragraph/context drop.
    for line in &lines {
        for a in line.ruby_annotations() {
            for f in a.line().fragments() {
                if let Fragment::GlyphRun(r) = f {
                    assert_eq!(r.font_data().unwrap().data.as_ref(), FONTS[1].bytes);
                    assert!(r.glyphs().all(|g| g.id != 0));
                }
            }
        }
    }
    let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 256, 180).unwrap();
    assert_eq!(
        count, 11,
        "five base and six reading glyphs must each paint once"
    );
    assert!(dominant(&image, 0) > 30);
    assert!(dominant(&image, 2) > 20);
    assert!(dominant(&image, 1) > 20);
    let mut owners = Vec::new();
    let (image, count) = glyph_paint::try_paint_on_canvas(
        &lines,
        |node| {
            owners.push(node);
            if node == NodeId(20) { BLUE } else { RED }
        },
        &[],
        256,
        180,
    )
    .unwrap();
    assert_eq!(count, 11);
    assert!(owners.contains(&NodeId(20)));
    assert!(dominant(&image, 2) > 20);
}

struct Pen(tiny_skia::PathBuilder);
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.0.quad_to(x, y, z, w);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.0.cubic_to(a, b, c, d, e, f);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
fn outline(run: shodo::GlyphRunView<'_>, id: u32) -> tiny_skia::Path {
    let data = run.font_data().unwrap();
    let font = FontRef::from_index(data.data.as_ref(), data.index).unwrap();
    let glyph = font.outline_glyphs().get(GlyphId::new(id)).unwrap();
    let mut pen = Pen(tiny_skia::PathBuilder::new());
    glyph
        .draw(
            DrawSettings::unhinted(
                Size::new(run.font_size()),
                LocationRef::new(run.normalized_coords()),
            ),
            &mut pen,
        )
        .unwrap();
    pen.0.finish().unwrap()
}
fn parent_point(a: shodo::RubyAnnotationView<'_>, x: f32, y: f32) -> (f32, f32) {
    let t = a.transform();
    (
        t.inline_inline * x + t.inline_block * y + t.inline_offset,
        t.block_inline * x + t.block_block * y + t.block_offset,
    )
}
fn only_blue(image: &tiny_skia::Pixmap) -> Vec<u8> {
    image
        .pixels()
        .iter()
        .flat_map(|p| {
            if p.blue() > p.red() && p.blue() > p.green() {
                [p.red(), p.green(), p.blue(), p.alpha()]
            } else {
                [255, 255, 255, 255]
            }
        })
        .collect()
}

#[test]
fn ruby_vertical_pixel_bounds_match_literal_asymmetric_outline_orientation() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        for position in [RubyPosition::Over, RubyPosition::Under] {
            for orientation in [TextOrientation::Upright, TextOrientation::Sideways] {
                let mut s = style(16.0, FONTS[0].family, BLUE);
                s.text_orientation = orientation;
                let mut b = builder(mode);
                b.push_ruby(
                    NodeId(8),
                    &cjk(24.0, RED),
                    ruby(
                        "日",
                        content(20, "F", &s),
                        RubyVisibility::Visible,
                        position,
                    ),
                );
                let lines = layout(b);
                let a = lines[0].ruby_annotations().next().unwrap();
                let run = a
                    .line()
                    .fragments()
                    .find_map(|f| match f {
                        Fragment::GlyphRun(r) => Some(r),
                        _ => None,
                    })
                    .unwrap();
                let glyph = run.glyphs().next().unwrap();
                assert_ne!(glyph.id, 0);
                let (actual, count) =
                    glyph_paint::try_paint_styled_on_canvas(&lines, 256, 256).unwrap();
                assert_eq!(count, 2);
                let origin = run.glyph_origin(0).unwrap();
                let origin = parent_point(a, origin.0, origin.1);
                let converter = PhysicalConverter::new(
                    mode,
                    lines[0].used_direction(),
                    PhysicalSize {
                        width: 236.0,
                        height: 236.0,
                    },
                );
                let (x, y) = converter.point(origin.0, origin.1 + lines[0].block_offset());
                // Literal physical columns, independent of glyph_transform:
                // upright F stays F; sideways F rotates clockwise in both modes.
                let (xx, xy, yx, yy) = if orientation == TextOrientation::Upright {
                    (1.0, 0.0, 0.0, -1.0)
                } else {
                    (0.0, 1.0, 1.0, 0.0)
                };
                let mut expected = tiny_skia::Pixmap::new(256, 256).unwrap();
                expected.fill(tiny_skia::Color::WHITE);
                let mut paint = tiny_skia::Paint::default();
                paint.set_color_rgba8(0, 0, 180, 255);
                expected.fill_path(
                    &outline(run, glyph.id),
                    &paint,
                    tiny_skia::FillRule::Winding,
                    tiny_skia::Transform::from_row(xx, xy, yx, yy, 10.0 + x, 10.0 + y),
                    None,
                );
                assert!(dominant(&expected, 2) > 0);
                assert_eq!(
                    only_blue(&actual),
                    expected.data(),
                    "{mode:?}/{position:?}/{orientation:?}"
                );
            }
        }
    }
}

#[test]
fn ruby_nested_translation_is_composed_once_on_a_later_line() {
    for mode in [
        WritingMode::HorizontalTb,
        WritingMode::VerticalRl,
        WritingMode::VerticalLr,
    ] {
        let mut child = builder(mode);
        child.push_ruby(
            NodeId(81),
            &cjk(24.0, RED),
            ruby(
                "本",
                content(20, "に", &cjk(12.0, BLUE)),
                RubyVisibility::Visible,
                RubyPosition::Over,
            ),
        );
        let outer = ruby(
            "日",
            RubyContent::from_builder(child),
            RubyVisibility::Visible,
            RubyPosition::Under,
        );
        let mut b = builder(mode);
        b.push_text(
            TextSource::Dom {
                node: NodeId(30),
                offset: 0,
            },
            "語",
        )
        .push_forced_break(NodeId(31));
        b.push_ruby(NodeId(8), &cjk(24.0, RED), outer);
        let lines = layout(b);
        let outside = lines[1].ruby_annotations().next().unwrap();
        let inside = outside.line().ruby_annotations().next().unwrap();
        let run = inside
            .line()
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        let glyph = run.glyphs().next().unwrap();
        let (actual, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 256, 256).unwrap();
        assert_eq!(count, 4);
        let origin = run.glyph_origin(0).unwrap();
        let origin = parent_point(inside, origin.0, origin.1);
        let origin = parent_point(outside, origin.0, origin.1);
        let converter = PhysicalConverter::new(
            mode,
            lines[1].used_direction(),
            PhysicalSize {
                width: 236.0,
                height: 236.0,
            },
        );
        let (x, y) = converter.point(origin.0, origin.1 + lines[1].block_offset());
        let mut expected = tiny_skia::Pixmap::new(256, 256).unwrap();
        expected.fill(tiny_skia::Color::WHITE);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(0, 0, 180, 255);
        expected.fill_path(
            &outline(run, glyph.id),
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::from_row(1.0, 0.0, 0.0, -1.0, 10.0 + x, 10.0 + y),
            None,
        );
        assert!(dominant(&expected, 2) > 0);
        assert_eq!(only_blue(&actual), expected.data(), "{mode:?}");
    }
}

#[test]
fn ruby_color_annotations_hidden_and_collapsed_use_retained_bitmap_policy() {
    let fonts = load_emoji_fonts(&Limits::default()).unwrap();
    for visibility in [
        RubyVisibility::Visible,
        RubyVisibility::Hidden,
        RubyVisibility::Collapse,
    ] {
        let mut b = builder(WritingMode::HorizontalTb);
        b.push_ruby(
            NodeId(8),
            &cjk(24.0, RED),
            ruby(
                "日",
                content(20, "😀", &style(16.0, EMOJI_FONTS[0].family, BLUE)),
                visibility,
                RubyPosition::Over,
            ),
        );
        let p = b
            .build(&mut LayoutContext::new(), &fonts.base.collection)
            .unwrap();
        assert!(p.warnings().is_empty());
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            200.0,
            &AtomicSizes::EMPTY,
        );
        let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 256, 100).unwrap();
        assert_eq!(
            count,
            if visibility == RubyVisibility::Visible {
                2
            } else {
                1
            }
        );
        let intrinsic = image
            .pixels()
            .iter()
            .filter(|p| p.red() != p.green() && p.green() != p.blue())
            .count();
        assert_eq!(intrinsic > 20, visibility == RubyVisibility::Visible);
    }
}

#[test]
fn ruby_annotation_clipping_is_rejected_when_the_base_fits() {
    let mut b = builder(WritingMode::HorizontalTb);
    b.push_ruby(
        NodeId(8),
        &cjk(24.0, RED),
        ruby(
            "日",
            content(20, "日本語", &cjk(36.0, BLUE)),
            RubyVisibility::Visible,
            RubyPosition::Under,
        ),
    );
    let lines = layout(b);
    assert_eq!(
        glyph_paint::try_paint_styled_on_canvas(&lines, 160, 55).err(),
        Some(glyph_paint::PaintError::ClippedOutput)
    );
}

#[test]
fn ruby_inter_character_pixels_use_the_complete_axis_conversion_in_both_directions() {
    use shodo::geometry::Direction;
    for direction in [Direction::Ltr, Direction::Rtl] {
        let base = InlineStyle {
            direction,
            ..cjk(24.0, RED)
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                direction,
                root: base.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(
            NodeId(8),
            &base,
            ruby(
                "日",
                content(20, "にほん", &cjk(12.0, BLUE)),
                RubyVisibility::Visible,
                RubyPosition::InterCharacter,
            ),
        );
        let lines = layout(b);
        let a = lines[0].ruby_annotations().next().unwrap();
        assert_eq!(a.line().writing_mode(), WritingMode::VerticalRl);
        let (actual, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 256, 256).unwrap();
        assert_eq!(count, 4);
        let converter = PhysicalConverter::new(
            WritingMode::HorizontalTb,
            direction,
            PhysicalSize {
                width: 236.0,
                height: 236.0,
            },
        );
        let mut expected = tiny_skia::Pixmap::new(256, 256).unwrap();
        expected.fill(tiny_skia::Color::WHITE);
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(0, 0, 180, 255);
        for f in a.line().fragments() {
            if let Fragment::GlyphRun(run) = f {
                for (index, glyph) in run.glyphs().enumerate() {
                    let origin = run.glyph_origin(index).unwrap();
                    let origin = parent_point(a, origin.0, origin.1);
                    let (x, y) = converter.point(origin.0, origin.1 + lines[0].block_offset());
                    // CJK reading outlines remain physically upright even
                    // though the child and parent logical axes differ.
                    expected.fill_path(
                        &outline(run, glyph.id),
                        &paint,
                        tiny_skia::FillRule::Winding,
                        tiny_skia::Transform::from_row(1.0, 0.0, 0.0, -1.0, 10.0 + x, 10.0 + y),
                        None,
                    );
                }
            }
        }
        assert!(dominant(&expected, 2) > 0);
        assert_eq!(only_blue(&actual), expected.data(), "{direction:?}");
    }
}
