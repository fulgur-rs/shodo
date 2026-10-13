use shodo::LayoutContext;
use shodo::ParagraphBuilder;
use shodo::font::{FontCollection, FontOptions};
use shodo::mapping::{Affinity, MappingKind, TextOrigin};
use shodo::ruby::{
    Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
    RubyVisibility,
};

fn fonts() -> FontCollection {
    FontCollection::with_options(
        &Limits::default(),
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}
use shodo::limits::{LimitKind, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle, TextTransform};

#[test]
fn expanded_metadata_is_bounded_during_analysis_before_shaping() {
    for (text, transform, language) in [
        ("ß".repeat(64), TextTransform::Uppercase, None),
        ("a".repeat(64), TextTransform::FullWidth, None),
        ("i\u{307}".repeat(64), TextTransform::Uppercase, Some("lt")),
    ] {
        let style = ParagraphStyle {
            root: InlineStyle {
                text_transform: transform,
                lang: language.map(str::to_owned),
                ..Default::default()
            },
            ..Default::default()
        };
        let limits = Limits {
            max_mapping_bytes: Some(512),
            max_shaped_glyphs: Some(0),
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &text,
        );
        let error = builder
            .analyze()
            .err()
            .expect("scalar mapping metadata was unbounded");
        assert_eq!(error.kind, LimitKind::MappingBytes);
        assert_eq!(error.limit, 512);
        assert!(error.actual > error.limit);
    }
}

#[test]
fn disabling_dom_mapping_does_not_bypass_transform_metadata_limit() {
    let style = ParagraphStyle {
        root: InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..Default::default()
        },
        ..Default::default()
    };
    let limits = Limits {
        max_mapping_bytes: Some(128),
        ..Default::default()
    };
    for mapping in [false, true] {
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder
            .with_offset_mapping(mapping)
            .push_text(TextSource::Generated { node: NodeId(1) }, &"ß".repeat(64));
        let error = builder
            .analyze()
            .err()
            .expect("generated/disabled mapping bypassed metadata limit");
        assert_eq!(error.kind, LimitKind::MappingBytes);
    }
}

#[test]
fn whitespace_dom_and_generated_records_have_the_same_admission_boundary() {
    for generated in [false, true] {
        let style = ParagraphStyle::default();
        let limits = Limits {
            max_mapping_bytes: Some(256),
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        for i in 0..32 {
            let source = if generated {
                TextSource::Generated { node: NodeId(i) }
            } else {
                TextSource::Dom {
                    node: NodeId(i),
                    offset: 0,
                }
            };
            builder.push_text(source, "a");
        }
        let error = builder
            .analyze()
            .err()
            .expect("whitespace metadata was unbounded");
        assert_eq!(error.kind, LimitKind::MappingBytes);
        assert_eq!(error.limit, 256);
    }
    let style = ParagraphStyle::default();
    let limits = Limits {
        max_mapping_bytes: Some(512),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &limits);
    for offset in 0..32 {
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset,
            },
            "a",
        );
    }
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert_eq!(paragraph.offset_mapping().unwrap().units().len(), 1);
}

#[test]
fn zero_mapping_budget_accepts_empty_data_and_rejects_the_first_record() {
    let style = ParagraphStyle::default();
    let limits = Limits {
        max_mapping_bytes: Some(0),
        ..Default::default()
    };
    let empty = ParagraphBuilder::new(&style, &limits)
        .build(&mut LayoutContext::new(), &fonts())
        .unwrap();
    assert!(empty.offset_mapping().unwrap().units().is_empty());
    assert_eq!(
        empty.offset_mapping().unwrap().dom_to_text(NodeId(1), 0),
        None
    );
    for mapping in [false, true] {
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.with_offset_mapping(mapping).push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            "a",
        );
        let error = builder.analyze().err().unwrap();
        assert_eq!(error.kind, LimitKind::MappingBytes);
        assert_eq!(error.limit, 0);
        assert!(error.actual > 0);
    }
}

#[test]
fn disabled_budget_preserves_scalar_boundaries_and_both_lazy_query_directions() {
    let style = ParagraphStyle {
        root: InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..Default::default()
        },
        ..Default::default()
    };
    for limits in [
        Limits::default(),
        Limits {
            max_mapping_bytes: None,
            ..Default::default()
        },
        Limits::unlimited(),
    ] {
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 10,
            },
            "ßß",
        );
        let paragraph = builder.build(&mut LayoutContext::new(), &fonts()).unwrap();
        assert_eq!(paragraph.text(), "SSSS");
        let mapping = paragraph.offset_mapping().unwrap();
        assert_eq!(mapping.units().len(), 2);
        assert!(
            mapping
                .units()
                .iter()
                .all(|unit| unit.kind == MappingKind::Expanded)
        );
        for (dom, text) in [(10, 0), (12, 2)] {
            assert_eq!(
                mapping.dom_to_text(NodeId(1), dom + 1),
                Some((text, Affinity::Downstream))
            );
            assert_eq!(
                mapping.text_to_dom(text + 1, Affinity::Downstream),
                Some(TextOrigin::Dom {
                    node: NodeId(1),
                    offset: dom
                })
            );
            let copy = mapping.clone();
            assert_eq!(
                copy.text_to_dom(text + 1, Affinity::Upstream),
                Some(TextOrigin::Dom {
                    node: NodeId(1),
                    offset: dom
                })
            );
            assert_eq!(
                copy.dom_to_text(NodeId(1), dom + 1),
                Some((text, Affinity::Downstream))
            );
        }
    }
}

