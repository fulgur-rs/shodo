use shodo::font::FontCollection;
use shodo::geometry::{BaselineKind, WritingMode};
use shodo::limits::{LimitKind, Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{GenericFamily, InlineStyle, ParagraphStyle, TextOrientation};
use shodo::{LayoutContext, Paragraph, ParagraphBuilder, RichText};

fn fonts() -> FontCollection {
    FontCollection::with_options(
        &Limits::default(),
        shodo::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset: 0,
    }
}

#[test]
fn builds_and_keeps_identity_across_clones() {
    let mut cx = LayoutContext::new();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(dom(1), "hello  world");
    let para = b.build(&mut cx, &fonts()).unwrap();
    assert_eq!(para.text(), "hello world");
    let clone = para.clone();
    assert_eq!(clone.id(), para.id());
    assert_eq!(clone.start_token(), para.start_token());

    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(dom(1), "hello  world");
    let rebuilt = b.build(&mut cx, &fonts()).unwrap();
    assert_ne!(
        rebuilt.id(),
        para.id(),
        "identical content still gets a new id"
    );
    assert_ne!(rebuilt.start_token(), para.start_token());
}

#[test]
fn paragraph_analysis_can_be_shaped_later_without_a_layout_context() {
    fn assert_send<T: Send>() {}
    assert_send::<shodo::ParagraphAnalysis>();

    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(dom(1), "analysis first");
    let analysis = b.analyze().unwrap();
    let para = analysis.shape(&mut LayoutContext::new(), &fonts()).unwrap();
    assert_eq!(para.text(), "analysis first");
}

#[test]
fn empty_and_all_space_paragraphs_build() {
    let mut cx = LayoutContext::new();
    for text in ["", "   "] {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), text);
        assert_eq!(b.build(&mut cx, &fonts()).unwrap().text(), "");
    }
}

#[test]
fn builder_errors_surface_from_build() {
    let limits = Limits {
        max_text_bytes: Some(2),
        ..Limits::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(dom(1), "abc");
    let err = b.build(&mut LayoutContext::new(), &fonts()).unwrap_err();
    assert_eq!(err.kind, LimitKind::TextBytes);
}

#[test]
fn styled_breaks_obey_item_style_and_style_byte_limits() {
    let br = InlineStyle {
        font_size: 42.0,
        ..Default::default()
    };
    for (limits, expected) in [
        (
            Limits {
                max_items: Some(0),
                ..Limits::default()
            },
            LimitKind::Items,
        ),
        (
            Limits {
                max_styles: Some(1),
                ..Limits::default()
            },
            LimitKind::Styles,
        ),
        (
            Limits {
                max_style_bytes: Some(0),
                ..Limits::default()
            },
            LimitKind::StyleBytes,
        ),
    ] {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_forced_break_with_style(NodeId(3), &br);
        // The first failure remains latched, including on further large styles.
        b.push_forced_break_with_style(NodeId(4), &br);
        let error = b.build(&mut LayoutContext::new(), &fonts()).unwrap_err();
        assert_eq!(error.kind, expected);
    }
    let limits = Limits {
        max_styles: Some(1),
        ..Limits::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_forced_break_with_style(NodeId(3), &InlineStyle::default());
    assert_eq!(
        b.build(&mut LayoutContext::new(), &fonts()).unwrap().text(),
        "\n"
    );
}

#[test]
fn shaped_glyph_limit_is_enforced_at_build() {
    let limits = Limits {
        max_shaped_glyphs: Some(2),
        ..Limits::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(dom(1), "abc");
    let err = b.build(&mut LayoutContext::new(), &fonts()).unwrap_err();
    assert_eq!(err.kind, LimitKind::ShapedGlyphs);
}

#[test]
fn unclosed_inline_boxes_are_closed_with_a_warning() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(NodeId(1), &InlineStyle::default(), InlineEdges::default())
        .push_text(dom(2), "x");
    let para = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(
        para.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::UnbalancedInline)
    );
}

#[test]
fn first_line_style_applies_without_a_font() {
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_size: 32.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(dom(1), "x")
        .push_forced_break(NodeId(2))
        .push_text(dom(3), "y");
    let para = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(
        !para
            .warnings()
            .iter()
            .any(|w| w.message.contains("::first-line"))
    );
    let lines = para.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &shodo::AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].inline_size(), 32.0);
    assert_eq!(lines[1].inline_size(), 16.0);
    for line in &lines {
        let glyphs: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                shodo::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .flat_map(|r| r.glyphs().map(|g| g.id))
            .collect();
        assert_eq!(glyphs, vec![0], "explicit missing-font output");
    }
}

