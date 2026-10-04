//! Throwaway probe for all-at-once versus query-direction OffsetMapping indexes.
//! Run `time` without allocation-counting, `alloc` with that feature enabled,
//! or `massif baseline|directional dom|text|both 4096` under Valgrind Massif.
use std::collections::{BTreeMap, BTreeSet};
use std::hint::black_box;
use std::ops::Range;
use std::sync::OnceLock;
use std::time::Instant;

mod node {
    pub use shodo::node::NodeId;
}

// Include the checked-in implementation so this executable can construct
// synthetic mappings without widening Shodo's public or crate-private API.
#[allow(dead_code)]
#[path = "../../../crates/shodo/src/mapping.rs"]
mod current;

use current::{Affinity, MappingKind, MappingUnit, OffsetMapping, TextOrigin, TransformSpan};

#[cfg(feature = "allocation-counting")]
#[global_allocator]
static ALLOCATOR: shodo_bench::allocator::CountingAllocator<std::alloc::System> =
    shodo_bench::allocator::CountingAllocator::new(std::alloc::System);

#[derive(Debug)]
struct TextIndexes {
    text: RangeIndex,
    generated: RangeIndex,
}

#[derive(Debug, Default)]
struct DirectionalIndexes {
    dom: OnceLock<BTreeMap<node::NodeId, RangeIndex>>,
    text: OnceLock<TextIndexes>,
}

#[derive(Debug)]
struct IndexedRange {
    start: u32,
    end: u32,
    ordinal: usize,
    subtree_end: u32,
    subtree_first: usize,
}

/// This is the existing RangeIndex representation. Only which records are
/// initialized together changes in the directional candidate.
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
        let through = self
            .ranges
            .partition_point(|r| r.start < offset || !strict_start && r.start == offset);
        let mut found = usize::MAX;
        self.find(0, self.ranges.len(), through, offset, &mut found);
        (found != usize::MAX).then_some(found)
    }

    fn find(&self, begin: usize, end: usize, through: usize, offset: u32, found: &mut usize) {
        if begin >= end || begin >= through {
            return;
        }
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

    fn for_each_closed(&self, offset: u32, mut visit: impl FnMut(usize)) {
        let through = self.ranges.partition_point(|r| r.start <= offset);
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
        let through = self.ends.partition_point(|(end, _)| *end <= offset);
        self.ends
            .get(through.checked_sub(1)?)
            .filter(|(end, _)| *end == offset)
            .map(|(_, ordinal)| *ordinal)
    }

    fn last_start(&self, offset: u32) -> Option<usize> {
        let through = self.ranges.partition_point(|r| r.start <= offset);
        self.ranges
            .get(through.checked_sub(1)?)
            .filter(|r| r.start == offset)
            .map(|r| r.ordinal)
    }
}