#[test]
fn first_line_metadata_is_aggregate_for_reuse_and_different_transforms() {
    for different_transform in [false, true] {
        let normal = InlineStyle {
            text_transform: TextTransform::Uppercase,
            ..Default::default()
        };
        let mut first = normal.clone();
        first.paint.color = [255, 0, 0, 255];
        if different_transform {
            first.text_transform = TextTransform::FullWidth;
        }
        let style = ParagraphStyle {
            root: normal.clone(),
            first_line: Some(first),
            ..Default::default()
        };
        let limits = Limits {
            max_mapping_bytes: Some(6000),
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &"ßa".repeat(8),
        );
        let error = builder
            .analyze()
            .err()
            .expect("first-line metadata bypassed aggregate cap");
        assert_eq!(error.kind, LimitKind::MappingBytes);
        assert_eq!(error.limit, 6000);
        assert!(error.actual > 6000);
        let style = ParagraphStyle {
            root: normal,
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: 0,
            },
            &"ßa".repeat(8),
        );
        assert!(
            builder.analyze().is_ok(),
            "one dataset must fit the same cap"
        );
    }
}

fn ruby_with(
    base_text: &str,
    base_style: &InlineStyle,
    base_limits: &Limits,
    readings: usize,
) -> Ruby {
    let annotation_style = InlineStyle {
        text_transform: TextTransform::Uppercase,
        ..Default::default()
    };
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: RubyContent::text(
                TextSource::Dom {
                    node: NodeId(10),
                    offset: 0,
                },
                base_text,
                base_style,
                base_limits,
            ),
            align: RubyAlign::default(),
        }],
        (0..readings)
            .map(|index| RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20 + index as u64),
                    content: RubyContent::text(
                        TextSource::Dom {
                            node: NodeId(20 + index as u64),
                            offset: 0,
                        },
                        &"ßa".repeat(8),
                        &annotation_style,
                        &Limits::unlimited(),
                    ),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn imported_base_limit_covers_remap_splits_without_restricting_unrelated_text() {
    let root = ParagraphStyle::default();
    let expanding = InlineStyle {
        text_transform: TextTransform::Uppercase,
        ..Default::default()
    };
    let local = Limits {
        max_mapping_bytes: Some(1000),
        ..Limits::unlimited()
    };
    let mut builder = ParagraphBuilder::new(&root, &Limits::unlimited());
    builder.push_ruby(
        NodeId(8),
        &root.root,
        ruby_with(&"ß".repeat(8), &expanding, &local, 0),
    );
    let error = builder
        .analyze()
        .err()
        .expect("base remap records escaped its own cap");
    assert_eq!(error.kind, LimitKind::MappingBytes);
    assert_eq!(error.limit, 1000);
    assert!(error.actual > 1000);

    let local = Limits {
        // The imported base also owns generated isolate open/close records.
        max_mapping_bytes: Some(512),
        ..Limits::unlimited()
    };
    let root = ParagraphStyle {
        root: expanding.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&root, &Limits::unlimited());
    builder.push_text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        &"ß".repeat(32),
    );
    builder.push_ruby(NodeId(8), &root.root, ruby_with("a", &expanding, &local, 0));
    let result = builder.analyze();
    assert!(
        result.is_ok(),
        "an unrelated root must not inherit the smallest base cap: {:?}",
        result.err()
    );
}

#[test]
fn ruby_readings_share_the_parent_mapping_admission_budget() {
    let root = ParagraphStyle::default();
    let limits = Limits {
        max_mapping_bytes: Some(6000),
        ..Default::default()
    };
    for readings in [1, 2] {
        let mut builder = ParagraphBuilder::new(&root, &limits);
        builder.push_ruby(
            NodeId(8),
            &root.root,
            ruby_with("a", &root.root, &Limits::unlimited(), readings),
        );
        let result = builder.build(&mut LayoutContext::new(), &fonts());
        if readings == 1 {
            assert!(result.is_ok(), "one reading must fit: {result:?}");
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind, LimitKind::MappingBytes);
            assert_eq!(error.limit, 6000);
            assert!(error.actual > 6000);
        }
    }
}
