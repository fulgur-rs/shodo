//! Borrowed structural keys: no Debug formatting or cloned key payloads.
use crate::style::*;
use std::hash::{BuildHasher, Hash, Hasher};
use std::mem::discriminant;

type Pair<'a> = (&'a InlineStyle, Option<&'a InlineStyle>);

/// Logical bytes retained per style: a fingerprint and a style index.
/// Bucket/allocator overhead and spare capacity follow the style budget's
/// existing exclusions. Colliding entries conservatively each charge a hash.
pub(super) const BYTES: u64 = (size_of::<u64>() + size_of::<u32>()) as u64;

#[derive(Clone, Copy)]
pub(super) struct Key<'a>(pub(super) Pair<'a>);

impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_inline(self.0.0, state);
        self.0.1.is_some().hash(state);
        if let Some(first_line) = self.0.1 {
            hash_inline(first_line, state);
        }
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        same_inline(self.0.0, other.0.0)
            && match (self.0.1, other.0.1) {
                (Some(a), Some(b)) => same_inline(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for Key<'_> {}

pub(super) fn fingerprint(pair: Pair<'_>, state: &impl BuildHasher) -> u64 {
    state.hash_one(Key(pair))
}

/// Debug-style float identity: all NaNs share a key, while signed zeros differ.
/// Keep this internal; public style PartialEq continues to use float equality.
#[derive(Clone, Copy)]
struct Float(u32);

fn float(value: f32) -> Float {
    Float(value.to_bits())
}

impl PartialEq for Float {
    fn eq(&self, other: &Self) -> bool {
        // Most comparisons have identical bits. Delay NaN normalization until
        // needed so consecutive style reuse does not scan every float eagerly.
        self.0 == other.0 || (f32::from_bits(self.0).is_nan() && f32::from_bits(other.0).is_nan())
    }
}

impl Eq for Float {}

impl Hash for Float {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (if f32::from_bits(self.0).is_nan() {
            f32::NAN.to_bits()
        } else {
            self.0
        })
        .hash(state);
    }
}

fn decoration(value: &TextDecoration) -> impl Eq + Hash {
    (
        value.color,
        value.offset.map(float),
        value.thickness.map(float),
    )
}

struct Variations<'a>(&'a [FontVariation]);

impl Hash for Variations<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for variation in self.0 {
            variation.tag.hash(state);
            float(variation.value).hash(state);
        }
    }
}

impl PartialEq for Variations<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0)
                .all(|(a, b)| a.tag == b.tag && float(a.value) == float(b.value))
    }
}

impl Eq for Variations<'_> {}

// Hashing and comparison visit the same exhaustive field list. Compare each
// projection lazily: constructing one large tuple eagerly regresses the common
// consecutive-style fast path, even when the first paint field differs.
macro_rules! inline_fields {
    ($visitor:ident, $($args:expr),+) => {
        $visitor!($($args),+;
            paint => paint,
            font_families => value,
            font_size => number,
            font_weight => number,
            font_width => number,
            font_style => font_style,
            font_variations => variations,
            font_features => value,
            font_kerning => value,
            font_variant_ligatures => value,
            font_variant_caps => value,
            font_variant_numeric => value,
            font_variant_east_asian => value,
            font_variant_position => value,
            font_variant_alternates => value,
            font_optical_sizing => value,
            font_synthesis => value,
            font_size_adjust => font_size_adjust,
            lang => value,
            line_height => line_height,
            letter_spacing => number,
            word_spacing => number,
            word_spacing_percent => number,
            white_space_collapse => value,
            text_wrap_mode => value,
            line_break => value,
            word_break => value,
            overflow_wrap => value,
            hyphens => value,
            hyphenate_character => value,
            text_transform => value,
            word_space_transform => value,
            tab_size => tab_size,
            text_autospace => value,
            text_spacing_trim => value,
            hanging_punctuation => value,
            vertical_align => vertical_align,
            direction => value,
            unicode_bidi => value,
            text_orientation => value,
            text_combine_upright => value,
            text_emphasis => value,
            text_box_edge => value,
            box_decoration_break => value,
        )
    };
}

macro_rules! hash_fields {
    ($style:expr, $state:expr; $($field:ident => $project:ident,)+) => {{
        // Adding a field to InlineStyle requires adding its projection here.
        let InlineStyle { $($field: _,)+ } = $style;
        $($project(&($style).$field).hash($state);)+
    }};
}

macro_rules! equal_fields {
    ($a:expr, $b:expr; $($field:ident => $project:ident,)+) => {
        true $(&& $project(&($a).$field) == $project(&($b).$field))+
    };
}

fn hash_inline(style: &InlineStyle, state: &mut impl Hasher) {
    inline_fields!(hash_fields, style, state);
}

