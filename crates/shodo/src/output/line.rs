//! Line assembly, metrics, and fragment access.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use peniko::FontData;

use super::{
    AnchorFragment, AtomicFragment, BreakReason, FontId, Fragment, GlyphRunView, InlineBoxFragment,
    Line, LineMetrics, NodeId, RubyAnnotationView, TextCombination,
};
use crate::geometry::{BaselineKind, LayoutUnit, LogicalRect, Saturation};
use crate::line::Scan;
use crate::line::fragments::{self, GlyphSource, RecordKind};
use crate::paragraph::{AtomicSizes, BreakToken, FloatCursor, Paragraph};

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Line")
            .field("units", &self.units)
            .field("reason", &self.reason)
            .field("inline_size", &self.inline_size())
            .finish()
    }
}

impl Line {
    /// Writing mode used by this line's logical coordinates and glyph transforms.
    pub fn writing_mode(&self) -> crate::geometry::WritingMode {
        self.data.style.writing_mode
    }

    /// One square per combined typographic character, including preserved
    /// tabs with no glyphs. Place emphasis once per square; the internal
    /// glyph clusters are excluded from independent emphasis placement.
    pub fn text_combinations(&self) -> impl ExactSizeIterator<Item = &TextCombination> {
        self.combinations.iter()
    }

    pub(crate) fn combination_at(&self, offset: u32) -> Option<&TextCombination> {
        let index = self
            .combinations
            .partition_point(|c| c.text_range.end <= offset as usize);
        self.combinations
            .get(index)
            .filter(|c| c.text_range.start <= offset as usize)
    }
    pub(crate) fn ruby_caret_padding(&self, text: &Range<u32>) -> (f32, f32) {
        let begin = self
            .ruby_caret_gaps
            .partition_point(|gap| gap.text.end <= text.start);
        let mut before = LayoutUnit::ZERO;
        let mut after = LayoutUnit::ZERO;
        for gap in self.ruby_caret_gaps[begin..]
            .iter()
            .take_while(|gap| gap.text.start < text.end)
        {
            if text.start <= gap.text.start {
                before = before + gap.before;
            }
            if gap.text.end <= text.end {
                after = after + gap.after;
            }
        }
        (before.to_f32(), after.to_f32())
    }

