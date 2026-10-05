//! Source-safe unit opportunities, indexed once before monotone lane matching.
use super::budget::RubyBudget;
use super::cuts::{self, LaneSpan, SafeCut};
use super::prepare::{PreparedBase, PreparedLane, PreparedRuby, RubyData};
use crate::analysis::units::{BreakClass, UnitKind};
use crate::limits::{LimitExceeded, LimitKind};
use crate::paragraph::ParagraphData;
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
pub(super) fn visit() {
    #[cfg(test)]
    VISITS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(super) fn take_visits() -> usize {
    VISITS.with(|count| count.replace(0))
}

struct Index {
    ordinals: Vec<usize>,
    classes: Vec<BreakClass>,
    emergency_min_content: Vec<bool>,
}

impl Index {
    fn new(data: &ParagraphData) -> Self {
        let mut ordinals = Vec::with_capacity(data.units.len() + 1);
        let mut classes = vec![BreakClass::Prohibited; data.units.len() + 1];
        let mut emergency_min_content = vec![false];
        ordinals.push(0);
        let mut character = 0;
        let mut caret = 0;
        let mut ordinal = 0;
        for (i, unit) in data.units.iter().enumerate() {
            visit();
            let cursor = i + 1;
            if matches!(
                unit.kind,
                UnitKind::Cluster { .. } | UnitKind::Atomic { .. } | UnitKind::Tab
            ) {
                if matches!(unit.kind, UnitKind::Cluster { .. }) {
                    while character < data.breaks.typographic_starts.len()
                        && data.breaks.typographic_starts[character] < unit.text.end
                    {
                        visit();
                        let start = data.breaks.typographic_starts[character];
                        while caret < data.breaks.caret_cuts.len()
                            && data.breaks.caret_cuts[caret] < start
                        {
                            visit();
                            caret += 1;
                        }
                        if start >= unit.text.start
                            && data.breaks.caret_cuts.get(caret) == Some(&start)
                        {
                            ordinal += 1;
                        }
                        character += 1;
                    }
                } else {
                    ordinal += 1;
                }
                classes[cursor] = unit.break_after;
                if unit
                    .shared_cluster
                    .as_ref()
                    .is_some_and(|c| cursor < c.units.end)
                    || unit
                        .combine
                        .is_some_and(|c| cursor < data.combine_spans[c as usize].units.end)
                {
                    classes[cursor] = BreakClass::Prohibited;
                }
            } else if matches!(
                unit.kind,
                UnitKind::ForcedBreak | UnitKind::BlockInInline { .. }
            ) {
                classes[cursor] = BreakClass::Mandatory;
            }
            ordinals.push(ordinal);
            emergency_min_content.push(
                if matches!(unit.kind, UnitKind::Close { .. } | UnitKind::BidiControl) {
                    emergency_min_content[i]
                } else {
                    unit.emergency_min_content
                },
            );
        }
        let mut result = Self {
            ordinals,
            classes,
            emergency_min_content,
        };
        for ruby in data.ruby.containers.iter().rev() {
            result.restrict(ruby);
        }
        result
    }

    fn restrict(&mut self, ruby: &PreparedRuby) {
        for class in &mut self.classes[ruby.units.start + 1..ruby.units.end] {
            visit();
            *class = BreakClass::Prohibited;
        }
        for cut in &ruby.cuts {
            visit();
            self.classes[cut.unit] = cut.class;
        }
    }

    fn safe(&self, range: Range<usize>) -> Vec<SafeCut> {
        let origin = self.ordinals[range.start];
        let mut cuts = vec![SafeCut {
            ordinal: 0,
            unit: range.start,
            class: BreakClass::Prohibited,
        }];
        for unit in range.start + 1..range.end {
            visit();
            let class = self.classes[unit];
            if class != BreakClass::Prohibited {
                cuts.push(SafeCut {
                    ordinal: self.ordinals[unit] - origin,
                    unit,
                    class,
                });
            }
        }
        if range.start != range.end {
            cuts.push(SafeCut {
                ordinal: self.ordinals[range.end] - origin,
                unit: range.end,
                class: BreakClass::Allowed,
            });
        }
        cuts
    }

    fn paragraph_cuts(&self, data: &ParagraphData) -> Vec<SafeCut> {
        let mut content_end = 0;
        for (i, unit) in data.units.iter().enumerate().rev() {
            visit();
            if !matches!(
                unit.kind,
                UnitKind::Open { .. } | UnitKind::Close { .. } | UnitKind::BidiControl
            ) {
                content_end = i + 1;
                break;
            }
        }
        for ruby in &data.ruby.containers {
            visit();
            if !ruby.lanes.is_empty() || ruby.columns.iter().any(|b| !b.text.is_empty()) {
                content_end = content_end.max(ruby.units.end);
            }
        }
        let mut cuts = self.safe(0..data.units.len());
        // A completed nested ruby can leave only enclosing isolate/inline
        // closers. They belong to the endpoint, not an empty continuation.
        cuts.retain(|cut| {
            cut.unit == 0
                || cut.unit == data.units.len()
                || cut.unit < content_end
                || cut.class == BreakClass::Mandatory
        });
        cuts
    }

    fn bases(
        &self,
        range: Range<usize>,
        columns: &[PreparedBase],
        lanes: &[PreparedLane],
    ) -> Vec<SafeCut> {
        let mut cuts = self.safe(range.clone());
        // The endpoint of each base consumes its closing inline/isolate
        // controls as well. This makes the next base's continuation unambiguous.
        let mut column = 0;
        for cut in cuts.iter_mut().skip(1) {
            visit();
            while column < columns.len() && columns[column].units.end <= cut.unit {
                visit();
                column += 1;
            }
            if let Some(base) = columns.get(column)
                && cut.unit < base.units.end
                && self.ordinals[cut.unit] == self.ordinals[base.units.end]
                && cut.class != BreakClass::Mandatory
            {
                cut.unit = if column + 1 == columns.len() {
                    range.end
                } else {
                    base.units.end
                };
            }
        }
        // A true spanning annotation forbids a cut between its bases. Merged
        // annotations retain their separate pairings and are grouped at layout.
        let mut coverage = vec![0isize; columns.len() + 1];
        for lane in lanes {
            visit();
            let start = lane.columns.start;
            let end = lane.columns.end;
            if start + 1 < end {
                coverage[start + 1] += 1;
                coverage[end] -= 1;
            }
        }
        let mut covered = 0;
        let mut boundary = 0;
        cuts.retain(|cut| {
            visit();
            while boundary < columns.len() && columns[boundary].units.end < cut.unit {
                visit();
                covered += coverage[boundary + 1];
                boundary += 1;
            }
            if boundary < columns.len() && cut.unit == columns[boundary].units.end {
                let crosses = covered + coverage[boundary + 1] > 0;
                !crosses || cut.class == BreakClass::Mandatory || cut.unit == range.end
            } else {
                true
            }
        });
        cuts.dedup_by_key(|c| c.unit);
        cuts
    }
}

pub(super) fn prepare_cuts(
    data: &mut ParagraphData,
    containers: &mut [PreparedRuby],
    budget: &mut RubyBudget,
    normal: Option<(&RubyData, &[Option<u32>])>,
    bases: &mut super::base_budget::BaseScopes,
) -> Result<(), LimitExceeded> {
    let mut index = Index::new(data);
    for (container, ruby) in containers.iter_mut().enumerate().rev() {
        let origin = index.ordinals[ruby.units.start];
        let spans: Vec<_> = ruby
            .lanes
            .iter()
            .map(|lane| {
                let first = &ruby.columns[lane.columns.start];
                let last = &ruby.columns[lane.columns.end - 1];
                let end = if lane.columns.end == ruby.columns.len() {
                    ruby.units.end
                } else {
                    last.units.end
                };
                LaneSpan {
                    units: first.units.start..end,
                    ordinals: index.ordinals[first.units.start] - origin
                        ..index.ordinals[end] - origin,
                }
            })
            .collect();
        let base = index.bases(ruby.units.clone(), &ruby.columns, &ruby.lanes);
        let lanes: Vec<_> = ruby
            .lanes
            .iter()
            .map(|lane| {
                let data = &lane.paragraph.data;
                Index::new(data).paragraph_cuts(data)
            })
            .collect();
        // Reserve candidate cells and monotone lane walks for both count and
        // build, even when most candidate rows will be rejected. The first-line
        // source-matching path shares this same budget. Its binary searches are
        // additionally bounded by the existing input-array limits.
        let lane_cuts = lanes
            .iter()
            .fold(0u64, |sum, lane| sum.saturating_add(lane.len() as u64));
        let work = (base.len() as u64)
            .saturating_mul((lanes.len() as u64).saturating_add(1))
            .saturating_add(lane_cuts)
            .saturating_mul(2);
        bases.container_cost(container, LimitKind::RubyCutWork, work)?;
        budget.charge(LimitKind::RubyCutWork, work)?;
        // Count the exact retained table before allocating its lane-cell product.
        let count = if let Some((normal, cursors)) = normal {
            let mut count = 0;
            source_matched_cuts(
                &base,
                &lanes,
                &normal.containers[container],
                cursors,
                |_, _, _| count += 1,
            );
            count
        } else {
            cuts::count_spanned(&base, &lanes, &spans)
        };
        bases.container(
            container,
            (count as u64).saturating_mul(1 + lanes.len() as u64),
        )?;
        budget.charge(
            LimitKind::Items,
            (count as u64).saturating_mul(1 + lanes.len() as u64),
        )?;
        ruby.cuts = if let Some((normal, cursors)) = normal {
            let mut paired = cuts::PairedBuilder::new(count, lanes.len());
            source_matched_cuts(
                &base,
                &lanes,
                &normal.containers[container],
                cursors,
                |unit, lanes, class| {
                    paired.push(unit, lanes, class);
                },
            );
            paired.finish()
        } else if spans
            .iter()
            .all(|s| s.units == ruby.units && s.ordinals.start == 0)
        {
            cuts::build_counted(&base, &lanes, count)
        } else {
            cuts::build_spanned_counted(&base, &lanes, &spans, count)
        };
        index.restrict(ruby);
    }
    // Install the coordinated opportunities in the ordinary unit stream so
    // every existing greedy/intrinsic/plan consumer sees the same legal cuts.
    // Preserve normal CSS at container endpoints; an endpoint is always a
    // correspondence cursor, but is not necessarily an external soft break.
    let externals: Vec<_> = containers
        .iter()
        .map(|ruby| {
            data.units[ruby.units.clone()]
                .iter()
                .rev()
                .find(|u| {
                    !matches!(
                        u.kind,
                        UnitKind::Open { .. } | UnitKind::Close { .. } | UnitKind::BidiControl
                    )
                })
                .map_or(BreakClass::Prohibited, |u| u.break_after)
        })
        .collect();
    for (ruby, external) in containers.iter().zip(externals).rev() {
        for (i, unit) in data.units[ruby.units.clone()].iter_mut().enumerate() {
            unit.break_after = index.classes[ruby.units.start + i + 1];
            unit.emergency_min_content = index.emergency_min_content[ruby.units.start + i + 1];
        }
        if let Some(last) = data.units.get_mut(ruby.units.end - 1) {
            last.break_after = if ruby.units.end == index.classes.len() - 1
                || index.classes[ruby.units.end] == BreakClass::Prohibited
            {
                BreakClass::Prohibited
            } else {
                external
            };
        }
    }
    Ok(())
}

/// The parent token cannot retain independent progress for every reading.
/// Therefore an alternate cut must consume exactly the sources identified by
/// the corresponding normal cut, with safe boundaries in both shaped sets.
fn source_matched_cuts(
    bases: &[SafeCut],
    alternate_lanes: &[Vec<SafeCut>],
    normal: &PreparedRuby,
    parent_cursors: &[Option<u32>],
    mut emit: impl FnMut(usize, &[usize], BreakClass),
) {
    let mut selected = Vec::with_capacity(alternate_lanes.len());
    let mut previous = None;
    for base in bases {
        visit();
        let Some(normal_unit) = parent_cursors.get(base.unit).copied().flatten() else {
            continue;
        };
        let index = normal
            .cuts
            .partition_point(|cut| cut.unit < normal_unit as usize);
        let Some(cut) = normal
            .cuts
            .get(index)
            .filter(|cut| cut.unit == normal_unit as usize)
        else {
            continue;
        };
        if previous == Some(index) {
            continue;
        }
        selected.clear();
        let mut class = base.class;
        for (lane, opportunities) in alternate_lanes.iter().enumerate() {
            visit();
            let child = &normal.lanes[lane].paragraph.data;
            let unit = if let Some(first) = &child.first_line {
                let Some(unit) = first.alternate_cursor(cut.lanes[lane] as u32) else {
                    break;
                };
                unit
            } else {
                cut.lanes[lane]
            };
            let index = opportunities.partition_point(|cut| cut.unit < unit);
            let Some(safe) = opportunities.get(index).filter(|cut| cut.unit == unit) else {
                break;
            };
            selected.push(unit);
            class = cuts::combined(class, safe.class);
        }
        if selected.len() == alternate_lanes.len() {
            emit(base.unit, &selected, class);
            previous = Some(index);
        }
    }
}

/// Preorder source intervals, augmented with each subtree's furthest end.
/// Unlike a flat start search this keeps ancestors crossing a continuation,
/// and unlike a paragraph scan it skips unrelated siblings in logarithmic work.
#[derive(Default)]
pub(crate) struct ContainerIndex {
    ends: Vec<usize>,
    leaves: usize,
}

impl ContainerIndex {
    pub(crate) fn new(
        containers: &[PreparedRuby],
        budget: &mut RubyBudget,
        bases: &mut super::base_budget::BaseScopes,
    ) -> Result<Self, LimitExceeded> {
        if containers.is_empty() {
            return Ok(Self::default());
        }
        debug_assert!(
            containers
                .windows(2)
                .all(|pair| pair[0].units.start <= pair[1].units.start)
        );
        let leaves = containers.len().next_power_of_two();
        budget.charge(LimitKind::Items, (leaves as u64).saturating_mul(2))?;
        bases.index(containers.len(), leaves)?;
        let mut ends = vec![0; leaves * 2];
        for (i, ruby) in containers.iter().enumerate() {
            visit();
            ends[leaves + i] = ruby.units.end;
        }
        for i in (1..leaves).rev() {
            visit();
            ends[i] = ends[i * 2].max(ends[i * 2 + 1]);
        }
        Ok(Self { ends, leaves })
    }

    /// Visit intersecting containers in structural order. The callback may
    /// extend `through` to the next legal paired endpoint; later siblings use
    /// that new bound, preserving the candidate look-ahead contract.
    ///
    /// Containers are sorted by source start, so the visit is a left-to-right
    /// fold: a subtree whose furthest end is `<= start` is skipped and the
    /// walk continues, while the first container starting at or after
    /// `through` stops it (every later container starts there too, and
    /// `through` changes only inside `emit`).
    pub(crate) fn intersecting(
        &self,
        containers: &[PreparedRuby],
        start: usize,
        through: &mut usize,
        emit: impl FnMut(usize, &mut usize),
    ) {
        self.intersecting_from(containers, start, 0, through, emit);
    }

    /// `intersecting` restricted to container indices `>= from`, so a walk
    /// stopped at `from` can resume with a larger `through`. The stop test
    /// looks at the first leaf `>= from` of each subtree; anything it prunes
    /// the full walk would stop at as well, so the visit order of the full
    /// walk's suffix is unchanged.
    pub(crate) fn intersecting_from(
        &self,
        containers: &[PreparedRuby],
        start: usize,
        from: usize,
        through: &mut usize,
        mut emit: impl FnMut(usize, &mut usize),
    ) {
        if self.leaves == 0 || start >= *through || from >= containers.len() {
            return;
        }
        self.walk(
            containers,
            1,
            0..self.leaves,
            start,
            from,
            through,
            &mut emit,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        containers: &[PreparedRuby],
        node: usize,
        range: Range<usize>,
        start: usize,
        from: usize,
        through: &mut usize,
        emit: &mut impl FnMut(usize, &mut usize),
    ) {
        visit();
        let first = range.start.max(from);
        if range.end <= from
            || self.ends[node] <= start
            || first >= containers.len()
            || containers[first].units.start >= *through
        {
            return;
        }
        if range.len() == 1 {
            emit(range.start, through);
        } else {
            let middle = range.start + range.len() / 2;
            self.walk(
                containers,
                node * 2,
                range.start..middle,
                start,
                from,
                through,
                emit,
            );
            self.walk(
                containers,
                node * 2 + 1,
                middle..range.end,
                start,
                from,
                through,
                emit,
            );
        }
    }
}
