//! Mapping between caller text offsets (UTF-8 bytes within a node's text)
//! and offsets in the processed text shodo lays out. White-space collapsing
//! removes characters and text-transform can change lengths, so the mapping
//! is a list of runs.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::OnceLock;

use crate::node::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingKind {
    /// Same length on both sides.
    Identity,
    /// Removed by white-space collapsing; `text` is empty.
    Collapsed,
    /// Length changed (for example `ß` to `SS`); indivisible.
    Expanded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingUnit {
    pub kind: MappingKind,
    pub node: NodeId,
    pub dom: Range<u32>,
    pub text: Range<u32>,
}

/// Which side of a boundary a position belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOrigin {
    Dom {
        node: NodeId,
        offset: u32,
    },
    /// Generated content, atomics and control characters have no DOM offset.
    Generated {
        node: NodeId,
    },
}

#[derive(Debug, Default)]
/// Offset queries lazily retain private interval indexes. Original record order
/// decides ambiguous source overlaps; clones start without retained indexes.
pub struct OffsetMapping {
    units: Vec<MappingUnit>,
    generated: Vec<(Range<u32>, NodeId)>,
    index: OnceLock<MappingIndex>,
}

impl Clone for OffsetMapping {
    fn clone(&self) -> Self {
        Self {
            units: self.units.clone(),
            generated: self.generated.clone(),
            index: OnceLock::new(),
        }
    }
}

impl PartialEq for OffsetMapping {
    fn eq(&self, other: &Self) -> bool {
        self.units == other.units && self.generated == other.generated
    }
}
impl Eq for OffsetMapping {}

#[derive(Debug, Default)]
struct MappingIndex {
    dom: BTreeMap<NodeId, RangeIndex>,
    text: RangeIndex,
    generated: RangeIndex,
}

#[derive(Debug)]
struct IndexedRange {
    start: u32,
    end: u32,
    ordinal: usize,
    subtree_end: u32,
    subtree_first: usize,
}

/// Balanced interval tree in start order. Source offsets can be reused or
/// supplied out of order, and transform spans can overlap. Subtree bounds
/// prune unrelated ranges while original ordinals preserve tie-breaking.
#[derive(Debug, Default)]
struct RangeIndex {
    ranges: Vec<IndexedRange>,
    ends: Vec<(u32, usize)>,
}

impl RangeIndex {
    fn new(records: impl IntoIterator<Item = (Range<u32>, usize)>) -> Self {
        let mut ranges: Vec<_> = records
            .into_iter()
            .map(|(range, ordinal)| IndexedRange {
                start: range.start,
                end: range.end,
                ordinal,
                subtree_end: range.end,
                subtree_first: ordinal,
            })
            .collect();
        let mut ends: Vec<_> = ranges.iter().map(|r| (r.end, r.ordinal)).collect();
        ends.sort_unstable();
        ranges.sort_unstable_by_key(|r| (r.start, r.ordinal));
        Self::annotate(&mut ranges);
        Self { ranges, ends }
    }

    fn annotate(ranges: &mut [IndexedRange]) -> (u32, usize) {
        if ranges.is_empty() {
            return (0, usize::MAX);
        }
        let middle = ranges.len() / 2;
        let (left, rest) = ranges.split_at_mut(middle);
        let (root, right) = rest.split_first_mut().unwrap();
        let (left_end, left_first) = Self::annotate(left);
        let (right_end, right_first) = Self::annotate(right);
        root.subtree_end = root.end.max(left_end).max(right_end);
        root.subtree_first = root.ordinal.min(left_first).min(right_first);
        (root.subtree_end, root.subtree_first)
    }

    fn first_containing(&self, offset: u32, strict_start: bool) -> Option<usize> {
        let through = self.ranges.partition_point(|r| {
            #[cfg(test)]
            tests::visit();
            r.start < offset || !strict_start && r.start == offset
        });
        let mut found = usize::MAX;
        self.find(0, self.ranges.len(), through, offset, &mut found);
        (found != usize::MAX).then_some(found)
    }

