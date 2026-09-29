//! Mutations caught: reshaping/tinting accepted emoji, wrong bitmap bearings,
//! baseline/transform placement, missing whitespace, and unsafe decode bounds.
use shodo::geometry::{Direction, WritingMode};
use shodo::style::{FontFamily, InlineStyle, PaintStyle, ParagraphStyle, TextOrientation};
use shodo::{AtomicSizes, Fragment, LayoutContext, Line, RichText};
use shodo_fixtures::{EMOJI_FONTS, FONTS, load_emoji_fonts};
use shodo_harness::glyph_paint;
fn sample(text: &str, mode: WritingMode, color: [u8; 4]) -> Vec<Line> {
    let fonts = load_emoji_fonts(&Default::default()).unwrap();
    let s = InlineStyle {
        font_size: 24.,
        font_families: vec![FontFamily::Named(EMOJI_FONTS[0].family.into())],
        text_orientation: TextOrientation::Upright,
        paint: PaintStyle {
            color,
            ..Default::default()
        },
        ..Default::default()
    };
    let root = ParagraphStyle {
        root: s.clone(),
        writing_mode: mode,
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let p = RichText::new(&root)
        .push(text, &s)
        .build(&mut cx, &fonts.base.collection)
        .unwrap();
    p.break_all(&mut cx, &Default::default(), 400., &AtomicSizes::EMPTY)
}
fn intrinsic(image: &tiny_skia::Pixmap) -> usize {
    image
        .pixels()
        .iter()
        .filter(|p| p.alpha() > 0 && p.red() != p.green() && p.green() != p.blue())
        .count()
}
#[test]
fn real_color_glyph_paints_intrinsic_pixels() {
    let lines = sample("😀", WritingMode::HorizontalTb, [0, 0, 255, 255]);
    let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 100, 70).unwrap();
    assert_eq!(count, 1);
    assert!(intrinsic(&image) > 100);
    let (other, other_count) = glyph_paint::try_paint(&lines, |_| [255, 0, 0, 255], &[]).unwrap();
    assert_eq!(other_count, 1);
    assert!(intrinsic(&other) > 100);
    // Retained text color must not tint intrinsic CBDT pixels.
    let recolored = sample("😀", WritingMode::HorizontalTb, [255, 0, 0, 128]);
    assert_eq!(
        glyph_paint::try_paint_styled_on_canvas(&recolored, 100, 70)
            .unwrap()
            .0
            .data(),
        image.data()
    );
}
#[test]
fn bitmap_font_spaces_and_ignored_selectors_draw_no_ink() {
    let text = "😀 ☺️👩‍💻🇯🇵1️⃣";
    let lines = sample(text, WritingMode::HorizontalTb, [0, 0, 255, 255]);
    let count: usize = lines
        .iter()
        .flat_map(|l| l.fragments())
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r.glyphs().len())
            } else {
                None
            }
        })
        .sum();
    let (image, drawn) = glyph_paint::try_paint_styled_on_canvas(&lines, 240, 70).unwrap();
    assert_eq!(drawn, count);
    assert!(intrinsic(&image) > 500);
}
#[test]
fn vertical_upright_color_and_clipping_use_public_transforms() {
    for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
        let lines = sample("😀👩‍💻", mode, [0, 0, 0, 255]);
        let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 100, 160).unwrap();
        assert_eq!(count, 2);
        assert!(intrinsic(&image) > 200);
        assert_eq!(
            glyph_paint::try_paint_styled_on_canvas(&lines, 10, 10).err(),
            Some(glyph_paint::PaintError::ClippedOutput)
        );
    }
    let lines = sample("😀", WritingMode::HorizontalTb, [0, 0, 0, 255]);
    assert_eq!(
        glyph_paint::try_paint_on_canvas(&lines, |_| [0, 0, 0, 255], &[], 20, 50).err(),
        Some(glyph_paint::PaintError::ClippedOutput)
    );
}

