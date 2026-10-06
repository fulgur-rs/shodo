use crate::font::{FontCollection, FontOptions};
use crate::limits::{Limits, WarningKind};
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::style::{LineOptions, ParagraphStyle, TextAlign};
use crate::{
    AtomicSize, AtomicSizes, BreakToken, LayoutContext, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn paragraph(
    text: &str,
    limits: &Limits,
    first: bool,
    atomic: bool,
) -> (Paragraph, FontCollection) {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut style = ParagraphStyle::default();
    if first {
        style.first_line = Some(style.root.clone());
    }
    let mut b = ParagraphBuilder::new(&style, limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, text);
    if atomic {
        b.push_atomic(NodeId(2), &style.root, InlineEdges::default());
    }
    (b.build(&mut LayoutContext::new(), &fonts).unwrap(), fonts)
}
fn reject(
    p: &Paragraph,
    cx: &mut LayoutContext,
    token: BreakToken,
    o: &LineOptions,
    c: &LineConstraint<'_>,
    a: &AtomicSizes,
) -> f32 {
    let LineResult::BlockSizeExceeded { needed_block_size } = p.next_line(cx, token, o, c, a)
    else {
        panic!("height0")
    };
    needed_block_size
}
fn signature(l: &crate::Line) -> String {
    let glyphs = l
        .fragments()
        .flat_map(|f| match f {
            crate::Fragment::GlyphRun(r) => r.glyphs().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    format!(
        "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{:?}",
        l.text_range(),
        l.break_token(),
        l.metrics(),
        (l.inline_size(), l.block_size(), l.block_offset()),
        l.fragments().collect::<Vec<_>>(),
        glyphs,
        l.displaced_floats()
    )
}
#[test]
fn geometry_dependencies_recompute_after_rejection() {
    let (p, fonts) = paragraph("alpha beta gamma delta", &Limits::default(), false, false);
    let options = LineOptions::default();
    for field in 0..9 {
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(1000.);
        c.max_block_size = Some(0.);
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &options,
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_some());
        let mut token = p.start_token();
        let mut o = options;
        let mut a = AtomicSizes::new();
        match field {
            0 => c.available_inline_size = 60.,
            1 => c.inline_start_offset = 10.,
            2 => c.block_offset = 20.,
            3 => o.text_align = TextAlign::Center,
            4 => c.max_graphemes = Some(3),
            5 => c.floats_placed_through = Some(crate::FloatCursor(0)),
            6 => token.flags = BreakToken::AFTER_FORCED,
            7 => a.insert(NodeId(99), AtomicSize::default()),
            8 => {
                fonts
                    .register(crate::test_support::fonts::LATIN.to_vec())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        c.max_block_size = None;
        crate::output::construction_probe::reset();
        let LineResult::Line(actual) = p.next_line(&mut cx, token, &o, &c, &a) else {
            panic!("accept")
        };
        assert_eq!(
            crate::output::construction_probe::count(),
            1,
            "dependency{field}"
        );
        assert!(cx.completed.is_none());
        let LineResult::Line(fresh) = p.next_line(&mut LayoutContext::new(), token, &o, &c, &a)
        else {
            panic!("fresh")
        };
        assert_eq!(signature(&actual), signature(&fresh));
    }
}
#[test]
fn atomic_equal_generations_with_distinct_revisions_do_not_reuse_height() {
    let (p, _) = paragraph("alpha ", &Limits::default(), false, true);
    let mut small = AtomicSizes::new();
    let mut large = AtomicSizes::new();
    small.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 10.,
            block_size: 40.,
            ..Default::default()
        },
    );
    large.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 50.,
            block_size: 80.,
            ..Default::default()
        },
    );
    assert_eq!(small.generation(), large.generation());
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(1000.);
    c.max_block_size = Some(0.);
    let small_height = reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &small,
    );
    let large_height = reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &large,
    );
    assert!(large_height > small_height);
    assert_eq!(large_height, 84.703125); //80px atomic ascent + fixed Latin4.703125px descent.
    c.max_block_size = None;
    let LineResult::Line(l) =
        p.next_line(&mut cx, p.start_token(), &Default::default(), &c, &large)
    else {
        panic!("accept")
    };
    assert_eq!(l.block_size(), 84.703125);
}
#[test]
fn height_sanitization_and_suppression_cannot_hide_fallbacks() {
    for cap in [Some(0), Some(1), Some(1024)] {
        let limits = Limits {
            max_warnings: cap,
            ..Default::default()
        };
        let (p, _) = paragraph("alpha beta", &limits, false, false);
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(1000.);
        c.max_block_size = Some(0.);
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_some());
        c.max_block_size = Some(f32::NAN);
        for _ in 0..2 {
            let got = reject(
                &p,
                &mut cx,
                p.start_token(),
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            );
            assert!(cx.completed.is_none());
            let mut fresh = LayoutContext::new();
            let want = reject(
                &p,
                &mut fresh,
                p.start_token(),
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(got, want);
            assert_eq!(cx.take_warnings(), fresh.take_warnings());
        }
        c.max_block_size = Some(0.);
        cx.warnings.push(WarningKind::Unsupported, "prior warning");
        cx.warnings.push(WarningKind::Unsupported, "prior warning");
        if cap.is_some_and(|n| n < 2) {
            assert!(cx.warnings.is_suppressed());
            reject(
                &p,
                &mut cx,
                p.start_token(),
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            );
            assert!(cx.completed.is_none());
        }
    }
}
#[test]
fn first_line_plans_and_resource_warnings_keep_original_path() {
    let (p, _) = paragraph("alpha beta gamma", &Limits::default(), true, false);
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(1000.);
    c.max_block_size = Some(0.);
    for _ in 0..2 {
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_none());
    }
    let (p, _) = paragraph("alpha beta gamma", &Limits::default(), false, false);
    let plan = p.plan_breaks(&mut cx, &Default::default(), 1000., &AtomicSizes::EMPTY);
    c.break_plan = Some(&plan);
    for _ in 0..2 {
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_none());
    }
    let (p, _) = paragraph("alpha", &Limits::default(), false, true);
    c.break_plan = None;
    for _ in 0..2 {
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_none());
        assert!(
            cx.take_warnings()
                .iter()
                .any(|w| w.kind == WarningKind::MissingAtomicSize)
        );
    }
}
#[test]
fn invalid_done_other_owner_and_shrink_release_completed_ownership() {
    let (p, _) = paragraph("alpha", &Limits::default(), false, false);
    let (other, _) = paragraph("other", &Limits::default(), false, false);
    let mut c = LineConstraint::new(1000.);
    c.max_block_size = Some(0.);
    for mode in 0..4 {
        let mut cx = LayoutContext::new();
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_some());
        match mode {
            0 => assert!(matches!(
                p.next_line(
                    &mut cx,
                    other.start_token(),
                    &Default::default(),
                    &c,
                    &AtomicSizes::EMPTY
                ),
                LineResult::InvalidToken
            )),
            1 => {
                let mut done = p.start_token();
                done.unit = p.data.units.len() as u32;
                assert!(matches!(
                    p.next_line(&mut cx, done, &Default::default(), &c, &AtomicSizes::EMPTY),
                    LineResult::Done
                ));
            }
            2 => {
                let _ = other.next_line(
                    &mut cx,
                    other.start_token(),
                    &Default::default(),
                    &LineConstraint::new(1000.),
                    &AtomicSizes::EMPTY,
                );
            }
            3 => cx.shrink_to(usize::MAX),
            _ => unreachable!(),
        }
        assert!(cx.completed.is_none());
    }
    let mut cx = LayoutContext::new();
    let weak = std::sync::Arc::downgrade(&p.data);
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    drop(p);
    assert!(
        weak.upgrade().is_some(),
        "shared paragraph is prolonged by retained state"
    );
    cx.shrink_to(0);
    assert!(weak.upgrade().is_none(), "all context owners released");
}
#[test]
fn oversized_geometry_is_not_retained_and_accepted_line_is_independent() {
    let limits = Limits::default();
    let (_, fonts) = paragraph("", &limits, false, false);
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    for i in 0..1000 {
        b.push_text(TextSource::Generated { node: NodeId(i) }, "ab ");
    }
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(100000.);
    c.max_block_size = Some(0.);
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    assert!(
        cx.completed.is_none(),
        "large positions/fragments exceed owned capacity cap"
    );
    let (p, _) = paragraph("alpha beta", &Limits::default(), false, false);
    c.available_inline_size = 1000.;
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    c.max_block_size = None;
    let LineResult::Line(l) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("accept")
    };
    let original = signature(&l);
    c.max_block_size = Some(0.);
    c.available_inline_size = 40.;
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    cx.shrink_to(0);
    assert_eq!(
        signature(&l),
        original,
        "later context mutation cannot mutate returned owned line"
    );
}

