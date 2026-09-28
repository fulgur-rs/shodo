//! Mutations caught: losing source paint at shaping, splitting GSUB for paint,
//! selecting the wrong shared owner, dropping first-line paint or leaking NaN.
use shodo::geometry::Direction;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, PaintStyle, ParagraphStyle, TextDecoration};
use shodo::{
    AtomicSizes, Fragment, GlyphRunView, LayoutContext, Line, Paragraph, ParagraphBuilder, RichText,
};
use shodo_fixtures::{FONTS, load_fonts};

const RED: [u8; 4] = [200, 10, 20, 128];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 160, 0, 255];
fn style(font: usize, color: [u8; 4]) -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[font].family.into())],
        paint: PaintStyle {
            color,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn lines(p: &Paragraph, width: f32) -> Vec<Line> {
    p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        width,
        &AtomicSizes::EMPTY,
    )
}
fn runs(line: &Line) -> Vec<GlyphRunView<'_>> {
    line.fragments()
        .filter_map(|f| {
            if let Fragment::GlyphRun(r) = f {
                Some(r)
            } else {
                None
            }
        })
        .collect()
}
fn glyphs(lines: &[Line]) -> Vec<(u32, u32, f32, f32)> {
    lines
        .iter()
        .flat_map(runs)
        .flat_map(|r| r.glyphs())
        .map(|g| (g.id, g.cluster, g.inline_position, g.advance))
        .collect()
}

#[test]
fn default_and_explicit_paint_survive_both_builders() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style(0, RED);
    s.paint.underline = Some(TextDecoration {
        color: Some(BLUE),
        offset: Some(3.0),
        thickness: Some(2.0),
    });
    s.paint.strikethrough = Some(TextDecoration {
        color: None,
        offset: Some(-4.0),
        thickness: Some(1.0),
    });
    let root = ParagraphStyle::default();
    let p = RichText::new(&root)
        .push("a", &s)
        .push("b", &InlineStyle::default())
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    let rs = runs(&ls[0]);
    assert_eq!(rs[0].paint_style().color, RED);
    assert_eq!(
        rs[0].paint_style().underline,
        Some(TextDecoration {
            color: Some(BLUE),
            offset: Some(3.0),
            thickness: Some(2.0)
        })
    );
    assert_eq!(
        rs[0].paint_style().strikethrough,
        Some(TextDecoration {
            color: None,
            offset: Some(-4.0),
            thickness: Some(1.0)
        })
    );
    assert_eq!(rs.last().unwrap().paint_style().color, [0, 0, 0, 255]);
    assert_eq!(rs.last().unwrap().paint_style().underline, None);
    assert_eq!(rs.last().unwrap().paint_style().strikethrough, None);
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.open_inline(NodeId(7), &s, Default::default())
        .push_text(TextSource::Generated { node: NodeId(99) }, "a")
        .close_inline();
    let dom = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let domlines = lines(&dom, 1000.0);
    assert_eq!(runs(&domlines[0])[0].node(), Some(NodeId(99)));
    assert_eq!(runs(&domlines[0])[0].paint_style(), rs[0].paint_style());
}

#[test]
fn paint_boundaries_preserve_ffi_owner_and_arabic_joining() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (font, parts, dir) in [
        (0, vec!["f", "f", "i"], Direction::Ltr),
        (2, vec!["س", "ل", "ام"], Direction::Rtl),
    ] {
        let mut a = style(font, RED);
        a.direction = dir;
        a.paint.underline = Some(TextDecoration::default());
        let mut b = a.clone();
        b.paint.color = BLUE;
        b.paint.underline = None;
        b.paint.strikethrough = Some(TextDecoration {
            color: Some(GREEN),
            ..Default::default()
        });
        let root = ParagraphStyle {
            direction: dir,
            root: a.clone(),
            ..Default::default()
        };
        let mut rt = RichText::new(&root);
        for (i, t) in parts.iter().enumerate() {
            rt = rt.push(t, if i == 0 { &a } else { &b });
        }
        let split = rt
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let one = RichText::new(&root)
            .push(&parts.concat(), &a)
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let split = lines(&split, 1000.0);
        let one = lines(&one, 1000.0);
        assert_eq!(
            glyphs(&split),
            glyphs(&one),
            "paint must not split shaping: {parts:?}"
        );
        if font == 0 {
            let rs = runs(&split[0]);
            assert_eq!(rs.len(), 1);
            assert_eq!(rs[0].glyphs().len(), 1);
            assert_eq!(rs[0].text_range(), 0..3);
            assert_eq!(rs[0].node(), Some(NodeId(0)));
            assert_eq!(rs[0].paint_style().color, RED);
            assert!(rs[0].paint_style().underline.is_some());
        }
    }
}

