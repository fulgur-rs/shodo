//! Line breaking.

mod align;
mod decoration;
pub(crate) mod fragments;
pub(crate) mod metrics;
mod scan;
use scan::scan;

use crate::analysis::units::{BreakClass, Unit, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::{BreakReason, Line};
use crate::paragraph::{
    AtomicSizes, BreakToken, LineConstraint, LineResult, Paragraph, ParagraphData,
};
use crate::style::{LineOptions, TabSize};

/// Result of scanning one line.
pub(crate) struct Scan {
    pub(crate) end: usize,
    pub(crate) reason: BreakReason,
    /// Width of every unit in the line, in order.
    pub(crate) widths: Vec<LayoutUnit>,
    /// Content width, excluding text-indent and hanging trailing spaces.
    pub(crate) content: LayoutUnit,
    /// Index of the first hanging trailing space of the line (`end` when
    /// there is none). Units from here on are hanging spaces, inline box
    /// ends, bidi controls, out-of-flow anchors or a forced break.
    pub(crate) hang_start: usize,
}

impl Paragraph {
    /// Lays out the line starting at `token`. Pure: the same inputs always
    /// give the same result, so a token can be retried with other
    /// constraints.
    /// If a line cannot fit even at the top of a page, retry the same token
    /// with `max_block_size: None` to accept the overflowing line.
    pub fn next_line(
        &self,
        cx: &mut LayoutContext,
        token: BreakToken,
        options: &LineOptions,
        constraint: &LineConstraint<'_>,
        atomics: &AtomicSizes,
    ) -> LineResult {
        let data = &*self.data;
        cx.warnings.set_max(data.limits.max_warnings);
        if token.para != data.id || token.unit as usize > data.units.len() {
            return LineResult::InvalidToken;
        }
        let start = token.unit as usize;
        if start == data.units.len() {
            return LineResult::Done;
        }
        if let UnitKind::BlockInInline { node } = data.units[start].kind {
            let token_after = BreakToken {
                para: data.id,
                unit: token.unit + 1,
                flags: 0,
            };
            return LineResult::BlockInInline { node, token_after };
        }
        let mut sat = Saturation::default();
        let constraint = crate::sanitize::constraint(*constraint, &mut cx.warnings, &mut sat);
        let available = non_negative(
            constraint.available_inline_size,
            "available_inline_size",
            cx,
            &mut sat,
        );
        let offset = non_negative(
            constraint.inline_start_offset,
            "inline_start_offset",
            cx,
            &mut sat,
        );
        let mut options = *options;
        options.text_indent.length = crate::sanitize::layout_length(
            options.text_indent.length,
            false,
            &mut cx.warnings,
            &mut sat,
        );
        let indent = text_indent(&options, token.flags, &mut sat);
        let mut scan = scan(
            data, start, available, offset, indent, atomics, cx, &mut sat,
        );
        let alignment = align::apply(
            data, start, &mut scan, &options, available, indent, &mut sat,
        );
        let origin = offset.add(indent, &mut sat).add(alignment.shift, &mut sat);
        let mut line = Line::new(
            self,
            token,
            scan,
            origin,
            constraint.block_offset,
            atomics,
            &mut sat,
        );
        line.positions = alignment.positions;
        cx.warnings.record_saturation(&sat);
        if constraint
            .max_block_size
            .is_some_and(|max| line.block_size() > max)
        {
            return LineResult::BlockSizeExceeded {
                needed_block_size: line.block_size(),
            };
        }
        LineResult::Line(line)
    }
}

fn non_negative(
    value: f32,
    what: &str,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    let v = LayoutUnit::from_f32_round(value, sat);
    if v < LayoutUnit::ZERO {
        cx.warnings.push(
            WarningKind::NegativeInput,
            format!("negative {what} replaced with 0"),
        );
        LayoutUnit::ZERO
    } else {
        v
    }
}

/// CSS Text 3 §8.1: the first line (and, with `each-line`, lines after a
/// forced break) is indented; `hanging` inverts which lines are.
fn text_indent(options: &LineOptions, flags: u8, sat: &mut Saturation) -> LayoutUnit {
    let ti = options.text_indent;
    let first = flags & BreakToken::FIRST_LINE != 0;
    let after_forced = flags & BreakToken::AFTER_FORCED != 0;
    let applies = first || (ti.each_line && after_forced);
    if applies != ti.hanging {
        LayoutUnit::from_f32_round(ti.length, sat)
    } else {
        LayoutUnit::ZERO
    }
}
