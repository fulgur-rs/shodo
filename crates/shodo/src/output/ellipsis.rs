//! `text-overflow: ellipsis` on an accepted line.

use std::ops::Range;

use super::{Fragment, Line, OverlayCluster};
use crate::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::line::fragments::{FragmentRecord, GlyphSource, RecordKind};
use crate::shape::{GlyphStore, ShapedRun};

/// Where [`Line::truncate_with_ellipsis`] placed the ellipsis.
#[derive(Clone, Debug, PartialEq)]
pub struct Truncation {
    /// Indices of the ellipsis glyph runs in [`Line::fragments`]: one run,
    /// or one per font when the ellipsis falls back to three periods.
    pub fragments: Range<usize>,
    /// Inline start of the ellipsis, where the remaining content ends.
    pub inline_start: f32,
    /// Advance of the ellipsis.
    pub inline_size: f32,
}

/// A visible piece truncation keeps or hides as a whole.
struct Piece {
    record: usize,
    start: LayoutUnit,
    end: LayoutUnit,
    /// Logical cluster index within the record, for glyph runs.
    cluster: Option<usize>,
    text: Range<u32>,
}

/// Per glyph-run cluster geometry, in logical order.
struct RunClusters {
    glyphs: Vec<Range<u32>>,
    texts: Vec<Range<u32>>,
    /// Pen offsets of each cluster start, then the record size.
    bounds: Vec<LayoutUnit>,
    reversed: bool,
}

