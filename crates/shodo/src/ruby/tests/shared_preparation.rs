use super::builder::{INHERITED_STYLE_COPIES, PREPARED_MAPS};
use super::pairing::{COMPARED_BASES, COMPARED_BYTES, normalize};
use crate::ParagraphBuilder;
use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::ruby::*;
use crate::style::{FontFamily, InlineStyle, ParagraphStyle};

fn snapshot(id: usize) -> RubyContent {
    let paragraph = ParagraphStyle {
        first_line: Some(InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(1024))],
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut source = ParagraphBuilder::new(&paragraph, &Limits::unlimited());
    for i in 0..3 {
        source.open_inline(
            NodeId(i),
            &InlineStyle {
                font_size: 20.0 + (id * 3) as f32 + i as f32,
                ..Default::default()
            },
            InlineEdges::default(),
        );
        source.push_text(TextSource::Generated { node: NodeId(i) }, "ab");
        source.close_inline();
    }
    RubyContent::from_builder(source)
}

fn bases(contents: impl IntoIterator<Item = RubyContent>) -> Ruby {
    Ruby::new(
        contents
            .into_iter()
            .enumerate()
            .map(|(i, content)| RubyBase {
                node: NodeId(i as u64 + 100),
                content,
                align: RubyAlign::default(),
            })
            .collect(),
        vec![],
    )
    .unwrap()
}

#[test]
fn inherited_preparation_scales_with_unique_snapshots_not_aliases() {
    for unique in [1, 4] {
        let snapshots: Vec<_> = (0..unique).map(snapshot).collect();
        for copies in [1, 8, 64] {
            let input = bases((0..unique * copies).map(|i| snapshots[i % unique].clone()));
            let mut parent =
                ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
            INHERITED_STYLE_COPIES.with(|n| n.set(0));
            PREPARED_MAPS.with(|n| n.set(0));
            parent.push_ruby(NodeId(1000), &InlineStyle::default(), input);
            assert_eq!(parent.error(), None);
            assert_eq!(
                INHERITED_STYLE_COPIES.with(|n| n.get()),
                (unique * 4) as u64,
                "{unique}/{copies}"
            );
            assert_eq!(PREPARED_MAPS.with(|n| n.get()), unique as u64);
        }
    }
}

fn equality_input(shared: bool, object: bool, merge: RubyMerge) -> Ruby {
    let make = || {
        let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
        source.push_text(
            TextSource::Generated { node: NodeId(1) },
            &"ab".repeat(2048),
        );
        if object {
            source.push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default());
        }
        RubyContent::from_builder(source)
    };
    let content = make();
    let annotation = if shared { content.clone() } else { make() };
    let mut input = bases([content]);
    input.levels = vec![RubyLevel {
        style: RubyStyle {
            merge,
            ..Default::default()
        },
        annotations: vec![RubyAnnotation {
            node: NodeId(3),
            content: annotation,
            span: RubySpan::All,
            visibility: RubyVisibility::Visible,
        }],
    }];
    input
}

#[test]
fn merge_never_compares_text_that_cannot_hide_annotations() {
    let input = equality_input(false, false, RubyMerge::Merge);
    COMPARED_BYTES.with(|n| n.set(0));
    COMPARED_BASES.with(|n| n.set(0));
    let result = normalize(&input, &Limits::unlimited()).unwrap();
    assert!(!result.levels[0].annotations[0].auto_hidden);
    assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
    assert_eq!(COMPARED_BASES.with(|n| n.get()), 0);
}

#[test]
fn single_column_identity_avoids_text_scans_but_objects_stay_distinct() {
    for object in [false, true] {
        let input = equality_input(true, object, RubyMerge::Separate);
        COMPARED_BYTES.with(|n| n.set(0));
        COMPARED_BASES.with(|n| n.set(0));
        let result = normalize(&input, &Limits::unlimited()).unwrap();
        assert_eq!(result.levels[0].annotations[0].auto_hidden, !object);
        assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
        assert_eq!(COMPARED_BASES.with(|n| n.get()), 0);
    }
}

