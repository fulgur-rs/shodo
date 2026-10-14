use super::*;
use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
use crate::limits::Limits;
use crate::node::{NodeId, TextSource};
use crate::style::{FontFamily, ParagraphStyle, WordBreak};

fn paragraph(text: &str, arabic: bool, limits: &Limits) -> crate::Paragraph {
    let fonts = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    fonts
        .register_face(
            if arabic {
                crate::test_support::fonts::ARABIC
            } else {
                crate::test_support::fonts::LATIN
            }
            .to_vec(),
            0,
            FontFaceDescriptor {
                family: "Candidate".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut style = ParagraphStyle::default();
    style.root.font_families = vec![FontFamily::Named("Candidate".into())];
    style.root.word_break = WordBreak::BreakAll;
    let mut builder = crate::ParagraphBuilder::new(&style, limits);
    builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
    builder.build(&mut LayoutContext::new(), &fonts).unwrap()
}

fn fill_probe_snapshots(data: &ParagraphData, cx: &mut LayoutContext) {
    cx.edge_shapes.begin(data);
    let large = Arc::new((
        crate::shape::GlyphStore {
            id: vec![0; EDGE_SHAPE_ENTRY_COST_MAX],
            ..Default::default()
        },
        Vec::new(),
    ));
    let empty = Arc::new((crate::shape::GlyphStore::default(), Vec::new()));
    for (base, snapshot) in [(100_000, large), (200_000, empty)] {
        for i in base..base + EDGE_SHAPE_CACHE_ENTRIES {
            if !cx.edge_shapes.insert((i, i + 1, None), &snapshot) {
                break;
            }
        }
    }
}

#[test]
fn selected_windows_reuse_unused_summary_space_after_probe_snapshots_fill() {
    use std::sync::atomic::Ordering;
    for (text, arabic, unlimited) in [
        ("ffi", false, false),
        ("ببب", true, false),
        ("ffi", false, true),
        ("ببب", true, true),
    ] {
        let mut p = paragraph(text, arabic, &Limits::default());
        let data = Arc::get_mut(&mut p.data).unwrap();
        if unlimited {
            data.limits.max_reshape_window_bytes = None;
            data.limits.max_shaped_glyphs = None;
        }
        let mut cx = LayoutContext::new();
        fill_probe_snapshots(data, &mut cx);
        let mut first_sat = Saturation::default();
        let first = measure(data, 0, 1, &mut cx, &mut first_sat);
        assert!(
            !first.is_empty(),
            "exercise selected partial/cursive glyphs"
        );
        let first_spent = cx.edge_reshape_spent;
        let first_warnings = cx.take_warnings();
        let before = data.edge_shape_calls.load(Ordering::Relaxed);
        cx.edge_reshape_spent = 0;
        let mut second_sat = Saturation::default();
        let second = measure(data, 0, 1, &mut cx, &mut second_sat);
        assert_eq!(
            first
                .iter()
                .map(|w| format!("{:?}{:?}", w.overlay, w.changes))
                .collect::<Vec<_>>(),
            second
                .iter()
                .map(|w| format!("{:?}{:?}", w.overlay, w.changes))
                .collect::<Vec<_>>()
        );
        assert_eq!(cx.edge_reshape_spent, first_spent);
        assert_eq!(cx.take_warnings(), first_warnings);
        assert_eq!(second_sat, first_sat);
        assert!(cx.edge_shapes.cost <= EDGE_SHAPE_CACHE_COST);
        assert_eq!(
            data.edge_shape_calls.load(Ordering::Relaxed),
            before,
            "{text}: selected window must survive probe admission exhaustion"
        );
    }
}

#[test]
fn warmed_candidates_and_deltas_do_not_clone_glyph_stores() {
    for (text, arabic) in [("ffi", false), ("ببب", true)] {
        let p = paragraph(text, arabic, &Limits::default());
        let data = &p.data;
        let end = 1;
        if !arabic {
            assert!(group(data, 0).end > end, "exercise partial ligature suffix");
        }
        let mut cx = LayoutContext::new();
        data.edge_shape_calls
            .store(0, std::sync::atomic::Ordering::Relaxed);
        crate::shape::cache_clone_probe::reset();
        let expected = candidate(data, 0, end, &mut cx, &mut Saturation::default());
        assert!(expected.1);
        assert!(cx.edge_shapes.len() > 0, "exercise cache hits");
        let shape_calls = data
            .edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            crate::shape::cache_clone_probe::count() <= shape_calls,
            "cache misses may clone once for retention; evaluation must not clone"
        );
        cx.edge_reshape_spent = 0;
        crate::shape::cache_clone_probe::reset();
        let actual = candidate(data, 0, end, &mut cx, &mut Saturation::default());
        assert_eq!(actual, expected);
        assert_eq!(
            crate::shape::cache_clone_probe::count(),
            0,
            "{text} candidate"
        );
        assert_eq!(
            data.edge_shape_calls
                .load(std::sync::atomic::Ordering::Relaxed),
            shape_calls
        );
        cx.edge_reshape_spent = 0;
        crate::shape::cache_clone_probe::reset();
        assert_eq!(
            delta(data, 0, end, &mut cx, &mut Saturation::default()),
            expected.0
        );
        assert_eq!(crate::shape::cache_clone_probe::count(), 0, "{text} delta");
    }
}

// Reference the owned output path that candidate evaluation used before the
// optimization, including its selected-source and remaining-suffix probes.
fn owned_candidate(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (LayoutUnit, bool) {
    let windows = measure(data, start, end, cx, sat);
    let delta = cost(&windows, end, sat);
    let covered = covers_partial_edges(data, start, end, windows.iter().map(|w| &w.overlay.text));
    drop(windows);
    let remainder = last(data, start, end).is_some_and(|at| group(data, at).end > end);
    let valid = covered
        && (!remainder || {
            let windows = measure(data, end, data.units.len(), cx, sat);
            covers_partial_edges(
                data,
                end,
                data.units.len(),
                windows.iter().map(|w| &w.overlay.text),
            )
        });
    (delta, valid)
}

#[test]
fn borrowed_candidates_preserve_owned_measurement_effects() {
    use std::sync::atomic::Ordering;
    for (text, arabic) in [("ffi", false), ("ببب", true)] {
        for (window_bytes, glyph_budget) in [(None, None), (Some(2), None), (None, Some(1))] {
            let mut p = paragraph(text, arabic, &Limits::default());
            let data = Arc::get_mut(&mut p.data).unwrap();
            data.limits.max_reshape_window_bytes = window_bytes;
            data.limits.max_shaped_glyphs = glyph_budget;
            let mut owned_cx = LayoutContext::new();
            let mut borrowed_cx = LayoutContext::new();
            // Include a budget crossing and a globally suppressed warning sink.
            for (spent, warning_limit) in [(0, None), (126, Some(0)), (u64::MAX, Some(1))] {
                for warm in [false, true] {
                    if !warm {
                        owned_cx.edge_shapes.clear();
                        borrowed_cx.edge_shapes.clear();
                    }
                    owned_cx.edge_reshape_spent = spent;
                    borrowed_cx.edge_reshape_spent = spent;
                    owned_cx.warnings = WarningSink::new(warning_limit);
                    borrowed_cx.warnings = WarningSink::new(warning_limit);
                    let seed = Saturation {
                        saturated: 7,
                        non_finite: 3,
                    };
                    let mut owned_sat = seed;
                    let mut borrowed_sat = seed;
                    data.edge_shape_calls.store(0, Ordering::Relaxed);
                    let expected = owned_candidate(data, 0, 1, &mut owned_cx, &mut owned_sat);
                    let expected_calls = data.edge_shape_calls.load(Ordering::Relaxed);
                    data.edge_shape_calls.store(0, Ordering::Relaxed);
                    let actual = candidate(data, 0, 1, &mut borrowed_cx, &mut borrowed_sat);
                    assert_eq!(
                        actual, expected,
                        "{text}, {window_bytes:?}, {glyph_budget:?}, {spent}, {warm}"
                    );
                    assert_eq!(borrowed_sat, owned_sat);
                    assert_eq!(borrowed_cx.edge_reshape_spent, owned_cx.edge_reshape_spent);
                    assert_eq!(borrowed_cx.warnings.take(), owned_cx.warnings.take());
                    assert_eq!(
                        data.edge_shape_calls.load(Ordering::Relaxed),
                        expected_calls
                    );
                }
            }
        }
    }
}

#[test]
fn borrowed_candidates_preserve_new_saturations_from_uncached_shapes() {
    let mut p = paragraph("ببب", true, &Limits::default());
    let data = Arc::get_mut(&mut p.data).unwrap();
    // Reshape the existing normal-width source with a size that saturates the
    // generated advances, without changing its original edge flags or widths.
    for style in &mut data.styles {
        style.font_size = 1e20;
    }
    let mut owned_cx = LayoutContext::new();
    let mut borrowed_cx = LayoutContext::new();
    let mut owned_sat = Saturation::default();
    let mut borrowed_sat = Saturation::default();
    let expected = owned_candidate(data, 0, 2, &mut owned_cx, &mut owned_sat);
    let actual = candidate(data, 0, 2, &mut borrowed_cx, &mut borrowed_sat);
    assert!(owned_sat.saturated > 0, "exercise lossy shaping arithmetic");
    assert_eq!(actual, expected);
    assert_eq!(borrowed_sat, owned_sat);
    assert_eq!(borrowed_cx.edge_reshape_spent, owned_cx.edge_reshape_spent);
    assert_eq!(borrowed_cx.warnings.take(), owned_cx.warnings.take());
    assert_eq!(
        borrowed_cx.edge_shapes.len(),
        0,
        "unclean shapes are not cached"
    );
}

#[test]
fn candidate_measurements_release_shared_glyph_handles() {
    let p = paragraph("ffi", false, &Limits::default());
    let mut cx = LayoutContext::new();
    let expected = candidate(&p.data, 0, 1, &mut cx, &mut Saturation::default());
    assert!(expected.1);
    let windows: Vec<MeasuredWindow> =
        measure_edit(&p.data, 0, 1, None, &mut cx, &mut Saturation::default());
    assert!(!windows.is_empty(), "retain a measured partial edge");
    let retained: Vec<_> = cx
        .edge_shapes
        .entries
        .values()
        .map(Arc::downgrade)
        .collect();
    cx.edge_shapes.clear();
    assert!(
        retained.iter().all(|weak| weak.upgrade().is_none()),
        "width/coverage results must not keep cached glyph allocations alive"
    );
    assert_eq!(windows[0].delta(1, &mut Saturation::default()), expected.0);
}

#[test]
fn compact_measurements_replay_without_glyphs_and_fall_back_at_budget_boundaries() {
    use std::sync::atomic::Ordering;
    for (text, arabic) in [("ffi", false), ("ببب", true)] {
        let p = paragraph(text, arabic, &Limits::default());
        let data = &p.data;
        let mut cached = LayoutContext::new();
        let mut recorded_sat = Saturation::default();
        let expected = candidate(data, 0, 1, &mut cached, &mut recorded_sat);
        assert!(cached.edge_shapes.measurement((0, 1)).is_some());
        // Draw snapshots may have been refused or released independently.
        cached.edge_shapes.entries.clear();
        cached.edge_shapes.cost = cached.edge_shapes.measurement_storage_cost();
        let before = data.edge_shape_calls.load(Ordering::Relaxed);
        cached.edge_reshape_spent = 0;
        let mut replayed_sat = Saturation::default();
        assert_eq!(
            candidate(data, 0, 1, &mut cached, &mut replayed_sat),
            expected
        );
        assert_eq!(replayed_sat, recorded_sat);
        assert_eq!(data.edge_shape_calls.load(Ordering::Relaxed), before);
        let limit = data.limits.max_reshape_window_bytes.unwrap() * EDGE_RESHAPE_LINE_WINDOWS;
        for (spent, suppress) in [(limit - 1, false), (limit + 1, false), (0, true)] {
            let mut fresh = LayoutContext::new();
            cached.edge_reshape_spent = spent;
            fresh.edge_reshape_spent = spent;
            cached.warnings = WarningSink::new(None);
            fresh.warnings = WarningSink::new(None);
            if suppress {
                for cx in [&mut cached, &mut fresh] {
                    cx.warnings.set_max(Some(0));
                    cx.warnings.push(WarningKind::Unsupported, "suppressed");
                }
            }
            let mut cached_sat = Saturation {
                saturated: 7,
                non_finite: 3,
            };
            let mut fresh_sat = cached_sat;
            let actual = candidate(data, 0, 1, &mut cached, &mut cached_sat);
            let expected = owned_candidate(data, 0, 1, &mut fresh, &mut fresh_sat);
            assert_eq!(actual, expected, "{text}, {spent}, {suppress}");
            assert_eq!(cached_sat, fresh_sat);
            assert_eq!(cached.edge_reshape_spent, fresh.edge_reshape_spent);
            assert_eq!(cached.warnings.take(), fresh.warnings.take());
        }
    }
}

#[test]
fn compact_measurements_preserve_arithmetic_saturation() {
    let mut p = paragraph("AVA", false, &Limits::default());
    let data = Arc::get_mut(&mut p.data).unwrap();
    for advance in &mut data.glyphs.advance {
        *advance = LayoutUnit::from_raw(i32::MAX);
    }
    for unit in &mut data.units {
        unit.unsafe_to_break = true;
    }
    let mut cached = LayoutContext::new();
    let mut recorded = Saturation::default();
    let expected = candidate(data, 0, data.units.len(), &mut cached, &mut recorded);
    assert!(!recorded.is_clean(), "fixture must saturate arithmetic");
    assert!(
        cached
            .edge_shapes
            .measurement((0, data.units.len()))
            .is_some()
    );
    cached.edge_reshape_spent = 0;
    let mut replayed = Saturation::default();
    assert_eq!(
        candidate(data, 0, data.units.len(), &mut cached, &mut replayed),
        expected
    );
    assert_eq!(replayed, recorded);
}

#[test]
fn zero_delta_edge_measurements_without_a_charge_budget_are_reused() {
    let limits = Limits {
        max_reshape_window_bytes: None,
        ..Limits::default()
    };
    let mut p = paragraph("ببب", true, &limits);
    let data = Arc::get_mut(&mut p.data).unwrap();
    data.units.last_mut().unwrap().unsafe_to_break = true;
    let mut cx = LayoutContext::new();
    assert_eq!(
        candidate(
            data,
            0,
            data.units.len(),
            &mut cx,
            &mut Saturation::default()
        ),
        (LayoutUnit::ZERO, true)
    );
    assert!(
        data.edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0,
        "fixture must perform an edge reshape before checking retention"
    );
    assert!(
        cx.edge_shapes.measurement((0, data.units.len())).is_some(),
        "zero delta can still require expensive edge shaping"
    );
    cx.edge_shapes.entries.clear();
    cx.edge_shapes.cost = cx.edge_shapes.measurement_storage_cost();
    let before = data
        .edge_shape_calls
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        candidate(
            data,
            0,
            data.units.len(),
            &mut cx,
            &mut Saturation::default()
        ),
        (LayoutUnit::ZERO, true)
    );
    assert_eq!(
        data.edge_shape_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        before
    );
}
