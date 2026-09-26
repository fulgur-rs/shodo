//! Size and variation-dependent metrics, including CSS ch/ic units.

use super::{FontCollection, FontId, FontMetrics, FontQuery, NormalizedCoord};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    raw::{TableProvider, types::Tag},
};

/// A CSS unit's advance in pixels and the face which actually supplies it.
/// `id` is None when the CSS fallback advance is used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontUnit {
    pub id: Option<FontId>,
    pub advance: f32,
}

/// Vertical line metrics. None is returned for faces without vhea.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerticalFontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

impl FontCollection {
    /// Horizontal metrics at the font's default variation location. The S0
    /// built-in face and unknown identities use the documented stub metrics.
    pub fn metrics(&self, id: FontId, size: f32) -> FontMetrics {
        self.metrics_with_coords(id, size, &[])
            .unwrap_or_else(|| stub_metrics(valid_size(size).unwrap_or(0.)))
    }

    /// Actual horizontal metrics at normalized variation coordinates, with
    /// descent and decoration offsets measured downward from the baseline.
    /// Unknown faces and invalid sizes return None.
    pub fn metrics_with_coords(
        &self,
        id: FontId,
        size: f32,
        coords: &[NormalizedCoord],
    ) -> Option<FontMetrics> {
        let size = valid_size(size)?;
        let data = self.font_data(id)?;
        if id.layer == self.root().layer.id && id.index == 0 {
            return Some(stub_metrics(size));
        }
        let font = FontRef::from_index(data.data.as_ref(), data.index).ok()?;
        let m = font.metrics(Size::new(size), LocationRef::new(coords));
        let defaults = stub_metrics(size);
        Some(FontMetrics {
            ascent: m.ascent,
            descent: -m.descent,
            line_gap: m.leading,
            underline_offset: m.underline.map_or(defaults.underline_offset, |d| -d.offset),
            underline_thickness: m
                .underline
                .map_or(defaults.underline_thickness, |d| d.thickness),
            strikeout_offset: m.strikeout.map_or(defaults.strikeout_offset, |d| -d.offset),
            strikeout_thickness: m
                .strikeout
                .map_or(defaults.strikeout_thickness, |d| d.thickness),
        })
    }

    /// Vertical vhea metrics with MVAR corrections at the supplied location.
    pub fn vertical_metrics(
        &self,
        id: FontId,
        size: f32,
        coords: &[NormalizedCoord],
    ) -> Option<VerticalFontMetrics> {
        let size = valid_size(size)?;
        let data = self.font_data(id)?;
        let font = FontRef::from_index(data.data.as_ref(), data.index).ok()?;
        let vhea = font.vhea().ok()?;
        let scale = Size::new(size).linear_scale(font.head().ok()?.units_per_em());
        let delta = |tag: &[u8; 4]| {
            font.mvar()
                .ok()
                .and_then(|m| m.metric_delta(Tag::new(tag), coords).ok())
                .map_or(0., |d| d.to_f32())
        };
        Some(VerticalFontMetrics {
            ascent: (vhea.ascender().to_i16() as f32 + delta(b"vasc")) * scale,
            descent: -(vhea.descender().to_i16() as f32 + delta(b"vdsc")) * scale,
            line_gap: (vhea.line_gap().to_i16() as f32 + delta(b"vlgp")) * scale,
        })
    }

    /// Advance of '0' from the selected fallback face. Missing glyph: 0.5em.
    pub fn resolve_ch(&self, query: &FontQuery, size: f32) -> FontUnit {
        self.resolve_unit(query, size, '0', 0.5)
    }
    /// Advance of U+6C34 from the selected fallback face. Missing glyph: 1em.
    pub fn resolve_ic(&self, query: &FontQuery, size: f32) -> FontUnit {
        self.resolve_unit(query, size, '水', 1.)
    }

    fn resolve_unit(&self, query: &FontQuery, size: f32, ch: char, fallback: f32) -> FontUnit {
        let size = valid_size(size).unwrap_or(0.);
        let missing = FontUnit {
            id: None,
            advance: size * fallback,
        };
        let mut encoded = [0; 4];
        let Some(found) = self.match_cluster(query, ch.encode_utf8(&mut encoded)) else {
            return missing;
        };
        let Some(data) = self.font_data(found.id) else {
            return missing;
        };
        let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
            return missing;
        };
        let Some(glyph) = font.charmap().map(ch) else {
            return missing;
        };
        let location = font
            .axes()
            .location(found.variations.iter().map(|v| (Tag::new(&v.tag), v.value)));
        match font
            .glyph_metrics(Size::new(size), location.coords())
            .advance_width(glyph)
        {
            Some(advance) => FontUnit {
                id: Some(found.id),
                advance,
            },
            None => missing,
        }
    }
}
fn valid_size(size: f32) -> Option<f32> {
    (size.is_finite() && size >= 0.).then(|| size.min(1e6))
}
fn stub_metrics(size: f32) -> FontMetrics {
    FontMetrics {
        ascent: 0.8 * size,
        descent: 0.2 * size,
        line_gap: 0.,
        underline_offset: 0.1 * size,
        underline_thickness: 0.05 * size,
        strikeout_offset: -0.3 * size,
        strikeout_thickness: 0.05 * size,
    }
}
