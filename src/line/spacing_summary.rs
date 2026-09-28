//! Associative visual boundary costs and an online UAX #9 level stack.
//! A candidate folds at most 127 frames (implicit levels 0–126); it never
//! reorders its whole prefix.
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Text,
    Cursive,
    Atomic,
    Barrier,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Edge {
    pub(super) tracking: i32,
    pub(super) kind: Kind,
    pub(super) unit: u32,
    pub(super) box_node: u32,
    pub(super) class: super::autospace::Class,
    pub(super) punctuation: super::punctuation::Punctuation,
}

pub(super) fn allowed(a: Edge, b: Edge) -> bool {
    !(a.kind == Kind::Barrier
        || b.kind == Kind::Barrier
        || a.kind == b.kind && matches!(a.kind, Kind::Cursive | Kind::Atomic))
}

pub(super) fn gap(a: Edge, b: Edge) -> i64 {
    if !allowed(a, b) {
        0
    } else {
        (i64::from(a.tracking) + i64::from(b.tracking)) / 2
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Summary {
    pub(super) first: Option<Edge>,
    pub(super) last: Option<Edge>,
    pub(super) cost: i64,
    pub(super) before: bool,
    pub(super) after: bool,
    pub(super) hang_before: bool,
    pub(super) hang_after: bool,
}

impl Summary {
    pub(super) fn leaf(edge: Edge) -> Self {
        let (left, right) = edge.punctuation.own_blanks();
        Self {
            first: Some(edge),
            last: Some(edge),
            cost: -i64::from(left.raw()) - i64::from(right.raw()),
            before: false,
            after: false,
            hang_before: false,
            hang_after: false,
        }
    }

    pub(super) fn barrier() -> Self {
        Self {
            before: true,
            after: true,
            hang_before: true,
            hang_after: true,
            ..Default::default()
        }
    }

    pub(super) fn join(self, other: Self, data: Option<&ParagraphData>) -> Self {
        Self {
            first: self.first.or(other.first),
            last: other.last.or(self.last),
            cost: self.cost
                + other.cost
                + self.last.zip(other.first).map_or(0, |(a, b)| {
                    gap(a, b)
                        + data.map_or(0, |d| {
                            let blocked = self.after || other.before;
                            let (right, left) = super::punctuation::boundary(d, a, b, blocked);
                            super::autospace::gap(d, a, b, blocked)
                                - i64::from(right.raw())
                                - i64::from(left.raw())
                        })
                }),
            before: if self.first.is_some() {
                self.before
            } else {
                self.before || other.before
            },
            after: if other.last.is_some() {
                other.after
            } else {
                self.after || other.after
            },
            hang_before: if self.first.is_some() {
                self.hang_before
            } else {
                self.hang_before || other.hang_before
            },
            hang_after: if other.last.is_some() {
                other.hang_after
            } else {
                self.hang_after || other.hang_after
            },
        }
    }

    fn reverse(self) -> Self {
        Self {
            first: self.last,
            last: self.first,
            before: self.after,
            after: self.before,
            hang_before: self.hang_after,
            hang_after: self.hang_before,
            ..self
        }
    }

    pub(super) fn width(self, sat: &mut Saturation) -> LayoutUnit {
        raw(self.cost, sat)
    }
}

pub(super) fn raw(value: i64, sat: &mut Saturation) -> LayoutUnit {
    let clamped = value.clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    if clamped != value {
        sat.saturated += 1;
    }
    LayoutUnit::from_raw(clamped as i32)
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    level: u8,
    summary: Summary,
}

#[derive(Clone, Debug)]
pub(super) struct Cursor {
    frames: Vec<Frame>,
    #[cfg(test)]
    visits: std::cell::Cell<usize>,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            frames: vec![Frame {
                level: 0,
                summary: Summary::default(),
            }],
            #[cfg(test)]
            visits: std::cell::Cell::new(0),
        }
    }
}

impl Cursor {
    fn append(frame: &mut Frame, summary: Summary, data: Option<&ParagraphData>) {
        frame.summary = if frame.level.is_multiple_of(2) {
            frame.summary.join(summary, data)
        } else {
            summary.join(frame.summary, data)
        };
    }

    pub(super) fn push(&mut self, level: u8, summary: Summary, data: Option<&ParagraphData>) {
        #[cfg(test)]
        self.visits.set(self.visits.get() + 1);
        let mut carry = None;
        while self.frames.last().unwrap().level > level {
            #[cfg(test)]
            self.visits.set(self.visits.get() + 1);
            let mut popped = self.frames.pop().unwrap();
            if let Some(summary) = carry {
                Self::append(&mut popped, summary, data);
            }
            carry = Some(popped.summary);
        }
        if self.frames.last().unwrap().level < level {
            self.frames.push(Frame {
                level,
                summary: Summary::default(),
            });
        }
        let frame = self.frames.last_mut().unwrap();
        if let Some(summary) = carry {
            Self::append(frame, summary, data);
        }
        Self::append(
            frame,
            if level.is_multiple_of(2) {
                summary
            } else {
                summary.reverse()
            },
            data,
        );
    }

