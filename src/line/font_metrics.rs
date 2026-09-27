//! Font metrics resolved once from the same instance used for shaping.
use crate::font::{FontCollection, FontId, FontMetrics, FontQuery};
use crate::limits::WarningSink;
use crate::style::InlineStyle;
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyleMetrics {
    pub(crate) font: FontId,
    pub(crate) size: f32,
    pub(crate) metrics: FontMetrics,
    pub(crate) space: f32,
    pub(crate) ch: f32,
}

pub(crate) fn resolve(
    fonts: &FontCollection,
    style: &InlineStyle,
    warnings: &mut WarningSink,
) -> StyleMetrics {
    let query = FontQuery {
        families: style.font_families.clone(),
        weight: style.font_weight,
        width: style.font_width,
        style: style.font_style,
        language: style.lang.clone(),
        synthesis: style.font_synthesis,
        ..Default::default()
    };
    let space = character_advance(fonts, style, &query, ' ', 1.0, warnings);
    let ch = character_advance(fonts, style, &query, '0', 0.5, warnings);
    if let Some(found) = fonts.match_primary(&query)
        && let Some(data) = fonts.font_data(found.id)
    {
        let (_, instance, size) = crate::shape::resolve_instance(
            data.data.as_ref(),
            data.index,
            &found,
            style,
            *b"Latn",
            warnings,
        );
        return StyleMetrics {
            font: found.id,
            size,
            space,
            ch,
            metrics: fonts
                .metrics_with_coords(found.id, size, &instance.coords)
                .expect("retained primary face"),
        };
    }
    let font = fonts.primary_font();
    StyleMetrics {
        font,
        size: style.font_size,
        space,
        ch,
        metrics: fonts.metrics(font, style.font_size),
    }
}

/// Resolve the character's fallback face and its actual adjusted instance,
/// including explicit variations, optical size and HVAR glyph advances.
fn character_advance(
    fonts: &FontCollection,
    style: &InlineStyle,
    query: &FontQuery,
    ch: char,
    fallback: f32,
    warnings: &mut WarningSink,
) -> f32 {
    let mut encoded = [0; 4];
    let Some(found) = fonts.match_cluster(query, ch.encode_utf8(&mut encoded)) else {
        return style.font_size * fallback;
    };
    let Some(data) = fonts.font_data(found.id) else {
        return style.font_size * fallback;
    };
    let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
        return style.font_size * fallback;
    };
    let (_, instance, size) = crate::shape::resolve_instance(
        data.data.as_ref(),
        data.index,
        &found,
        style,
        *b"Latn",
        warnings,
    );
    font.charmap()
        .map(ch)
        .and_then(|glyph| {
            font.glyph_metrics(Size::new(size), LocationRef::new(&instance.coords))
                .advance_width(glyph)
        })
        .unwrap_or(size * fallback)
}
