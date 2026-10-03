//! Size and variation-dependent metrics, including CSS ch/ic units.

use super::{FontCollection, FontData, FontId, FontMetrics, FontQuery, NormalizedCoord};
use crate::geometry::{LayoutUnit, Saturation};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    raw::{TableProvider, types::Tag},
};

/// A CSS unit's advance in pixels and the face which actually supplies it.
/// `id` is None when the CSS fallback advance is used.
/// A selected character's advance uses the same 1/64px rounding as shaping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontUnit {
    pub id: Option<FontId>,
    pub advance: f32,
}

struct UnitEntry {
    query: FontQuery,
    size: f32,
    ch: char,
    result: FontUnit,
    used: std::sync::atomic::AtomicU64,
}

/// Per-layer LRU of final CSS unit values. Each result includes the selected
/// face, so both a missing glyph and a fallback face require generation-based
/// invalidation. The small cap keeps linear lookup cheap and bounds retained
/// queries independently of the larger cluster-match cache.
#[derive(Default)]
pub(super) struct UnitCache {
    slots: Vec<UnitEntry>,
    clock: std::sync::atomic::AtomicU64,
    generations: Option<(u64, Option<u64>)>,
}

impl UnitCache {
    #[cfg(test)]
    fn len(&self) -> usize {
        self.slots.len()
    }

    fn sync(&mut self, generations: (u64, Option<u64>)) {
        if self.generations != Some(generations) {
            self.slots.clear();
            self.generations = Some(generations);
        }
    }

    fn get(
        &self,
        generations: (u64, Option<u64>),
        query: &FontQuery,
        size: f32,
        ch: char,
    ) -> Option<FontUnit> {
        if self.generations != Some(generations) {
            return None;
        }
        let entry = self
            .slots
            .iter()
            .find(|entry| entry.size == size && entry.ch == ch && entry.query == *query)?;
        let stamp = self
            .clock
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        entry
            .used
            .fetch_max(stamp, std::sync::atomic::Ordering::Relaxed);
        Some(entry.result)
    }

