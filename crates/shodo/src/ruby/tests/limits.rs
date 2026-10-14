//! Prepared lanes must retain real fonts and their own source projection.
use crate::font::FontFaceDescriptor;
use crate::limits::Limits;
use crate::mapping::TextOrigin;
use crate::node::{NodeId, TextSource};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
use crate::{LayoutContext, ParagraphBuilder};

const CJK: &[u8] = crate::test_support::fonts::CJK;

fn style(size: f32) -> InlineStyle {
    InlineStyle {
        font_size: size,
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    }
}

#[test]
fn annotation_retains_real_fonts() {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    let face = fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 40,
                },
                "日",
                &style(24.0),
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20),
                        offset: 70,
                    },
                    "にほん",
                    &style(12.0),
                    &Limits::default(),
                ),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    builder.push_ruby(NodeId(8), &style(24.0), ruby);
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    drop(fonts);
    let container = &paragraph.data.ruby.containers[0];
    assert_eq!(container.node, NodeId(8));
    let lane = &container.lanes[0];
    assert_eq!(lane.node, Some(NodeId(20)));
    let child = &lane.paragraph;
    // Literal IDs independently extracted from the pinned fixture's cmap with
    // FontTools before implementation: に=208, ほ=224, ん=248.
    assert_eq!(child.data.glyphs.id, [208, 224, 248]);
    assert!(!child.data.runs.is_empty());
    for run in &child.data.runs {
        assert_eq!(run.font, face);
        assert_eq!(run.font_size, 12.0);
    }
    assert_eq!(child.data.fonts.font_data(face).unwrap().data.as_ref(), CJK);
    let mapping = child.offset_mapping().unwrap();
    for offset in [70, 73, 76, 79] {
        let (at, affinity) = mapping.dom_to_text(NodeId(20), offset).unwrap();
        assert_eq!(
            mapping.text_to_dom(at, affinity),
            Some(TextOrigin::Dom {
                node: NodeId(20),
                offset
            })
        );
    }
    assert!(
        paragraph
            .offset_mapping()
            .unwrap()
            .dom_to_text(NodeId(20), 70)
            .is_none()
    );
}

#[test]
fn annotation_only_first_line_change_prepares_both_sets() {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut reading = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style(12.0),
            first_line: Some(style(18.0)),
            ..Default::default()
        },
        &Limits::default(),
    );
    reading.push_text(
        TextSource::Dom {
            node: NodeId(20),
            offset: 0,
        },
        "にほん",
    );
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                "日",
                &style(24.0),
                &Limits::default(),
            ),
            align: RubyAlign::default(),
        }],
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
    // Two independently retained parent sets and two reading sets. Each parent
    // has15 bytes/13 items/3 styles; each reading15/5/2. Pairing metadata and
    // two two-cell cuts add8 items plus2 interval-index cells per parent set. Glyphs are2+6=8.
    for (kind, total) in [
        (crate::limits::LimitKind::TextBytes, 60),
        (crate::limits::LimitKind::Items, 56),
        (crate::limits::LimitKind::Styles, 10),
        (crate::limits::LimitKind::ShapedGlyphs, 8),
    ] {
        for cap in [total - 1, total] {
            let mut limits = Limits::default();
            match kind {
                crate::limits::LimitKind::TextBytes => limits.max_text_bytes = Some(cap),
                crate::limits::LimitKind::Items => limits.max_items = Some(cap),
                crate::limits::LimitKind::Styles => limits.max_styles = Some(cap),
                crate::limits::LimitKind::ShapedGlyphs => limits.max_shaped_glyphs = Some(cap),
                _ => unreachable!(),
            }
            let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            builder.push_ruby(NodeId(8), &style(24.0), ruby.clone());
            let result = builder.build(&mut LayoutContext::new(), &fonts);
            if cap < total {
                let error = result.expect_err("both sets share the aggregate budget");
                assert_eq!((error.kind, error.limit), (kind, cap));
            } else {
                let p = result.unwrap();
                let normal = &p.data.ruby.containers[0].lanes[0].paragraph;
                assert!(normal.data.runs.iter().all(|run| run.font_size == 12.0));
                let first = p
                    .data
                    .first_line
                    .as_ref()
                    .expect("annotation changes activate first-line geometry");
                let alternate = &first.data.ruby.containers[0].lanes[0].paragraph;
                assert!(alternate.data.runs.iter().all(|run| run.font_size == 18.0));
                assert_eq!(normal.data.glyphs.len(), 3);
                assert_eq!(alternate.data.glyphs.len(), 3);
            }
        }
    }
}

