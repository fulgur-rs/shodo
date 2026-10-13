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
