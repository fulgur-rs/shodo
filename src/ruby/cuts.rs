//! Monotone nearest-progress correspondence of already validated safe cuts.
//! Preparation supplies only legal grapheme/cluster/composition boundaries.
use crate::analysis::units::BreakClass;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(crate) struct LaneSpan {
    pub(crate) units: Range<usize>,
    pub(crate) ordinals: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SafeCut {
    pub(crate) ordinal: usize,
    pub(crate) unit: usize,
    pub(crate) class: BreakClass,
}

#[derive(Clone, Debug)]
pub(crate) struct PairedCut {
    pub(crate) unit: usize,
    #[allow(dead_code)] // line measurement consumes these cursors in Task 3
    pub(crate) lanes: Vec<usize>,
    pub(crate) class: BreakClass,
}

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
pub(crate) fn take_visits() -> usize {
    VISITS.with(|count| count.replace(0))
}

fn distance(base: SafeCut, base_total: usize, lane: SafeCut, lane_total: usize) -> u128 {
    (base.ordinal as u128 * lane_total as u128).abs_diff(lane.ordinal as u128 * base_total as u128)
}

fn next_mandatory(lane: &[SafeCut], after: usize) -> usize {
    let mut index = after;
    while index < lane.len() {
        visit();
        if lane[index].class == BreakClass::Mandatory {
            break;
        }
        index += 1;
    }
    index
}

pub(super) fn combined(a: BreakClass, b: BreakClass) -> BreakClass {
    if a == BreakClass::Mandatory || b == BreakClass::Mandatory {
        BreakClass::Mandatory
    } else if a == BreakClass::Emergency || b == BreakClass::Emergency {
        BreakClass::Emergency
    } else if a == BreakClass::Hyphen || b == BreakClass::Hyphen {
        BreakClass::Hyphen
    } else {
        a
    }
}

pub(crate) fn build(base: &[SafeCut], lanes: &[Vec<SafeCut>]) -> Vec<PairedCut> {
    let Some(start) = base.first() else {
        return Vec::new();
    };
    let end = base.last().unwrap();
    let spans = vec![
        LaneSpan {
            units: start.unit..end.unit,
            ordinals: start.ordinal..end.ordinal
        };
        lanes.len()
    ];
    build_spanned(base, lanes, &spans)
}

/// A lane participates only in the interval of bases it actually annotates.
/// Before/after that interval its cursor is fixed and cannot constrain a cut.
pub(crate) fn build_spanned(
    base: &[SafeCut],
    lanes: &[Vec<SafeCut>],
    spans: &[LaneSpan],
) -> Vec<PairedCut> {
    let mut result = Vec::new();
    walk_spanned(base, lanes, spans, |unit, lanes, class| {
        result.push(PairedCut {
            unit,
            lanes: lanes.to_vec(),
            class,
        });
    });
    result
}

pub(crate) fn count_spanned(base: &[SafeCut], lanes: &[Vec<SafeCut>], spans: &[LaneSpan]) -> usize {
    let mut count = 0;
    walk_spanned(base, lanes, spans, |_, _, _| count += 1);
    count
}

fn walk_spanned(
    base: &[SafeCut],
    lanes: &[Vec<SafeCut>],
    spans: &[LaneSpan],
    mut emit: impl FnMut(usize, &[usize], BreakClass),
) {
    assert_eq!(lanes.len(), spans.len(), "every lane has a base interval");
    let Some(start) = base.first() else {
        return;
    };
    // Empty lanes are inactive rather than synthetic source paragraphs.
    let mut closest = vec![0usize; lanes.len()];
    let mut last = vec![0usize; lanes.len()];
    let mut mandatory: Vec<_> = lanes.iter().map(|lane| next_mandatory(lane, 1)).collect();
    let mut cursors: Vec<_> = lanes
        .iter()
        .map(|lane| lane.first().map_or(0, |cut| cut.unit))
        .collect();
    emit(start.unit, &cursors, start.class);
    let mut selected = Vec::with_capacity(lanes.len());
    let end = *base.last().unwrap();
    for cut in base.iter().skip(1).take(base.len().saturating_sub(2)) {
        visit();
        if cut.class == BreakClass::Prohibited {
            continue;
        }
        let forced = cut.class == BreakClass::Mandatory;
        selected.clear();
        let mut class = cut.class;
        let mut legal = true;
        for (i, lane) in lanes.iter().enumerate() {
            let Some(lane_end) = lane.last() else {
                selected.push(0);
                continue;
            };
            let span = &spans[i];
            if cut.unit <= span.units.start {
                selected.push(0);
                continue;
            }
            if cut.unit >= span.units.end {
                selected.push(lane.len() - 1);
                continue;
            }
            let local = SafeCut {
                ordinal: cut.ordinal.saturating_sub(span.ordinals.start),
                ..*cut
            };
            let total = span.ordinals.end.saturating_sub(span.ordinals.start);
            let mut index = closest[i];
            while index + 1 < lane.len() {
                visit();
                if distance(local, total, lane[index + 1], lane_end.ordinal)
                    >= distance(local, total, lane[index], lane_end.ordinal)
                {
                    break;
                }
                index += 1;
            }
            closest[i] = index;
            // Never step over a mandatory child cut in pursuit of a closer
            // regular opportunity. The next forced cut wins first.
            if mandatory[i] <= index && mandatory[i] < lane.len() {
                index = mandatory[i];
            }
            if forced && index <= last[i] {
                // Mandatory parent breaks override nowrap. An exhausted lane
                // remains empty on subsequent forced base fragments.
                index = (last[i] + 1).min(lane.len() - 1);
            }
            if !forced && (index <= last[i] || index + 1 == lane.len()) {
                legal = false;
            }
            class = combined(class, lane[index].class);
            selected.push(index);
        }
        if !legal {
            continue;
        }
        for (i, index) in selected.iter().copied().enumerate() {
            last[i] = index;
            if mandatory[i] <= index {
                mandatory[i] = next_mandatory(&lanes[i], index + 1);
            }
        }
        for (i, &index) in selected.iter().enumerate() {
            cursors[i] = lanes[i].get(index).map_or(0, |cut| cut.unit);
        }
        emit(cut.unit, &cursors, class);
    }
    if end.unit != start.unit {
        for (i, lane) in lanes.iter().enumerate() {
            cursors[i] = lane.last().map_or(0, |cut| cut.unit);
        }
        emit(end.unit, &cursors, end.class);
    }
}
