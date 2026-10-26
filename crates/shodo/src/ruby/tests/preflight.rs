//! Rejected input must not spend its text budget on normalization comparisons.
use crate::ParagraphBuilder;
use crate::limits::{LimitKind, Limits};
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::ruby::pairing::{COMPARED_BASES, COMPARED_BYTES};
use crate::ruby::*;
use crate::style::{InlineStyle, ParagraphStyle};

fn reset_admission_visits() {
    super::builder::SUMMARY_ITEM_VISITS.with(|n| n.set(0));
    super::pairing::NORMALIZE_CALLS.with(|n| n.set(0));
    super::pairing::OBJECT_ITEM_VISITS.with(|n| n.set(0));
    COMPARED_BYTES.with(|n| n.set(0));
    super::builder::PREPARED_MAPS.with(|n| n.set(0));
}

#[test]
fn snapshot_summary_is_frozen_once_and_excludes_annotation_objects() {
    let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    source.open_inline(
        NodeId(1),
        &InlineStyle {
            font_size: 20.0,
            ..Default::default()
        },
        InlineEdges::default(),
    );
    source.push_text(TextSource::Generated { node: NodeId(2) }, "a");
    reset_admission_visits();
    let snapshot = RubyContent::from_builder(source);
    assert_eq!(super::builder::SUMMARY_ITEM_VISITS.with(|n| n.get()), 3);
    let cost = super::builder::InputCost::content(&snapshot.0);
    assert_eq!(
        (
            cost.text,
            cost.items,
            cost.styles,
            cost.style_bytes,
            cost.depth
        ),
        (1, 3, 2, 1224, 1)
    );
    assert!(!snapshot.0.holds_object);
    reset_admission_visits();
    for _ in 0..128 {
        assert_eq!(
            super::builder::InputCost::content(&snapshot.clone().0),
            cost
        );
    }
    assert_no_admission_payload_work();

    let mut object = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    object.push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default());
    let object = RubyContent::from_builder(object);
    assert!(object.0.holds_object);
    let mut nested = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    nested.push_ruby(
        NodeId(4),
        &InlineStyle::default(),
        ruby(content("a"), object, 1),
    );
    let nested = RubyContent::from_builder(nested);
    assert!(
        !nested.0.holds_object,
        "an annotation object is not in the base source stream"
    );
    let cost = nested.0.cost;
    assert_eq!(
        (
            cost.text,
            cost.items,
            cost.styles,
            cost.style_bytes,
            cost.depth
        ),
        (1, 14, 3, 2048, 2)
    );
    assert_eq!(super::builder::InputCost::measure(&nested.0).0, cost);
}

#[test]
fn prepared_alias_maps_share_storage_and_intern_only_once_per_operation() {
    for (first_line, exact) in [(false, 475), (true, 696)] {
        for limit in [exact - 1, exact] {
            let shared = content("");
            let input = Ruby::new(
                vec![
                    RubyBase {
                        node: NodeId(1),
                        content: shared,
                        align: RubyAlign::default()
                    };
                    2
                ],
                vec![],
            )
            .unwrap();
            let limits = Limits {
                max_style_work: Some(limit),
                ..Default::default()
            };
            let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            reset_admission_visits();
            if first_line {
                parent.push_ruby_with_first_line(
                    NodeId(2),
                    &InlineStyle::default(),
                    &InlineStyle {
                        font_size: 18.0,
                        ..Default::default()
                    },
                    input,
                );
            } else {
                parent.push_ruby(NodeId(2), &InlineStyle::default(), input);
            }
            if limit == exact {
                assert_eq!(parent.error(), None, "{first_line}/{limit}");
                assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 1);
                assert_eq!(super::builder::PREPARED_MAPS.with(|n| n.get()), 1);
            } else {
                assert_eq!(
                    parent.error().unwrap().kind,
                    LimitKind::StyleWork,
                    "{first_line}/{limit}"
                );
                assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 0);
                assert!(parent.stack.is_empty() && parent.items.is_empty());
            }
            assert_eq!(super::builder::SUMMARY_ITEM_VISITS.with(|n| n.get()), 0);
            assert_eq!(super::pairing::OBJECT_ITEM_VISITS.with(|n| n.get()), 0);
        }
    }
}

