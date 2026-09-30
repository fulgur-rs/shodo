use super::index::Segment;

/// Source search never changes the original segment order used by paint/hit.
pub(super) enum SourceIndex {
    Ordered,
    Tree(Vec<Entry>),
}
pub(super) struct Entry {
    segment: usize,
    max_end: u32,
}
impl SourceIndex {
    pub(super) fn new(segments: &[Segment]) -> Self {
        if segments
            .windows(2)
            .all(|s| s[0].text.start <= s[1].text.start && s[0].text.end <= s[1].text.end)
        {
            return Self::Ordered;
        }
        let mut entries: Vec<_> = (0..segments.len())
            .map(|segment| Entry {
                segment,
                max_end: 0,
            })
            .collect();
        entries.sort_unstable_by_key(|e| (segments[e.segment].text.start, e.segment));
        Self::summarize(&mut entries, segments);
        Self::Tree(entries)
    }
    fn summarize(entries: &mut [Entry], segments: &[Segment]) -> u32 {
        if entries.is_empty() {
            return 0;
        }
        let mid = entries.len() / 2;
        let (left, rest) = entries.split_at_mut(mid);
        let (entry, right) = rest.split_first_mut().unwrap();
        entry.max_end = segments[entry.segment]
            .text
            .end
            .max(Self::summarize(left, segments))
            .max(Self::summarize(right, segments));
        entry.max_end
    }
    pub(super) fn for_each(
        &self,
        from: u32,
        to: u32,
        segments: &[Segment],
        mut emit: impl FnMut(&Segment),
    ) {
        if from >= to {
            return;
        }
        match self {
            Self::Ordered => {
                let begin = segments.partition_point(|s| {
                    #[cfg(test)]
                    super::selection::tests::visit();
                    s.text.end <= from
                });
                let end = segments.partition_point(|s| {
                    #[cfg(test)]
                    super::selection::tests::visit();
                    s.text.start < to
                });
                // Every interval has start <= end, so begin <= end for from < to.
                for segment in &segments[begin..end] {
                    emit(segment);
                }
            }
            Self::Tree(entries) => Self::query(entries, from, to, segments, &mut emit),
        }
    }
    fn query(
        entries: &[Entry],
        from: u32,
        to: u32,
        segments: &[Segment],
        emit: &mut impl FnMut(&Segment),
    ) {
        if entries.is_empty() {
            return;
        }
        let mid = entries.len() / 2;
        let entry = &entries[mid];
        #[cfg(test)]
        super::selection::tests::visit();
        if entry.max_end <= from {
            return;
        }
        Self::query(&entries[..mid], from, to, segments, emit);
        let segment = &segments[entry.segment];
        if segment.text.start >= to {
            return;
        }
        if segment.text.end > from {
            emit(segment);
        }
        Self::query(&entries[mid + 1..], from, to, segments, emit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LogicalRect;

    fn segments(ranges: &[std::ops::Range<u32>]) -> Vec<Segment> {
        ranges
            .iter()
            .enumerate()
            .map(|(i, text)| Segment {
                text: text.clone(),
                from: i as f32,
                to: i as f32 + 1.0,
                rect: LogicalRect {
                    inline_start: i as f32,
                    inline_size: 1.0,
                    block_start: 0.0,
                    block_size: 1.0,
                },
            })
            .collect()
    }
    fn selected(index: &SourceIndex, segments: &[Segment], from: u32, to: u32) -> Vec<usize> {
        let mut ids = Vec::new();
        index.for_each(from, to, segments, |s| {
            ids.push(s.rect.inline_start as usize)
        });
        ids.sort_unstable();
        ids
    }
    #[test]
    fn unordered_overlaps_keep_distinct_duplicate_geometry_and_max_endpoints() {
        let segments = segments(&[20..30, 0..100, 3..5, 3..5, 6..9, u32::MAX - 2..u32::MAX]);
        let index = SourceIndex::new(&segments);
        for (from, to, want) in [
            (0, 1, vec![1]),
            (4, 7, vec![1, 2, 3, 4]),
            (10, 20, vec![1]),
            (20, 21, vec![0, 1]),
            (30, 31, vec![1]),
            (100, 101, vec![]),
            (u32::MAX - 1, u32::MAX, vec![5]),
            (4, 4, vec![]),
        ] {
            assert_eq!(selected(&index, &segments, from, to), want, "{from}..{to}");
        }
    }
    #[test]
    fn ordered_duplicate_ranges_respect_open_source_intersection_bounds() {
        let segments = segments(&[0..3, 3..6, 3..6, 6..9]);
        let index = SourceIndex::new(&segments);
        for (from, to, want) in [
            (0, 3, vec![0]),
            (3, 4, vec![1, 2]),
            (2, 7, vec![0, 1, 2, 3]),
            (9, 10, vec![]),
            (3, 3, vec![]),
        ] {
            assert_eq!(selected(&index, &segments, from, to), want);
        }
        let empty = SourceIndex::new(&[]);
        assert!(selected(&empty, &[], 0, u32::MAX).is_empty());
    }
    #[test]
    fn nonmonotonic_ends_and_empty_internal_ranges_keep_the_original_predicate() {
        let segments = segments(&[0..100, 1..2, 4..4, 3..6]);
        let index = SourceIndex::new(&segments);
        assert_eq!(selected(&index, &segments, 3, 5), vec![0, 2, 3]);
        assert_eq!(selected(&index, &segments, 50, 51), vec![0]);
        assert_eq!(selected(&index, &segments, 2, 3), vec![0]);
    }
}