    fn insert(
        &mut self,
        generations: (u64, Option<u64>),
        query: &FontQuery,
        size: f32,
        ch: char,
        result: FontUnit,
        cap: usize,
    ) {
        self.sync(generations);
        if cap == 0 || query.families.len() > 128 {
            return;
        }
        let key_bytes = query.families.iter().fold(
            query.language.as_ref().map_or(0, String::len),
            |bytes, family| {
                bytes.saturating_add(match family {
                    crate::style::FontFamily::Named(name) => name.len(),
                    _ => 0,
                })
            },
        );
        if key_bytes > 4096 {
            return;
        }
        let stamp = self
            .clock
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        let entry = UnitEntry {
            query: query.clone(),
            size,
            ch,
            result,
            used: std::sync::atomic::AtomicU64::new(stamp),
        };
        if let Some(existing) = self
            .slots
            .iter_mut()
            .find(|old| old.size == size && old.ch == ch && old.query == *query)
        {
            *existing = entry;
        } else if self.slots.len() < cap {
            self.slots.push(entry);
        } else if let Some((oldest, _)) = self
            .slots
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.used.load(std::sync::atomic::Ordering::Relaxed))
        {
            self.slots[oldest] = entry;
        }
    }
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
        #[cfg(test)]
        super::record_metric_font_ref_open();
        let font = FontRef::from_index(data.data.as_ref(), data.index).ok()?;
        Some(horizontal_metrics_from_font(&font, size, coords))
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
        #[cfg(test)]
        super::record_metric_font_ref_open();
        let font = FontRef::from_index(data.data.as_ref(), data.index).ok()?;
        vertical_metrics_from_font(&font, size, coords)
    }

    /// Resolve both metric sets from the caller's already acquired face.
    pub(crate) fn metrics_from_data(
        &self,
        id: FontId,
        data: &FontData,
        size: f32,
        coords: &[NormalizedCoord],
    ) -> (Option<FontMetrics>, Option<VerticalFontMetrics>) {
        let Some(size) = valid_size(size) else {
            return (None, None);
        };
        #[cfg(test)]
        super::record_metric_font_ref_open();
        let font = FontRef::from_index(data.data.as_ref(), data.index).ok();
        let horizontal = if id.layer == self.root().layer.id && id.index == 0 {
            Some(stub_metrics(size))
        } else {
            font.as_ref()
                .map(|font| horizontal_metrics_from_font(font, size, coords))
        };
        let vertical = font
            .as_ref()
            .and_then(|font| vertical_metrics_from_font(font, size, coords));
        (horizontal, vertical)
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
        let generations = self.generations();
        if let Some(unit) = self.caches().units.get(generations, query, size, ch) {
            return unit;
        }
        let unit = self.resolve_unit_uncached(query, size, ch, fallback);
        // A registration or fallback change during shaping must not make an
        // old value available to calls that observe the new generation.
        if self.generations() == generations {
            let mut caches = self.write_caches();
            let cap = self.layer.match_cache_entries.min(64);
            caches.units.insert(generations, query, size, ch, unit, cap);
        }
        unit
    }

    pub(super) fn resolve_unit_uncached(
        &self,
        query: &FontQuery,
        size: f32,
        ch: char,
        fallback: f32,
    ) -> FontUnit {
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
        #[cfg(test)]
        super::record_metric_font_ref_open();
        let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
            return missing;
        };
        if font.charmap().map(ch).is_none() {
            return missing;
        }
        let Ok(shape_font) = harfrust::FontRef::from_index(data.data.as_ref(), data.index) else {
            return missing;
        };
        let Some(shaper_data) = self.shaper_data(found.id) else {
            return missing;
        };
        let instance = harfrust::ShaperInstance::from_variations(
            &shape_font,
            found.variations.iter().map(|v| harfrust::Variation {
                tag: harfrust::Tag::new(&v.tag),
                value: v.value,
            }),
        );
        let shaper = shaper_data
            .shaper(&shape_font)
            .instance(Some(&instance))
            .build();
        let upem = shaper.units_per_em();
        if upem == 0 {
            return missing;
        }
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str(ch.encode_utf8(&mut encoded));
        buffer.set_direction(harfrust::Direction::LeftToRight);
        buffer.set_script(
            harfrust::Script::from_iso15924_tag(harfrust::Tag::new(&query.script))
                .unwrap_or(harfrust::script::UNKNOWN),
        );
        if let Some(language) = query.language.as_ref().and_then(|l| l.parse().ok()) {
            buffer.set_language(language);
        }
        let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
        if shaped.glyph_positions().is_empty() {
            return missing;
        }
        let mut sat = Saturation::default();
        let scale = size / upem as f32;
        let advance = shaped
            .glyph_positions()
            .iter()
            .fold(LayoutUnit::ZERO, |sum, pos| {
                sum.add(
                    LayoutUnit::from_f32_round(pos.x_advance as f32 * scale, &mut sat),
                    &mut sat,
                )
            });
        FontUnit {
            id: Some(found.id),
            advance: advance.to_f32(),
        }
    }
}
fn horizontal_metrics_from_font(
    font: &FontRef<'_>,
    size: f32,
    coords: &[NormalizedCoord],
) -> FontMetrics {
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
    FontMetrics {
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
    }
}