#[test]
fn projected_raw_items_exact_boundary_and_multiple_limit_priority() {
    for cap in [Some(14), Some(15), None] {
        let input = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(1),
                    content: content(""),
                    align: RubyAlign::default()
                };
                2
            ],
            vec![],
        )
        .unwrap();
        let limits = Limits {
            max_items: cap,
            max_styles: Some(2),
            ..Default::default()
        };
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        reset_admission_visits();
        parent.push_ruby(NodeId(2), &InlineStyle::default(), input);
        if cap == Some(14) {
            let error = parent.error().unwrap();
            assert_eq!(
                (error.kind, error.limit, error.actual),
                (LimitKind::Items, 14, 15)
            );
            assert_no_admission_payload_work();
        } else {
            assert_eq!(parent.error(), None);
            assert_eq!(parent.items.len() as u64 + parent.ruby_cost.items, 15);
        }
    }
    // Earlier item admission intentionally precedes competing style failures.
    let limits = Limits {
        max_items: Some(4),
        max_styles: Some(1),
        ..Default::default()
    };
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    parent.push_ruby(
        NodeId(1),
        &InlineStyle::default(),
        Ruby::new(vec![], vec![]).unwrap(),
    );
    assert_eq!(parent.error().unwrap().kind, LimitKind::Items);
}

fn assert_no_admission_payload_work() {
    assert_eq!(super::builder::SUMMARY_ITEM_VISITS.with(|n| n.get()), 0);
    assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 0);
    assert_eq!(super::pairing::OBJECT_ITEM_VISITS.with(|n| n.get()), 0);
    assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
}

#[test]
fn aliased_payload_is_admitted_before_summary_walks_or_normalization() {
    let style = InlineStyle::default();
    let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    for _ in 0..256 {
        source
            .open_inline(NodeId(1), &style, InlineEdges::default())
            .close_inline();
    }
    let shared = RubyContent::from_builder(source);
    for base_aliases in [false, true] {
        let input = if base_aliases {
            Ruby::new(
                vec![
                    RubyBase {
                        node: NodeId(1),
                        content: shared.clone(),
                        align: RubyAlign::default()
                    };
                    16
                ],
                vec![],
            )
            .unwrap()
        } else {
            ruby(content(""), shared.clone(), 16)
        };
        let limits = Limits {
            max_items: Some(1024),
            ..Default::default()
        };
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        reset_admission_visits();
        parent.push_ruby(NodeId(2), &style, input);
        assert_eq!(parent.error().unwrap().kind, LimitKind::Items);
        assert_no_admission_payload_work();
        assert!(parent.items.is_empty() && parent.rubies.is_empty());
    }
}

#[test]
fn retained_style_and_depth_limits_precede_normalization() {
    let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
    source.open_inline(
        NodeId(1),
        &InlineStyle {
            font_size: 20.0,
            ..Default::default()
        },
        InlineEdges::default(),
    );
    let shared = RubyContent::from_builder(source);
    for (limits, kind) in [
        (
            Limits {
                max_styles: Some(8),
                ..Default::default()
            },
            LimitKind::Styles,
        ),
        (
            Limits {
                max_nesting_depth: Some(2),
                ..Default::default()
            },
            LimitKind::NestingDepth,
        ),
    ] {
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        let input = ruby(content(""), shared.clone(), 16);
        reset_admission_visits();
        parent.push_ruby(NodeId(2), &InlineStyle::default(), input);
        assert_eq!(parent.error().unwrap().kind, kind);
        assert_no_admission_payload_work();
    }
}

