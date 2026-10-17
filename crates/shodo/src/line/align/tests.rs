use super::*;

#[test]
fn uniform_level_detection_requires_exact_values() {
    assert_eq!(uniform_opportunity_level([0, 0].into_iter()), Some(0));
    assert_eq!(uniform_opportunity_level([2, 2].into_iter()), Some(2));
    assert_eq!(uniform_opportunity_level([1, 1].into_iter()), Some(1));
    assert_eq!(uniform_opportunity_level([3, 3].into_iter()), Some(3));
    assert_eq!(uniform_opportunity_level([0, 2].into_iter()), None);
    assert_eq!(uniform_opportunity_level([1, 3].into_iter()), None);
    assert_eq!(uniform_opportunity_level([].into_iter()), None);
}

fn opportunities(counts: &[i64], owned_at: Option<usize>) -> Vec<Opportunity> {
    counts
        .iter()
        .enumerate()
        .map(|(unit, count)| Opportunity {
            point: Point {
                unit,
                owned: (owned_at == Some(unit)).then_some((7, 11)),
            },
            count: *count,
        })
        .collect()
}

fn unit_order(opportunities: &[Opportunity]) -> Vec<usize> {
    opportunities
        .iter()
        .map(|opportunity| opportunity.point.unit)
        .collect()
}

#[test]
fn uniform_levels_skip_reorder_vectors_and_preserve_visual_order() {
    for level in 0..=3 {
        for base_level in 0..=1 {
            reset_reorder_vector_constructions();
            let input = opportunities(&[1, 2, 1], Some(1));
            let output = reorder_opportunities(input, base_level, |_| level);
            let mut expected = vec![0, 1, 2];
            if level % 2 != base_level % 2 {
                expected.reverse();
            }
            assert_eq!(
                unit_order(&output),
                expected,
                "level={level}, base={base_level}"
            );
            assert_eq!(
                reorder_vector_constructions(),
                0,
                "uniform level={level}, base={base_level} should avoid reorder vectors"
            );
        }
    }
}

#[test]
fn uniform_opportunities_preserve_residual_distribution_and_owned_metadata() {
    for levels in [&[0, 2][..], &[1, 3][..]] {
        for base_level in 0..=1 {
            reset_reorder_vector_constructions();
            let input = opportunities(&[1, 2], Some(1));
            let actual = reorder_opportunities(input, base_level, |opportunity| {
                levels[opportunity.point.unit]
            });
            let bidi_levels: Vec<_> = levels
                .iter()
                .map(|level| Level::new(*level).unwrap())
                .collect();
            let mut expected = BidiInfo::reorder_visual(&bidi_levels);
            if base_level % 2 == 1 {
                expected.reverse();
            }
            assert_eq!(
                unit_order(&actual),
                expected,
                "levels={levels:?}, base={base_level}"
            );
            assert_eq!(reorder_vector_constructions(), 1);
        }
    }

    reset_reorder_vector_constructions();
    let actual = reorder_opportunities(opportunities(&[1, 1, 2], Some(2)), 1, |_| 0);
    assert_eq!(unit_order(&actual), [2, 1, 0]);
    assert_eq!(actual[0].count, 2);
    assert_eq!(actual[0].point.owned, Some((7, 11)));
    let n: i64 = actual.iter().map(|opportunity| opportunity.count).sum();
    let mut remainder = 7_i64 % n;
    let per_boundary = 7_i64 / n;
    let extras: Vec<_> = actual
        .iter()
        .map(|opportunity| {
            let residual = remainder.min(opportunity.count);
            remainder -= residual;
            per_boundary * opportunity.count + residual
        })
        .collect();
    assert_eq!(extras, [4, 2, 1]);
    assert_eq!(reorder_vector_constructions(), 0);
}
