//! Eligibility for copying first-line work while retaining separate data.

use crate::style::{
    InlineStyle,
    identity::{authored_float, inherited_float_bits, same_variations},
};

#[cfg(test)]
std::thread_local! {
    pub(super) static FORCE_REFERENCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static STYLE_COMPARISON_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn enabled() -> bool {
    #[cfg(test)]
    return FORCE_REFERENCE.with(|reference| !reference.get());
    #[cfg(not(test))]
    true
}

pub(super) fn styles_match(normal: &[InlineStyle], alternate: &[InlineStyle]) -> bool {
    normal.len() == alternate.len()
        && normal.iter().zip(alternate).all(|(a, b)| {
            // Reject inherited numeric differences before cloning owned
            // family/feature payloads on the shape-changing fallback path.
            if inherited_float_bits(a) != inherited_float_bits(b) {
                return false;
            }
            #[cfg(test)]
            STYLE_COMPARISON_CLONES.with(|count| count.set(count.get() + 1));
            let mut without_paint = a.clone();
            without_paint.paint = b.paint;
            without_paint == *b
                && same_variations(&a.font_variations, &b.font_variations, authored_float)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{FontStyle, LineHeight};

    #[test]
    fn inherited_floats_require_authored_bits_and_reject_nan() {
        let fields: [fn(&mut InlineStyle, f32); 10] = [
            |s, v| s.font_size = v,
            |s, v| s.font_weight = v,
            |s, v| s.font_width = v,
            |s, v| s.font_style = FontStyle::Oblique(v),
            |s, v| {
                s.font_size_adjust = Some(crate::style::FontSizeAdjust {
                    metric: crate::style::FontMetricKind::ExHeight,
                    value: v,
                })
            },
            |s, v| s.line_height = LineHeight::Number(v),
            |s, v| s.letter_spacing = v,
            |s, v| s.word_spacing = v,
            |s, v| s.word_spacing_percent = v,
            |s, v| {
                s.font_variations = vec![crate::style::FontVariation {
                    tag: *b"wght",
                    value: v,
                }]
            },
        ];
        for set in fields {
            for (left, right, expected) in [
                (0.0, -0.0, false),
                (1.0, 1.0, true),
                (f32::NAN, f32::NAN, false),
            ] {
                let mut a = InlineStyle::default();
                let mut b = InlineStyle::default();
                set(&mut a, left);
                set(&mut b, right);
                assert_eq!(styles_match(&[a], &[b]), expected);
            }
        }
        // Non-inherited layout properties retain their public equality policy.
        let a = InlineStyle {
            tab_size: crate::style::TabSize::Px(0.0),
            ..Default::default()
        };
        let b = InlineStyle {
            tab_size: crate::style::TabSize::Px(-0.0),
            ..Default::default()
        };
        assert!(styles_match(&[a], &[b]));
    }
}
