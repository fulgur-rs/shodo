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
    /// remaining content, so it can end before `available`. Inline boxes that
    /// start before the hidden content keep their geometry, as in Blink, so
    /// the caller's overflow clip trims a box the ellipsis cuts.
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
        if self.ellipsis.is_some() || self.empty || !available.is_finite() {
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

        let (pieces, by_record, clusters) = self.pieces();
        let (containers, container_of) = self.ruby_extents(&pieces);
        // Ruby bases and their annotations are kept or hidden together.
        let effective_end = |i: usize| {
            container_of[i].map_or(pieces[i].end, |c| pieces[i].end.max(containers[c].2))
        };
        let effective_start = |i: usize| {
            container_of[i].map_or(pieces[i].start, |c| pieces[i].start.min(containers[c].1))
        };
        // Everything from the first piece that would end past the cut onward
        // is hidden, so the remaining content is one run from inline-start.
        let mut hidden_from = (0..pieces.len())
            .filter(|&i| effective_end(i) > cut)
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
        let mut kept: Vec<bool> = pieces.iter().map(|p| p.start < hidden_from).collect();
        // Within a glyph run, keep a logical prefix (a suffix when the run
        // displays reversed) even when negative spacing makes cluster
        // positions overlap, so narrowing never moves a kept glyph.
        for (record, range) in by_record.iter().enumerate() {
            if let Some(run) = &clusters[record] {
                let mut order: Vec<usize> = range.clone().collect();
                if run.reversed {
                    order.reverse();
                }
                let mut open = true;
                for i in order {
                    open &= kept[i];
                    kept[i] = open;
                }
            }
        }
        // Negative spacing can put a run's first kept cluster after one that
        // fails; still keep the first piece of the visually first record.
        if !kept.iter().any(|k| *k)
            && let Some(first) = (0..pieces.len()).min_by_key(|&i| pieces[i].start)
            && let Some(record) = by_record.iter().position(|r| r.contains(&first))
        {
            let range = by_record[record].clone();
            let walk_start = match &clusters[record] {
                Some(run) if run.reversed => range.end - 1,
                _ => range.start,
            };
            kept[walk_start] = true;
        }
        let mut end = (0..pieces.len())
            .filter(|&i| kept[i])
            .map(|i| pieces[i].end)
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
        let hidden_text = HiddenText::new(
            (0..pieces.len())
                .filter(|&i| !kept[i])
                .map(|i| pieces[i].text.clone()),
        );
        let ellipsis_text = hidden_text.start().unwrap_or(self.text_range.end);

        // Rebuild the records in visual order.
        let mut map = vec![None; self.fragments.len()];
        let mut records = Vec::with_capacity(self.fragments.len() + runs.len());
        let mut shifts = Vec::with_capacity(records.capacity());
        let has_emphasis_offsets = !self.emphasis_offsets.is_empty();
        let mut emphasis_offsets = if has_emphasis_offsets {
            Vec::with_capacity(records.capacity())
        } else {
            Vec::new()
        };
        let records_in = std::mem::take(&mut self.fragments);
        for (index, mut record) in records_in.into_iter().enumerate() {
            let own = by_record[index].clone();
            let kept_count = own.clone().filter(|&i| kept[i]).count();
            let keep = match &mut record.kind {
                RecordKind::Glyphs {
                    source,
                    glyphs,
                    text,
                    ..
                } => {
                    if kept_count == own.len() {
                        true
                    } else if kept_count == 0 {
                        false
                    } else {
                        let run = clusters[index].as_ref().expect("cluster geometry");
                        let kept_clusters =
                            own.filter(|&i| kept[i]).filter_map(|i| pieces[i].cluster);
                        narrow(
                            &mut record.inline_size,
                            source,
                            glyphs,
                            text,
                            run,
                            kept_clusters,
                        );
                        true
                    }
                }
                RecordKind::Atomic { .. } => kept_count == own.len(),
                // Like Blink, a box keeps its geometry; the caller's overflow
                // clip trims what extends under or past the ellipsis.
                RecordKind::InlineBox { .. } => record.inline_start < hidden_from,
                RecordKind::Anchor { .. } => true,
            };
            if keep {
                map[index] = Some(records.len() as u32);
                shifts.push(self.block_shifts[index]);
                if has_emphasis_offsets {
                    emphasis_offsets.push(self.emphasis_offsets[index]);
                }
                records.push(record);
            }
        }
        for record in &mut records {
            if let RecordKind::InlineBox { parent, .. } = &mut record.kind {
                *parent = parent.and_then(|p| map[p as usize]);
            }
        }
        self.ruby.retain(|annotation| {
            !hidden_text
                .intersects(annotation.base_text.start as u32..annotation.base_text.end as u32)
        });
        self.combinations.retain(|square| {
            !hidden_text.intersects(square.text_range.start as u32..square.text_range.end as u32)
        });
        self.tabs.retain_mut(|tab| {
            tab.width = tab.width.min(end.sub(tab.start, &mut sat));
            tab.start < end
        });
        if self
            .visible_hyphen
            .is_some_and(|h| hidden_text.intersects(h..h + 1))
        {
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
            if has_emphasis_offsets {
                emphasis_offsets.push((LayoutUnit::ZERO, LayoutUnit::ZERO));
            }
            position = position.add(run_width, &mut sat);
        }
        debug_assert_eq!(overlay.len(), offset as usize + glyph_count);
        self.overlay = Some(Box::new(overlay));
        self.overlay_clusters = overlay_clusters.into_boxed_slice();
        self.overlay_runs = overlay_runs.into_boxed_slice();
        let last_index = records.len();
        self.fragments = records;
        self.block_shifts = shifts;
        self.emphasis_offsets = emphasis_offsets;
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

    /// Pieces in record order, the piece range of each record, and each
    /// glyph run's cluster geometry.
    #[allow(clippy::type_complexity)]
    fn pieces(&self) -> (Vec<Piece>, Vec<Range<usize>>, Vec<Option<RunClusters>>) {
        let mut pieces = Vec::new();
        let mut by_record = Vec::with_capacity(self.fragments.len());
        let mut clusters = Vec::with_capacity(self.fragments.len());
        for (index, record) in self.fragments.iter().enumerate() {
            let begin = pieces.len();
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
                        start,
                        end,
                        cluster: None,
                        text,
                    });
                    clusters.push(None);
                }
                _ => clusters.push(None),
            }
            by_record.push(begin..pieces.len());
        }
        (pieces, by_record, clusters)
    }

    /// Text range and visual extent of each ruby container on this line,
    /// which truncation keeps or hides with its annotations as a whole, and
    /// the container of each piece.
    #[allow(clippy::type_complexity)]
    fn ruby_extents(
        &self,
        pieces: &[Piece],
    ) -> (
        Vec<(Range<u32>, LayoutUnit, LayoutUnit)>,
        Vec<Option<usize>>,
    ) {
        let containers = &self.data.ruby.containers;
        let first = containers.partition_point(|c| c.units.end <= self.units.start as usize);
        // Containers are in text order and do not overlap.
        let mut extents: Vec<(Range<u32>, LayoutUnit, LayoutUnit)> = containers[first..]
            .iter()
            .take_while(|c| c.units.start < self.units.end as usize)
            .filter_map(|container| {
                container
                    .columns
                    .iter()
                    .map(|c| c.text.clone())
                    .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end))
                    .map(|text| (text, LayoutUnit::MAX, LayoutUnit::MIN))
            })
            .collect();
        let owners = pieces
            .iter()
            .map(|piece| {
                let at = extents.partition_point(|(text, ..)| text.end <= piece.text.start);
                let owner = extents.get_mut(at)?;
                if owner.0.start > piece.text.start {
                    return None;
                }
                owner.1 = owner.1.min(piece.start);
                owner.2 = owner.2.max(piece.end);
                Some(at)
            })
            .collect();
        (extents, owners)
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
    kept_clusters: impl Iterator<Item = usize>,
) {
    let (first, last) = kept_clusters.fold((usize::MAX, 0), |(lo, hi), k| (lo.min(k), hi.max(k)));
    let count = run.texts.len();
    debug_assert!(first <= last && last < count);
    debug_assert!(if run.reversed {
        last == count - 1
    } else {
        first == 0
    });
    let glyphs = run.glyphs[first].start..run.glyphs[last].end;
    *text = run.texts[first].start..run.texts[last].end;
    *inline_size = run.bounds[last + 1] - run.bounds[first];
    // Unit glyph ranges can extend past a record that starts or ends mid-unit.
    let clamp = |range: Range<u32>, within: Range<u32>| {
        let start = range.start.max(within.start);
        start..range.end.min(within.end).max(start)
    };
    match source {
        GlyphSource::Shared => *shared = clamp(glyphs, shared.clone()),
        GlyphSource::Overlay {
            glyphs: pair,
            clusters,
            ..
        } => {
            let base = clusters.0;
            let kept = clamp(glyphs, pair.0..pair.1);
            *pair = (kept.start, kept.end);
            *clusters = (base + first as u32, base + last as u32 + 1);
        }
    }
}

