//! Glyph placement, transforms, and shaping-cluster iteration.

use peniko::FontData;

use super::{
    Cluster, ClusterFlags, FontId, Glyph, GlyphRunView, GlyphSource, GlyphTransform, Glyphs,
    LayoutUnit, NodeId,
};
use std::ops::Range;

impl ClusterFlags {
    fn from_source(source: &str, synthetic_hyphen: bool) -> Self {
        use icu_properties::{
            CodePointMapData,
            props::{GeneralCategory, GeneralCategoryGroup},
        };
        let categories = CodePointMapData::<GeneralCategory>::new();
        let punctuation = |ch| GeneralCategoryGroup::Punctuation.contains(categories.get(ch));
        let excluded = |ch| {
            let category = categories.get(ch);
            if GeneralCategoryGroup::Separator.contains(category)
                || matches!(
                    category,
                    GeneralCategory::Control
                        | GeneralCategory::Format
                        | GeneralCategory::Unassigned
                )
            {
                return true;
            }
            if !punctuation(ch) {
                return false;
            }
            // CSS Text Decoration 3 keeps emphasis on punctuation whose
            // compatibility decomposition contains one of these symbols.
            !icu_normalizer::DecomposingNormalizer::new_nfkd()
                .normalize_iter(std::iter::once(ch))
                .any(|c| {
                    matches!(
                        c,
                        '#' | '%'
                            | '\u{2030}'
                            | '\u{2031}'
                            | '\u{066a}'
                            | '\u{0609}'
                            | '\u{060a}'
                            | '&'
                            | '\u{204a}'
                            | '@'
                            | '\u{00a7}'
                            | '\u{00b6}'
                            | '\u{204b}'
                            | '\u{2053}'
                            | '\u{303d}'
                    )
                })
        };
        let first = source.chars().next();
        Self {
            whitespace: first.is_some_and(char::is_whitespace),
            punctuation: first.is_some_and(punctuation),
            synthetic_hyphen,
            emphasis_excluded: synthetic_hyphen || source.chars().all(excluded),
        }
    }
}

