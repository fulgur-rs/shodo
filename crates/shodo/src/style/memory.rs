//! Allocation sizes of cloned resolved styles, before retaining caller data.
use super::{FontFamily, InlineStyle, ParagraphStyle};

fn vec_bytes<T>(values: &[T]) -> u64 {
    (values.len() as u64).saturating_mul(std::mem::size_of::<T>() as u64)
}

fn families(values: &[FontFamily]) -> u64 {
    values.iter().fold(vec_bytes(values), |bytes, family| {
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
        std::mem::size_of::<InlineStyle>() as u64,
        families(selected!(font_families)),
        vec_bytes(selected!(font_variations)),
        vec_bytes(selected!(font_features)),
        vec_bytes(&alternates.styleset),
        vec_bytes(&alternates.character_variant),
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
