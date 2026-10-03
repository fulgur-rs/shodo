//! Font metrics resolved once from the same instance used for shaping.
use crate::font::{FontCollection, FontId, FontMatch, FontMetrics, FontQuery};
use crate::limits::WarningSink;
use crate::style::InlineStyle;
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[cfg(test)]
pub(crate) mod resolve_counts {
    use std::cell::Cell;

    std::thread_local! {
        static RESOLVES: Cell<usize> = const { Cell::new(0) };
        static INSTANCE_RESOLVES: Cell<usize> = const { Cell::new(0) };
        static MATCHES: Cell<usize> = const { Cell::new(0) };
        static KEY_HASHES: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn reset() {
        RESOLVES.with(|count| count.set(0));
        INSTANCE_RESOLVES.with(|count| count.set(0));
        MATCHES.with(|count| count.set(0));
        KEY_HASHES.with(|count| count.set(0));
    }

    pub(crate) fn snapshot() -> (usize, usize) {
        (RESOLVES.with(Cell::get), INSTANCE_RESOLVES.with(Cell::get))
    }

    pub(crate) fn matches() -> usize {
        MATCHES.with(Cell::get)
    }

    pub(crate) fn key_hashes() -> usize {
        KEY_HASHES.with(Cell::get)
    }

    pub(crate) fn record_key_hash() {
        KEY_HASHES.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn record_resolve() {
        RESOLVES.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn record_instance_resolve() {
        INSTANCE_RESOLVES.with(|count| count.set(count.get() + 1));
    }

    pub(crate) fn record_match() {
        MATCHES.with(|count| count.set(count.get() + 1));
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StyleMetrics {
    pub(crate) font: FontId,
    pub(crate) size: f32,
    pub(crate) metrics: FontMetrics,
    pub(crate) vertical_metrics: Option<crate::font::VerticalFontMetrics>,
    pub(crate) space: f32,
    pub(crate) ch: f32,
    pub(crate) ic: f32,
}

const PROBE_INSTANCE_CACHE_CAPACITY: usize = 4;

struct ProbeInstanceCacheEntry {
    matched: FontMatch,
    instance: Arc<crate::shape::RunInstance>,
    size: f32,
}

struct ProbeInstanceCache {
    entries: [Option<ProbeInstanceCacheEntry>; PROBE_INSTANCE_CACHE_CAPACITY],
    len: usize,
    generations: Option<(u64, Option<u64>)>,
}

impl Default for ProbeInstanceCache {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            len: 0,
            generations: None,
        }
    }
}

impl ProbeInstanceCache {
    fn resolve(
        &mut self,
        fonts: &FontCollection,
        matched: FontMatch,
        bytes: &[u8],
        index: u32,
        style: &InlineStyle,
        warnings: &mut WarningSink,
    ) -> (Arc<crate::shape::RunInstance>, f32) {
        let generations = fonts.generations();
        if self.generations != Some(generations) {
            self.entries = std::array::from_fn(|_| None);
            self.len = 0;
            self.generations = Some(generations);
        }

        if let Some(entry) = self.entries[..self.len]
            .iter()
            .flatten()
            .find(|entry| entry.matched == matched)
            && fonts.generations() == generations
        {
            return (Arc::clone(&entry.instance), entry.size);
        }

        let before = warnings.checkpoint();
        #[cfg(test)]
        resolve_counts::record_instance_resolve();
        let (_, instance, size) =
            crate::shape::resolve_instance(bytes, index, &matched, style, *b"Latn", warnings);
        let after = warnings.checkpoint();
        if matches!((before, after), (Some(before), Some(after)) if before == after)
            && fonts.generations() == generations
            && self.len < PROBE_INSTANCE_CACHE_CAPACITY
        {
            self.entries[self.len] = Some(ProbeInstanceCacheEntry {
                matched,
                instance: Arc::clone(&instance),
                size,
            });
            self.len += 1;
        }
        (instance, size)
    }
}

#[derive(Clone, Copy)]
struct StyleMetricsKey<'a> {
    style: &'a InlineStyle,
    generations: (u64, Option<u64>),
}

impl Hash for StyleMetricsKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        #[cfg(test)]
        resolve_counts::record_key_hash();
        let style = self.style;
        self.generations.hash(state);
        style.font_families.len().hash(state);
        for family in &style.font_families {
            match family {
                crate::style::FontFamily::Named(name) => {
                    0_u8.hash(state);
                    name.hash(state);
                }
                crate::style::FontFamily::Generic(generic) => {
                    1_u8.hash(state);
                    generic.hash(state);
                }
            }
        }
        hash_f32(style.font_weight, state);
        hash_f32(style.font_width, state);
        match style.font_style {
            crate::style::FontStyle::Normal => 0_u8.hash(state),
            crate::style::FontStyle::Italic => 1_u8.hash(state),
            crate::style::FontStyle::Oblique(angle) => {
                2_u8.hash(state);
                hash_f32(angle, state);
            }
        }
        style.lang.hash(state);
        style.font_synthesis.weight.hash(state);
        style.font_synthesis.style.hash(state);
        style.font_synthesis.small_caps.hash(state);
        hash_f32(style.font_size, state);
        style.font_variations.len().hash(state);
        for variation in &style.font_variations {
            variation.tag.hash(state);
            hash_f32(variation.value, state);
        }
        style.font_optical_sizing.hash(state);
        match style.font_size_adjust {
            Some(adjust) => {
                1_u8.hash(state);
                hash_metric_kind(adjust.metric, state);
                hash_f32(adjust.value, state);
            }
            None => 0_u8.hash(state),
        }
    }
}