#[test]
fn emphasis_offset_capacity_obeys_completed_budget_in_root_and_ruby_child() {
    use crate::output::ruby::{RubyAnnotationRecord, RubyTransform};
    use crate::{RubyVisibility, geometry::LayoutUnit};

    let (p, _) = paragraph("alpha", &Limits::default(), false, false);
    let options = LineOptions::default();
    let constraint = LineConstraint::new(1000.);
    let line = p
        .break_all(
            &mut LayoutContext::new(),
            &options,
            1000.,
            &AtomicSizes::EMPTY,
        )
        .remove(0);
    let header = std::mem::size_of::<super::CompletedLine>()
        + std::mem::size_of::<Option<Box<super::CompletedLine>>>();
    let budget = super::MAX_BYTES - header;
    let element = std::mem::size_of::<(LayoutUnit, LayoutUnit)>();
    for child in [false, true] {
        for exceeds in [false, true] {
            let mut trial = line.clone();
            if child {
                trial.ruby.push(RubyAnnotationRecord {
                    container: NodeId(2),
                    base_nodes: Vec::new(),
                    node: Some(NodeId(3)),
                    level: 0,
                    base_text: 0..5,
                    paragraph: p.clone(),
                    line: line.clone(),
                    visibility: RubyVisibility::Visible,
                    transform: RubyTransform {
                        inline_inline: 1.,
                        inline_block: 0.,
                        block_inline: 0.,
                        block_block: 1.,
                        inline_offset: 0.,
                        block_offset: 0.,
                    },
                });
            }
            let ordinary = trial.owned_heap_bytes(usize::MAX).unwrap();
            let capacity = (budget - ordinary) / element + usize::from(exceeds);
            let offsets = if child {
                &mut trial.ruby[0].line.emphasis_offsets
            } else {
                &mut trial.emphasis_offsets
            };
            // Unused capacity is still owned, even when no marks are painted.
            *offsets = Vec::with_capacity(capacity);
            let requested = ordinary + offsets.capacity() * element;
            assert_eq!(trial.owned_heap_bytes(usize::MAX), Some(requested));
            assert_eq!(trial.owned_heap_bytes(budget).is_none(), exceeds);
            let key = super::Key::new(
                &p,
                p.start_token(),
                options,
                &constraint,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(
                super::CompletedLine::retain(key, trial).is_none(),
                exceeds,
                "child={child}, exceeds={exceeds}, requested={requested}, budget={budget}"
            );
        }
    }
}

#[test]
fn retained_annotation_geometry_is_reused_without_content_rescans() {
    use crate::style::{LineHeight, TextEmphasis, TextEmphasisPosition, TextEmphasisShape};
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_size = 10.;
    style.root.line_height = LineHeight::Px(40.);
    style.root.text_emphasis = Some(TextEmphasis {
        shape: TextEmphasisShape::Dot,
        filled: true,
        position: TextEmphasisPosition::OverRight,
    });
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "alpha");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let mut c = LineConstraint::new(1000.);
    c.max_block_size = Some(0.);
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &options,
        &c,
        &AtomicSizes::EMPTY,
    );
    let retained = cx
        .completed
        .as_ref()
        .expect("small clean geometry is retained");
    let expected = retained.line.annotation_metrics();
    let storage = retained.line.owned_heap_bytes(usize::MAX).unwrap();
    let fragments = retained.line.fragments.as_ptr();
    assert!(expected.space_over > 0. && expected.space_under > 0.);
    crate::output::annotation_probe::reset();
    for _ in 0..2 {
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &options,
            &c,
            &AtomicSizes::EMPTY,
        );
        let saved = &cx.completed.as_ref().unwrap().line;
        assert_eq!(saved.annotation_metrics(), expected);
        assert_eq!(saved.owned_heap_bytes(usize::MAX), Some(storage));
    }
    c.max_block_size = None;
    let LineResult::Line(line) =
        p.next_line(&mut cx, p.start_token(), &options, &c, &AtomicSizes::EMPTY)
    else {
        panic!("accepted")
    };
    assert_eq!(line.annotation_metrics(), expected);
    assert_eq!(
        line.fragments.as_ptr(),
        fragments,
        "cached owned vectors are moved"
    );
    assert_eq!(crate::output::annotation_probe::count(), 0);
    assert!(cx.completed.is_none());
}

