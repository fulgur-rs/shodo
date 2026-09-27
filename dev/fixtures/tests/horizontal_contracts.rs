use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    raw::types::GlyphId,
};
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1.0 / 32.0, "{a} vs {b}");
}

fn fixed_style() -> ParagraphStyle {
    ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
            font_size: 20.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn glyphs(line: &shodo::Line) -> Vec<shodo::Glyph> {
    line.fragments()
        .flat_map(|f| match f {
            Fragment::GlyphRun(r) => r.glyphs().collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn actual_font_alignment_last_line_and_indent_cover_every_value() {
    use shodo::style::{LineOptions, TextAlign, TextAlignLast, TextIndent};
    use shodo::{LineConstraint, LineResult};
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = fixed_style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a b");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let font = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let gm = font.glyph_metrics(Size::new(20.0), LocationRef::default());
    let advance = |c| gm.advance_width(font.charmap().map(c).unwrap()).unwrap();
    let natural = advance('a') + advance(' ') + advance('b');
    let plain = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    let saved = glyphs(&plain[0]);
    for align in [
        TextAlign::Start,
        TextAlign::End,
        TextAlign::Left,
        TextAlign::Right,
        TextAlign::Center,
        TextAlign::Justify,
        TextAlign::JustifyAll,
        TextAlign::MatchParent,
    ] {
        for last in [
            TextAlignLast::Auto,
            TextAlignLast::Start,
            TextAlignLast::End,
            TextAlignLast::Left,
            TextAlignLast::Right,
            TextAlignLast::Center,
            TextAlignLast::Justify,
        ] {
            let options = LineOptions {
                text_align: align,
                text_align_last: last,
                text_indent: TextIndent {
                    length: 10.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let constraint = LineConstraint {
                inline_start_offset: 7.0,
                ..LineConstraint::new(100.0)
            };
            let LineResult::Line(line) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("line expected")
            };
            let end = match last {
                TextAlignLast::Auto => matches!(align, TextAlign::End | TextAlign::Right),
                TextAlignLast::End | TextAlignLast::Right => true,
                _ => false,
            };
            let centered = last == TextAlignLast::Center
                || last == TextAlignLast::Auto && align == TextAlign::Center;
            let justified = last == TextAlignLast::Justify
                || last == TextAlignLast::Auto && align == TextAlign::JustifyAll;
            let shift = if end {
                90.0 - natural
            } else if centered {
                (90.0 - natural) / 2.0
            } else {
                0.0
            };
            let output = glyphs(&line);
            close(output[0].inline_position, 17.0 + shift);
            let final_glyph = output.last().unwrap();
            close(
                final_glyph.inline_position + final_glyph.advance,
                if justified {
                    107.0
                } else {
                    17.0 + shift + natural
                },
            );
            assert_eq!(
                glyphs(&plain[0]),
                saved,
                "an accepted Line must remain immutable"
            );
        }
    }
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
        .push_forced_break(NodeId(2))
        .push_text(TextSource::Generated { node: NodeId(3) }, "b")
        .push_forced_break(NodeId(4))
        .push_text(TextSource::Generated { node: NodeId(5) }, "c");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    for hanging in [false, true] {
        for each_line in [false, true] {
            let options = LineOptions {
                text_indent: TextIndent {
                    length: 10.0,
                    hanging,
                    each_line,
                },
                ..Default::default()
            };
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &options,
                100.0,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(lines.len(), 3);
            for (i, line) in lines.iter().enumerate() {
                close(
                    glyphs(line)[0].inline_position,
                    if (i == 0 || each_line) != hanging {
                        10.0
                    } else {
                        0.0
                    },
                );
            }
        }
    }
}

#[test]
fn real_font_next_line_is_pure_and_height_trials_do_not_consume_source() {
    use shodo::{LineConstraint, LineResult};
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = fixed_style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "aa bb cc dd");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut other = ParagraphBuilder::new(&style, &limits);
    other.push_text(TextSource::Generated { node: NodeId(1) }, "other");
    let other = other
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut context = LayoutContext::new();
    assert!(matches!(
        other.next_line(
            &mut context,
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(50.0),
            &AtomicSizes::EMPTY
        ),
        LineResult::InvalidToken
    ));
    for width in [0.0, 50.0, 1000.0] {
        let expected = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        let mut token = p.start_token();
        for line in &expected {
            let constraint = LineConstraint::new(width);
            let rejected = LineConstraint {
                max_block_size: Some(0.0),
                ..constraint
            };
            assert!(matches!(
                p.next_line(
                    &mut context,
                    token,
                    &Default::default(),
                    &rejected,
                    &AtomicSizes::EMPTY
                ),
                LineResult::BlockSizeExceeded { .. }
            ));
            for _ in 0..2 {
                let LineResult::Line(actual) = p.next_line(
                    &mut context,
                    token,
                    &Default::default(),
                    &constraint,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!("line expected")
                };
                assert_eq!(actual.text_range(), line.text_range());
                assert_eq!(actual.break_token(), line.break_token());
                assert_eq!(glyphs(&actual), glyphs(line));
                close(actual.block_size(), line.block_size());
            }
            assert_ne!(token, line.break_token());
            token = line.break_token();
        }
        assert!(matches!(
            p.next_line(
                &mut context,
                token,
                &Default::default(),
                &LineConstraint::new(width),
                &AtomicSizes::EMPTY
            ),
            LineResult::Done
        ));
    }
}

#[test]
fn actual_font_float_page_trials_restore_reports_and_geometry() {
    use shodo::node::OutOfFlowKind;
    use shodo::{LineConstraint, LineResult};
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = fixed_style();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "b")
        .push_out_of_flow(NodeId(4), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(5) }, "c");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut context = LayoutContext::new();
    let mut reports = Vec::new();
    let mut saved = None;
    for trial in 0..2 {
        let mut constraint = LineConstraint::new(1.0); // first unbreakable word overflows
        let mut cursor = None;
        for node in [NodeId(2), NodeId(4)] {
            constraint.floats_placed_through = cursor;
            let LineResult::FloatEncountered {
                node: actual,
                line_start,
                inline_position,
                float_cursor,
            } = p.next_line(
                &mut context,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            )
            else {
                panic!("overflowing first word must still report its floats")
            };
            assert_eq!(actual, node);
            assert_eq!(line_start, p.start_token());
            reports.push((actual, inline_position, float_cursor));
            cursor = Some(float_cursor);
        }
        constraint.floats_placed_through = cursor;
        if trial == 0 {
            constraint.max_block_size = Some(0.0);
            assert!(matches!(
                p.next_line(
                    &mut context,
                    p.start_token(),
                    &Default::default(),
                    &constraint,
                    &AtomicSizes::EMPTY
                ),
                LineResult::BlockSizeExceeded { .. }
            ));
            constraint.max_block_size = None;
        }
        let LineResult::Line(line) = p.next_line(
            &mut context,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line after accepted floats")
        };
        assert_eq!(line.text_range(), 0..9);
        let output = glyphs(&line);
        for ((_, position, _), glyph) in reports[trial * 2..].iter().zip(output.iter().skip(1)) {
            close(*position, glyph.inline_position);
        }
        if let Some(previous) = &saved {
            assert_eq!(&output, previous);
        }
        saved = Some(output);
    }
    assert_eq!(reports[..2], reports[2..]);
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "aa b")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(3) }, "bbb");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let LineResult::Line(first) = p.next_line(
        &mut context,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(30.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("float in next unbreakable word must not be reported early")
    };
    assert_eq!(first.text_range(), 0..3);
    assert!(matches!(
        p.next_line(
            &mut context,
            first.break_token(),
            &Default::default(),
            &LineConstraint::new(30.0),
            &AtomicSizes::EMPTY
        ),
        LineResult::FloatEncountered {
            node: NodeId(2),
            ..
        }
    ));
}

