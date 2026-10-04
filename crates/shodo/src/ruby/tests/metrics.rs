//! Literal fixed-point spacing policies used by the real accepted formatter.
use crate::RubyAlign;
use crate::geometry::{LayoutUnit, Saturation};

fn unit(value: f32) -> LayoutUnit {
    LayoutUnit::from_f32_round(value, &mut Saturation::default())
}

#[test]
fn all_ruby_alignments_position_two_ten_pixel_units_inside_forty_pixels() {
    for (align, expected) in [
        (RubyAlign::Start, [0.0, 10.0]),
        (RubyAlign::Center, [10.0, 20.0]),
        (RubyAlign::SpaceBetween, [0.0, 30.0]),
        (RubyAlign::SpaceAround, [5.0, 25.0]),
    ] {
        let gaps = super::align::gaps(align, 2, unit(20.0));
        let mut pen = unit(0.0);
        let positions: Vec<_> = gaps
            .iter()
            .map(|(before, after)| {
                let position = pen + *before;
                pen = pen + unit(10.0) + *before + *after;
                position.to_f32()
            })
            .collect();
        assert_eq!(positions, expected, "{align:?}");
        assert_eq!(pen, unit(40.0));
    }
}

#[test]
fn single_space_between_centers_and_empty_alignment_has_no_slots() {
    assert_eq!(
        super::align::gaps(RubyAlign::SpaceBetween, 1, unit(30.0)),
        [(unit(15.0), unit(15.0))]
    );
    for align in [
        RubyAlign::Start,
        RubyAlign::Center,
        RubyAlign::SpaceBetween,
        RubyAlign::SpaceAround,
    ] {
        assert!(super::align::gaps(align, 0, unit(30.0)).is_empty());
    }
}

#[test]
fn alignment_gaps_preserve_every_fixed_point_remainder() {
    for align in [
        RubyAlign::Start,
        RubyAlign::Center,
        RubyAlign::SpaceBetween,
        RubyAlign::SpaceAround,
    ] {
        for count in 1..17 {
            let gaps = super::align::gaps(align, count, LayoutUnit::from_raw(103));
            assert_eq!(
                gaps.iter()
                    .map(|(before, after)| before.raw() + after.raw())
                    .sum::<i32>(),
                103,
                "{align:?}/{count}"
            );
        }
    }
}

#[test]
fn appended_alignment_gaps_keep_the_prefix_and_match_fresh_gaps() {
    let prefix = (unit(7.0), unit(9.0));
    for align in [
        RubyAlign::Start,
        RubyAlign::Center,
        RubyAlign::SpaceBetween,
        RubyAlign::SpaceAround,
    ] {
        for count in 0..17 {
            let extra = LayoutUnit::from_raw(103);
            let expected = super::align::gaps(align, count, extra);
            let mut combined = vec![prefix; 3];
            super::align::append_gaps(align, count, extra, &mut combined);
            assert_eq!(&combined[..3], &[prefix; 3], "{align:?}/{count} prefix");
            assert_eq!(&combined[3..], expected, "{align:?}/{count} suffix");
        }
    }
}
