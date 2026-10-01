//! Rejected input must not spend its text budget on normalization comparisons.
use crate::ParagraphBuilder;
use crate::limits::{LimitKind, Limits};
use crate::node::{NodeId, TextSource};
use crate::ruby::pairing::{COMPARED_BASES, COMPARED_BYTES};
use crate::ruby::*;
use crate::style::{InlineStyle, ParagraphStyle};

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
