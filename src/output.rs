//! Line layout output.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use peniko::FontData;

use crate::font::FontId;
use crate::geometry::{BaselineKind, LayoutUnit, LogicalRect, Saturation};
use crate::line::Scan;
use crate::line::fragments::{self, FragmentRecord, RecordKind};
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{AtomicSizes, BreakToken, FloatCursor, Paragraph, ParagraphData};
use crate::style::LineHeight;

/// Why a line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    /// At a soft break opportunity.
    Regular,
    /// At a forced break (`<br>`, preserved newline).
    Forced,
    /// Inside a word because of `overflow-wrap` (not produced yet).
    Emergency,
    /// Before a block-level box inside inline content.
    BlockInInline,
    /// At the end of the paragraph.
    End,
}

/// One laid-out line. Owns a reference to its paragraph's data, so it can
/// outlive the `Paragraph` handle and be sent between threads.
#[derive(Clone)]
pub struct Line {
    pub(crate) data: Arc<ParagraphData>,
    pub(crate) break_token: BreakToken,
    pub(crate) reason: BreakReason,
    pub(crate) units: Range<u32>,
    pub(crate) origin: LayoutUnit,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) baseline: LayoutUnit,
    pub(crate) ascent: LayoutUnit,
    pub(crate) descent: LayoutUnit,
    pub(crate) block_offset: f32,
    pub(crate) displaced: Vec<(NodeId, FloatCursor)>,
    pub(crate) fragments: Vec<FragmentRecord>,
    pub(crate) block_shifts: Vec<LayoutUnit>,
    pub(crate) empty: bool,
}

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
    pub(crate) fn new(
        para: &Paragraph,
        token: BreakToken,
        scan: Scan,
        origin: LayoutUnit,
        block_offset: f32,
        atomics: &AtomicSizes,
        sat: &mut Saturation,
    ) -> Line {
        let data = &para.data;
        let root = &data.styles[0];
        let size = root.font_size;
        let m = data.fonts.metrics(data.fonts.primary_font(), size);
        let line_height = match root.line_height {
            LineHeight::Normal => m.ascent + m.descent + m.line_gap,
            LineHeight::Px(v) => v,
            LineHeight::Number(n) => n * size,
        };
        let flags = match scan.reason {
            BreakReason::Forced => BreakToken::AFTER_FORCED,
            _ => 0,
        };
        let origin_units = token.unit as usize..scan.end;
        let records = fragments::build(
            data,
            origin_units,
            scan.hang_start,
            &scan.widths,
            origin,
            atomics,
        );
        let metrics =
            crate::line::metrics::measure(data, token.unit as usize..scan.end, &records, sat);
        Line {
            data: Arc::clone(&para.data),
            break_token: BreakToken {
                para: data.id,
                unit: scan.end as u32,
                flags,
            },
            reason: scan.reason,
            units: token.unit..scan.end as u32,
            origin,
            inline_size: scan.content,
            block_size: if scan.reason == BreakReason::Forced && metrics.empty {
                LayoutUnit::from_f32_ceil(line_height, sat)
            } else {
                metrics.block_size
            },
            baseline: metrics.baseline,
            ascent: LayoutUnit::from_f32_round(m.ascent, sat),
            descent: LayoutUnit::from_f32_round(m.descent, sat),
            block_offset,
            displaced: Vec::new(),
            fragments: records,
            block_shifts: metrics.shifts,
            empty: metrics.empty,
        }
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

    /// Line advance (distance to the next line).
    pub fn block_size(&self) -> f32 {
        self.block_size.to_f32()
    }

    pub fn block_offset(&self) -> f32 {
        self.block_offset
    }

    /// Position of a baseline, from the top of the line box. Only the
    /// alphabetic baseline comes from font data; the others are derived from
    /// the strut's ascent and descent.
    pub fn baseline(&self, kind: BaselineKind) -> f32 {
        let alphabetic = self.baseline.to_f32();
        let (ascent, descent) = (self.ascent.to_f32(), self.descent.to_f32());
        match kind {
            BaselineKind::Alphabetic => alphabetic,
            BaselineKind::Central => alphabetic - (ascent - descent) / 2.0,
            BaselineKind::Ideographic => alphabetic + descent,
            BaselineKind::Hanging => alphabetic - 0.8 * ascent,
        }
    }

    /// Range of the paragraph's processed text covered by this line.
    pub fn text_range(&self) -> Range<usize> {
        let units = &self.data.units[self.units.start as usize..self.units.end as usize];
        match (units.first(), units.last()) {
            (Some(first), Some(last)) => first.text.start as usize..last.text.end as usize,
            _ => 0..0,
        }
    }

    /// Floats reported for this line whose anchors ended up after its end.
    pub fn displaced_floats(&self) -> &[(NodeId, FloatCursor)] {
        &self.displaced
    }
}

/// A positioned piece of a line.
#[derive(Clone, Copy, Debug)]
pub enum Fragment<'a> {
    GlyphRun(GlyphRunView<'a>),
    Atomic(AtomicFragment),
    InlineBox(InlineBoxFragment),
    OutOfFlowAnchor(AnchorFragment),
}

