//! Consumer-visible ruby input contracts. These tests catch annotation text
//! leaking into the parent, source remapping loss and discarded input errors.
use shodo::limits::{LimitKind, Limits, WarningKind};
use shodo::mapping::TextOrigin;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo::{
    AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder, Ruby, RubyAlign,
    RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle, RubyVisibility,
};
use shodo_fixtures::load_fonts;

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![
            FontFamily::Named("Shodo Fixture CJK".into()),
            FontFamily::Named("Shodo Fixture Latin".into()),
        ],
        ..Default::default()
    }
}

fn source(node: u64, offset: u32) -> TextSource {
    TextSource::Dom {
        node: NodeId(node),
        offset,
    }
}

fn content(node: u64, text: &str, size: f32) -> RubyContent {
    RubyContent::text(source(node, 0), text, &style(size), &Limits::default())
}

fn base(node: u64, text: &str) -> RubyBase {
    RubyBase {
        node: NodeId(node),
        content: content(node, text, 24.0),
        align: RubyAlign::default(),
    }
}

fn annotation(node: u64, text: &str, span: RubySpan) -> RubyAnnotation {
    RubyAnnotation {
        node: NodeId(node),
        content: content(node, text, 12.0),
        span,
        visibility: RubyVisibility::Visible,
    }
}

fn level(annotations: Vec<RubyAnnotation>) -> RubyLevel {
    RubyLevel {
        annotations,
        style: RubyStyle::default(),
    }
}

fn logical_text(text: &str) -> String {
    // shodo's existing Paragraph::text includes generated bidi controls.
    // Removing only those controls leaves literal expected base content.
    text.chars()
        .filter(|ch| !matches!(*ch, '\u{2066}'..='\u{2069}' | '\u{202a}'..='\u{202e}'))
        .collect()
}

#[test]
fn ruby_parent_source_projection() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let root = ParagraphStyle {
        root: style(24.0),
        ..Default::default()
    };
    let mut inner = ParagraphBuilder::new(&root, &limits);
    inner
        .push_text(source(10, 40), "日本")
        .push_text(source(11, 70), "語");
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(9),
            content: RubyContent::from_builder(inner),
            align: RubyAlign::default(),
        }],
        vec![level(vec![annotation(20, "にほんご", RubySpan::Auto)])],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(&root, &limits);
    b.push_text(source(1, 0), "A ")
        .push_ruby(NodeId(8), &style(24.0), ruby)
        .push_text(source(2, 0), " Z");
    let paragraph = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert_eq!(logical_text(paragraph.text()), "A 日本語 Z");
    let mapping = paragraph.offset_mapping().unwrap();
    for (node, offset) in [(10, 40), (10, 43), (10, 46), (11, 70), (11, 73)] {
        let (at, affinity) = mapping.dom_to_text(NodeId(node), offset).unwrap();
        assert_eq!(
            mapping.text_to_dom(at, affinity),
            Some(TextOrigin::Dom {
                node: NodeId(node),
                offset,
            }),
            "node{node} offset{offset}"
        );
    }
    assert_eq!(mapping.dom_to_text(NodeId(20), 0), None);
    assert!(!paragraph.text().contains("にほんご"));
}

#[test]
fn ruby_auto_all_and_explicit_spans_preserve_base_order() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let cases = [
        Ruby::new(
            vec![base(10, "日")],
            vec![level(vec![
                annotation(20, "に", RubySpan::Auto),
                annotation(21, "ほん", RubySpan::Auto),
            ])],
        ),
        Ruby::new(
            vec![base(10, "日"), base(11, "本"), base(12, "語")],
            vec![level(vec![annotation(20, "にほんご", RubySpan::All)])],
        ),
        Ruby::new(
            vec![base(10, "日"), base(11, "本"), base(12, "語")],
            vec![level(vec![
                annotation(20, "に", RubySpan::Columns(0..1)),
                annotation(21, "ほんご", RubySpan::Columns(1..3)),
            ])],
        ),
        Ruby::new(
            vec![],
            vec![level(vec![annotation(20, "にほん", RubySpan::Auto)])],
        ),
    ];
    for (case, expected) in cases.into_iter().zip(["日", "日本語", "日本語", ""]) {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_ruby(NodeId(8), &style(24.0), case.unwrap());
        let p = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        assert_eq!(logical_text(p.text()), expected);
        assert!(
            p.offset_mapping()
                .unwrap()
                .dom_to_text(NodeId(20), 0)
                .is_none()
        );
    }
}

