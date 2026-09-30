use crate::analysis::units::BreakClass;
use crate::ruby::cuts::{SafeCut, build, take_visits};

fn cut(ordinal: usize, unit: usize, class: BreakClass) -> SafeCut {
    SafeCut {
        ordinal,
        unit,
        class,
    }
}

fn regular(count: usize, unit_scale: usize) -> Vec<SafeCut> {
    (0..=count)
        .map(|i| {
            cut(
                i,
                i * unit_scale,
                if i == 0 {
                    BreakClass::Prohibited
                } else {
                    BreakClass::Allowed
                },
            )
        })
        .collect()
}

#[test]
fn monotone_multi_level_cuts_consume_all_lanes() {
    let base = regular(4, 3);
    let lanes = vec![
        (0..=4)
            .map(|i| cut(i * 2, i * 4, BreakClass::Allowed))
            .collect(),
        (0..=4)
            .map(|i| cut(i * 3, i * 6, BreakClass::Allowed))
            .collect(),
    ];
    let paired = build(&base, &lanes);
    assert_eq!(
        paired.iter().map(|c| c.unit).collect::<Vec<_>>(),
        [0, 3, 6, 9, 12]
    );
    assert_eq!(
        paired
            .iter()
            .map(|c| c.lanes.iter().copied().collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [
            vec![0, 0],
            vec![4, 6],
            vec![8, 12],
            vec![12, 18],
            vec![16, 24]
        ]
    );
}

#[test]
fn equal_proportional_distances_choose_earlier_cut() {
    let base = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(2, 6, BreakClass::Allowed),
        cut(4, 12, BreakClass::Allowed),
    ];
    let paired = build(&base, &[regular(3, 2)]);
    assert_eq!(paired.len(), 3);
    assert_eq!(paired[1].unit, 6);
    assert_eq!(paired[1].lanes, [2]);
}

#[test]
fn repeated_nearest_cuts_do_not_create_empty_internal_fragments() {
    let paired = build(&regular(4, 3), &[regular(3, 2)]);
    assert_eq!(
        paired.iter().map(|c| c.unit).collect::<Vec<_>>(),
        [0, 3, 9, 12]
    );
    assert_eq!(
        paired.iter().map(|c| c.lanes[0]).collect::<Vec<_>>(),
        [0, 2, 4, 6]
    );
}

#[test]
fn no_wrap_lane_only_allows_complete_pair() {
    let paired = build(
        &regular(4, 3),
        &[vec![
            cut(0, 0, BreakClass::Prohibited),
            cut(8, 16, BreakClass::Allowed),
        ]],
    );
    assert_eq!(paired.iter().map(|c| c.unit).collect::<Vec<_>>(), [0, 12]);
    assert_eq!(
        paired.iter().map(|c| c.lanes[0]).collect::<Vec<_>>(),
        [0, 16]
    );
}

#[test]
fn mandatory_and_emergency_classes_keep_their_precedence() {
    let base = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(1, 3, BreakClass::Allowed),
        cut(2, 6, BreakClass::Mandatory),
        cut(3, 9, BreakClass::Allowed),
    ];
    let lane = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(1, 2, BreakClass::Emergency),
        cut(2, 4, BreakClass::Allowed),
        cut(3, 6, BreakClass::Allowed),
    ];
    let paired = build(&base, &[lane]);
    assert_eq!(paired[1].class, BreakClass::Emergency);
    assert_eq!(paired[2].class, BreakClass::Mandatory);
}

#[test]
fn zero_text_endpoints_still_advance_real_unit_cursors() {
    let paired = build(
        &[
            cut(0, 1, BreakClass::Prohibited),
            cut(0, 3, BreakClass::Allowed),
        ],
        &[vec![
            cut(0, 11, BreakClass::Prohibited),
            cut(0, 13, BreakClass::Allowed),
        ]],
    );
    assert_eq!(paired.iter().map(|c| c.unit).collect::<Vec<_>>(), [1, 3]);
    assert_eq!(
        paired.iter().map(|c| c.lanes[0]).collect::<Vec<_>>(),
        [11, 13]
    );
    assert!(build(&[], &[]).is_empty());
}

#[test]
fn long_ruby_cut_index_is_linear() {
    let mut visits = Vec::new();
    for count in [256, 512] {
        take_visits();
        let paired = build(
            &regular(count, 3),
            &[regular(count * 2, 2), regular(count * 3, 2)],
        );
        assert_eq!(paired.len(), count + 1);
        visits.push(take_visits());
    }
    assert!(visits[0] > 0);
    assert!(
        visits[1] <= visits[0] * 3,
        "doubling cut input must not quadruple visits: {visits:?}"
    );
}

#[test]
fn mandatory_base_break_survives_a_no_wrap_lane() {
    let base = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(1, 3, BreakClass::Mandatory),
        cut(2, 6, BreakClass::Allowed),
    ];
    let lane = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(3, 6, BreakClass::Allowed),
    ];
    let paired = build(&base, &[lane]);
    assert_eq!(paired.iter().map(|c| c.unit).collect::<Vec<_>>(), [0, 3, 6]);
    assert_eq!(
        paired.iter().map(|c| c.lanes[0]).collect::<Vec<_>>(),
        [0, 6, 6]
    );
    assert_eq!(paired[1].class, BreakClass::Mandatory);
}