impl Line {
    /// Truncates the line for `text-overflow: ellipsis` when its content ends
    /// past `available`, measured like fragment positions from the line's
    /// inline-start edge. Returns `None` and leaves the line unchanged when it
    /// fits, when `available` is not finite, when it was already truncated, or
    /// when the ellipsis cannot be shaped within the paragraph's limits.
    ///
    /// This follows Blink's `LineTruncator`. The ellipsis is U+2026 when the
    /// line style's primary font has it, otherwise three periods, shaped in
    /// the line's root style (the first-line root on the first formatted
    /// line) on the root baseline; it does not change the line's height.
    /// Clusters, atomic inlines and combined text that would end past the
    /// ellipsis are removed from [`Self::fragments`], together with everything
    /// after them at the inline-end side; ruby bases and their annotations go
    /// together. The first cluster or atomic inline on the line stays even
    /// when it does not fit, for the caller's clip. The ellipsis follows the
    /// remaining content, so it can end before `available`. Inline boxes cut by
    /// the ellipsis end at the remaining content and lose their end edge.
    ///
    /// The line's text, offsets, break token and metrics are unchanged, so
    /// layout of later lines is unaffected. Hidden text has no geometry: hit
    /// testing, carets and selections see only the remaining fragments. The
    /// ellipsis runs report [`super::GlyphRunView::is_ellipsis`], an empty text
    /// range at the first hidden text offset, no node and the root paint.
    pub fn truncate_with_ellipsis(
        &mut self,
        cx: &mut LayoutContext,
        available: f32,
    ) -> Option<Truncation> {
        if self.ellipsis.is_some() || !available.is_finite() {
            return None;
        }
        let mut sat = Saturation::default();
        let limit = LayoutUnit::from_f32_round(available, &mut sat);
        if self.origin.add(self.inline_size, &mut sat) <= limit {
            return None;
        }
        let (store, runs) = shape_ellipsis(self, cx, &mut sat)?;
        let width = store
            .advance
            .iter()
            .fold(LayoutUnit::ZERO, |w, a| w.add(*a, &mut sat));
        let cut = limit.sub(width, &mut sat);

        let (pieces, clusters) = self.pieces();
        let ruby = self.ruby_extents(&pieces);
        let effective_end = |piece: &Piece| {
            ruby.iter()
                .find(|(text, _, _)| text.start <= piece.text.start && piece.text.start < text.end)
                .map_or(piece.end, |(_, _, end)| piece.end.max(*end))
        };
        let effective_start = |piece: &Piece| {
            ruby.iter()
                .find(|(text, _, _)| text.start <= piece.text.start && piece.text.start < text.end)
                .map_or(piece.start, |(_, start, _)| piece.start.min(*start))
        };
        // Everything from the first piece that would end past the cut onward
        // is hidden, so the remaining content is one run from inline-start.
        let mut hidden_from = pieces
            .iter()
            .filter(|p| effective_end(p) > cut)
            .map(effective_start)
            .min()
            .unwrap_or(LayoutUnit::MAX);
        let first = pieces.iter().map(|p| p.start).min();
        if let Some(first) = first
            && hidden_from <= first
        {
            // Keep the first piece for the caller's clip (CSS Overflow 3 §3.2).
            hidden_from = pieces
                .iter()
                .map(|p| p.start)
                .filter(|start| *start > first)
                .min()
                .unwrap_or(LayoutUnit::MAX);
        }
        let kept = |piece: &Piece| piece.start < hidden_from;
        let mut end = pieces
            .iter()
            .filter(|p| kept(p))
            .map(|p| p.end)
            .max()
            .unwrap_or(self.origin);
        for record in &self.fragments {
            if matches!(record.kind, RecordKind::InlineBox { .. }) {
                let box_end = record.inline_start.add(record.inline_size, &mut sat);
                if record.inline_start < hidden_from && box_end <= cut {
                    end = end.max(box_end);
                }
            }
        }
        let hidden_text: Vec<Range<u32>> = pieces
            .iter()
            .filter(|p| !kept(p))
            .map(|p| p.text.clone())
            .collect();
        let ellipsis_text = hidden_text
            .iter()
            .map(|t| t.start)
            .min()
            .unwrap_or(self.text_range.end);

        // Rebuild the records in visual order.
        let mut map = vec![None; self.fragments.len()];
        let mut records = Vec::with_capacity(self.fragments.len() + runs.len());
        let mut shifts = Vec::with_capacity(records.capacity());
        let records_in = std::mem::take(&mut self.fragments);
        for (index, mut record) in records_in.into_iter().enumerate() {
            let keep = match &mut record.kind {
                RecordKind::Glyphs {
                    source,
                    glyphs,
                    text,
                    ..
                } => {
                    let own: Vec<&Piece> = pieces.iter().filter(|p| p.record == index).collect();
                    let kept_count = own.iter().filter(|p| kept(p)).count();
                    if kept_count == own.len() {
                        true
                    } else if kept_count == 0 {
                        false
                    } else {
                        let run = clusters[index].as_ref().expect("cluster geometry");
                        narrow(
                            &mut record.inline_size,
                            source,
                            glyphs,
                            text,
                            run,
                            &own,
                            &kept,
                        );
                        true
                    }
                }
                RecordKind::Atomic { .. } => {
                    pieces.iter().find(|p| p.record == index).is_none_or(&kept)
                }
                RecordKind::InlineBox { end_edge, .. } => {
                    if record.inline_start >= hidden_from {
                        false
                    } else {
                        let box_end = record.inline_start.add(record.inline_size, &mut sat);
                        if box_end > cut {
                            record.inline_size =
                                end.sub(record.inline_start, &mut sat).max(LayoutUnit::ZERO);
                            *end_edge = false;
                        }
                        true
                    }
                }
                RecordKind::Anchor { .. } => true,
            };
            if keep {
                map[index] = Some(records.len() as u32);
                shifts.push(self.block_shifts[index]);
                records.push(record);
            }
        }
        for record in &mut records {
            if let RecordKind::InlineBox { parent, .. } = &mut record.kind {
                *parent = parent.and_then(|p| map[p as usize]);
            }
        }
        let intersects = |range: &Range<u32>| {
            hidden_text
                .iter()
                .any(|t| t.start < range.end.max(range.start + 1) && range.start < t.end)
        };
        self.ruby.retain(|annotation| {
            let base = annotation.base_text.start as u32..annotation.base_text.end as u32;
            !intersects(&base)
        });
        self.combinations.retain(|square| {
            !intersects(&(square.text_range.start as u32..square.text_range.end as u32))
        });
        self.tabs.retain(|tab| tab.start < hidden_from);
        if self.visible_hyphen.is_some_and(|h| intersects(&(h..h + 1))) {
            self.visible_hyphen = None;
        }

        // Append the ellipsis glyphs to the line's overlay store.
        let first_index = records.len();
        let mut overlay = self.overlay.take().map_or_else(GlyphStore::default, |o| *o);
        let mut overlay_clusters = std::mem::take(&mut self.overlay_clusters).into_vec();
        let mut overlay_runs = std::mem::take(&mut self.overlay_runs).into_vec();
        let offset = overlay.len() as u32;
        let glyph_count = store.len();
        append(&mut overlay, store, ellipsis_text);
        let mut position = end;
        for mut run in runs {
            let glyphs = offset + run.glyphs.start..offset + run.glyphs.end;
            let run_width = overlay.advance[glyphs.start as usize..glyphs.end as usize]
                .iter()
                .fold(LayoutUnit::ZERO, |w, a| w.add(*a, &mut sat));
            let cluster = overlay_clusters.len() as u32;
            overlay_clusters.push(OverlayCluster {
                glyphs: glyphs.clone(),
                text: ellipsis_text..ellipsis_text,
            });
            run.glyphs = glyphs.clone();
            run.text = ellipsis_text..ellipsis_text;
            let run_index = overlay_runs.len() as u32;
            overlay_runs.push(run);
            records.push(FragmentRecord {
                kind: RecordKind::Glyphs {
                    source: GlyphSource::Overlay {
                        glyphs: (glyphs.start, glyphs.end),
                        clusters: (cluster, cluster + 1),
                        run: Some(run_index),
                    },
                    run: 0,
                    glyphs: 0..0,
                    item: 0,
                    text: ellipsis_text..ellipsis_text,
                },
                inline_start: position,
                inline_size: run_width,
                level: self.data.base_level,
            });
            shifts.push(LayoutUnit::ZERO);
            position = position.add(run_width, &mut sat);
        }
        debug_assert_eq!(overlay.len(), offset as usize + glyph_count);
        self.overlay = Some(Box::new(overlay));
        self.overlay_clusters = overlay_clusters.into_boxed_slice();
        self.overlay_runs = overlay_runs.into_boxed_slice();
        let last_index = records.len();
        self.fragments = records;
        self.block_shifts = shifts;
        self.ellipsis = Some(first_index as u32..last_index as u32);
        self.inline_size = position.sub(self.origin, &mut sat);
        self.trailing_whitespace = LayoutUnit::ZERO;
        self.hanging_end = LayoutUnit::ZERO;
        cx.warnings.record_saturation(&sat);
        Some(Truncation {
            fragments: first_index..last_index,
            inline_start: end.to_f32(),
            inline_size: position.sub(end, &mut sat).to_f32(),
        })
    }