#[test]
fn nonfinite_and_extreme_styles_keep_geometry_finite_and_source_progressing() {
    use shodo::style::TabSize;
    use shodo::{LineConstraint, LineResult};
    let limits = shodo::limits::Limits {
        max_reshape_window_bytes: Some(0),
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    for value in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -1.0e30,
        0.0,
        1.0e30,
    ] {
        let mut style = fixed_style();
        style.root.font_size = value;
        style.root.letter_spacing = value;
        style.root.word_spacing = value;
        style.root.tab_size = TabSize::Px(value);
        style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a\t b");
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let mut token = p.start_token();
        let mut context = LayoutContext::new();
        let mut accepted = Vec::new();
        for _ in 0..12 {
            let constraint = LineConstraint {
                inline_start_offset: value,
                block_offset: value,
                ..LineConstraint::new(value)
            };
            match p.next_line(
                &mut context,
                token,
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) {
                LineResult::Line(line) => {
                    assert_ne!(token, line.break_token());
                    for number in [
                        line.inline_size(),
                        line.block_size(),
                        line.block_offset(),
                        line.hang_end(),
                        line.metrics().baseline,
                    ] {
                        assert!(number.is_finite());
                    }
                    for g in glyphs(&line) {
                        for number in [g.inline_position, g.block_offset, g.advance] {
                            assert!(number.is_finite());
                        }
                    }
                    token = line.break_token();
                    accepted.push(line);
                }
                LineResult::Done => break,
                other => panic!("{other:?}"),
            }
        }
        assert!(matches!(
            p.next_line(
                &mut context,
                token,
                &Default::default(),
                &LineConstraint::new(0.0),
                &AtomicSizes::EMPTY
            ),
            LineResult::Done
        ));
        let index = shodo::hit::LineLayout::new(&accepted);
        assert!(index.hit_test(f32::NAN, 0.0).is_none());
        for coord in [f32::NEG_INFINITY, 0.0, f32::INFINITY] {
            let hit = index.hit_test(coord, coord).unwrap();
            let caret = index.caret(hit.position).unwrap();
            assert!(caret.rect.inline_start.is_finite());
            assert!(caret.rect.block_start.is_finite());
            assert!(caret.rect.block_size.is_finite());
            assert!(caret.rect.block_size >= 0.0);
        }
    }
}

