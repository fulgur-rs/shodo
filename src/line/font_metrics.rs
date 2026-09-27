//! Font metrics resolved once from the same instance used for shaping.
use crate::font::{FontCollection, FontId, FontMetrics, FontQuery};
use crate::limits::WarningSink;
use crate::style::InlineStyle;

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyleMetrics {
    pub(crate) font: FontId,
    pub(crate) size: f32,
    pub(crate) metrics: FontMetrics,
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
            metrics: fonts
                .metrics_with_coords(found.id, size, &instance.coords)
                .expect("retained primary face"),
        };
    }
    let font = fonts.primary_font();
    StyleMetrics {
        font,
        size: style.font_size,
        metrics: fonts.metrics(font, style.font_size),
    }
}
