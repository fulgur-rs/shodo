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
        let scale = size / f32::from(m.units_per_em);
        let os2 = font.os2().ok();
        let delta = |tag: &[u8; 4]| {
            font.mvar()
                .ok()
                .and_then(|m| m.metric_delta(Tag::new(tag), coords).ok())
                .map_or(0.0, |d| d.to_f32())
        };
        Some(FontMetrics {
            ascent: m.ascent,
            descent: -m.descent,
            line_gap: m.leading,
            x_height: m.x_height.filter(|h| *h > 0.0).unwrap_or(defaults.x_height),
            cap_height: m
                .cap_height
                .filter(|h| *h > 0.0)
                .unwrap_or(defaults.cap_height),
            subscript_offset: os2.as_ref().map_or(defaults.subscript_offset, |t| {
                (f32::from(t.y_subscript_y_offset()) + delta(b"sbyo")) * scale
            }),
            superscript_offset: os2.as_ref().map_or(defaults.superscript_offset, |t| {
                (f32::from(t.y_superscript_y_offset()) + delta(b"spyo")) * scale
            }),
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
        x_height: 0.5 * size,
        cap_height: 0.66 * size,
        subscript_offset: 0.2 * size,
        superscript_offset: 0.3 * size,
        underline_offset: 0.1 * size,
        underline_thickness: 0.05 * size,
        strikeout_offset: -0.3 * size,
        strikeout_thickness: 0.05 * size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, FontVariation, InlineStyle, ParagraphStyle};
    use crate::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};

    #[test]
    fn variation_and_decoration_metrics_match_run_instance() {
        let base = include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf");
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(base[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(base[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(base[at + 12..at + 16].try_into().unwrap()) as usize;
            tables.push((
                base[at..at + 4].try_into().unwrap(),
                base[offset..offset + len].to_vec(),
            ));
        }
        let mut fvar = Vec::new();
        for value in [1u16, 0, 16, 2, 1, 20, 0, 8] {
            fvar.extend(value.to_be_bytes());
        }
        fvar.extend(b"wght");
        for value in [100i32, 400, 900] {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
        let records = [
            (*b"cpht", 30i16),
            (*b"hasc", 100),
            (*b"hdsc", -50),
            (*b"sbyo", 20),
            (*b"spyo", 30),
            (*b"xhgt", 25),
        ];
        let mut mvar = Vec::new();
        for value in [
            1u16,
            0,
            0,
            8,
            records.len() as u16,
            12 + records.len() as u16 * 8,
        ] {
            mvar.extend(value.to_be_bytes());
        }
        for (index, (tag, _)) in records.iter().enumerate() {
            mvar.extend(tag);
            mvar.extend(0u16.to_be_bytes());
            mvar.extend((index as u16).to_be_bytes());
        }
        mvar.extend(1u16.to_be_bytes());
        mvar.extend(12u32.to_be_bytes());
        mvar.extend(1u16.to_be_bytes());
        mvar.extend(22u32.to_be_bytes());
        for value in [1u16, 1, 0, 16384, 16384] {
            mvar.extend(value.to_be_bytes());
        }
        for value in [records.len() as u16, 1, 1, 0] {
            mvar.extend(value.to_be_bytes());
        }
        for (_, delta) in records {
            mvar.extend(delta.to_be_bytes());
        }
        tables.push((*b"fvar", fvar));
        tables.push((*b"MVAR", mvar));
        tables.sort_by_key(|t| t.0);
        let bytes = crate::font::sfnt::build_sfnt(&tables);
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                bytes,
                0,
                FontFaceDescriptor {
                    family: "Variable".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_size: 20.0,
                font_families: vec![FontFamily::Named("Variable".into())],
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: 900.0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a");
        let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            100.0,
            &AtomicSizes::EMPTY,
        );
        let run = lines[0]
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.normalized_coords().len(), 1);
        let data = run.font_data().unwrap();
        let font = FontRef::from_index(data.data.as_ref(), data.index).unwrap();
        let m = font.metrics(
            Size::new(run.font_size()),
            LocationRef::new(run.normalized_coords()),
        );
        let nominal = font.metrics(Size::new(run.font_size()), LocationRef::default());
        let published = run.metrics();
        let scale = 20.0 / f32::from(m.units_per_em);
        let close = |a: f32, b: f32| assert!((a - b).abs() <= 1.0 / 32.0, "{a} vs {b}");
        close(m.ascent - nominal.ascent, 100.0 * scale);
        close(published.ascent, m.ascent);
        close(published.descent, -m.descent);
        close(published.x_height, m.x_height.unwrap());
        close(published.cap_height, m.cap_height.unwrap());
        close(lines[0].block_size(), m.ascent - m.descent + m.leading);
        let os2 = font.os2().unwrap();
        close(
            published.subscript_offset,
            (f32::from(os2.y_subscript_y_offset()) + 20.0) * scale,
        );
        close(
            published.superscript_offset,
            (f32::from(os2.y_superscript_y_offset()) + 30.0) * scale,
        );
        close(published.underline_offset, -m.underline.unwrap().offset);
        close(published.strikeout_offset, -m.strikeout.unwrap().offset);
    }
}
