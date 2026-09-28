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
    for spans in [
        std::iter::once(1..1).collect(),
        std::iter::once(0..4).collect(),
        vec![0..2, 1..3],
    ] {
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
fn base_snapshot_shaping_cap_survives_import_and_first_line() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (alternate, cap, wanted_glyphs) in
        [(false, 0, 1), (false, 1, 1), (true, 1, 2), (true, 2, 2)]
    {
        let own = Limits {
            max_shaped_glyphs: Some(cap),
            ..Default::default()
        };
        let root = ParagraphStyle {
            root: style(24.0),
            first_line: alternate.then(|| style(36.0)),
            ..Default::default()
        };
        let mut content = ParagraphBuilder::new(&root, &own);
        content.push_text(source(10, 40), "日");
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::from_builder(content),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        parent.push_ruby(NodeId(8), &style(24.0), ruby);
        let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
        if cap < wanted_glyphs {
            let error = result.expect_err("base own cap survives its import into a larger parent");
            assert_eq!(
                (error.kind, error.limit, error.actual),
                (LimitKind::ShapedGlyphs, cap, wanted_glyphs)
            );
        } else {
            assert_eq!(logical_text(result.unwrap().text()), "日");
        }
    }
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

#[test]
fn imported_base_own_cap_covers_nested_readings_and_both_style_sets() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (alternate, cap, wanted) in [(false, 1, 2), (false, 2, 2), (true, 3, 4), (true, 4, 4)] {
        let own = Limits {
            max_shaped_glyphs: Some(cap),
            ..Default::default()
        };
        let mut reading = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(12.0),
                first_line: alternate.then(|| style(18.0)),
                ..Default::default()
            },
            &Limits::default(),
        );
        reading.push_text(source(20, 70), "に");
        let nested = Ruby::new(
            vec![base(10, "日")],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20),
                    content: RubyContent::from_builder(reading),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut content = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                first_line: alternate.then(|| style(36.0)),
                ..Default::default()
            },
            &own,
        );
        content.push_ruby(NodeId(18), &style(24.0), nested);
        let outer = Ruby::new(
            vec![RubyBase {
                node: NodeId(9),
                content: RubyContent::from_builder(content),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        parent.push_ruby(NodeId(8), &style(24.0), outer);
        let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
        if cap < wanted {
            let error = result.expect_err(
                "nested retained readings belong to their imported base's own resource scope",
            );
            assert_eq!(
                (error.kind, error.limit, error.actual),
                (LimitKind::ShapedGlyphs, cap, wanted)
            );
        } else {
            let p = result.unwrap();
            assert_eq!(logical_text(p.text()), "日");
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                100.0,
                &shodo::AtomicSizes::EMPTY,
            );
            assert!(
                lines
                    .iter()
                    .flat_map(|line| line.ruby_annotations())
                    .any(|a| !a.line().is_empty())
            );
        }
    }
}

#[test]
fn imported_base_retained_text_items_and_styles_obey_own_exact_caps() {
    use shodo::style::TextTransform;
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (alternate, kind, total) in [
        (false, LimitKind::TextBytes, 7),
        (true, LimitKind::TextBytes, 16),
        (false, LimitKind::Items, 5),
        (true, LimitKind::Items, 10),
        (false, LimitKind::Styles, 2),
        (true, LimitKind::Styles, 4),
    ] {
        for cap in [total - 1, total] {
            let mut own = Limits::default();
            match kind {
                LimitKind::TextBytes => own.max_text_bytes = Some(cap),
                LimitKind::Items => own.max_items = Some(cap),
                LimitKind::Styles => own.max_styles = Some(cap),
                _ => unreachable!(),
            }
            let mut first = style(36.0);
            first.text_transform = TextTransform::FullWidth;
            let mut input = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style(24.0),
                    first_line: alternate.then_some(first),
                    ..Default::default()
                },
                &own,
            );
            input.push_text(source(10, 40), "A");
            let r = Ruby::new(
                vec![RubyBase {
                    node: NodeId(10),
                    content: RubyContent::from_builder(input),
                    align: RubyAlign::default(),
                }],
                vec![],
            )
            .unwrap();
            let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
            parent.push_ruby(NodeId(8), &style(24.0), r);
            let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
            if cap < total {
                let error =
                    result.expect_err("both retained sets obey each base occurrence's own cap");
                assert_eq!((error.kind, error.limit), (kind, cap));
                assert!(error.actual > cap);
            } else {
                let p = result.unwrap();
                assert_eq!(logical_text(p.text()), "A");
                let lines = p.break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    100.0,
                    &AtomicSizes::EMPTY,
                );
                assert_eq!(
                    lines
                        .iter()
                        .map(|l| logical_text(l.text()))
                        .collect::<String>(),
                    if alternate { "Ａ" } else { "A" }
                );
            }
        }
    }
}

