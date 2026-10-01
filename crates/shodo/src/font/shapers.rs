//! Bounded immutable shaping handles with shared-reader LRU touches.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct Entry {
    index: u32,
    data: Arc<harfrust::ShaperData>,
    used: AtomicU64,
}

#[derive(Default)]
pub(super) struct ShaperCache {
    slots: Vec<Entry>,
    index: HashMap<u32, usize>,
    clock: AtomicU64,
}

impl ShaperCache {
    pub(super) fn get(&self, index: u32) -> Option<Arc<harfrust::ShaperData>> {
        let entry = self.slots.get(*self.index.get(&index)?)?;
        #[cfg(test)]
        super::SHAPER_SEARCH_COMPARISONS.with(|count| count.set(count.get() + 1));
        let stamp = self.clock.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        entry.used.fetch_max(stamp, Ordering::Relaxed);
        Some(entry.data.clone())
    }

    // The caller holds the cache writer and rechecks after constructing a
    // missing handle, so each font index has at most one retained entry.
    pub(super) fn insert(&mut self, index: u32, data: Arc<harfrust::ShaperData>, cap: u64) {
        if cap == 0 {
            return;
        }
        let stamp = self.clock.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        let entry = Entry {
            index,
            data,
            used: AtomicU64::new(stamp),
        };
        let slot = if self.slots.len() as u64 >= cap {
            let Some((slot, victim)) = self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.used.load(Ordering::Relaxed))
            else {
                return;
            };
            self.index.remove(&victim.index);
            self.slots[slot] = entry;
            slot
        } else {
            self.slots.push(entry);
            self.slots.len() - 1
        };
        self.index.insert(index, slot);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    #[cfg(test)]
    pub(super) fn iter(&self) -> impl Iterator<Item = (&u32, &Arc<harfrust::ShaperData>)> {
        self.slots.iter().map(|entry| (&entry.index, &entry.data))
    }
}
