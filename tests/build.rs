use shodo::font::FontCollection;
use shodo::geometry::{BaselineKind, WritingMode};
use shodo::limits::{LimitKind, Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle, TextOrientation};
use shodo::{LayoutContext, Paragraph, ParagraphBuilder, RichText};

fn fonts() -> FontCollection {
    FontCollection::new(&Limits::default())
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
fn first_line_style_is_reported_as_unsupported() {
    let style = ParagraphStyle {
        first_line: Some(InlineStyle::default()),
        ..ParagraphStyle::default()
    };
    let para = ParagraphBuilder::new(&style, &Limits::default())
        .build(&mut LayoutContext::new(), &fonts())
        .unwrap();
    assert!(
        para.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::Unsupported)
    );
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
