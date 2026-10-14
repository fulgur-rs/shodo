use crate::font::{FontCollection, FontOptions};
use crate::limits::Limits;
use crate::node::NodeId;
use crate::style::{LineOptions, ParagraphStyle};
use crate::{AtomicSizes, LayoutContext, ParagraphBuilder};

fn synthetic_paragraph(
    style: &ParagraphStyle,
    limits: &Limits,
    text: &str,
) -> (crate::Paragraph, FontCollection) {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut chars: Vec<_> = text.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    fonts
        .register(crate::font::test_font_bytes("Summary", &chars, 600))
        .unwrap();
    let mut builder = ParagraphBuilder::new(style, limits);
    builder.push_text(crate::node::TextSource::Generated { node: NodeId(1) }, text);
    (
        builder.build(&mut LayoutContext::new(), &fonts).unwrap(),
        fonts,
    )
}

fn assert_trials_equal_full(
    p: &crate::Paragraph,
    full: &mut LayoutContext,
    trial: &mut LayoutContext,
    width: f32,
) -> usize {
    let options = LineOptions::default();
    let expected: Vec<_> = p
        .break_all(full, &options, width, &AtomicSizes::EMPTY)
        .into_iter()
        .map(|l| (l.break_token(), l.block_size().to_bits()))
        .collect();
    crate::output::construction_probe::reset();
    let actual: Vec<_> = p
        .fixed_width_trials(trial, &options, width, &AtomicSizes::EMPTY)
        .map(|l| (l.token, l.block_size.to_bits()))
        .collect();
    let constructors = crate::output::construction_probe::count();
    assert_eq!(actual, expected, "width={width}");
    assert_eq!(trial.take_warnings(), full.take_warnings(), "width={width}");
    assert_eq!(trial.edge_reshape_spent, full.edge_reshape_spent);
    assert_eq!(trial.ruby_walk_steps, full.ruby_walk_steps);
    assert_eq!(trial.ruby_line_work, full.ruby_line_work);
    constructors
}

#[test]
fn eligible_trial_heights_match_full_with_warm_contexts_and_tiny_budgets() {
    for height in [
        crate::style::LineHeight::Normal,
        crate::style::LineHeight::Px(0.),
        crate::style::LineHeight::Px(5.125),
        crate::style::LineHeight::Number(1.3333),
    ] {
        for limited in [false, true] {
            let mut style = ParagraphStyle::default();
            style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
            style.root.line_height = height;
            let mut limits = Limits::default();
            if limited {
                limits.max_reshape_window_bytes = Some(0);
                limits.max_warnings = Some(0);
            }
            let (p, _fonts) = synthetic_paragraph(&style, &limits, &"ab ab ".repeat(16));
            let mut full = LayoutContext::new();
            let mut trial = LayoutContext::new();
            for width in [0., 16., 63.125, 240., 4096., 63.125] {
                assert_eq!(
                    assert_trials_equal_full(&p, &mut full, &mut trial, width),
                    0,
                    "safe synthetic Latin should summarize: height={height:?}, limited={limited}, width={width}"
                );
            }
        }
    }
}

#[test]
fn numeric_outliers_preserve_completed_line_saturation_warnings() {
    for ascent in [4096.125, 1_000_000., f32::MAX, f32::NAN] {
        for warning_limit in [None, Some(0), Some(1)] {
            let limits = Limits {
                max_warnings: warning_limit,
                ..Default::default()
            };
            let mut style = ParagraphStyle::default();
            style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
            let (mut p, _fonts) = synthetic_paragraph(&style, &limits, &"ab ab ".repeat(16));
            std::sync::Arc::get_mut(&mut p.data).unwrap().style_metrics[0]
                .metrics
                .ascent = ascent;
            let mut full = LayoutContext::new();
            let mut trial = LayoutContext::new();
            for width in [32., 4096., 32.] {
                assert!(
                    assert_trials_equal_full(&p, &mut full, &mut trial, width) > 0,
                    "unsafe metric must materialize: ascent={ascent}, warning_limit={warning_limit:?}"
                );
            }
        }
    }
}

#[test]
fn cjk_spacing_keeps_completed_trial_geometry() {
    let limits = Limits::default();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
    let (p, _fonts) = synthetic_paragraph(&style, &limits, &"日本語".repeat(16));
    assert!(
        assert_trials_equal_full(
            &p,
            &mut LayoutContext::new(),
            &mut LayoutContext::new(),
            64.
        ) > 0
    );
}

#[test]
fn shaped_font_metric_outliers_preserve_full_numeric_effects() {
    for ascent in [4096.125, f32::MAX, f32::NAN] {
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
        let (mut p, _fonts) = synthetic_paragraph(&style, &Limits::default(), &"ab ab ".repeat(16));
        for run in &mut std::sync::Arc::get_mut(&mut p.data).unwrap().runs {
            std::sync::Arc::make_mut(&mut run.instance)
                .metrics
                .as_mut()
                .unwrap()
                .ascent = ascent;
        }
        assert!(
            assert_trials_equal_full(
                &p,
                &mut LayoutContext::new(),
                &mut LayoutContext::new(),
                32.
            ) > 0
        );
    }
}