    /// Pieces in record order with each glyph run's cluster geometry.
    fn pieces(&self) -> (Vec<Piece>, Vec<Option<RunClusters>>) {
        let mut pieces = Vec::new();
        let mut clusters = Vec::with_capacity(self.fragments.len());
        for (index, record) in self.fragments.iter().enumerate() {
            let start = record.inline_start;
            let end = start + record.inline_size;
            match (&record.kind, self.view(index)) {
                (RecordKind::Glyphs { .. }, Fragment::GlyphRun(run))
                    if run.orientation() != crate::GlyphOrientation::Combined
                        && run.glyphs().len() > 0 =>
                {
                    let reversed = record.level % 2 != self.data.base_level % 2;
                    let mut geometry = RunClusters {
                        glyphs: Vec::new(),
                        texts: Vec::new(),
                        bounds: Vec::new(),
                        reversed,
                    };
                    for cluster in run.geometry_clusters() {
                        let at = if cluster.glyphs.is_empty() {
                            geometry.bounds.last().copied().unwrap_or(LayoutUnit::ZERO)
                        } else {
                            run.logical_pen(cluster.glyphs.start)
                        };
                        geometry.bounds.push(at);
                        geometry.glyphs.push(cluster.glyphs);
                        geometry.texts.push(cluster.text);
                    }
                    geometry.bounds.push(record.inline_size);
                    for k in 0..geometry.texts.len() {
                        let (from, to) = (geometry.bounds[k], geometry.bounds[k + 1]);
                        let (a, b) = if reversed {
                            (record.inline_size - to, record.inline_size - from)
                        } else {
                            (from, to)
                        };
                        pieces.push(Piece {
                            record: index,
                            start: start + a,
                            end: start + b,
                            cluster: Some(k),
                            text: geometry.texts[k].clone(),
                        });
                    }
                    clusters.push(Some(geometry));
                }
                (RecordKind::Glyphs { text, .. }, _) => {
                    pieces.push(Piece {
                        record: index,
                        start,
                        end,
                        cluster: None,
                        text: text.clone(),
                    });
                    clusters.push(None);
                }
                (RecordKind::Atomic { unit, .. }, _) => {
                    let text = self.data.units[*unit as usize].text.clone();
                    pieces.push(Piece {
                        record: index,
                        start,
                        end,
                        cluster: None,
                        text,
                    });
                    clusters.push(None);
                }
                _ => clusters.push(None),
            }
        }
        (pieces, clusters)
    }

