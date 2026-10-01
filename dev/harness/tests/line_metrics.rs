use shodo::geometry::BaselineKind;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, FontMetricKind, FontSizeAdjust, InlineStyle, LineHeight, ParagraphStyle,
    VerticalAlign,
};
use shodo::{AtomicSize, AtomicSizes, Fragment, LayoutContext, Line, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, MetadataProvider};

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0 / 32.0,
        "actual {actual}, expected {expected}"
    );
}

fn layout(style: &ParagraphStyle, add: impl FnOnce(&mut ParagraphBuilder)) -> Line {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut builder = ParagraphBuilder::new(style, &Limits::default());
    add(&mut builder);
    builder
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            10000.0,
            &AtomicSizes::EMPTY,
        )
        .remove(0)
}

fn style(family: usize, size: f32) -> InlineStyle {
    InlineStyle {
        font_families: vec![FontFamily::Named(FONTS[family].family.into())],
        font_size: size,
        ..Default::default()
    }
}

#[test]
fn normal_line_height_uses_selected_font() {
    for (family, text) in [(0, "a"), (1, "水"), (2, "سلام")] {
        let font = FontRef::from_index(FONTS[family].bytes, FONTS[family].face_index).unwrap();
        let expected = font.metrics(Size::new(20.0), LocationRef::default());
        let line = layout(
            &ParagraphStyle {
                root: style(family, 20.0),
                ..Default::default()
            },
            |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            },
        );
        close(
            line.block_size(),
            expected.ascent - expected.descent + expected.leading,
        );
        close(
            line.baseline(BaselineKind::Alphabetic),
            expected.ascent + expected.leading / 2.0,
        );
    }
}

#[test]
fn segment_break_ignorables_preserve_fallback_line_metrics() {
    // Reproduce WPT css/css-text/line-breaking/segment-break-transformation-ignorable-1.
    let segmented = "\n  水\u{fe00}\n  日\u{fe00}\n  本\u{e0100}\n  語\u{e0100}\n  水\u{00ad}\n  日\u{200e}\n  本\n  \u{200e}語\n";
    let reference = "\n  水\u{fe00}日\u{fe00}本\u{e0100}語\u{e0100}水日本語\n";
    let mut root = self::style(0, 24.0);
    root.lang = Some("zh".into());
    let style = ParagraphStyle {
        root,
        ..Default::default()
    };
    let fonts = load_fonts(&Limits::default()).unwrap();
    let layout = |text, node| {
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_text(TextSource::Generated { node: NodeId(node) }, text);
        builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap()
            .break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                10000.0,
                &AtomicSizes::EMPTY,
            )
            .remove(0)
    };
    let segmented = layout(segmented, 1);
    let reference = layout(reference, 2);
    assert!(
        segmented
            .fragments()
            .filter_map(|fragment| match fragment {
                Fragment::GlyphRun(run) if run.font() == fonts.ids[1] => {
                    Some(
                        run.glyphs()
                            .any(|glyph| glyph.id != 0 && glyph.advance > 0.0),
                    )
                }
                _ => None,
            })
            .any(|has_visible_glyph| has_visible_glyph),
        "CJK fallback must shape visible glyphs"
    );
    assert_eq!(segmented.inline_size(), reference.inline_size());
    assert_eq!(
        segmented.block_size(),
        reference.block_size(),
        "segmented={:?}, reference={:?}",
        segmented.metrics(),
        reference.metrics()
    );
    assert_eq!(
        segmented.baseline(BaselineKind::Alphabetic),
        reference.baseline(BaselineKind::Alphabetic)
    );
}

#[test]
fn mixed_face_and_adjusted_run_metrics_drive_line_box() {
    let mut child = style(0, 40.0);
    child.font_size_adjust = Some(FontSizeAdjust {
        metric: FontMetricKind::ExHeight,
        value: 1.0,
    });
    let line = layout(
        &ParagraphStyle {
            root: style(2, 16.0),
            ..Default::default()
        },
        |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "سلام")
                .open_inline(NodeId(2), &child, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(3) }, "a")
                .close_inline();
        },
    );
    let font = FontRef::from_index(FONTS[0].bytes, FONTS[0].face_index).unwrap();
    let unscaled = font.metrics(Size::unscaled(), LocationRef::default());
    let adjusted_size = 40.0 * f32::from(unscaled.units_per_em) / unscaled.x_height.unwrap();
    let expected = font.metrics(Size::new(adjusted_size), LocationRef::default());
    let run = line
        .fragments()
        .find_map(|f| match f {
            Fragment::GlyphRun(r) if r.node() == Some(NodeId(3)) => Some(r),
            _ => None,
        })
        .unwrap();
    close(run.font_size(), adjusted_size);
    close(
        line.block_size(),
        expected.ascent - expected.descent + expected.leading,
    );
    close(
        line.baseline(BaselineKind::Alphabetic),
        expected.ascent + expected.leading / 2.0,
    );
}

