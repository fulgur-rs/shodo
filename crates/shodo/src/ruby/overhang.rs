//! Indexed actual font-content geometry for ruby edge allowances.
use super::geometry::Bounds;
use super::prepare::PreparedRuby;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext};
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct NeighborIndex {
    box_units: Vec<Range<usize>>,
    neighbor_boxes: std::collections::HashMap<(u32, usize), Vec<u32>>,
    neighbors: crate::line::spacing_summary::VisualNeighbors,
    columns: crate::line::spacing_summary::VisualNeighbors,
}

fn combine(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, None) | (None, a) => a,
    }
}
impl NeighborIndex {
    fn new(data: &ParagraphData, _cx: &mut LayoutContext) -> Self {
        let mut box_units = vec![0..0; data.boxes.len()];
        for (i, u) in data.units.iter().enumerate() {
            #[cfg(test)]
            {
                _cx.ruby_measure_visits += 1;
            }
            match u.kind {
                UnitKind::Open { box_index } => box_units[box_index as usize].start = i,
                UnitKind::Close { box_index } => box_units[box_index as usize].end = i + 1,
                _ => {}
            }
        }
        let event = |i: usize| match data.units[i].kind {
            UnitKind::Cluster { .. }
            | UnitKind::Atomic { .. }
            | UnitKind::Tab
            | UnitKind::ForcedBreak
            | UnitKind::BlockInInline { .. } => true,
            UnitKind::Open { box_index } | UnitKind::Close { box_index } => {
                let e = data.boxes[box_index as usize].edges;
                e.inline_start_total() != 0.0 || e.inline_end_total() != 0.0
            }
            _ => matches!(
                data.items[data.units[i].item as usize].kind,
                crate::analysis::ItemKind::RubyBoundary { .. }
            ),
        };
        let neighbors = crate::line::spacing_summary::VisualNeighbors::new(
            data.units
                .iter()
                .enumerate()
                .map(|(i, u)| (u.level, event(i))),
        );
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += data.units.len();
        }
        let mut content_counts = vec![0];
        for u in &data.units {
            let content = matches!(
                u.kind,
                UnitKind::Cluster { .. }
                    | UnitKind::Atomic { .. }
                    | UnitKind::Tab
                    | UnitKind::ForcedBreak
                    | UnitKind::BlockInInline { .. }
            );
            content_counts.push(content_counts.last().unwrap() + usize::from(content));
        }
        let columns = crate::line::spacing_summary::VisualNeighbors::rendered(
            data.units.iter().enumerate().filter_map(|(i, u)| {
                let content = matches!(
                    u.kind,
                    UnitKind::Cluster { .. } | UnitKind::Atomic { .. } | UnitKind::Tab
                );
                let empty = if let crate::analysis::ItemKind::RubyBoundary {
                    ruby,
                    boundary: super::builder::Boundary::BaseClose(column),
                } = data.items[u.item as usize].kind
                {
                    let units = &data.ruby.containers[ruby as usize].columns[column].units;
                    content_counts[units.end] == content_counts[units.start]
                } else {
                    false
                };
                // Match retained bidi pieces: source-only controls do not enter
                // L2 reordering. An empty column's reserved closing slot does.
                let piece = content
                    || empty
                    || matches!(
                        u.kind,
                        UnitKind::Open { .. }
                            | UnitKind::Close { .. }
                            | UnitKind::Float { .. }
                            | UnitKind::Absolute { .. }
                    );
                piece.then_some((i, u.level, content || empty))
            }),
        );
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += data.units.len() * 2;
        }
        Self {
            box_units,
            neighbor_boxes: Default::default(),
            neighbors,
            columns,
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn neighbor_bounds(
        &mut self,
        data: &ParagraphData,
        selected: &Range<usize>,
        unit: usize,
        ruby_start: usize,
        atomics: &AtomicSizes,
        share: &mut crate::line::metric_index::ProfileShare,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Option<Bounds> {
        let mut boxes = vec![None];
        if let Some(parent) = data.units[unit].parent_box {
            let independent = self
                .neighbor_boxes
                .entry((parent, ruby_start))
                .or_insert_with(|| {
                    let mut boxes = Vec::new();
                    let mut cursor = Some(parent);
                    while let Some(b) = cursor {
                        #[cfg(test)]
                        {
                            cx.ruby_measure_visits += 1;
                        }
                        // Common ancestors are not independent neighboring paint.
                        if self.box_units[b as usize].contains(&ruby_start) {
                            break;
                        }
                        boxes.push(b);
                        cursor = data.boxes[b as usize].parent;
                    }
                    boxes
                });
            boxes.extend(independent.iter().map(|b| Some(*b)));
        }
        let range = unit..unit + 1;
        let geometry = crate::line::metric_index::content_shared(
            data,
            selected.clone(),
            std::slice::from_ref(&range),
            &boxes,
            atomics,
            share,
            cx,
            sat,
        );
        geometry.paints[1..]
            .iter()
            .fold(geometry.leaf_areas[0], |bounds, paint| {
                combine(bounds, Some(*paint))
            })
    }
}

/// Neighbor index value slot; see `line::metric_index::take_index`.
fn take_neighbors(data: &ParagraphData, cx: &mut LayoutContext) -> Box<NeighborIndex> {
    let key = (data.id, data as *const ParagraphData as usize);
    match cx
        .ruby_ranges
        .neighbors
        .get_mut(&key)
        .and_then(Option::take)
    {
        Some(index) => index,
        None => Box::new(NeighborIndex::new(data, cx)),
    }
}

fn put_neighbors(data: &ParagraphData, index: Box<NeighborIndex>, cx: &mut LayoutContext) {
    let key = (data.id, data as *const ParagraphData as usize);
    *cx.ruby_ranges.neighbors.entry(key).or_default() = Some(index);
}

/// Physical line-right column, from the same sparse pieces used by retained L2.
pub(crate) fn rightmost_column(
    data: &ParagraphData,
    bases: &[Range<usize>],
    columns: &Range<usize>,
    cx: &mut LayoutContext,
) -> usize {
    let index = take_neighbors(data, cx);
    let target = bases[columns.start].start..bases[columns.end - 1].end;
    let (_, right) = index.columns.edges(target, false);
    #[cfg(test)]
    {
        cx.ruby_measure_visits += index.columns.take_visits();
    }
    put_neighbors(data, index, cx);
    right
        .and_then(|u| {
            let i = bases.partition_point(|b| b.end <= u);
            bases.get(i).filter(|b| b.contains(&u)).map(|_| i)
        })
        .unwrap_or(columns.end - 1)
}

pub(crate) struct ColumnGeometry {
    pub(crate) contents: Vec<Bounds>,
    pub(crate) area: Bounds,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn columns(
    data: &ParagraphData,
    ruby: &PreparedRuby,
    column_start: usize,
    selected: &Range<usize>,
    bases: &[Range<usize>],
    atomics: &AtomicSizes,
    share: &mut crate::line::metric_index::ProfileShare,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> ColumnGeometry {
    let boxes: Vec<_> = ruby.columns[column_start..column_start + bases.len()]
        .iter()
        .map(|c| c.box_index.or(ruby.box_index))
        .collect();
    let geometry = crate::line::metric_index::content_shared(
        data,
        selected.clone(),
        bases,
        &boxes,
        atomics,
        share,
        cx,
        sat,
    );
    let mut area = None;
    for (base, (content, bounds)) in bases
        .iter()
        .zip(geometry.contents.iter().zip(&geometry.areas))
    {
        if !base.is_empty() {
            area = combine(area, combine(Some(*content), *bounds));
        }
    }
    let area = area.unwrap_or_else(|| {
        crate::line::metric_index::content_shared(
            data,
            selected.clone(),
            &[],
            &[ruby.box_index],
            atomics,
            share,
            cx,
            sat,
        )
        .contents[0]
    });
    ColumnGeometry {
        contents: geometry.contents,
        area,
    }
}

/// An actual same-line neighbor must be plain text whose block-axis content
/// clears the annotation track. Source-free box/control boundaries are skipped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn allowances(
    data: &ParagraphData,
    ruby: &PreparedRuby,
    selected: &Range<usize>,
    bases: &[Range<usize>],
    columns: &Range<usize>,
    before_track: bool,
    area: Bounds,
    cap: LayoutUnit,
    atomics: &AtomicSizes,
    share: &mut crate::line::metric_index::ProfileShare,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (LayoutUnit, LayoutUnit) {
    if cap == LayoutUnit::ZERO || columns.is_empty() {
        return (LayoutUnit::ZERO, LayoutUnit::ZERO);
    }
    // Move the scalar index out only while querying, so counters and bounded
    // actual advance queries can use the same context without cloning tables.
    let mut index = take_neighbors(data, cx);
    let target = ruby.units.start.max(selected.start)..ruby.units.end.min(selected.end);
    let (before, after) = index
        .neighbors
        .around(selected, &target, data.base_level % 2 == 1);
    #[cfg(test)]
    {
        cx.ruby_measure_visits += index.neighbors.take_visits();
    }
    let (first, last) = index.columns.edges(target, data.base_level % 2 == 1);
    #[cfg(test)]
    {
        cx.ruby_measure_visits += index.columns.take_visits();
    }
    let column_at = |unit: Option<usize>| {
        unit.and_then(|u| {
            let i = bases.partition_point(|b| b.end <= u);
            bases.get(i).filter(|b| b.contains(&u)).map(|_| i)
        })
    };
    let mut allowance = |i: Option<usize>| {
        let Some(i) = i else {
            return LayoutUnit::ZERO;
        };
        if i < selected.start || i >= selected.end {
            return LayoutUnit::ZERO;
        }
        let u = &data.units[i];
        if !matches!(u.kind, UnitKind::Cluster { space: false, .. })
            || u.shared_cluster.is_some()
            || u.combine.is_some()
        {
            return LayoutUnit::ZERO;
        }
        let Some(neighbor) =
            index.neighbor_bounds(data, selected, i, ruby.units.start, atomics, share, cx, sat)
        else {
            return LayoutUnit::ZERO;
        };
        if (before_track && neighbor.top < area.top)
            || (!before_track && neighbor.bottom > area.bottom)
        {
            return LayoutUnit::ZERO;
        }
        crate::line::ruby_range_width(data, i..i + 1, atomics, cx, sat)
            .min(cap)
            .max(LayoutUnit::ZERO)
    };
    let leading = if column_at(first).is_some_and(|i| columns.contains(&i)) {
        allowance(before)
    } else {
        LayoutUnit::ZERO
    };
    let trailing = if column_at(last).is_some_and(|i| columns.contains(&i)) {
        allowance(after)
    } else {
        LayoutUnit::ZERO
    };
    put_neighbors(data, index, cx);
    (leading, trailing)
}
