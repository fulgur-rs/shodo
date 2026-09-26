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