impl DirectionalIndexes {
    fn dom<'a>(&'a self, mapping: &OffsetMapping) -> &'a BTreeMap<node::NodeId, RangeIndex> {
        self.dom.get_or_init(|| {
            let mut sources: BTreeMap<node::NodeId, Vec<(Range<u32>, usize)>> = BTreeMap::new();
            for (ordinal, unit) in mapping.units().iter().enumerate() {
                sources
                    .entry(unit.node)
                    .or_default()
                    .push((unit.dom.clone(), ordinal));
            }
            sources
                .into_iter()
                .map(|(node, ranges)| (node, RangeIndex::new(ranges)))
                .collect()
        })
    }

    fn text<'a>(
        &'a self,
        mapping: &OffsetMapping,
        generated: &[(Range<u32>, node::NodeId)],
    ) -> &'a TextIndexes {
        self.text.get_or_init(|| TextIndexes {
            text: RangeIndex::new(
                mapping
                    .units()
                    .iter()
                    .enumerate()
                    .filter(|(_, unit)| unit.kind != MappingKind::Collapsed)
                    .map(|(ordinal, unit)| (unit.text.clone(), ordinal)),
            ),
            generated: RangeIndex::new(
                generated
                    .iter()
                    .enumerate()
                    .filter(|(_, (range, _))| !range.is_empty())
                    .map(|(ordinal, (range, _))| (range.clone(), ordinal)),
            ),
        })
    }

    fn dom_to_text(
        &self,
        mapping: &OffsetMapping,
        node: node::NodeId,
        offset: u32,
    ) -> Option<(u32, Affinity)> {
        let ranges = self.dom(mapping).get(&node)?;
        if let Some(ordinal) = ranges.first_containing(offset, false) {
            let unit = &mapping.units()[ordinal];
            let text = match unit.kind {
                MappingKind::Identity => unit.text.start.saturating_add(offset - unit.dom.start),
                MappingKind::Collapsed => unit.text.end,
                MappingKind::Expanded => unit.text.start,
            };
            return Some((text, Affinity::Downstream));
        }
        ranges
            .last_end(offset)
            .map(|ordinal| (mapping.units()[ordinal].text.end, Affinity::Upstream))
    }

    fn source_candidates(
        &self,
        mapping: &OffsetMapping,
        node: node::NodeId,
        offset: u32,
        requested: Affinity,
    ) -> Vec<(u32, Affinity)> {
        let mut candidates = Vec::new();
        let Some(ranges) = self.dom(mapping).get(&node) else {
            return candidates;
        };
        let candidate = |ordinal: usize| {
            let unit = &mapping.units()[ordinal];
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

    fn text_to_dom(
        &self,
        mapping: &OffsetMapping,
        generated: &[(Range<u32>, node::NodeId)],
        offset: u32,
        affinity: Affinity,
    ) -> Option<TextOrigin> {
        let indexes = self.text(mapping, generated);
        let generated_origin = |ordinal: usize| TextOrigin::Generated {
            node: generated[ordinal].1,
        };
        if let Some(ordinal) = indexes.generated.first_containing(offset, true) {
            return Some(generated_origin(ordinal));
        }
        let downstream = indexes
            .generated
            .last_start(offset)
            .map(generated_origin)
            .or_else(|| {
                let unit = &mapping.units()[indexes.text.first_containing(offset, false)?];
                let dom = match unit.kind {
                    MappingKind::Identity => {
                        unit.dom.start.saturating_add(offset - unit.text.start)
                    }
                    _ => unit.dom.start,
                };
                Some(TextOrigin::Dom {
                    node: unit.node,
                    offset: dom,
                })
            });
        let upstream = indexes
            .text
            .last_end(offset)
            .map(|ordinal| {
                let unit = &mapping.units()[ordinal];
                TextOrigin::Dom {
                    node: unit.node,
                    offset: unit.dom.end,
                }
            })
            .or_else(|| indexes.generated.last_end(offset).map(generated_origin));
        match affinity {
            Affinity::Upstream => upstream.or(downstream),
            Affinity::Downstream => downstream.or(upstream),
        }
    }
}

#[derive(Debug)]
struct ProbeData {
    mapping: OffsetMapping,
    generated: Vec<(Range<u32>, node::NodeId)>,
}

type DomQuery = (node::NodeId, u32);
type TextQuery = (u32, Affinity);

#[derive(Debug)]
struct CachedCandidate {
    mapping: OffsetMapping,
    generated: Vec<(Range<u32>, node::NodeId)>,
    indexes: DirectionalIndexes,
}

impl Clone for CachedCandidate {
    fn clone(&self) -> Self {
        Self {
            mapping: self.mapping.clone(),
            generated: self.generated.clone(),
            indexes: DirectionalIndexes::default(),
        }
    }
}

impl CachedCandidate {
    fn from_data(data: ProbeData) -> Self {
        Self {
            mapping: data.mapping,
            generated: data.generated,
            indexes: DirectionalIndexes::default(),
        }
    }

    fn push_unit(&mut self, unit: MappingUnit) {
        self.mapping.push_unit(unit);
        self.invalidate_all();
    }

    fn push_generated(&mut self, range: Range<u32>, node: node::NodeId) {
        self.mapping.push_generated(range.clone(), node);
        append_generated_mirror(&mut self.generated, range, node);
        self.indexes.text.take();
    }

    fn remap_text(&mut self, spans: &[TransformSpan]) {
        self.mapping.remap_text(spans);
        self.generated = compact_generated(self.generated.drain(..).map(|(range, node)| {
            (
                TransformSpan::map_position(spans, range.start)
                    ..TransformSpan::map_position(spans, range.end),
                node,
            )
        }));
        self.invalidate_all();
    }

    fn invalidate_all(&mut self) {
        self.indexes.dom.take();
        self.indexes.text.take();
    }
}

fn append_generated_mirror(
    generated: &mut Vec<(Range<u32>, node::NodeId)>,
    range: Range<u32>,
    node: node::NodeId,
) {
    if let Some((last, last_node)) = generated.last_mut()
        && *last_node == node
        && last.end == range.start
    {
        last.end = range.end;
    } else {
        generated.push((range, node));
    }
}

fn append_generated(
    mapping: &mut OffsetMapping,
    generated: &mut Vec<(Range<u32>, node::NodeId)>,
    range: Range<u32>,
    node: node::NodeId,
) {
    mapping.push_generated(range.clone(), node);
    append_generated_mirror(generated, range, node);
}

fn compact_generated(
    generated: impl IntoIterator<Item = (Range<u32>, node::NodeId)>,
) -> Vec<(Range<u32>, node::NodeId)> {
    let mut compacted: Vec<(Range<u32>, node::NodeId)> = Vec::new();
    for (range, node) in generated {
        if let Some((last, last_node)) = compacted.last_mut()
            && *last_node == node
            && last.end == range.start
        {
            last.end = range.end;
        } else {
            compacted.push((range, node));
        }
    }
    compacted
}

fn workload(records: usize) -> ProbeData {
    let mut mapping = OffsetMapping::default();
    let mut generated = Vec::new();
    let mut text_offset = 0_u32;
    for ordinal in 0..records {
        let node = node::NodeId((ordinal % 64) as u64);
        let dom_start = ((ordinal / 64) * 4) as u32;
        let kind = match ordinal % 7 {
            0 => MappingKind::Collapsed,
            1 => MappingKind::Expanded,
            _ => MappingKind::Identity,
        };
        let (dom_len, text_len) = match kind {
            MappingKind::Identity => (1, 1),
            MappingKind::Collapsed => (3, 0),
            MappingKind::Expanded => (2, 3),
        };
        mapping.push_unit(MappingUnit {
            kind,
            node,
            dom: dom_start..dom_start + dom_len,
            text: text_offset..text_offset + text_len,
        });
        text_offset += text_len;
        if ordinal % 5 == 4 {
            let generated_node = node::NodeId(10_000 + (ordinal % 8) as u64);
            append_generated(
                &mut mapping,
                &mut generated,
                text_offset..text_offset + 2,
                generated_node,
            );
            text_offset += 2;
        }
    }
    ProbeData { mapping, generated }
}

fn overlap_workload() -> ProbeData {
    let mut mapping = OffsetMapping::default();
    let mut generated = Vec::new();
    for (kind, node, dom, text) in [
        (MappingKind::Expanded, 7, 0..4, 0..1),
        (MappingKind::Identity, 7, 1..3, 1..3),
        (MappingKind::Collapsed, 7, 2..5, 1..1),
        (MappingKind::Identity, 8, 0..2, 3..5),
        (MappingKind::Expanded, 7, 8..10, 6..8),
    ] {
        mapping.push_unit(MappingUnit {
            kind,
            node: node::NodeId(node),
            dom,
            text,
        });
    }
    for (range, node) in [(1..4, 90), (1..3, 91), (2..2, 92), (5..7, 90)] {
        append_generated(&mut mapping, &mut generated, range, node::NodeId(node));
    }
    ProbeData { mapping, generated }
}

fn verify_equivalence(data: &ProbeData) -> usize {
    let candidate = DirectionalIndexes::default();
    verify_equivalence_with(&data.mapping, &data.generated, &candidate)
}

fn verify_equivalence_with(
    mapping: &OffsetMapping,
    generated: &[(Range<u32>, node::NodeId)],
    candidate: &DirectionalIndexes,
) -> usize {
    let before = mapping.clone();
    let mut verified = 0;
    let units = mapping.units();
    let mut dom_queries = BTreeSet::new();
    let mut text_queries = BTreeSet::from([0, u32::MAX]);
    for unit in units {
        for offset in [
            unit.dom.start.saturating_sub(1),
            unit.dom.start,
            unit.dom.start.saturating_add(1),
            unit.dom.end.saturating_sub(1),
            unit.dom.end,
            unit.dom.end.saturating_add(1),
        ] {
            dom_queries.insert((unit.node, offset));
        }
        for offset in [
            unit.text.start.saturating_sub(1),
            unit.text.start,
            unit.text.start.saturating_add(1),
            unit.text.end.saturating_sub(1),
            unit.text.end,
            unit.text.end.saturating_add(1),
        ] {
            text_queries.insert(offset);
        }
    }
    for (range, _) in generated {
        for offset in [
            range.start.saturating_sub(1),
            range.start,
            range.end,
            range.end.saturating_add(1),
        ] {
            text_queries.insert(offset);
        }
    }
    for (node, offset) in dom_queries {
        assert_eq!(
            mapping.dom_to_text(node, offset),
            candidate.dom_to_text(mapping, node, offset),
            "dom_to_text {node:?}/{offset}"
        );
        for affinity in [Affinity::Upstream, Affinity::Downstream] {
            assert_eq!(
                mapping.source_candidates(node, offset, affinity),
                candidate.source_candidates(mapping, node, offset, affinity),
                "source_candidates {node:?}/{offset}/{affinity:?}"
            );
            verified += 1;
        }
        verified += 1;
    }
    for offset in text_queries {
        for affinity in [Affinity::Upstream, Affinity::Downstream] {
            assert_eq!(
                mapping.text_to_dom(offset, affinity),
                candidate.text_to_dom(mapping, generated, offset, affinity),
                "text_to_dom {offset}/{affinity:?}"
            );
            verified += 1;
        }
    }
    assert_eq!(*mapping, before, "queries cannot alter equality");
    verified
}

fn verify_invalidation() -> usize {
    let mut candidate = CachedCandidate::from_data(workload(96));
    let mut verified =
        verify_equivalence_with(&candidate.mapping, &candidate.generated, &candidate.indexes);

    let cloned = candidate.clone();
    verified += verify_equivalence_with(&cloned.mapping, &cloned.generated, &cloned.indexes);

    let end = candidate
        .mapping
        .units()
        .iter()
        .map(|unit| unit.text.end)
        .chain(candidate.generated.iter().map(|(range, _)| range.end))
        .max()
        .unwrap_or(0);
    candidate.push_unit(MappingUnit {
        kind: MappingKind::Identity,
        node: node::NodeId(20_000),
        dom: 0..1,
        text: end + 3..end + 4,
    });
    candidate.indexes.dom(&candidate.mapping);
    candidate.push_generated(end + 5..end + 6, node::NodeId(20_001));
    verified +=
        verify_equivalence_with(&candidate.mapping, &candidate.generated, &candidate.indexes);

    let middle = (end + 8) / 2;
    let spans = [
        TransformSpan {
            old: 0..middle,
            new: 0..middle,
            kind: MappingKind::Identity,
        },
        TransformSpan {
            old: middle..end + 8,
            new: middle + 2..end + 10,
            kind: MappingKind::Expanded,
        },
    ];
    candidate.remap_text(&spans);
    verified +=
        verify_equivalence_with(&candidate.mapping, &candidate.generated, &candidate.indexes);
    verified
}

fn query_sets(data: &ProbeData) -> (Vec<DomQuery>, Vec<TextQuery>) {
    let stride = (data.mapping.units().len() / 64).max(1);
    let mut dom = Vec::new();
    let mut text = Vec::new();
    for unit in data.mapping.units().iter().step_by(stride) {
        dom.push((unit.node, unit.dom.start));
        dom.push((unit.node, unit.dom.end));
        for offset in [unit.text.start, unit.text.end] {
            text.push((offset, Affinity::Upstream));
            text.push((offset, Affinity::Downstream));
        }
    }
    for (range, _) in data.generated.iter().step_by(stride) {
        for offset in [range.start, range.end] {
            text.push((offset, Affinity::Upstream));
            text.push((offset, Affinity::Downstream));
        }
    }
    if dom.is_empty() {
        dom.push((node::NodeId(0), 0));
    }
    if text.is_empty() {
        text.extend([(0, Affinity::Upstream), (0, Affinity::Downstream)]);
    }
    (dom, text)
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

const COLD_BATCH: usize = 16;

fn time_cold_dom(data: &ProbeData, query: (node::NodeId, u32), samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mappings: Vec<_> = (0..COLD_BATCH).map(|_| data.mapping.clone()).collect();
        let started = Instant::now();
        for mapping in &mappings {
            black_box(mapping.dom_to_text(query.0, query.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_cold_text(data: &ProbeData, query: (u32, Affinity), samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mappings: Vec<_> = (0..COLD_BATCH).map(|_| data.mapping.clone()).collect();
        let started = Instant::now();
        for mapping in &mappings {
            black_box(mapping.text_to_dom(query.0, query.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_cold_both(
    data: &ProbeData,
    dom: (node::NodeId, u32),
    text: (u32, Affinity),
    samples: usize,
) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mappings: Vec<_> = (0..COLD_BATCH).map(|_| data.mapping.clone()).collect();
        let started = Instant::now();
        for mapping in &mappings {
            black_box(mapping.dom_to_text(dom.0, dom.1));
            black_box(mapping.text_to_dom(text.0, text.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_cold_dom(data: &ProbeData, query: (node::NodeId, u32), samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.dom_to_text(&data.mapping, query.0, query.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_cold_text(data: &ProbeData, query: (u32, Affinity), samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.text_to_dom(&data.mapping, &data.generated, query.0, query.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_cold_both(
    data: &ProbeData,
    dom: (node::NodeId, u32),
    text: (u32, Affinity),
    samples: usize,
) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.dom_to_text(&data.mapping, dom.0, dom.1));
            black_box(index.text_to_dom(&data.mapping, &data.generated, text.0, text.1));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_init_dom(data: &ProbeData, samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.dom(&data.mapping));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_init_text(data: &ProbeData, samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.text(&data.mapping, &data.generated));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_candidate_init_all(data: &ProbeData, samples: usize) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let indexes: Vec<_> = (0..COLD_BATCH)
            .map(|_| DirectionalIndexes::default())
            .collect();
        let started = Instant::now();
        for index in &indexes {
            black_box(index.dom(&data.mapping));
            black_box(index.text(&data.mapping, &data.generated));
        }
        times.push((started.elapsed().as_nanos() / COLD_BATCH as u128) as u64);
    }
    median(times)
}

fn time_warm_dom(
    data: &ProbeData,
    queries: &[(node::NodeId, u32)],
    samples: usize,
    repeats: usize,
    candidate: bool,
) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let index = DirectionalIndexes::default();
        let baseline = data.mapping.clone();
        if candidate {
            index.dom(&data.mapping);
        } else {
            baseline.dom_to_text(queries[0].0, queries[0].1);
        }
        let started = Instant::now();
        for i in 0..repeats {
            let (node, offset) = queries[i % queries.len()];
            if candidate {
                black_box(index.dom_to_text(&data.mapping, node, offset));
            } else {
                black_box(baseline.dom_to_text(node, offset));
            }
        }
        times.push((started.elapsed().as_nanos() / repeats as u128) as u64);
    }
    median(times)
}

fn time_warm_text(
    data: &ProbeData,
    queries: &[(u32, Affinity)],
    samples: usize,
    repeats: usize,
    candidate: bool,
) -> u64 {
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let index = DirectionalIndexes::default();
        let baseline = data.mapping.clone();
        if candidate {
            index.text(&data.mapping, &data.generated);
        } else {
            baseline.text_to_dom(queries[0].0, queries[0].1);
        }
        let started = Instant::now();
        for i in 0..repeats {
            let (offset, affinity) = queries[i % queries.len()];
            if candidate {
                black_box(index.text_to_dom(&data.mapping, &data.generated, offset, affinity));
            } else {
                black_box(baseline.text_to_dom(offset, affinity));
            }
        }
        times.push((started.elapsed().as_nanos() / repeats as u128) as u64);
    }
    median(times)
}

#[cfg(feature = "allocation-counting")]
fn allocations(data: &ProbeData) -> serde_json::Value {
    use serde_json::json;

    let dom = (node::NodeId(1), 1);
    let text = (1, Affinity::Downstream);

    let baseline_dom = data.mapping.clone();
    let baseline_dom_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let result = baseline_dom.dom_to_text(dom.0, dom.1);
        (result, scope.finish())
    };
    let candidate_dom = DirectionalIndexes::default();
    let candidate_dom_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let result = candidate_dom.dom_to_text(&data.mapping, dom.0, dom.1);
        (result, scope.finish())
    };
    assert_eq!(baseline_dom_result.0, candidate_dom_result.0);

    let baseline_text = data.mapping.clone();
    let baseline_text_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let result = baseline_text.text_to_dom(text.0, text.1);
        (result, scope.finish())
    };
    let candidate_text = DirectionalIndexes::default();
    let candidate_text_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let result = candidate_text.text_to_dom(&data.mapping, &data.generated, text.0, text.1);
        (result, scope.finish())
    };
    assert_eq!(baseline_text_result.0, candidate_text_result.0);

    let baseline_both = data.mapping.clone();
    let baseline_both_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let dom = baseline_both.dom_to_text(dom.0, dom.1);
        let text = baseline_both.text_to_dom(text.0, text.1);
        ((dom, text), scope.finish())
    };
    let candidate_both = DirectionalIndexes::default();
    let candidate_both_result = {
        let scope = ALLOCATOR.begin().unwrap();
        let dom = candidate_both.dom_to_text(&data.mapping, dom.0, dom.1);
        let text = candidate_both.text_to_dom(&data.mapping, &data.generated, text.0, text.1);
        ((dom, text), scope.finish())
    };
    assert_eq!(baseline_both_result.0, candidate_both_result.0);

    json!({
        "dom_only": {
            "all_indexes": baseline_dom_result.1,
            "directional": candidate_dom_result.1,
        },
        "text_only": {
            "all_indexes": baseline_text_result.1,
            "directional": candidate_text_result.1,
        },
        "both": {
            "all_indexes": baseline_both_result.1,
            "directional": candidate_both_result.1,
        }
    })
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(not(feature = "allocation-counting"))]
fn massif_probe(mut args: impl Iterator<Item = String>) {
    let strategy = args
        .next()
        .expect("massif mode requires baseline or directional");
    let direction = args
        .next()
        .expect("massif mode requires dom, text, or both");
    let records = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(4096);
    assert!(matches!(strategy.as_str(), "baseline" | "directional"));
    assert!(matches!(direction.as_str(), "dom" | "text" | "both"));

    let data = workload(records);
    let (dom_queries, text_queries) = query_sets(&data);
    match (strategy.as_str(), direction.as_str()) {
        ("baseline", "dom") => {
            black_box(data.mapping.dom_to_text(dom_queries[0].0, dom_queries[0].1));
        }
        ("baseline", "text") => {
            black_box(
                data.mapping
                    .text_to_dom(text_queries[0].0, text_queries[0].1),
            );
        }
        ("baseline", "both") => {
            black_box(data.mapping.dom_to_text(dom_queries[0].0, dom_queries[0].1));
            black_box(
                data.mapping
                    .text_to_dom(text_queries[0].0, text_queries[0].1),
            );
        }
        ("directional", "dom") => {
            let index = DirectionalIndexes::default();
            black_box(index.dom_to_text(&data.mapping, dom_queries[0].0, dom_queries[0].1));
        }
        ("directional", "text") => {
            let index = DirectionalIndexes::default();
            black_box(index.text_to_dom(
                &data.mapping,
                &data.generated,
                text_queries[0].0,
                text_queries[0].1,
            ));
        }
        ("directional", "both") => {
            let index = DirectionalIndexes::default();
            black_box(index.dom_to_text(&data.mapping, dom_queries[0].0, dom_queries[0].1));
            black_box(index.text_to_dom(
                &data.mapping,
                &data.generated,
                text_queries[0].0,
                text_queries[0].1,
            ));
        }
        _ => unreachable!(),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "time".into());
    if mode == "massif" {
        #[cfg(feature = "allocation-counting")]
        panic!("Massif mode requires the default allocator");
        #[cfg(not(feature = "allocation-counting"))]
        {
            massif_probe(args);
            return;
        }
    }
    assert!(matches!(mode.as_str(), "time" | "alloc"));
    #[cfg(feature = "allocation-counting")]
    assert_ne!(mode, "time", "time mode must omit allocation-counting");
    #[cfg(not(feature = "allocation-counting"))]
    assert_ne!(mode, "alloc", "alloc mode requires allocation-counting");
    let samples = env_usize("SHODO_OFFSET_MAPPING_SAMPLES", 13);
    let repeats = env_usize("SHODO_OFFSET_MAPPING_WARM_QUERIES", 512);
    assert!(samples > 0 && repeats > 0);

    for records in [16, 256, 4096] {
        let data = workload(records);
        let verified_queries = verify_equivalence(&data) + verify_equivalence(&overlap_workload());
        let (dom_queries, text_queries) = query_sets(&data);
        if mode == "time" {
            let base_dom_cold = time_cold_dom(&data, dom_queries[0], samples);
            let candidate_dom_cold = time_candidate_cold_dom(&data, dom_queries[0], samples);
            let candidate_dom_init = time_candidate_init_dom(&data, samples);
            let base_dom_warm = time_warm_dom(&data, &dom_queries, samples, repeats, false);
            let candidate_dom_warm = time_warm_dom(&data, &dom_queries, samples, repeats, true);

            let base_text_cold = time_cold_text(&data, text_queries[0], samples);
            let candidate_text_cold = time_candidate_cold_text(&data, text_queries[0], samples);
            let candidate_text_init = time_candidate_init_text(&data, samples);
            let base_text_warm = time_warm_text(&data, &text_queries, samples, repeats, false);
            let candidate_text_warm = time_warm_text(&data, &text_queries, samples, repeats, true);

            let base_both_cold = time_cold_both(&data, dom_queries[0], text_queries[0], samples);
            let candidate_both_cold =
                time_candidate_cold_both(&data, dom_queries[0], text_queries[0], samples);
            let all_indexes_init = time_candidate_init_all(&data, samples);
            println!(
                "{}",
                serde_json::json!({
                    "records": records,
                    "generated_ranges": data.generated.len(),
                    "verified_queries": verified_queries,
                    "samples": samples,
                    "warm_queries_per_sample": repeats,
                    "dom_only": {
                        "all_indexes": {
                            "cold_first_query_ns": base_dom_cold,
                            "estimated_init_ns": base_dom_cold.saturating_sub(base_dom_warm),
                            "warm_query_ns": base_dom_warm,
                        },
                        "directional": {
                            "cold_first_query_ns": candidate_dom_cold,
                            "index_init_ns": candidate_dom_init,
                            "warm_query_ns": candidate_dom_warm,
                        }
                    },
                    "text_only": {
                        "all_indexes": {
                            "cold_first_query_ns": base_text_cold,
                            "estimated_init_ns": base_text_cold.saturating_sub(base_text_warm),
                            "warm_query_ns": base_text_warm,
                        },
                        "directional": {
                            "cold_first_query_ns": candidate_text_cold,
                            "index_init_ns": candidate_text_init,
                            "warm_query_ns": candidate_text_warm,
                        }
                    },
                    "both_first_pair_ns": {
                        "all_indexes": {
                            "cold_first_pair": base_both_cold,
                            "index_init": all_indexes_init,
                        },
                        "directional": candidate_both_cold,
                    }
                })
            );
        } else {
            #[cfg(feature = "allocation-counting")]
            println!(
                "{}",
                serde_json::json!({
                    "records": records,
                    "generated_ranges": data.generated.len(),
                    "verified_queries": verified_queries,
                    "allocations": allocations(&data),
                })
            );
            #[cfg(not(feature = "allocation-counting"))]
            unreachable!();
        }
    }
    println!(
        "{{\"invalidation_query_checks\":{}}}",
        verify_invalidation()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_split_matches_boundaries_generated_overlaps_and_affinity() {
        assert!(verify_equivalence(&workload(256)) > 1000);
        assert!(verify_equivalence(&overlap_workload()) > 50);
    }

    #[test]
    fn clone_push_and_remap_states_match_fresh_directional_indexes() {
        assert!(verify_invalidation() > 3000);
    }
}