#[test]
fn resource_and_saturating_retries_keep_fresh_warning_order_and_budget_reset() {
    for mode in 0..4 {
        let mut limits = Limits::default();
        if mode == 0 {
            limits.max_reshape_window_bytes = Some(0);
        }
        if mode == 1 {
            limits.max_shaping_run_bytes = Some(1);
        }
        if mode == 2 {
            limits.max_shaper_cache_entries = Some(0);
        }
        let (p, _) = paragraph("office alpha beta gamma", &limits, false, false);
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(45.);
        c.max_block_size = Some(0.);
        if mode == 3 {
            c.inline_start_offset = f32::MAX;
        }
        for _ in 0..3 {
            let actual = reject(
                &p,
                &mut cx,
                p.start_token(),
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            );
            let mut fresh = LayoutContext::new();
            let want = reject(
                &p,
                &mut fresh,
                p.start_token(),
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(actual, want);
            let got = cx.take_warnings();
            assert_eq!(got, fresh.take_warnings());
            if mode == 3 || !got.is_empty() {
                assert!(cx.completed.is_none());
            }
            // Hit may spend zero; a later fallback starts from zero rather than accumulated spending.
            assert!(cx.edge_reshape_spent <= limits.max_reshape_window_bytes.unwrap_or(u64::MAX));
        }
    }
}

#[test]
fn intervening_context_operations_invalidate_completed_trial_dependencies() {
    let (p, fonts) = paragraph("alpha beta gamma", &Limits::default(), false, false);
    for operation in 0..2 {
        let mut cx = LayoutContext::new();
        let mut c = LineConstraint::new(1000.);
        c.max_block_size = Some(0.);
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(cx.completed.is_some());
        if operation == 0 {
            p.intrinsic_sizes(&mut cx, &Default::default(), &Default::default());
        } else {
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
            b.push_text(
                TextSource::Generated { node: NodeId(8) },
                "different input ffi",
            );
            b.build(&mut cx, &fonts).unwrap();
        }
        crate::output::construction_probe::reset();
        c.max_block_size = None;
        let LineResult::Line(actual) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("accept")
        };
        assert_eq!(
            crate::output::construction_probe::count(),
            1,
            "intervening operation{operation} changed context cache/budget dependencies"
        );
        let LineResult::Line(fresh) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("fresh")
        };
        assert_eq!(signature(&actual), signature(&fresh));
    }
}

