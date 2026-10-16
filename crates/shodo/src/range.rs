//! Shared intersections of source-text and prepared-unit ranges.
use std::ops::Range;

/// Intersection with empty results anchored at the later start. Text byte
/// offsets and prepared unit indices share the same boundary semantics.
pub(crate) fn intersect<T: Ord + Copy>(a: &Range<T>, b: &Range<T>) -> Range<T> {
    let start = a.start.max(b.start);
    start..a.end.min(b.end).max(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersections_keep_overlap_and_normalize_empty_ranges() {
        let cases: [(Range<usize>, Range<usize>, Range<usize>); 7] = [
            (1..5, 3..7, 3..5),
            (1..8, 3..5, 3..5),
            (1..5, 1..5, 1..5),
            (1..3, 3..7, 3..3),
            (1..3, 5..7, 5..5),
            (3..3, 1..7, 3..3),
            (1..3, 5..5, 5..5),
        ];
        for (a, b, expected) in cases {
            assert_eq!(intersect(&a, &b), expected);
            assert_eq!(intersect(&b, &a), expected);
        }
        assert_eq!(intersect(&(1u32..5), &(3u32..7)), 3u32..5);
    }
}
