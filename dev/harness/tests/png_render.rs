use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{AtomicSize, AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};
use shodo_harness::glyph_paint;

#[test]
fn vertical_outlines_match_literal_physical_rotation_and_compression() {
    use shodo::geometry::{Direction, PhysicalConverter, PhysicalSize, WritingMode};
    use shodo::style::{TextCombineUpright, TextOrientation};
    use skrifa::{
        FontRef, GlyphId, MetadataProvider,
        instance::{LocationRef, Size},
        outline::{DrawSettings, OutlinePen},
    };
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
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    for mode in [
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
                for combine in [false, true] {
                    let style = ParagraphStyle {
                        writing_mode: mode,
                        direction,
                        root: InlineStyle {
                            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
                            font_size: 32.0,
                            direction,
                            text_orientation: orientation,
                            text_combine_upright: if combine {
                                TextCombineUpright::All
                            } else {
                                TextCombineUpright::None
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    // F's asymmetric outline makes a mirrored/sideways glyph visible.
                    let mut builder = ParagraphBuilder::new(&style, &limits);
                    builder.push_text(
                        TextSource::Generated { node: NodeId(1) },
                        if combine { "FFF" } else { "F" },
                    );
                    let p = builder
                        .build(&mut LayoutContext::new(), &fonts.collection)
                        .unwrap_or_else(|e| {
                            panic!("{mode:?}/{orientation:?}/{direction:?}/{combine}: {e}")
                        });
                    let lines = p.break_all(
                        &mut LayoutContext::new(),
                        &Default::default(),
                        200.0,
                        &AtomicSizes::EMPTY,
                    );
                    assert_eq!(lines.len(), 1);
                    assert_eq!(lines[0].writing_mode(), mode);
                    let (actual, count) =
                        glyph_paint::try_paint_on_canvas(&lines, |_| [0, 0, 0, 255], &[], 256, 256)
                            .unwrap_or_else(|e| {
                                panic!("{mode:?}/{orientation:?}/{direction:?}/{combine}: {e}")
                            });
                    let mut expected = tiny_skia::Pixmap::new(256, 256).unwrap();
                    expected.fill(tiny_skia::Color::WHITE);
                    let converter = PhysicalConverter::new(
                        mode,
                        lines[0].used_direction(),
                        PhysicalSize {
                            width: 236.0,
                            height: 236.0,
                        },
                    );
                    let mut expected_count = 0;
                    for fragment in lines[0].fragments() {
                        let Fragment::GlyphRun(run) = fragment else {
                            continue;
                        };
                        let data = run.font_data().unwrap();
                        let font = FontRef::from_index(data.data.as_ref(), data.index).unwrap();
                        let natural: f32 = run.glyphs().map(|g| g.advance).sum();
                        let scale = (32.0 / natural).min(1.0);
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(0, 0, 0, 255);
                        // Literal physical x/y-up columns: upright, CW, CCW.
                        // Do not use glyph_transform to derive the expectation.
                        let columns = if matches!(mode, WritingMode::SidewaysLr) {
                            (0.0, -1.0, -1.0, 0.0)
                        } else if matches!(mode, WritingMode::SidewaysRl) {
                            (0.0, 1.0, 1.0, 0.0)
                        } else if combine {
                            (scale, 0.0, 0.0, -1.0)
                        } else if orientation == TextOrientation::Upright {
                            (1.0, 0.0, 0.0, -1.0)
                        } else {
                            (0.0, 1.0, 1.0, 0.0)
                        };
                        for (index, glyph) in run.glyphs().enumerate() {
                            let outline =
                                font.outline_glyphs().get(GlyphId::new(glyph.id)).unwrap();
                            let mut pen = Pen(tiny_skia::PathBuilder::new());
                            outline
                                .draw(
                                    DrawSettings::unhinted(
                                        Size::new(run.font_size()),
                                        LocationRef::new(run.normalized_coords()),
                                    ),
                                    &mut pen,
                                )
                                .unwrap();
                            let (inline, block) = run.glyph_origin(index).unwrap();
                            let (x, y) = converter.point(inline, block + lines[0].block_offset());
                            if let Some(path) = pen.0.finish() {
                                let transform = tiny_skia::Transform::from_row(
                                    columns.0,
                                    columns.1,
                                    columns.2,
                                    columns.3,
                                    10.0 + x,
                                    10.0 + y,
                                );
                                expected.fill_path(
                                    &path,
                                    &paint,
                                    tiny_skia::FillRule::Winding,
                                    transform,
                                    None,
                                );
                            }
                            expected_count += 1;
                        }
                    }
                    assert_eq!(count, expected_count);
                    assert!(
                        expected
                            .data()
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|p| p[0] < 128)
                    );
                    assert!(
                        actual.data() == expected.data(),
                        "pixel mismatch: {mode:?}/{orientation:?}/{direction:?}/{combine}"
                    );
                }
            }
        }
    }
}

#[test]
fn atomic_border_rectangle_is_painted_with_accepted_line_block_offset() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 32.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style.clone(),
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    )
    .push_forced_break(NodeId(88))
    .push_atomic(NodeId(9), &style, Default::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(2),
            offset: 0,
        },
        "b",
    );
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(9),
        AtomicSize {
            inline_size: 30.0,
            block_size: 20.0,
            baseline: Some(20.0),
            margins: Default::default(),
        },
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &atomics,
    );
    assert_eq!(lines.len(), 2);
    assert!(lines[1].block_offset() > 0.0);
    let atomic = lines[1]
        .fragments()
        .find_map(|f| match f {
            Fragment::Atomic(a) => Some(a),
            _ => None,
        })
        .unwrap();
    assert_eq!(atomic.border_rect.inline_size, 30.0);
    assert_eq!(atomic.border_rect.block_size, 20.0);
    let (image, count) = glyph_paint::try_paint(
        &lines,
        |owner| {
            if owner == NodeId(9) {
                [0, 128, 0, 255]
            } else {
                [0, 0, 0, 255]
            }
        },
        &[],
    )
    .unwrap();
    assert_eq!(count, 2);
    let x = (10.0 + atomic.border_rect.inline_start + 15.0) as usize;
    let y = (10.0 + lines[1].block_offset() + atomic.border_rect.block_start + 10.0) as usize;
    let index = 4 * (y * image.width() as usize + x);
    assert_eq!(&image.data()[index..index + 4], &[0, 128, 0, 255]);
    assert!(
        image
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
    );
}

#[test]
fn unsupported_synthetic_weight_is_reported_instead_of_silently_ignored() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_weight: 700.0,
        font_size: 32.0,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style,
            ..Default::default()
        },
        &limits,
    );
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &AtomicSizes::EMPTY,
    );
    assert!(
        lines[0]
            .fragments()
            .any(|f| matches!(f,Fragment::GlyphRun(r) if r.embolden()))
    );
    assert!(matches!(
        glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]),
        Err(glyph_paint::PaintError::UnsupportedSynthesis)
    ));
}

#[test]
fn fallback_stub_is_reported_instead_of_drawing_invented_outlines() {
    let limits = Default::default();
    let fonts = shodo::font::FontCollection::with_options(
        &limits,
        shodo::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        "a",
    );
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        200.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(
        glyph_paint::try_paint(&lines, |_| [0, 0, 0, 255], &[]).map(|(_, count)| count),
        Err(glyph_paint::PaintError::MissingOutline)
    );
}