#[test]
fn annotation_shaping_runs_share_the_parent_item_limit() {
    use crate::geometry::WritingMode;
    use crate::limits::{LimitExceeded, LimitKind};

    let limits = Limits {
        max_items: Some(300),
        ..Limits::default()
    };
    let fonts = crate::test_support::fonts::collection(&limits);
    let annotation_text = "a§b".repeat(100);
    let annotation = |node| RubyAnnotation {
        node: NodeId(node),
        content: RubyContent::text(
            TextSource::Dom {
                node: NodeId(node),
                offset: 0,
            },
            &annotation_text,
            &InlineStyle::default(),
            &limits,
        ),
        span: RubySpan::Auto,
        visibility: RubyVisibility::Visible,
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                "日",
                &InlineStyle::default(),
                &limits,
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![annotation(20), annotation(21)],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let style = ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_ruby(NodeId(8), &style.root, ruby);

    let error = builder
        .build(&mut LayoutContext::new(), &fonts)
        .expect_err("separate annotation itemizations share the parent's shaping-run cap");

    assert_eq!(
        error,
        LimitExceeded {
            kind: LimitKind::Items,
            limit: 300,
            actual: 301,
        }
    );
}

#[test]
fn imported_base_shaping_runs_use_the_base_snapshot_item_limit() {
    use crate::geometry::WritingMode;
    use crate::limits::{LimitExceeded, LimitKind};

    let parent_limits = Limits {
        max_items: Some(20),
        ..Limits::default()
    };
    let base_limits = Limits {
        max_items: Some(7),
        ..Limits::default()
    };
    let fonts = crate::test_support::fonts::collection(&parent_limits);
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                "a§b§a§b§a",
                &InlineStyle::default(),
                &base_limits,
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(20),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(20),
                        offset: 0,
                    },
                    "x",
                    &InlineStyle::default(),
                    &parent_limits,
                ),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let style = ParagraphStyle {
        writing_mode: WritingMode::VerticalRl,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &parent_limits);
    builder.push_ruby(NodeId(8), &style.root, ruby);

    let error = builder
        .build(&mut LayoutContext::new(), &fonts)
        .expect_err("an imported base keeps its own shaping-run cap");

    assert_eq!(
        error,
        LimitExceeded {
            kind: LimitKind::Items,
            limit: 7,
            actual: 8,
        }
    );
}

#[test]
fn annotation_forced_breaks_preserve_noncollapsible_spaces() {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for (collapse, expected) in [
        (crate::style::WhiteSpaceCollapse::Preserve, "に     ほん"),
        (crate::style::WhiteSpaceCollapse::Collapse, "にほん"),
    ] {
        let mut reading = ParagraphBuilder::new(
            &ParagraphStyle {
                root: InlineStyle {
                    white_space_collapse: collapse,
                    ..style(12.0)
                },
                ..Default::default()
            },
            &Limits::default(),
        );
        reading
            .push_text(
                TextSource::Dom {
                    node: NodeId(20),
                    offset: 70,
                },
                "に  ",
            )
            .push_forced_break(NodeId(21))
            .push_text(
                TextSource::Dom {
                    node: NodeId(22),
                    offset: 100,
                },
                "  ほん",
            );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(10),
                content: RubyContent::text(
                    TextSource::Dom {
                        node: NodeId(10),
                        offset: 0,
                    },
                    "日",
                    &style(24.0),
                    &Limits::default(),
                ),
                align: RubyAlign::default(),
            }],
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
        let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        builder.push_ruby(NodeId(8), &style(24.0), ruby);
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let child = &p.data.ruby.containers[0].lanes[0].paragraph;
        let logical: String = child
            .text()
            .chars()
            .filter(|ch| !matches!(*ch, '\u{2066}'..='\u{2069}' | '\u{202a}'..='\u{202e}'))
            .collect();
        assert_eq!(logical, expected);
        assert!(
            !child
                .data
                .units
                .iter()
                .any(|u| matches!(u.kind, crate::analysis::units::UnitKind::ForcedBreak))
        );
        assert!(
            child
                .offset_mapping()
                .unwrap()
                .dom_to_text(NodeId(22), 108)
                .is_some()
        );
    }
}