#[test]
fn first_line_retains_resolved_and_legacy_paint() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let normal = style(0, RED);
    let mut first = normal.clone();
    first.paint.color = GREEN;
    first.paint.underline = Some(TextDecoration::default());
    let root = ParagraphStyle {
        root: normal.clone(),
        first_line: Some(first),
        ..Default::default()
    };
    for resolved in [false, true] {
        let mut blue = normal.clone();
        blue.paint.color = BLUE;
        blue.paint.strikethrough = Some(TextDecoration::default());
        let rt = if resolved {
            RichText::new(&root).push_with_first_line("one two three four", &normal, &blue)
        } else {
            RichText::new(&root).push("one two three four", &normal)
        };
        let p = rt
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let ls = lines(&p, 60.0);
        assert!(ls.len() >= 2);
        for r in runs(&ls[0]) {
            assert_eq!(r.paint_style().color, if resolved { BLUE } else { GREEN });
            assert_eq!(r.paint_style().underline.is_some(), !resolved);
            assert_eq!(r.paint_style().strikethrough.is_some(), resolved);
        }
        for r in ls.iter().skip(1).flat_map(runs) {
            assert_eq!(r.paint_style(), &normal.paint);
        }
    }
}

#[test]
fn invalid_decoration_lengths_warn_and_normalize() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut s = style(0, RED);
    s.paint.underline = Some(TextDecoration {
        color: None,
        offset: Some(f32::NAN),
        thickness: Some(f32::INFINITY),
    });
    s.paint.strikethrough = Some(TextDecoration {
        color: None,
        offset: Some(2e7),
        thickness: Some(-1.0),
    });
    let p = RichText::new(&ParagraphStyle::default())
        .push("a", &s)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NonFiniteInput)
    );
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NegativeInput)
    );
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::Saturated)
    );
    let ls = lines(&p, 1000.0);
    let rs = runs(&ls[0]);
    let paint = rs[0].paint_style();
    assert_eq!(paint.underline, Some(TextDecoration::default()));
    assert_eq!(
        paint.strikethrough,
        Some(TextDecoration {
            color: None,
            offset: Some(1e7),
            thickness: Some(0.0)
        })
    );
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1.0 / 32.0, "{a} vs {b}");
}
#[test]
fn source_spans_partition_ffi_and_resolve_real_metrics() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut a = style(0, RED);
    a.paint.underline = Some(TextDecoration::default());
    let mut b = a.clone();
    b.paint.color = BLUE;
    b.paint.underline = Some(TextDecoration {
        color: Some(GREEN),
        offset: Some(3.0),
        thickness: Some(2.0),
    });
    b.paint.strikethrough = Some(TextDecoration {
        color: None,
        offset: Some(-4.0),
        thickness: Some(1.0),
    });
    let p = RichText::new(&ParagraphStyle {
        root: a.clone(),
        ..Default::default()
    })
    .push("f", &a)
    .push("f", &b)
    .push("i", &a)
    .build(&mut LayoutContext::new(), &fonts.collection)
    .unwrap();
    let ls = lines(&p, 1000.0);
    let r = runs(&ls[0])[0];
    let spans = ls[0].paint_spans();
    assert_eq!(spans.len(), 3);
    assert_eq!(
        spans
            .iter()
            .map(|s| (s.node, s.text_range.clone()))
            .collect::<Vec<_>>(),
        vec![
            (Some(NodeId(0)), 0..1),
            (Some(NodeId(1)), 1..2),
            (Some(NodeId(2)), 2..3)
        ]
    );
    close(spans[0].rect.inline_start, 0.0);
    close(spans[0].rect.inline_size, 315.0 * 16.0 / 1000.0);
    close(spans[1].rect.inline_start, 315.0 * 16.0 / 1000.0);
    close(spans[1].rect.inline_size, (631.0 - 315.0) * 16.0 / 1000.0);
    close(spans[2].rect.inline_start, 631.0 * 16.0 / 1000.0);
    close(
        spans.iter().map(|s| s.rect.inline_size).sum(),
        r.inline_size(),
    );
    let u = spans[0].underline().unwrap();
    assert_eq!(u.color, RED);
    close(u.rect.block_start, r.baseline() + 1.6 - 0.4);
    close(u.rect.block_size, 0.8);
    let u = spans[1].underline().unwrap();
    assert_eq!(u.color, GREEN);
    close(u.rect.block_start, r.baseline() + 2.0);
    close(u.rect.block_size, 2.0);
    close(u.rect.inline_start, 5.04);
    close(u.rect.inline_size, 5.056);
    let s = spans[1].strikethrough().unwrap();
    assert_eq!(s.color, BLUE);
    close(s.rect.block_start, r.baseline() - 4.5);
    close(s.rect.block_size, 1.0);
    assert_eq!(
        r.glyphs().len(),
        1,
        "decoration output must not duplicate shared glyph"
    );
}