impl PartialEq for StyleMetricsKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.generations == other.generations && same_metric_inputs(self.style, other.style)
    }
}

impl Eq for StyleMetricsKey<'_> {}

fn hash_f32<H: Hasher>(value: f32, state: &mut H) {
    (if value == 0.0 { 0 } else { value.to_bits() }).hash(state);
}

fn same_f32(a: f32, b: f32) -> bool {
    a == b || (a.is_nan() && b.is_nan() && a.to_bits() == b.to_bits())
}

fn hash_metric_kind<H: Hasher>(metric: crate::style::FontMetricKind, state: &mut H) {
    use crate::style::FontMetricKind;
    match metric {
        FontMetricKind::ExHeight => 0_u8,
        FontMetricKind::CapHeight => 1,
        FontMetricKind::ChWidth => 2,
        FontMetricKind::IcWidth => 3,
        FontMetricKind::IcHeight => 4,
    }
    .hash(state);
}

fn same_metric_inputs(a: &InlineStyle, b: &InlineStyle) -> bool {
    let same_font_style = match (a.font_style, b.font_style) {
        (crate::style::FontStyle::Normal, crate::style::FontStyle::Normal)
        | (crate::style::FontStyle::Italic, crate::style::FontStyle::Italic) => true,
        (crate::style::FontStyle::Oblique(a), crate::style::FontStyle::Oblique(b)) => {
            same_f32(a, b)
        }
        _ => false,
    };
    let same_variations = a.font_variations.len() == b.font_variations.len()
        && a.font_variations
            .iter()
            .zip(&b.font_variations)
            .all(|(a, b)| a.tag == b.tag && same_f32(a.value, b.value));
    let same_size_adjust = match (a.font_size_adjust, b.font_size_adjust) {
        (None, None) => true,
        (Some(a), Some(b)) => a.metric == b.metric && same_f32(a.value, b.value),
        _ => false,
    };

    a.font_families == b.font_families
        && same_f32(a.font_weight, b.font_weight)
        && same_f32(a.font_width, b.font_width)
        && same_font_style
        && a.lang == b.lang
        && a.font_synthesis == b.font_synthesis
        && same_f32(a.font_size, b.font_size)
        && same_variations
        && a.font_optical_sizing == b.font_optical_sizing
        && same_size_adjust
}

pub(crate) fn resolve(
    fonts: &FontCollection,
    style: &InlineStyle,
    warnings: &mut WarningSink,
) -> StyleMetrics {
    #[cfg(test)]
    resolve_counts::record_resolve();
    let query = FontQuery {
        families: style.font_families.clone(),
        weight: style.font_weight,
        width: style.font_width,
        style: style.font_style,
        language: style.lang.clone(),
        synthesis: style.font_synthesis,
        ..Default::default()
    };
    let mut instance_cache = ProbeInstanceCache::default();
    let space = character_advance(
        fonts,
        style,
        &query,
        ' ',
        1.0,
        &mut instance_cache,
        warnings,
    );
    let ch = character_advance(
        fonts,
        style,
        &query,
        '0',
        0.5,
        &mut instance_cache,
        warnings,
    );
    let ic = character_advance(
        fonts,
        style,
        &query,
        '水',
        1.0,
        &mut instance_cache,
        warnings,
    );
    #[cfg(test)]
    resolve_counts::record_match();
    if let Some(found) = fonts.match_primary(&query)
        && let Some(data) = fonts.font_data(found.id)
    {
        let font = found.id;
        let (instance, size) = instance_cache.resolve(
            fonts,
            found,
            data.data.as_ref(),
            data.index,
            style,
            warnings,
        );
        return StyleMetrics {
            font,
            size,
            space,
            ch,
            ic,
            metrics: fonts
                .metrics_with_coords(font, size, &instance.coords)
                .expect("retained primary face"),
            vertical_metrics: fonts.vertical_metrics(font, size, &instance.coords),
        };
    }
    let font = fonts.primary_font();
    StyleMetrics {
        font,
        size: style.font_size,
        space,
        ch,
        ic,
        metrics: fonts.metrics(font, style.font_size),
        vertical_metrics: None,
    }
}