#[test]
fn parent_x_height_drives_middle_and_text_edges() {
    let mut root = style(1, 20.0);
    root.line_height = LineHeight::Px(100.0);
    let parent_font = FontRef::from_index(FONTS[1].bytes, FONTS[1].face_index).unwrap();
    let parent = parent_font.metrics(Size::new(20.0), LocationRef::default());
    let child_font = FontRef::from_index(FONTS[0].bytes, FONTS[0].face_index).unwrap();
    let child_metrics = child_font.metrics(Size::new(20.0), LocationRef::default());
    for (align, expected_shift) in [
        (
            VerticalAlign::Middle,
            (child_metrics.ascent + child_metrics.descent) / 2.0 - parent.x_height.unwrap() / 2.0,
        ),
        (VerticalAlign::TextTop, child_metrics.ascent - parent.ascent),
        (
            VerticalAlign::TextBottom,
            child_metrics.descent - parent.descent,
        ),
    ] {
        let mut child = style(0, 20.0);
        child.vertical_align = align;
        let line = layout(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            |b| {
                b.open_inline(NodeId(2), &child, InlineEdges::default())
                    .push_text(TextSource::Generated { node: NodeId(3) }, "a")
                    .close_inline();
            },
        );
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        close(
            run.baseline() - line.baseline(BaselineKind::Alphabetic),
            expected_shift,
        );
    }
}

#[test]
fn all_vertical_align_values_match_font_and_atomic_oracles() {
    let parent_font = FontRef::from_index(FONTS[2].bytes, 0).unwrap();
    let parent = parent_font.metrics(Size::new(24.0), LocationRef::default());
    let os2 = parent_font.os2().unwrap();
    let scale = 24.0 / f32::from(parent.units_per_em);
    let child_font = FontRef::from_index(FONTS[0].bytes, 0).unwrap();
    let child_metrics = child_font.metrics(Size::new(16.0), LocationRef::default());
    let lead = (32.0 - child_metrics.ascent + child_metrics.descent) / 2.0;
    let a = child_metrics.ascent + lead;
    let d = -child_metrics.descent + lead;
    for align in [
        VerticalAlign::Baseline,
        VerticalAlign::Length(7.0),
        VerticalAlign::Sub,
        VerticalAlign::Super,
        VerticalAlign::TextTop,
        VerticalAlign::TextBottom,
        VerticalAlign::Middle,
        VerticalAlign::Top,
        VerticalAlign::Bottom,
    ] {
        let mut root = style(2, 24.0);
        root.line_height = LineHeight::Px(100.0);
        let mut child = style(0, 16.0);
        child.line_height = LineHeight::Px(32.0);
        child.vertical_align = align;
        let line = layout(
            &ParagraphStyle {
                root,
                ..Default::default()
            },
            |b| {
                b.open_inline(NodeId(2), &child, InlineEdges::default())
                    .push_text(TextSource::Generated { node: NodeId(3) }, "a")
                    .close_inline();
            },
        );
        let run = line
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        let baseline = line.baseline(BaselineKind::Alphabetic);
        let expected = match align {
            VerticalAlign::Baseline => baseline,
            VerticalAlign::Length(v) => baseline - v,
            VerticalAlign::Sub => baseline + f32::from(os2.y_subscript_y_offset()) * scale,
            VerticalAlign::Super => baseline - f32::from(os2.y_superscript_y_offset()) * scale,
            VerticalAlign::TextTop => baseline + a - parent.ascent,
            VerticalAlign::TextBottom => baseline - parent.descent - d,
            VerticalAlign::Middle => baseline + (a - d) / 2.0 - parent.x_height.unwrap() / 2.0,
            VerticalAlign::Top => a,
            VerticalAlign::Bottom => 100.0 - d,
        };
        close(run.baseline(), expected);
        close(line.block_size(), 100.0);
        let published = run.metrics();
        close(published.ascent, child_metrics.ascent);
        close(published.descent, -child_metrics.descent);
        close(published.x_height, child_metrics.x_height.unwrap());
        close(published.cap_height, child_metrics.cap_height.unwrap());
        close(
            published.underline_offset,
            -child_metrics.underline.unwrap().offset,
        );
        close(
            published.strikeout_offset,
            -child_metrics.strikeout.unwrap().offset,
        );
        assert_eq!(run.clusters().next().unwrap().source_char, Some('a'));
    }
}