#[test]
fn public_full_and_pretty_keep_completed_line_materialization() {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
    let (p, _fonts) = synthetic_paragraph(&style, &Limits::default(), &"ab ab ".repeat(16));
    crate::output::construction_probe::reset();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        64.,
        &AtomicSizes::EMPTY,
    );
    assert!(!lines.is_empty());
    assert!(crate::output::construction_probe::count() > 0);
    crate::output::construction_probe::reset();
    let options = LineOptions {
        text_wrap_style: crate::style::TextWrapStyle::Pretty,
        ..Default::default()
    };
    p.plan_breaks(
        &mut LayoutContext::new(),
        &options,
        64.,
        &AtomicSizes::EMPTY,
    );
    assert!(crate::output::construction_probe::count() > 0);
}

#[test]
fn balance_eligible_text_does_not_construct_trial_lines() {
    let limits = Limits {
        max_balance_iterations: Some(1),
        ..Default::default()
    };
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named("Summary".into())];
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register(crate::font::test_font_bytes(
            "Summary",
            &['a', 'b', ' '],
            600,
        ))
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated { node: NodeId(1) },
        &"ab ab ".repeat(16),
    );
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let options = LineOptions {
        text_wrap_style: crate::style::TextWrapStyle::Balance,
        ..Default::default()
    };
    let mut cx = LayoutContext::new();
    crate::output::construction_probe::reset();
    let plan = p.plan_breaks(&mut cx, &options, 64., &AtomicSizes::EMPTY);
    assert!(plan.ends.len() > 1);
    assert_eq!(
        crate::output::construction_probe::count(),
        0,
        "eligible Balance trials must not construct completed Lines"
    );
    assert!(cx.take_warnings().is_empty());
}

#[test]
fn trial_driver_matches_completed_text_lines() {
    let limits = Limits::default();
    let style = ParagraphStyle::default();
    let fonts = crate::test_support::fonts::collection(&limits);
    fonts
        .register(crate::test_support::fonts::LATIN.to_vec())
        .unwrap();
    let mut builder = ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated { node: NodeId(1) },
        "Alpha beta gamma delta epsilon zeta eta theta.",
    );
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    for width in [0., 16., 63.125, 240., 4096.] {
        let options = LineOptions::default();
        let mut full_cx = LayoutContext::new();
        let expected: Vec<_> = p
            .break_all(&mut full_cx, &options, width, &AtomicSizes::EMPTY)
            .into_iter()
            .map(|l| (l.break_token(), l.block_size().to_bits()))
            .collect();
        let mut trial_cx = LayoutContext::new();
        let actual: Vec<_> = p
            .fixed_width_trials(&mut trial_cx, &options, width, &AtomicSizes::EMPTY)
            .map(|l| (l.token, l.block_size.to_bits()))
            .collect();
        assert_eq!(actual, expected, "width={width}");
        assert_eq!(
            trial_cx.take_warnings(),
            full_cx.take_warnings(),
            "width={width}"
        );
    }
}

#[test]
fn trial_driver_preserves_first_line_float_and_block_progress() {
    for first_line in [false, true] {
        let limits = Limits {
            max_warnings: None,
            ..Default::default()
        };
        let mut style = ParagraphStyle::default();
        if first_line {
            style.first_line = Some(crate::style::InlineStyle {
                font_size: 32.,
                text_transform: crate::style::TextTransform::Uppercase,
                ..style.root.clone()
            });
        }
        let fonts = crate::test_support::fonts::collection(&limits);
        fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let mut builder = ParagraphBuilder::new(&style, &limits);
        let text = |id| crate::node::TextSource::Generated { node: NodeId(id) };
        builder
            .push_text(text(1), "Straße ffi alpha beta ")
            .push_out_of_flow(NodeId(2), crate::node::OutOfFlowKind::Float)
            .push_text(text(3), "gamma delta ")
            .push_block_in_inline(NodeId(4))
            .push_text(text(5), "epsilon ")
            .push_forced_break(NodeId(6))
            .push_text(text(7), "zeta eta theta.");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        for width in [0., 32., 80., 240., f32::MAX] {
            let options = LineOptions::default();
            let mut full = LayoutContext::new();
            let expected: Vec<_> = p
                .break_all(&mut full, &options, width, &AtomicSizes::EMPTY)
                .into_iter()
                .map(|l| (l.break_token(), l.block_size().to_bits()))
                .collect();
            let mut trial = LayoutContext::new();
            let actual: Vec<_> = p
                .fixed_width_trials(&mut trial, &options, width, &AtomicSizes::EMPTY)
                .map(|l| (l.token, l.block_size.to_bits()))
                .collect();
            assert_eq!(actual, expected, "first_line={first_line}, width={width}");
            assert_eq!(
                trial.take_warnings(),
                full.take_warnings(),
                "first_line={first_line}, width={width}"
            );
        }
    }
}
