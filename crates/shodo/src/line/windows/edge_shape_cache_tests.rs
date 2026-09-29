use super::*;
use crate::shape::GlyphStore;

fn window(glyphs: usize) -> ShapedWindow {
    let store = GlyphStore {
        id: vec![0; glyphs],
        ..Default::default()
    };
    (store, Vec::new())
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
