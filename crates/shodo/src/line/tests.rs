use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::output::BreakReason;
use crate::style::{LineOptions, ParagraphStyle};
use crate::{AtomicSize, AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

#[test]
fn completed_shifts_place_every_empty_long_and_reshaped_fragment() {
    use crate::font::{FontFaceDescriptor, FontOptions};
    use crate::style::{FontFamily, InlineStyle, LineHeight, VerticalAlign};
    let limits = Limits::unlimited();
    let fonts = crate::font::FontCollection::with_options(
        &limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("Latin".into())],
            font_size: 10.0,
            line_height: LineHeight::Px(12.0),
            ..Default::default()
        },
        ..Default::default()
    };
    for count in [0, 2, 512] {
        for reshape in [false, true] {
            if count == 0 && reshape {
                continue;
            }
            let mut b = ParagraphBuilder::new(&style, &limits);
            if count == 0 {
                b.open_inline(NodeId(0), &style.root, Default::default());
                b.close_inline();
            }
            for n in 0..count {
                b.open_inline(
                    NodeId(2 * n),
                    &InlineStyle {
                        vertical_align: VerticalAlign::Length(if n % 2 == 0 { 2.0 } else { -3.0 }),
                        ..style.root.clone()
                    },
                    Default::default(),
                );
                b.push_text(
                    TextSource::Generated {
                        node: NodeId(2 * n + 1),
                    },
                    "a",
                );
                b.close_inline();
            }
            if reshape {
                b.push_text(
                    TextSource::Generated {
                        node: NodeId(2 * count + 1),
                    },
                    "hy\u{ad}phenation",
                );
            }
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            let mut cx = LayoutContext::new();
            let mut constraint = crate::LineConstraint::new(1_000_000.0);
            if reshape {
                let crate::LineResult::Line(full) = p.next_line(
                    &mut cx,
                    p.start_token(),
                    &LineOptions::default(),
                    &constraint,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!()
                };
                let prefix: f32 = full
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(run) if run.node().is_some_and(|n| n.0 < 2 * count) => {
                            Some(run.inline_size())
                        }
                        _ => None,
                    })
                    .sum();
                constraint.available_inline_size = prefix + 16.0;
            }
            let crate::LineResult::Line(line) = p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            assert_eq!(line.block_shifts.len(), line.fragments.len());
            assert_eq!(line.overlay.is_some(), reshape);
            assert_eq!(line.block_size(), if count == 0 { 0.0 } else { 17.0 });
            assert_eq!(line.is_last(), !reshape);
            let mut seen = 0;
            for run in line.fragments().filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            }) {
                let node = run.node().unwrap().0;
                let shift = if node == 2 * count + 1 {
                    0.0
                } else if (node / 2) % 2 == 0 {
                    -2.0
                } else {
                    3.0
                };
                assert_eq!(run.baseline(), line.metrics().baseline + shift);
                let origin = run.glyph_origin(0).unwrap();
                assert_eq!(
                    origin.1,
                    run.baseline() + run.glyphs().get(0).unwrap().block_offset
                );
                seen += 1;
            }
            assert_eq!(seen, count as usize + usize::from(reshape));
            assert!(cx.take_warnings().is_empty());
        }
    }
}

#[test]
fn completed_shift_metrics_preserve_saturation_warning_order() {
    let style = ParagraphStyle::default();
    let limits = Limits::default();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
    let p = b
        .build(&mut LayoutContext::new(), &final_review_fonts())
        .unwrap();
    let mut cx = LayoutContext::new();
    let constraint = crate::LineConstraint {
        inline_start_offset: f32::NAN,
        block_offset: f32::MAX,
        ..crate::LineConstraint::new(f32::MAX)
    };
    let crate::LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(line.block_shifts.len(), line.fragments.len());
    assert!(line.overflow_rect().block_size.is_finite());
    let warnings = cx.take_warnings();
    use crate::limits::{Warning, WarningKind};
    assert_eq!(
        warnings,
        [
            Warning {
                kind: WarningKind::Saturated,
                message: "layout length clamped to 1e7 px".into()
            },
            Warning {
                kind: WarningKind::NonFiniteInput,
                message: "non-finite layout length replaced with 0".into()
            },
            Warning {
                kind: WarningKind::Saturated,
                message: "layout length clamped to 1e7 px".into()
            },
        ]
    );
    let crate::LineResult::Line(narrow) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &crate::LineConstraint::new(1.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(narrow.block_shifts.len(), narrow.fragments.len());
    assert!(narrow.inline_size() > 1.0);
    assert!(narrow.overflow_rect().inline_size.is_finite());
    assert_eq!(
        narrow.overflow_rect(),
        line.overflow_rect(),
        "offsets are not part of the line-local ink overflow rect"
    );
    assert!(cx.take_warnings().is_empty());
}

#[test]
fn displacement_visits_only_float_anchors_not_paragraph_suffixes() {
    let mut root = ParagraphStyle::default();
    root.root.font_size = 10.0;
    let mut b = ParagraphBuilder::new(&root, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, &"a ".repeat(256));
    let p = b
        .build(
            &mut LayoutContext::new(),
            &crate::font::FontCollection::new(&Limits::default()),
        )
        .unwrap();
    let mut cx = LayoutContext::new();
    assert_eq!(
        p.break_all(&mut cx, &LineOptions::default(), 10.0, &AtomicSizes::EMPTY)
            .len(),
        256
    );
    assert_eq!(cx.float_search_visits, 0);
}

#[test]
fn atomic_baseline_lookup_is_constant_per_atomic() {
    let root = ParagraphStyle::default();
    let mut b = ParagraphBuilder::new(&root, &Limits::default());
    let mut sizes = AtomicSizes::new();
    for n in 0..256 {
        b.push_atomic(NodeId(n), &root.root, InlineEdges::default());
        sizes.insert(
            NodeId(n),
            AtomicSize {
                inline_size: 10.0,
                block_size: 10.0,
                ..Default::default()
            },
        );
    }
    let p = b
        .build(
            &mut LayoutContext::new(),
            &crate::font::FontCollection::new(&Limits::default()),
        )
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        10000.0,
        &sizes,
    );
    assert_eq!(lines.len(), 1);
    assert!(
        p.data
            .baseline_queries
            .load(std::sync::atomic::Ordering::Relaxed)
            <= 256
    );
}

