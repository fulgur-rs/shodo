//! Internal style identities. Each caller chooses its numeric policy; public
//! `InlineStyle::PartialEq` keeps ordinary float equality.

use super::{FontSizeAdjust, FontStyle, FontVariation, InlineStyle, LineHeight};
use std::hash::{Hash, Hasher};

/// Exact authored bits, including signed zero and NaN payloads.
pub(crate) fn authored_float(value: f32) -> u32 {
    value.to_bits()
}

/// Metrics merge signed zeros, but retain individual NaN payloads.
pub(crate) fn metric_float(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// Debug-style float identity: all NaNs share a key, while signed zeros differ.
/// Keep this internal; public style PartialEq continues to use float equality.
#[derive(Clone, Copy)]
pub(crate) struct InternedFloat(u32);

impl InternedFloat {
    pub(crate) fn new(value: f32) -> Self {
        Self(value.to_bits())
    }
}

impl PartialEq for InternedFloat {
    fn eq(&self, other: &Self) -> bool {
        // Most comparisons have identical bits. Delay NaN normalization until
        // needed so consecutive style reuse does not scan every float eagerly.
        self.0 == other.0 || (f32::from_bits(self.0).is_nan() && f32::from_bits(other.0).is_nan())
    }
}

impl Eq for InternedFloat {}

impl Hash for InternedFloat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (if f32::from_bits(self.0).is_nan() {
            f32::NAN.to_bits()
        } else {
            self.0
        })
        .hash(state);
    }
}

/// Variation order, tags and values all contribute to the chosen identity.
pub(crate) fn same_variations<T: PartialEq>(
    a: &[FontVariation],
    b: &[FontVariation],
    float: impl Fn(f32) -> T,
) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.tag == b.tag && float(a.value) == float(b.value))
}

pub(crate) fn font_style_key<T>(
    value: &FontStyle,
    float: impl Fn(f32) -> T,
) -> (std::mem::Discriminant<FontStyle>, Option<T>) {
    (
        std::mem::discriminant(value),
        match value {
            FontStyle::Oblique(value) => Some(float(*value)),
            FontStyle::Normal | FontStyle::Italic => None,
        },
    )
}

pub(crate) fn size_adjust_key<T>(
    value: Option<FontSizeAdjust>,
    float: impl Fn(f32) -> T,
) -> Option<(super::FontMetricKind, T)> {
    value.map(|value| (value.metric, float(value.value)))
}

fn same_resolution_inputs<T: PartialEq>(
    a: &InlineStyle,
    b: &InlineStyle,
    float: impl Fn(f32) -> T,
) -> bool {
    float(a.font_size) == float(b.font_size)
        && size_adjust_key(a.font_size_adjust, &float)
            == size_adjust_key(b.font_size_adjust, &float)
        && a.font_optical_sizing == b.font_optical_sizing
        && same_variations(&a.font_variations, &b.font_variations, &float)
}

/// Only inputs consumed by instance resolution, using authored float bits.
pub(crate) fn same_instance_inputs(a: &InlineStyle, b: &InlineStyle) -> bool {
    same_resolution_inputs(a, b, authored_float) && a.lang == b.lang
}

/// Matching and instance inputs consumed by metrics. Emphasis needs an em box;
/// its paint content, and shaping features, do not affect this key.
pub(crate) fn same_metric_inputs(a: &InlineStyle, b: &InlineStyle) -> bool {
    a.font_families == b.font_families
        && metric_float(a.font_weight) == metric_float(b.font_weight)
        && metric_float(a.font_width) == metric_float(b.font_width)
        && font_style_key(&a.font_style, metric_float)
            == font_style_key(&b.font_style, metric_float)
        && a.lang == b.lang
        && a.font_synthesis == b.font_synthesis
        && same_resolution_inputs(a, b, metric_float)
        && a.text_emphasis.is_some() == b.text_emphasis.is_some()
}

pub(crate) fn shaping_compatible(a: &InlineStyle, b: &InlineStyle) -> bool {
    // Face/variation selection is compared separately through FontMatch. Keep
    // layout-only properties on their source items instead of cutting GSUB/GPOS.
    macro_rules! same {
        ($($field:ident),* $(,)?) => { true $(&& a.$field == b.$field)* };
    }
    same!(
        font_size,
        font_variations,
        font_features,
        font_kerning,
        font_variant_ligatures,
        font_variant_caps,
        font_variant_numeric,
        font_variant_east_asian,
        font_variant_position,
        font_variant_alternates,
        font_optical_sizing,
        font_synthesis,
        font_size_adjust,
        lang,
        letter_spacing,
        word_spacing,
        word_spacing_percent
    )
}

// PartialEq equates signed zeros. RunInstance exposes variation values and
// shaping depends on these inherited floats; require their exact bits too.
pub(crate) fn inherited_float_bits(style: &InlineStyle) -> [u32; 9] {
    // No rest pattern: new fields must explicitly choose their inherited policy.
    let InlineStyle {
        paint: _,
        font_families: _,
        font_size,
        font_weight,
        font_width,
        font_style,
        font_variations: _,
        font_features: _,
        font_kerning: _,
        font_variant_ligatures: _,
        font_variant_caps: _,
        font_variant_numeric: _,
        font_variant_east_asian: _,
        font_variant_position: _,
        font_variant_alternates: _,
        font_optical_sizing: _,
        font_synthesis: _,
        font_size_adjust,
        lang: _,
        line_height,
        letter_spacing,
        word_spacing,
        word_spacing_percent,
        white_space_collapse: _,
        text_wrap_mode: _,
        line_break: _,
        word_break: _,
        overflow_wrap: _,
        hyphens: _,
        hyphenate_character: _,
        text_transform: _,
        word_space_transform: _,
        tab_size: _,
        text_autospace: _,
        text_spacing_trim: _,
        hanging_punctuation: _,
        vertical_align: _,
        direction: _,
        unicode_bidi: _,
        text_orientation: _,
        text_combine_upright: _,
        text_emphasis: _,
        text_box_edge: _,
        box_decoration_break: _,
    } = style;
    [
        font_size.to_bits(),
        font_weight.to_bits(),
        font_width.to_bits(),
        match *font_style {
            FontStyle::Oblique(value) => value.to_bits(),
            _ => 0,
        },
        font_size_adjust.map_or(0, |adjust| adjust.value.to_bits()),
        match *line_height {
            LineHeight::Px(value) | LineHeight::Number(value) => value.to_bits(),
            _ => 0,
        },
        letter_spacing.to_bits(),
        word_spacing.to_bits(),
        word_spacing_percent.to_bits(),
    ]
}