fn vertical_metrics_from_font(
    font: &FontRef<'_>,
    size: f32,
    coords: &[NormalizedCoord],
) -> Option<VerticalFontMetrics> {
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
    fn cached_units_match_uncached_shaping_for_query_size_and_character() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        for (family, width) in [("First", 600), ("Second", 800)] {
            fonts
                .register_face(
                    crate::font::browser_tests::test_font(family, &['0', '水'], width),
                    0,
                    FontFaceDescriptor {
                        family: family.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let base = FontQuery {
            families: vec![FontFamily::Named("First".into())],
            ..Default::default()
        };
        let mut queries = vec![base.clone()];
        queries.push(FontQuery {
            families: vec![FontFamily::Named("Second".into())],
            ..base.clone()
        });
        queries.push(FontQuery {
            weight: 650.,
            ..base.clone()
        });
        queries.push(FontQuery {
            script: *b"Hani",
            language: Some("ja".into()),
            ..base.clone()
        });
        for query in &queries {
            for size in [0., 16., 16.5, 22., f32::NAN, f32::INFINITY] {
                for (ch, fallback) in [('0', 0.5), ('水', 1.)] {
                    let expected = fonts.resolve_unit_uncached(query, size, ch, fallback);
                    assert_eq!(fonts.resolve_unit(query, size, ch, fallback), expected);
                    assert_eq!(fonts.resolve_unit(query, size, ch, fallback), expected);
                }
            }
        }
        assert!(fonts.caches().units.len() > 0);
    }

    #[test]
    fn unit_cache_is_bounded_optional_and_invalidated_by_both_layers() {
        let limits = Limits::default();
        let shared = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                match_cache_entries: 2,
                ..Default::default()
            },
        );
        let document = FontCollection::for_document(&shared, &limits);
        let query = FontQuery {
            families: vec![FontFamily::Named("Later".into())],
            ..Default::default()
        };
        assert_eq!(document.resolve_ch(&query, 10.).id, None);
        let shared_id = shared
            .register_face(
                crate::font::browser_tests::test_font("Later", &['0'], 600),
                0,
                FontFaceDescriptor {
                    family: "Later".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(document.resolve_ch(&query, 10.).id, Some(shared_id));
        let local_id = document
            .register_face(
                crate::font::browser_tests::test_font("Later", &['0'], 800),
                0,
                FontFaceDescriptor {
                    family: "Later".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(document.resolve_ch(&query, 10.).id, Some(local_id));
        for size in [10., 11., 12., 13.] {
            assert_eq!(
                document.resolve_ch(&query, size),
                document.resolve_unit_uncached(&query, size, '0', 0.5)
            );
            assert!(document.caches().units.len() <= 2);
        }
        let disabled = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                match_cache_entries: 0,
                ..Default::default()
            },
        );
        disabled.resolve_ch(&query, 10.);
        assert_eq!(disabled.caches().units.len(), 0);
    }

    #[test]
    fn unit_cache_recovers_from_poison_and_concurrent_registration() {
        use std::sync::{Arc, Barrier};
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let query = FontQuery {
            families: vec![FontFamily::Named("Concurrent".into())],
            ..Default::default()
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = fonts.state();
            panic!("poison font layer for recovery test");
        }));
        assert_eq!(fonts.resolve_ch(&query, 16.).id, None);
        let barrier = Arc::new(Barrier::new(5));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let fonts = fonts.clone();
                let query = query.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..100 {
                        let unit = fonts.resolve_ch(&query, 16.);
                        assert!(unit.id.is_none() || unit.advance == 9.59375);
                    }
                });
            }
            barrier.wait();
            fonts
                .register_face(
                    crate::font::browser_tests::test_font("Concurrent", &['0'], 600),
                    0,
                    FontFaceDescriptor {
                        family: "Concurrent".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        });
        assert_eq!(
            fonts.resolve_ch(&query, 16.),
            fonts.resolve_unit_uncached(&query, 16., '0', 0.5)
        );
        assert!(fonts.resolve_ch(&query, 16.).id.is_some());
    }

    fn mvar_font(base: &[u8], records: &[([u8; 4], i16)]) -> Vec<u8> {
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
        crate::font::sfnt::build_sfnt(&tables)
    }

    #[test]
    fn variation_and_decoration_metrics_match_run_instance() {
        let records = [
            (*b"cpht", 30i16),
            (*b"hasc", 100),
            (*b"hdsc", -50),
            (*b"sbyo", 20),
            (*b"spyo", 30),
            (*b"xhgt", 25),
        ];
        let bytes = mvar_font(crate::test_support::fonts::LATIN, &records);
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

    #[test]
    fn resolved_run_reuses_its_acquired_font_data_for_metrics() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::font::browser_tests::test_font("Reuse", &['a'], 500),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Reuse".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named("Reuse".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = crate::LayoutContext::new();
        let mut builder = crate::ParagraphBuilder::new(&style, &limits);
        builder.push_text(
            crate::node::TextSource::Dom {
                node: crate::node::NodeId(1),
                offset: 7,
            },
            "a",
        );
        let p = builder.build(&mut cx, &fonts).unwrap();
        assert_eq!(p.data.shape_items.len(), 1);
        crate::font::FONT_DATA_ACQUISITIONS.with(|count| count.set(0));
        let mut warnings = crate::limits::WarningSink::default();
        let (glyphs, runs) = crate::shape::shape_items(
            &mut cx,
            &p.data.shape_items,
            &p.data.styles,
            &fonts,
            style.writing_mode,
            &limits,
            &mut warnings,
            &mut crate::geometry::Saturation::default(),
        )
        .unwrap();
        assert!(warnings.take().is_empty());
        assert_eq!(glyphs.id, [1]);
        assert_eq!(glyphs.cluster, [0]);
        assert_eq!(glyphs.advance[0].to_f32(), 8.0);
        assert_eq!(runs.len(), 1);
        let m = runs[0].instance.metrics.unwrap();
        for (actual, expected) in [(m.ascent, 12.0), (m.descent, 4.0), (m.line_gap, 1.6)] {
            assert!((actual - expected).abs() < 1.0 / 65536.0);
        }
        assert_eq!(
            crate::font::FONT_DATA_ACQUISITIONS.with(|count| count.get()),
            1,
            "a shaped run must reuse its acquired font data for metrics"
        );
    }

    #[test]
    fn acquired_data_metrics_preserve_root_stub_document_face_and_invalid_sizes() {
        let limits = Limits::default();
        let shared = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let stub_id = shared.primary_font();
        let stub_data = shared.font_data(stub_id).unwrap();
        let (stub, vertical) = shared.metrics_from_data(stub_id, &stub_data, 10.0, &[]);
        assert_eq!(stub.unwrap().ascent, 8.0);
        assert!(vertical.is_none());
        let doc = FontCollection::for_document(&shared, &limits);
        let id = doc
            .register(crate::font::browser_tests::test_font(
                "Metrics",
                &['a'],
                500,
            ))
            .unwrap();
        assert_eq!(id.index(), 0);
        let data = doc.font_data(id).unwrap();
        let (metrics, vertical) = doc.metrics_from_data(id, &data, 10.0, &[]);
        let m = metrics.unwrap();
        assert_eq!((m.ascent, m.descent, m.line_gap), (7.5, 2.5, 1.0));
        assert_eq!(Some(m), doc.metrics_with_coords(id, 10.0, &[]));
        assert_eq!(vertical, doc.vertical_metrics(id, 10.0, &[]));
        for size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(doc.metrics_from_data(id, &data, size, &[]), (None, None));
            assert!(doc.metrics_with_coords(id, size, &[]).is_none());
            assert!(doc.vertical_metrics(id, size, &[]).is_none());
        }
        for size in [0.0, 1e7] {
            let (h, v) = doc.metrics_from_data(id, &data, size, &[]);
            assert_eq!(h, doc.metrics_with_coords(id, size, &[]));
            assert_eq!(v, doc.vertical_metrics(id, size, &[]));
        }
        let absent = FontId {
            layer: id.layer,
            index: u32::MAX,
        };
        assert!(doc.metrics_with_coords(absent, 10.0, &[]).is_none());
        assert!(doc.vertical_metrics(absent, 10.0, &[]).is_none());
        let old = m;
        doc.register(crate::font::browser_tests::test_font("Other", &['a'], 600))
            .unwrap();
        assert_eq!(doc.metrics_from_data(id, &data, 10.0, &[]).0, Some(old));
        assert_eq!(data.data.as_ref(), doc.font_data(id).unwrap().data.as_ref());
    }

    #[test]
    fn acquired_vertical_metrics_preserve_literal_mvar_deltas_in_public_runs() {
        let bytes = mvar_font(
            crate::test_support::fonts::CJK,
            &[(*b"vasc", 20), (*b"vdsc", -30), (*b"vlgp", 10)],
        );
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let id = fonts
            .register_face(
                bytes,
                0,
                FontFaceDescriptor {
                    family: "VerticalVariable".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let data = fonts.font_data(id).unwrap();
        assert_eq!(
            FontRef::from_index(data.data.as_ref(), 0)
                .unwrap()
                .head()
                .unwrap()
                .units_per_em(),
            1000
        );
        let nominal = fonts.vertical_metrics(id, 20.0, &[]).unwrap();
        let coords = [NormalizedCoord::from_bits(16384)];
        let (horizontal, varied) = fonts.metrics_from_data(id, &data, 20.0, &coords);
        assert_eq!(horizontal, fonts.metrics_with_coords(id, 20.0, &coords));
        let varied = varied.unwrap();
        assert_eq!(Some(varied), fonts.vertical_metrics(id, 20.0, &coords));
        for (actual, expected) in [
            (varied.ascent - nominal.ascent, 0.4),
            (varied.descent - nominal.descent, 0.6),
            (varied.line_gap - nominal.line_gap, 0.2),
        ] {
            assert!(
                (actual - expected).abs() < 1.0 / 32768.0,
                "{actual} vs {expected}"
            );
        }
        let style = ParagraphStyle {
            writing_mode: crate::geometry::WritingMode::VerticalRl,
            root: InlineStyle {
                font_families: vec![FontFamily::Named("VerticalVariable".into())],
                font_size: 20.0,
                text_orientation: crate::style::TextOrientation::Upright,
                font_variations: vec![FontVariation {
                    tag: *b"wght",
                    value: 900.0,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "水");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts).unwrap();
        assert!(p.warnings().is_empty());
        let lines = p.break_all(&mut cx, &Default::default(), 100.0, &AtomicSizes::EMPTY);
        let run = lines[0]
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(run.normalized_coords(), coords);
        assert_eq!(run.vertical_metrics(), Some(varied));
    }
}
