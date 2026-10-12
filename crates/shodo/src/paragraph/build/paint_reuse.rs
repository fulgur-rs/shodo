//! Eligibility for copying first-line work while retaining separate data.

use crate::style::{FontStyle, InlineStyle, LineHeight};

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
                && a.font_variations
                    .iter()
                    .zip(&b.font_variations)
                    .all(|(a, b)| a.value.to_bits() == b.value.to_bits())
        })
}

// PartialEq equates signed zeros. RunInstance exposes variation values and
// shaping depends on these inherited floats; require their exact bits too.
fn inherited_float_bits(style: &InlineStyle) -> [u32; 9] {
    [
        style.font_size.to_bits(),
        style.font_weight.to_bits(),
        style.font_width.to_bits(),
        match style.font_style {
            FontStyle::Oblique(value) => value.to_bits(),
            _ => 0,
        },
        style
            .font_size_adjust
            .map_or(0, |adjust| adjust.value.to_bits()),
        match style.line_height {
            LineHeight::Px(value) | LineHeight::Number(value) => value.to_bits(),
            _ => 0,
        },
        style.letter_spacing.to_bits(),
        style.word_spacing.to_bits(),
        style.word_spacing_percent.to_bits(),
    ]
}
