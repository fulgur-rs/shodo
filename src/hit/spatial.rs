//! A balanced rectangle tree built once; query depth is bounded by log2(N).
use crate::geometry::LogicalRect;
#[derive(Clone, Copy)]
struct Bounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}
impl Bounds {
    fn rect(r: LogicalRect) -> Self {
        Self {
            left: r.inline_start.min(r.inline_start + r.inline_size),
            right: r.inline_start.max(r.inline_start + r.inline_size),
            top: r.block_start.min(r.block_start + r.block_size),
            bottom: r.block_start.max(r.block_start + r.block_size),
        }
    }
    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            right: self.right.max(other.right),
            top: self.top.min(other.top),
            bottom: self.bottom.max(other.bottom),
        }
    }
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
    fn distance(self, y: f32) -> f32 {
        if y < self.top {
            self.top - y
        } else if y > self.bottom {
            y - self.bottom
        } else {
            0.0
        }
    }
}
struct Node {
    bounds: Bounds,
    children: Option<(usize, usize)>,
    value: usize,
}
pub(super) struct Tree {
    nodes: Vec<Node>,
    root: Option<usize>,
    #[cfg(test)]
    visits: std::sync::atomic::AtomicUsize,
}
impl Tree {
    pub(super) fn new(rects: impl Iterator<Item = (usize, LogicalRect)>, vertical: bool) -> Self {
        let mut rects: Vec<_> = rects.map(|(i, r)| (i, Bounds::rect(r))).collect();
        rects.sort_by(|a, b| {
            if vertical {
                a.1.top.total_cmp(&b.1.top)
            } else {
                a.1.left.total_cmp(&b.1.left)
            }
        });
        let mut tree = Self {
            nodes: Vec::with_capacity(rects.len().saturating_mul(2)),
            root: None,
            #[cfg(test)]
            visits: Default::default(),
        };
        if !rects.is_empty() {
            tree.root = Some(tree.build(&rects));
        }
        tree
    }
    fn build(&mut self, rects: &[(usize, Bounds)]) -> usize {
        let node = if rects.len() == 1 {
            Node {
                bounds: rects[0].1,
                children: None,
                value: rects[0].0,
            }
        } else {
            let mid = rects.len() / 2;
            let left = self.build(&rects[..mid]);
            let right = self.build(&rects[mid..]);
            Node {
                bounds: self.nodes[left].bounds.union(self.nodes[right].bounds),
                children: Some((left, right)),
                value: 0,
            }
        };
        let index = self.nodes.len();
        self.nodes.push(node);
        index
    }
    fn visit(&self) {
        #[cfg(test)]
        self.visits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    pub(super) fn contains(&self, x: f32, y: f32) -> bool {
        self.root.is_some_and(|r| self.contains_node(r, x, y))
    }
    pub(super) fn containing(&self, x: f32, y: f32) -> Option<usize> {
        self.root.and_then(|root| self.containing_node(root, x, y))
    }
    fn containing_node(&self, index: usize, x: f32, y: f32) -> Option<usize> {
        self.visit();
        let node = &self.nodes[index];
        if !node.bounds.contains(x, y) {
            return None;
        }
        node.children.map_or(Some(node.value), |(a, b)| {
            self.containing_node(a, x, y)
                .or_else(|| self.containing_node(b, x, y))
        })
    }
    fn contains_node(&self, i: usize, x: f32, y: f32) -> bool {
        self.visit();
        let n = &self.nodes[i];
        n.bounds.contains(x, y)
            && n.children
                .is_none_or(|(a, b)| self.contains_node(a, x, y) || self.contains_node(b, x, y))
    }
    pub(super) fn nearest_y(&self, y: f32) -> Option<usize> {
        let root = self.root?;
        let y = if y == f32::INFINITY {
            self.nodes[root].bounds.bottom
        } else if y == f32::NEG_INFINITY {
            self.nodes[root].bounds.top
        } else {
            y
        };
        let mut best = None;
        self.nearest_node(root, y, &mut best);
        best.map(|(_, i)| i)
    }
    fn nearest_node(&self, i: usize, y: f32, best: &mut Option<(f32, usize)>) {
        self.visit();
        let n = &self.nodes[i];
        let distance = n.bounds.distance(y);
        if best.is_some_and(|(d, _)| d == 0.0 || distance > d) {
            return;
        }
        if let Some((mut a, mut b)) = n.children {
            if self.nodes[b].bounds.distance(y) < self.nodes[a].bounds.distance(y) {
                std::mem::swap(&mut a, &mut b);
            }
            self.nearest_node(a, y, best);
            self.nearest_node(b, y, best);
        } else if best.is_none_or(|(d, _)| distance < d) {
            *best = Some((distance, n.value));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_coordinate_queries_do_not_scan_all_characters_or_lines() {
        let tree = Tree::new(
            (0..100_000).map(|i| {
                (
                    i,
                    LogicalRect {
                        inline_start: i as f32,
                        inline_size: 1.0,
                        block_start: 0.0,
                        block_size: 20.0,
                    },
                )
            }),
            false,
        );
        for i in 0..1000 {
            assert!(tree.contains(i as f32 * 99.0 + 0.5, 10.0));
        }
        assert!(tree.visits.load(std::sync::atomic::Ordering::Relaxed) < 50_000);
        let tree = Tree::new(
            (0..100_000).map(|i| {
                (
                    i,
                    LogicalRect {
                        inline_start: 0.0,
                        inline_size: 0.0,
                        block_start: i as f32 * 20.0,
                        block_size: 20.0,
                    },
                )
            }),
            true,
        );
        for i in 0..1000 {
            assert_eq!(tree.nearest_y(i as f32 * 99.0 * 20.0 + 10.0), Some(i * 99));
        }
        assert!(tree.visits.load(std::sync::atomic::Ordering::Relaxed) < 50_000);
        assert_eq!(tree.nearest_y(f32::INFINITY), Some(99_999));
        assert_eq!(tree.nearest_y(f32::NEG_INFINITY), Some(0));
    }
}
