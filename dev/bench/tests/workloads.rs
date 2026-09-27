use shodo::{Fragment, LayoutContext, limits::Limits};
use shodo_bench::{Operation, Workload, digest, layout, workloads};
use shodo_fixtures::load_fonts;

#[test]
fn matrix_has_fixed_cases_and_scales_with_real_glyph_output() {
    let all = workloads();
    assert_eq!(all.len(), 54);
    for case in shodo_fixtures::cases() {
        for scale in [1, 8, 64] {
            assert!(all.iter().any(|w| w.id == case.id && w.scale == scale));
        }
    }
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    for id in [
        "latin-short",
        "japanese-short",
        "arabic-short",
        "combining-latin",
        "mixed-scripts",
        "fallback",
    ] {
        let w = Workload::named(id, 1).unwrap();
        let mut cx = LayoutContext::new();
        let ps = w.build(&mut cx, &fonts, &limits).unwrap();
        let run = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
        let d = digest(&run, &fonts).unwrap();
        assert!(d.glyphs > 0 && d.lines > 0);
        assert_eq!(d.synthetic_glyphs, 0, "{id}");
        assert_eq!(
            run.lines.last().unwrap().text_range().end,
            ps[0].text().len(),
            "{id}"
        );
    }
}

#[test]
fn width_reuse_matches_rebuild_for_tabs_and_actual_float_retries() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    for id in ["preserved-tabs", "float-retry"] {
        let w = Workload::named(id, 1).unwrap();
        let mut cx = LayoutContext::new();
        let ps = w.build(&mut cx, &fonts, &limits).unwrap();
        let reuse = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::ReuseWidths).unwrap();
        let rebuilt = layout(
            &w,
            &ps,
            &mut LayoutContext::new(),
            &fonts,
            &limits,
            Operation::RebuildWidths,
        )
        .unwrap();
        assert_eq!(
            digest(&reuse, &fonts).unwrap(),
            digest(&rebuilt, &fonts).unwrap(),
            "{id}"
        );
        assert!(reuse.lines.len() >= 3);
        if id == "float-retry" {
            assert_eq!(reuse.float_reports, 6);
        } else {
            // Literal input One  two\tthree\nFour five. at90px consumes the tab.
            assert_eq!(reuse.lines[0].text_range(), 0..9);
        }
    }
}

#[test]
fn height_retries_keep_the_token_and_complete_the_same_source() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let w = Workload::named("japanese-short", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let plain = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
    let pages = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::PageRetry).unwrap();
    assert_eq!(pages.height_retries, pages.lines.len());
    assert_eq!(
        pages
            .lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>(),
        plain
            .lines
            .iter()
            .map(|l| (l.text_range(), l.inline_size()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn many_short_workload_builds_independent_paragraphs() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    for scale in [1, 8, 64] {
        let w = Workload::named("many-short-latin", scale).unwrap();
        let ps = w.build(&mut LayoutContext::new(), &fonts, &limits).unwrap();
        assert_eq!(ps.len(), scale);
        assert!(
            ps.iter()
                .all(|p| p.text() == shodo_fixtures::case("latin-short").unwrap().text)
        );
    }
}

#[test]
fn digest_is_stable_across_independent_font_collections() {
    let limits = Limits::default();
    let w = Workload::named("mixed-scripts", 1).unwrap();
    let mut actual = Vec::new();
    for _ in 0..2 {
        let fonts = load_fonts(&limits).unwrap();
        let mut cx = LayoutContext::new();
        let ps = w.build(&mut cx, &fonts, &limits).unwrap();
        actual.push(
            digest(
                &layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap(),
                &fonts,
            )
            .unwrap(),
        );
    }
    assert_eq!(actual[0], actual[1]);
}

#[test]
fn nested_atomic_fixture_uses_real_geometry() {
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let w = Workload::named("nested-atomic", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let run = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
    let atoms: Vec<_> = run
        .lines
        .iter()
        .flat_map(|l| l.fragments())
        .filter_map(|f| {
            if let Fragment::Atomic(a) = f {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(atoms.len(), 1);
    assert_eq!(
        (
            atoms[0].border_rect.inline_size,
            atoms[0].border_rect.block_size
        ),
        (20.0, 20.0)
    );
    let boxes: Vec<_> = run
        .lines
        .iter()
        .flat_map(|l| l.fragments())
        .filter_map(|f| {
            if let Fragment::InlineBox(b) = f {
                Some(b)
            } else {
                None
            }
        })
        .collect();
    assert!(boxes.iter().filter(|b| b.has_end_edge).count() >= 4);
    assert_eq!(atoms[0].baseline - atoms[0].border_rect.block_start, 16.0);
    assert!(
        boxes
            .iter()
            .filter(|b| b.has_end_edge)
            .all(|b| (b.rect.inline_size - b.content_rect.inline_size - 2.0).abs() < 0.0001)
    );
    let measured = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::Intrinsic).unwrap();
    let omitted = ps[0].intrinsic_sizes(
        &mut cx,
        &shodo::style::LineOptions::default(),
        &shodo::AtomicIntrinsics::EMPTY,
    );
    assert_eq!(
        measured.intrinsics[0].max_content,
        omitted.max_content + 20.0
    );
}

#[test]
fn invalid_workloads_and_foreign_fonts_are_not_successful_measurements() {
    assert!(Workload::named("missing", 1).is_err());
    assert!(Workload::named("latin-short", 0).is_err());
    assert!(Workload::named("latin-short", 2).is_err());
    let limits = Limits::default();
    let fonts = load_fonts(&limits).unwrap();
    let other = load_fonts(&limits).unwrap();
    let w = Workload::named("latin-short", 1).unwrap();
    let mut cx = LayoutContext::new();
    let ps = w.build(&mut cx, &fonts, &limits).unwrap();
    let run = layout(&w, &ps, &mut cx, &fonts, &limits, Operation::AllLines).unwrap();
    assert!(digest(&run, &other).is_err());
    let tiny = Limits {
        max_text_bytes: Some(1),
        ..Default::default()
    };
    assert!(w.build(&mut cx, &fonts, &tiny).is_err());
}
