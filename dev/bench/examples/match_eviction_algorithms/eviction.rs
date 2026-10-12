//! Lower-bound recency heap. Shared hits update only the entry's atomic stamp;
//! the exclusive insertion path refreshes stale roots until it finds the LRU.
//! Exactly one heap node/position is retained per live slot, including after
//! hash replacement. There is no append-only queue of hit records.
#[derive(Default)]
pub(super) struct Eviction {
    heap: Vec<(u64, u32)>,
    positions: Vec<u32>,
}

impl Eviction {
    pub(super) fn push(&mut self, slot: u32, stamp: u64) {
        debug_assert_eq!(slot as usize, self.positions.len());
        let pos = self.heap.len();
        self.positions.push(pos as u32);
        self.heap.push((stamp, slot));
        self.up(pos);
    }

    pub(super) fn update(&mut self, slot: u32, stamp: u64) {
        let pos = self.positions[slot as usize] as usize;
        self.heap[pos].0 = stamp;
        if pos > 0 && self.heap[pos] < self.heap[(pos - 1) / 2] {
            self.up(pos);
        } else {
            self.down(pos);
        }
    }

    /// `used` must stay stable during this call. The caller holds the cache's
    /// write lock, so all earlier shared-hit updates have finished. Cached
    /// stamps are lower bounds; equal stamps retain the scan's lowest-slot tie.
    pub(super) fn victim(&mut self, mut used: impl FnMut(u32) -> u64) -> Option<u32> {
        loop {
            let &(stamp, slot) = self.heap.first()?;
            let current = used(slot);
            if current == stamp {
                return Some(slot);
            }
            self.update(slot, current);
        }
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.heap.swap(a, b);
        self.positions[self.heap[a].1 as usize] = a as u32;
        self.positions[self.heap[b].1 as usize] = b as u32;
    }

    fn up(&mut self, mut pos: usize) {
        while pos > 0 {
            let parent = (pos - 1) / 2;
            if self.heap[parent] <= self.heap[pos] {
                break;
            }
            self.swap(pos, parent);
            pos = parent;
        }
    }

    fn down(&mut self, mut pos: usize) {
        while let Some(left) = pos.checked_mul(2).and_then(|p| p.checked_add(1)) {
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.heap.len() && self.heap[right] < self.heap[left] {
                right
            } else {
                left
            };
            if self.heap[pos] <= self.heap[child] {
                break;
            }
            self.swap(pos, child);
            pos = child;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Eviction;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    #[test]
    fn deferred_hit_updates_agree_with_scan_at_capacity_boundaries() {
        for cap in [1, 8, 255, 256, 257, 4096] {
            let used: Vec<_> = (1..=cap as u64).map(AtomicU64::new).collect();
            let mut heap = Eviction::default();
            for (slot, stamp) in used.iter().enumerate() {
                heap.push(slot as u32, stamp.load(Relaxed));
            }
            let mut random = 17u64;
            let mut stamp = cap as u64;
            for _ in 0..1500 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                stamp += 1;
                used[random as usize % cap].store(stamp, Relaxed);
                let expected = (0..cap).min_by_key(|slot| used[*slot].load(Relaxed));
                let victim = heap.victim(|slot| used[slot as usize].load(Relaxed));
                assert_eq!(victim.map(|slot| slot as usize), expected, "cap={cap}");
                stamp += 1;
                let slot = victim.unwrap();
                used[slot as usize].store(stamp, Relaxed);
                heap.update(slot, stamp);
                assert_eq!(heap.heap.len(), cap);
                assert_eq!(heap.positions.len(), cap);
            }
        }
    }

    #[test]
    fn replacements_can_move_in_both_directions_and_ties_use_lowest_slot() {
        let mut heap = Eviction::default();
        let mut used: Vec<_> = (1..=256).collect();
        for (slot, stamp) in used.iter().enumerate() {
            heap.push(slot as u32, *stamp);
        }
        used[0] = u64::MAX;
        heap.update(0, u64::MAX);
        used[1] = 0;
        heap.update(1, 0);
        assert_eq!(heap.victim(|slot| used[slot as usize]), Some(1));
        used.fill(u64::MAX);
        assert_eq!(heap.victim(|slot| used[slot as usize]), Some(0));
    }
}
