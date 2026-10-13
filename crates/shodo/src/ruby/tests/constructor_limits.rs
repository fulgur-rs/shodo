use super::input::{SPAN_TABLES, VALIDATED_ANNOTATIONS};
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::node::NodeId;
use crate::ruby::*;
use crate::style::ParagraphStyle;

fn empty() -> RubyContent {
    RubyContent::from_builder(crate::ParagraphBuilder::new(
        &ParagraphStyle::default(),
        &Limits::default(),
    ))
}

fn base(content: &RubyContent) -> RubyBase {
    RubyBase {
        node: NodeId(1),
        content: content.clone(),
        align: RubyAlign::default(),
    }
}

fn annotation(content: &RubyContent, span: RubySpan) -> RubyAnnotation {
    RubyAnnotation {
        node: NodeId(2),
        content: content.clone(),
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

fn limits(max_items: u64) -> Limits {
    Limits {
        max_items: Some(max_items),
        ..Default::default()
    }
}

fn rejected_before_validation(
    bases: Vec<RubyBase>,
    levels: Vec<RubyLevel>,
    limit: u64,
    actual: u64,
) {
    VALIDATED_ANNOTATIONS.with(|count| count.set(0));
    SPAN_TABLES.with(|count| count.set(0));
    let Err(error) = Ruby::with_limits(bases, levels, &limits(limit)) else {
        panic!("oversized constructor input must fail");
    };
    assert_eq!(
        error,
        RubyError::Limit(LimitExceeded {
            kind: LimitKind::Items,
            limit,
            actual,
        })
    );
    assert_eq!(VALIDATED_ANNOTATIONS.with(|count| count.get()), 0);
    assert_eq!(SPAN_TABLES.with(|count| count.get()), 0);
}

#[test]
fn oversized_annotation_table_is_rejected_before_element_walk_and_sort() {
    let content = empty();
    rejected_before_validation(
        vec![],
        vec![level(vec![annotation(&content, RubySpan::Auto); 8])],
        8,
        10,
    );
}

#[test]
fn base_and_level_counts_are_checked_before_the_level_walk() {
    let content = empty();
    rejected_before_validation(vec![base(&content); 8], vec![], 8, 9);
    // The malformed annotation must not be visited when the outer lengths fail.
    rejected_before_validation(
        vec![],
        vec![
            level(vec![annotation(
                &content,
                RubySpan::Columns(std::ops::Range { start: 3, end: 2 })
            )]);
            8
        ],
        8,
        9,
    );
}

#[test]
fn annotation_lengths_share_one_container_budget_across_levels() {
    let content = empty();
    rejected_before_validation(
        vec![],
        vec![level(vec![annotation(&content, RubySpan::Auto); 3]); 2],
        8,
        9,
    );
    // A malformed early level cannot enter validation before a later level's
    // annotation table has been admitted.
    rejected_before_validation(
        vec![],
        vec![
            level(vec![annotation(&content, RubySpan::Columns(0..0))]),
            level(vec![annotation(&content, RubySpan::Auto); 6]),
        ],
        8,
        10,
    );
}

#[test]
fn exact_limit_one_over_disabled_and_unlimited() {
    let content = empty();
    let levels = vec![level(vec![annotation(&content, RubySpan::Auto); 3])];
    let ruby = Ruby::with_limits(vec![], levels.clone(), &limits(5)).unwrap();
    assert_eq!(ruby.columns, 3);
    rejected_before_validation(vec![], levels.clone(), 4, 5);
    for budget in [
        Limits {
            max_items: None,
            ..limits(0)
        },
        Limits::unlimited(),
    ] {
        assert_eq!(
            Ruby::with_limits(vec![], levels.clone(), &budget)
                .unwrap()
                .columns,
            3
        );
    }
    assert_eq!(Ruby::new(vec![], levels).unwrap().columns, 3);
}

#[test]
fn empty_container_still_counts_one_item() {
    assert_eq!(
        Ruby::with_limits(vec![], vec![], &limits(1))
            .unwrap()
            .columns,
        0
    );
    rejected_before_validation(vec![], vec![], 0, 1);
    assert!(Ruby::new(vec![], vec![]).is_ok());
}

#[test]
fn visibility_merge_and_shared_content_do_not_discount_records() {
    let content = empty();
    for visibility in [
        RubyVisibility::Visible,
        RubyVisibility::Hidden,
        RubyVisibility::Collapse,
    ] {
        for merge in [RubyMerge::Separate, RubyMerge::Merge, RubyMerge::Auto] {
            let mut annotations = vec![annotation(&content, RubySpan::Auto); 4];
            for item in &mut annotations {
                item.visibility = visibility;
            }
            rejected_before_validation(
                vec![base(&content); 2],
                vec![RubyLevel {
                    annotations,
                    style: RubyStyle {
                        merge,
                        ..Default::default()
                    },
                }],
                7,
                8,
            );
        }
    }
}

#[test]
fn structural_error_precedence_and_original_annotation_indices_are_preserved() {
    let content = empty();
    let invalid = level(vec![annotation(&content, RubySpan::Columns(1..1))]);
    rejected_before_validation(vec![base(&content)], vec![invalid.clone()], 3, 4);
    assert_eq!(
        Ruby::with_limits(vec![base(&content)], vec![invalid], &limits(4)).unwrap_err(),
        RubyError::InvalidSpan {
            level: 0,
            annotation: 0,
            span: 1..1,
            columns: 1
        }
    );
    let overlap = level(vec![
        annotation(&content, RubySpan::Columns(1..3)),
        annotation(&content, RubySpan::Columns(0..2)),
    ]);
    assert_eq!(
        Ruby::with_limits(vec![base(&content); 3], vec![overlap], &limits(7)).unwrap_err(),
        RubyError::OverlappingSpans {
            level: 0,
            first: 1,
            second: 0
        }
    );
}

#[test]
fn unsorted_explicit_spans_auto_and_cross_level_padding_keep_their_order() {
    let content = empty();
    let levels = vec![
        level(vec![
            annotation(&content, RubySpan::Columns(1..2)),
            annotation(&content, RubySpan::Columns(0..1)),
        ]),
        level(vec![annotation(&content, RubySpan::Auto); 2]),
        level(vec![annotation(&content, RubySpan::All)]),
    ];
    let ruby = Ruby::with_limits(vec![base(&content)], levels, &limits(10)).unwrap();
    assert_eq!(ruby.columns, 2);
    assert_eq!(ruby.levels[0].annotations[0].span, RubySpan::Columns(1..2));
    assert_eq!(ruby.levels[0].annotations[1].span, RubySpan::Columns(0..1));
    assert_eq!(ruby.levels[1].annotations[1].span, RubySpan::Auto);
    assert_eq!(ruby.levels[2].annotations[0].span, RubySpan::All);
    let normalized = super::pairing::normalize(&ruby, &limits(11)).unwrap();
    assert_eq!(normalized.bases.len(), 2);
    assert_eq!(normalized.levels[0].annotations[0].columns, 0..1);
    assert_eq!(normalized.levels[0].annotations[1].columns, 1..2);
}

#[test]
fn normalization_and_cloned_parent_inputs_still_enforce_projected_aggregate_limits() {
    let content = empty();
    let ruby = Ruby::with_limits(
        vec![],
        vec![level(vec![annotation(&content, RubySpan::Auto); 3])],
        &limits(5),
    )
    .unwrap();
    let error = super::pairing::normalize(&ruby, &limits(5)).unwrap_err();
    assert_eq!(
        (error.kind, error.limit, error.actual),
        (LimitKind::Items, 5, 8)
    );
    let mut builder = crate::ParagraphBuilder::new(&ParagraphStyle::default(), &limits(25));
    let style = crate::style::InlineStyle::default();
    builder.push_ruby(NodeId(3), &style, ruby.clone());
    assert!(builder.error().is_none());
    assert_eq!(builder.items.len(), 10);
    assert_eq!(builder.ruby_cost.items, 8);
    builder.push_ruby(NodeId(4), &style, ruby);
    let error = builder.error().unwrap();
    assert_eq!(
        (error.kind, error.limit, error.actual),
        (LimitKind::Items, 25, 26)
    );
}

#[test]
fn default_constructor_applies_the_default_budget_before_validation() {
    let content = empty();
    let count = 1 << 20;
    VALIDATED_ANNOTATIONS.with(|count| count.set(0));
    SPAN_TABLES.with(|count| count.set(0));
    let Err(error) = Ruby::new(
        vec![],
        vec![level(vec![annotation(&content, RubySpan::Auto); count])],
    ) else {
        panic!("default constructor must enforce the default item cap");
    };
    assert_eq!(
        error,
        RubyError::Limit(LimitExceeded {
            kind: LimitKind::Items,
            limit: 1 << 20,
            actual: (1 << 20) + 2
        })
    );
    assert_eq!(VALIDATED_ANNOTATIONS.with(|count| count.get()), 0);
    assert_eq!(SPAN_TABLES.with(|count| count.get()), 0);
}

#[test]
fn limit_error_conversion_and_display_preserve_limit_fields() {
    let error = LimitExceeded {
        kind: LimitKind::Items,
        limit: 4,
        actual: 5,
    };
    assert_eq!(RubyError::from(error), RubyError::Limit(error));
    assert_eq!(RubyError::Limit(error).to_string(), error.to_string());
}

#[test]
fn explicit_raised_limit_admits_larger_trusted_input() {
    let content = empty();
    let count = 1 << 20;
    let ruby = Ruby::with_limits(
        vec![],
        vec![level(vec![annotation(&content, RubySpan::Auto); count])],
        &limits((1 << 20) + 2),
    )
    .unwrap();
    assert_eq!(ruby.columns, count);
    assert_eq!(ruby.levels[0].annotations.len(), count);
}