#[test]
fn grapheme_limited_trial_does_not_prolong_an_uncached_paragraph() {
    let (p, _) = paragraph(&"alpha beta ".repeat(100), &Limits::default(), false, false);
    let weak = std::sync::Arc::downgrade(&p.data);
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(80.);
    c.max_graphemes = Some(3);
    c.max_block_size = Some(0.);
    reject(
        &p,
        &mut cx,
        p.start_token(),
        &Default::default(),
        &c,
        &AtomicSizes::EMPTY,
    );
    assert!(cx.partial.is_none());
    drop(p);
    assert!(
        weak.upgrade().is_none(),
        "a short rejected line must not newly prolong the large shared paragraph"
    );
}

#[test]
fn child_only_build_warnings_disqualify_completed_ruby_trial() {
    use crate::font::FontFaceDescriptor;
    use crate::ruby::*;
    use crate::style::{FontFamily, InlineStyle};
    let limits = Limits::default();
    let fonts = FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let inline = InlineStyle {
        font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
        ..Default::default()
    };
    let child_limits = Limits {
        max_shaping_run_bytes: Some(1),
        ..Default::default()
    };
    let reading = InlineStyle {
        font_size: 8.,
        ..inline.clone()
    };
    let ruby = Ruby::new(
        vec![RubyBase {
            node: NodeId(1),
            content: RubyContent::text(
                TextSource::Generated { node: NodeId(1) },
                "日本語",
                &inline,
                &limits,
            ),
            align: RubyAlign::default(),
        }],
        vec![RubyLevel {
            annotations: vec![RubyAnnotation {
                node: NodeId(2),
                content: RubyContent::text(
                    TextSource::Generated { node: NodeId(2) },
                    "にほんご",
                    &reading,
                    &child_limits,
                ),
                span: RubySpan::All,
                visibility: RubyVisibility::Visible,
            }],
            style: RubyStyle::default(),
        }],
    )
    .unwrap();
    let style = ParagraphStyle {
        root: inline.clone(),
        ..Default::default()
    };
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_ruby(NodeId(0), &inline, ruby);
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    assert!(p.warnings().is_empty(), "root build is clean");
    assert!(
        !p.data.ruby.containers[0].lanes[0]
            .paragraph
            .warnings()
            .is_empty(),
        "real child-only run budget fallback"
    );
    let mut cx = LayoutContext::new();
    let mut c = LineConstraint::new(80.);
    c.max_block_size = Some(0.);
    for _ in 0..2 {
        reject(
            &p,
            &mut cx,
            p.start_token(),
            &Default::default(),
            &c,
            &AtomicSizes::EMPTY,
        );
        assert!(
            cx.take_warnings().is_empty(),
            "build warnings are retained on the child rather than emitted at layout"
        );
        assert!(
            cx.completed.is_none(),
            "selected child resource fallbacks are excluded by conservative policy"
        );
    }
}