#[test]
fn deep_inline_metrics_do_not_recurse() {
    let limits = Limits {
        max_nesting_depth: None,
        ..Default::default()
    };
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(0, 16.0),
            ..Default::default()
        },
        &limits,
    );
    for node in 0..2048 {
        b.open_inline(NodeId(node + 10), &style(0, 16.0), InlineEdges::default());
    }
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    for _ in 0..2048 {
        b.close_inline();
    }
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    let m = FontRef::from_index(FONTS[0].bytes, 0)
        .unwrap()
        .metrics(Size::new(16.0), LocationRef::default());
    close(lines[0].block_size(), m.ascent - m.descent + m.leading);
    assert_eq!(lines[0].text_range(), 0..1);
}

#[test]
fn vertical_align_and_inline_edges_stop_cross_box_shaping() {
    let root = ParagraphStyle {
        root: style(0, 20.0),
        ..Default::default()
    };
    let ids = |line: &Line| {
        line.fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect::<Vec<_>>()
    };
    let expected: Vec<_> = ["f", "fi"]
        .into_iter()
        .flat_map(|text| {
            ids(&layout(&root, |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            }))
        })
        .collect();
    for (align, lead_margin, lead_padding) in [
        (VerticalAlign::Length(8.0), 0.0, 0.0),
        (VerticalAlign::Baseline, 0.0, 2.0),
        (VerticalAlign::Baseline, -2.0, 2.0),
    ] {
        let mut child = root.root.clone();
        child.vertical_align = align;
        let mut edges = InlineEdges::default();
        edges.margin.inline_start = lead_margin;
        edges.padding.inline_start = lead_padding;
        let actual = layout(&root, |b| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, "f")
                .open_inline(NodeId(2), &child, edges)
                .push_text(TextSource::Generated { node: NodeId(3) }, "fi")
                .close_inline();
        });
        assert_eq!(ids(&actual), expected, "align={align:?}, edges={edges:?}");
        let runs: Vec<_> = actual
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(runs.len(), 2);
        close(
            runs[0].baseline(),
            actual.baseline(BaselineKind::Alphabetic),
        );
        close(
            runs[1].baseline() - runs[0].baseline(),
            if align == VerticalAlign::Length(8.0) {
                -8.0
            } else {
                0.0
            },
        );
    }
}

#[test]
fn atomic_vertical_align_uses_parent_font_and_margin_box() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let font = FontRef::from_index(FONTS[2].bytes, 0).unwrap();
    let parent = font.metrics(Size::new(24.0), LocationRef::default());
    let os2 = font.os2().unwrap();
    let scale = 24.0 / f32::from(parent.units_per_em);
    for align in [
        VerticalAlign::Baseline,
        VerticalAlign::Length(7.0),
        VerticalAlign::Sub,
        VerticalAlign::Super,
        VerticalAlign::TextTop,
        VerticalAlign::TextBottom,
        VerticalAlign::Middle,
        VerticalAlign::Top,
        VerticalAlign::Bottom,
    ] {
        let mut root = style(2, 24.0);
        root.line_height = LineHeight::Px(100.0);
        let mut child = style(0, 16.0);
        child.vertical_align = align;
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_atomic(NodeId(4), &child, InlineEdges::default());
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let mut sizes = AtomicSizes::new();
        sizes.insert(
            NodeId(4),
            AtomicSize {
                inline_size: 10.0,
                block_size: 20.0,
                baseline: Some(12.0),
                ..Default::default()
            },
        );
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            100.0,
            &sizes,
        );
        let line = &lines[0];
        let atomic = line
            .fragments()
            .find_map(|f| match f {
                Fragment::Atomic(a) => Some(a),
                _ => None,
            })
            .unwrap();
        let base = line.baseline(BaselineKind::Alphabetic);
        let expected = match align {
            VerticalAlign::Baseline => base,
            VerticalAlign::Length(v) => base - v,
            VerticalAlign::Sub => base + f32::from(os2.y_subscript_y_offset()) * scale,
            VerticalAlign::Super => base - f32::from(os2.y_superscript_y_offset()) * scale,
            VerticalAlign::TextTop => base + 12.0 - parent.ascent,
            VerticalAlign::TextBottom => base - parent.descent - 8.0,
            VerticalAlign::Middle => base + 2.0 - parent.x_height.unwrap() / 2.0,
            VerticalAlign::Top => 12.0,
            VerticalAlign::Bottom => 92.0,
        };
        close(atomic.baseline, expected);
        close(atomic.margin_rect.block_start, expected - 12.0);
        close(line.block_size(), 100.0);
    }
}