#[test]
fn cluster_views_visit_only_their_own_clusters() {
    let root = ParagraphStyle::default();
    let mut b = ParagraphBuilder::new(&root, &Limits::default());
    for n in 0..256 {
        b.push_text(TextSource::Generated { node: NodeId(n) }, "a");
    }
    let p = b
        .build(
            &mut LayoutContext::new(),
            &crate::font::FontCollection::new(&Limits::default()),
        )
        .unwrap();
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        10000.0,
        &AtomicSizes::EMPTY,
    );
    let count: usize = lines[0]
        .fragments()
        .filter_map(|f| match f {
            Fragment::GlyphRun(r) => Some(r.clusters().count()),
            _ => None,
        })
        .sum();
    assert_eq!(count, 256);
    assert_eq!(
        p.data
            .cluster_queries
            .load(std::sync::atomic::Ordering::Relaxed),
        256
    );
}

fn final_review_fonts() -> crate::font::FontCollection {
    crate::font::FontCollection::with_options(
        &Limits::default(),
        crate::font::FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    )
}

#[test]
fn final_review_first_line_empty_siblings_use_bounded_cursor_comparisons() {
    let mut style = ParagraphStyle::default();
    style.first_line = Some(style.root.clone());
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    let count = 2000;
    for n in 0..count {
        b.open_inline(NodeId(n), &style.root, Default::default())
            .close_inline();
    }
    b.push_text(
        TextSource::Generated {
            node: NodeId(count),
        },
        "a",
    );
    let p = b
        .build(&mut LayoutContext::new(), &final_review_fonts())
        .unwrap();
    let visits = p
        .data
        .cursor_queries
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        visits <= p.data.units.len() * 4,
        "{visits} cursor comparisons for {} units",
        p.data.units.len()
    );
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &Default::default(),
        100.0,
        &AtomicSizes::EMPTY,
    );
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].text_range(), 0..1);
}

#[test]
fn final_review_giant_grapheme_reuses_identical_font_queries_across_styles() {
    use crate::analysis::itemize::MATCH_CALLS;
    for query_mode in [0, 1, 2, 3, 4] {
        let mut style = ParagraphStyle::default();
        if query_mode == 1 {
            style.root.font_weight = 300.0;
        } else if query_mode == 2 {
            style.root.font_weight = 2000.0;
        } else if query_mode == 3 {
            style.root.font_style = crate::style::FontStyle::Oblique(120.0);
        } else if query_mode == 4 {
            style.root.font_width = -10.0;
        }
        let mut tracked = style.root.clone();
        tracked.letter_spacing = 1.0;
        if query_mode == 1 {
            tracked.font_weight = 700.0;
        } else if query_mode == 2 {
            tracked.font_weight = 3000.0;
        } else if query_mode == 3 {
            tracked.font_style = crate::style::FontStyle::Oblique(170.0);
        } else if query_mode == 4 {
            tracked.font_width = -20.0;
        }
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(0) }, "a");
        for n in 1..=4096 {
            b.open_inline(
                NodeId(n),
                if n % 2 == 0 { &style.root } else { &tracked },
                Default::default(),
            )
            .push_text(TextSource::Generated { node: NodeId(n) }, "\u{301}")
            .close_inline();
        }
        MATCH_CALLS.with(|calls| calls.set(0));
        let p = b
            .build(&mut LayoutContext::new(), &final_review_fonts())
            .unwrap();
        let calls = MATCH_CALLS.with(|calls| calls.get());
        assert_eq!(
            calls,
            if query_mode == 1 { 2 } else { 1 },
            "queries matched once per grapheme"
        );
        assert_eq!(p.text().len(), 8193);
    }
}

#[test]
fn final_review_zero_window_bounds_unsafe_chain_expansion() {
    let fonts = final_review_fonts();
    fonts
        .register(crate::test_support::fonts::ARABIC.to_vec())
        .unwrap();
    for budget in [0, 4, 16] {
        let limits = Limits {
            max_reshape_window_bytes: Some(budget),
            ..Default::default()
        };
        let mut style = ParagraphStyle::default();
        style.root.text_wrap_mode = crate::style::TextWrapMode::NoWrap;
        for count in [128, 256] {
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.push_text(
                TextSource::Generated { node: NodeId(1) },
                &"ب".repeat(count),
            );
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            assert!(p.data.units.iter().any(|u| u.unsafe_to_break));
            p.data
                .window_queries
                .store(0, std::sync::atomic::Ordering::Relaxed);
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1.0,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].text_range(), 0..count * 2);
            let visits = p
                .data
                .window_queries
                .load(std::sync::atomic::Ordering::Relaxed);
            assert!(
                visits <= count * (budget as usize / 2 + 4),
                "{visits} expansion visits for {count} letters"
            );
        }
    }
}

/// Cursive-joining scripts (Arabic) mark nearly every letter
/// `unsafe_to_break`/`unsafe_to_concat`, and that flag also sits on spaces
/// between words, so the safe (`unsafe_to_break == false`) boundaries a
/// backward scan can stop at are rare (sentence punctuation only). `scan()`
/// used to re-measure a hypothetical "line ended here" edge window (a real,
/// uncached harfrust shaping call) at *every* unit as it advances, including
/// the ~90% that fall at a `Prohibited` break position where the line
/// cannot end there anyway. Real break opportunities are a small fraction
/// of the characters in continuous-script text, so a correctly bounded scan
/// needs far fewer real shape calls than there are characters.
#[test]
fn final_review_prohibited_positions_do_not_reshape_unsafe_edge_windows() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::ARABIC.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let sentence = "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} \u{0628}\u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}. \u{0627}\u{0644}\u{0643}\u{062a}\u{0627}\u{0628}\u{0629} \u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629} \u{062c}\u{0645}\u{064a}\u{0644}\u{0629}\u{060c} \u{0648}\u{062a}\u{062a}\u{0635}\u{0644} \u{0627}\u{0644}\u{062d}\u{0631}\u{0648}\u{0641} \u{0641}\u{064a} \u{0627}\u{0644}\u{0643}\u{0644}\u{0645}\u{0627}\u{062a}.";
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Arabic".into(),
    )];
    style.root.font_size = 16.0;
    for reps in [4usize, 8, 16, 32] {
        let text: String = vec![sentence; reps].join(" ");
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        p.data
            .edge_shape_calls
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            320.0,
            &AtomicSizes::EMPTY,
        );
        assert!(!lines.is_empty());
        let shape_calls = p
            .data
            .edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed);
        let chars = text.chars().count();
        assert!(
            shape_calls < chars,
            "{shape_calls} edge-window shape calls for {chars} characters at reps={reps}; \
             a bounded scan needs far fewer real shape calls than characters, since most \
             positions are Prohibited break positions where the line cannot end"
        );
    }
}

