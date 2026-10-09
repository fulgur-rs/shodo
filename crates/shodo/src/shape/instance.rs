//! Resolved shaping instance shared by every run emitted from one window.
use crate::font::{FontMatch, NormalizedCoord};
use crate::limits::{WarningKind, WarningSink};
use crate::style::{FontMetricKind, FontVariation, InlineStyle};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    raw::TableProvider,
};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
std::thread_local! {
    pub(super) static INSTANCE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static COORDINATE_INSTANCE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static VARIATION_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunInstance {
    pub(crate) metrics: Option<crate::font::FontMetrics>,
    pub(crate) vertical_metrics: Option<crate::font::VerticalFontMetrics>,
    pub(crate) coords: Vec<NormalizedCoord>,
    pub(crate) variations: Vec<FontVariation>,
    pub(crate) embolden: bool,
    pub(crate) skew: Option<f32>,
    pub(crate) script: [u8; 4],
    pub(crate) language: Option<String>,
    pub(crate) features: Arc<[harfrust::Feature]>,
}

pub(crate) fn resolve(
    bytes: &[u8],
    index: u32,
    found: &FontMatch,
    style: &InlineStyle,
    script: [u8; 4],
    warnings: &mut WarningSink,
) -> (harfrust::ShaperInstance, Arc<RunInstance>, f32) {
    let (shaper, instance, size, warning, _) =
        resolve_with_warning(bytes, index, found, style, script);
    if let Some(warning) = warning {
        warning.emit(warnings);
    }
    (shaper, instance, size)
}

/// Resolution emits at most one size-adjust warning. Keep its effect even when
/// the caller's sink suppresses it, so reuse can replay every attempted push.
#[derive(Clone, Copy)]
pub(super) enum ResolutionWarning {
    SizeClamped,
    AdjustUnavailable,
}
impl ResolutionWarning {
    pub(super) fn emit(self, warnings: &mut WarningSink) {
        let (kind, message) = match self {
            Self::SizeClamped => (
                WarningKind::Saturated,
                "adjusted font size clamped to 1e6 px",
            ),
            Self::AdjustUnavailable => (
                WarningKind::Unsupported,
                "font-size-adjust metric unavailable; retaining font size",
            ),
        };
        warnings.push(kind, message);
    }
}

pub(super) fn resolve_with_warning(
    bytes: &[u8],
    index: u32,
    found: &FontMatch,
    style: &InlineStyle,
    script: [u8; 4],
) -> (
    harfrust::ShaperInstance,
    Arc<RunInstance>,
    f32,
    Option<ResolutionWarning>,
    bool,
) {
    #[cfg(test)]
    INSTANCE_BUILDS.with(|count| count.set(count.get() + 1));
    let mut warning = None;
    let font = harfrust::FontRef::from_index(bytes, index).expect("registered face");
    #[cfg(test)]
    crate::font::record_metric_font_ref_open();
    let metric_font = FontRef::from_index(bytes, index).expect("registered metric face");
    let explicit = |tag| {
        style
            .font_variations
            .iter()
            .rfind(|v| v.tag == tag)
            .copied()
    };
    let mut variations = found.variations.clone();
    // Preserve first-occurrence order and last-setting values without scanning
    // the growing vector for every author-controlled tag.
    let mut variation_indices = HashMap::new();
    for (i, variation) in variations.iter().enumerate() {
        variation_indices.entry(variation.tag).or_insert(i);
    }
    // Explicit coordinates must be included while determining size-adjust.
    for v in &style.font_variations {
        set_variation(&mut variations, &mut variation_indices, *v);
    }
    let mut size = style.font_size;
    if let Some(adjust) = style.font_size_adjust {
        #[cfg(test)]
        COORDINATE_INSTANCE_BUILDS.with(|count| count.set(count.get() + 1));
        let metric_instance = harfrust::ShaperInstance::from_variations(
            &font,
            variations.iter().map(|v| harfrust::Variation {
                tag: harfrust::Tag::new(&v.tag),
                value: v.value,
            }),
        );
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
                warning = Some(ResolutionWarning::SizeClamped);
                size = 1e6;
            }
        } else {
            warning = Some(ResolutionWarning::AdjustUnavailable);
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
            &mut variation_indices,
            FontVariation {
                tag: *b"opsz",
                value: size,
            },
        );
    }
    // Explicit settings are last; axes() and harfrust clamp to supported ranges.
    for v in &style.font_variations {
        set_variation(&mut variations, &mut variation_indices, *v);
    }
    #[cfg(test)]
    COORDINATE_INSTANCE_BUILDS.with(|count| count.set(count.get() + 1));
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
        metrics: None,
        vertical_metrics: None,
        coords: instance.coords().to_vec(),
        variations,
        embolden: found.embolden,
        skew: found.skew,
        script,
        language: style.lang.clone(),
        // Item orientation/width features are supplied by shape_items. Metric
        // consumers only need the resolved coordinates and size.
        features: Arc::default(),
    });
    // Default coordinates may be cleared while Harfrust retains its heap
    // capacity. Inspect the face's axis count, not the resulting slice length.
    let inline_coords = font.fvar().ok().is_none_or(|fvar| fvar.axis_count() <= 11);
    (instance, result, size, warning, inline_coords)
}
fn set_variation(
    variations: &mut Vec<FontVariation>,
    indices: &mut HashMap<[u8; 4], usize>,
    value: FontVariation,
) {
    #[cfg(test)]
    VARIATION_LOOKUPS.with(|count| count.set(count.get() + 1));
    match indices.entry(value.tag) {
        std::collections::hash_map::Entry::Occupied(entry) => variations[*entry.get()] = value,
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(variations.len());
            variations.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_variations_have_bounded_lookup_work_and_last_wins() {
        let fonts = crate::font::FontCollection::with_options(
            &crate::limits::Limits::default(),
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let found = FontMatch {
            id: fonts
                .register(crate::test_support::fonts::LATIN.to_vec())
                .unwrap(),
            variations: vec![FontVariation {
                tag: *b"TEST",
                value: 1.0,
            }],
            embolden: false,
            skew: None,
        };
        for count in [128u32, 512] {
            let mut settings: Vec<_> = (0..count)
                .map(|i| FontVariation {
                    tag: [
                        b'T',
                        b'A' + (i / 26 / 26) as u8,
                        b'A' + ((i / 26) % 26) as u8,
                        b'A' + (i % 26) as u8,
                    ],
                    value: i as f32,
                })
                .collect();
            settings.push(FontVariation {
                tag: *b"TEST",
                value: 2.0,
            });
            settings.push(FontVariation {
                tag: settings[0].tag,
                value: -1.0,
            });
            let style = InlineStyle {
                font_variations: settings,
                ..Default::default()
            };
            VARIATION_LOOKUPS.with(|visits| visits.set(0));
            let (_, run, _) = resolve(
                crate::test_support::fonts::LATIN,
                0,
                &found,
                &style,
                *b"Latn",
                &mut WarningSink::default(),
            );
            assert_eq!(run.variations.len(), count as usize + 1);
            assert_eq!(
                run.variations[0],
                FontVariation {
                    tag: *b"TEST",
                    value: 2.0
                }
            );
            assert_eq!(run.variations[1].value, -1.0);
            for (i, value) in run.variations[2..].iter().enumerate() {
                assert_eq!(value.value, (i + 1) as f32);
            }
            assert!(
                VARIATION_LOOKUPS.with(|visits| visits.get()) <= 4 * style.font_variations.len()
            );
        }
    }

    #[test]
    fn no_size_adjust_uses_only_final_coordinate_instance() {
        let fonts = crate::font::FontCollection::with_options(
            &crate::limits::Limits::default(),
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let found = FontMatch {
            id: fonts
                .register(crate::test_support::fonts::LATIN.to_vec())
                .unwrap(),
            variations: Vec::new(),
            embolden: false,
            skew: None,
        };
        for adjust in [
            Some(crate::style::FontSizeAdjust {
                metric: FontMetricKind::ExHeight,
                value: 0.5,
            }),
            None,
        ] {
            let style = InlineStyle {
                font_size: 16.0,
                font_size_adjust: adjust,
                ..Default::default()
            };
            COORDINATE_INSTANCE_BUILDS.with(|count| count.set(0));
            let mut warnings = WarningSink::default();
            let (shaper, run, size) = resolve(
                crate::test_support::fonts::LATIN,
                0,
                &found,
                &style,
                *b"Latn",
                &mut warnings,
            );
            assert!(warnings.take().is_empty());
            assert!(run.coords.is_empty());
            assert_eq!(run.coords, shaper.coords());
            assert!(run.variations.is_empty());
            if adjust.is_none() {
                assert_eq!(size, 16.0);
            } else {
                assert!(size.is_finite() && size > 0.0);
            }
            assert_eq!(
                COORDINATE_INSTANCE_BUILDS.with(|count| count.get()),
                if adjust.is_some() { 2 } else { 1 },
                "preliminary coordinates are needed only for size-adjust"
            );
        }
    }

    #[test]
    fn metric_instance_does_not_build_shaping_features() {
        let fonts = crate::font::FontCollection::with_options(
            &crate::limits::Limits::default(),
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let found = FontMatch {
            id: fonts
                .register(crate::test_support::fonts::LATIN.to_vec())
                .unwrap(),
            variations: Vec::new(),
            embolden: false,
            skew: None,
        };
        let style = InlineStyle {
            font_kerning: crate::style::FontKerning::Normal,
            font_features: vec![crate::style::FontFeature {
                tag: *b"liga",
                value: 0,
            }],
            ..Default::default()
        };
        super::super::features::STYLE_FEATURE_BUILDS.with(|count| count.set(0));
        let (_, instance, size) = resolve(
            crate::test_support::fonts::LATIN,
            0,
            &found,
            &style,
            *b"Latn",
            &mut WarningSink::default(),
        );
        assert_eq!(size, style.font_size);
        assert!(instance.coords.is_empty());
        assert_eq!(instance.script, *b"Latn");
        assert_eq!(
            super::super::features::STYLE_FEATURE_BUILDS.with(|count| count.get()),
            0,
            "coordinate-only resolution must not construct shaping features"
        );
    }

    #[test]
    fn ic_height_adjust_uses_vertical_variation_delta() {
        let bytes = crate::test_support::fonts::CJK;
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

        // The same VVAR delta must reach the public paragraph's TTB glyph
        // advance, rather than affecting only font-size-adjust's ic-height.
        let limits = crate::limits::Limits::default();
        let fonts = crate::font::FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                bytes,
                0,
                crate::font::FontFaceDescriptor {
                    family: "Vertical Variable".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let paragraph_style = crate::style::ParagraphStyle {
            writing_mode: crate::geometry::WritingMode::VerticalRl,
            root: InlineStyle {
                font_size: 16.0,
                font_families: vec![crate::style::FontFamily::Named("Vertical Variable".into())],
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: 900.0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = crate::ParagraphBuilder::new(&paragraph_style, &limits);
        builder.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "水",
        );
        let paragraph = builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap();
        assert_eq!(paragraph.data.glyphs.advance[0].to_f32(), 17.59375);
    }
}