    fn find(&self, begin: usize, end: usize, through: usize, offset: u32, found: &mut usize) {
        if begin >= end || begin >= through {
            return;
        }
        #[cfg(test)]
        tests::visit();
        let middle = begin + (end - begin) / 2;
        let root = &self.ranges[middle];
        if root.subtree_end <= offset || root.subtree_first >= *found {
            return;
        }
        if middle < through && offset < root.end {
            *found = (*found).min(root.ordinal);
        }
        self.find(begin, middle, through, offset, found);
        self.find(middle + 1, end, through, offset, found);
    }

    /// All closed intervals, including source endpoints and empty ranges.
    fn for_each_closed(&self, offset: u32, mut visit: impl FnMut(usize)) {
        let through = self.ranges.partition_point(|r| {
            #[cfg(test)]
            tests::visit();
            r.start <= offset
        });
        self.visit_closed(0, self.ranges.len(), through, offset, &mut visit);
    }

    fn visit_closed(
        &self,
        begin: usize,
        end: usize,
        through: usize,
        offset: u32,
        visit: &mut impl FnMut(usize),
    ) {
        if begin >= end || begin >= through {
            return;
        }
        #[cfg(test)]
        tests::visit();
        let middle = begin + (end - begin) / 2;
        let root = &self.ranges[middle];
        if root.subtree_end < offset {
            return;
        }
        if middle < through && offset <= root.end {
            visit(root.ordinal);
        }
        self.visit_closed(begin, middle, through, offset, visit);
        self.visit_closed(middle + 1, end, through, offset, visit);
    }

    fn last_end(&self, offset: u32) -> Option<usize> {
        let through = self.ends.partition_point(|(end, _)| {
            #[cfg(test)]
            tests::visit();
            *end <= offset
        });
        self.ends
            .get(through.checked_sub(1)?)
            .filter(|(end, _)| *end == offset)
            .map(|(_, ordinal)| *ordinal)
    }

    fn last_start(&self, offset: u32) -> Option<usize> {
        let through = self.ranges.partition_point(|r| {
            #[cfg(test)]
            tests::visit();
            r.start <= offset
        });
        self.ranges
            .get(through.checked_sub(1)?)
            .filter(|r| r.start == offset)
            .map(|r| r.ordinal)
    }
}

/// A scalar transform, or a coalesced byte-preserving sequence. Expanded
/// spans also cover one-to-many scalar changes with equal UTF-8 byte lengths.
pub(crate) struct TransformSpan {
    pub(crate) old: Range<u32>,
    pub(crate) new: Range<u32>,
    pub(crate) kind: MappingKind,
}

impl TransformSpan {
    pub(crate) fn source_position(spans: &[Self], pos: u32) -> u32 {
        let index = spans.partition_point(|s| s.new.end <= pos);
        let Some(span) = spans.get(index) else {
            return spans.last().map_or(0, |s| s.old.end);
        };
        match span.kind {
            MappingKind::Identity => span.old.start + pos.saturating_sub(span.new.start),
            _ => span.old.start,
        }
    }

    pub(crate) fn map_position(spans: &[Self], pos: u32) -> u32 {
        let index = spans.partition_point(|s| s.old.end <= pos);
        let Some(span) = spans.get(index) else {
            return spans.last().map_or(0, |s| s.new.end);
        };
        match span.kind {
            MappingKind::Identity => span.new.start + pos.saturating_sub(span.old.start),
            _ => span.new.start,
        }
    }
}