fn same_inline(a: &InlineStyle, b: &InlineStyle) -> bool {
    inline_fields!(equal_fields, a, b)
}

fn value<T>(value: &T) -> &T {
    value
}

fn number(value: &f32) -> Float {
    float(*value)
}

fn paint(value: &PaintStyle) -> impl Eq + Hash {
    (
        value.color,
        value.underline.as_ref().map(decoration),
        value.strikethrough.as_ref().map(decoration),
    )
}

fn variations(value: &[FontVariation]) -> Variations<'_> {
    Variations(value)
}

fn font_style(value: &FontStyle) -> impl Eq + Hash {
    (
        discriminant(value),
        match value {
            FontStyle::Oblique(value) => Some(float(*value)),
            FontStyle::Normal | FontStyle::Italic => None,
        },
    )
}

fn font_size_adjust(value: &Option<FontSizeAdjust>) -> impl Eq + Hash {
    value.map(|value| (value.metric, float(value.value)))
}

fn line_height(value: &LineHeight) -> impl Eq + Hash {
    (
        discriminant(value),
        match value {
            LineHeight::Px(value) | LineHeight::Number(value) => Some(float(*value)),
            LineHeight::Normal => None,
        },
    )
}

fn tab_size(value: &TabSize) -> impl Eq + Hash {
    (
        discriminant(value),
        match value {
            TabSize::Spaces(value) | TabSize::Px(value) => float(*value),
        },
    )
}

fn vertical_align(value: &VerticalAlign) -> impl Eq + Hash {
    (
        discriminant(value),
        match value {
            VerticalAlign::Length(value) => Some(float(*value)),
            VerticalAlign::Baseline
            | VerticalAlign::Sub
            | VerticalAlign::Super
            | VerticalAlign::TextTop
            | VerticalAlign::TextBottom
            | VerticalAlign::Middle
            | VerticalAlign::Top
            | VerticalAlign::Bottom => None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::RandomState;

    #[test]
    fn float_fields_keep_debug_identity_and_equal_hashes() {
        let fields: [fn(&mut InlineStyle, f32); 18] = [
            |s, v| s.font_size = v,
            |s, v| s.font_weight = v,
            |s, v| s.font_width = v,
            |s, v| s.font_style = FontStyle::Oblique(v),
            |s, v| {
                s.font_variations = vec![FontVariation {
                    tag: *b"wght",
                    value: v,
                }]
            },
            |s, v| {
                s.font_size_adjust = Some(FontSizeAdjust {
                    metric: FontMetricKind::ExHeight,
                    value: v,
                })
            },
            |s, v| s.line_height = LineHeight::Px(v),
            |s, v| s.line_height = LineHeight::Number(v),
            |s, v| s.letter_spacing = v,
            |s, v| s.word_spacing = v,
            |s, v| s.word_spacing_percent = v,
            |s, v| s.tab_size = TabSize::Spaces(v),
            |s, v| s.tab_size = TabSize::Px(v),
            |s, v| s.vertical_align = VerticalAlign::Length(v),
            |s, v| {
                s.paint.underline = Some(TextDecoration {
                    offset: Some(v),
                    ..Default::default()
                })
            },
            |s, v| {
                s.paint.underline = Some(TextDecoration {
                    thickness: Some(v),
                    ..Default::default()
                })
            },
            |s, v| {
                s.paint.strikethrough = Some(TextDecoration {
                    offset: Some(v),
                    ..Default::default()
                })
            },
            |s, v| {
                s.paint.strikethrough = Some(TextDecoration {
                    thickness: Some(v),
                    ..Default::default()
                })
            },
        ];
        let values = [
            0.0,
            -0.0,
            1.0,
            f32::from_bits(1.0_f32.to_bits() + 1),
            -2.0,
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::from_bits(0x7fc0_0001),
            f32::from_bits(0xffc0_1234),
        ];
        let state = RandomState::new();
        let normal = InlineStyle::default();
        for (field, set) in fields.into_iter().enumerate() {
            for a in values {
                for b in values {
                    let mut left = InlineStyle::default();
                    let mut right = InlineStyle::default();
                    set(&mut left, a);
                    set(&mut right, b);
                    for (left, right) in [
                        ((&left, None), (&right, None)),
                        ((&normal, Some(&left)), (&normal, Some(&right))),
                    ] {
                        let expected = format!("{left:?}") == format!("{right:?}");
                        assert_eq!(
                            Key(left) == Key(right),
                            expected,
                            "field {field}: {a:?}, {b:?}"
                        );
                        if expected {
                            assert_eq!(fingerprint(left, &state), fingerprint(right, &state));
                        }
                    }
                }
            }
        }
    }
}
