use super::*;
use crate::node::TextSource;

fn input(text: &str, style: &ParagraphStyle, limits: &Limits) -> ParagraphBuilder {
    let mut builder = ParagraphBuilder::new(style, limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    builder
}

#[test]
fn analysis_budget_rejects_before_break_analysis_without_shaping() {
    let limits = Limits {
        max_analysis_bytes: Some(32),
        max_shaped_glyphs: Some(0),
        ..Default::default()
    };
    ANALYSIS_PASSES.with(|n| n.set(0));
    crate::analysis::reset_analysis_producers();
    let result = input(&"a".repeat(1024), &ParagraphStyle::default(), &limits).analyze();
    assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes && e.limit == 32));
    assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 0);
    assert_eq!(crate::analysis::analysis_producers(), (0, 0, 0, 0));
}

#[test]
fn analysis_budget_exact_boundary_and_build_error_match() {
    let style = ParagraphStyle::default();
    let text = "abc a\u{0301} \u{200f}";
    let used = input(text, &style, &Limits::unlimited())
        .analyze()
        .unwrap()
        .state
        .normal
        .analysis_cost
        .used;
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    for cap in [0, 1, used - 1, used, used + 1] {
        let limits = Limits {
            max_analysis_bytes: Some(cap),
            ..Default::default()
        };
        let analyze = input(text, &style, &limits).analyze().map(|_| ());
        let build = input(text, &style, &limits)
            .build(&mut crate::LayoutContext::new(), &fonts)
            .map(|_| ());
        assert_eq!(analyze, build);
        assert_eq!(analyze.is_ok(), cap >= used);
        if cap == used - 1 {
            assert_eq!(
                analyze,
                Err(LimitExceeded {
                    kind: LimitKind::AnalysisBytes,
                    limit: cap,
                    actual: used
                })
            );
        }
    }
}

#[test]
fn analysis_budget_first_line_source_cuts_are_reserved_before_projection() {
    let mut style = ParagraphStyle::default();
    style.first_line = Some(style.root.clone());
    let limits = Limits {
        max_analysis_bytes: Some(128),
        ..Default::default()
    };
    crate::analysis::reset_analysis_producers();
    ANALYSIS_PASSES.with(|n| n.set(0));
    let result = input(&"a".repeat(64), &style, &limits).analyze();
    assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
    // The raw flags fit. Source cuts, transform and normal analysis have not run.
    assert_eq!(crate::analysis::analysis_producers(), (1, 0, 0, 0));
    assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 0);
}

#[test]
fn analysis_budget_case_context_and_tcy_omissions_are_reserved_before_producers() {
    for case in [true, false] {
        let mut style = ParagraphStyle::default();
        if case {
            style.root.text_transform = crate::style::TextTransform::Uppercase;
        } else {
            style.writing_mode = crate::geometry::WritingMode::VerticalRl;
            style.root.text_combine_upright = crate::style::TextCombineUpright::All;
        }
        let limits = Limits {
            max_analysis_bytes: Some(512),
            ..Default::default()
        };
        crate::analysis::reset_analysis_producers();
        let result = input(&"a".repeat(256), &style, &limits).analyze();
        assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
        assert_eq!(crate::analysis::analysis_producers(), (1, 0, 0, 0));
    }
}

#[test]
fn analysis_budget_complex_line_and_word_scratch_is_admitted_before_icu() {
    for scalar in ['ก', 'က', 'ក', 'ກ'] {
        for case in [false, true] {
            let mut style = ParagraphStyle::default();
            if case {
                style.root.text_transform = crate::style::TextTransform::Uppercase;
            }
            let text = scalar.to_string().repeat(128);
            let limits = Limits {
                max_analysis_bytes: Some(4096),
                ..Default::default()
            };
            crate::analysis::reset_analysis_producers();
            ANALYSIS_PASSES.with(|n| n.set(0));
            let result = input(&text, &style, &limits).analyze();
            assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
            assert_eq!(crate::analysis::analysis_producers().1, 0);
            assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 0);
            // Normal Unicode segmentation and transforms remain available with
            // a sufficiently large/default budget, in both feature modes.
            assert!(input(&text, &style, &Limits::default()).analyze().is_ok());
        }
    }
}