    /// Effective inline direction for paint coordinate conversion. Vertical
    /// `text-orientation: upright` uses LTR without changing inherited style.
    pub fn used_direction(&self) -> crate::geometry::Direction {
        crate::analysis::bidi::used_root_direction(&self.data.style, &self.data.styles[0])
    }
    pub fn metrics(&self) -> LineMetrics {
        let baseline = self.baseline.to_f32();
        let root_style = &self.data.styles[0];
        let root = self.data.style_metrics[0];
        let upright = matches!(
            self.data.style.writing_mode,
            crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
        ) && root_style.text_orientation != crate::style::TextOrientation::Sideways;
        let (a, d) = if upright {
            root.vertical_metrics
                .map_or((root.size / 2.0, root.size / 2.0), |v| {
                    (v.ascent, v.descent)
                })
        } else {
            (root.metrics.ascent, root.metrics.descent)
        };
        LineMetrics {
            ascent: baseline,
            descent: self.block_size.to_f32() - baseline,
            baseline,
            text_over: if self.data.style.writing_mode == crate::geometry::WritingMode::VerticalLr {
                baseline + a
            } else {
                baseline - a
            },
            text_under: if self.data.style.writing_mode == crate::geometry::WritingMode::VerticalLr
            {
                baseline - d
            } else {
                baseline + d
            },
        }
    }
    /// Leading hanging punctuation advance.
    pub fn hang_start(&self) -> f32 {
        self.hanging_start.to_f32()
    }
    /// Trailing hanging advance excluded from [`Self::inline_size`], including
    /// eligible whitespace and punctuation. At paragraph ends and forced breaks,
    /// preserved whitespace that fits remains in the line width; only its
    /// overflowing part hangs. See [`Self::trailing_whitespace`] for the full
    /// trailing space/tab advance, including retained whitespace.
    pub fn hang_end(&self) -> f32 {
        self.hanging_end.to_f32()
    }
    /// Bounds of nominal glyph ink and painted box geometry, relative to this
    /// line's top. Renderer-added stroke, antialiasing and decorations can
    /// extend these bounds; block_offset is applied by the caller.
    pub fn overflow_rect(&self) -> LogicalRect {
        use skrifa::{
            MetadataProvider,
            instance::{LocationRef, Size},
            raw::types::GlyphId,
        };
        let mut bounds: Option<LogicalRect> = None;
        let mut include = |rect: LogicalRect| {
            if rect.inline_size <= 0.0 || rect.block_size <= 0.0 {
                return;
            }
            bounds = Some(bounds.map_or(rect, |previous| {
                let left = previous.inline_start.min(rect.inline_start);
                let top = previous.block_start.min(rect.block_start);
                LogicalRect {
                    inline_start: left,
                    block_start: top,
                    inline_size: (previous.inline_start + previous.inline_size)
                        .max(rect.inline_start + rect.inline_size)
                        - left,
                    block_size: (previous.block_start + previous.block_size)
                        .max(rect.block_start + rect.block_size)
                        - top,
                }
            }));
        };
        for fragment in self.fragments() {
            match fragment {
                Fragment::RubyAnnotation(a) => {
                    if a.visibility() != crate::RubyVisibility::Visible {
                        continue;
                    }
                    let r = a.line().overflow_rect();
                    let t = a.transform();
                    let mut left = f32::INFINITY;
                    let mut right = f32::NEG_INFINITY;
                    let mut top = f32::INFINITY;
                    let mut bottom = f32::NEG_INFINITY;
                    for i in [r.inline_start, r.inline_start + r.inline_size] {
                        for b in [r.block_start, r.block_start + r.block_size] {
                            let x = t.inline_inline * i + t.inline_block * b + t.inline_offset;
                            let y = t.block_inline * i + t.block_block * b + t.block_offset;
                            left = left.min(x);
                            right = right.max(x);
                            top = top.min(y);
                            bottom = bottom.max(y);
                        }
                    }
                    include(LogicalRect {
                        inline_start: left,
                        block_start: top,
                        inline_size: right - left,
                        block_size: bottom - top,
                    });
                }
                Fragment::GlyphRun(run) => {
                    let data = run.font_data();
                    let font = data
                        .as_ref()
                        .and_then(|d| skrifa::FontRef::from_index(d.data.as_ref(), d.index).ok());
                    let metrics = font.as_ref().map(|f| {
                        f.glyph_metrics(
                            Size::new(run.font_size()),
                            LocationRef::new(run.normalized_coords()),
                        )
                    });
                    for (index, glyph) in run.glyphs().enumerate() {
                        if let Some(b) = metrics
                            .as_ref()
                            .and_then(|m| m.bounds(GlyphId::new(glyph.id)))
                        {
                            let skew = run.skew().unwrap_or(0.0).to_radians().tan();
                            let (mut left, mut right) = (
                                b.x_min + (skew * b.y_min).min(skew * b.y_max),
                                b.x_max + (skew * b.y_min).max(skew * b.y_max),
                            );
                            if self.data.base_level % 2 == 1 {
                                let natural = run.natural_advance(index);
                                (left, right) = (natural - right, natural - left);
                            }
                            include(LogicalRect {
                                inline_start: glyph.inline_position + left,
                                inline_size: right - left,
                                block_start: run.baseline() + glyph.block_offset - b.y_max,
                                block_size: b.y_max - b.y_min,
                            });
                        } else if metrics.is_none() {
                            let m = run.metrics();
                            include(LogicalRect {
                                inline_start: glyph.inline_position,
                                inline_size: run.natural_advance(index),
                                block_start: run.baseline() + glyph.block_offset - m.ascent,
                                block_size: m.ascent + m.descent,
                            });
                        }
                    }
                }
                Fragment::Atomic(a) => include(a.border_rect),
                Fragment::InlineBox(b) => include(b.rect),
                Fragment::OutOfFlowAnchor(_) => {}
            }
        }
        bounds.unwrap_or_default()
    }
    pub(crate) fn new(
        para: &Paragraph,
        token: BreakToken,
        scan: Scan,
        origin: LayoutUnit,
        block_offset: f32,
        atomics: &AtomicSizes,
        sat: &mut Saturation,
    ) -> Line {
        #[cfg(test)]
        super::construction_probe::record();
        #[cfg(test)]
        super::owner_probe::record(&para.data);
        let data = &para.data;
        let m = data.style_metrics[0].metrics;
        let flags = match scan.reason {
            BreakReason::Forced => BreakToken::AFTER_FORCED,
            _ => 0,
        };
        let origin_units = token.unit as usize..scan.end;
        let visible_hyphen = scan
            .overlays
            .iter()
            .find_map(|w| w.hyphen.as_ref().map(|text| text.start));
        let (mut records, tabs) = fragments::build(
            data,
            origin_units,
            scan.hang_start,
            &scan.widths,
            origin,
            atomics,
            visible_hyphen,
            scan.leading.as_deref(),
        );
        crate::line::autospace::exclude_from_boxes(data, &mut records, &scan.autospace_gaps, sat);
        if let Some(leading) = &scan.leading {
            for record in &mut records {
                let RecordKind::Atomic { size, unit, .. } = &record.kind else {
                    continue;
                };
                let natural = LayoutUnit::from_f32_round(
                    size.inline_size.max(0.0) + size.margins.inline_sum(),
                    sat,
                );
                let before = leading[*unit as usize - token.unit as usize];
                let before = if record.level % 2 != data.base_level % 2 {
                    record.inline_size.sub(natural, sat).sub(before, sat)
                } else {
                    before
                };
                record.inline_start = record.inline_start.add(before, sat);
                record.inline_size = natural;
            }
        }
        // Resource-limited whole clusters can overlap following transparent
        // markers in source order. Their complete source extent still belongs
        // to this line. Cache it so public range queries remain constant-time.
        let text_range = data.units[token.unit as usize..scan.end]
            .iter()
            .fold(None, |range, u| {
                Some(range.map_or_else(
                    || u.text.clone(),
                    |range: Range<u32>| range.start.min(u.text.start)..range.end.max(u.text.end),
                ))
            })
            .unwrap_or(0..0);
        let hanging_end = scan.hanging_end;
        let trailing_whitespace =
            crate::line::trailing_advance(data, token.unit as usize, scan.end, &scan.widths, sat);
        let hanging_start = scan.punctuation_edges.hang_start;
        let mut ruby_caret_gaps = scan.ruby_caret_gaps;
        ruby_caret_gaps.sort_by_key(|gap| gap.text.start);
        Line {
            #[cfg(test)]
            _clone_probe: super::clone_probe::CloneProbe,
            ruby: Vec::new(),
            ruby_caret_gaps,
            data: Arc::clone(&para.data),
            break_token: BreakToken {
                para: data.id,
                unit: scan.end as u32,
                flags,
            },
            reason: scan.reason,
            units: token.unit..scan.end as u32,
            text_range,
            inline_size: scan.content,
            trailing_whitespace,
            hanging_end,
            hanging_start,
            visible_hyphen,
            block_size: LayoutUnit::ZERO,
            baseline: LayoutUnit::ZERO,
            ascent: LayoutUnit::from_f32_round(
                if matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && data.styles[0].text_orientation != crate::style::TextOrientation::Sideways
                {
                    data.style_metrics[0]
                        .vertical_metrics
                        .map_or(data.style_metrics[0].size / 2.0, |v| v.ascent)
                } else {
                    m.ascent
                },
                sat,
            ),
            descent: LayoutUnit::from_f32_round(
                if matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && data.styles[0].text_orientation != crate::style::TextOrientation::Sideways
                {
                    data.style_metrics[0]
                        .vertical_metrics
                        .map_or(data.style_metrics[0].size / 2.0, |v| v.descent)
                } else {
                    m.descent
                },
                sat,
            ),
            block_offset,
            displaced: Vec::new(),
            block_shifts: vec![LayoutUnit::ZERO; records.len()],
            fragments: records,
            empty: false,
            positions: None,
            glyph_spacing: None,
            overlay: None,
            overlay_clusters: Box::default(),
            overlay_runs: Box::default(),
            pending_overlays: scan.overlays,
            tabs,
            combinations: Vec::new(),
        }
    }

