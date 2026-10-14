//! Allocation sizes of cloned resolved styles, before retaining caller data.
use super::{FontFamily, InlineStyle, ParagraphStyle};

crate::logical::logical_sizes! {
    INLINE_STYLE_BYTES: InlineStyle = 384;
    FONT_FAMILY_BYTES: FontFamily = 24;
}

fn vec_bytes<T>(values: &[T], element: u64) -> u64 {
    (values.len() as u64).saturating_mul(element)
}

/// Element types with fixed-width fields only, so `size_of` is target-independent.
fn fixed_vec_bytes<T>(values: &[T]) -> u64 {
    vec_bytes(values, std::mem::size_of::<T>() as u64)
}

fn families(values: &[FontFamily]) -> u64 {
    values
        .iter()
        .fold(vec_bytes(values, FONT_FAMILY_BYTES), |bytes, family| {
            bytes.saturating_add(match family {
                FontFamily::Named(name) => name.len() as u64,
                FontFamily::Generic(_) => 0,
            })
        })
}

fn string(value: &Option<String>) -> u64 {
    value.as_ref().map_or(0, |s| s.len() as u64)
}

pub(crate) fn inline(style: &InlineStyle) -> u64 {
    alternate(style, style, style, false)
}

/// Mirrors first-line property selection without cloning any variable data.
/// Call after normal-style sanitization, which can change inheritance equality.
pub(crate) fn alternate(
    normal: &InlineStyle,
    root: &InlineStyle,
    first: &InlineStyle,
    legacy: bool,
) -> u64 {
    macro_rules! selected {
        ($field:ident) => {
            if !legacy || normal.$field == root.$field {
                &first.$field
            } else {
                &normal.$field
            }
        };
    }
    let alternates = selected!(font_variant_alternates);
    [
        INLINE_STYLE_BYTES,
        families(selected!(font_families)),
        fixed_vec_bytes(selected!(font_variations)),
        fixed_vec_bytes(selected!(font_features)),
        fixed_vec_bytes(&alternates.styleset),
        fixed_vec_bytes(&alternates.character_variant),
        string(selected!(lang)),
        string(&normal.hyphenate_character),
    ]
    .into_iter()
    .fold(0u64, u64::saturating_add)
}

pub(crate) fn paragraph(style: &ParagraphStyle) -> u64 {
    inline(&style.root).saturating_add(style.first_line.as_ref().map_or(0, inline))
}

pub(crate) fn styles<'a>(styles: impl IntoIterator<Item = &'a InlineStyle>) -> u64 {
    styles
        .into_iter()
        .fold(0u64, |bytes, style| bytes.saturating_add(inline(style)))
}

#[cfg(test)]
mod charge_tests {
    use super::*;

    #[test]
    fn style_charge_is_a_fixed_logical_amount() {
        let style = InlineStyle {
            font_families: vec![FontFamily::Named("A".into())],
            lang: Some("ja".into()),
            ..Default::default()
        };
        // Header + one family slot + family name + language, identical on every target.
        assert_eq!(inline(&style), 384 + 24 + 1 + 2);
    }
}