/// A soft hyphen is a break candidate only with its hyphen, so a scan used to
/// shape both the hyphenated and the unhyphenated edge at every soft hyphen
/// it passed, though only the last fitting one can end the line. Wide lines
/// of densely hyphenated text must shape a few edges per line, not several
/// per soft hyphen.
#[test]
fn soft_hyphens_are_measured_only_near_the_line_end() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Latin".into(),
    )];
    style.root.font_size = 16.0;
    let word = "hy\u{ad}phen\u{ad}ation par\u{ad}a\u{ad}graph mea\u{ad}sure\u{ad}ment";
    let text = vec![word; 40].join(" ");
    let layout = |text: &str| {
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, text);
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        p.data
            .edge_shape_calls
            .store(0, std::sync::atomic::Ordering::Relaxed);
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &LineOptions::default(),
            437.0,
            &AtomicSizes::EMPTY,
        );
        let calls = p
            .data
            .edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed);
        (lines, calls)
    };
    let (lines, hyphenated) = layout(&text);
    let (_, plain) = layout(&text.replace('\u{ad}', ""));
    assert!(lines.len() > 4);
    assert!(
        lines
            .iter()
            .any(|l| l.text()[l.text_range()].ends_with('\u{ad}')),
        "the fixture must take at least one hyphen break"
    );
    // An eager scan made 2515 calls here against 398 without soft hyphens.
    assert!(
        hyphenated < 3 * plain,
        "{hyphenated} edge-window shape calls with soft hyphens, {plain} without"
    );
}

/// A line holding more deferred candidates than the scan retains measures
/// the oldest ones early. It must still end at the last fitting hyphen, not
/// at an early one that was measured to bound the retained state.
#[test]
fn soft_hyphens_beyond_the_deferred_bound_still_break_late() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Latin".into(),
    )];
    style.root.font_size = 16.0;
    let count = 3 * super::scan::DEFERRED_MAX;
    let text = "ab\u{ad}".repeat(count);
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let whole = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        1.0e6,
        &AtomicSizes::EMPTY,
    );
    let width = whole[0].inline_size() * 2.0 / 3.0;
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        width,
        &AtomicSizes::EMPTY,
    );
    let first = &lines[0];
    let taken = first.text()[first.text_range()].matches('\u{ad}').count();
    assert!(first.text()[first.text_range()].ends_with('\u{ad}'));
    assert!(first.inline_size() <= width);
    // About two thirds of the hyphens fit; the bound is a third of them.
    assert!(
        taken > 3 * count / 5,
        "took {taken} of {count} hyphens at {width}px"
    );
}

/// Every allowed break candidate on a line used to reshape the same handful of
/// edge windows again (a 959-character Latin paragraph made 482 shape calls for
/// 41 distinct windows). Identical windows must be shaped once per context.
#[test]
fn edge_window_shapes_are_reused_across_candidates_and_relayouts() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Latin".into(),
    )];
    style.root.font_size = 16.0;
    let words = 120usize;
    let text = vec!["Typography affects readability of wave forms"; words / 6].join(" ");
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let calls = |p: &crate::Paragraph| {
        p.data
            .edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    let mut cx = LayoutContext::new();
    let summary = |lines: &[crate::Line]| {
        lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size().to_bits()))
            .collect::<Vec<_>>()
    };
    let first = p.break_all(&mut cx, &LineOptions::default(), 200.0, &AtomicSizes::EMPTY);
    let first_calls = calls(&p);
    let second = p.break_all(&mut cx, &LineOptions::default(), 200.0, &AtomicSizes::EMPTY);
    assert_eq!(summary(&first), summary(&second));
    assert_eq!(calls(&p), first_calls, "re-layout reshaped cached windows");
    // HarfRust 0.14 also marks unsuccessful pair-positioning matches unsafe
    // to concatenate, so more distinct windows are needed than in 0.12.
    // Check the cache invariant directly instead of assuming a window count.
    assert!(first_calls > 0, "fixture did not exercise edge shaping");
    assert_eq!(
        first_calls,
        cx.edge_shapes.len(),
        "a distinct edge window was shaped more than once"
    );
}

/// `shrink_to` is the documented way to release a context's retained memory;
/// the reshaped-window cache must go with it.
#[test]
fn shrink_to_releases_the_edge_window_cache() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Latin".into(),
    )];
    style.root.font_size = 16.0;
    let text = vec!["Typography affects readability of wave forms"; 20].join(" ");
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut cx = LayoutContext::new();
    p.break_all(&mut cx, &LineOptions::default(), 200.0, &AtomicSizes::EMPTY);
    assert!(
        cx.edge_shapes.len() > 0,
        "the layout should have cached windows"
    );
    cx.shrink_to(usize::MAX);
    assert_eq!(cx.edge_shapes.len(), 0, "shrink_to must release the cache");
}

fn arabic_fonts() -> crate::font::FontCollection {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::ARABIC.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Arabic".into(),
                ..Default::default()
            },
        )
        .unwrap();
    fonts
}

fn arabic_paragraph(limits: Limits, reps: usize) -> crate::Paragraph {
    let sentence = "\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} \u{0628}\u{0627}\u{0644}\u{0639}\u{0627}\u{0644}\u{0645}. \u{0627}\u{0644}\u{0643}\u{062a}\u{0627}\u{0628}\u{0629} \u{0627}\u{0644}\u{0639}\u{0631}\u{0628}\u{064a}\u{0629} \u{062c}\u{0645}\u{064a}\u{0644}\u{0629}\u{060c}";
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Arabic".into(),
    )];
    style.root.font_size = 16.0;
    style.root.word_break = crate::style::WordBreak::BreakAll;
    let text = vec![sentence; reps].join(" ");
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    b.build(&mut LayoutContext::new(), &arabic_fonts()).unwrap()
}

