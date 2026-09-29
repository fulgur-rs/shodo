//! Size and variation-dependent metrics, including CSS ch/ic units.

use super::{FontCollection, FontId, FontMetrics, FontQuery, NormalizedCoord};
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
    used: u64,
}

/// Per-layer LRU of final CSS unit values. Each result includes the selected
/// face, so both a missing glyph and a fallback face require generation-based
/// invalidation. The small cap keeps linear lookup cheap and bounds retained
/// queries independently of the larger cluster-match cache.
#[derive(Default)]
pub(super) struct UnitCache {
    slots: Vec<UnitEntry>,
    clock: u64,
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
        &mut self,
        generations: (u64, Option<u64>),
        query: &FontQuery,
        size: f32,
        ch: char,
    ) -> Option<FontUnit> {
        self.sync(generations);
        let entry = self
            .slots
            .iter_mut()
            .find(|entry| entry.size == size && entry.ch == ch && entry.query == *query)?;
        self.clock = self.clock.wrapping_add(1);
        entry.used = self.clock;
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
        self.clock = self.clock.wrapping_add(1);
        let entry = UnitEntry {
            query: query.clone(),
            size,
            ch,
            result,
            used: self.clock,
        };
        if let Some(existing) = self
            .slots
            .iter_mut()
            .find(|old| old.size == size && old.ch == ch && old.query == *query)
        {
            *existing = entry;
        } else if self.slots.len() < cap {
            self.slots.push(entry);
        } else if let Some((oldest, _)) = self.slots.iter().enumerate().min_by_key(|(_, e)| e.used)
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
        let generations = self.generations();
        if let Some(unit) = self.state().units.get(generations, query, size, ch) {
            return unit;
        }
        let unit = self.resolve_unit_uncached(query, size, ch, fallback);
        // A registration or fallback change during shaping must not make an
        // old value available to calls that observe the new generation.
        if self.generations() == generations {
            let mut state = self.state();
            let cap = state.options.match_cache_entries.min(64);
            state.units.insert(generations, query, size, ch, unit, cap);
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
        assert!(fonts.state().units.len() > 0);
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
            assert!(document.state().units.len() <= 2);
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
        assert_eq!(disabled.state().units.len(), 0);
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

    #[test]
    fn variation_and_decoration_metrics_match_run_instance() {
        let base = crate::test_support::fonts::LATIN;
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