#[derive(Default)]
struct StyleMetricsCache<'a> {
    entries: HashMap<StyleMetricsKey<'a>, usize>,
}

impl<'a> StyleMetricsCache<'a> {
    fn resolve(
        &mut self,
        fonts: &FontCollection,
        style: &'a InlineStyle,
        resolved: &[StyleMetrics],
        warnings: &mut WarningSink,
    ) -> StyleMetrics {
        let generations = fonts.generations();
        let key = StyleMetricsKey { style, generations };
        match self.entries.entry(key) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                if fonts.generations() == generations {
                    return resolved[*entry.get()];
                }
                resolve(fonts, style, warnings)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let before = warnings.checkpoint();
                let metrics = resolve(fonts, style, warnings);
                let after = warnings.checkpoint();
                if matches!((before, after), (Some(before), Some(after)) if before == after)
                    && fonts.generations() == generations
                {
                    entry.insert(resolved.len());
                }
                metrics
            }
        }
    }
}

pub(crate) fn resolve_styles(
    fonts: &FontCollection,
    styles: &[InlineStyle],
    warnings: &mut WarningSink,
) -> Vec<StyleMetrics> {
    let mut cache = StyleMetricsCache::default();
    let mut resolved = Vec::with_capacity(styles.len());
    for style in styles {
        resolved.push(cache.resolve(fonts, style, &resolved, warnings));
    }
    resolved
}