impl<'a> GlyphRunView<'a> {
    /// Retained paint of the first source scalar owning this run's glyphs.
    /// Shared clusters are drawn once with this color; source decorations use
    /// separate source geometry. The accepted first-line style is retained.
    pub fn paint_style(&self) -> &'a crate::style::PaintStyle {
        &self.data().styles[self.style_index() as usize].paint
    }

    pub fn style_index(&self) -> u32 {
        self.data().items[self.item as usize].style
    }

    /// Paint owner of this run: the source item's [`NodeId`], normally supplied
    /// by [`crate::node::TextSource`], rather than its enclosing inline box.
    /// A cluster shared across nodes belongs to the item supplying its first
    /// scalar. Iterate the returned glyphs once; do not repaint them for every
    /// source node overlapping [`Self::text_range`]. [`Self::paint_style`]
    /// supplies resolved solid paint; other effects remain caller-owned. Use the line's offset mapping and [`crate::hit::LineLayout`] for
    /// each source node's selection, link, decoration and caret regions.
    pub fn node(&self) -> Option<NodeId> {
        self.data().items[self.item as usize].node
    }

    /// Caller source of this run's first processed scalar. A DOM source's
    /// offset is a UTF-8 byte offset within its text node, with collapsing and
    /// text transformation resolved. Shared clusters have one paint owner;
    /// use [`crate::Line::owners`] for every contributing DOM node and range.
    ///
    /// Returns `None` if offset mapping was disabled when building the
    /// paragraph, or this run has no caller-owned source. [`Self::node`] is
    /// available independently of offset mapping.
    pub fn source(&self) -> Option<crate::node::TextSource> {
        use crate::mapping::{Affinity, TextOrigin};
        use crate::node::TextSource;
        let node = self.node()?;
        match self
            .line
            .offset_mapping()?
            .text_to_dom(self.text.0, Affinity::Downstream)?
        {
            TextOrigin::Dom {
                node: owner,
                offset,
            } if owner == node => Some(TextSource::Dom { node, offset }),
            TextOrigin::Generated { node: owner } if owner == node => {
                Some(TextSource::Generated { node })
            }
            _ => None,
        }
    }

    fn run_data(&self) -> &'a crate::shape::ShapedRun {
        match self.source {
            GlyphSource::Overlay { run: Some(run), .. } => &self.line.overlay_runs[run as usize],
            _ => &self.data().runs[self.run as usize],
        }
    }

    pub fn font(&self) -> FontId {
        self.run_data().font
    }

    pub fn orientation(&self) -> crate::GlyphOrientation {
        self.run_data().orientation
    }

    /// Maps an outline's local x/y axes into logical axes. Apply the
    /// [`crate::geometry::PhysicalConverter`] to this displacement after
    /// positioning the glyph origin, so RTL never mirrors the outline.
    pub fn glyph_transform(&self) -> GlyphTransform {
        use crate::GlyphOrientation as O;
        use crate::geometry::{Direction, WritingMode};
        let mode = self.data().style.writing_mode;
        let ltr = self.line.used_direction() == Direction::Ltr;
        let inline_sign = if mode == WritingMode::SidewaysLr {
            if ltr { -1.0 } else { 1.0 }
        } else if ltr {
            1.0
        } else {
            -1.0
        };
        let block_sign = if matches!(mode, WritingMode::VerticalRl | WritingMode::SidewaysRl) {
            -1.0
        } else {
            1.0
        };
        match self.orientation() {
            O::Horizontal => GlyphTransform {
                inline_x: inline_sign,
                inline_y: 0.0,
                block_x: 0.0,
                block_y: 1.0,
            },
            O::Upright => GlyphTransform {
                inline_x: 0.0,
                inline_y: inline_sign,
                block_x: block_sign,
                block_y: 0.0,
            },
            O::Combined => {
                let index = self
                    .data()
                    .combine_spans
                    .partition_point(|span| span.text.end <= self.text.0);
                GlyphTransform {
                    inline_x: 0.0,
                    inline_y: inline_sign,
                    block_x: block_sign * self.data().combine_geometry.scales[index],
                    block_y: 0.0,
                }
            }
            O::SidewaysClockwise => GlyphTransform {
                inline_x: inline_sign,
                inline_y: 0.0,
                block_x: 0.0,
                block_y: -block_sign,
            },
            O::SidewaysCounterClockwise => GlyphTransform {
                inline_x: -inline_sign,
                inline_y: 0.0,
                block_x: 0.0,
                block_y: block_sign,
            },
        }
    }

    /// Font outline origin in line-local logical coordinates. RTL flow uses
    /// the advance cell's inline end, including the original shaping advance
    /// and excluding layout spacing. Rotation is applied by `glyph_transform`.
    pub fn glyph_origin(&self, index: usize) -> Option<(f32, f32)> {
        let gi = (self.glyphs.0 as usize).checked_add(index)?;
        if gi >= self.glyphs.1 as usize {
            return None;
        }
        let glyph = self.glyphs().get(index)?;
        let store = match self.source {
            GlyphSource::Shared => &self.data().glyphs,
            GlyphSource::Overlay { .. } => self.line.overlay.as_deref()?,
        };
        let ltr = self.line.used_direction() == crate::geometry::Direction::Ltr;
        let negative_inline = if self.orientation() == crate::GlyphOrientation::Combined {
            false
        } else {
            !ltr
        };
        let inline = glyph.inline_position
            + if negative_inline {
                store.advance[gi].to_f32()
            } else {
                0.0
            };
        Some((inline, self.baseline() + glyph.block_offset))
    }

    /// Font outline origin `(x, y)` in this line's physical layout container.
    /// `container` is the full physical size, including the extent from which
    /// RTL inline positions and right-to-left block positions are measured.
    /// The result includes [`crate::Line::block_offset`] and uses the line's
    /// effective inline direction, including upright vertical flow.
    ///
    /// Like [`Self::glyph_origin`], this uses the natural shaping advance for
    /// an RTL outline origin; letter spacing and justification stay outside
    /// the advance cell. Apply [`Self::glyph_transform`] to outline vectors
    /// and convert those vectors with [`crate::geometry::PhysicalConverter`].
    /// Returns `None` when `index` is outside this run.
    ///
    /// Retained ruby child lines use their own layout container. To paint them
    /// in the parent, compose [`crate::RubyAnnotationView::transform`] with
    /// [`Self::glyph_origin`], then add the parent line's block offset and
    /// convert with the parent line's physical converter.
    pub fn physical_origin(
        &self,
        index: usize,
        container: crate::geometry::PhysicalSize,
    ) -> Option<(f32, f32)> {
        let (inline, block) = self.glyph_origin(index)?;
        let converter = crate::geometry::PhysicalConverter::new(
            self.line.writing_mode(),
            self.line.used_direction(),
            container,
        );
        Some(converter.point(inline, block + self.line.block_offset()))
    }

    /// Metrics at this run's actual size and normalized variation location.
    pub fn metrics(&self) -> crate::font::FontMetrics {
        self.run_data()
            .instance
            .metrics
            .unwrap_or_else(|| self.data().fonts.metrics(self.font(), self.font_size()))
    }

    /// Resolved vhea/MVAR metrics for this run's font instance, when present.
    /// Glyph-specific vertical advances and origins are reflected in `glyphs`.
    pub fn vertical_metrics(&self) -> Option<crate::font::VerticalFontMetrics> {
        self.run_data().instance.vertical_metrics
    }

    pub fn font_size(&self) -> f32 {
        self.run_data().font_size
    }

    /// Emphasis marks of this run's style, or `None` without
    /// `text-emphasis`. See [`crate::EmphasisMark`] for placement.
    pub fn emphasis_mark(&self) -> Option<crate::EmphasisMark> {
        use crate::style::TextEmphasisShape as S;
        let style = &self.data().styles[self.style_index() as usize];
        let emphasis = style.text_emphasis?;
        let character = match (emphasis.shape, emphasis.filled) {
            (S::Dot, true) => '\u{2022}',
            (S::Dot, false) => '\u{25E6}',
            (S::Circle, true) => '\u{25CF}',
            (S::Circle, false) => '\u{25CB}',
            (S::DoubleCircle, true) => '\u{25C9}',
            (S::DoubleCircle, false) => '\u{25CE}',
            (S::Triangle, true) => '\u{25B2}',
            (S::Triangle, false) => '\u{25B3}',
            (S::Sesame, true) => '\u{FE45}',
            (S::Sesame, false) => '\u{FE46}',
            (S::Custom(character), _) => character,
        };
        let line_over =
            crate::line::metrics::emphasis_over(emphasis.position, self.data().style.writing_mode);
        let half = self.font_size() / 2.0;
        let (ascent, descent) = match self.orientation() {
            crate::GlyphOrientation::Combined => (half, half),
            crate::GlyphOrientation::Upright => self
                .vertical_metrics()
                .map_or((half, half), |v| (v.ascent, v.descent)),
            _ => {
                let metrics = self.metrics();
                (metrics.ascent, metrics.descent)
            }
        };
        Some(crate::EmphasisMark {
            character,
            font_size: crate::line::metrics::emphasis_mark_extent(style),
            line_over,
            offset: if line_over { ascent } else { descent },
        })
    }

    /// Normalized variation coordinates in the face's axis order.
    pub fn normalized_coords(&self) -> &'a [crate::font::NormalizedCoord] {
        &self.run_data().instance.coords
    }
    pub fn variations(&self) -> &'a [crate::style::FontVariation] {
        &self.run_data().instance.variations
    }
    pub fn embolden(&self) -> bool {
        self.run_data().instance.embolden
    }
    /// Synthetic slant supplied by font matching, in degrees.
    pub fn skew(&self) -> Option<f32> {
        self.run_data().instance.skew
    }
    pub fn script(&self) -> [u8; 4] {
        self.run_data().instance.script
    }
    pub fn language(&self) -> Option<&'a str> {
        self.run_data().instance.language.as_deref()
    }

    pub fn font_data(&self) -> Option<FontData> {
        self.data().fonts.font_data(self.font())
    }

    /// Color glyph tables of this run's face; see
    /// [`crate::font::ColorGlyphFormats`]. Reads the table headers on each
    /// call, so cache the result per [`Self::font`] when painting many runs.
    pub fn color_glyph_formats(&self) -> crate::font::ColorGlyphFormats {
        self.font_data()
            .map(|data| crate::font::ColorGlyphFormats::from_font_data(&data))
            .unwrap_or_default()
    }

    pub fn bidi_level(&self) -> u8 {
        self.record.level
    }

    /// Processed-text range covered by this run. A shared cluster can include
    /// text from several source nodes despite having one paint owner. This is
    /// a range in the accepted line's [`crate::Line::text`] dataset, not a DOM range.
    pub fn text_range(&self) -> Range<usize> {
        self.text.0 as usize..self.text.1 as usize
    }

    pub fn inline_start(&self) -> f32 {
        self.record.inline_start.to_f32()
    }

    pub fn inline_size(&self) -> f32 {
        self.record.inline_size.to_f32()
    }

    /// Dominant baseline in logical block coordinates from block-start.
    pub fn baseline(&self) -> f32 {
        (self.line.baseline + self.block_shift).to_f32()
    }

    pub fn glyphs(&self) -> Glyphs<'a> {
        Glyphs {
            view: *self,
            next: self.glyphs.0,
            end: self.glyphs.1,
        }
    }

    fn cluster_parts(
        &self,
    ) -> impl ExactSizeIterator<Item = (Range<u32>, Range<u32>, &'a crate::shape::GlyphStore)> + '_
    {
        let data = self.data();
        let (begin, count) = match self.source {
            GlyphSource::Shared if self.glyphs.0 < self.glyphs.1 => {
                let begin = data.glyph_clusters[self.glyphs.0 as usize] as usize;
                let end = data.glyph_clusters[(self.glyphs.1 - 1) as usize] as usize;
                (begin, end + 1 - begin)
            }
            GlyphSource::Shared => (0, 1),
            GlyphSource::Overlay { clusters, .. } => {
                (clusters.0 as usize, (clusters.1 - clusters.0) as usize)
            }
        };
        (0..count).map(move |i| match self.source {
            GlyphSource::Shared if self.glyphs.0 == self.glyphs.1 => (
                self.glyphs.0..self.glyphs.1,
                self.text.0..self.text.1,
                &data.glyphs,
            ),
            GlyphSource::Shared => {
                let u = &data.units[data.clusters[begin + i] as usize];
                let crate::analysis::units::UnitKind::Cluster { glyphs, .. } = &u.kind else {
                    unreachable!()
                };
                (glyphs.clone(), u.shaping_text().clone(), &data.glyphs)
            }
            GlyphSource::Overlay { .. } => {
                let c = &self.line.overlay_clusters[begin + i];
                (
                    c.glyphs.clone(),
                    c.text.clone(),
                    self.line.overlay.as_deref().expect("overlay store"),
                )
            }
        })
    }

    pub fn clusters(&self) -> impl ExactSizeIterator<Item = Cluster> + '_ {
        let data = self.data();
        self.cluster_parts().map(move |(glyphs, text, store)| {
            #[cfg(test)]
            data.cluster_queries
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let source = data
                .text
                .get(text.start as usize..text.end as usize)
                .unwrap_or_default();
            let mut flags =
                ClusterFlags::from_source(source, self.line.visible_hyphen == Some(text.start));
            if self.orientation() == crate::GlyphOrientation::Combined {
                flags.emphasis_excluded = true;
            }
            Cluster {
                source_char: source.chars().next(),
                flags,
                text_range: text.start as usize..text.end as usize,
                advance: glyphs.clone().map(|g| self.glyph(g).advance).sum(),
                shaping_advance: glyphs.map(|g| store.advance[g as usize].to_f32()).sum(),
            }
        })
    }

    pub(crate) fn geometry_clusters(
        &self,
    ) -> impl ExactSizeIterator<Item = crate::output::GeometryCluster> + '_ {
        self.cluster_parts().map(move |(glyphs, text, store)| {
            let first_glyph_id = if glyphs.start < glyphs.end {
                Some(store.id[glyphs.start as usize])
            } else {
                None
            };
            let mut advance = 0.0;
            let mut shaping_advance = 0.0;
            for glyph in glyphs.clone() {
                advance += self.glyph(glyph).advance;
                shaping_advance += store.advance[glyph as usize].to_f32();
            }
            crate::output::GeometryCluster {
                text,
                glyphs,
                first_glyph_id,
                advance,
                shaping_advance,
            }
        })
    }

    fn glyph(&self, g: u32) -> Glyph {
        let store = match self.source {
            GlyphSource::Shared => &self.data().glyphs,
            GlyphSource::Overlay { .. } => self.line.overlay.as_deref().expect("overlay store"),
        };
        let gi = g as usize;
        if matches!(self.source, GlyphSource::Shared)
            && let Some(paint) = self
                .data()
                .combine_geometry
                .glyphs
                .get(gi)
                .copied()
                .flatten()
        {
            let span = &self.data().combine_spans[paint.span];
            let sign = if self.data().style.writing_mode == crate::geometry::WritingMode::VerticalRl
            {
                -1.0
            } else {
                1.0
            };
            let baseline = self.data().combine_geometry.baselines[paint.span]
                + store.offset_block[gi].to_f32();
            return Glyph {
                id: store.id[gi],
                inline_position: self.record.inline_start.to_f32()
                    + if self.line.used_direction() == crate::geometry::Direction::Ltr {
                        baseline
                    } else {
                        span.em - baseline
                    },
                block_offset: sign * (paint.x - span.em / 2.0),
                advance: store.advance[gi].to_f32() + paint.extra,
                cluster: store.cluster[gi],
            };
        }
        let first = self.glyphs.0 as usize;
        let (rel, advance) = if matches!(self.source, GlyphSource::Shared)
            && let Some((start, positions)) = &self.line.positions
        {
            let pen = positions[(g - start) as usize];
            let spacing = self
                .line
                .glyph_spacing
                .as_ref()
                .map(|(start, v)| v[(g - start) as usize]);
            let rel = pen - positions[(self.glyphs.0 - start) as usize]
                + spacing.map_or(LayoutUnit::ZERO, |s| s.leading);
            let end = if g + 1 < self.glyphs.1 {
                positions[(g + 1 - start) as usize] - positions[(self.glyphs.0 - start) as usize]
            } else {
                self.record.inline_size
            };
            (
                rel,
                spacing.map_or(end - rel, |s| store.advance[gi] + s.extra),
            )
        } else {
            let rel = store.pen[gi] - store.pen[first]
                + store.leading.as_ref().map_or(LayoutUnit::ZERO, |v| v[gi]);
            let advance = if let Some(spacing) = &store.spacing {
                store.advance[gi] + spacing[gi]
            } else if matches!(self.source, GlyphSource::Overlay { .. })
                && self.line.positions.is_some()
                && g + 1 == self.glyphs.1
            {
                self.record.inline_size - rel
            } else {
                store.advance[gi]
            };
            (rel, advance)
        };
        // Runs are stored in logical order and reversed for display here; a
        // real shaper that emits right-to-left runs in visual order must not
        // be reversed twice.
        let reversed = self.record.level % 2 != self.line.data.base_level % 2;
        let pen = if reversed {
            // Layout spacing belongs between clusters; subtracting it from
            // a glyph's ink origin would detach a mark from its base.
            self.record.inline_size - rel - store.advance[gi]
        } else {
            rel
        };
        let offset = if reversed {
            LayoutUnit::ZERO - store.offset_inline[gi]
        } else {
            store.offset_inline[gi]
        };
        let position = self.record.inline_start + pen + offset;
        Glyph {
            id: store.id[gi],
            inline_position: position.to_f32(),
            block_offset: store.offset_block[gi].to_f32(),
            advance: advance.to_f32(),
            cluster: store.cluster[gi],
        }
    }
}

impl Glyphs<'_> {
    /// The `index`-th glyph of the run, independent of iteration.
    pub fn get(&self, index: usize) -> Option<Glyph> {
        let g = (self.view.glyphs.0 as usize).checked_add(index)?;
        (g < self.view.glyphs.1 as usize).then(|| self.view.glyph(g as u32))
    }
}

impl Iterator for Glyphs<'_> {
    type Item = Glyph;

    fn next(&mut self) -> Option<Glyph> {
        (self.next < self.end).then(|| {
            self.next += 1;
            self.view.glyph(self.next - 1)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.end - self.next) as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Glyphs<'_> {}