#[test]
fn real_font_empty_block_prefix_and_missing_atomic_keep_output_contracts() {
    use shodo::node::InlineEdges;
    use shodo::{LineConstraint, LineResult};
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut style = fixed_style();
    style.first_line = Some(InlineStyle {
        font_size: 40.0,
        ..style.root.clone()
    });
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.open_inline(NodeId(1), &style.root, InlineEdges::default())
        .push_block_in_inline(NodeId(2))
        .close_inline()
        .push_atomic(NodeId(3), &style.root, InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(4) }, "a");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut context = LayoutContext::new();
    let constraint = LineConstraint::new(100.0);
    let LineResult::Line(empty) = p.next_line(
        &mut context,
        p.start_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("empty prefix")
    };
    assert!(empty.is_empty());
    close(empty.block_size(), 0.0);
    let LineResult::BlockInInline {
        node: NodeId(2),
        token_after: token,
    } = p.next_line(
        &mut context,
        empty.break_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    )
    else {
        panic!("block boundary")
    };
    let LineResult::Line(line) = p.next_line(
        &mut context,
        token,
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("normal line after block")
    };
    for fragment in line.fragments() {
        match fragment {
            Fragment::GlyphRun(run) => close(run.font_size(), 20.0),
            Fragment::Atomic(atomic) => {
                close(atomic.border_rect.inline_size, 0.0);
                close(atomic.border_rect.block_size, 0.0);
            }
            _ => {}
        }
    }
    assert!(
        context
            .take_warnings()
            .iter()
            .any(|w| w.kind == shodo::limits::WarningKind::MissingAtomicSize)
    );
    assert!(!line.is_empty());
}
#[test]
fn final_line_metrics_and_ink_overflow_are_explicit() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let root = InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[0].family.into())],
        font_size: 20.0,
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root,
            ..Default::default()
        },
        &limits,
    );
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a ");
    let lines = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
    let line = &lines[0];
    let run = line
        .fragments()
        .find_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .unwrap();
    let metrics = line.metrics();
    close(
        metrics.baseline,
        line.baseline(shodo::geometry::BaselineKind::Alphabetic),
    );
    close(metrics.ascent + metrics.descent, line.block_size());
    close(metrics.text_over, metrics.baseline - run.metrics().ascent);
    close(metrics.text_under, metrics.baseline + run.metrics().descent);
    assert_eq!(run.style_index(), 0);
    let glyph = run.glyphs().next().unwrap();
    let font = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let bounds = font
        .glyph_metrics(Size::new(20.0), LocationRef::default())
        .bounds(GlyphId::new(glyph.id))
        .unwrap();
    let ink = line.overflow_rect();
    close(ink.inline_start, glyph.inline_position + bounds.x_min);
    close(ink.inline_size, bounds.x_max - bounds.x_min);
    close(
        ink.block_start,
        run.baseline() + glyph.block_offset - bounds.y_max,
    );
    close(ink.block_size, bounds.y_max - bounds.y_min);
    close(line.hang_start(), 0.0);
    close(line.hang_end(), run.glyphs().last().unwrap().advance);
}

