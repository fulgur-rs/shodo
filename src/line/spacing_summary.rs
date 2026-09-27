//! Associative visual boundary costs and an online UAX #9 level stack.
//! A candidate folds at most 127 frames (implicit levels 0–126); it never
//! reorders its whole prefix.
use crate::geometry::{LayoutUnit, Saturation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text,
    Cursive,
    Atomic,
    Barrier,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Edge {
    pub(super) tracking: i32,
    pub(super) kind: Kind,
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
}

impl Summary {
    pub(super) fn leaf(edge: Edge) -> Self {
        Self {
            first: Some(edge),
            last: Some(edge),
            cost: 0,
        }
    }

    pub(super) fn join(self, other: Self) -> Self {
        Self {
            first: self.first.or(other.first),
            last: other.last.or(self.last),
            cost: self.cost + other.cost + self.last.zip(other.first).map_or(0, |(a, b)| gap(a, b)),
        }
    }

    fn reverse(self) -> Self {
        Self {
            first: self.last,
            last: self.first,
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
    fn append(frame: &mut Frame, summary: Summary) {
        frame.summary = if frame.level.is_multiple_of(2) {
            frame.summary.join(summary)
        } else {
            summary.join(frame.summary)
        };
    }

    pub(super) fn push(&mut self, level: u8, summary: Summary) {
        #[cfg(test)]
        self.visits.set(self.visits.get() + 1);
        let mut carry = None;
        while self.frames.last().unwrap().level > level {
            #[cfg(test)]
            self.visits.set(self.visits.get() + 1);
            let mut popped = self.frames.pop().unwrap();
            if let Some(summary) = carry {
                Self::append(&mut popped, summary);
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
            Self::append(frame, summary);
        }
        Self::append(
            frame,
            if level.is_multiple_of(2) {
                summary
            } else {
                summary.reverse()
            },
        );
    }

    pub(super) fn summary(&self) -> Summary {
        let mut carry = Summary::default();
        for frame in self.frames.iter().rev() {
            #[cfg(test)]
            self.visits.set(self.visits.get() + 1);
            let mut frame = *frame;
            Self::append(&mut frame, carry);
            carry = frame.summary;
        }
        carry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_bidi::{BidiInfo, Level};

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
                })
                .collect();
            let mut cursor = Cursor::default();
            for end in 1..=6 {
                cursor.push(levels[end - 1].number(), Summary::leaf(edges[end - 1]));
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
                    cursor.summary().cost,
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
                }),
            );
        }
        assert_eq!(cursor.frames.len(), 127);
        assert_eq!(cursor.summary().cost, 126 * 64);
        let mut cursor = Cursor::default();
        for i in 0..100_000 {
            let level = ((i * 37) % 127) as u8;
            cursor.push(
                level,
                Summary::leaf(Edge {
                    tracking: 64,
                    kind: Kind::Text,
                }),
            );
            assert_eq!(cursor.summary().cost, i * 64);
            assert!(cursor.frames.len() <= 127);
        }
        assert!(cursor.visits.get() < 100_000 * 130);
    }
}