#[test]
fn vertical_bitmap_pixels_match_independent_upright_placement() {
    use skrifa::{
        FontRef, GlyphId,
        bitmap::{BitmapData, BitmapFormat, BitmapStrikes},
        instance::Size,
    };

    let font = FontRef::new(EMOJI_FONTS[0].bytes).unwrap();
    let bitmap = BitmapStrikes::with_format(&font, BitmapFormat::Cbdt)
        .unwrap()
        .glyph_for_size(Size::new(24.), GlyphId::new(16))
        .unwrap();
    let BitmapData::Png(png) = bitmap.data else {
        panic!("fixed grinning-face fixture must contain a PNG");
    };
    assert_eq!((bitmap.width, bitmap.height), (136, 128));
    assert_eq!((bitmap.ppem_x, bitmap.ppem_y), (109., 109.));
    assert_eq!((bitmap.inner_bearing_x, bitmap.inner_bearing_y), (0., 101.));
    let decoded = tiny_skia::Pixmap::decode_png(png).unwrap();

    // Fixed font: UPEM 2048, h advance 2550, v advance 2500,
    // vhea ascender/descender +/-1275, hhea ascender 1900; no VORG/outline.
    // At 24px the vertical origin is (14.9375, 22.265625), after 1/64px
    // rounding. The empty-cluster primary face is the mono fixture, whose
    // vhea ascender/descender are 1900/-500. Its strut contributes 22.265625px
    // above; the color run contributes 14.94140625px below. Their union is
    // ceil-to-1/64(37.20703125) = 37.21875px wide, with baselines
    // 22.265625 (rl) and 37.21875-22.265625 = 14.953125 (lr).
    // With a 10px inset and an 80px inner width, upright origins are
    // x=10+80-22.265625-14.9375 (rl), x=10+14.953125-14.9375 (lr).
    // Two cmap U+1F600 -> gid16 glyphs are 2500*24/2048=29.296875px apart.
    // Expected placement uses these literals, never accepted glyph origins,
    // glyph transforms, PhysicalConverter, or the caller's bitmap painter.
    let scale = 24. / 109.;
    for (mode, x) in [
        (WritingMode::VerticalRl, 52.796875),
        (WritingMode::VerticalLr, 10.015625),
    ] {
        let mut expected = tiny_skia::Pixmap::new(100, 160).unwrap();
        expected.fill(tiny_skia::Color::WHITE);
        for y in [32.265625, 61.5625] {
            expected.draw_pixmap(
                0,
                0,
                decoded.as_ref(),
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Bilinear,
                    ..Default::default()
                },
                tiny_skia::Transform::from_row(scale, 0., 0., scale, x, y - 101. * scale),
                None,
            );
        }
        let lines = sample("😀😀", mode, [0, 0, 0, 255]);
        let (actual, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 100, 160).unwrap();
        assert_eq!(count, 2);
        // Permit one RGBA quantization unit, as in the translation test below.
        let mismatch = actual
            .data()
            .iter()
            .zip(expected.data())
            .enumerate()
            .find(|(_, (actual, expected))| actual.abs_diff(**expected) > 1);
        assert!(
            mismatch.is_none(),
            "{mode:?}: upright placement/orientation pixel mismatch: {mismatch:?}"
        );
    }
}
#[test]
fn latin_color_and_decorations_coexist_with_intrinsic_emoji() {
    let fonts = load_emoji_fonts(&Default::default()).unwrap();
    let s = InlineStyle {
        font_size: 24.,
        font_families: vec![
            FontFamily::Named(FONTS[0].family.into()),
            FontFamily::Named(EMOJI_FONTS[0].family.into()),
        ],
        paint: PaintStyle {
            color: [0, 0, 255, 255],
            underline: Some(shodo::style::TextDecoration {
                color: Some([255, 0, 0, 255]),
                offset: Some(8.),
                thickness: Some(2.),
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let root = ParagraphStyle {
        root: s.clone(),
        direction: Direction::Ltr,
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let p = RichText::new(&root)
        .push("A😀B", &s)
        .build(&mut cx, &fonts.base.collection)
        .unwrap();
    let lines = p.break_all(&mut cx, &Default::default(), 400., &AtomicSizes::EMPTY);
    let (image, count) = glyph_paint::try_paint_styled_on_canvas(&lines, 140, 80).unwrap();
    assert_eq!(count, 3);
    assert!(intrinsic(&image) > 100);
    assert!(
        image
            .pixels()
            .iter()
            .filter(|p| p.blue() == 255 && p.red() == 0 && p.green() == 0)
            .count()
            > 20
    );
    assert!(
        image
            .pixels()
            .iter()
            .filter(|p| p.red() == 255 && p.blue() == 0 && p.green() == 0)
            .count()
            > 20
    );
}

#[test]
fn accepted_glyph_id_and_origin_control_bitmap_pixels() {
    use glyph_paint::bitmap_paint::paint_bitmap;
    use skrifa::{FontRef, GlyphId};
    let font = FontRef::new(EMOJI_FONTS[0].bytes).unwrap();
    let mut first = tiny_skia::Pixmap::new(140, 100).unwrap();
    let mut shifted = first.clone();
    let mut different = first.clone();
    assert!(
        paint_bitmap(
            &font,
            GlyphId::new(16),
            24.,
            tiny_skia::Transform::from_translate(20., 50.),
            &mut first,
            true
        )
        .unwrap()
    );
    assert!(
        paint_bitmap(
            &font,
            GlyphId::new(16),
            24.,
            tiny_skia::Transform::from_translate(60., 50.),
            &mut shifted,
            true
        )
        .unwrap()
    );
    assert!(
        paint_bitmap(
            &font,
            GlyphId::new(5),
            24.,
            tiny_skia::Transform::from_translate(20., 50.),
            &mut different,
            true
        )
        .unwrap()
    );
    assert!(intrinsic(&first) > 100);
    assert_ne!(first.data(), different.data());
    // Independent CBDT metrics:136x128, inner bearings(0,101), strike109ppem.
    // At24px and origin(20,50), image bounds are[20,49.945]x[27.761,55.945].
    for y in 0..100 {
        for x in 0..100 {
            let a = first.pixel(x, y).unwrap();
            let b = shifted.pixel(x + 40, y).unwrap();
            for (a, b) in [a.red(), a.green(), a.blue(), a.alpha()].into_iter().zip([
                b.red(),
                b.green(),
                b.blue(),
                b.alpha(),
            ]) {
                // Bilinear sampling through float inverse matrices can differ
                // by one quantization unit under an integer translation.
                assert!(a.abs_diff(b) <= 1, "translated channel {a} != {b}");
            }
            if first.pixel(x, y).unwrap().alpha() > 0 {
                assert!((20..51).contains(&x) && (27..57).contains(&y));
            }
        }
    }
}

#[test]
fn decoder_preserves_premultiplied_alpha() {
    use glyph_paint::bitmap_paint::paint_bitmap_glyph;
    use skrifa::{
        FontRef, GlyphId,
        bitmap::{BitmapData, BitmapFormat, BitmapStrikes},
        instance::Size,
    };
    let font = FontRef::new(EMOJI_FONTS[0].bytes).unwrap();
    let mut glyph = BitmapStrikes::with_format(&font, BitmapFormat::Cbdt)
        .unwrap()
        .glyph_for_size(Size::new(24.), GlyphId::new(16))
        .unwrap();
    let mut pixel = tiny_skia::Pixmap::new(1, 1).unwrap();
    pixel.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 128));
    let png = pixel.encode_png().unwrap();
    glyph.data = BitmapData::Png(&png);
    glyph.width = 1;
    glyph.height = 1;
    glyph.ppem_x = 1.;
    glyph.ppem_y = 1.;
    glyph.inner_bearing_x = 0.;
    glyph.inner_bearing_y = 0.;
    let mut target = tiny_skia::Pixmap::new(4, 4).unwrap();
    paint_bitmap_glyph(
        &glyph,
        1.,
        tiny_skia::Transform::from_translate(1., 1.),
        &mut target,
        true,
    )
    .unwrap();
    let got = target.pixel(1, 1).unwrap();
    assert_eq!(
        [got.red(), got.green(), got.blue(), got.alpha()],
        [128, 0, 0, 128]
    );
}

#[test]
fn invalid_bitmaps_and_placement_leave_target_untouched() {
    use glyph_paint::{PaintError, bitmap_paint::paint_bitmap_glyph};
    use skrifa::{
        FontRef, GlyphId,
        bitmap::{BitmapData, BitmapFormat, BitmapStrikes, Origin},
        instance::Size,
    };
    let font = FontRef::new(EMOJI_FONTS[0].bytes).unwrap();
    let actual = BitmapStrikes::with_format(&font, BitmapFormat::Cbdt)
        .unwrap()
        .glyph_for_size(Size::new(24.), GlyphId::new(16))
        .unwrap();
    let mut canvas = tiny_skia::Pixmap::new(100, 100).unwrap();
    let before = canvas.data().to_vec();
    let mut invalid = Vec::new();
    let mut g = actual.clone();
    g.data = BitmapData::Png(b"bad PNG");
    invalid.push(g);
    let mut g = actual.clone();
    g.width += 1;
    invalid.push(g);
    let mut g = actual.clone();
    g.ppem_y = 0.;
    invalid.push(g);
    let mut g = actual.clone();
    g.inner_bearing_x = f32::NAN;
    invalid.push(g);
    let mut g = actual.clone();
    g.width = u32::MAX;
    g.height = u32::MAX;
    invalid.push(g);
    let mut truncated = actual.clone();
    let BitmapData::Png(png) = actual.data else {
        panic!()
    };
    truncated.data = BitmapData::Png(&png[..33]);
    invalid.push(truncated);
    for g in invalid {
        assert_eq!(
            paint_bitmap_glyph(
                &g,
                24.,
                tiny_skia::Transform::from_translate(20., 50.),
                &mut canvas,
                true
            ),
            Err(PaintError::InvalidBitmap)
        );
        assert_eq!(canvas.data(), before);
    }
    let mut unsupported = actual.clone();
    unsupported.data = BitmapData::Bgra(&[]);
    assert_eq!(
        paint_bitmap_glyph(
            &unsupported,
            24.,
            tiny_skia::Transform::from_translate(20., 50.),
            &mut canvas,
            true
        ),
        Err(PaintError::UnsupportedBitmap)
    );
    unsupported = actual.clone();
    unsupported.placement_origin = Origin::BottomLeft;
    assert_eq!(
        paint_bitmap_glyph(
            &unsupported,
            24.,
            tiny_skia::Transform::from_translate(20., 50.),
            &mut canvas,
            true
        ),
        Err(PaintError::UnsupportedBitmap)
    );
    for transform in [
        tiny_skia::Transform::from_translate(f32::NAN, 50.),
        tiny_skia::Transform::from_scale(0., 0.),
    ] {
        assert_eq!(
            paint_bitmap_glyph(&actual, 24., transform, &mut canvas, true),
            Err(PaintError::InvalidBitmap)
        );
    }
    assert_eq!(
        paint_bitmap_glyph(
            &actual,
            24.,
            tiny_skia::Transform::from_translate(-40., 50.),
            &mut canvas,
            true
        ),
        Err(PaintError::ClippedOutput)
    );
    assert_eq!(canvas.data(), before);
}

#[test]
fn color_glyph_synthesis_is_rejected_before_rasterization() {
    let fonts = load_emoji_fonts(&Default::default()).unwrap();
    let s = InlineStyle {
        font_size: 24.,
        font_style: shodo::style::FontStyle::Oblique(14.),
        font_families: vec![FontFamily::Named(EMOJI_FONTS[0].family.into())],
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    let p = RichText::new(&ParagraphStyle {
        root: s.clone(),
        ..Default::default()
    })
    .push("😀", &s)
    .build(&mut cx, &fonts.base.collection)
    .unwrap();
    let lines = p.break_all(&mut cx, &Default::default(), 400., &AtomicSizes::EMPTY);
    assert!(
        lines
            .iter()
            .flat_map(|l| l.fragments())
            .any(|f| matches!(f,Fragment::GlyphRun(r) if r.skew().is_some()))
    );
    assert_eq!(
        glyph_paint::try_paint_styled_on_canvas(&lines, 100, 70).err(),
        Some(glyph_paint::PaintError::UnsupportedSynthesis)
    );
}