    pub(super) fn summary(&self, data: Option<&ParagraphData>) -> Summary {
        let mut carry = Summary::default();
        for frame in self.frames.iter().rev() {
            #[cfg(test)]
            self.visits.set(self.visits.get() + 1);
            let mut frame = *frame;
            Self::append(&mut frame, carry, data);
            carry = frame.summary;
        }
        carry
    }
}

/// A balanced range index inside each UAX #9 embedding group. Internal nodes
/// retain the exact same associative visual boundary costs as Cursor. Clipping
/// a range preserves embedding groups and never scans its complete prefix.
#[derive(Debug, Default)]
pub(super) struct RangeIndex {
    nodes: Vec<RangeNode>,
    root: Option<usize>,
    #[cfg(test)]
    visits: std::cell::Cell<usize>,
}

#[derive(Debug)]
struct RangeNode {
    range: std::ops::Range<usize>,
    summary: Summary,
    children: Option<(usize, usize)>,
    reversed: bool,
}

impl RangeIndex {
    pub(super) fn new(
        values: impl Iterator<Item = (u8, Summary)>,
        data: Option<&ParagraphData>,
    ) -> Self {
        struct Group {
            level: u8,
            children: Vec<usize>,
        }
        let mut result = Self::default();
        let mut groups = vec![Group {
            level: 0,
            children: Vec::new(),
        }];
        for (i, (level, summary)) in values.enumerate() {
            let mut carry = None;
            while groups.last().unwrap().level > level {
                let mut group = groups.pop().unwrap();
                if let Some(node) = carry {
                    group.children.push(node);
                }
                carry = result.balance(&group.children, group.level % 2 == 1, data);
            }
            if groups.last().unwrap().level < level {
                groups.push(Group {
                    level,
                    children: Vec::new(),
                });
            }
            let group = groups.last_mut().unwrap();
            if let Some(node) = carry {
                group.children.push(node);
            }
            let node = result.nodes.len();
            result.nodes.push(RangeNode {
                range: i..i + 1,
                summary: if level % 2 == 1 {
                    summary.reverse()
                } else {
                    summary
                },
                children: None,
                reversed: level % 2 == 1,
            });
            group.children.push(node);
        }
        let mut carry = None;
        while let Some(mut group) = groups.pop() {
            if let Some(node) = carry {
                group.children.push(node);
            }
            carry = result.balance(&group.children, group.level % 2 == 1, data);
        }
        result.root = carry;
        result
    }

    fn balance(
        &mut self,
        children: &[usize],
        reversed: bool,
        data: Option<&ParagraphData>,
    ) -> Option<usize> {
        if children.is_empty() {
            return None;
        }
        if children.len() == 1 {
            return Some(children[0]);
        }
        let half = children.len() / 2;
        let left = self.balance(&children[..half], reversed, data).unwrap();
        let right = self.balance(&children[half..], reversed, data).unwrap();
        let summary = if reversed {
            self.nodes[right]
                .summary
                .join(self.nodes[left].summary, data)
        } else {
            self.nodes[left]
                .summary
                .join(self.nodes[right].summary, data)
        };
        let range = self.nodes[left].range.start..self.nodes[right].range.end;
        let node = self.nodes.len();
        self.nodes.push(RangeNode {
            range,
            summary,
            children: Some((left, right)),
            reversed,
        });
        Some(node)
    }

    pub(super) fn query(
        &self,
        range: std::ops::Range<usize>,
        data: Option<&ParagraphData>,
    ) -> Summary {
        self.root.map_or(Summary::default(), |root| {
            self.query_node(root, &range, None, data)
        })
    }

    /// Replace one source leaf for an accepted discretionary glyph. Only the
    /// leaf's path and the selected range edges are visited; other nodes reuse
    /// their exact visual summaries.
    pub(super) fn query_replace(
        &self,
        range: std::ops::Range<usize>,
        replacement: Option<(usize, Summary)>,
        data: Option<&ParagraphData>,
    ) -> Summary {
        self.root.map_or(Summary::default(), |root| {
            self.query_node(root, &range, replacement, data)
        })
    }

    #[cfg(test)]
    pub(super) fn take_visits(&self) -> usize {
        self.visits.replace(0)
    }