#[test]
fn multiline_bidi_justification_and_whitespace_spans() {
    use shodo::hit::{LineLayout, TextPosition};
    use shodo::mapping::Affinity;
    use shodo::style::{LineOptions, TextAlign, WhiteSpaceCollapse};
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut a = style(0, RED);
    a.paint.underline = Some(TextDecoration::default());
    a.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let root = ParagraphStyle {
        root: a.clone(),
        ..Default::default()
    };
    let mut b = style(2, BLUE);
    b.direction = Direction::Rtl;
    b.white_space_collapse = WhiteSpaceCollapse::Preserve;
    b.paint.strikethrough = Some(TextDecoration::default());
    let p = RichText::new(&root)
        .push("one\t two ", &a)
        .push("سلام سلام ", &b)
        .push("three four", &a)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions {
            text_align: TextAlign::Justify,
            ..Default::default()
        },
        85.0,
        &AtomicSizes::EMPTY,
    );
    assert!(ls.len() > 1);
    let index = LineLayout::new(&ls);
    let mut covered = String::new();
    for (n, line) in ls.iter().enumerate() {
        for s in line.paint_spans() {
            covered.push_str(&line.text()[s.text_range.clone()]);
            assert!(s.rect.inline_size >= 0.0 && s.rect.block_start.is_finite());
            let rect = index.selection_rects(
                TextPosition {
                    line: n,
                    offset: s.text_range.start as u32,
                    affinity: Affinity::Downstream,
                },
                TextPosition {
                    line: n,
                    offset: s.text_range.end as u32,
                    affinity: Affinity::Upstream,
                },
            );
            if s.rect.inline_size > 0.0 {
                assert_eq!(rect, vec![s.rect]);
            }
        }
    }
    assert_eq!(covered, p.text());
    assert!(
        ls.iter()
            .flat_map(|l| l.paint_spans())
            .any(|s| &p.text()[s.text_range.clone()] == "\t")
    );
    let p = RichText::new(&ParagraphStyle::default())
        .push("  a   b  ", &style(0, RED))
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    let spans = ls[0].paint_spans();
    let rendered = spans
        .iter()
        .map(|s| &ls[0].text()[s.text_range.clone()])
        .collect::<String>();
    // Collapsing removed the original repeated bytes; the final trailing
    // space retains selection advance even though CSS decoration may skip it.
    assert_eq!(rendered, "a b ");
    assert!(spans.last().unwrap().rect.inline_size > 0.0);
}

#[test]
fn fallback_adjusted_metrics_and_first_line_spans() {
    use shodo::style::{FontMetricKind, FontSizeAdjust};
    use skrifa::{
        FontRef, MetadataProvider,
        instance::{LocationRef, Size},
    };
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut a = style(0, RED);
    a.paint.underline = Some(TextDecoration::default());
    a.font_size_adjust = Some(FontSizeAdjust {
        metric: FontMetricKind::ExHeight,
        value: 0.6,
    });
    a.font_families
        .push(FontFamily::Named(FONTS[1].family.into()));
    let mut first = a.clone();
    first.paint.color = GREEN;
    let root = ParagraphStyle {
        root: a.clone(),
        first_line: Some(first),
        ..Default::default()
    };
    let p = RichText::new(&root)
        .push("a水 a水 a水", &a)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 40.0);
    assert!(ls.len() > 1);
    let mut seen = std::collections::BTreeSet::new();
    for (n, line) in ls.iter().enumerate() {
        for s in line.paint_spans() {
            let r = runs(line)
                .into_iter()
                .find(|r| {
                    r.text_range().start <= s.text_range.start
                        && r.text_range().end >= s.text_range.end
                })
                .unwrap();
            seen.insert(s.font);
            assert_eq!(s.font, r.font());
            close(s.font_size, r.font_size());
            assert_eq!(s.style.color, if n == 0 { GREEN } else { RED });
            let fd = r.font_data().unwrap();
            let font = FontRef::from_index(fd.data.as_ref(), fd.index).unwrap();
            let m = font.metrics(
                Size::new(s.font_size),
                LocationRef::new(r.normalized_coords()),
            );
            let u = m.underline.unwrap();
            let d = s.underline().unwrap();
            close(d.rect.block_size, u.thickness);
            close(
                d.rect.block_start,
                line.block_offset() + r.baseline() - u.offset - u.thickness / 2.0,
            );
        }
    }
    assert_eq!(seen.len(), 2);
    assert!(
        ls.iter()
            .flat_map(|l| l.paint_spans())
            .any(|s| s.font_size > 16.0)
    );
}