fn break_all_arabic(
    p: &crate::Paragraph,
    width: f32,
    cx: &mut LayoutContext,
) -> (Vec<(std::ops::Range<usize>, u32)>, Vec<String>) {
    let lines = p.break_all(cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
    let summary = lines
        .iter()
        .map(|l| (l.text_range(), l.inline_size().to_bits()))
        .collect();
    (
        summary,
        cx.take_warnings().into_iter().map(|w| w.message).collect(),
    )
}

/// Each edge window is bounded, but a long unsafe-joined line could ask for
/// one per break candidate. Once a fitting break is known, the scan defers
/// the edge reshape of later candidates and measures only the ones it needs
/// when the line ends, so a line that fits whole stays far under the per-line
/// budget. The result must not depend on what the cache already holds.
#[test]
fn edge_reshape_work_per_line_is_deferred_and_deterministic() {
    let limits = Limits::default();
    let paragraph = arabic_paragraph(limits, 24);
    let mut cold = LayoutContext::new();
    let (lines, warnings) = break_all_arabic(&paragraph, 200_000.0, &mut cold);
    assert!(
        !warnings.iter().any(|m| m.contains("edge reshape budget")),
        "deferred candidates must not exhaust the per-line reshape budget; got {warnings:?}"
    );
    assert!(!lines.is_empty());
    // The same layout with a warm cache is identical: the budget charges
    // requests, not cache misses.
    let mut warm = LayoutContext::new();
    let first = break_all_arabic(&paragraph, 200_000.0, &mut warm);
    assert!(
        warm.edge_shapes.len() > 0,
        "second layout must start with cached windows"
    );
    let second = break_all_arabic(&paragraph, 200_000.0, &mut warm);
    assert_eq!(first.0, second.0);
    assert_eq!(first.0, lines);
    assert_eq!(first.1, second.1);
    assert_eq!(first.1, warnings);
}

#[test]
fn ordinary_text_stays_under_the_edge_reshape_budget() {
    let mut cx = LayoutContext::new();
    let (lines, warnings) =
        break_all_arabic(&arabic_paragraph(Limits::default(), 24), 320.0, &mut cx);
    assert!(lines.len() > 1);
    assert!(
        !warnings.iter().any(|m| m.contains("edge reshape budget")),
        "default limits must not trip the budget on ordinary wrapping: {warnings:?}"
    );
}

/// A long paragraph used to fill the glyph cache near its beginning, forcing
/// every later candidate through shaping again on every repeated layout.
#[test]
fn long_arabic_relayout_reuses_width_measurements_after_glyph_retention_fills() {
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Arabic".into(),
    )];
    style.root.font_size = 16.0;
    style.root.lang = Some("ar".into());
    style.direction = crate::geometry::Direction::Rtl;
    let text = vec![
        "مرحبا بالعالم. الكتابة العربية جميلة، وتتصل الحروف في الكلمات.";
        12 * 64
    ]
    .join(" ");
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(TextSource::Generated { node: NodeId(1) }, &text);
    let paragraph = builder
        .build(&mut LayoutContext::new(), &arabic_fonts())
        .unwrap();
    let calls = || {
        paragraph
            .data
            .edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    };
    let mut warm = LayoutContext::new();
    let before = calls();
    let first = break_all_arabic(&paragraph, 420.0, &mut warm);
    let cold_calls = calls() - before;
    assert!(cold_calls > 100, "fixture must exercise a long edge scan");
    let before = calls();
    let second = break_all_arabic(&paragraph, 420.0, &mut warm);
    let warm_calls = calls() - before;
    assert_eq!(first, second);
    eprintln!("cache {:?}", warm.edge_shapes);
    assert!(
        warm_calls < cold_calls / 3,
        "warm layout reshaped {warm_calls} windows after {cold_calls} cold calls"
    );
    for width in [240.0, 480.0, 320.0] {
        let before = calls();
        let changed = break_all_arabic(&paragraph, width, &mut warm);
        let changed_calls = calls() - before;
        let before = calls();
        let fresh = break_all_arabic(&paragraph, width, &mut LayoutContext::new());
        let fresh_calls = calls() - before;
        assert_eq!(changed, fresh, "changed width {width}");
        let before = calls();
        let repeated = break_all_arabic(&paragraph, width, &mut warm);
        let repeated_calls = calls() - before;
        assert_eq!(repeated, fresh, "repeated width {width}");
        assert!(
            repeated_calls <= changed_calls,
            "repeat at {width}: {repeated_calls} vs changed {changed_calls}"
        );
        eprintln!(
            "Arabic/64 width={width} cold={fresh_calls} changed={changed_calls} repeated={repeated_calls} cache={:?}",
            warm.edge_shapes
        );
    }
    eprintln!("Arabic/64 fixed cold={cold_calls} warm={warm_calls}");
}

#[test]
fn edge_measurements_follow_the_owner_across_many_short_paragraphs() {
    use std::sync::atomic::Ordering;
    let mut cx = LayoutContext::new();
    for index in 0..24 {
        let paragraph = arabic_paragraph(Limits::default(), 1 + index % 3);
        let width = 120.0 + 10.0 * index as f32;
        let first = break_all_arabic(&paragraph, width, &mut cx);
        let fresh = break_all_arabic(&paragraph, width, &mut LayoutContext::new());
        assert_eq!(first, fresh, "paragraph {index}");
        let before = paragraph.data.edge_shape_calls.load(Ordering::Relaxed);
        assert_eq!(break_all_arabic(&paragraph, width, &mut cx), fresh);
        assert_eq!(
            paragraph.data.edge_shape_calls.load(Ordering::Relaxed),
            before,
            "paragraph {index} should retain all short windows"
        );
    }
    cx.shrink_to(usize::MAX);
    assert_eq!(cx.edge_shapes.len(), 0);
}

#[derive(Clone, Copy, Debug)]
enum Terminator {
    End,
    Forced,
    BlockInInline,
}

/// Kerned Latin text in RTL-override spans, every position a soft break,
/// followed by the spans' ends and `terminator`. Shaped in visual order, a
/// pair's kerning sits on its logically second glyph, so a line that starts
/// after a cut through the pair loses it: every candidate on that line is
/// wider once its edges are reshaped than its shared advances say.
fn closing_units_before(terminator: Terminator, nested: usize, isolate: bool) -> crate::Paragraph {
    closing_units_with(
        terminator,
        nested,
        isolate,
        (
            InlineEdges::default(),
            crate::style::BoxDecorationBreak::Slice,
        ),
        "",
        crate::style::WhiteSpaceCollapse::Collapse,
        false,
    )
}

