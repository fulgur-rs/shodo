//! Per-line bounds indexes. Visual fragments may repeat an owner or source range.
use super::{Bounds, Frame, RecordKind, Saturation};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

#[inline]
fn visit() {
    #[cfg(test)]
    super::bounds_tests::visit();
}
fn include(target: &mut Option<Bounds>, bounds: Bounds) {
    *target = Some(target.map_or(bounds, |old| old.union(bounds)));
}

pub(super) struct RecordBounds {
    boxes: HashMap<u32, Bounds>,
    glyphs: Intervals,
    atomics: Intervals,
}
impl RecordBounds {
    pub(super) fn new(frame: &Frame<'_>, sat: &mut Saturation) -> Self {
        let mut pending = BTreeMap::<u32, Bounds>::new();
        let mut glyphs = Vec::new();
        let mut atomics = Vec::new();
        for (i, record) in frame.records.iter().enumerate() {
            visit();
            let Some(bounds) = frame.record_bounds(i, sat) else {
                continue;
            };
            if let Some(owner) = frame.owner(record) {
                pending
                    .entry(owner)
                    .and_modify(|b| *b = b.union(bounds))
                    .or_insert(bounds);
            }
            match &record.kind {
                RecordKind::Glyphs { text, .. } => {
                    glyphs.push((text.start as usize..text.end as usize, bounds))
                }
                RecordKind::Atomic { unit, .. } => {
                    atomics.push((*unit as usize..*unit as usize + 1, bounds))
                }
                _ => {}
            }
        }
        let mut boxes = HashMap::new();
        // Box IDs are allocated in preorder. Each active owner is completed
        // once, including ancestors that have no record on this continuation.
        // Never walk all paragraph boxes for each short line.
        while let Some((owner, bounds)) = pending.pop_last() {
            visit();
            boxes.insert(owner, bounds);
            if let Some(parent) = frame.data.boxes[owner as usize].parent {
                debug_assert!(parent < owner);
                pending
                    .entry(parent)
                    .and_modify(|b| *b = b.union(bounds))
                    .or_insert(bounds);
            }
        }
        Self {
            boxes,
            glyphs: Intervals::new(glyphs),
            atomics: Intervals::new(atomics),
        }
    }
    pub(super) fn column(
        &self,
        frame: &Frame<'_>,
        owner: Option<u32>,
        units: &Range<usize>,
    ) -> Option<Bounds> {
        if let Some(owner) = owner {
            return self.boxes.get(&owner).copied();
        }
        // Anonymous columns use the original text-overlap/atomic-unit rule,
        // never the enclosing box's padding or unrelated glyph fragments.
        let mut bounds = self.atomics.overlapping(units);
        for unit in &frame.data.units[units.clone()] {
            visit();
            if let Some(other) = self
                .glyphs
                .overlapping(&(unit.text.start as usize..unit.text.end as usize))
            {
                include(&mut bounds, other);
            }
        }
        bounds
    }
}

#[derive(Clone, Copy)]
struct Summary {
    min_start: usize,
    max_start: usize,
    min_end: usize,
    max_end: usize,
    bounds: Bounds,
}
/// Sorted interval tree with subtree unions. Identical or overlapping glyph
/// ranges (shared clusters, bidi fragments, overlays) are all retained.
struct Intervals {
    tree: Vec<Option<Summary>>,
    leaves: usize,
}
impl Intervals {
    fn new(mut entries: Vec<(Range<usize>, Bounds)>) -> Self {
        entries.sort_unstable_by_key(|(range, _)| range.start);
        let leaves = entries.len().max(1).next_power_of_two();
        let mut tree: Vec<Option<Summary>> = vec![None; 2 * leaves];
        for (i, (range, bounds)) in entries.into_iter().enumerate() {
            visit();
            tree[leaves + i] = Some(Summary {
                min_start: range.start,
                max_start: range.start,
                min_end: range.end,
                max_end: range.end,
                bounds,
            });
        }
        for i in (1..leaves).rev() {
            visit();
            tree[i] = match (tree[2 * i], tree[2 * i + 1]) {
                (Some(a), Some(b)) => Some(Summary {
                    min_start: a.min_start.min(b.min_start),
                    max_start: a.max_start.max(b.max_start),
                    min_end: a.min_end.min(b.min_end),
                    max_end: a.max_end.max(b.max_end),
                    bounds: a.bounds.union(b.bounds),
                }),
                (a, b) => a.or(b),
            };
        }
        Self { tree, leaves }
    }
    fn overlapping(&self, range: &Range<usize>) -> Option<Bounds> {
        let mut bounds = None;
        self.query(1, range, &mut bounds);
        bounds
    }
    fn query(&self, node: usize, range: &Range<usize>, bounds: &mut Option<Bounds>) {
        visit();
        let Some(summary) = self.tree[node] else {
            return;
        };
        if summary.min_start >= range.end || summary.max_end <= range.start {
            return;
        }
        if summary.max_start < range.end && summary.min_end > range.start {
            include(bounds, summary.bounds);
        } else if node < self.leaves {
            self.query(2 * node, range, bounds);
            self.query(2 * node + 1, range, bounds);
        }
    }
}

#[derive(Default)]
pub(super) struct CompletedBounds(BTreeMap<(usize, usize), (usize, Bounds)>);
impl CompletedBounds {
    pub(super) fn include_children(&mut self, units: &Range<usize>, mut base: Bounds) -> Bounds {
        // Fragments arrive children first. Their whole bounds already include
        // descendants; consume enclosed roots once instead of rescanning them
        // in every ancestor. Identity preserves equal clipped starts.
        let children: Vec<_> = self
            .0
            .range((units.start, 0)..(units.end, 0))
            .filter_map(|(key, (end, area))| {
                visit();
                (*end <= units.end).then_some((*key, *area))
            })
            .collect();
        for (key, area) in children {
            self.0.remove(&key);
            base = base.union(area);
        }
        base
    }
    pub(super) fn insert(&mut self, units: Range<usize>, container: usize, bounds: Bounds) {
        self.0.insert((units.start, container), (units.end, bounds));
    }
}