impl OffsetMapping {
    fn index(&self) -> &MappingIndex {
        self.index.get_or_init(|| {
            let mut sources: BTreeMap<NodeId, Vec<(Range<u32>, usize)>> = BTreeMap::new();
            for (i, unit) in self.units.iter().enumerate() {
                sources
                    .entry(unit.node)
                    .or_default()
                    .push((unit.dom.clone(), i));
            }
            MappingIndex {
                dom: sources
                    .into_iter()
                    .map(|(node, ranges)| (node, RangeIndex::new(ranges)))
                    .collect(),
                text: RangeIndex::new(
                    self.units
                        .iter()
                        .enumerate()
                        .filter(|(_, u)| u.kind != MappingKind::Collapsed)
                        .map(|(i, u)| (u.text.clone(), i)),
                ),
                generated: RangeIndex::new(
                    self.generated
                        .iter()
                        .enumerate()
                        .filter(|(_, (range, _))| !range.is_empty())
                        .map(|(i, (range, _))| (range.clone(), i)),
                ),
            }
        })
    }

    /// Every inverse candidate in this dataset, normalized to the requested
    /// source side before a caller restricts candidates to accepted lines.
    pub(crate) fn source_candidates(
        &self,
        node: NodeId,
        offset: u32,
        requested: Affinity,
    ) -> Vec<(u32, Affinity)> {
        let mut candidates = Vec::new();
        let Some(ranges) = self.index().dom.get(&node) else {
            return candidates;
        };
        let candidate = |ordinal: usize| {
            let unit = &self.units[ordinal];
            let interior = unit.dom.start < offset && offset < unit.dom.end;
            let (text, affinity) = if offset == unit.dom.end {
                (unit.text.end, Affinity::Upstream)
            } else {
                match unit.kind {
                    MappingKind::Identity => (
                        unit.text.start.saturating_add(offset - unit.dom.start),
                        if interior {
                            requested
                        } else {
                            Affinity::Downstream
                        },
                    ),
                    MappingKind::Collapsed => (unit.text.end, Affinity::Downstream),
                    MappingKind::Expanded => (unit.text.start, Affinity::Downstream),
                }
            };
            (text, affinity, interior)
        };
        let mut preferred = false;
        ranges.for_each_closed(offset, |ordinal| {
            let (_, affinity, interior) = candidate(ordinal);
            preferred |= interior || affinity == requested;
        });
        ranges.for_each_closed(offset, |ordinal| {
            let (text, affinity, interior) = candidate(ordinal);
            if interior || affinity == requested || !preferred {
                candidates.push((text, affinity));
            }
        });
        candidates.sort_unstable_by_key(|&(text, _)| text);
        candidates
    }

    pub(crate) fn remap_text(&mut self, spans: &[TransformSpan]) {
        self.index.take();
        let units = std::mem::take(&mut self.units);
        let generated = std::mem::take(&mut self.generated);
        for unit in units {
            if unit.kind != MappingKind::Identity {
                self.push_unit(MappingUnit {
                    text: TransformSpan::map_position(spans, unit.text.start)
                        ..TransformSpan::map_position(spans, unit.text.end),
                    ..unit
                });
                continue;
            }
            let first = spans.partition_point(|s| s.old.end <= unit.text.start);
            for span in &spans[first..] {
                if span.old.start >= unit.text.end {
                    break;
                }
                let begin = span.old.start.max(unit.text.start);
                let end = span.old.end.min(unit.text.end);
                let new_text = if span.kind == MappingKind::Identity {
                    span.new.start + begin - span.old.start..span.new.start + end - span.old.start
                } else {
                    span.new.clone()
                };
                self.push_unit(MappingUnit {
                    kind: span.kind,
                    node: unit.node,
                    dom: unit.dom.start.saturating_add(begin - unit.text.start)
                        ..unit.dom.start.saturating_add(end - unit.text.start),
                    text: new_text,
                });
            }
        }
        for (range, node) in generated {
            self.push_generated(
                TransformSpan::map_position(spans, range.start)
                    ..TransformSpan::map_position(spans, range.end),
                node,
            );
        }
    }

