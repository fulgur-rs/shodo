//! Retain one edge for ordinary clusters, full summaries only for exceptions.
use super::super::autospace::Class;
use super::super::spacing_summary::{Edge, Kind, Summary};

/// A leaf's unit is its storage index, leaving room for the precomputed cost
/// without growing the edge. Costs outside i32 stay in full summaries.
#[derive(Clone, Copy, Debug)]
struct Leaf {
    tracking: i32,
    cost: i32,
    box_node: u32,
    punct: u32,
    kind: Kind,
    class: Class,
}

impl Leaf {
    #[inline]
    fn summary(self, unit: u32) -> Summary {
        let edge = Edge {
            tracking: self.tracking,
            unit,
            box_node: self.box_node,
            punct: self.punct,
            kind: self.kind,
            class: self.class,
        };
        Summary {
            first: Some(edge),
            last: Some(edge),
            cost: i64::from(self.cost),
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Stored {
    Leaf(Leaf),
    Empty {
        before: bool,
        after: bool,
        hang_before: bool,
        hang_after: bool,
        autospace_before: bool,
        autospace_after: bool,
    },
    Full(usize),
}

impl Stored {
    #[inline]
    fn resolve(self, unit: usize, full: &[Summary]) -> Summary {
        match self {
            // Leaf creation checked that its u32 unit equals this index.
            Self::Leaf(leaf) => leaf.summary(unit as u32),
            Self::Empty {
                before,
                after,
                hang_before,
                hang_after,
                autospace_before,
                autospace_after,
            } => Summary {
                before,
                after,
                hang_before,
                hang_after,
                autospace_before,
                autospace_after,
                ..Default::default()
            },
            Self::Full(index) => full[index],
        }
    }
}

#[derive(Debug)]
pub(crate) struct Summaries(Storage);

#[derive(Debug)]
enum Storage {
    Compact {
        entries: Vec<Stored>,
        full: Vec<Summary>,
    },
    Dense(Vec<Summary>),
}

impl Default for Summaries {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl Summaries {
    pub(super) fn with_capacity(units: usize) -> Self {
        Self(Storage::Compact {
            entries: Vec::with_capacity(units),
            full: Vec::new(),
        })
    }

    pub(super) fn push(&mut self, summary: Summary) {
        let Storage::Compact { entries, full } = &mut self.0 else {
            if let Storage::Dense(entries) = &mut self.0 {
                entries.push(summary);
            }
            return;
        };
        let stored = if summary.first.is_none() && summary.last.is_none() && summary.cost == 0 {
            Stored::Empty {
                before: summary.before,
                after: summary.after,
                hang_before: summary.hang_before,
                hang_after: summary.hang_after,
                autospace_before: summary.autospace_before,
                autospace_after: summary.autospace_after,
            }
        } else if let Some(edge) = summary.first
            && edge.unit as usize == entries.len()
            && summary.last == Some(edge)
            && !(summary.before
                || summary.after
                || summary.hang_before
                || summary.hang_after
                || summary.autospace_before
                || summary.autospace_after)
            && let Ok(cost) = i32::try_from(summary.cost)
        {
            Stored::Leaf(Leaf {
                tracking: edge.tracking,
                cost,
                box_node: edge.box_node,
                punct: edge.punct,
                kind: edge.kind,
                class: edge.class,
            })
        } else {
            let index = full.len();
            full.push(summary);
            Stored::Full(index)
        };
        entries.push(stored);
        // Account for Vec capacity, not just exception count. Ligature-heavy
        // or resource-limited paragraphs must not retain more than the old
        // dense representation. Conversion happens at most once per build.
        let compact_bytes =
            entries.capacity() * size_of::<Stored>() + full.capacity() * size_of::<Summary>();
        if compact_bytes > entries.capacity() * size_of::<Summary>() {
            let mut dense = Vec::with_capacity(entries.capacity());
            dense.extend(
                entries
                    .iter()
                    .enumerate()
                    .map(|(unit, s)| s.resolve(unit, full)),
            );
            self.0 = Storage::Dense(dense);
        }
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> Summary {
        match &self.0 {
            Storage::Compact { entries, full } => entries[index].resolve(index, full),
            Storage::Dense(entries) => entries[index],
        }
    }

    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> usize {
        match &self.0 {
            Storage::Compact { entries, full } => {
                entries.capacity() * size_of::<Stored>() + full.capacity() * size_of::<Summary>()
            }
            Storage::Dense(entries) => entries.capacity() * size_of::<Summary>(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_edges_preserve_large_costs_and_nonlocal_unit_indices() {
        let mut summaries = Summaries::with_capacity(32);
        let mut expected = Vec::new();
        for (unit, cost) in [(0, i64::MIN), (1, i64::MAX), (91, -12)] {
            let edge = Edge {
                unit,
                ..Default::default()
            };
            let summary = Summary {
                first: Some(edge),
                last: Some(edge),
                cost,
                ..Default::default()
            };
            expected.push(summary);
            summaries.push(summary);
        }
        for (index, summary) in expected.into_iter().enumerate() {
            assert_eq!(summaries.get(index), summary);
        }
    }
    #[test]
    fn compact_summaries_preserve_precomputed_leaf_costs_and_empty_barriers() {
        let edge = Edge {
            tracking: 42,
            kind: Kind::Cursive,
            unit: 0,
            box_node: 3,
            class: crate::line::autospace::Class::Digit,
            punct: 0,
        };
        let mut expected = vec![Summary {
            first: Some(edge),
            last: Some(edge),
            cost: -12,
            ..Default::default()
        }];
        for flags in 0..64 {
            expected.push(Summary {
                before: flags & 1 != 0,
                after: flags & 2 != 0,
                hang_before: flags & 4 != 0,
                hang_after: flags & 8 != 0,
                autospace_before: flags & 16 != 0,
                autospace_after: flags & 32 != 0,
                ..Default::default()
            });
        }
        let mut summaries = Summaries::with_capacity(expected.len());
        for &summary in &expected {
            summaries.push(summary);
        }
        assert!(summaries.retained_bytes() <= 32 * expected.len());
        for (index, &summary) in expected.iter().enumerate() {
            assert_eq!(summaries.get(index), summary);
        }
    }

    #[test]
    fn exception_capacity_falls_back_to_dense_without_losing_earlier_entries() {
        let mut expected = vec![Summary::default(); 3];
        let edge = Edge {
            punct: 0,
            ..Default::default()
        };
        expected[0] = Summary {
            first: Some(edge),
            last: Some(edge),
            cost: -12,
            ..Default::default()
        };
        let second_edge = Edge {
            unit: 1,
            tracking: -7,
            ..Default::default()
        };
        expected[1] = Summary {
            first: Some(second_edge),
            last: Some(second_edge),
            cost: 23,
            ..Default::default()
        };
        for index in 3..32 {
            expected.push(Summary {
                first: Some(Edge {
                    tracking: 42,
                    unit: index,
                    ..Default::default()
                }),
                last: Some(Edge {
                    tracking: -9,
                    unit: index,
                    kind: Kind::Atomic,
                    box_node: 7,
                    class: crate::line::autospace::Class::Ideograph,
                    ..Default::default()
                }),
                cost: i64::MIN,
                hang_after: true,
                autospace_before: true,
                ..Default::default()
            });
        }
        let mut summaries = Summaries::with_capacity(expected.len());
        for (end, &summary) in expected.iter().enumerate() {
            summaries.push(summary);
            assert!(summaries.retained_bytes() <= 56 * expected.len());
            for (index, &earlier) in expected[..=end].iter().enumerate() {
                assert_eq!(summaries.get(index), earlier);
            }
        }
    }
}