struct ReferenceGuard(bool);
impl ReferenceGuard {
    fn new() -> Self {
        Self(paint_reuse::FORCE_REFERENCE.with(|f| f.replace(true)))
    }
}
impl Drop for ReferenceGuard {
    fn drop(&mut self) {
        paint_reuse::FORCE_REFERENCE.with(|f| f.set(self.0));
    }
}

#[test]
fn analysis_budget_first_line_clone_and_replay_have_identical_boundaries() {
    for transform in [
        crate::style::TextTransform::None,
        crate::style::TextTransform::Uppercase,
    ] {
        let mut style = ParagraphStyle::default();
        style.root.text_transform = transform;
        let mut first = style.root.clone();
        first.paint.color = [255, 0, 0, 255];
        style.first_line = Some(first);
        let text = "Straße a\u{0301} \u{200f}";
        let state = input(text, &style, &Limits::unlimited())
            .analyze()
            .unwrap()
            .state;
        assert!(state.alternate_reuses_analysis);
        let normal = state.normal.analysis_cost.used;
        let both = normal + state.alternate.as_ref().unwrap().analysis_cost.used;
        for cap in [0, normal - 1, normal, normal + 1, both - 1, both, both + 1] {
            let limits = Limits {
                max_analysis_bytes: Some(cap),
                ..Default::default()
            };
            let snapshot = |reference: bool| {
                let _guard = reference.then(ReferenceGuard::new);
                input(text, &style, &limits).analyze().map(|a| {
                    (
                        a.state.normal.analysis_cost.used,
                        a.state.alternate.as_ref().unwrap().analysis_cost.used,
                        a.state.normal.breaks.caret_cuts,
                        a.state
                            .alternate
                            .as_ref()
                            .unwrap()
                            .breaks
                            .caret_cuts
                            .clone(),
                    )
                })
            };
            let candidate = snapshot(false);
            assert_eq!(
                candidate,
                snapshot(true),
                "cap={cap}, transform={transform:?}"
            );
            assert_eq!(candidate.is_ok(), cap >= both);
        }
    }
}

#[test]
fn analysis_budget_transformed_bytes_and_mapping_disabled_cannot_bypass() {
    let mut style = ParagraphStyle::default();
    style.root.text_transform = crate::style::TextTransform::FullWidth;
    let plain_used = input("abc", &ParagraphStyle::default(), &Limits::unlimited())
        .analyze()
        .unwrap()
        .state
        .normal
        .analysis_cost
        .used;
    let limits = Limits {
        max_analysis_bytes: Some(plain_used),
        ..Default::default()
    };
    for mapping in [false, true] {
        let mut builder = input("abc", &style, &limits);
        builder.with_offset_mapping(mapping);
        let result = builder.analyze();
        assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
        let mut builder = input("abc", &style, &Limits::default());
        builder.with_offset_mapping(mapping);
        assert!(builder.analyze().is_ok());
    }
}

fn content(text: &str, limits: &Limits) -> crate::ruby::RubyContent {
    crate::ruby::RubyContent::from_builder(input(text, &ParagraphStyle::default(), limits))
}

fn ruby(
    base: crate::ruby::RubyContent,
    annotation: crate::ruby::RubyContent,
    lanes: usize,
) -> crate::ruby::Ruby {
    use crate::ruby::*;
    Ruby::new(
        vec![RubyBase {
            node: NodeId(10),
            content: base,
            align: RubyAlign::default(),
        }],
        (0..lanes)
            .map(|i| RubyLevel {
                annotations: vec![RubyAnnotation {
                    node: NodeId(20 + i as u64),
                    content: annotation.clone(),
                    span: RubySpan::Auto,
                    visibility: RubyVisibility::Visible,
                }],
                style: RubyStyle::default(),
            })
            .collect(),
    )
    .unwrap()
}