#[test]
fn nearest_search_does_not_skip_an_annotation_mandatory_break() {
    let lane = vec![
        cut(0, 0, BreakClass::Prohibited),
        cut(1, 2, BreakClass::Mandatory),
        cut(2, 4, BreakClass::Allowed),
        cut(3, 6, BreakClass::Allowed),
        cut(4, 8, BreakClass::Allowed),
    ];
    let paired = build(&regular(2, 3), &[lane]);
    assert_eq!(paired[1].unit, 3);
    assert_eq!(paired[1].lanes[0], 2);
    assert_eq!(paired[1].class, BreakClass::Mandatory);
}

#[test]
fn single_safe_cursor_is_valid_empty_content() {
    let paired = build(&[cut(0, 7, BreakClass::Prohibited)], &[]);
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].unit, 7);
    assert!(paired[0].lanes.is_empty());
}

#[test]
fn spanning_and_partial_lanes_keep_their_own_base_intervals() {
    use crate::ruby::cuts::{LaneSpan, build_spanned};
    let mut base = regular(4, 3);
    // A spanning annotation forbids the boundary between the two bases.
    base[2].class = BreakClass::Prohibited;
    let lanes = vec![regular(4, 2), regular(4, 2), regular(8, 2)];
    let spans = [
        LaneSpan {
            units: 0..6,
            ordinals: 0..2,
        },
        LaneSpan {
            units: 6..12,
            ordinals: 2..4,
        },
        LaneSpan {
            units: 0..12,
            ordinals: 0..4,
        },
    ];
    let paired = build_spanned(&base, &lanes, &spans);
    assert_eq!(
        paired.iter().map(|cut| cut.unit).collect::<Vec<_>>(),
        [0, 3, 9, 12]
    );
    assert_eq!(
        paired
            .iter()
            .map(|cut| cut.lanes.iter().copied().collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![0, 0, 0], vec![4, 0, 4], vec![8, 4, 12], vec![8, 8, 16]]
    );
}

#[test]
fn inactive_short_lane_does_not_block_another_base() {
    use crate::ruby::cuts::{LaneSpan, build_spanned};
    let lanes = vec![
        vec![
            cut(0, 0, BreakClass::Prohibited),
            cut(1, 2, BreakClass::Allowed),
        ],
        regular(2, 2),
    ];
    let base = regular(4, 3);
    let spans = [
        LaneSpan {
            units: 0..6,
            ordinals: 0..2,
        },
        LaneSpan {
            units: 6..12,
            ordinals: 2..4,
        },
    ];
    let paired = build_spanned(&base, &lanes, &spans);
    assert_eq!(
        paired.iter().map(|cut| cut.unit).collect::<Vec<_>>(),
        [0, 6, 9, 12]
    );
    assert_eq!(
        paired
            .iter()
            .map(|cut| cut.lanes.iter().copied().collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![0, 0], vec![2, 0], vec![2, 2], vec![2, 4]]
    );
}

#[test]
fn cursor_storage_preserves_rows_that_share_a_parent_unit() {
    use crate::ruby::cuts::PairedBuilder;
    let mut builder = PairedBuilder::new(9, 2);
    let expected = [
        [7, 11],
        [7, 12],
        [8, 12],
        [8, 13],
        [9, 13],
        [9, 14],
        [10, 14],
        [10, 15],
        [11, 15],
    ];
    for (row, lanes) in expected.iter().enumerate() {
        builder.push(
            42,
            lanes,
            if row == 3 {
                BreakClass::Mandatory
            } else {
                BreakClass::Allowed
            },
        );
    }
    let cuts = builder.finish();
    for (row, cut) in cuts.iter().enumerate() {
        assert_eq!(cut.unit, 42);
        assert_eq!(cut.lanes, expected[row]);
        assert_eq!(
            cut.class,
            if row == 3 {
                BreakClass::Mandatory
            } else {
                BreakClass::Allowed
            }
        );
    }
}

#[test]
fn dense_and_sparse_columns_preserve_values_and_bound_retention() {
    use crate::ruby::cuts::PairedBuilder;
    for rows in [1, 2, 7, 8, 9, 65] {
        for bound in [rows, 200] {
            let mut builder = PairedBuilder::new(bound, 3);
            for row in 0..rows {
                builder.push(
                    row * 3,
                    &[row * 2 + 5, 17, usize::from(row >= rows / 2)],
                    BreakClass::Allowed,
                );
            }
            let cuts = builder.finish();
            for (row, cut) in cuts.iter().enumerate() {
                assert_eq!(cut.unit, row * 3);
                assert_eq!(cut.lanes, [row * 2 + 5, 17, usize::from(row >= rows / 2)]);
            }
            if rows >= 8 {
                assert_eq!(cuts[0].lanes.dense_columns(), 1);
                assert!(
                    cuts[0].lanes.table_payload_bytes()
                        <= rows * 3 * std::mem::size_of::<usize>() + 160
                );
            }
        }
    }
}

#[test]
fn many_dense_lane_builder_uses_row_major_fallback() {
    use crate::ruby::cuts::PairedBuilder;
    let mut builder = PairedBuilder::new(8, 64);
    for row in 0..8 {
        builder.push(row * 3, &[row; 64], BreakClass::Allowed);
    }
    let cuts = builder.finish();
    for (row, cut) in cuts.iter().enumerate() {
        assert_eq!(cut.lanes, [row; 64]);
    }
    assert!(
        cuts[0].lanes.table_payload_bytes() <= 8 * 64 * std::mem::size_of::<usize>() + 64,
        "dense64 must shed column metadata, actual {} bytes",
        cuts[0].lanes.table_payload_bytes()
    );
}