    pub(crate) fn measure_metrics(&mut self, sat: &mut Saturation) {
        let metrics = crate::line::metrics::measure(
            &self.data,
            self.units.start as usize..self.units.end as usize,
            &self.fragments,
            &self.overlay_runs,
            sat,
        );
        self.block_size = metrics.block_size;
        self.baseline = metrics.baseline;
        self.block_shifts = metrics.shifts;
        self.empty = metrics.empty;
        self.measure_combinations(&metrics.combination_shifts);
    }

    fn measure_combinations(&mut self, shifts: &crate::hashing::FastMap<usize, LayoutUnit>) {
        self.combinations.clear();
        if self.data.combine_spans.is_empty() {
            return;
        }
        let mut origins = std::collections::HashMap::new();
        for record in &self.fragments {
            if let RecordKind::Glyphs { text, .. } = &record.kind {
                let index = self
                    .data
                    .combine_spans
                    .partition_point(|span| span.text.end <= text.start);
                if self
                    .data
                    .combine_spans
                    .get(index)
                    .is_some_and(|span| span.text.start <= text.start)
                {
                    origins.entry(index).or_insert(record.inline_start.to_f32());
                }
            }
        }
        for tab in &self.tabs {
            let unit = &self.data.units[tab.unit as usize];
            let index = self
                .data
                .combine_spans
                .partition_point(|span| span.text.end <= unit.text.start);
            if self
                .data
                .combine_spans
                .get(index)
                .is_some_and(|span| span.text.start <= unit.text.start)
            {
                origins.entry(index).or_insert(tab.start.to_f32());
            }
        }
        for (index, inline_start) in origins {
            let span = &self.data.combine_spans[index];
            let baseline = (self.baseline + shifts[&index]).to_f32();
            self.combinations.push(TextCombination {
                text_range: span.text.start as usize..span.text.end as usize,
                square: LogicalRect {
                    inline_start,
                    inline_size: span.em,
                    block_start: baseline - span.em / 2.0,
                    block_size: span.em,
                },
            });
        }
        self.combinations
            .sort_by_key(|combination| combination.text_range.start);
    }