#[test]
fn ruby_rejects_invalid_explicit_spans_without_shaping() {
    for spans in [vec![1..1], vec![0..4], vec![0..2, 1..3]] {
        let annotations = spans
            .into_iter()
            .enumerate()
            .map(|(i, span)| annotation(20 + i as u64, "に", RubySpan::Columns(span)))
            .collect();
        assert!(
            Ruby::new(
                vec![base(10, "日"), base(11, "本"), base(12, "語")],
                vec![level(annotations)]
            )
            .is_err()
        );
    }
}

#[test]
fn ruby_nested_input_and_unclosed_boxes_preserve_sources_and_warnings() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let inner = Ruby::new(
        vec![base(10, "日本")],
        vec![level(vec![annotation(20, "にほん", RubySpan::Auto)])],
    )
    .unwrap();
    let mut nested = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    nested
        .open_inline(NodeId(5), &style(24.0), InlineEdges::default())
        .push_text(source(6, 100), "A")
        .push_ruby(NodeId(7), &style(24.0), inner);
    let outer = Ruby::new(
        vec![RubyBase {
            node: NodeId(4),
            content: RubyContent::from_builder(nested),
            align: RubyAlign::default(),
        }],
        vec![level(vec![annotation(30, "よみ", RubySpan::Auto)])],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_ruby(NodeId(3), &style(24.0), outer);
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert_eq!(logical_text(p.text()), "A日本");
    assert!(
        p.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::UnbalancedInline)
    );
    assert!(
        p.offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(6), 100)
            .is_some()
    );
    assert!(
        p.offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(10), 0)
            .is_some()
    );
    assert_eq!(p.offset_mapping().unwrap().dom_to_text(NodeId(20), 0), None);
}

#[test]
fn ruby_content_terminal_limit_error_is_not_discarded() {
    let limits = Limits {
        max_text_bytes: Some(2),
        ..Default::default()
    };
    let fonts = load_fonts(&Limits::default()).unwrap();
    for bad_annotation in [false, true] {
        let mut bad = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        bad.push_text(source(50, 0), "日本");
        let bad = RubyContent::from_builder(bad);
        let ruby = if bad_annotation {
            Ruby::new(
                vec![base(10, "日")],
                vec![level(vec![RubyAnnotation {
                    node: NodeId(50),
                    content: bad,
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }])],
            )
        } else {
            Ruby::new(
                vec![RubyBase {
                    node: NodeId(50),
                    content: bad,
                    align: RubyAlign::default(),
                }],
                vec![level(vec![annotation(20, "に", RubySpan::Auto)])],
            )
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_ruby(NodeId(8), &style(24.0), ruby.unwrap());
        let error = b
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap_err();
        assert_eq!(error.kind, LimitKind::TextBytes);
        assert_eq!((error.limit, error.actual), (2, 6));
    }
}

#[test]
fn ruby_empty_container_terminates_incremental_layout() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &style(24.0), Ruby::new(vec![], vec![]).unwrap());
    let mut cx = LayoutContext::new();
    let p = b.build(&mut cx, &fonts.collection).unwrap();
    assert_eq!(logical_text(p.text()), "");
    let mut token = p.start_token();
    for _ in 0..3 {
        match p.next_line(
            &mut cx,
            token,
            &Default::default(),
            &LineConstraint::new(0.0),
            &AtomicSizes::EMPTY,
        ) {
            LineResult::Done => return,
            LineResult::Line(line) => {
                assert_ne!(line.break_token(), token);
                token = line.break_token();
            }
            other => panic!("empty ruby did not progress: {other:?}"),
        }
    }
    panic!("empty ruby failed to terminate");
}