#[test]
fn reused_base_snapshot_has_independent_own_scopes_and_shared_parent_cap() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let shared = RubyContent::text(
        source(10, 40),
        "日",
        &style(24.0),
        &Limits {
            max_shaped_glyphs: Some(1),
            ..Default::default()
        },
    );
    let r = Ruby::new(
        (0..2)
            .map(|i| RubyBase {
                node: NodeId(10 + i),
                content: shared.clone(),
                align: RubyAlign::default(),
            })
            .collect(),
        vec![],
    )
    .unwrap();
    for parent_cap in [1, 2] {
        let mut parent = ParagraphBuilder::new(
            &ParagraphStyle::default(),
            &Limits {
                max_shaped_glyphs: Some(parent_cap),
                ..Default::default()
            },
        );
        parent.push_ruby(NodeId(8), &style(24.0), r.clone());
        let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
        if parent_cap == 1 {
            let error = result.unwrap_err();
            assert_eq!(
                (error.kind, error.limit, error.actual),
                (LimitKind::ShapedGlyphs, 1, 2)
            );
        } else {
            assert_eq!(logical_text(result.unwrap().text()), "日日");
        }
    }
}

#[test]
fn imported_base_wrapper_checks_own_limits_before_builder_growth() {
    for (kind, own) in [
        (
            LimitKind::Styles,
            Limits {
                max_styles: Some(1),
                ..Default::default()
            },
        ),
        (
            LimitKind::Items,
            Limits {
                max_items: Some(2),
                ..Default::default()
            },
        ),
        (
            LimitKind::NestingDepth,
            Limits {
                max_nesting_depth: Some(0),
                ..Default::default()
            },
        ),
    ] {
        let r = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(source(10, 40), "日", &style(24.0), &own),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        parent.push_ruby(NodeId(8), &style(24.0), r);
        assert_eq!(
            parent
                .error()
                .expect("wrapper resources must be checked before import grows the parent")
                .kind,
            kind
        );
    }
}