    /// Text range and visual extent of each ruby container on this line,
    /// which truncation keeps or hides with its annotations as a whole.
    fn ruby_extents(&self, pieces: &[Piece]) -> Vec<(Range<u32>, LayoutUnit, LayoutUnit)> {
        let containers = &self.data.ruby.containers;
        if containers.is_empty() {
            return Vec::new();
        }
        let first = containers.partition_point(|c| c.units.end <= self.units.start as usize);
        let mut extents = Vec::new();
        for container in containers[first..]
            .iter()
            .take_while(|c| c.units.start < self.units.end as usize)
        {
            let Some(text) = container
                .columns
                .iter()
                .map(|c| c.text.clone())
                .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end))
            else {
                continue;
            };
            let inside = pieces
                .iter()
                .filter(|p| text.start <= p.text.start && p.text.start < text.end);
            let bounds = inside.fold(None, |acc: Option<(LayoutUnit, LayoutUnit)>, p| {
                Some(acc.map_or((p.start, p.end), |(s, e)| (s.min(p.start), e.max(p.end))))
            });
            if let Some((start, end)) = bounds {
                extents.push((text, start, end));
            }
        }
        extents
    }
}

/// Narrows a partly kept glyph run to its kept clusters: a logical prefix,
/// or a suffix when the run displays reversed.
fn narrow(
    inline_size: &mut LayoutUnit,
    source: &mut GlyphSource,
    shared: &mut Range<u32>,
    text: &mut Range<u32>,
    run: &RunClusters,
    own: &[&Piece],
    kept: &impl Fn(&Piece) -> bool,
) {
    let kept_clusters: Vec<usize> = own
        .iter()
        .filter(|p| kept(p))
        .filter_map(|p| p.cluster)
        .collect();
    let (first, last) = (
        *kept_clusters.iter().min().expect("kept cluster"),
        *kept_clusters.iter().max().expect("kept cluster"),
    );
    let glyphs = run.glyphs[first].start..run.glyphs[last].end;
    *text = run.texts[first].start..run.texts[last].end;
    *inline_size = run.bounds[last + 1] - run.bounds[first];
    debug_assert!(first == 0 || run.reversed);
    match source {
        GlyphSource::Shared => *shared = glyphs,
        GlyphSource::Overlay {
            glyphs: pair,
            clusters,
            ..
        } => {
            let base = clusters.0;
            *pair = (glyphs.start, glyphs.end);
            *clusters = (base + first as u32, base + last as u32 + 1);
        }
    }
}