    fn query_node(
        &self,
        i: usize,
        range: &std::ops::Range<usize>,
        replacement: Option<(usize, Summary)>,
        data: Option<&ParagraphData>,
    ) -> Summary {
        #[cfg(test)]
        {
            self.visits.set(self.visits.get() + 1);
        }
        let node = &self.nodes[i];
        if range.start >= node.range.end || range.end <= node.range.start {
            return Summary::default();
        }
        if range.start <= node.range.start
            && node.range.end <= range.end
            && replacement.is_none_or(|(at, _)| !node.range.contains(&at))
        {
            return node.summary;
        }
        let Some((a, b)) = node.children else {
            return replacement
                .filter(|(at, _)| node.range.contains(at))
                .map_or(node.summary, |(_, summary)| {
                    if node.reversed {
                        summary.reverse()
                    } else {
                        summary
                    }
                });
        };
        let left = self.query_node(a, range, replacement, data);
        let right = self.query_node(b, range, replacement, data);
        if node.reversed {
            right.join(left, data)
        } else {
            left.join(right, data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_bidi::{BidiInfo, Level};

    #[test]
    fn indexed_ranges_match_online_bidi_spacing_for_every_small_interval() {
        for profile in 0..4096usize {
            let values: Vec<_> = (0..6)
                .map(|i| {
                    (
                        ((profile >> (2 * i)) & 3) as u8,
                        Summary::leaf(Edge {
                            tracking: (i as i32 - 2) * 64,
                            kind: [Kind::Text, Kind::Cursive, Kind::Atomic, Kind::Barrier]
                                [(i + profile) % 4],
                            ..Default::default()
                        }),
                    )
                })
                .collect();
            let index = RangeIndex::new(values.iter().copied(), None);
            for start in 0..6 {
                for end in start + 1..=6 {
                    let mut cursor = Cursor::default();
                    for (level, value) in &values[start..end] {
                        cursor.push(*level, *value, None);
                    }
                    let actual = index.query(start..end, None);
                    let expected = cursor.summary(None);
                    assert_eq!(actual.cost, expected.cost, "{profile}: {start}..{end}");
                    assert_eq!(
                        actual.first.map(|e| e.tracking),
                        expected.first.map(|e| e.tracking)
                    );
                    assert_eq!(
                        actual.last.map(|e| e.tracking),
                        expected.last.map(|e| e.tracking)
                    );
                }
            }
        }
    }

    #[test]
    fn indexed_prefix_queries_do_not_rescan_all_preceding_characters() {
        for count in [512, 1024, 4096] {
            let index = RangeIndex::new(
                (0..count).map(|i| {
                    (
                        ((i * 37) % 127) as u8,
                        Summary::leaf(Edge {
                            tracking: 64,
                            kind: Kind::Text,
                            ..Default::default()
                        }),
                    )
                }),
                None,
            );
            let mut cursor = Cursor::default();
            for end in 1..=count {
                cursor.push(
                    (((end - 1) * 37) % 127) as u8,
                    Summary::leaf(Edge {
                        tracking: 64,
                        kind: Kind::Text,
                        ..Default::default()
                    }),
                    None,
                );
                assert_eq!(index.query(0..end, None).cost, cursor.summary(None).cost);
            }
            assert!(
                index.visits.get() < count * 127,
                "{} visits for{count}",
                index.visits.get()
            );
            assert!(index.nodes.len() < count * 2);
        }
    }

    #[test]
    fn bidi_spacing_candidates_match_independent_reorder() {
        // Exhaustive small prefixes compare against full UAX #9 reordering,
        // with unequal styles and atomic/cursive/transparent boundaries.
        for profile in 0..4096usize {
            let levels: Vec<_> = (0..6)
                .map(|i| Level::new(((profile >> (2 * i)) & 3) as u8).unwrap())
                .collect();
            let kinds = [
                Kind::Text,
                Kind::Cursive,
                Kind::Cursive,
                Kind::Atomic,
                Kind::Atomic,
                Kind::Barrier,
            ];
            let edges: Vec<_> = (0..6)
                .map(|i| Edge {
                    tracking: ((i as i32) - 2) * 64,
                    kind: kinds[(i + profile) % 6],
                    ..Default::default()
                })
                .collect();
            let mut cursor = Cursor::default();
            for end in 1..=6 {
                cursor.push(
                    levels[end - 1].number(),
                    Summary::leaf(edges[end - 1]),
                    None,
                );
                let order = BidiInfo::reorder_visual(&levels[..end]);
                let expected: i64 = order
                    .windows(2)
                    .map(|p| {
                        let a = edges[p[0]];
                        let b = edges[p[1]];
                        if a.kind == Kind::Barrier
                            || b.kind == Kind::Barrier
                            || a.kind == b.kind && matches!(a.kind, Kind::Atomic | Kind::Cursive)
                        {
                            0
                        } else {
                            (i64::from(a.tracking) + i64::from(b.tracking)) / 2
                        }
                    })
                    .sum();
                assert_eq!(
                    cursor.summary(None).cost,
                    expected,
                    "profile {profile}, prefix {end}"
                );
            }
        }
    }

    #[test]
    fn long_alternating_prefixes_keep_bounded_work_and_memory() {
        let mut cursor = Cursor::default();
        for level in 0..=126 {
            cursor.push(
                level,
                Summary::leaf(Edge {
                    tracking: 64,
                    kind: Kind::Text,
                    ..Default::default()
                }),
                None,
            );
        }
        assert_eq!(cursor.frames.len(), 127);
        assert_eq!(cursor.summary(None).cost, 126 * 64);
        let mut cursor = Cursor::default();
        for i in 0..100_000 {
            let level = ((i * 37) % 127) as u8;
            cursor.push(
                level,
                Summary::leaf(Edge {
                    tracking: 64,
                    kind: Kind::Text,
                    ..Default::default()
                }),
                None,
            );
            assert_eq!(cursor.summary(None).cost, i * 64);
            assert!(cursor.frames.len() <= 127);
        }
        assert!(cursor.visits.get() < 100_000 * 130);
    }
}
