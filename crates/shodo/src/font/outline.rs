//! Bounded glyph ink reuse for overflow queries. No font blobs, paragraphs,
//! lines or renderer transforms are retained by this cache.
use super::{FontCollection, FontId, NormalizedCoord};
use skrifa::metrics::BoundingBox;
use std::collections::{HashMap, VecDeque};

const INSTANCES: usize = 16;
const GLYPHS: usize = 1024;
const COORDS: usize = 64;

#[cfg(test)]
thread_local! {
    pub(crate) static BYPASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct GlyphBounds {
    values: HashMap<u32, Option<BoundingBox>, foldhash::fast::RandomState>,
    enabled: bool,
}
impl Default for GlyphBounds {
    fn default() -> Self {
        Self {
            values: HashMap::default(),
            enabled: true,
        }
    }
}
impl GlyphBounds {
    pub(crate) fn get_or_compute(
        &mut self,
        glyph: u32,
        compute: impl FnOnce() -> Option<BoundingBox>,
    ) -> Option<BoundingBox> {
        if !self.enabled {
            return compute();
        }
        if let Some(bounds) = self.values.get(&glyph) {
            return *bounds;
        }
        let bounds = compute();
        // Keep the table allocation bounded even during a single giant run.
        // Clearing at capacity needs neither per-hit recency writes nor a
        // second eviction index; misses always recompute the original value.
        if self.values.len() == GLYPHS {
            self.values.clear();
        }
        self.values.insert(glyph, bounds);
        bounds
    }
}

struct Instance {
    font: FontId,
    size: u32,
    coords: Box<[NormalizedCoord]>,
    glyphs: GlyphBounds,
}

#[derive(Default)]
pub(super) struct Cache {
    instances: VecDeque<Instance>,
}
impl Cache {
    fn with_instance<R>(
        &mut self,
        font: FontId,
        size: f32,
        coords: &[NormalizedCoord],
        query: impl FnOnce(&mut GlyphBounds) -> R,
    ) -> R {
        let eligible = coords.len() <= COORDS;
        #[cfg(test)]
        let eligible = eligible && !BYPASS.with(std::cell::Cell::get);
        if !eligible {
            return query(&mut GlyphBounds {
                enabled: false,
                ..Default::default()
            });
        }
        let index = if let Some(index) = self.instances.iter().position(|entry| {
            entry.font == font && entry.size == size.to_bits() && *entry.coords == *coords
        }) {
            index
        } else {
            if self.instances.len() == INSTANCES {
                self.instances.pop_front();
            }
            self.instances.push_back(Instance {
                font,
                size: size.to_bits(),
                coords: coords.into(),
                glyphs: GlyphBounds::default(),
            });
            self.instances.len() - 1
        };
        query(&mut self.instances[index].glyphs)
    }
}

impl FontCollection {
    /// Route shared faces to their owning layer so separate documents reuse
    /// application fonts. Callers acquire font data before taking this guard;
    /// the callback never acquires catalog/cache locks or recurses into ruby.
    pub(crate) fn with_outline_bounds<R>(
        &self,
        font: FontId,
        size: f32,
        coords: &[NormalizedCoord],
        query: impl FnOnce(&mut GlyphBounds) -> R,
    ) -> R {
        if font.layer != self.layer.id {
            if let Some(parent) = &self.layer.parent {
                return parent.with_outline_bounds(font, size, coords, query);
            }
            return query(&mut GlyphBounds {
                enabled: false,
                ..Default::default()
            });
        }
        self.layer
            .outline_bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .with_instance(font, size, coords, query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skrifa::{
        MetadataProvider,
        instance::{LocationRef, Size},
        raw::types::GlyphId,
    };
    use std::cell::Cell;

    #[test]
    fn missing_font_reference_resolves_fallback_metrics_once_per_run() {
        use crate::limits::Limits;
        use crate::node::{NodeId, TextSource};
        use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
        let fonts = crate::test_support::fonts::collection(&Limits::default());
        let id = fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                super::super::FontFaceDescriptor {
                    family: "Fallback".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Fallback".into())],
                font_size: 16.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "aaaa");
        let mut cx = LayoutContext::new();
        let p = builder.build(&mut cx, &fonts).unwrap();
        let LineResult::Line(mut line) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("line")
        };
        drop(p);
        drop(cx);
        // Exercise the defensive unavailable-FontRef path without weakening
        // public registration validation. The original shaped glyphs remain.
        let data = std::sync::Arc::get_mut(&mut line.data).unwrap();
        std::sync::Arc::make_mut(&mut data.runs[0].instance).metrics = None;
        fonts.state().faces[id.index() as usize] =
            super::super::FontData::new(super::super::Blob::from(vec![0]), 0);
        super::super::FONT_DATA_ACQUISITIONS.with(|n| n.set(0));
        let rect = line.overflow_rect();
        assert_eq!(
            super::super::FONT_DATA_ACQUISITIONS.with(Cell::get),
            2,
            "one run font lookup and one fallback metrics lookup, outside the glyph loop"
        );
        assert_eq!(rect.inline_start.to_bits(), 0.0f32.to_bits());
        assert_eq!(rect.inline_size.to_bits(), line.inline_size().to_bits());
        assert_eq!(
            rect.block_start.to_bits(),
            (line.baseline(crate::geometry::BaselineKind::Alphabetic) - 12.8).to_bits()
        );
        assert_eq!(rect.block_size.to_bits(), 16.0f32.to_bits());
    }

    fn lookup(
        cache: &mut Cache,
        font: FontId,
        size: f32,
        coords: &[NormalizedCoord],
        glyph: u32,
        count: &Cell<usize>,
    ) -> Option<BoundingBox> {
        let face = skrifa::FontRef::new(crate::test_support::fonts::LATIN).unwrap();
        let metrics = face.glyph_metrics(Size::new(size), LocationRef::new(coords));
        cache.with_instance(font, size, coords, |bounds| {
            bounds.get_or_compute(glyph, || {
                count.set(count.get() + 1);
                metrics.bounds(GlyphId::new(glyph))
            })
        })
    }

    #[test]
    fn absent_outlines_are_retained() {
        let mut cache = Cache::default();
        let count = Cell::new(0);
        let font = FontId { layer: 1, index: 1 };
        for _ in 0..8 {
            assert!(lookup(&mut cache, font, 16.0, &[], u32::MAX, &count).is_none());
        }
        assert_eq!(count.get(), 1, "an invalid glyph is negative-cached");
    }

    #[test]
    fn font_size_bits_full_coordinates_and_glyph_select_distinct_entries() {
        let mut cache = Cache::default();
        let count = Cell::new(0);
        let font = FontId { layer: 1, index: 1 };
        let a = [
            NormalizedCoord::from_bits(100),
            NormalizedCoord::from_bits(200),
        ];
        let b = [
            NormalizedCoord::from_bits(100),
            NormalizedCoord::from_bits(201),
        ];
        let cases = [
            (font, 16.0, &a[..], 1),
            (FontId { layer: 2, index: 1 }, 16.0, &a[..], 1),
            (FontId { layer: 1, index: 2 }, 16.0, &a[..], 1),
            (font, f32::from_bits(16.0f32.to_bits() + 1), &a[..], 1),
            (font, 16.0, &b[..], 1),
            (font, 16.0, &a[..1], 1),
            (font, 16.0, &a[..], 2),
            (font, 0.0, &a[..], 1),
            (font, -0.0, &a[..], 1),
        ];
        for _ in 0..2 {
            for (font, size, coords, glyph) in cases {
                let actual = lookup(&mut cache, font, size, coords, glyph, &count);
                let face = skrifa::FontRef::new(crate::test_support::fonts::LATIN).unwrap();
                let expected = face
                    .glyph_metrics(Size::new(size), LocationRef::new(coords))
                    .bounds(GlyphId::new(glyph));
                assert_eq!(
                    actual.map(|b| [
                        b.x_min.to_bits(),
                        b.y_min.to_bits(),
                        b.x_max.to_bits(),
                        b.y_max.to_bits()
                    ]),
                    expected.map(|b| [
                        b.x_min.to_bits(),
                        b.y_min.to_bits(),
                        b.x_max.to_bits(),
                        b.y_max.to_bits()
                    ])
                );
            }
        }
        assert_eq!(count.get(), cases.len());
    }

    #[test]
    fn caps_evict_and_oversized_coordinates_bypass_retention() {
        let mut cache = Cache::default();
        let count = Cell::new(0);
        let font = FontId { layer: 1, index: 1 };
        for glyph in 0..=GLYPHS as u32 {
            lookup(&mut cache, font, 16.0, &[], glyph, &count);
            assert!(cache.instances[0].glyphs.values.len() <= GLYPHS);
            assert!(cache.instances[0].glyphs.values.capacity() <= GLYPHS * 2);
        }
        let before = count.get();
        lookup(&mut cache, font, 16.0, &[], 0, &count);
        assert_eq!(
            count.get(),
            before + 1,
            "old glyphs recompute after eviction"
        );
        for size in 17..=17 + INSTANCES {
            lookup(&mut cache, font, size as f32, &[], 1, &count);
            assert!(cache.instances.len() <= INSTANCES);
        }
        let before = count.get();
        lookup(&mut cache, font, 16.0, &[], 0, &count);
        assert_eq!(
            count.get(),
            before + 1,
            "old instances recompute after eviction"
        );
        let coords = vec![NormalizedCoord::default(); COORDS + 1];
        let before = count.get();
        for _ in 0..2 {
            lookup(&mut cache, font, 16.0, &coords, 1, &count);
        }
        assert_eq!(count.get(), before + 2);
        assert!(
            cache
                .instances
                .iter()
                .all(|entry| entry.coords.len() <= COORDS)
        );
    }
}
