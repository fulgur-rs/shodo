//! Source-safe unit opportunities, indexed once before monotone lane matching.
use super::budget::RubyBudget;
use super::cuts::{self, LaneSpan, SafeCut};
use super::prepare::{PreparedBase, PreparedLane, PreparedRuby};
use crate::analysis::units::{BreakClass, UnitKind};
use crate::limits::{LimitExceeded, LimitKind};
use crate::paragraph::ParagraphData;
use std::ops::Range;

#[cfg(test)]
thread_local! {
    static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[inline]
fn visit() {
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
}

impl Index {
    fn new(data: &ParagraphData) -> Self {
        let mut ordinals = Vec::with_capacity(data.units.len() + 1);
        let mut classes = vec![BreakClass::Prohibited; data.units.len() + 1];
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
        }
        let mut result = Self { ordinals, classes };
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
    data: &ParagraphData,
    containers: &mut [PreparedRuby],
    budget: &mut RubyBudget,
) -> Result<(), LimitExceeded> {
    let mut index = Index::new(data);
    for ruby in containers.iter_mut().rev() {
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
        // Count the exact retained table before allocating its lane-cell product.
        let count = cuts::count_spanned(&base, &lanes, &spans);
        budget.charge(
            LimitKind::Items,
            (count as u64).saturating_mul(1 + lanes.len() as u64),
        )?;
        ruby.cuts = if spans
            .iter()
            .all(|s| s.units == ruby.units && s.ordinals.start == 0)
        {
            cuts::build(&base, &lanes)
        } else {
            cuts::build_spanned(&base, &lanes, &spans)
        };
        index.restrict(ruby);
    }
    Ok(())
}
