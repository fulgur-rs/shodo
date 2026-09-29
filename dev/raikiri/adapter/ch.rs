//! raikiri → shodo adapter for simple `ch` lengths.
//!
//! `ChFontKey` covers family, size, weight and style only. Variation axes and
//! vertical text orientation are not represented, so callers must reject
//! cascades that depend on them instead of measuring with a different font.
use raikiri_style::property::{
    Direction, FontFamilyName, FontStyle as RFontStyle, FontVariationSettings, WritingMode,
};
use raikiri_style::{ChFontKey, ChLengthProvenance, ComputedValues};
use shodo::font::{ChLength, FontCollection, FontQuery};
use shodo::node::Sides;
use shodo::style::{FontFamily, FontStyle};

pub fn family(names: &[FontFamilyName]) -> Vec<FontFamily> {
    // Named fixture families only; never host fonts.
    names
        .iter()
        .map(|n| FontFamily::Named(n.as_str().to_owned()))
        .collect()
}

pub fn font_style(style: RFontStyle) -> Result<FontStyle, String> {
    match style {
        RFontStyle::Normal => Ok(FontStyle::Normal),
        RFontStyle::Italic => Ok(FontStyle::Italic),
        RFontStyle::Oblique => Ok(FontStyle::Oblique(14.0)),
        _ => Err("font style unsupported".into()),
    }
}

/// Rejects font inputs that `ChFontKey` cannot carry. Measuring with the key
/// alone would silently ignore them, so a cascade that sets any of these
/// fails closed (`font-optical-sizing:auto` on an `opsz` face and
/// `font-stretch` remain unassessed; see docs/records/raikiri-ch-used-value.md).
pub fn require_keyed_font_inputs(values: &ComputedValues) -> Result<(), String> {
    if values.font_variation_settings != FontVariationSettings::Normal {
        return Err("font-variation-settings is not carried by ChFontKey".into());
    }
    Ok(())
}

/// The declaring-font `ch` length for `factor` (no fallback approximation).
pub fn ch_length(key: &ChFontKey, factor: f32) -> Result<ChLength, String> {
    Ok(ChLength {
        query: FontQuery {
            families: family(&key.family),
            weight: key.weight,
            style: font_style(key.style)?,
            ..Default::default()
        },
        size: key.size.0,
        factor,
    })
}

/// Resolves a value that is a `ch` factor of its declaring font, or the
/// absolute `fallback_px` when it was not authored in `ch`.
pub fn resolve_px(
    fonts: &FontCollection,
    factor: Option<f32>,
    key: Option<&ChFontKey>,
    fallback_px: f32,
) -> Result<f32, String> {
    let Some(factor) = factor else {
        return Ok(fallback_px);
    };
    let key = key.ok_or("ch value lost its declaring-font key")?;
    let unit = ch_length(key, factor)?
        .resolve(fonts)
        .ok_or("ch value has a non-finite factor or size")?;
    Ok(unit.advance)
}

/// A non-inherited edge (margin/padding) uses its own provenance.
pub fn resolve_edge(
    fonts: &FontCollection,
    provenance: Option<&ChLengthProvenance>,
    fallback_px: f32,
) -> Result<f32, String> {
    resolve_px(
        fonts,
        provenance.map(|p| p.factor),
        provenance.map(|p| &p.font),
        fallback_px,
    )
}

/// Physical sides in px, as computed by the cascade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Physical {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Physical → logical mapping for horizontal writing modes only.
pub fn to_logical(dir: Direction, wm: WritingMode, p: Physical) -> Result<Sides, String> {
    if wm != WritingMode::HorizontalTb {
        return Err("vertical writing modes unsupported".into());
    }
    let (inline_start, inline_end) = match dir {
        Direction::Ltr => (p.left, p.right),
        Direction::Rtl => (p.right, p.left),
        _ => return Err("direction unsupported".into()),
    };
    Ok(Sides {
        inline_start,
        inline_end,
        block_start: p.top,
        block_end: p.bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shodo::limits::Limits;

    fn phys() -> Physical {
        Physical {
            top: 1.,
            right: 2.,
            bottom: 3.,
            left: 4.,
        }
    }

    #[test]
    fn factor_without_declaring_key_is_an_error() {
        let fonts = FontCollection::new(&Limits::default());
        assert!(resolve_px(&fonts, Some(2.), None, 9.).is_err());
        assert_eq!(resolve_px(&fonts, None, None, 9.), Ok(9.));
    }

    #[test]
    fn ltr_maps_left_to_inline_start() {
        let s = to_logical(Direction::Ltr, WritingMode::HorizontalTb, phys()).unwrap();
        assert_eq!((s.inline_start, s.inline_end), (4., 2.));
        assert_eq!((s.block_start, s.block_end), (1., 3.));
    }

    #[test]
    fn rtl_maps_right_to_inline_start() {
        let s = to_logical(Direction::Rtl, WritingMode::HorizontalTb, phys()).unwrap();
        assert_eq!((s.inline_start, s.inline_end), (2., 4.));
    }

    #[test]
    fn vertical_modes_are_unsupported() {
        for wm in [
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            assert!(to_logical(Direction::Ltr, wm, phys()).is_err());
        }
    }
}