    pub fn break_token(&self) -> BreakToken {
        self.break_token
    }

    pub fn break_reason(&self) -> BreakReason {
        self.reason
    }

    /// Whether this is the last line of the paragraph or of a forced-break
    /// section (the line `text-align-last` applies to).
    pub fn is_last(&self) -> bool {
        matches!(
            self.reason,
            BreakReason::Forced | BreakReason::End | BreakReason::BlockInInline
        )
    }

    /// Width of the content, excluding hanging trailing spaces, text-indent
    /// and the inline-start offset.
    pub fn inline_size(&self) -> f32 {
        self.inline_size.to_f32()
    }

    /// Final advance of logically trailing ASCII spaces and tabs present on
    /// this line, whether retained in [`Self::inline_size`] or hanging outside
    /// it. Includes spacing and justification applied to those units.
    ///
    /// Trailing bidi controls, forced breaks, inline end edges and out-of-flow
    /// anchors are ignored. Box edges and punctuation do not contribute;
    /// text-combined typographic squares and in-flow objects end the sequence.
    /// Whitespace removed during paragraph processing contributes no advance.
    pub fn trailing_whitespace(&self) -> f32 {
        self.trailing_whitespace.to_f32()
    }

    /// Line advance (distance to the next line).
    pub fn block_size(&self) -> f32 {
        self.block_size.to_f32()
    }

    pub fn block_offset(&self) -> f32 {
        self.block_offset
    }

