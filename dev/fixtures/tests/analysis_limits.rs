use shodo::font::{FontCollection, FontOptions};
use shodo::limits::{Limits, WarningKind};
use shodo::node::{NodeId, TextSource};
use shodo::style::{OverflowWrap, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

#[test]
fn missing_font_non_latin_marks_and_default_ignorables_have_no_advance() {
    let fonts = FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    for suffix in [
        "\u{64b}",
        "\u{1ab0}",
        "\u{93e}",
        "\u{fe0f}",
        "\u{e0100}",
        "\u{200d}",
        "\u{200c}",
        "\u{2060}",
        "\u{200b}",
    ] {
        let text = format!("a{suffix}");
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        let actual = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].inline_size(), 16.0, "{suffix:?}");
        assert_eq!(actual[0].text_range(), 0..text.len());
        let advances: Vec<_> = actual[0]
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.advance))
            .collect();
        assert_eq!(advances.iter().sum::<f32>(), 16.0, "{suffix:?}");
    }
}

#[test]
fn tiny_limit_combining_sequence_terminates() {
    for budget in 0..=4 {
        let limits = Limits {
            max_shaping_run_bytes: Some(budget),
            max_warnings: Some(2),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut style = ParagraphStyle::default();
        style.root.overflow_wrap = OverflowWrap::Anywhere;
        let text = format!("a{}", "\u{64b}".repeat(300));
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert!(
            p.warnings()
                .iter()
                .any(|w| w.kind == WarningKind::Unsupported)
        );
        assert!(p.warnings().len() <= 3);
        let actual = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(actual.len(), 1, "budget {budget}: grapheme retained whole");
        assert_eq!(actual[0].text_range(), 0..text.len());
        assert_eq!(actual[0].inline_size(), 16.0);
        let glyphs: Vec<_> = actual[0]
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect();
        assert_eq!(
            glyphs,
            vec![0; 301],
            "budget {budget}: no missing or duplicate source"
        );
    }
}

#[test]
fn out_of_flow_cross_node_cluster_no_duplicates() {
    use shodo::node::OutOfFlowKind;
    use shodo::style::InlineStyle;
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    for window in [0, 1, 2, 4, 4096] {
        for first_line in [false, true] {
            let limits = Limits {
                max_reshape_window_bytes: Some(window),
                ..Default::default()
            };
            let style = ParagraphStyle {
                first_line: first_line.then(|| InlineStyle {
                    font_size: 32.0,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.push_text(TextSource::Generated { node: NodeId(1) }, "f")
                .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
                .open_inline(NodeId(5), &style.root, Default::default())
                .close_inline()
                .push_text(TextSource::Generated { node: NodeId(3) }, "f")
                .push_out_of_flow(NodeId(4), OutOfFlowKind::Absolute)
                .push_text(TextSource::Generated { node: NodeId(6) }, "i");
            let p = b
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap();
            let actual = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(actual.len(), 1);
            assert_eq!(
                actual[0].text_range(),
                0..p.text().len(),
                "window={window}, first-line={first_line}: whole source retained"
            );
            let runs: Vec<_> = actual[0]
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .collect();
            assert_eq!(runs.iter().map(|r| r.glyphs().len()).sum::<usize>(), 1);
            assert_eq!(runs[0].node(), Some(NodeId(1)));
            assert_eq!(runs[0].clusters().next().unwrap().text_range, 0..9);
            if window < 9 {
                assert!(
                    p.warnings()
                        .iter()
                        .any(|w| w.kind == WarningKind::Unsupported)
                );
            }
        }
    }
}

#[test]
fn all_corpus_public_glyph_ids_have_font_data() {
    use skrifa::raw::TableProvider;
    let limits = Limits::default();
    let fonts = shodo_fixtures::load_fonts(&limits).unwrap();
    let mut cx = LayoutContext::new();
    for case in shodo_fixtures::cases() {
        let p = case.build(&mut cx, &fonts, &limits).unwrap();
        let lines = p.break_all(
            &mut cx,
            &Default::default(),
            case.width,
            &AtomicSizes::EMPTY,
        );
        assert!(!lines.is_empty(), "{}", case.id);
        let mut count = 0;
        for line in &lines {
            for run in line.fragments().filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            }) {
                let data = run
                    .font_data()
                    .expect("every corpus run retains its actual face");
                let face = skrifa::FontRef::from_index(data.data.as_ref(), data.index).unwrap();
                let glyph_count = u32::from(face.maxp().unwrap().num_glyphs());
                for g in run.glyphs() {
                    assert!(
                        g.id < glyph_count,
                        "{}: glyph {} outside actual face",
                        case.id,
                        g.id
                    );
                    assert!(
                        g.advance.is_finite()
                            && g.inline_position.is_finite()
                            && g.block_offset.is_finite()
                    );
                    count += 1;
                }
            }
        }
        assert!(count > 0, "{}", case.id);
    }
}

#[test]
fn shrink_zero_releases_first_line_and_document_font_layer() {
    use shodo::font::FontFaceDescriptor;
    use shodo::node::OutOfFlowKind;
    use shodo::style::{FontFamily, InlineStyle};
    use shodo::{LineConstraint, LineResult};
    let limits = Limits::default();
    let shared = shodo_fixtures::load_fonts(&limits).unwrap();
    let document = FontCollection::for_document(&shared.collection, &limits);
    document
        .register_face(
            shodo_fixtures::FONTS[0].bytes.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Temporary first-line face".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let layer = document.layer_handle();
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_families: vec![FontFamily::Named("Temporary first-line face".into())],
            font_size: 32.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi ")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "tail");
    let mut cx = LayoutContext::new();
    let p = b.build(&mut cx, &document).unwrap();
    assert!(matches!(
        p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY
        ),
        LineResult::FloatEncountered { .. }
    ));
    drop(document);
    drop(p);
    assert!(
        layer.is_alive(),
        "partial line retains the selected first-line set"
    );
    cx.shrink_to(0);
    assert!(
        !layer.is_alive(),
        "shrink releases alternate data and its font layer"
    );
}

#[test]
fn corpus_first_line_budget_and_width_variations_preserve_source_and_cache_results() {
    use shodo::style::InlineStyle;
    let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
    for case in shodo_fixtures::cases() {
        for (run_budget, window_budget, warning_cap) in [(0, 0, 0), (4, 4, 2), (65536, 4096, 2)] {
            let limits = Limits {
                max_shaping_run_bytes: Some(run_budget),
                max_reshape_window_bytes: Some(window_budget),
                max_warnings: Some(warning_cap),
                ..Default::default()
            };
            let root = InlineStyle {
                font_size: case.font_size,
                lang: case.lang.clone(),
                overflow_wrap: OverflowWrap::Anywhere,
                ..Default::default()
            };
            let style = ParagraphStyle {
                first_line: Some(InlineStyle {
                    font_size: case.font_size * 2.0,
                    ..root.clone()
                }),
                root,
                direction: case.direction,
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(&style, &limits);
            for (n, c) in case.text.chars().enumerate() {
                b.push_text(
                    TextSource::Generated {
                        node: NodeId(n as u64 + 1),
                    },
                    &c.to_string(),
                );
            }
            let p = b
                .build(&mut LayoutContext::new(), &fonts.collection)
                .unwrap();
            assert!(p.warnings().len() <= warning_cap as usize + 1);
            let mut warm = LayoutContext::new();
            for width in [0.0, case.width, 1.0, 10000.0, case.width] {
                let cold_lines = p.break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    width,
                    &AtomicSizes::EMPTY,
                );
                let warm_lines =
                    p.break_all(&mut warm, &Default::default(), width, &AtomicSizes::EMPTY);
                let snapshot = |lines: &[shodo::Line]| {
                    lines
                        .iter()
                        .map(|line| {
                            let glyphs: Vec<_> = line
                                .fragments()
                                .filter_map(|f| match f {
                                    Fragment::GlyphRun(r) => Some(r),
                                    _ => None,
                                })
                                .flat_map(|r| {
                                    r.glyphs().map(|g| {
                                        (
                                            g.id,
                                            g.advance.to_bits(),
                                            g.inline_position.to_bits(),
                                            g.block_offset.to_bits(),
                                        )
                                    })
                                })
                                .collect();
                            (line.text_range(), line.inline_size().to_bits(), glyphs)
                        })
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    snapshot(&cold_lines),
                    snapshot(&warm_lines),
                    "{}, run={run_budget}, window={window_budget}, width={width}",
                    case.id
                );
                let mut end = 0;
                for line in &warm_lines {
                    assert_eq!(line.text(), p.text());
                    assert_eq!(
                        line.text_range().start,
                        end,
                        "{}: no source lost or repeated",
                        case.id
                    );
                    end = line.text_range().end;
                    assert!(line.inline_size().is_finite());
                }
                assert_eq!(end, p.text().len(), "{}: full source consumed", case.id);
                assert!(warm.take_warnings().len() <= warning_cap as usize + 1);
            }
        }
    }
}
