//! Monotone nearest-progress correspondence of already validated safe cuts.
//! Preparation supplies only legal grapheme/cluster/composition boundaries.
use crate::analysis::units::BreakClass;
use std::ops::{Index, Range};
use std::sync::Arc;

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
    pub(crate) lanes: LaneCursors,
    pub(crate) class: BreakClass,
}

/// An immutable view of one emitted row, keyed by row ordinal rather than unit.
/// Parent unit endpoints need not uniquely identify a correspondence row.
#[derive(Clone, Debug)]
pub(crate) struct LaneCursors {
    row: usize,
    table: Arc<CursorTable>,
}

#[derive(Debug)]
enum CursorTable {
    Dense { lanes: usize, values: Vec<usize> },
    Columns(Vec<CursorColumn>),
}

#[derive(Debug)]
enum CursorColumn {
    Sparse {
        initial: usize,
        changes: Vec<(usize, usize)>,
    },
    Dense(Vec<usize>),
}

impl CursorColumn {
    fn at(&self, row: usize) -> &usize {
        match self {
            Self::Dense(values) => &values[row],
            Self::Sparse { initial, changes } => {
                let after = changes.partition_point(|&(changed, _)| changed <= row);
                if after == 0 {
                    initial
                } else {
                    &changes[after - 1].1
                }
            }
        }
    }

    fn expand(&self, rows: usize, capacity: usize) -> Vec<usize> {
        let Self::Sparse { initial, changes } = self else {
            unreachable!("only sparse columns need expansion")
        };
        let mut values = Vec::with_capacity(capacity);
        let mut value = *initial;
        let mut next = 0;
        for row in 0..rows {
            if next < changes.len() && changes[next].0 == row {
                value = changes[next].1;
                next += 1;
            }
            values.push(value);
        }
        values
    }

    fn push(&mut self, row: usize, value: usize, expected: usize) {
        match self {
            Self::Dense(values) => values.push(value),
            Self::Sparse { initial, changes } => {
                if row == 0 {
                    *initial = value;
                } else if changes.last().map_or(*initial, |&(_, last)| last) != value {
                    // Each transition stores two full usize values. Convert once
                    // before transition density costs more than a dense column.
                    if (changes.len() + 1).saturating_mul(2) >= expected {
                        let mut values = self.expand(row, expected);
                        values.push(value);
                        *self = Self::Dense(values);
                    } else {
                        if changes.is_empty() {
                            changes.reserve_exact(1);
                        }
                        changes.push((row, value));
                    }
                }
            }
        }
    }

    fn finish(mut self, rows: usize) -> Self {
        if let Self::Sparse { changes, .. } = &self
            && changes.capacity().saturating_mul(2) > rows
        {
            // Test wrappers use an upper bound; actual legal row count and Vec
            // capacity, rather than that bound, decide final retention.
            self = Self::Dense(self.expand(rows, rows));
        }
        self
    }
}

impl Index<usize> for LaneCursors {
    type Output = usize;
    fn index(&self, lane: usize) -> &usize {
        match &*self.table {
            CursorTable::Dense { lanes, values } => {
                assert!(lane < *lanes);
                &values[self.row * lanes + lane]
            }
            CursorTable::Columns(columns) => columns[lane].at(self.row),
        }
    }
}

/// Storage is built from exactly the old correspondence stream. Production
/// callers have already counted and charged its logical cell product.
pub(crate) struct PairedBuilder {
    expected: usize,
    rows: Vec<(usize, BreakClass)>,
    table: CursorTable,
}

impl PairedBuilder {
    pub(crate) fn new(expected: usize, lanes: usize) -> Self {
        let table = if expected < 8 || lanes == 0 {
            CursorTable::Dense {
                lanes,
                values: Vec::with_capacity(expected.saturating_mul(lanes)),
            }
        } else {
            CursorTable::Columns(
                (0..lanes)
                    .map(|_| CursorColumn::Sparse {
                        initial: 0,
                        changes: Vec::new(),
                    })
                    .collect(),
            )
        };
        Self {
            expected,
            rows: Vec::new(),
            table,
        }
    }