/// Resolve the character's fallback face and its actual adjusted instance,
/// including explicit variations, optical size and HVAR glyph advances.
fn character_advance(
    fonts: &FontCollection,
    style: &InlineStyle,
    query: &FontQuery,
    ch: char,
    fallback: f32,
    instance_cache: &mut ProbeInstanceCache,
    warnings: &mut WarningSink,
) -> f32 {
    let mut encoded = [0; 4];
    #[cfg(test)]
    resolve_counts::record_match();
    let Some(found) = fonts.match_cluster(query, ch.encode_utf8(&mut encoded)) else {
        return style.font_size * fallback;
    };
    let Some(data) = fonts.font_data(found.id) else {
        return style.font_size * fallback;
    };
    #[cfg(test)]
    crate::font::record_metric_font_ref_open();
    let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
        return style.font_size * fallback;
    };
    let (instance, size) = instance_cache.resolve(
        fonts,
        found,
        data.data.as_ref(),
        data.index,
        style,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        font::{FontFaceDescriptor, FontOptions},
        limits::{Limits, WarningKind},
        style::{
            FontFamily, FontMetricKind, FontSizeAdjust, FontStyle, FontSynthesis, FontVariation,
            GenericFamily, PaintStyle,
        },
        test_support::fonts::LATIN,
    };

    fn fonts() -> FontCollection {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
    }

    fn style() -> InlineStyle {
        InlineStyle {
            font_families: vec![FontFamily::Named("Latin".into())],
            ..Default::default()
        }
    }

    #[test]
    fn warning_free_probes_reuse_the_same_font_instance() {
        let fonts = fonts();
        let mut warnings = WarningSink::default();

        resolve_counts::reset();
        crate::font::reset_metric_font_ref_opens();
        resolve(&fonts, &style(), &mut warnings);

        assert!(warnings.as_slice().is_empty());
        assert_eq!(resolve_counts::snapshot(), (1, 1));
        assert_eq!(resolve_counts::matches(), 4);
        assert_eq!(crate::font::metric_font_ref_opens(), 10);
    }

    #[test]
    fn fallback_face_keeps_a_separate_probe_instance() {
        let fonts = fonts();
        let cjk = fonts
            .register_face(
                crate::test_support::fonts::CJK.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = InlineStyle {
            font_families: vec![
                FontFamily::Named("Latin".into()),
                FontFamily::Named("CJK".into()),
            ],
            ..style()
        };
        let query = FontQuery {
            families: style.font_families.clone(),
            weight: style.font_weight,
            width: style.font_width,
            style: style.font_style,
            language: style.lang.clone(),
            synthesis: style.font_synthesis,
            ..Default::default()
        };

        assert_eq!(fonts.match_cluster(&query, "水").unwrap().id, cjk);
        let primary = fonts.match_primary(&query).unwrap().id;
        let mut warnings = WarningSink::default();
        resolve_counts::reset();
        crate::font::reset_metric_font_ref_opens();
        let metrics = resolve(&fonts, &style, &mut warnings);

        assert_eq!(metrics.font, primary);
        assert_eq!(resolve_counts::snapshot().1, 2);
        assert_eq!(resolve_counts::matches(), 4);
        assert_eq!(crate::font::metric_font_ref_opens(), 9);
        assert!(warnings.as_slice().is_empty());
    }

    fn assert_metrics_eq(actual: StyleMetrics, expected: StyleMetrics) {
        assert_eq!(actual.font, expected.font);
        assert_eq!(actual.size.to_bits(), expected.size.to_bits());
        assert_eq!(actual.metrics, expected.metrics);
        assert_eq!(actual.vertical_metrics, expected.vertical_metrics);
        assert_eq!(actual.space.to_bits(), expected.space.to_bits());
        assert_eq!(actual.ch.to_bits(), expected.ch.to_bits());
        assert_eq!(actual.ic.to_bits(), expected.ic.to_bits());
    }

    #[test]
    fn paint_only_styles_share_metrics_and_keep_the_full_result() {
        let fonts = fonts();
        let base = style();
        let styles: Vec<_> = (0..64)
            .map(|i| InlineStyle {
                paint: PaintStyle {
                    color: [i as u8, 20, 30, 255],
                    ..Default::default()
                },
                ..base.clone()
            })
            .collect();

        let mut expected_warnings = WarningSink::default();
        resolve_counts::reset();
        let expected = resolve(&fonts, &base, &mut expected_warnings);
        let expected_counts = resolve_counts::snapshot();

        let mut actual_warnings = WarningSink::default();
        resolve_counts::reset();
        let actual = resolve_styles(&fonts, &styles, &mut actual_warnings);
        let actual_counts = resolve_counts::snapshot();

        assert_eq!(actual.len(), styles.len());
        assert_eq!(actual_warnings.as_slice(), expected_warnings.as_slice());
        assert_eq!(actual_counts, expected_counts);
        for metrics in actual {
            assert_metrics_eq(metrics, expected);
        }
    }

    #[test]
    fn every_metric_input_keeps_a_distinct_cache_entry() {
        let fonts = fonts();
        let base = style();
        let styles = [
            base.clone(),
            InlineStyle {
                font_families: vec![FontFamily::Generic(GenericFamily::Serif)],
                ..base.clone()
            },
            InlineStyle {
                font_weight: 700.0,
                ..base.clone()
            },
            InlineStyle {
                font_width: 80.0,
                ..base.clone()
            },
            InlineStyle {
                font_style: FontStyle::Italic,
                ..base.clone()
            },
            InlineStyle {
                font_style: FontStyle::Oblique(10.0),
                ..base.clone()
            },
            InlineStyle {
                font_style: FontStyle::Oblique(20.0),
                ..base.clone()
            },
            InlineStyle {
                lang: Some("ja".into()),
                ..base.clone()
            },
            InlineStyle {
                font_synthesis: FontSynthesis {
                    weight: false,
                    ..base.font_synthesis
                },
                ..base.clone()
            },
            InlineStyle {
                font_synthesis: FontSynthesis {
                    style: false,
                    ..base.font_synthesis
                },
                ..base.clone()
            },
            InlineStyle {
                font_synthesis: FontSynthesis {
                    small_caps: false,
                    ..base.font_synthesis
                },
                ..base.clone()
            },
            InlineStyle {
                font_size: 19.0,
                ..base.clone()
            },
            InlineStyle {
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: 650.0,
                }],
                ..base.clone()
            },
            InlineStyle {
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: 700.0,
                }],
                ..base.clone()
            },
            InlineStyle {
                font_variations: vec![FontVariation {
                    tag: *b"wdth",
                    value: 650.0,
                }],
                ..base.clone()
            },
            InlineStyle {
                font_optical_sizing: false,
                ..base.clone()
            },
            InlineStyle {
                font_size_adjust: Some(FontSizeAdjust {
                    metric: FontMetricKind::ExHeight,
                    value: 0.5,
                }),
                ..base.clone()
            },
            InlineStyle {
                font_size_adjust: Some(FontSizeAdjust {
                    metric: FontMetricKind::ExHeight,
                    value: 0.6,
                }),
                ..base.clone()
            },
            InlineStyle {
                font_size_adjust: Some(FontSizeAdjust {
                    metric: FontMetricKind::CapHeight,
                    value: 0.5,
                }),
                ..base
            },
        ];

        let mut warnings = WarningSink::default();
        resolve_counts::reset();
        let metrics = resolve_styles(&fonts, &styles, &mut warnings);

        assert_eq!(metrics.len(), styles.len());
        assert_eq!(resolve_counts::snapshot().0, styles.len());
        assert!(warnings.as_slice().is_empty());
    }

    #[test]
    fn distinct_cache_misses_hash_each_key_once() {
        let fonts = fonts();
        let styles: Vec<_> = (0..3)
            .map(|i| InlineStyle {
                font_size: 17.0 + i as f32,
                ..style()
            })
            .collect();
        let mut warnings = WarningSink::default();

        resolve_counts::reset();
        resolve_styles(&fonts, &styles, &mut warnings);

        assert_eq!(resolve_counts::key_hashes(), styles.len());
    }

    #[test]
    fn colliding_key_hashes_still_compare_exact_metric_inputs() {
        #[derive(Default)]
        struct ConstantHasher;

        impl std::hash::Hasher for ConstantHasher {
            fn finish(&self) -> u64 {
                0
            }

            fn write(&mut self, _: &[u8]) {}
        }

        let first = style();
        let second = InlineStyle {
            font_size: 21.0,
            ..first.clone()
        };
        let generations = (0, None);
        let first_key = StyleMetricsKey {
            style: &first,
            generations,
        };
        let second_key = StyleMetricsKey {
            style: &second,
            generations,
        };
        let mut entries =
            HashMap::with_hasher(std::hash::BuildHasherDefault::<ConstantHasher>::default());
        entries.insert(first_key, 1);
        entries.insert(second_key, 2);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries.get(&first_key), Some(&1));
        assert_eq!(entries.get(&second_key), Some(&2));
    }

    #[test]
    fn font_generation_invalidates_a_cached_metric_result() {
        let fonts = fonts();
        let style = style();
        let mut cache = StyleMetricsCache::default();
        let mut resolved = Vec::new();
        let mut warnings = WarningSink::default();

        resolve_counts::reset();
        let before = cache.resolve(&fonts, &style, &resolved, &mut warnings);
        resolved.push(before);
        let same_generation = cache.resolve(&fonts, &style, &resolved, &mut warnings);
        assert_metrics_eq(same_generation, before);
        assert_eq!(resolve_counts::snapshot().0, 1);

        let replacement = fonts
            .register_face(
                LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let after = cache.resolve(&fonts, &style, &resolved, &mut warnings);

        assert_eq!(after.font, replacement);
        assert_ne!(after.font, before.font);
        assert_eq!(resolve_counts::snapshot().0, 2);
    }

    #[test]
    fn unsupported_size_adjust_preserves_duplicate_and_suppressed_warnings() {
        let fonts = fonts();
        let style = InlineStyle {
            font_size_adjust: Some(FontSizeAdjust {
                metric: FontMetricKind::IcHeight,
                value: 0.5,
            }),
            ..style()
        };
        let styles = [style.clone(), style];

        let mut expected_warnings = WarningSink::new(Some(2));
        for style in &styles {
            resolve(&fonts, style, &mut expected_warnings);
        }
        let expected = expected_warnings.as_slice().to_vec();
        assert!(
            expected
                .iter()
                .any(|warning| warning.kind == WarningKind::Unsupported)
        );
        assert!(
            expected
                .iter()
                .any(|warning| warning.kind == WarningKind::Suppressed)
        );
        assert!(expected_warnings.is_suppressed());

        let mut actual_warnings = WarningSink::new(Some(2));
        resolve_counts::reset();
        let actual = resolve_styles(&fonts, &styles, &mut actual_warnings);

        assert_eq!(actual.len(), 2);
        assert_eq!(actual_warnings.as_slice(), expected.as_slice());
        assert!(actual_warnings.is_suppressed());
        assert_eq!(resolve_counts::snapshot().0, 2);
        assert_eq!(resolve_counts::snapshot().1, 6);
    }
}