#[test]
fn independent_and_multicolumn_inputs_keep_exact_comparison() {
    let mut input = equality_input(false, false, RubyMerge::Separate);
    COMPARED_BYTES.with(|n| n.set(0));
    assert!(
        normalize(&input, &Limits::unlimited()).unwrap().levels[0]
            .annotations
            .last()
            .unwrap()
            .auto_hidden
    );
    assert_eq!(COMPARED_BYTES.with(|n| n.get()), 4096);
    // An identical Arc is not proof that two concatenated columns equal it.
    let shared = input.bases[0].content.clone();
    input = Ruby::new(
        vec![input.bases[0].clone(); 2],
        vec![RubyLevel {
            style: RubyStyle::default(),
            annotations: vec![RubyAnnotation {
                node: NodeId(4),
                content: shared,
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
        }],
    )
    .unwrap();
    assert!(!normalize(&input, &Limits::unlimited()).unwrap().levels[0].annotations[0].auto_hidden);
    // Identity works for one column inside a larger ruby, not just column zero.
    input.levels[0].annotations[0].span = RubySpan::Columns(1..2);
    COMPARED_BYTES.with(|n| n.set(0));
    assert!(
        normalize(&input, &Limits::unlimited()).unwrap().levels[0]
            .annotations
            .last()
            .unwrap()
            .auto_hidden
    );
    assert_eq!(COMPARED_BYTES.with(|n| n.get()), 0);
}

#[test]
fn aliases_preserve_nested_output_warnings_and_per_occurrence_bytes() {
    let make = |id| {
        let mut source = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
        let child = snapshot(id);
        source.push_ruby(NodeId(20), &InlineStyle::default(), bases([child]));
        // A snapshot closes this and imports its warning on every occurrence.
        source.open_inline(NodeId(21), &InlineStyle::default(), InlineEdges::default());
        RubyContent::from_builder(source)
    };
    let first = make(0);
    let second = make(1);
    let build = |shared: bool, limits: &Limits| {
        let input = bases((0..6).map(|i| {
            if shared {
                if i % 2 == 0 {
                    first.clone()
                } else {
                    second.clone()
                }
            } else {
                make(i % 2)
            }
        }));
        let mut parent = ParagraphBuilder::new(&ParagraphStyle::default(), limits);
        parent.push_ruby(NodeId(1000), &InlineStyle::default(), input);
        parent
    };
    let shared = build(true, &Limits::unlimited());
    let independent = build(false, &Limits::unlimited());
    assert_eq!(shared.error(), None);
    assert_eq!(independent.error(), None);
    assert_eq!(shared.text, independent.text);
    assert_eq!(
        format!("{:?}", shared.items),
        format!("{:?}", independent.items)
    );
    assert_eq!(shared.styles, independent.styles);
    assert_eq!(shared.first_line_styles, independent.first_line_styles);
    assert_eq!(
        format!("{:?}", shared.warnings),
        format!("{:?}", independent.warnings)
    );
    assert_eq!(shared.ruby_cost, independent.ruby_cost);
    assert_eq!(shared.rubies.len(), 7);
    for (left, right) in shared.rubies.iter().zip(&independent.rubies) {
        assert_eq!(left.style, right.style);
        for (left, right) in left.normalized.bases.iter().zip(&right.normalized.bases) {
            assert_eq!(left.retained_style_indices, right.retained_style_indices);
            assert_eq!(left.retained_styles, right.retained_styles);
        }
    }
    let exact = shared.style_bytes + shared.ruby_cost.style_bytes;
    for cap in [exact - 1, exact] {
        let parent = build(
            true,
            &Limits {
                max_style_bytes: Some(cap),
                ..Limits::unlimited()
            },
        );
        if cap == exact {
            assert_eq!(parent.error(), None);
        } else {
            assert_eq!(
                parent.error().unwrap().kind,
                crate::limits::LimitKind::StyleBytes
            );
            assert_eq!(parent.error().unwrap().actual, exact);
        }
    }
}

#[test]
fn repeated_aliases_still_charge_every_retained_index_before_normalization() {
    let shared = RubyContent::text(
        TextSource::Generated { node: NodeId(1) },
        "",
        &InlineStyle::default(),
        &Limits::default(),
    );
    for cap in [1263, 1264] {
        let mut parent = ParagraphBuilder::new(
            &ParagraphStyle::default(),
            &Limits {
                max_style_bytes: Some(cap),
                ..Default::default()
            },
        );
        super::pairing::NORMALIZE_CALLS.with(|n| n.set(0));
        PREPARED_MAPS.with(|n| n.set(0));
        parent.push_ruby(
            NodeId(2),
            &InlineStyle::default(),
            bases([shared.clone(), shared.clone()]),
        );
        assert_eq!(PREPARED_MAPS.with(|n| n.get()), 1);
        if cap == 1263 {
            assert_eq!(
                parent.error().unwrap().kind,
                crate::limits::LimitKind::StyleBytes
            );
            assert_eq!(parent.error().unwrap().actual, 1264);
            assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 0);
        } else {
            assert_eq!(parent.error(), None);
            assert_eq!(super::pairing::NORMALIZE_CALLS.with(|n| n.get()), 1);
        }
    }
}
