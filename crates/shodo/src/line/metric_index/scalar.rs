//! Scalar metric summaries and selected-range queries.
use super::super::metrics::RecordProfile;
use super::MetricIndex;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub(super) top: f32,
    pub(super) bottom: f32,
}
pub(super) fn union(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Bounds {
            top: a.top.min(b.top),
            bottom: a.bottom.max(b.bottom),
        }),
        (a, None) | (None, a) => a,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Summary {
    pub(super) normal: Option<Bounds>,
    pub(super) raw: Option<Bounds>,
    pub(super) height: f32,
    pub(super) bottom_height: f32,
    pub(super) active: bool,
}
impl Summary {
    pub(super) fn join(self, other: Self) -> Self {
        Self {
            normal: union(self.normal, other.normal),
            raw: union(self.raw, other.raw),
            height: self.height.max(other.height),
            bottom_height: self.bottom_height.max(other.bottom_height),
            active: self.active || other.active,
        }
    }
    pub(super) fn profile(p: RecordProfile, active: bool) -> Self {
        let bounds = Some(Bounds {
            top: p.top,
            bottom: p.bottom,
        });
        Self {
            normal: (p.group.is_none() && p.own_group.is_none())
                .then_some(bounds)
                .flatten(),
            raw: bounds,
            active,
            ..Default::default()
        }
    }
}

impl MetricIndex {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn query(
        &self,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, Summary>,
        excluded: &BTreeSet<usize>,
        removed: &[Range<usize>],
        cx: &mut LayoutContext,
    ) -> Summary {
        self.query_node(
            1,
            0..self.tree.len() / 2,
            range,
            replacements,
            excluded,
            removed,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn query_node(
        &self,
        node: usize,
        source: Range<usize>,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, Summary>,
        excluded: &BTreeSet<usize>,
        removed: &[Range<usize>],
        _cx: &mut LayoutContext,
    ) -> Summary {
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if source.end <= range.start || range.end <= source.start {
            return Summary::default();
        }
        if range.start <= source.start
            && source.end <= range.end
            && replacements.range(source.clone()).next().is_none()
            && excluded.range(source.clone()).next().is_none()
        {
            if removed
                .iter()
                .any(|r| r.start <= source.start && source.end <= r.end)
            {
                return self.nonglyph[node];
            }
            if !removed
                .iter()
                .any(|r| r.start < source.end && source.start < r.end)
            {
                return self.tree[node];
            }
        }
        if source.end - source.start == 1 {
            let mut result = replacements.get(&source.start).copied().unwrap_or(
                if removed.iter().any(|r| r.contains(&source.start)) {
                    self.nonglyph[node]
                } else {
                    self.tree[node]
                },
            );
            if excluded.contains(&source.start) {
                result.height = 0.0;
                result.bottom_height = 0.0;
            }
            return result;
        }
        let mid = (source.start + source.end) / 2;
        self.query_node(
            node * 2,
            source.start..mid,
            range,
            replacements,
            excluded,
            removed,
            _cx,
        )
        .join(self.query_node(
            node * 2 + 1,
            mid..source.end,
            range,
            replacements,
            excluded,
            removed,
            _cx,
        ))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ScalarMetrics {
    pub(crate) baseline: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) empty: bool,
}

pub(crate) fn measure(
    data: &ParagraphData,
    range: Range<usize>,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> ScalarMetrics {
    if range.is_empty() {
        return ScalarMetrics {
            baseline: LayoutUnit::ZERO,
            block_size: LayoutUnit::ZERO,
            empty: true,
        };
    }
    #[cfg(test)]
    {
        cx.ruby_scalar_calls += 1;
    }
    let mut index = super::take_index(data, atomics, cx);
    let end = super::super::plan::hyphen_end(data, range.end).filter(|e| *e > range.start);
    let windows = end
        .and_then(|end| super::super::hyphen::line(data, range.start, end, cx, sat))
        .unwrap_or_else(|| super::super::windows::measure(data, range.start, range.end, cx, sat));
    let metrics = index.select(data, &range, &windows, cx, sat).metrics;
    super::put_index(data, index, cx);
    metrics
}