#[test]
fn first_line_analysis_warnings_follow_normal_shaping_warnings() {
    let style = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_size: f32::NAN,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(dom(1), "a b");
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();

    let warnings = paragraph.warnings();
    let missing_font = warnings
        .iter()
        .position(|warning| warning.message.contains("missing font"))
        .unwrap();
    let first_line_analysis = warnings
        .iter()
        .position(|warning| {
            warning.kind == WarningKind::NonFiniteInput && warning.message.contains("font-size")
        })
        .unwrap();
    assert!(missing_font < first_line_analysis);
}

#[test]
fn negative_and_non_finite_font_sizes_are_neutralized() {
    let bad = InlineStyle {
        font_size: f32::NAN,
        ..InlineStyle::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(NodeId(1), &bad, InlineEdges::default())
        .push_text(dom(2), "x")
        .close_inline();
    let para = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(
        para.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::NonFiniteInput)
    );
}

#[test]
fn required_baseline_follows_the_parent_inline_box() {
    let build = |wm: WritingMode, orientation: TextOrientation| -> Paragraph {
        let style = ParagraphStyle {
            writing_mode: wm,
            ..ParagraphStyle::default()
        };
        let span = InlineStyle {
            text_orientation: orientation,
            ..InlineStyle::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_atomic(NodeId(1), &InlineStyle::default(), InlineEdges::default())
            .open_inline(NodeId(2), &span, InlineEdges::default())
            .push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default())
            .close_inline();
        b.build(&mut LayoutContext::new(), &fonts()).unwrap()
    };
    let horizontal = build(WritingMode::HorizontalTb, TextOrientation::Mixed);
    assert_eq!(
        horizontal.required_baseline(NodeId(1)),
        Some(BaselineKind::Alphabetic)
    );

    let vertical = build(WritingMode::VerticalRl, TextOrientation::Sideways);
    assert_eq!(
        vertical.required_baseline(NodeId(1)),
        Some(BaselineKind::Central)
    );
    assert_eq!(
        vertical.required_baseline(NodeId(3)),
        Some(BaselineKind::Alphabetic)
    );
    assert_eq!(vertical.required_baseline(NodeId(99)), None);
}

#[test]
fn font_generations_record_both_layers() {
    let shared = fonts();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    let para = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
        .build(&mut LayoutContext::new(), &doc)
        .unwrap();
    assert_eq!(para.font_generations(), (0, Some(0)));
}

#[test]
fn paragraph_reports_its_font_layer_and_staleness() {
    let shared = fonts();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    let para = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
        .build(&mut LayoutContext::new(), &doc)
        .unwrap();
    assert_eq!(para.font_layer_handle().id(), doc.layer_handle().id());
    assert!(!para.font_is_stale());

    shared.set_generic_families(GenericFamily::Serif, vec!["Changed".into()]);
    assert!(para.font_is_stale());
}

#[test]
fn rich_text_assigns_sequential_nodes() {
    let para = RichText::new(&ParagraphStyle::default())
        .push("Hello ", &InlineStyle::default())
        .push(
            "世界",
            &InlineStyle {
                font_weight: 700.0,
                ..InlineStyle::default()
            },
        )
        .build(&mut LayoutContext::new(), &fonts())
        .unwrap();
    assert_eq!(para.text(), "Hello 世界");
    let m = para.offset_mapping().unwrap();
    assert_eq!(m.dom_to_text(NodeId(1), 3).map(|(t, _)| t), Some(9));
}

#[test]
fn paragraph_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Paragraph>();
}