fn closing_units_with(
    terminator: Terminator,
    nested: usize,
    isolate: bool,
    (edges, decoration): (InlineEdges, crate::style::BoxDecorationBreak),
    trailing: &str,
    collapse: crate::style::WhiteSpaceCollapse,
    trailing_inside: bool,
) -> crate::Paragraph {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named(
        "Shodo Fixture Latin".into(),
    )];
    style.root.font_size = 16.0;
    style.root.white_space_collapse = collapse;
    let mut rtl = style.root.clone();
    rtl.box_decoration_break = decoration;
    rtl.direction = crate::geometry::Direction::Rtl;
    rtl.unicode_bidi = if isolate {
        crate::style::UnicodeBidi::IsolateOverride
    } else {
        crate::style::UnicodeBidi::BidiOverride
    };
    let mut b = ParagraphBuilder::new(&style, &Limits::default());
    b.with_line_break_override(|_| crate::LineBreakOverride::Allow);
    b.open_inline(NodeId(2), &rtl, edges);
    for n in 0..nested {
        b.open_inline(NodeId(10 + n as u64), &rtl, edges);
    }
    b.push_text(TextSource::Generated { node: NodeId(3) }, "VAVAVAVAVAVA");
    if trailing_inside {
        b.push_text(TextSource::Generated { node: NodeId(6) }, trailing);
    }
    for _ in 0..=nested {
        b.close_inline();
    }
    if !trailing_inside {
        b.push_text(TextSource::Generated { node: NodeId(6) }, trailing);
    }
    match terminator {
        Terminator::End => &mut b,
        Terminator::Forced => b.push_forced_break(NodeId(4)),
        Terminator::BlockInInline => b.push_block_in_inline(NodeId(4)),
    };
    if !matches!(terminator, Terminator::End) {
        b.push_text(TextSource::Generated { node: NodeId(5) }, "x");
    }
    b.build(&mut LayoutContext::new(), &fonts).unwrap()
}

/// A deferred break candidate followed only by box ends and bidi controls
/// before a forced break or block-in-inline is still where the line ends, so
/// its reshaped edge must fit. Each line here starts after a cut through a
/// kerned pair and is wider than its shared advances; the last text line has
/// to fall back to an earlier verified break instead of overflowing.
#[test]
fn deferred_break_before_closing_units_is_checked_at_the_line_end() {
    for terminator in [Terminator::Forced, Terminator::BlockInInline] {
        for (nested, isolate) in [(0, false), (2, false), (0, true), (2, true)] {
            let p = closing_units_before(terminator, nested, isolate);
            for width in [28.125, 28.25, 28.625, 28.75] {
                let lines = p.break_all(
                    &mut LayoutContext::new(),
                    &LineOptions::default(),
                    width,
                    &AtomicSizes::EMPTY,
                );
                let case = format!("{terminator:?} nested={nested} isolate={isolate} at {width}px");
                for line in &lines {
                    assert!(
                        line.inline_size() <= width,
                        "{case}: line {:?} is {}px wide",
                        line.text_range(),
                        line.inline_size()
                    );
                }
                let (last, before) = lines.split_last().unwrap();
                let (terminal, regular) = before.split_last().unwrap();
                assert_eq!(
                    last.text_range(),
                    p.text().len() - 1..p.text().len(),
                    "{case}"
                );
                assert_eq!(
                    terminal.break_reason(),
                    match terminator {
                        Terminator::End => unreachable!(),
                        Terminator::Forced => BreakReason::Forced,
                        Terminator::BlockInInline => BreakReason::BlockInInline,
                    },
                    "{case}"
                );
                assert!(
                    regular
                        .iter()
                        .all(|l| l.break_reason() == BreakReason::Regular),
                    "{case}"
                );
                // Every box end and its closing controls stay on the line
                // that ends the span's text.
                let closers = |text: &str| {
                    text.chars()
                        .filter(|c| matches!(c, '\u{202c}' | '\u{2069}'))
                        .count()
                };
                assert_eq!(
                    closers(&p.text()[terminal.text_range()]),
                    closers(p.text()),
                    "{case}"
                );
                assert_eq!(closers(p.text()), (nested + 1) * (1 + isolate as usize));
                let boxes = |line: &crate::Line| {
                    line.fragments()
                        .filter(|f| matches!(f, Fragment::InlineBox(_)))
                        .count()
                };
                assert_eq!(boxes(terminal), nested + 1, "{case}");
                assert_eq!(boxes(last), 0, "{case}");
            }
        }
    }
}