/// Text ranges truncation hid, sorted and coalesced for binary search.
struct HiddenText(Vec<Range<u32>>);

impl HiddenText {
    fn new(ranges: impl Iterator<Item = Range<u32>>) -> Self {
        let mut ranges: Vec<_> = ranges.collect();
        ranges.sort_unstable_by_key(|r| r.start);
        let mut merged: Vec<Range<u32>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        Self(merged)
    }

    fn start(&self) -> Option<u32> {
        self.0.first().map(|r| r.start)
    }

    /// Whether `range` overlaps hidden text; an empty range counts as the
    /// character at its start.
    fn intersects(&self, range: Range<u32>) -> bool {
        let end = range.end.max(range.start + 1);
        let at = self.0.partition_point(|r| r.end <= range.start);
        self.0.get(at).is_some_and(|r| r.start < end)
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
    let first = text.chars().next().expect("ellipsis text");
    let font = data
        .fonts
        .match_scripted(&query, script, &text[..first.len_utf8()]);
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

#[cfg(test)]
mod tests {
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::line::punctuation::tests::{add_cmap_format12_mappings, cjk_font, cjk_tables};
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, LineOptions, ParagraphStyle};
    use crate::{
        AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
    };
    use skrifa::MetadataProvider;

    #[test]
    fn fonts_with_an_ellipsis_glyph_shape_one_character() {
        // The CJK fixture with U+2026 mapped to its ideographic comma.
        let base = skrifa::FontRef::new(crate::test_support::fonts::CJK).unwrap();
        let glyph = base.charmap().map('、').unwrap().to_u32() as u16;
        let mut tables = cjk_tables();
        add_cmap_format12_mappings(&mut tables, &[(0x2026, glyph)]);
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                cjk_font(&mut tables),
                0,
                FontFaceDescriptor {
                    family: "Ellipsis".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named("Ellipsis".into())];
        style.root.font_size = 10.0;
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "日本語日本語");
        let mut cx = LayoutContext::new();
        let paragraph = builder.build(&mut cx, &fonts).unwrap();
        let LineResult::Line(mut line) = paragraph.next_line(
            &mut cx,
            paragraph.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(1e6),
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected a line")
        };
        let cut = line.truncate_with_ellipsis(&mut cx, 35.0).unwrap();
        let ellipsis: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(run) if run.is_ellipsis() => {
                    Some(run.glyphs().map(|g| g.id).collect::<Vec<_>>())
                }
                _ => None,
            })
            .collect();
        assert_eq!(ellipsis, [vec![u32::from(glyph)]]);
        assert_eq!(cut.fragments.len(), 1);
        assert!(cut.inline_start + cut.inline_size <= 35.0 + 0.01);
    }
}
