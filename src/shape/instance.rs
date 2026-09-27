//! Resolved shaping instance shared by every run emitted from one window.
use crate::font::{FontMatch, NormalizedCoord};
use crate::limits::{WarningKind, WarningSink};
use crate::style::{FontMetricKind, FontVariation, InlineStyle};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    raw::TableProvider,
};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub(crate) struct RunInstance {
    pub(crate) coords: Vec<NormalizedCoord>,
    pub(crate) variations: Vec<FontVariation>,
    pub(crate) embolden: bool,
    pub(crate) skew: Option<f32>,
    pub(crate) script: [u8; 4],
    pub(crate) language: Option<String>,
    pub(crate) features: Vec<harfrust::Feature>,
}

pub(super) fn resolve(
    bytes: &[u8],
    index: u32,
    found: &FontMatch,
    style: &InlineStyle,
    script: [u8; 4],
    warnings: &mut WarningSink,
) -> (harfrust::ShaperInstance, Arc<RunInstance>, f32) {
    let font = harfrust::FontRef::from_index(bytes, index).expect("registered face");
    let metric_font = FontRef::from_index(bytes, index).expect("registered metric face");
    let explicit = |tag| {
        style
            .font_variations
            .iter()
            .rfind(|v| v.tag == tag)
            .copied()
    };
    let mut variations = found.variations.clone();
    // Explicit coordinates must be included while determining size-adjust.
    for v in &style.font_variations {
        set_variation(&mut variations, *v);
    }
    let metric_instance = harfrust::ShaperInstance::from_variations(
        &font,
        variations.iter().map(|v| harfrust::Variation {
            tag: harfrust::Tag::new(&v.tag),
            value: v.value,
        }),
    );
    let mut size = style.font_size;
    if let Some(adjust) = style.font_size_adjust {
        let location = LocationRef::new(metric_instance.coords());
        let m = metric_font.metrics(Size::unscaled(), location);
        let metric = match adjust.metric {
            FontMetricKind::ExHeight => m.x_height,
            FontMetricKind::CapHeight => m.cap_height,
            FontMetricKind::ChWidth | FontMetricKind::IcWidth => metric_font
                .charmap()
                .map(if adjust.metric == FontMetricKind::ChWidth {
                    '0'
                } else {
                    '水'
                })
                .and_then(|g| {
                    metric_font
                        .glyph_metrics(Size::unscaled(), location)
                        .advance_width(g)
                }),
            FontMetricKind::IcHeight => metric_font.charmap().map('水').and_then(|g| {
                let nominal = f32::from(metric_font.vmtx().ok()?.advance(g)?);
                let delta = metric_font
                    .vvar()
                    .ok()
                    .and_then(|v| v.advance_height_delta(g, metric_instance.coords()).ok())
                    .map_or(0.0, |d| d.to_f32());
                Some(nominal + delta)
            }),
        };
        if let Some(metric) = metric.filter(|v| v.is_finite() && *v > 0.0) {
            size = style.font_size * adjust.value * (m.units_per_em as f32 / metric);
            if !size.is_finite() || size > 1e6 {
                warnings.push(
                    WarningKind::Saturated,
                    "adjusted font size clamped to 1e6 px",
                );
                size = 1e6;
            }
        } else {
            warnings.push(
                WarningKind::Unsupported,
                "font-size-adjust metric unavailable; retaining font size",
            );
        }
    }
    if style.font_optical_sizing
        && explicit(*b"opsz").is_none()
        && metric_font
            .axes()
            .iter()
            .any(|a| a.tag() == harfrust::Tag::new(b"opsz"))
    {
        set_variation(
            &mut variations,
            FontVariation {
                tag: *b"opsz",
                value: size,
            },
        );
    }
    // Explicit settings are last; axes() and harfrust clamp to supported ranges.
    for v in &style.font_variations {
        set_variation(&mut variations, *v);
    }
    let instance = harfrust::ShaperInstance::from_variations(
        &font,
        variations.iter().map(|v| harfrust::Variation {
            tag: harfrust::Tag::new(&v.tag),
            value: v.value,
        }),
    );
    // Expose the clamped design coordinates as well as normalized coordinates.
    for variation in &mut variations {
        if let Some(axis) = metric_font
            .axes()
            .iter()
            .find(|a| a.tag() == harfrust::Tag::new(&variation.tag))
        {
            variation.value = variation.value.clamp(axis.min_value(), axis.max_value());
        }
    }
    let result = Arc::new(RunInstance {
        coords: instance.coords().to_vec(),
        variations,
        embolden: found.embolden,
        skew: found.skew,
        script,
        language: style.lang.clone(),
        features: super::features::features(style),
    });
    (instance, result, size)
}
fn set_variation(variations: &mut Vec<FontVariation>, value: FontVariation) {
    if let Some(old) = variations.iter_mut().find(|v| v.tag == value.tag) {
        *old = value;
    } else {
        variations.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ic_height_adjust_uses_vertical_variation_delta() {
        let bytes = include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf");
        let font = FontRef::from_index(bytes, 0).unwrap();
        let glyph = font.charmap().map('水').unwrap();
        let nominal = font.vmtx().unwrap().advance(glyph).unwrap() as f32;
        let upem = font
            .metrics(Size::unscaled(), LocationRef::default())
            .units_per_em as f32;
        let mut fvar = Vec::new();
        for value in [1u16, 0, 16, 2, 1, 20, 0, 8] {
            fvar.extend(value.to_be_bytes());
        }
        fvar.extend(b"wght");
        for value in [100i32, 400, 900] {
            fvar.extend((value << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
        let mut vvar = Vec::new();
        for value in [1u16, 0] {
            vvar.extend(value.to_be_bytes());
        }
        for offset in [24u32, 0, 0, 0, 0] {
            vvar.extend(offset.to_be_bytes());
        }
        // One variation region and one delta set per glyph up to 水. Only
        // that glyph gains100 design units at wght's maximum coordinate.
        vvar.extend(1u16.to_be_bytes());
        vvar.extend(12u32.to_be_bytes());
        vvar.extend(1u16.to_be_bytes());
        vvar.extend(22u32.to_be_bytes());
        for value in [1u16, 1, 0, 16384, 16384] {
            vvar.extend(value.to_be_bytes());
        }
        for value in [glyph.to_u32() as u16 + 1, 1, 1, 0] {
            vvar.extend(value.to_be_bytes());
        }
        for id in 0..=glyph.to_u32() {
            vvar.extend((if id == glyph.to_u32() { 100i16 } else { 0 }).to_be_bytes());
        }
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            tables.push((
                bytes[at..at + 4].try_into().unwrap(),
                bytes[offset..offset + len].to_vec(),
            ));
        }
        tables.push((*b"fvar", fvar));
        tables.push((*b"VVAR", vvar));
        tables.sort_by_key(|(tag, _)| *tag);
        let bytes = crate::font::sfnt::build_sfnt(&tables);
        let style = InlineStyle {
            font_size_adjust: Some(crate::style::FontSizeAdjust {
                metric: FontMetricKind::IcHeight,
                value: 0.6,
            }),
            ..Default::default()
        };
        let found = FontMatch {
            id: crate::font::FontCollection::new(&crate::limits::Limits::default()).primary_font(),
            variations: vec![FontVariation {
                tag: *b"wght",
                value: 900.0,
            }],
            embolden: false,
            skew: None,
        };
        let (instance, _, size) = resolve(
            &bytes,
            0,
            &found,
            &style,
            *b"Hani",
            &mut WarningSink::default(),
        );
        let varied = FontRef::from_index(&bytes, 0).unwrap();
        assert_eq!(
            varied
                .vvar()
                .unwrap()
                .advance_height_delta(glyph, instance.coords())
                .unwrap()
                .to_f32(),
            100.0
        );
        assert!((size - 16.0 * 0.6 * upem / (nominal + 100.0)).abs() < 0.01);
    }
}
