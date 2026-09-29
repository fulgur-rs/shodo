//! These tests catch wrong pairing, post-transform hiding and expansion before
//! the projected allocation guard. Expected ranges and visibility are literal.
use crate::limits::{LimitKind, Limits};
use crate::node::{NodeId, TextSource};
use crate::ruby::pairing::normalize;
use crate::ruby::*;
use crate::style::{InlineStyle, TextTransform};

fn content(node: u64, text: &str, transform: TextTransform) -> RubyContent {
    RubyContent::text(
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        },
        text,
        &InlineStyle {
            text_transform: transform,
            ..Default::default()
        },
        &Limits::default(),
    )
}

fn base(node: u64, text: &str) -> RubyBase {
    RubyBase {
        node: NodeId(node),
        content: content(node, text, TextTransform::None),
        align: RubyAlign::default(),
    }
}

fn annotation(node: u64, text: &str, span: RubySpan) -> RubyAnnotation {
    RubyAnnotation {
        node: NodeId(node),
        content: content(node, text, TextTransform::None),
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

#[test]
fn anonymous_padding_has_no_fabricated_sources() {
    let r = Ruby::new(
        vec![base(1, "日")],
        vec![level(vec![
            annotation(2, "に", RubySpan::Auto),
            annotation(3, "ほん", RubySpan::Auto),
        ])],
    )
    .unwrap();
    let n = normalize(&r, &Limits::default()).unwrap();
    assert_eq!(n.bases.len(), 2);
    assert_eq!(n.bases[0].node, Some(NodeId(1)));
    assert_eq!(n.bases[1].node, None);
    assert!(n.bases[1].content.is_none());
    assert_eq!(n.levels[0].annotations[0].columns, 0..1);
    assert_eq!(n.levels[0].annotations[1].columns, 1..2);
}

#[test]
fn missing_annotation_padding_and_all_span_keep_columns() {
    let r = Ruby::new(
        vec![base(1, "日"), base(2, "本"), base(3, "語")],
        vec![
            level(vec![annotation(4, "に", RubySpan::Auto)]),
            level(vec![annotation(5, "にほんご", RubySpan::All)]),
            level(vec![
                annotation(6, "に", RubySpan::Columns(0..1)),
                annotation(7, "ほんご", RubySpan::Columns(1..3)),
            ]),
        ],
    )
    .unwrap();
    let n = normalize(&r, &Limits::default()).unwrap();
    assert_eq!(n.levels[0].annotations.len(), 3);
    assert_eq!(n.levels[0].annotations[1].columns, 1..2);
    assert_eq!(n.levels[0].annotations[1].node, None);
    assert!(n.levels[0].annotations[1].content.is_none());
    assert_eq!(n.levels[1].annotations.len(), 1);
    assert_eq!(n.levels[1].annotations[0].columns, 0..3);
    assert_eq!(n.levels[2].annotations[1].columns, 1..3);
}

#[test]
fn ruby_hiding_uses_original_text_not_transforms() {
    for (base_text, annotation_text, merge, hidden) in [
        ("ab", "ab", RubyMerge::Separate, true),
        ("ab", "ab", RubyMerge::Auto, true),
        ("AB", "ab", RubyMerge::Separate, false),
        ("ab", "ab", RubyMerge::Merge, false),
        ("a b", "a  b", RubyMerge::Separate, false),
    ] {
        let mut a = annotation(2, annotation_text, RubySpan::Auto);
        a.content = content(2, annotation_text, TextTransform::Uppercase);
        let r = Ruby::new(
            vec![base(1, base_text)],
            vec![RubyLevel {
                annotations: vec![a],
                style: RubyStyle {
                    merge,
                    ..Default::default()
                },
            }],
        )
        .unwrap();
        let n = normalize(&r, &Limits::default()).unwrap();
        assert_eq!(
            n.levels[0].annotations[0].auto_hidden, hidden,
            "{base_text:?}/{annotation_text:?}/{merge:?}"
        );
    }
}

#[test]
fn hidden_and_collapsed_annotations_preserve_pairing() {
    let mut hidden = annotation(4, "に", RubySpan::Auto);
    hidden.visibility = RubyVisibility::Hidden;
    let mut collapsed = annotation(5, "ほん", RubySpan::Auto);
    collapsed.visibility = RubyVisibility::Collapse;
    let r = Ruby::new(
        vec![base(1, "日"), base(2, "本")],
        vec![level(vec![hidden, collapsed])],
    )
    .unwrap();
    let n = normalize(&r, &Limits::default()).unwrap();
    assert_eq!(n.levels[0].annotations[0].columns, 0..1);
    assert_eq!(
        n.levels[0].annotations[0].visibility,
        RubyVisibility::Hidden
    );
    assert_eq!(n.levels[0].annotations[1].columns, 1..2);
    assert_eq!(
        n.levels[0].annotations[1].visibility,
        RubyVisibility::Collapse
    );
    assert!(n.levels[0].annotations[1].content.is_some());
}

#[test]
fn anonymous_columns_are_bounded_before_expansion() {
    let r = Ruby::new(
        vec![],
        vec![level(
            (0..32).map(|i| annotation(i, "", RubySpan::Auto)).collect(),
        )],
    )
    .unwrap();
    let error = normalize(
        &r,
        &Limits {
            max_items: Some(8),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, LimitKind::Items);
    assert_eq!(error.limit, 8);
    assert!(error.actual >= 32);
}

#[test]
fn explicit_ranges_can_use_auto_padded_columns_on_another_level() {
    let r = Ruby::new(
        vec![base(1, "日")],
        vec![
            level(vec![
                annotation(2, "に", RubySpan::Auto),
                annotation(3, "ほん", RubySpan::Auto),
            ]),
            level(vec![annotation(4, "よみ", RubySpan::Columns(0..2))]),
        ],
    )
    .unwrap();
    let n = normalize(&r, &Limits::default()).unwrap();
    assert_eq!(n.bases.len(), 2);
    assert_eq!(n.levels[1].annotations[0].columns, 0..2);
    assert!(
        Ruby::new(
            vec![base(1, "日")],
            vec![level(vec![annotation(4, "よみ", RubySpan::Columns(0..2))])]
        )
        .is_err()
    );
}
