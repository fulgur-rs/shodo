//! Position-only profiles for clipped, unsized groups in styled-break datasets.
use super::MetricIndex;
use super::scalar::{Bounds, union};
use crate::LayoutContext;
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Ghost {
    pub(super) all: Option<Bounds>,
    pub(super) bare: Option<Bounds>,
}

impl Ghost {
    pub(super) fn join(self, other: Self) -> Self {
        Self {
            all: union(self.all, other.all),
            bare: union(self.bare, other.bare),
        }
    }
}

impl MetricIndex {
    pub(super) fn ghost_query(
        &self,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, Bounds>,
        removed: &[Range<usize>],
        cx: &mut LayoutContext,
    ) -> Option<Bounds> {
        self.ghost_node(
            1,
            0..self.ghosts.len() / 2,
            range,
            replacements,
            removed,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn ghost_node(
        &self,
        node: usize,
        source: Range<usize>,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, Bounds>,
        removed: &[Range<usize>],
        _cx: &mut LayoutContext,
    ) -> Option<Bounds> {
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if source.end <= range.start || range.end <= source.start {
            return None;
        }
        if range.start <= source.start
            && source.end <= range.end
            && replacements.range(source.clone()).next().is_none()
        {
            if removed
                .iter()
                .any(|r| r.start <= source.start && source.end <= r.end)
            {
                return self.ghosts[node].bare;
            }
            if !removed
                .iter()
                .any(|r| r.start < source.end && source.start < r.end)
            {
                return self.ghosts[node].all;
            }
        }
        if source.end - source.start == 1 {
            if let Some(replacement) = replacements.get(&source.start) {
                return union(self.ghosts[node].bare, Some(*replacement));
            }
            return if removed.iter().any(|r| r.contains(&source.start)) {
                self.ghosts[node].bare
            } else {
                self.ghosts[node].all
            };
        }
        let mid = (source.start + source.end) / 2;
        union(
            self.ghost_node(
                node * 2,
                source.start..mid,
                range,
                replacements,
                removed,
                _cx,
            ),
            self.ghost_node(
                node * 2 + 1,
                mid..source.end,
                range,
                replacements,
                removed,
                _cx,
            ),
        )
    }
}
