//! Line layout output.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::geometry::{BaselineKind, LayoutUnit, Saturation};
use crate::line::Scan;
use crate::node::NodeId;
use crate::paragraph::{BreakToken, FloatCursor, Paragraph, ParagraphData};
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
    pub(crate) widths: Vec<LayoutUnit>,
    pub(crate) origin: LayoutUnit,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) baseline: LayoutUnit,
    pub(crate) ascent: LayoutUnit,
    pub(crate) descent: LayoutUnit,
    pub(crate) block_offset: f32,
    pub(crate) displaced: Vec<(NodeId, FloatCursor)>,
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
        // CSS 2.1 §10.8.1: half the leading goes above the ascent.
        let half_leading = (line_height - (m.ascent + m.descent)) / 2.0;
        let flags = match scan.reason {
            BreakReason::Forced => BreakToken::AFTER_FORCED,
            _ => 0,
        };
        Line {
            data: Arc::clone(&para.data),
            break_token: BreakToken {
                para: data.id,
                unit: scan.end as u32,
                flags,
            },
            reason: scan.reason,
            units: token.unit..scan.end as u32,
            widths: scan.widths,
            origin,
            inline_size: scan.content,
            block_size: LayoutUnit::from_f32_ceil(line_height, sat),
            baseline: LayoutUnit::from_f32_round(half_leading + m.ascent, sat),
            ascent: LayoutUnit::from_f32_round(m.ascent, sat),
            descent: LayoutUnit::from_f32_round(m.descent, sat),
            block_offset,
            displaced: Vec::new(),
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