#[test]
fn imported_style_admission_preserves_dedup_before_normalization() {
    for font_size in [16.0, 20.0] {
        let base = RubyContent::text(
            TextSource::Generated { node: NodeId(1) },
            "",
            &InlineStyle {
                font_size,
                ..Default::default()
            },
            &Limits::unlimited(),
        );
        let input = Ruby::new(
            vec![RubyBase {
                node: NodeId(1),
                content: base,
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let limits = Limits {
            max_styles: Some(2),
            ..Default::default()
        };
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        reset_admission_visits();
        parent.push_ruby(NodeId(2), &InlineStyle::default(), input);
        if font_size == 20.0 {
            assert_eq!(parent.error().unwrap().kind, LimitKind::Styles);
            assert_no_admission_payload_work();
        } else {
            assert_eq!(parent.error(), None);
            assert_eq!(parent.styles.len(), 2);
        }
    }
}

#[test]
fn scope_index_bytes_and_base_own_caps_are_checked_before_normalization() {
    for cap in [1255, 1256] {
        let input = Ruby::new(
            vec![RubyBase {
                node: NodeId(1),
                content: content(""),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let limits = Limits {
            max_style_bytes: Some(cap),
            ..Default::default()
        };
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        reset_admission_visits();
        parent.push_ruby(NodeId(2), &InlineStyle::default(), input);
        if cap == 1255 {
            let error = parent.error().unwrap();
            assert_eq!((error.kind, error.actual), (LimitKind::StyleBytes, 1256));
            assert_no_admission_payload_work();
        } else {
            assert_eq!(parent.error(), None);
        }
    }
    for (limits, kind) in [
        (
            Limits {
                max_items: Some(4),
                ..Default::default()
            },
            LimitKind::Items,
        ),
        (
            Limits {
                max_styles: Some(1),
                ..Default::default()
            },
            LimitKind::Styles,
        ),
        (
            Limits {
                max_nesting_depth: Some(1),
                ..Default::default()
            },
            LimitKind::NestingDepth,
        ),
    ] {
        let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        source
            .open_inline(NodeId(1), &InlineStyle::default(), InlineEdges::default())
            .close_inline();
        source
            .push_forced_break(NodeId(2))
            .push_forced_break(NodeId(3));
        assert_eq!(source.error(), None);
        let input = Ruby::new(
            vec![RubyBase {
                node: NodeId(4),
                content: RubyContent::from_builder(source),
                align: RubyAlign::default(),
            }],
            vec![],
        )
        .unwrap();
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        reset_admission_visits();
        parent.push_ruby(NodeId(5), &InlineStyle::default(), input);
        assert_eq!(parent.error().unwrap().kind, kind);
        assert_no_admission_payload_work();
    }
}

#[test]
fn explicit_first_line_import_never_copies_discarded_inherited_payload() {
    let paragraph = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("x".repeat(4096))],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut source = ParagraphBuilder::new(&paragraph, &Limits::default());
    for index in 0..8 {
        source
            .open_inline_with_first_line(
                NodeId(index),
                &InlineStyle {
                    font_size: 20.0 + index as f32,
                    ..Default::default()
                },
                &InlineStyle::default(),
                InlineEdges::default(),
            )
            .close_inline();
    }
    let shared = RubyContent::from_builder(source);
    let input = Ruby::new(
        vec![
            RubyBase {
                node: NodeId(10),
                content: shared,
                align: RubyAlign::default()
            };
            4
        ],
        vec![],
    )
    .unwrap();
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    super::builder::INHERITED_STYLE_COPIES.with(|n| n.set(0));
    parent.push_ruby(NodeId(11), &InlineStyle::default(), input);
    assert_eq!(parent.error(), None);
    // Only the root lacks an explicit alternative, once per base. The eight
    // explicit alternatives must never clone the unused 4-KiB inherited family.
    assert_eq!(super::builder::INHERITED_STYLE_COPIES.with(|n| n.get()), 4);
}

#[test]
fn wrapper_prediction_bounds_repeated_legacy_style_payload_before_copying() {
    let root = InlineStyle {
        unicode_bidi: crate::style::UnicodeBidi::Isolate,
        font_variations: vec![
            crate::style::FontVariation {
                tag: *b"wght",
                value: 0.0
            };
            6
        ],
        ..Default::default()
    };
    let paragraph = ParagraphStyle {
        root: root.clone(),
        first_line: Some(InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("x".repeat(4096))],
            word_break: crate::style::WordBreak::BreakAll,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut source = ParagraphBuilder::new(&paragraph, &Limits::default());
    for mask in 1..16 {
        let mut normal = root.clone();
        for (bit, axis) in normal.font_variations.iter_mut().enumerate() {
            axis.value = if mask & (1 << bit) == 0 { 0.0 } else { -0.0 };
        }
        source
            .open_inline(NodeId(mask), &normal, InlineEdges::default())
            .close_inline();
    }
    assert_eq!(source.error(), None);
    let input = Ruby::new(
        vec![RubyBase {
            node: NodeId(20),
            content: RubyContent::from_builder(source),
            align: RubyAlign::default(),
        }],
        vec![],
    )
    .unwrap();
    let limits = Limits {
        max_style_work: Some(10_000),
        ..Default::default()
    };
    let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    super::builder::WRAPPER_INHERITED_COPIES.with(|n| n.set(0));
    reset_admission_visits();
    parent.push_ruby(NodeId(21), &InlineStyle::default(), input);
    assert_eq!(parent.error().unwrap().kind, LimitKind::StyleWork);
    assert!(super::builder::WRAPPER_INHERITED_COPIES.with(|n| n.get()) <= 2);
    assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 0);
}

fn content(text: &str) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(1),
            offset: 0,
        },
        text,
        &InlineStyle::default(),
        &Limits::default(),
    )
}

fn ruby(base: RubyContent, reading: RubyContent, count: usize) -> Ruby {
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base,
            align: RubyAlign::default(),
        }],
        (0..count)
            .map(|i| RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20 + i as u64),
                    content: reading.clone(),
                    span: RubySpan::All,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn oversized_shared_readings_are_rejected_before_comparing_any_text() {
    let text = "a".repeat(4096);
    let base = content(&text);
    let different = content(&format!("{}b", &text[..4095]));
    for reading in [base.clone(), different] {
        for visibility in [
            RubyVisibility::Visible,
            RubyVisibility::Hidden,
            RubyVisibility::Collapse,
        ] {
            for merge in [RubyMerge::Separate, RubyMerge::Auto, RubyMerge::Merge] {
                for first_line in [false, true] {
                    let mut ruby = ruby(base.clone(), reading.clone(), 32);
                    for level in &mut ruby.levels {
                        level.style.merge = merge;
                        level.annotations[0].visibility = visibility;
                    }
                    let limits = Limits {
                        max_text_bytes: Some(8192),
                        ..Default::default()
                    };
                    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
                    b.push_text(
                        TextSource::Dom {
                            node: NodeId(2),
                            offset: 0,
                        },
                        "prefix",
                    );
                    let before = (b.text.clone(), b.items.len(), b.styles.len());
                    COMPARED_BYTES.with(|n| n.set(0));
                    if first_line {
                        b.push_ruby_with_first_line(
                            NodeId(8),
                            &InlineStyle::default(),
                            &InlineStyle::default(),
                            ruby,
                        );
                    } else {
                        b.push_ruby(NodeId(8), &InlineStyle::default(), ruby);
                    }
                    let error = b.error.expect("aggregate input exceeds the parent budget");
                    assert_eq!(
                        (error.kind, error.limit, error.actual),
                        (LimitKind::TextBytes, 8192, 6 + 33 * 4096)
                    );
                    assert_eq!(
                        COMPARED_BYTES.with(|n| n.get()),
                        0,
                        "{visibility:?}/{merge:?}/first_line={first_line}"
                    );
                    assert_eq!((b.text.clone(), b.items.len(), b.styles.len()), before);
                    assert!(b.rubies.is_empty());
                    assert_eq!(RubyContent::from_builder(b).0.error, Some(error));
                }
            }
        }
    }
}

#[test]
fn existing_annotation_text_is_counted_before_the_next_ruby_comparison() {
    let limits = Limits {
        max_text_bytes: Some(100),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_ruby(
        NodeId(8),
        &InlineStyle::default(),
        ruby(content("a"), content(&"b".repeat(80)), 1),
    );
    assert!(b.error.is_none());
    let before = (b.text.clone(), b.items.len(), b.rubies.len());
    COMPARED_BYTES.with(|n| n.set(0));
    b.push_ruby(
        NodeId(9),
        &InlineStyle::default(),
        ruby(content("abcdefghij"), content("abcdefghij"), 1),
    );
    let error = b.error.unwrap();
    assert_eq!(
        (error.kind, error.limit, error.actual),
        (LimitKind::TextBytes, 100, 101)
    );
    assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
    assert_eq!((b.text.clone(), b.items.len(), b.rubies.len()), before);
}

#[test]
fn nested_readings_are_counted_in_both_base_and_annotation_snapshots() {
    let mut nested = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    nested.push_ruby(
        NodeId(8),
        &InlineStyle::default(),
        ruby(content("a"), content(&"b".repeat(100)), 1),
    );
    let nested = RubyContent::from_builder(nested);
    for (base, reading) in [(nested.clone(), content("a")), (content("a"), nested)] {
        let limits = Limits {
            max_text_bytes: Some(50),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        COMPARED_BYTES.with(|n| n.set(0));
        b.push_ruby(NodeId(9), &InlineStyle::default(), ruby(base, reading, 1));
        let error = b.error.unwrap();
        assert_eq!(
            (error.kind, error.limit, error.actual),
            (LimitKind::TextBytes, 50, 102)
        );
        assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
        assert!(b.text.is_empty() && b.rubies.is_empty());
    }
}

#[test]
fn exact_source_budget_preserves_original_text_comparison_and_hiding() {
    for cap in [Some(12), None] {
        let limits = Limits {
            max_text_bytes: cap,
            ..Default::default()
        };
        let input = Ruby::new(
            vec![
                RubyBase {
                    node: NodeId(10),
                    content: content("ab"),
                    align: RubyAlign::default(),
                },
                RubyBase {
                    node: NodeId(11),
                    content: content("cd"),
                    align: RubyAlign::default(),
                },
            ],
            vec![
                RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(20),
                        content: content("abcd"),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                },
                RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(21),
                        content: content("abce"),
                        span: RubySpan::All,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                },
            ],
        )
        .unwrap();
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        COMPARED_BYTES.with(|n| n.set(0));
        b.push_ruby(NodeId(8), &InlineStyle::default(), input);
        assert!(b.error.is_none());
        assert_eq!(COMPARED_BYTES.with(|n| n.get()), 8);
        let normalized = &b.rubies[0].normalized;
        assert!(normalized.levels[0].annotations[0].auto_hidden);
        assert!(!normalized.levels[1].annotations[0].auto_hidden);
        assert_eq!(b.text, "abcd");
    }
}

#[test]
fn empty_base_columns_are_not_rescanned_for_every_level() {
    for sparse_text in [false, true] {
        let empty = content("");
        let bases = (0..64)
            .map(|i| RubyBase {
                node: NodeId(100 + i),
                content: match (sparse_text, i) {
                    (true, 9) => content("a"),
                    (true, 54) => content("b"),
                    _ => empty.clone(),
                },
                align: RubyAlign::default(),
            })
            .collect();
        let mut input = ruby(
            empty.clone(),
            content(if sparse_text { "ab" } else { "" }),
            32,
        );
        input = Ruby::new(bases, input.levels).unwrap();
        let limits = Limits {
            max_text_bytes: Some(if sparse_text { 66 } else { 0 }),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        COMPARED_BASES.with(|n| n.set(0));
        b.push_ruby(NodeId(8), &InlineStyle::default(), input);
        assert!(b.error.is_none());
        assert!(
            b.rubies[0]
                .normalized
                .levels
                .iter()
                .all(|l| l.annotations[0].auto_hidden)
        );
        assert_eq!(
            COMPARED_BASES.with(|n| n.get()),
            if sparse_text { 64 } else { 0 }
        );
    }
}