#[test]
fn ruby_shared_annotation_input_is_charged_for_every_lane() {
    let shared = content(20, "に", 12.0);
    let ruby = Ruby::new(
        vec![base(10, "日")],
        (0..2)
            .map(|i| {
                level(vec![RubyAnnotation {
                    node: NodeId(20 + i),
                    content: shared.clone(),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }])
            })
            .collect(),
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_text_bytes: Some(8),
            ..Default::default()
        },
    );
    b.push_ruby(NodeId(8), &style(24.0), ruby);
    let error = b.error().unwrap();
    assert_eq!(error.kind, LimitKind::TextBytes);
    assert_eq!((error.limit, error.actual), (8, 9));
}

#[test]
fn plain_text_after_ruby_includes_retained_annotation_budget() {
    let ruby = Ruby::new(
        vec![base(10, "日")],
        vec![level(vec![annotation(20, "に", RubySpan::Auto)])],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_text_bytes: Some(8),
            ..Default::default()
        },
    );
    b.push_ruby(NodeId(8), &style(24.0), ruby);
    assert!(b.error().is_none());
    b.push_text(source(30, 0), "abcd");
    let error = b.error().unwrap();
    assert_eq!(error.kind, LimitKind::TextBytes);
    assert_eq!((error.limit, error.actual), (8, 10));
}

#[test]
fn base_inline_depth_is_checked_in_parent_context() {
    let mut inner = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    for node in 0..4 {
        inner.open_inline(NodeId(node), &style(24.0), InlineEdges::default());
    }
    inner.push_text(source(10, 0), "日");
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(9),
            content: RubyContent::from_builder(inner),
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_nesting_depth: Some(5),
            ..Default::default()
        },
    );
    b.push_ruby(NodeId(8), &style(24.0), ruby);
    let error = b.error().unwrap();
    assert_eq!(error.kind, LimitKind::NestingDepth);
    assert_eq!((error.limit, error.actual), (5, 6));
}

#[test]
fn base_snapshot_first_line_inheritance_uses_its_own_root() {
    // Importing a snapshot must not resolve its descendant inheritance against
    // the unrelated outer paragraph root. The uppercased ß expands to SS.
    let normal = style(24.0);
    let first = InlineStyle {
        text_transform: shodo::style::TextTransform::Uppercase,
        ..normal.clone()
    };
    let mut inner = ParagraphBuilder::new(
        &ParagraphStyle {
            root: normal.clone(),
            first_line: Some(first),
            ..Default::default()
        },
        &Limits::default(),
    );
    inner
        .open_inline(
            NodeId(11),
            &InlineStyle {
                font_weight: 700.0,
                ..normal.clone()
            },
            InlineEdges::default(),
        )
        .push_text(source(10, 40), "ßa")
        .close_inline();
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(9),
            content: RubyContent::from_builder(inner),
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_ruby(NodeId(8), &normal, ruby);
    let fonts = load_fonts(&Limits::default()).unwrap();
    let mut cx = LayoutContext::new();
    let p = b.build(&mut cx, &fonts.collection).unwrap();
    assert_eq!(logical_text(p.text()), "ßa");
    let LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &LineConstraint::new(500.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected ruby base line");
    };
    assert_eq!(logical_text(line.text()), "SSA");
    let map = line.offset_mapping().unwrap();
    assert!(map.dom_to_text(NodeId(10), 40).is_some());
    assert!(map.dom_to_text(NodeId(10), 42).is_some());
}

#[test]
fn pairing_allocation_error_reports_original_parent_limit() {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits {
            max_items: Some(8),
            ..Default::default()
        },
    );
    for node in 0..5 {
        b.push_text(source(node, 0), "a");
    }
    assert!(b.error().is_none());
    b.push_ruby(
        NodeId(8),
        &style(24.0),
        Ruby::new(
            vec![base(10, "日")],
            vec![level(vec![annotation(20, "に", RubySpan::Auto)])],
        )
        .unwrap(),
    );
    let error = b.error().unwrap();
    assert_eq!(error.kind, LimitKind::Items);
    assert_eq!((error.limit, error.actual), (8, 9));
}