    pub(crate) fn push(&mut self, unit: usize, lanes: &[usize], class: BreakClass) {
        let row = self.rows.len();
        match &mut self.table {
            CursorTable::Dense {
                lanes: count,
                values,
            } => {
                assert_eq!(lanes.len(), *count);
                values.extend_from_slice(lanes);
            }
            CursorTable::Columns(columns) => {
                assert_eq!(lanes.len(), columns.len());
                for (column, &value) in columns.iter_mut().zip(lanes) {
                    column.push(row, value, self.expected);
                }
            }
        }
        self.rows.push((unit, class));
    }

    pub(crate) fn finish(self) -> Vec<PairedCut> {
        let table = match self.table {
            CursorTable::Columns(columns) => CursorTable::Columns(
                columns
                    .into_iter()
                    .map(|column| column.finish(self.rows.len()))
                    .collect(),
            ),
            dense => dense,
        };
        let table = Arc::new(table);
        self.rows
            .into_iter()
            .enumerate()
            .map(|(row, (unit, class))| PairedCut {
                unit,
                class,
                lanes: LaneCursors {
                    row,
                    table: Arc::clone(&table),
                },
            })
            .collect()
    }
}

#[cfg(test)]
impl LaneCursors {
    pub(crate) fn len(&self) -> usize {
        match &*self.table {
            CursorTable::Dense { lanes, .. } => *lanes,
            CursorTable::Columns(columns) => columns.len(),
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &usize> {
        (0..self.len()).map(|lane| &self[lane])
    }
    /// Heap payload capacities, including the shared table value, excluding
    /// Arc's control block/allocator overhead. Count the shared table only once.
    pub(crate) fn table_payload_bytes(&self) -> usize {
        let payload = match &*self.table {
            CursorTable::Dense { values, .. } => values.capacity() * std::mem::size_of::<usize>(),
            CursorTable::Columns(columns) => {
                columns.capacity() * std::mem::size_of::<CursorColumn>()
                    + columns
                        .iter()
                        .map(|column| match column {
                            CursorColumn::Dense(values) => {
                                values.capacity() * std::mem::size_of::<usize>()
                            }
                            CursorColumn::Sparse { changes, .. } => {
                                changes.capacity() * std::mem::size_of::<(usize, usize)>()
                            }
                        })
                        .sum::<usize>()
            }
        };
        std::mem::size_of::<CursorTable>() + payload
    }
    pub(crate) fn dense_columns(&self) -> usize {
        match &*self.table {
            CursorTable::Dense { lanes, .. } => *lanes,
            CursorTable::Columns(columns) => columns
                .iter()
                .filter(|column| matches!(column, CursorColumn::Dense(_)))
                .count(),
        }
    }
}

#[cfg(test)]
impl PartialEq for LaneCursors {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

#[cfg(test)]
impl<const N: usize> PartialEq<[usize; N]> for LaneCursors {
    fn eq(&self, other: &[usize; N]) -> bool {
        self.iter().eq(other.iter())
    }
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

#[cfg(test)]
pub(crate) fn build(base: &[SafeCut], lanes: &[Vec<SafeCut>]) -> Vec<PairedCut> {
    build_counted(base, lanes, base.len())
}

pub(crate) fn build_counted(
    base: &[SafeCut],
    lanes: &[Vec<SafeCut>],
    count: usize,
) -> Vec<PairedCut> {
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
    build_spanned_counted(base, lanes, &spans, count)
}

/// A lane participates only in the interval of bases it actually annotates.
/// Before/after that interval its cursor is fixed and cannot constrain a cut.
#[cfg(test)]
pub(crate) fn build_spanned(
    base: &[SafeCut],
    lanes: &[Vec<SafeCut>],
    spans: &[LaneSpan],
) -> Vec<PairedCut> {
    build_spanned_counted(base, lanes, spans, base.len())
}

pub(crate) fn build_spanned_counted(
    base: &[SafeCut],
    lanes: &[Vec<SafeCut>],
    spans: &[LaneSpan],
    count: usize,
) -> Vec<PairedCut> {
    let mut result = PairedBuilder::new(count, lanes.len());
    walk_spanned(base, lanes, spans, |unit, lanes, class| {
        result.push(unit, lanes, class);
    });
    result.finish()
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