#[test]
fn equal_root_and_base_glyph_room_reports_the_owning_base_original_cap() {
    use crate::limits::{LimitExceeded, LimitKind};
    let limits = Limits {
        max_shaped_glyphs: Some(10),
        ..Limits::default()
    };
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = style(16.0);
    for first_line in [false, true] {
        let paragraph_style = ParagraphStyle {
            root: root.clone(),
            first_line: first_line.then(|| root.clone()),
            ..Default::default()
        };
        let content = RubyContent::text(
            TextSource::Generated { node: NodeId(2) },
            "123456",
            &root,
            &Limits {
                max_shaped_glyphs: Some(5),
                ..Limits::default()
            },
        );
        let annotation = RubyContent::text(
            TextSource::Generated { node: NodeId(3) },
            "あ",
            &root,
            &Limits::default(),
        );
        let ruby = Ruby::new(
            vec![RubyBase {
                node: NodeId(2),
                content,
                align: RubyAlign::default(),
            }],
            vec![RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(3),
                    content: annotation,
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            }],
        )
        .unwrap();
        let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "12345");
        builder.push_ruby(NodeId(4), &root, ruby);
        let error = builder
            .build(&mut LayoutContext::new(), &fonts)
            .unwrap_err();
        assert_eq!(
            error,
            LimitExceeded {
                kind: LimitKind::ShapedGlyphs,
                limit: 5,
                actual: 6
            },
            "first_line={first_line}"
        );
    }
}

#[test]
fn sibling_base_first_line_costs_share_only_the_paragraph_ancestor() {
    use crate::limits::{LimitExceeded, LimitKind};
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = style(16.0);
    let mut first = root.clone();
    first.paint.color = [255, 0, 0, 255];
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        first_line: Some(first),
        ..Default::default()
    };
    for (paragraph_cap, left_cap, right_cap, expected) in [
        (12, 4, 6, None),
        (
            12,
            3,
            6,
            Some(LimitExceeded {
                kind: LimitKind::ShapedGlyphs,
                limit: 3,
                actual: 4,
            }),
        ),
        (
            12,
            4,
            5,
            Some(LimitExceeded {
                kind: LimitKind::ShapedGlyphs,
                limit: 5,
                actual: 6,
            }),
        ),
        (
            11,
            4,
            6,
            Some(LimitExceeded {
                kind: LimitKind::ShapedGlyphs,
                limit: 11,
                actual: 12,
            }),
        ),
    ] {
        let limits = Limits {
            max_shaped_glyphs: Some(paragraph_cap),
            ..Limits::default()
        };
        let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "1");
        for (node, text, cap) in [(NodeId(2), "12", left_cap), (NodeId(3), "123", right_cap)] {
            let content = RubyContent::text(
                TextSource::Generated { node },
                text,
                &root,
                &Limits {
                    max_shaped_glyphs: Some(cap),
                    ..Limits::default()
                },
            );
            let ruby = Ruby::new(
                vec![RubyBase {
                    node,
                    content,
                    align: RubyAlign::default(),
                }],
                Vec::new(),
            )
            .unwrap();
            builder.push_ruby(node, &root, ruby);
        }
        let result = builder.build(&mut LayoutContext::new(), &fonts);
        match expected {
            Some(error) => assert_eq!(result.unwrap_err(), error),
            None => {
                let paragraph = result.unwrap();
                assert_eq!(paragraph.data.glyphs.len(), 6);
                assert_eq!(
                    paragraph
                        .data
                        .first_line
                        .as_ref()
                        .unwrap()
                        .data
                        .glyphs
                        .len(),
                    6
                );
                assert!(paragraph.warnings().is_empty());
            }
        }
    }
}

#[test]
fn nested_annotation_keeps_its_own_non_cumulative_shaping_run_cap() {
    let limits = Limits::default();
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register_face(
            CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let root = style(16.0);
    let content =
        |node, text| RubyContent::text(TextSource::Generated { node }, text, &root, &limits);
    let inner = Ruby::new(
        vec![RubyBase {
            node: NodeId(2),
            content: content(NodeId(2), "12"),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(3),
                content: content(NodeId(3), "あ"),
                span: RubySpan::Auto,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let paragraph_style = ParagraphStyle {
        root: root.clone(),
        ..Default::default()
    };
    let mut outer_content = ParagraphBuilder::new(
        &paragraph_style,
        &Limits {
            max_shaping_run_bytes: Some(1),
            ..Limits::default()
        },
    );
    outer_content.push_ruby(NodeId(4), &root, inner);
    let outer = Ruby::new(
        vec![RubyBase {
            node: NodeId(5),
            content: RubyContent::from_builder(outer_content),
            align: RubyAlign::default(),
        }],
        Vec::new(),
    )
    .unwrap();
    let mut builder = ParagraphBuilder::new(&paragraph_style, &limits);
    builder.push_ruby(NodeId(6), &root, outer);
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let annotations: Vec<_> = paragraph
        .data
        .ruby
        .containers
        .iter()
        .flat_map(|container| &container.lanes)
        .collect();
    assert_eq!(annotations.len(), 1);
    assert!(
        annotations[0].paragraph.warnings().is_empty(),
        "annotation inherited its containing base's one-byte window cap: {:?}",
        annotations[0].paragraph.warnings()
    );
    assert!(
        paragraph.warnings().is_empty(),
        "annotation inherited its containing base's one-byte window cap: {:?}",
        paragraph.warnings()
    );
}
