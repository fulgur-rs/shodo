/// Merge two sorted offset lists while removing duplicates from either list.
pub(crate) fn merge_sorted_unique(left: &[u32], right: &[u32]) -> Vec<u32> {
    debug_assert!(left.windows(2).all(|pair| pair[0] <= pair[1]));
    debug_assert!(right.windows(2).all(|pair| pair[0] <= pair[1]));

    let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
    let (mut a, mut b) = (0, 0);
    while a < left.len() || b < right.len() {
        let next = match (left.get(a), right.get(b)) {
            (Some(&x), Some(&y)) if x <= y => {
                a += 1;
                x
            }
            (Some(_), Some(&y)) => {
                b += 1;
                y
            }
            (Some(&x), None) => {
                a += 1;
                x
            }
            (None, Some(&y)) => {
                b += 1;
                y
            }
            (None, None) => unreachable!("the merge loop requires an unconsumed value"),
        };
        if merged.last() != Some(&next) {
            merged.push(next);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    #[test]
    fn merging_sorted_offsets_sorts_and_deduplicates_both_inputs() {
        assert_eq!(
            super::merge_sorted_unique(&[0, 2, 2, 7], &[1, 2, 3, 7, 9]),
            [0, 1, 2, 3, 7, 9]
        );
        assert_eq!(super::merge_sorted_unique(&[], &[0, 0, 4]), [0, 4]);
        assert_eq!(super::merge_sorted_unique(&[1, 1], &[]), [1]);
    }
}
