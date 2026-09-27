use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::style::{LineOptions, ParagraphStyle};
use crate::{AtomicSize, AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

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
        .register(include_bytes!("../../dev/fixtures/assets/fonts/arabic.ttf").to_vec())
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
