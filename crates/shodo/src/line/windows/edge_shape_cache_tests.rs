use super::*;
use crate::shape::GlyphStore;

fn window(glyphs: usize) -> ShapedWindow {
    let store = GlyphStore {
        id: vec![0; glyphs],
        ..Default::default()
    };
    std::sync::Arc::new((store, Vec::new()))
}

#[test]
fn an_oversized_window_is_never_retained() {
    let mut cache = EdgeShapeCache::default();
    assert!(!cache.insert((0, 1, None), &window(EDGE_SHAPE_ENTRY_COST_MAX + 1)));
    assert_eq!(cache.entries.len(), 0);
    assert_eq!(cache.cost, 0);
    assert!(cache.insert((0, 1, None), &window(EDGE_SHAPE_ENTRY_COST_MAX)));
}

#[test]
fn retained_cost_never_exceeds_the_cap_even_with_few_entries() {
    let mut cache = EdgeShapeCache::default();
    // 64 entries at the per-entry maximum is far below the entry-count cap
    // (256) but far above the cost cap.
    for i in 0..64 {
        cache.insert((i, i + 1, None), &window(EDGE_SHAPE_ENTRY_COST_MAX));
        assert!(
            cache.cost <= EDGE_SHAPE_CACHE_COST,
            "retained {} > {} after {} entries",
            cache.cost,
            EDGE_SHAPE_CACHE_COST,
            i + 1
        );
    }
    assert!(cache.entries.len() < 64);
}

#[test]
fn replacing_a_key_keeps_the_cost_consistent_and_clear_releases_everything() {
    let mut cache = EdgeShapeCache::default();
    cache.insert((0, 1, None), &window(10));
    cache.insert((0, 1, None), &window(30));
    assert_eq!(cache.cost, 30);
    assert_eq!(cache.entries.len(), 1);
    cache.clear();
    assert_eq!((cache.cost, cache.entries.len()), (0, 0));
    assert_eq!(cache.entries.capacity(), 0, "clear must free the table");
}

#[test]
fn cache_retains_original_vectors_without_deep_copy() {
    let mut cache = EdgeShapeCache::default();
    let original = window(16);
    let pointer = original.0.id.as_ptr();
    assert!(cache.insert((0, 1, None), &original));
    assert_eq!(cache.get(&(0, 1, None)).unwrap().0.id.as_ptr(), pointer);
}

#[test]
fn cache_insert_and_read_handles_do_not_clone_glyph_store() {
    let mut cache = EdgeShapeCache::default();
    let original = window(16);
    crate::shape::cache_clone_probe::reset();
    assert!(cache.insert((0, 1, None), &original));
    let first = cache.get(&(0, 1, None)).unwrap().clone();
    let second = cache.get(&(0, 1, None)).unwrap().clone();
    assert_eq!(first.0.id, second.0.id);
    assert_eq!(crate::shape::cache_clone_probe::count(), 0);
}

#[test]
fn selected_output_is_owned_and_cannot_mutate_cached_vectors() {
    let mut cache = EdgeShapeCache::default();
    let original = window(16);
    assert!(cache.insert((0, 1, None), &original));
    let weak = std::sync::Arc::downgrade(&original);
    drop(original);
    crate::shape::cache_clone_probe::reset();
    let handle = WindowHandle::Shared(std::sync::Arc::clone(cache.get(&(0, 1, None)).unwrap()));
    let (mut output, _) = handle.into_owned();
    assert_eq!(crate::shape::cache_clone_probe::count(), 1);
    output.id[0] = 99;
    assert_eq!(cache.get(&(0, 1, None)).unwrap().0.id[0], 0);
    cache.clear();
    assert!(
        weak.upgrade().is_none(),
        "owned output must not retain cache container"
    );
    assert_eq!(output.id[0], 99, "output outlives cache release");
}