    pub(crate) fn push_unit(&mut self, unit: MappingUnit) {
        self.index.take();
        if let Some(last) = self.units.last_mut()
            && last.kind == unit.kind
            && unit.kind != MappingKind::Expanded
            && last.node == unit.node
            && last.dom.end == unit.dom.start
            && last.text.end == unit.text.start
        {
            last.dom.end = unit.dom.end;
            last.text.end = unit.text.end;
            return;
        }
        self.units.push(unit);
    }

    pub(crate) fn push_generated(&mut self, text: Range<u32>, node: NodeId) {
        self.index.take();
        if let Some((last, last_node)) = self.generated.last_mut()
            && *last_node == node
            && last.end == text.start
        {
            last.end = text.end;
            return;
        }
        self.generated.push((text, node));
    }

    pub fn units(&self) -> &[MappingUnit] {
        &self.units
    }

    /// DOM ownership in processed source order. Producer records and scalar
    /// remapping keep both text endpoints nondecreasing, including collapsed
    /// gaps. Binary searches restrict each line query to its local records.
    pub(crate) fn dom_owners(
        &self,
        text: Range<u32>,
    ) -> impl Iterator<Item = (NodeId, Range<u32>)> + '_ {
        let begin = self
            .units
            .partition_point(|unit| unit.text.end < text.start);
        let end = self
            .units
            .partition_point(|unit| unit.text.start <= text.end);
        let mut sources = self.units[begin..end]
            .iter()
            .filter_map(move |unit| {
                let dom = match unit.kind {
                    MappingKind::Collapsed => {
                        let at = unit.text.start;
                        // Gaps belong upstream, except at the paragraph start.
                        if !(text.start < at && at <= text.end || text.start == 0 && at == 0) {
                            return None;
                        }
                        unit.dom.clone()
                    }
                    MappingKind::Identity | MappingKind::Expanded => {
                        let start = unit.text.start.max(text.start);
                        let end = unit.text.end.min(text.end);
                        if start >= end {
                            return None;
                        }
                        if unit.kind == MappingKind::Identity {
                            unit.dom
                                .start
                                .saturating_add(start - unit.text.start)
                                .min(unit.dom.end)
                                ..unit
                                    .dom
                                    .start
                                    .saturating_add(end - unit.text.start)
                                    .min(unit.dom.end)
                        } else {
                            unit.dom.clone()
                        }
                    }
                };
                (!dom.is_empty()).then_some((unit.node, dom))
            })
            .peekable();
        std::iter::from_fn(move || {
            let (node, mut dom) = sources.next()?;
            while let Some((next_node, next_dom)) = sources.peek()
                && *next_node == node
                && next_dom.start <= dom.end
                && dom.start <= next_dom.end
            {
                let (_, next) = sources.next().unwrap();
                dom.start = dom.start.min(next.start);
                dom.end = dom.end.max(next.end);
            }
            Some((node, dom))
        })
    }

    /// Processed-text offset of a caller offset. Offsets inside a collapsed
    /// run map to the end of the gap; offsets inside an expanded run round to
    /// its start.
    pub fn dom_to_text(&self, node: NodeId, offset: u32) -> Option<(u32, Affinity)> {
        let index = self.index().dom.get(&node)?;
        if let Some(i) = index.first_containing(offset, false) {
            let u = &self.units[i];
            // Caller offsets can already be clamped near u32::MAX.
            let text = match u.kind {
                MappingKind::Identity => u.text.start.saturating_add(offset - u.dom.start),
                MappingKind::Collapsed => u.text.end,
                MappingKind::Expanded => u.text.start,
            };
            return Some((text, Affinity::Downstream));
        }
        index
            .last_end(offset)
            .map(|i| (self.units[i].text.end, Affinity::Upstream))
    }

    /// Caller offset of a processed-text offset.
    pub fn text_to_dom(&self, offset: u32, affinity: Affinity) -> Option<TextOrigin> {
        let index = self.index();
        let generated = |i: usize| TextOrigin::Generated {
            node: self.generated[i].1,
        };
        if let Some(i) = index.generated.first_containing(offset, true) {
            return Some(generated(i));
        }
        let downstream = index
            .generated
            .last_start(offset)
            .map(generated)
            .or_else(|| {
                let u = &self.units[index.text.first_containing(offset, false)?];
                let dom = match u.kind {
                    MappingKind::Identity => u.dom.start.saturating_add(offset - u.text.start),
                    _ => u.dom.start,
                };
                Some(TextOrigin::Dom {
                    node: u.node,
                    offset: dom,
                })
            });
        let upstream = index
            .text
            .last_end(offset)
            .map(|i| {
                let u = &self.units[i];
                TextOrigin::Dom {
                    node: u.node,
                    offset: u.dom.end,
                }
            })
            .or_else(|| index.generated.last_end(offset).map(generated));
        match affinity {
            Affinity::Upstream => upstream.or(downstream),
            Affinity::Downstream => downstream.or(upstream),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn unit(kind: MappingKind, dom: Range<u32>, text: Range<u32>) -> MappingUnit {
        MappingUnit {
            kind,
            node: NodeId(1),
            dom,
            text,
        }
    }

    #[test]
    fn affinity_selects_dom_or_generated_on_both_boundaries() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..1, 0..1));
        m.push_generated(1..3, NodeId(9));
        m.push_unit(unit(MappingKind::Identity, 1..2, 3..4));
        assert_eq!(
            m.text_to_dom(1, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(
            m.text_to_dom(3, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(2, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
    }

    #[test]
    fn identity_units_merge_and_map_both_ways() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..4, 2..4));
        assert_eq!(m.units().len(), 1);
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 3
            })
        );
    }

    #[test]
    fn collapsed_offsets_map_to_the_end_of_the_gap() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2)); // "a "
        m.push_unit(unit(MappingKind::Collapsed, 2..4, 2..2)); // "  " removed
        m.push_unit(unit(MappingKind::Identity, 4..5, 2..3)); // "b"
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((2, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 4
            })
        );
        assert_eq!(
            m.text_to_dom(2, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 2
            })
        );
    }

    #[test]
    fn expanded_interiors_round_to_the_start() {
        let mut m = OffsetMapping::default();
        // "ß" (2 bytes) became "SS" (2 bytes) in one unit, then "x".
        m.push_unit(unit(MappingKind::Expanded, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..3, 2..3));
        assert_eq!(m.dom_to_text(NodeId(1), 1), Some((0, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 0
            })
        );
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Upstream)));
    }

    #[test]
    fn generated_text_has_no_dom_offset() {
        let mut m = OffsetMapping::default();
        m.push_generated(0..3, NodeId(9));
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(m.dom_to_text(NodeId(9), 0), None);
    }
    std::thread_local! {
        static QUERY_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(super) fn visit() {
        QUERY_VISITS.with(|n| n.set(n.get() + 1));
    }
    pub(crate) fn reset_visits() {
        QUERY_VISITS.with(|n| n.set(0));
    }
    pub(crate) fn visits() -> usize {
        QUERY_VISITS.with(std::cell::Cell::get)
    }

    #[test]
    fn position_queries_do_not_scan_unrelated_mapping_records() {
        let mut m = OffsetMapping::default();
        for i in 0..4096 {
            m.push_unit(MappingUnit {
                kind: MappingKind::Identity,
                node: NodeId(u64::from(i)),
                dom: 0..2,
                text: 4 * i..4 * i + 2,
            });
            m.push_generated(4 * i + 2..4 * i + 4, NodeId(10000 + u64::from(i)));
        }
        // Build any lazy index once; measure work in repeated public queries.
        m.dom_to_text(NodeId(4095), 1);
        m.text_to_dom(16381, Affinity::Downstream);
        reset_visits();
        assert_eq!(
            m.dom_to_text(NodeId(4095), 1),
            Some((16381, Affinity::Downstream))
        );
        assert!(visits() < 128, "DOM query examined {} records", visits());
        reset_visits();
        assert_eq!(
            m.text_to_dom(16381, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(4095),
                offset: 1
            })
        );
        assert!(visits() < 128, "text query examined {} records", visits());
        reset_visits();
        assert_eq!(
            m.text_to_dom(16383, Affinity::Upstream),
            Some(TextOrigin::Generated {
                node: NodeId(14095)
            })
        );
        assert!(
            visits() < 128,
            "generated query examined {} records",
            visits()
        );
    }

    #[test]
    fn same_node_dom_queries_prune_many_nonmerged_ranges() {
        let mut m = OffsetMapping::default();
        for i in 0..4096 {
            m.push_unit(MappingUnit {
                kind: MappingKind::Expanded,
                node: NodeId(7),
                dom: 4 * i..4 * i + 2,
                text: 3 * i..3 * i + 1,
            });
        }
        assert_eq!(m.units().len(), 4096);
        // Exclude lazy index construction from the repeated-query work count.
        m.dom_to_text(NodeId(7), 0);
        for (offset, expected) in [
            (16381, Some((12285, Affinity::Downstream))),
            (16382, Some((12286, Affinity::Upstream))),
            (16383, None),
        ] {
            reset_visits();
            assert_eq!(m.dom_to_text(NodeId(7), offset), expected);
            let work = visits();
            assert!(
                work < 128,
                "same-node offset {offset} examined {work} records"
            );
        }
    }

    #[test]
    fn same_node_dom_queries_prune_short_ranges_under_an_early_long_overlap() {
        let mut m = OffsetMapping::default();
        m.push_unit(MappingUnit {
            kind: MappingKind::Expanded,
            node: NodeId(7),
            dom: 0..16384,
            text: 0..1,
        });
        for i in 1..4096 {
            m.push_unit(MappingUnit {
                kind: MappingKind::Expanded,
                node: NodeId(7),
                dom: 4 * i..4 * i + 2,
                text: 3 * i..3 * i + 1,
            });
        }
        assert_eq!(m.units().len(), 4096);
        m.dom_to_text(NodeId(7), 0);
        // A late short range's start, end and gap are all inside the first
        // record. Its original ordinal wins even over a later end boundary.
        // The gap also catches backwards prefix-max scans poisoned by the
        // long interval: no short range contains it, but the first one does.
        for (offset, expected) in [
            (16380, Some((0, Affinity::Downstream))),
            (16382, Some((0, Affinity::Downstream))),
            (16383, Some((0, Affinity::Downstream))),
            (16384, Some((1, Affinity::Upstream))),
            (16385, None),
        ] {
            reset_visits();
            assert_eq!(m.dom_to_text(NodeId(7), offset), expected);
            let work = visits();
            assert!(
                work < 128,
                "long-overlap offset {offset} examined {work} records"
            );
        }
        // With no containing interior, the last original end wins, including
        // an empty source range whose processed-text end is different.
        m.push_unit(MappingUnit {
            kind: MappingKind::Expanded,
            node: NodeId(7),
            dom: 16384..16384,
            text: 20000..20001,
        });
        assert_eq!(m.units().len(), 4097);
        m.dom_to_text(NodeId(7), 0);
        reset_visits();
        assert_eq!(
            m.dom_to_text(NodeId(7), 16384),
            Some((20001, Affinity::Upstream))
        );
        let work = visits();
        assert!(work < 128, "last-end tie examined {work} records");
    }

    #[test]
    fn repeated_nonmonotonic_sources_keep_first_interior_and_last_end() {
        let mut m = OffsetMapping::default();
        for (node, dom, text) in [
            (1, 10..12, 0..2),
            (2, 0..2, 2..4),
            (1, 0..2, 4..6),
            (1, 10..12, 6..8),
        ] {
            m.push_unit(MappingUnit {
                kind: MappingKind::Identity,
                node: NodeId(node),
                dom,
                text,
            });
        }
        assert_eq!(
            m.dom_to_text(NodeId(1), 11),
            Some((1, Affinity::Downstream))
        );
        assert_eq!(m.dom_to_text(NodeId(1), 12), Some((8, Affinity::Upstream)));
        assert_eq!(m.dom_to_text(NodeId(1), 1), Some((5, Affinity::Downstream)));
        assert_eq!(m.dom_to_text(NodeId(2), 1), Some((3, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(6, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 2
            })
        );
        assert_eq!(
            m.text_to_dom(6, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 10
            })
        );
    }

    #[test]
    fn initialized_queries_follow_merges_and_overlapping_transform_spans() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..1, 0..1));
        assert_eq!(m.dom_to_text(NodeId(1), 1), Some((1, Affinity::Upstream)));
        m.push_unit(unit(MappingKind::Identity, 1..2, 1..2));
        assert_eq!(m.dom_to_text(NodeId(1), 2), Some((2, Affinity::Upstream)));
        m.push_unit(MappingUnit {
            kind: MappingKind::Identity,
            node: NodeId(2),
            dom: 0..1,
            text: 2..3,
        });
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(2),
                offset: 0
            })
        );
        let before = m.clone();
        m.remap_text(&[TransformSpan {
            old: 0..3,
            new: 0..6,
            kind: MappingKind::Expanded,
        }]);
        assert_eq!(m.dom_to_text(NodeId(2), 0), Some((0, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 0
            })
        );
        assert_eq!(
            m.text_to_dom(6, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(2),
                offset: 1
            })
        );
        let unqueried = m.clone();
        m.text_to_dom(5, Affinity::Upstream);
        assert_eq!(m, unqueried, "index state is not mapping identity");
        assert_eq!(
            before.dom_to_text(NodeId(2), 0),
            Some((2, Affinity::Downstream))
        );
    }

    #[test]
    fn saturated_dom_and_empty_generated_boundaries_keep_affinity() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, u32::MAX - 2..u32::MAX, 0..5));
        m.push_unit(MappingUnit {
            kind: MappingKind::Identity,
            node: NodeId(2),
            dom: u32::MAX..u32::MAX,
            text: 5..9,
        });
        m.push_generated(5..5, NodeId(9));
        assert_eq!(
            m.dom_to_text(NodeId(1), u32::MAX - 1),
            Some((1, Affinity::Downstream))
        );
        assert_eq!(
            m.dom_to_text(NodeId(1), u32::MAX),
            Some((5, Affinity::Upstream))
        );
        assert_eq!(
            m.dom_to_text(NodeId(2), u32::MAX),
            Some((9, Affinity::Upstream))
        );
        assert_eq!(
            m.text_to_dom(4, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: u32::MAX
            })
        );
        assert_eq!(
            m.text_to_dom(5, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: u32::MAX
            })
        );
        assert_eq!(
            m.text_to_dom(5, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(2),
                offset: u32::MAX
            })
        );
    }
    // Independent linear oracle from the pre-change commit 4cae3a3.
    fn linear_dom(m: &OffsetMapping, node: NodeId, offset: u32) -> Option<(u32, Affinity)> {
        let mut at_end = None;
        for u in m.units.iter().filter(|u| u.node == node) {
            if u.dom.start <= offset && offset < u.dom.end {
                // Saturate: `dom` came from a caller-supplied offset and may
                // already be clamped near `u32::MAX` (see `push_unit`).
                let text = match u.kind {
                    MappingKind::Identity => u.text.start.saturating_add(offset - u.dom.start),
                    MappingKind::Collapsed => u.text.end,
                    MappingKind::Expanded => u.text.start,
                };
                return Some((text, Affinity::Downstream));
            }
            if offset == u.dom.end {
                at_end = Some((u.text.end, Affinity::Upstream));
            }
        }
        at_end
    }

    /// Caller offset of a processed-text offset.
    fn linear_text(m: &OffsetMapping, offset: u32, affinity: Affinity) -> Option<TextOrigin> {
        let mut downstream = None;
        let mut upstream = None;
        for (r, node) in &m.generated {
            if r.start < offset && offset < r.end {
                return Some(TextOrigin::Generated { node: *node });
            }
            if r.start == offset && !r.is_empty() {
                downstream = Some(TextOrigin::Generated { node: *node });
            }
            if r.end == offset && !r.is_empty() {
                upstream = Some(TextOrigin::Generated { node: *node });
            }
        }
        for u in m.units.iter().filter(|u| u.kind != MappingKind::Collapsed) {
            if downstream.is_none() && u.text.start <= offset && offset < u.text.end {
                // Saturate for the same reason as in `dom_to_text`: `u.dom.start`
                // may already be clamped near `u32::MAX`.
                let dom = match u.kind {
                    MappingKind::Identity => u.dom.start.saturating_add(offset - u.text.start),
                    _ => u.dom.start,
                };
                downstream = Some(TextOrigin::Dom {
                    node: u.node,
                    offset: dom,
                });
            }
            if u.text.end == offset {
                upstream = Some(TextOrigin::Dom {
                    node: u.node,
                    offset: u.dom.end,
                });
            }
        }
        match affinity {
            Affinity::Upstream => upstream.or(downstream),
            Affinity::Downstream => downstream.or(upstream),
        }
    }
    #[test]
    fn indexes_match_prechange_linear_semantics_for_overlaps_and_gaps() {
        let mut state = 0x51e9_u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u32
        };
        for _ in 0..256 {
            let mut m = OffsetMapping::default();
            for _ in 0..16 {
                let node = NodeId(u64::from(next() % 4));
                let start = next() % 20;
                let text_start = next() % 20;
                let kind = match next() % 3 {
                    0 => MappingKind::Identity,
                    1 => MappingKind::Collapsed,
                    _ => MappingKind::Expanded,
                };
                m.push_unit(MappingUnit {
                    kind,
                    node,
                    dom: start..start + next() % 5,
                    text: text_start
                        ..text_start
                            + if kind == MappingKind::Collapsed {
                                0
                            } else {
                                next() % 5
                            },
                });
                let generated = next() % 20;
                m.push_generated(
                    generated..generated + next() % 5,
                    NodeId(100 + u64::from(next() % 4)),
                );
            }
            for offset in (0..25).chain([u32::MAX]) {
                for node in [NodeId(0), NodeId(1), NodeId(2), NodeId(3), NodeId(99)] {
                    assert_eq!(
                        m.dom_to_text(node, offset),
                        linear_dom(&m, node, offset),
                        "{node:?}/{offset}: {m:?}"
                    );
                }
                for affinity in [Affinity::Upstream, Affinity::Downstream] {
                    assert_eq!(
                        m.text_to_dom(offset, affinity),
                        linear_text(&m, offset, affinity),
                        "{offset}/{affinity:?}: {m:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn generated_overlap_and_query_independent_equality_are_preserved() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..1, 0..1));
        m.push_generated(1..3, NodeId(9));
        m.push_generated(1..4, NodeId(8));
        m.push_generated(1..1, NodeId(7));
        m.push_unit(unit(MappingKind::Identity, 1..2, 4..5));
        let clone = m.clone();
        assert_eq!(
            m.text_to_dom(1, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(8) })
        );
        assert_eq!(
            m.text_to_dom(2, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(
            m.text_to_dom(4, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(8) })
        );
        assert_eq!(
            m.text_to_dom(4, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(m, clone, "first query cannot change mapping equality");
        assert_eq!(m.text_to_dom(6, Affinity::Downstream), None);
        m.push_generated(5..7, NodeId(10));
        assert_eq!(
            m.text_to_dom(6, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(10) })
        );
        m.push_generated(7..9, NodeId(10));
        assert_eq!(
            m.text_to_dom(8, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(10) })
        );
    }
}