#[test]
fn combining_sources_and_shy_keep_paint() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut a = style(0, RED);
    a.paint.underline = Some(TextDecoration::default());
    let mut b = a.clone();
    b.paint.color = BLUE;
    b.paint.strikethrough = Some(TextDecoration::default());
    let p = RichText::new(&ParagraphStyle::default())
        .push("a", &a)
        .push("\u{301}", &b)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    let spans = ls[0].paint_spans();
    assert_eq!(
        spans
            .iter()
            .map(|s| s.text_range.clone())
            .collect::<Vec<_>>(),
        vec![0..1, 1..3]
    );
    assert_eq!(spans[0].rect, spans[1].rect);
    assert_eq!(spans[1].style.color, BLUE);
    let index = shodo::hit::LineLayout::new(&ls);
    let caret = index
        .caret(shodo::hit::TextPosition {
            line: 0,
            offset: 1,
            affinity: shodo::mapping::Affinity::Downstream,
        })
        .unwrap();
    assert_eq!(
        caret.position.offset, 3,
        "paint must not invent an interior grapheme caret"
    );
    let p = RichText::new(&ParagraphStyle::default())
        .push("ab\u{ad}cd", &b)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 27.0);
    assert!(ls.len() > 1);
    assert!(
        runs(&ls[0])
            .iter()
            .flat_map(|r| r.clusters())
            .any(|c| c.flags.synthetic_hyphen)
    );
    let s = ls[0]
        .paint_spans()
        .into_iter()
        .find(|s| s.text_range == (2..4))
        .unwrap();
    assert_eq!(s.style.color, BLUE);
    assert!(s.strikethrough().is_some());
    // Source atomics are layout boxes, not resolved text-decoration ranges.
    let root = ParagraphStyle {
        root: a,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&root, &Limits::default());
    builder
        .push_text(TextSource::Generated { node: NodeId(1) }, "a")
        .push_atomic(NodeId(2), &root.root, Default::default())
        .push_text(TextSource::Generated { node: NodeId(3) }, "b");
    let p = builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    assert!(
        ls[0]
            .paint_spans()
            .iter()
            .all(|s| s.node != Some(NodeId(2)))
    );
}

#[test]
fn vertical_sideways_and_combined_decorations() {
    use shodo::geometry::WritingMode;
    use shodo::style::{TextCombineUpright, TextOrientation};
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (mode, text, combined, want_sign) in [
        (WritingMode::VerticalRl, "水", false, 1.0),
        (WritingMode::VerticalLr, "水", false, -1.0),
        (WritingMode::SidewaysRl, "a", false, 1.0),
        (WritingMode::SidewaysLr, "a", false, 1.0),
        (WritingMode::VerticalRl, "12", true, 1.0),
        (WritingMode::VerticalLr, "12", true, 1.0),
    ] {
        let mut s = style(if text == "水" { 1 } else { 0 }, RED);
        s.text_orientation = TextOrientation::Upright;
        s.paint.underline = Some(TextDecoration {
            color: Some(GREEN),
            offset: Some(3.0),
            thickness: Some(2.0),
        });
        if combined {
            s.text_combine_upright = TextCombineUpright::All;
        }
        let root = ParagraphStyle {
            writing_mode: mode,
            root: s.clone(),
            ..Default::default()
        };
        let p = RichText::new(&root)
            .push(text, &s)
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let ls = lines(&p, 1000.0);
        let rs = runs(&ls[0]);
        let spans = ls[0].paint_spans();
        assert!(!spans.is_empty());
        for span in &spans {
            let d = span.underline().unwrap();
            assert_eq!(d.color, GREEN);
            if !combined {
                close(d.rect.block_start, rs[0].baseline() + want_sign * 3.0 - 1.0);
                close(d.rect.block_size, 2.0);
                close(d.rect.inline_size, span.rect.inline_size);
            } else {
                close(d.rect.inline_size, 2.0);
                close(d.rect.block_start, span.rect.block_start);
                close(d.rect.block_size, span.rect.block_size);
                close(d.rect.inline_start, rs[0].glyph_origin(0).unwrap().0 + 2.0);
            }
        }
    }
}

#[test]
fn legacy_first_line_inherits_paint_components_independently() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let normal = style(0, RED);
    let mut first = normal.clone();
    first.paint.color = GREEN;
    first.paint.underline = Some(TextDecoration::default());
    let mut child = normal.clone();
    child.paint.color = BLUE;
    let root = ParagraphStyle {
        root: normal,
        first_line: Some(first),
        ..Default::default()
    };
    let p = RichText::new(&root)
        .push("a", &child)
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let ls = lines(&p, 1000.0);
    let r = runs(&ls[0])[0];
    assert_eq!(
        r.paint_style().color,
        BLUE,
        "child color does not prevent unrelated first-line underline inheritance"
    );
    assert!(r.paint_style().underline.is_some());
    assert!(ls[0].paint_spans()[0].underline().is_some());
}