#[test]
fn imported_base_scope_does_not_charge_neighboring_plain_text() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let own = Limits {
        max_shaped_glyphs: Some(1),
        ..Default::default()
    };
    let r = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(source(10, 40), "日", &style(24.0), &own),
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let mut parent = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(24.0),
            ..Default::default()
        },
        &Limits::default(),
    );
    parent.push_text(source(1, 0), "本");
    parent.push_ruby(NodeId(8), &style(24.0), r);
    parent.push_text(source(2, 0), "語");
    let p = parent
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    assert_eq!(logical_text(p.text()), "本日語");
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    let glyphs: Vec<_> = lines
        .iter()
        .flat_map(|line| line.fragments())
        .filter_map(|fragment| {
            if let shodo::Fragment::GlyphRun(run) = fragment {
                Some(run.glyphs().collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert_eq!(glyphs.len(), 3);
    assert!(glyphs.iter().all(|g| g.id != 0));
}

#[test]
fn imported_base_scope_counts_nested_metadata_cuts_and_owned_index_cells() {
    // Main: three isolate wrappers + 日 =21 bytes/17 items/2 styles.
    // Reading: one wrapper + に =9 bytes/5 items/2 styles. Nested pairing
    // and cuts add4+4 items; its exclusively owned interval leaf adds1.
    // The outer container and shared index cells stay in the parent cap.
    let fonts = load_fonts(&Limits::default()).unwrap();
    for (alternate, kind, total) in [
        (false, LimitKind::TextBytes, 30),
        (true, LimitKind::TextBytes, 60),
        (false, LimitKind::Items, 31),
        (true, LimitKind::Items, 62),
        (false, LimitKind::Styles, 4),
        (true, LimitKind::Styles, 8),
    ] {
        for cap in [total - 1, total] {
            let mut own = Limits::default();
            match kind {
                LimitKind::TextBytes => own.max_text_bytes = Some(cap),
                LimitKind::Items => own.max_items = Some(cap),
                LimitKind::Styles => own.max_styles = Some(cap),
                _ => unreachable!(),
            }
            let mut reading = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style(12.0),
                    first_line: alternate.then(|| style(18.0)),
                    ..Default::default()
                },
                &Limits::default(),
            );
            reading.push_text(source(20, 70), "に");
            let nested = Ruby::new(
                vec![base(10, "日")],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(20),
                        content: RubyContent::from_builder(reading),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            let mut input = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: style(24.0),
                    first_line: alternate.then(|| style(36.0)),
                    ..Default::default()
                },
                &own,
            );
            input.push_ruby(NodeId(18), &style(24.0), nested);
            let r = Ruby::new(
                vec![RubyBase {
                    node: NodeId(9),
                    content: RubyContent::from_builder(input),
                    align: RubyAlign::default(),
                }],
                vec![],
            )
            .unwrap();
            let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
            parent.push_ruby(NodeId(8), &style(24.0), r);
            let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
            if cap < total {
                let error = result.expect_err(
                    "nested metadata and reading datasets belong to the imported base scope",
                );
                assert_eq!((error.kind, error.limit), (kind, cap));
                assert!(error.actual > cap);
            } else {
                let p = result.unwrap();
                assert_eq!(logical_text(p.text()), "日");
                let lines = p.break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    100.0,
                    &AtomicSizes::EMPTY,
                );
                assert!(
                    lines
                        .iter()
                        .flat_map(|line| line.ruby_annotations())
                        .any(|a| !a.line().is_empty())
                );
            }
        }
    }
}

#[test]
fn imported_base_uses_its_own_shaping_run_budget_without_splitting_neighbors() {
    let fonts = load_fonts(&Limits::default()).unwrap();
    let latin = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
        ..style(24.0)
    };
    for own_glyph_cap in [1, 3] {
        let own = Limits {
            max_shaping_run_bytes: Some(1),
            max_shaped_glyphs: Some(own_glyph_cap),
            ..Default::default()
        };
        let r = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(source(10, 40), "ffi", &latin, &own),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let mut parent = ParagraphBuilder::new(
            &ParagraphStyle {
                root: latin.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        parent.push_ruby(NodeId(8), &latin, r);
        parent.push_text(source(20, 70), "ffi");
        let result = parent.build(&mut LayoutContext::new(), &fonts.collection);
        if own_glyph_cap == 1 {
            let error =
                result.expect_err("base's tiny run budget produces separate real f/f/i glyphs");
            assert_eq!(
                (error.kind, error.limit, error.actual),
                (LimitKind::ShapedGlyphs, 1, 2)
            );
        } else {
            let p = result.unwrap();
            assert_eq!(logical_text(p.text()), "ffiffi");
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1000.0,
                &AtomicSizes::EMPTY,
            );
            let mut base_glyphs = 0;
            let mut neighbor_glyphs = 0;
            for line in &lines {
                for fragment in line.fragments() {
                    if let shodo::Fragment::GlyphRun(run) = fragment {
                        assert!(run.glyphs().all(|g| g.id != 0));
                        match run.node() {
                            Some(NodeId(10)) => base_glyphs += run.glyphs().len(),
                            Some(NodeId(20)) => neighbor_glyphs += run.glyphs().len(),
                            _ => panic!("real text glyphs preserve their original owner"),
                        }
                    }
                }
            }
            assert_eq!((base_glyphs, neighbor_glyphs), (3, 1));
        }
    }
}