fn ruby_input(ruby: crate::ruby::Ruby, limits: &Limits) -> ParagraphBuilder {
    let mut builder = ParagraphBuilder::new(&ParagraphStyle::default(), limits);
    builder.push_ruby(NodeId(8), &InlineStyle::default(), ruby);
    builder
}

#[test]
fn analysis_budget_imported_base_and_nested_snapshot_keep_local_caps() {
    let local = Limits {
        max_analysis_bytes: Some(512),
        ..Limits::unlimited()
    };
    let root = Limits::unlimited();
    for nested in [false, true] {
        let base = if nested {
            crate::ruby::RubyContent::from_builder(ruby_input(
                ruby(content(&"a".repeat(1024), &root), content("b", &root), 1),
                &local,
            ))
        } else {
            content(&"a".repeat(1024), &local)
        };
        let result = ruby_input(ruby(base, content("b", &root), 1), &root).analyze();
        assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes && e.limit == 512));
    }
}

fn build_ruby(ruby: crate::ruby::Ruby, limits: &Limits) -> Result<(), LimitExceeded> {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    ruby_input(ruby, limits)
        .build(&mut crate::LayoutContext::new(), &fonts)
        .map(|_| ())
}

#[test]
fn analysis_budget_annotation_and_ancestor_caps_precede_child_analysis() {
    let local = Limits {
        max_analysis_bytes: Some(512),
        ..Limits::unlimited()
    };
    let root = Limits::unlimited();
    let annotation = content(&"a".repeat(1024), &local);
    let result = build_ruby(ruby(content("b", &root), annotation, 1), &root);
    assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes && e.limit == 512));

    let value = ruby(content("b", &root), content(&"a".repeat(1024), &root), 1);
    let normal = ruby_input(value.clone(), &root)
        .analyze()
        .unwrap()
        .state
        .normal
        .analysis_cost
        .used;
    let limits = Limits {
        max_analysis_bytes: Some(normal),
        ..root
    };
    crate::analysis::reset_analysis_producers();
    let result = build_ruby(value, &limits);
    assert!(
        matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes && e.limit == normal && e.actual > normal)
    );
    assert_eq!(crate::analysis::analysis_producers().0, 1); // parent only
}

#[test]
fn analysis_budget_arc_alias_lanes_share_the_aggregate_cap() {
    let root = Limits::unlimited();
    let base = content("b", &root);
    let annotation = content(&"a".repeat(32), &root);
    let one = ruby(base.clone(), annotation.clone(), 1);
    // Reach each cumulative phase boundary until one lane fits exactly.
    let mut cap = 0;
    for _ in 0..32 {
        let limits = Limits {
            max_analysis_bytes: Some(cap),
            ..Limits::unlimited()
        };
        match build_ruby(one.clone(), &limits) {
            Ok(()) => break,
            Err(e) => {
                assert_eq!(e.kind, LimitKind::AnalysisBytes);
                assert_eq!(e.limit, cap);
                assert!(e.actual > cap);
                cap = e.actual;
            }
        }
    }
    let limits = Limits {
        max_analysis_bytes: Some(cap),
        ..Limits::unlimited()
    };
    assert_eq!(build_ruby(one.clone(), &limits), Ok(()));
    let below = Limits {
        max_analysis_bytes: Some(cap - 1),
        ..Limits::unlimited()
    };
    assert!(matches!(build_ruby(one, &below), Err(e) if e.kind == LimitKind::AnalysisBytes));
    let result = build_ruby(ruby(base, annotation, 2), &limits);
    assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes && e.actual > cap));
}

#[test]
fn analysis_budget_default_rejects_the_reported_sixteen_mib_ascii_before_tables() {
    let limits = Limits::default();
    let text = "a".repeat(limits.max_text_bytes.unwrap() as usize);
    for mapping in [false, true] {
        let mut builder = input(&text, &ParagraphStyle::default(), &limits);
        builder.with_offset_mapping(mapping);
        assert!(builder.error.is_none()); // raw text and item admission succeeds
        ANALYSIS_PASSES.with(|n| n.set(0));
        let result = builder.analyze();
        assert!(
            matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes
            && e.limit == limits.max_analysis_bytes.unwrap())
        );
        assert_eq!(ANALYSIS_PASSES.with(std::cell::Cell::get), 0);
    }
    assert!(
        input(&text[..64 * 1024], &ParagraphStyle::default(), &limits)
            .analyze()
            .is_ok()
    );
}