    /// Position of a baseline in logical block coordinates from block-start.
    /// The dominant baseline uses font data; the others derive from the
    /// strut's line-over and line-under metrics.
    pub fn baseline(&self, kind: BaselineKind) -> f32 {
        let alphabetic = self.baseline.to_f32();
        let (ascent, descent) = (self.ascent.to_f32(), self.descent.to_f32());
        let over_sign = if self.data.style.writing_mode == crate::geometry::WritingMode::VerticalLr
        {
            1.0
        } else {
            -1.0
        };
        match kind {
            BaselineKind::Alphabetic => alphabetic,
            BaselineKind::Central => alphabetic + over_sign * (ascent - descent) / 2.0,
            BaselineKind::Ideographic => alphabetic - over_sign * descent,
            BaselineKind::Hanging => alphabetic + over_sign * 0.8 * ascent,
        }
    }

    /// The complete processed text set used by this line. `::first-line`
    /// transforms can make this differ from [`Paragraph::text`].
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Mapping for this line's processed text set, when enabled at build.
    pub fn offset_mapping(&self) -> Option<&crate::mapping::OffsetMapping> {
        self.data.mapping.as_ref()
    }

    /// DOM text owners and their UTF-8 byte ranges within each source node.
    /// Uses this line's selected text/mapping dataset, including first-line
    /// transforms and every source node in a shared shaping cluster.
    ///
    /// Entries follow logical source order, coalescing adjacent or overlapping
    /// ranges of the same node. Disjoint ranges can yield that node more than
    /// once. Collapsed gaps at a line boundary belong to the preceding line;
    /// leading gaps belong to the first line. Indivisible transforms retain
    /// their complete original DOM range.
    ///
    /// Generated content, object anchors and inline-box IDs have no DOM text
    /// range and are omitted. Retained ruby children have their own owners.
    /// The iterator is empty when offset mapping was disabled at build time.
    /// Queries allocate no owner list and visit only this line's mapping units
    /// after binary searches over the retained source records.
    pub fn owners(&self) -> impl Iterator<Item = (NodeId, Range<u32>)> + '_ {
        self.offset_mapping()
            .into_iter()
            .flat_map(move |mapping| mapping.dom_owners(self.text_range.clone()))
    }

    /// Range of [`Self::text`] covered by this line.
    pub fn text_range(&self) -> Range<usize> {
        self.text_range.start as usize..self.text_range.end as usize
    }

    /// Floats reported for this line whose anchors ended up after its end.
    pub fn displaced_floats(&self) -> &[(NodeId, FloatCursor)] {
        &self.displaced
    }
}

impl Line {
    /// Non-ruby fragments in visual order, followed by retained ruby annotations.
    ///
    /// Annotation order matches [`Self::ruby_annotations`]. The complete sequence
    /// does not guarantee a global physical visual order. Use
    /// [`RubyAnnotationView::transform`] for annotation placement, including bidi
    /// and vertical layouts. Nested annotations remain on the view's child [`Line`].
    /// Without ruby annotations, the existing visual order is unchanged.
    pub fn fragments(&self) -> impl ExactSizeIterator<Item = Fragment<'_>> + '_ {
        (0..self.fragments.len() + self.ruby.len()).map(move |i| self.view(i))
    }