/// The part of an inline box on one line. With `box-decoration-break:
/// slice`, only the first fragment has the start edge and only the last has
/// the end edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineBoxFragment {
    pub node: NodeId,
    /// Border box. Block sides cover the content area plus block padding and
    /// border, which do not affect the line height (CSS 2.1 §10.8.1).
    pub rect: LogicalRect,
    pub content_rect: LogicalRect,
    pub has_start_edge: bool,
    pub has_end_edge: bool,
    /// Index of the parent inline box fragment in [`Line::fragments`].
    pub parent: Option<usize>,
    /// Primary font of the box, for text-decoration metrics.
    pub font: FontId,
    pub font_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicFragment {
    pub node: NodeId,
    pub margin_rect: LogicalRect,
    pub border_rect: LogicalRect,
    /// Baseline position from the top of the line box.
    pub baseline: f32,
}

/// Static position of an out-of-flow box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorFragment {
    pub node: NodeId,
    pub kind: OutOfFlowKind,
    pub inline_position: f32,
}

/// A run of glyphs from one font, one element and one bidi level.
#[derive(Clone, Copy, Debug)]
pub struct GlyphRunView<'a> {
    block_shift: LayoutUnit,
    line: &'a Line,
    record: &'a FragmentRecord,
    run: u32,
    glyphs: (u32, u32),
    item: u32,
    text: (u32, u32),
}

/// One positioned glyph. `inline_position` is the glyph origin from the
/// container's content edge; `block_offset` is relative to the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub inline_position: f32,
    pub block_offset: f32,
    pub advance: f32,
    pub cluster: u32,
}

impl<'a> GlyphRunView<'a> {
    fn data(&self) -> &'a ParagraphData {
        &self.line.data
    }

    pub fn node(&self) -> Option<NodeId> {
        self.data().items[self.item as usize].node
    }

    pub fn font(&self) -> FontId {
        self.data().runs[self.run as usize].font
    }

    pub fn font_size(&self) -> f32 {
        self.data().runs[self.run as usize].font_size
    }

    pub fn font_data(&self) -> Option<FontData> {
        self.data().fonts.font_data(self.font())
    }

    pub fn bidi_level(&self) -> u8 {
        self.record.level
    }

    pub fn text_range(&self) -> Range<usize> {
        self.text.0 as usize..self.text.1 as usize
    }

    pub fn inline_start(&self) -> f32 {
        self.record.inline_start.to_f32()
    }

    pub fn inline_size(&self) -> f32 {
        self.record.inline_size.to_f32()
    }

    /// Alphabetic baseline from the top of the line box.
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

    fn glyph(&self, g: u32) -> Glyph {
        let store = &self.data().glyphs;
        let gi = g as usize;
        let rel = store.pen[gi] - store.pen[self.glyphs.0 as usize];
        let advance = store.advance[gi];
        // Runs are stored in logical order and reversed for display here; a
        // real shaper that emits right-to-left runs in visual order must not
        // be reversed twice.
        let reversed = self.record.level % 2 != self.line.data.base_level % 2;
        let pen = if reversed {
            self.record.inline_size - rel - advance
        } else {
            rel
        };
        let position = self.record.inline_start + pen + store.offset_inline[gi];
        Glyph {
            id: store.id[gi],
            inline_position: position.to_f32(),
            block_offset: store.offset_block[gi].to_f32(),
            advance: advance.to_f32(),
            cluster: store.cluster[gi],
        }
    }
}

/// Iterator over the glyphs of a run, with random access through `get`.
#[derive(Clone, Debug)]
pub struct Glyphs<'a> {
    view: GlyphRunView<'a>,
    next: u32,
    end: u32,
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

impl Line {
    /// Fragments in visual order.
    pub fn fragments(&self) -> impl ExactSizeIterator<Item = Fragment<'_>> + '_ {
        (0..self.fragments.len()).map(move |i| self.view(i))
    }

    pub fn fragment(&self, index: usize) -> Option<Fragment<'_>> {
        (index < self.fragments.len()).then(|| self.view(index))
    }

    /// Font data of any face used by the paragraph; works without the
    /// `FontCollection`, which the line keeps alive.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        self.data.fonts.font_data(id)
    }

    /// True when the line has no glyphs and no atomic inlines.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    fn view(&self, index: usize) -> Fragment<'_> {
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
            } => Fragment::GlyphRun(GlyphRunView {
                block_shift: self.block_shifts[index],
                line: self,
                record,
                run: *run,
                glyphs: (glyphs.start, glyphs.end),
                item: *item,
                text: (text.start, text.end),
            }),
            RecordKind::Atomic { node, size } => {
                let margin_block = size.margins.block_start + size.margins.block_end;
                let height = size.block_size + margin_block;
                let kind = self
                    .data
                    .baselines
                    .iter()
                    .find(|(n, _)| n == node)
                    .map(|(_, k)| *k);
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
                let style = &self.data.styles[info.style as usize];
                let font = self.data.fonts.primary_font();
                let m = self.data.fonts.metrics(font, style.font_size);
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
                    font_size: style.font_size,
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