/// Appends shaped ellipsis glyphs to the overlay store, attributing them to
/// the empty text range at `text`.
fn append(overlay: &mut GlyphStore, store: GlyphStore, text: u32) {
    let start = overlay.len();
    let count = store.len();
    if let Some(spacing) = &mut overlay.spacing {
        spacing.extend(std::iter::repeat_n(LayoutUnit::ZERO, count));
    }
    if let Some(leading) = &mut overlay.leading {
        leading.extend(std::iter::repeat_n(LayoutUnit::ZERO, count));
    }
    overlay.id.extend(store.id);
    overlay.advance.extend(store.advance);
    overlay.pen.extend(store.pen);
    overlay.offset_inline.extend(store.offset_inline);
    overlay.offset_block.extend(store.offset_block);
    overlay.cluster.extend(std::iter::repeat_n(text, count));
    overlay.flags.extend(store.flags);
    debug_assert_eq!(overlay.len(), start + count);
}

/// Shapes U+2026, or three periods when the line style's primary font lacks
/// it, in the line's root style.
fn shape_ellipsis(
    line: &Line,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    use crate::analysis::itemize::{Scalar, ShapeItem};
    use crate::font::FontQuery;
    use skrifa::MetadataProvider;
    let data = &line.data;
    let style = &data.styles[0];
    let primary = data.style_metrics[0].font;
    let has_ellipsis = data.fonts.font_data(primary).is_some_and(|font| {
        skrifa::FontRef::from_index(font.data.as_ref(), font.index)
            .ok()
            .and_then(|face| face.charmap().map('\u{2026}'))
            .is_some_and(|glyph| glyph.to_u32() != 0)
    });
    let text = if has_ellipsis { "\u{2026}" } else { "..." };
    let script = *b"Zyyy";
    let query = FontQuery {
        families: style.font_families.clone(),
        weight: style.font_weight,
        width: style.font_width,
        style: style.font_style,
        script,
        language: style.lang.clone(),
        synthesis: style.font_synthesis,
        ..Default::default()
    }
    .normalized();
    let font = data.fonts.match_scripted(&query, script, &text[..1]);
    let first = text.chars().next().expect("ellipsis text");
    let item = ShapeItem {
        segment: 0,
        scalars: text
            .char_indices()
            .map(|(offset, c)| Scalar {
                c,
                offset: offset as u32,
                end: (offset + c.len_utf8()) as u32,
                item: 0,
                grapheme_start: true,
            })
            .collect(),
        end: text.len() as u32,
        style: 0,
        level: data.base_level,
        script,
        font,
        orientation: crate::shape::orientation::resolve(
            data.style.writing_mode,
            style.text_orientation,
            first,
        ),
        combine: None,
        width_feature: None,
        before: String::new(),
        after: String::new(),
    };
    let items = [item];
    let features = crate::shape::FeatureSets::new(&items, &data.styles);
    let mut limits = data.limits.clone();
    limits.max_shaped_glyphs = Some(16);
    let mut warnings = std::mem::take(&mut cx.warnings);
    let result = crate::shape::shape_items_with_base_scopes(
        cx,
        &items,
        &data.styles,
        &data.fonts,
        data.style.writing_mode,
        &limits,
        &mut warnings,
        sat,
        None,
        &features,
    );
    cx.warnings = warnings;
    match result {
        Ok((store, runs)) if store.len() > 0 => Some((store, runs)),
        _ => {
            cx.warnings.push(
                crate::limits::WarningKind::Unsupported,
                "ellipsis could not be shaped; line left untruncated",
            );
            None
        }
    }
}