    /// Fragment at `index` in the same sequence as [`Self::fragments`].
    pub fn fragment(&self, index: usize) -> Option<Fragment<'_>> {
        (index < self.fragments.len() + self.ruby.len()).then(|| self.view(index))
    }

    /// Font data of any face used by the paragraph; works without the
    /// `FontCollection`, which the line keeps alive.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        self.data.fonts.font_data(id)
    }

    /// True when the line has no glyphs, atomic inlines, or painted inline
    /// edges. An empty line before a block has zero line advance.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    fn view(&self, index: usize) -> Fragment<'_> {
        if index >= self.fragments.len() {
            return Fragment::RubyAnnotation(RubyAnnotationView {
                record: &self.ruby[index - self.fragments.len()],
            });
        }
        let record = &self.fragments[index];
        let rect = |block_start: f32, block_size: f32, start: f32, size: f32| LogicalRect {
            inline_start: start,
            block_start,
            inline_size: size,
            block_size,
        };
        match &record.kind {
            RecordKind::Glyphs {
                run,
                glyphs,
                item,
                text,
                source,
            } => Fragment::GlyphRun(GlyphRunView {
                source: *source,
                block_shift: self.block_shifts[index],
                line: self,
                record,
                run: *run,
                glyphs: match source {
                    GlyphSource::Shared => (glyphs.start, glyphs.end),
                    GlyphSource::Overlay { glyphs, .. } => *glyphs,
                },
                item: *item,
                text: (text.start, text.end),
            }),
            RecordKind::Atomic { node, size, .. } => {
                let margin_block = size.margins.block_start + size.margins.block_end;
                let height = size.block_size + margin_block;
                let kind = self.data.baseline_kind(*node);
                // Missing baselines are synthesized from the margin box
                // (CSS Inline 3): bottom for alphabetic, middle for central.
                let baseline_from_top = size.baseline.unwrap_or(match kind {
                    Some(BaselineKind::Central) => height / 2.0,
                    _ => height,
                });
                let line_baseline = (self.baseline + self.block_shifts[index]).to_f32();
                let top = line_baseline - baseline_from_top;
                let start = record.inline_start.to_f32();
                let width = record.inline_size.to_f32();
                let m = size.margins;
                Fragment::Atomic(AtomicFragment {
                    node: *node,
                    margin_rect: rect(top, height, start, width),
                    border_rect: rect(
                        top + m.block_start,
                        size.block_size,
                        start + m.inline_start,
                        width - m.inline_start - m.inline_end,
                    ),
                    baseline: line_baseline,
                })
            }
            RecordKind::InlineBox {
                box_index,
                start_edge,
                end_edge,
                parent,
                reversed,
            } => {
                let info = &self.data.boxes[*box_index as usize];
                let resolved = self.data.style_metrics[info.style as usize];
                let font = resolved.font;
                let m = resolved.metrics;
                let e = info.edges;
                let pick = |on: bool, v: f32| if on { v } else { 0.0 };
                let margin_start = pick(*start_edge, e.margin.inline_start);
                let margin_end = pick(*end_edge, e.margin.inline_end);
                let inner_start = pick(*start_edge, e.border.inline_start + e.padding.inline_start);
                let inner_end = pick(*end_edge, e.border.inline_end + e.padding.inline_end);
                // A box whose direction opposes the paragraph's has its start
                // edge on the inline-end side.
                let (lead_margin, trail_margin, lead_inner, trail_inner) = if *reversed {
                    (margin_end, margin_start, inner_end, inner_start)
                } else {
                    (margin_start, margin_end, inner_start, inner_end)
                };
                let border_start = record.inline_start.to_f32() + lead_margin;
                let border_size = record.inline_size.to_f32() - lead_margin - trail_margin;
                let content_top = (self.baseline + self.block_shifts[index]).to_f32() - m.ascent;
                let content_height = m.ascent + m.descent;
                let above = e.padding.block_start + e.border.block_start;
                let below = e.padding.block_end + e.border.block_end;
                Fragment::InlineBox(InlineBoxFragment {
                    node: info.node,
                    rect: rect(
                        content_top - above,
                        content_height + above + below,
                        border_start,
                        border_size,
                    ),
                    content_rect: rect(
                        content_top,
                        content_height,
                        border_start + lead_inner,
                        border_size - lead_inner - trail_inner,
                    ),
                    has_start_edge: *start_edge,
                    has_end_edge: *end_edge,
                    parent: parent.map(|p| p as usize),
                    font,
                    font_size: resolved.size,
                })
            }
            RecordKind::Anchor { node, kind } => Fragment::OutOfFlowAnchor(AnchorFragment {
                node: *node,
                kind: *kind,
                inline_position: record.inline_start.to_f32(),
            }),
        }
    }
}