/// The paragraph end must include the line-start reshape delta even when
/// its last units prohibit a break. Closing bidi controls and inline boxes
/// stay with the final text rather than hiding the overflowing last line.
#[test]
fn paragraph_end_checks_the_reshaped_line_start_width() {
    for (nested, isolate) in [(0, false), (2, false), (0, true), (2, true)] {
        let p = closing_units_before(Terminator::End, nested, isolate);
        for width in [28.125, 28.25, 28.625, 28.75] {
            let mut cx = LayoutContext::new();
            let lines = p.break_all(&mut cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
            let case = format!("nested={nested} isolate={isolate} at {width}px");
            for line in &lines {
                assert!(
                    line.inline_size() <= width,
                    "{case}: line {:?} is {}px wide",
                    line.text_range(),
                    line.inline_size()
                );
            }
            let (last, before) = lines.split_last().unwrap();
            assert_eq!(last.break_reason(), BreakReason::End, "{case}");
            assert_eq!(last.text_range().end, p.text().len(), "{case}");
            assert!(
                p.text()[last.text_range()]
                    .trim_end_matches(['\u{202c}', '\u{2069}'])
                    .ends_with('A'),
                "{case}"
            );
            assert!(
                before
                    .iter()
                    .all(|line| line.break_reason() == BreakReason::Regular),
                "{case}"
            );
            let closers = |text: &str| {
                text.chars()
                    .filter(|c| matches!(c, '\u{202c}' | '\u{2069}'))
                    .count()
            };
            assert_eq!(
                closers(&p.text()[last.text_range()]),
                (nested + 1) * (1 + isolate as usize),
                "{case}"
            );
            assert_eq!(
                last.fragments()
                    .filter(|f| matches!(f, Fragment::InlineBox(_)))
                    .count(),
                nested + 1,
                "{case}"
            );
            assert!(cx.take_warnings().is_empty(), "{case}");
        }
    }
}

#[test]
fn paragraph_end_checks_visible_width_before_hanging_whitespace() {
    for collapse in [
        crate::style::WhiteSpaceCollapse::Collapse,
        crate::style::WhiteSpaceCollapse::Preserve,
    ] {
        for trailing in [" ", "\t"] {
            let p = closing_units_with(
                Terminator::End,
                0,
                false,
                (
                    InlineEdges::default(),
                    crate::style::BoxDecorationBreak::Slice,
                ),
                trailing,
                collapse,
                false,
            );
            for width in [28.125, 28.25, 28.625, 28.75] {
                let mut cx = LayoutContext::new();
                let lines =
                    p.break_all(&mut cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
                for line in &lines {
                    assert!(
                        line.inline_size() <= width,
                        "{collapse:?} trailing={trailing:?} width={width}: {:?} is {}px wide",
                        line.text_range(),
                        line.inline_size()
                    );
                }
                assert_eq!(lines.last().unwrap().break_reason(), BreakReason::End);
                assert_eq!(lines.last().unwrap().text_range().end, p.text().len());
                assert!(cx.take_warnings().is_empty());
            }
        }
    }
}

#[test]
fn paragraph_end_checks_pulled_closing_edge_widths() {
    for nested in [0, 2] {
        let mut edges = InlineEdges::default();
        edges.padding.inline_end = 1.0;
        edges.border.inline_end = 0.5;
        let p = closing_units_with(
            Terminator::End,
            nested,
            false,
            (edges, crate::style::BoxDecorationBreak::Slice),
            " ",
            crate::style::WhiteSpaceCollapse::Collapse,
            true,
        );
        for width in [16.0, 28.125, 28.25, 28.625, 28.75, 30.0, 48.0] {
            let mut cx = LayoutContext::new();
            let lines = p.break_all(&mut cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
            for line in &lines {
                assert!(
                    line.inline_size() <= width,
                    "nested={nested} width={width}: {:?} ({:?}) is {}px wide",
                    line.text_range(),
                    line.break_reason(),
                    line.inline_size()
                );
            }
            let last = lines.last().unwrap();
            assert_eq!(last.break_reason(), BreakReason::End);
            assert_eq!(last.text_range().end, p.text().len());
            assert_eq!(
                last.fragments()
                    .filter(|f| matches!(f, Fragment::InlineBox(_)))
                    .count(),
                nested + 1
            );
            assert!(cx.take_warnings().is_empty());
        }
    }
}

#[test]
fn paragraph_end_checks_closing_edges_before_zero_width_space() {
    for decoration in [
        crate::style::BoxDecorationBreak::Slice,
        crate::style::BoxDecorationBreak::Clone,
    ] {
        let mut edges = InlineEdges::default();
        edges.padding.inline_end = 1.0;
        edges.border.inline_end = 0.5;
        let p = closing_units_with(
            Terminator::End,
            0,
            false,
            (edges, decoration),
            "\u{200b}",
            crate::style::WhiteSpaceCollapse::Collapse,
            false,
        );
        // A line at its natural width must still fit in one line: reserving
        // Clone's closing edges twice would force an unnecessary break.
        let mut cx = LayoutContext::new();
        let natural = p.break_all(
            &mut cx,
            &LineOptions::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(natural.len(), 1);
        let exact = p.break_all(
            &mut cx,
            &LineOptions::default(),
            natural[0].inline_size(),
            &AtomicSizes::EMPTY,
        );
        assert_eq!(exact.len(), 1, "{decoration:?}");
        assert_eq!(exact[0].text_range(), 0..p.text().len());
        assert!(cx.take_warnings().is_empty());
        for width in [19.171875, 28.125, 28.625, 29.875] {
            let mut cx = LayoutContext::new();
            let lines = p.break_all(&mut cx, &LineOptions::default(), width, &AtomicSizes::EMPTY);
            for line in &lines {
                assert!(
                    line.inline_size() <= width,
                    "width={width}: {:?} ({:?}) is {}px wide",
                    line.text_range(),
                    line.break_reason(),
                    line.inline_size()
                );
            }
            assert_eq!(lines.last().unwrap().break_reason(), BreakReason::End);
            assert!(cx.take_warnings().is_empty());
        }
    }
}

#[test]
fn paragraph_end_keeps_closing_edges_before_hanging_whitespace() {
    let fonts = final_review_fonts();
    for (bytes, family) in [
        (crate::test_support::fonts::LATIN, "Shodo Fixture Latin"),
        (crate::test_support::fonts::CJK, "Shodo Fixture CJK"),
    ] {
        fonts
            .register_face(
                bytes.to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: family.into(),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    for text in ["、", ",", ")"] {
        for collapse in [
            crate::style::WhiteSpaceCollapse::Collapse,
            crate::style::WhiteSpaceCollapse::Preserve,
        ] {
            let mut style = ParagraphStyle::default();
            style.root.font_families = vec![
                crate::style::FontFamily::Named("Shodo Fixture Latin".into()),
                crate::style::FontFamily::Named("Shodo Fixture CJK".into()),
            ];
            style.root.font_size = 16.0;
            style.root.white_space_collapse = collapse;
            let mut rtl = style.root.clone();
            rtl.direction = crate::geometry::Direction::Rtl;
            rtl.unicode_bidi = crate::style::UnicodeBidi::BidiOverride;
            let mut edges = InlineEdges::default();
            edges.padding.inline_end = 1.0;
            edges.border.inline_end = 0.5;
            let mut b = ParagraphBuilder::new(&style, &Limits::default());
            b.with_line_break_override(|_| crate::LineBreakOverride::Allow);
            b.open_inline(NodeId(2), &rtl, InlineEdges::default());
            b.push_text(TextSource::Generated { node: NodeId(3) }, "VAVAVAVAVAVA");
            b.close_inline();
            b.open_inline(NodeId(5), &style.root, edges);
            b.push_text(TextSource::Generated { node: NodeId(6) }, text);
            b.close_inline();
            b.push_text(TextSource::Generated { node: NodeId(4) }, " ");
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            for force_end in [false, true] {
                let options = LineOptions {
                    hanging_punctuation: crate::style::HangingPunctuation {
                        force_end,
                        last: force_end,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                for width in [24.3125, 24.9375, 24.953125, 28.125, 28.625, 29.875, 48.0] {
                    let mut cx = LayoutContext::new();
                    let lines = p.break_all(&mut cx, &options, width, &AtomicSizes::EMPTY);
                    for line in &lines {
                        assert!(
                            line.inline_size() <= width,
                            "{text:?} {collapse:?} force_end={force_end} width={width}: {:?} ({:?}) is {}px wide",
                            line.text_range(),
                            line.break_reason(),
                            line.inline_size()
                        );
                    }
                    assert_eq!(lines.last().unwrap().break_reason(), BreakReason::End);
                    assert_eq!(lines.last().unwrap().text_range().end, p.text().len());
                    assert!(cx.take_warnings().is_empty());
                }
            }
        }
    }
}

fn assert_narrow_retries_match_fresh(p: &crate::Paragraph, width: f32) {
    let options = LineOptions::default();
    let mut token = p.start_token();
    loop {
        let crate::LineResult::Line(fresh) = p.next_line(
            &mut LayoutContext::new(),
            token,
            &options,
            &crate::LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("fresh line at {width}px");
        };
        let mut cx = LayoutContext::new();
        assert!(matches!(
            p.next_line(
                &mut cx,
                token,
                &options,
                &crate::LineConstraint::new(1000.0),
                &AtomicSizes::EMPTY,
            ),
            crate::LineResult::Line(_)
        ));
        assert!(cx.partial.is_some());
        let scans = cx.cache_visits;
        let crate::LineResult::Line(retry) = p.next_line(
            &mut cx,
            token,
            &options,
            &crate::LineConstraint::new(width),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("cached retry at {width}px");
        };
        assert!(
            retry.inline_size() <= width,
            "{width}px: {:?} is {}px",
            retry.text_range(),
            retry.inline_size()
        );
        assert_eq!(retry.text_range(), fresh.text_range(), "{width}px");
        assert_eq!(retry.inline_size(), fresh.inline_size(), "{width}px");
        assert_eq!(retry.break_reason(), fresh.break_reason(), "{width}px");
        assert_eq!(retry.break_token(), fresh.break_token());
        assert_eq!(cx.cache_visits, scans, "retry must reuse its scan");
        assert!(cx.cache_prepare_visits > 0);
        assert!(cx.take_warnings().is_empty());
        if fresh.break_reason() == BreakReason::End {
            break;
        }
        token = fresh.break_token();
    }
}

#[test]
fn paragraph_end_retries_check_hanging_space_width() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for collapse in [
        crate::style::WhiteSpaceCollapse::Collapse,
        crate::style::WhiteSpaceCollapse::Preserve,
    ] {
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![crate::style::FontFamily::Named(
            "Shodo Fixture Latin".into(),
        )];
        style.root.font_size = 16.0;
        style.root.white_space_collapse = collapse;
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.with_line_break_override(|_| crate::LineBreakOverride::Allow);
        b.push_text(
            TextSource::Generated { node: NodeId(1) },
            "\u{202e}VAVAVAVAVAVA ",
        );
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        for width in [28.125, 28.25, 28.625, 28.75] {
            assert_narrow_retries_match_fresh(&p, width);
        }
    }
}

#[test]
fn paragraph_end_retries_check_pulled_closing_edges() {
    for decoration in [
        crate::style::BoxDecorationBreak::Slice,
        crate::style::BoxDecorationBreak::Clone,
    ] {
        let mut edges = InlineEdges::default();
        edges.padding.inline_end = 1.0;
        edges.border.inline_end = 0.5;
        let p = closing_units_with(
            Terminator::End,
            0,
            false,
            (edges, decoration),
            "\u{200b}",
            crate::style::WhiteSpaceCollapse::Collapse,
            false,
        );
        for width in [19.171875, 28.125, 28.625, 29.875] {
            assert_narrow_retries_match_fresh(&p, width);
        }
    }
}

#[test]
fn paragraph_end_excludes_controls_after_hanging_whitespace() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for collapse in [
        crate::style::WhiteSpaceCollapse::Collapse,
        crate::style::WhiteSpaceCollapse::Preserve,
    ] {
        for trailing in [" ", "\t"] {
            for nested in [0, 1, 2] {
                for zero_space in [false, true] {
                    for isolate in [false, true] {
                        let mut style = ParagraphStyle::default();
                        style.root.font_families =
                            vec![crate::style::FontFamily::Named("Shodo Fixture CJK".into())];
                        style.root.font_size = 16.0;
                        style.root.white_space_collapse = collapse;
                        if zero_space {
                            style.root.word_spacing_percent = -100.0;
                        }
                        let mut inline = style.root.clone();
                        if isolate {
                            inline.unicode_bidi = crate::style::UnicodeBidi::Isolate;
                        }
                        let mut b = ParagraphBuilder::new(&style, &Limits::default());
                        for n in 0..=nested {
                            b.open_inline(NodeId(10 + n), &inline, InlineEdges::default());
                        }
                        b.push_text(TextSource::Generated { node: NodeId(3) }, "ああ、");
                        b.push_text(TextSource::Generated { node: NodeId(4) }, trailing);
                        for _ in 0..=nested {
                            b.close_inline();
                        }
                        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
                        let options = LineOptions {
                            hanging_punctuation: crate::style::HangingPunctuation {
                                force_end: true,
                                ..Default::default()
                            },
                            ..Default::default()
                        };
                        let mut cx = LayoutContext::new();
                        let crate::LineResult::Line(wide) = p.next_line(
                            &mut cx,
                            p.start_token(),
                            &options,
                            &crate::LineConstraint::new(1000.0),
                            &AtomicSizes::EMPTY,
                        ) else {
                            panic!("wide punctuation line");
                        };
                        let crate::LineResult::Line(fresh) = p.next_line(
                            &mut LayoutContext::new(),
                            p.start_token(),
                            &options,
                            &crate::LineConstraint::new(32.0),
                            &AtomicSizes::EMPTY,
                        ) else {
                            panic!("fresh punctuation line");
                        };
                        assert_eq!(
                            fresh.text_range(),
                            wide.text_range(),
                            "{collapse:?} {trailing:?} nested={nested} zero_space={zero_space} isolate={isolate}"
                        );
                        assert_eq!(fresh.break_reason(), BreakReason::End);
                        assert_eq!(fresh.inline_size(), 32.0);
                        let scans = cx.cache_visits;
                        let crate::LineResult::Line(retry) = p.next_line(
                            &mut cx,
                            p.start_token(),
                            &options,
                            &crate::LineConstraint::new(32.0),
                            &AtomicSizes::EMPTY,
                        ) else {
                            panic!("cached punctuation line");
                        };
                        assert_eq!(retry.text_range(), fresh.text_range());
                        assert_eq!(retry.inline_size(), fresh.inline_size());
                        assert_eq!(retry.break_reason(), fresh.break_reason());
                        if trailing == " " {
                            assert_eq!(cx.cache_visits, scans);
                            assert!(cx.cache_prepare_visits > 0);
                        }
                        assert!(cx.take_warnings().is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn hanging_tail_retains_preserved_space_before_outer_closing_edges() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::CJK.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture CJK".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![crate::style::FontFamily::Named("Shodo Fixture CJK".into())];
    style.root.font_size = 16.0;
    let mut preserved = style.root.clone();
    preserved.white_space_collapse = crate::style::WhiteSpaceCollapse::Preserve;
    preserved.unicode_bidi = crate::style::UnicodeBidi::Isolate;
    let mut outer = style.root.clone();
    outer.unicode_bidi = crate::style::UnicodeBidi::Isolate;
    for end_edge in [0.0, 1.5] {
        let mut edges = InlineEdges::default();
        edges.padding.inline_end = end_edge;
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.open_inline(NodeId(2), &outer, edges);
        b.open_inline(NodeId(3), &preserved, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(4) }, "ああ、 ");
        b.close_inline();
        b.push_text(TextSource::Generated { node: NodeId(5) }, " ");
        b.close_inline();
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        let options = LineOptions {
            hanging_punctuation: crate::style::HangingPunctuation {
                force_end: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut sat = crate::geometry::Saturation::default();
        let wide = super::scan::scan(
            &p.data,
            0,
            crate::geometry::LayoutUnit::from_f32_round(1000.0, &mut sat),
            crate::geometry::LayoutUnit::ZERO,
            crate::geometry::LayoutUnit::ZERO,
            p.start_token().flags,
            &options,
            &AtomicSizes::EMPTY,
            None,
            None,
            &mut LayoutContext::new(),
            &mut sat,
        );
        let mut cursor = super::spacing_summary::Cursor::default();
        let mut tail = super::punctuation::HangingTail::default();
        for at in 0..p.data.units.len() {
            tail.observe(
                &p.data,
                at,
                &cursor,
                super::whitespace::fits_hanging(&p.data, at),
                super::whitespace::transparent(&p.data, at),
            );
            super::spacing::push(&p.data, &mut cursor, at);
            let selected = tail
                .summary(&p.data, at + 1)
                .unwrap_or_else(|| cursor.summary(Some(&p.data)));
            // Compare each retained boundary with the existing boundary used
            // by materialization, including the controls before a later edge.
            let (visible_end, _) =
                super::whitespace::trailing(&p.data, 0, at + 1, &wide.widths[..at + 1], &mut sat);
            let mut visible = super::spacing_summary::Cursor::default();
            for i in 0..visible_end {
                super::spacing::push(&p.data, &mut visible, i);
            }
            let expected = visible.summary(Some(&p.data));
            assert_eq!(selected.last.map(|e| e.unit), expected.last.map(|e| e.unit));
            assert_eq!(selected.width(&mut sat), expected.width(&mut sat));
        }
        let edges = super::punctuation::edges(
            &p.data,
            tail.summary(&p.data, p.data.units.len())
                .unwrap_or_else(|| cursor.summary(Some(&p.data))),
            p.start_token().flags,
            true,
            Some(&options),
            crate::geometry::LayoutUnit::from_f32_round(1000.0, &mut sat),
            wide.content
                .add(wide.punctuation_edges.removed(&mut sat), &mut sat),
            &mut sat,
        );
        assert_eq!(edges.hang_end, wide.punctuation_edges.hang_end);
        assert_eq!(edges.end_trim, wide.punctuation_edges.end_trim);
    }
}

#[test]
fn hyphen_before_closing_controls_fits_at_its_materialized_width() {
    let fonts = final_review_fonts();
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            crate::font::FontFaceDescriptor {
                family: "Shodo Fixture Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    for isolate in [false, true] {
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![crate::style::FontFamily::Named(
            "Shodo Fixture Latin".into(),
        )];
        style.root.font_size = 16.0;
        let mut rtl = style.root.clone();
        rtl.direction = crate::geometry::Direction::Rtl;
        rtl.unicode_bidi = if isolate {
            crate::style::UnicodeBidi::IsolateOverride
        } else {
            crate::style::UnicodeBidi::BidiOverride
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.open_inline(NodeId(2), &rtl, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(3) }, ",VA VA\u{ad}");
        b.close_inline();
        b.push_atomic(NodeId(9), &style.root, InlineEdges::default());
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        let mut atomics = AtomicSizes::new();
        atomics.insert(
            NodeId(9),
            AtomicSize {
                inline_size: 2000.0,
                ..Default::default()
            },
        );
        let options = LineOptions {
            hanging_punctuation: crate::style::HangingPunctuation {
                force_end: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let crate::LineResult::Line(wide) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &crate::LineConstraint::new(1000.0),
            &atomics,
        ) else {
            panic!("wide hyphen line");
        };
        assert!(wide.fragments().any(|f| {
            match f {
                Fragment::GlyphRun(run) => run
                    .clusters()
                    .any(|c| c.source_char == Some('\u{ad}') && c.advance > 0.0),
                _ => false,
            }
        }));
        let scans = cx.cache_visits;
        for width in [
            wide.inline_size() + 1.0 / 64.0,
            wide.inline_size(),
            wide.inline_size() - 1.0 / 64.0,
            wide.inline_size() - 1.0 / 64.0,
        ] {
            let crate::LineResult::Line(fresh) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &options,
                &crate::LineConstraint::new(width),
                &atomics,
            ) else {
                panic!("fresh hyphen line");
            };
            if width >= wide.inline_size() {
                assert_eq!(fresh.text_range(), wide.text_range(), "isolate={isolate}");
                assert_eq!(fresh.inline_size(), wide.inline_size());
                assert_eq!(fresh.break_reason(), wide.break_reason());
            } else {
                assert!(fresh.text_range().end < wide.text_range().end);
            }
            let crate::LineResult::Line(retry) = p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &crate::LineConstraint::new(width),
                &atomics,
            ) else {
                panic!("cached hyphen line");
            };
            assert_eq!(
                retry.text_range(),
                fresh.text_range(),
                "isolate={isolate}, width={width}"
            );
            assert_eq!(retry.inline_size(), fresh.inline_size());
            assert_eq!(retry.break_reason(), fresh.break_reason());
            assert_eq!(retry.break_token(), fresh.break_token());
            assert!(retry.inline_size() <= width);
            assert_eq!(cx.cache_visits, scans);
        }
        assert!(cx.cache_prepare_visits > 0);
        assert!(cx.take_warnings().is_empty());
    }
}