#[test]
fn signed_atomic_margins_do_not_make_negative_caret_height() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle::default();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_atomic(NodeId(1), &style.root, Default::default());
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(1),
        shodo::AtomicSize {
            inline_size: 20.0,
            block_size: 20.0,
            baseline: Some(15.0),
            margins: shodo::node::Sides {
                block_start: -30.0,
                block_end: -30.0,
                ..Default::default()
            },
        },
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &sizes,
    );
    let index = shodo::hit::LineLayout::new(&lines);
    let caret = index
        .caret(shodo::hit::TextPosition {
            line: 0,
            offset: 0,
            affinity: shodo::mapping::Affinity::Downstream,
        })
        .unwrap();
    assert!(
        caret.rect.block_size >= 0.0,
        "negative atomic caret height {}",
        caret.rect.block_size
    );
    let atom = lines[0]
        .fragments()
        .find_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    close(caret.rect.block_start, atom.border_rect.block_start);
    close(caret.rect.block_size, 20.0);
}

#[test]
fn cluster_flags_preserve_source_and_discretionary_hyphens() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named(FONTS[0].family.into())],
            font_size: 20.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(
        TextSource::Generated { node: NodeId(1) },
        "a ,#\u{ff03} \u{301}ab\u{ad}cd",
    );
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        1000.0,
        &AtomicSizes::EMPTY,
    );
    let clusters: Vec<_> = lines[0]
        .fragments()
        .flat_map(|f| match f {
            Fragment::GlyphRun(r) => r.clusters().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    for cluster in &clusters {
        let flags = cluster.flags;
        match cluster.source_char.unwrap() {
            'a' | 'b' | 'c' | 'd' => assert!(!flags.emphasis_excluded),
            ',' => assert!(flags.punctuation && flags.emphasis_excluded),
            '#' | '\u{ff03}' => assert!(flags.punctuation && !flags.emphasis_excluded),
            ' ' => {
                assert!(flags.whitespace);
                assert_eq!(
                    flags.emphasis_excluded,
                    !p.text()[cluster.text_range.clone()].contains('\u{301}')
                );
            }
            _ => {}
        }
        assert!(!flags.synthetic_hyphen);
    }
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}cd");
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        35.0,
        &AtomicSizes::EMPTY,
    );
    assert!(lines.iter().flat_map(|l| l.fragments()).any(|f| {
        matches!(f, Fragment::GlyphRun(r) if r.clusters().any(|c| c.flags.synthetic_hyphen && c.source_char == Some('\u{ad}')))
    }));
}