#[test]
fn analysis_budget_transparent_gap_projection_and_plaintext_upright_remain_bounded() {
    let build = |limits: &Limits| {
        let mut builder = input("\u{200e}a", &ParagraphStyle::default(), limits);
        for _ in 0..16 {
            builder.push_out_of_flow(NodeId(4), crate::node::OutOfFlowKind::Absolute);
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "\u{200f}b");
        }
        builder
    };
    let state = build(&Limits::unlimited()).analyze().unwrap().state;
    let used = state.normal.analysis_cost.used;
    let small = Limits {
        max_analysis_bytes: Some(used - 1),
        ..Default::default()
    };
    assert!(matches!(build(&small).analyze(), Err(e) if e.kind == LimitKind::AnalysisBytes));
    let exact = Limits {
        max_analysis_bytes: Some(used),
        ..Default::default()
    };
    let bounded = build(&exact).analyze().unwrap().state;
    assert_eq!(
        bounded.normal.breaks.caret_cuts,
        state.normal.breaks.caret_cuts
    );

    let mut style = ParagraphStyle {
        unicode_bidi_plaintext: true,
        writing_mode: crate::geometry::WritingMode::VerticalRl,
        ..Default::default()
    };
    style.root.text_orientation = crate::style::TextOrientation::Upright;
    let text = "אב\u{2028}a\u{0301}";
    let state = input(text, &style, &Limits::default())
        .analyze()
        .unwrap()
        .state;
    assert_eq!(state.normal.processed.text, text);
    assert_eq!(state.normal.bidi.levels.len(), text.len());
    assert_eq!(state.normal.bidi.paragraphs.len(), 2);
}

#[test]
fn analysis_budget_cjk_nonstarter_case_context_is_reserved_before_dictionary() {
    use icu_properties::{CodePointMapData, props};
    for c in ['々', 'ゝ', 'ゞ'] {
        assert_eq!(
            CodePointMapData::<props::LineBreak>::new().get(c),
            props::LineBreak::Nonstarter
        );
        let text = c.to_string().repeat(4096);
        let limits = Limits {
            max_analysis_bytes: Some(4 * text.len() as u64 + 1024),
            ..Limits::unlimited()
        };
        let mut style = ParagraphStyle::default();
        style.root.text_transform = crate::style::TextTransform::Uppercase;
        crate::analysis::reset_analysis_producers();
        let result = input(&text, &style, &limits).analyze();
        assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
        assert_eq!(crate::analysis::analysis_producers().1, 0);
    }
}

#[test]
fn analysis_budget_also_bounds_long_graphemes_and_authored_controls() {
    let limits = Limits {
        max_analysis_bytes: Some(1024),
        ..Default::default()
    };
    for text in [
        format!("a{}", "\u{0301}".repeat(1024)),
        "\u{200f}".repeat(1024),
    ] {
        let result = input(&text, &ParagraphStyle::default(), &limits).analyze();
        assert!(matches!(result, Err(e) if e.kind == LimitKind::AnalysisBytes));
    }
}

#[test]
fn analysis_budget_disabled_preserves_unicode_analysis() {
    for limits in [
        Limits::unlimited(),
        Limits {
            max_analysis_bytes: None,
            ..Default::default()
        },
    ] {
        let state = input(
            "\u{200e}a\u{200f}b\u{200e}",
            &ParagraphStyle::default(),
            &limits,
        )
        .analyze()
        .unwrap()
        .state;
        assert_eq!(state.normal.breaks.graphemes, [3, 4, 8]);
        assert_eq!(state.normal.breaks.typographic_starts, [3, 7, 11]);
        assert_eq!(state.normal.breaks.caret_cuts, [3, 4, 7, 8, 11]);
    }
}