#[test]
fn sole_shared_handle_moves_vectors_after_cache_release() {
    let mut cache = EdgeShapeCache::default();
    let original = window(16);
    let pointer = original.0.id.as_ptr();
    assert!(cache.insert((0, 1, None), &original));
    let weak = std::sync::Arc::downgrade(&original);
    let handle = WindowHandle::Shared(original);
    cache.clear();
    crate::shape::cache_clone_probe::reset();
    let (output, _) = handle.into_owned();
    assert_eq!(crate::shape::cache_clone_probe::count(), 0);
    assert_eq!(output.id.as_ptr(), pointer);
    assert!(weak.upgrade().is_none());
}

#[test]
fn uncacheable_owned_handle_moves_without_copy() {
    let original = window(EDGE_SHAPE_ENTRY_COST_MAX + 1);
    let owned = std::sync::Arc::try_unwrap(original).unwrap();
    let pointer = owned.0.id.as_ptr();
    crate::shape::cache_clone_probe::reset();
    let (output, _) = WindowHandle::Owned(owned).into_owned();
    assert_eq!(crate::shape::cache_clone_probe::count(), 0);
    assert_eq!(output.id.as_ptr(), pointer);
}

#[test]
fn retained_window_trims_every_vector_and_empty_run_capacity() {
    fn slack<T: Clone>(value: T) -> Vec<T> {
        let mut vector = Vec::with_capacity(64);
        vector.resize(8, value);
        vector
    }
    let store = GlyphStore {
        id: slack(1),
        advance: slack(LayoutUnit::ZERO),
        pen: slack(LayoutUnit::ZERO),
        offset_inline: slack(LayoutUnit::ZERO),
        offset_block: slack(LayoutUnit::ZERO),
        cluster: slack(0),
        flags: slack(0),
        spacing: Some(slack(LayoutUnit::ZERO)),
        leading: Some(slack(LayoutUnit::ZERO)),
        ..Default::default()
    };
    let mut owned = (store, Vec::with_capacity(8));
    trim_retained_window(&mut owned);
    let store = &owned.0;
    assert_eq!(
        [
            store.id.capacity(),
            store.advance.capacity(),
            store.pen.capacity(),
            store.offset_inline.capacity(),
            store.offset_block.capacity(),
            store.cluster.capacity(),
            store.flags.capacity(),
            store.spacing.as_ref().unwrap().capacity(),
            store.leading.as_ref().unwrap().capacity(),
        ],
        [8; 9]
    );
    assert_eq!(owned.1.capacity(), 0);
    assert_eq!(store.id, vec![1; 8]);
    assert_eq!(store.spacing.as_ref().unwrap(), &vec![LayoutUnit::ZERO; 8]);
}

#[test]
fn glyph_budget_remains_part_of_the_cache_key() {
    let mut cache = EdgeShapeCache::default();
    let ordinary = window(16);
    let limited = window(1);
    assert!(cache.insert((0, 1, None), &ordinary));
    assert!(cache.insert((0, 1, Some(0)), &limited));
    assert_eq!(cache.get(&(0, 1, None)).unwrap().0.len(), 16);
    assert_eq!(cache.get(&(0, 1, Some(0))).unwrap().0.len(), 1);
    assert!(cache.get(&(0, 1, Some(1))).is_none());
    assert_eq!(cache.cost, 17);
}

#[test]
fn changing_paragraph_owner_releases_previous_shared_cache() {
    fn paragraph() -> crate::Paragraph {
        let style = crate::style::ParagraphStyle::default();
        let limits = crate::limits::Limits::default();
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "owner",
        );
        builder
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&limits),
            )
            .unwrap()
    }
    let first = paragraph();
    let second = paragraph();
    let mut cache = EdgeShapeCache::default();
    cache.begin(&first.data);
    let original = window(16);
    let weak = std::sync::Arc::downgrade(&original);
    assert!(cache.insert((0, 1, None), &original));
    drop(original);
    cache.begin(&first.data);
    assert_eq!(cache.len(), 1);
    cache.begin(&second.data);
    assert_eq!((cache.len(), cache.cost), (0, 0));
    assert!(weak.upgrade().is_none());
    assert_eq!(
        cache.owner,
        Some((
            second.data.id,
            std::sync::Arc::as_ptr(&second.data) as usize
        ))
    );
}
