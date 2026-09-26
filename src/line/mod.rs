//! Line breaking.

mod align;
pub(crate) mod cache;
mod decoration;
pub(crate) mod fragments;
mod intrinsic;
mod iter;
pub(crate) mod metrics;
mod plan;
mod reshape;
mod scan;

use crate::analysis::units::UnitKind;
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::{BreakReason, Line};
use crate::paragraph::{
    AtomicSizes, BreakToken, FloatCursor, LineConstraint, LineResult, Paragraph,
};
use crate::style::LineOptions;

/// Result of scanning one line.
#[derive(Clone, Debug)]
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
        let planned_end = constraint.break_plan.and_then(|p| {
            if plan::matches(p, self, &options, &constraint, atomics) {
                let index = p.ends.partition_point(|end| *end <= token.unit);
                let valid_start = token.unit == 0
                    || index > 0 && p.ends[index - 1] == token.unit
                    || start > 0
                        && matches!(data.units[start - 1].kind, UnitKind::BlockInInline { .. })
                        && p.ends.binary_search(&(token.unit - 1)).is_ok();
                valid_start
                    .then(|| p.ends.get(index).copied())
                    .flatten()
                    .map(|end| end as usize)
            } else {
                cx.warnings.push(
                    WarningKind::Unsupported,
                    "break plan inputs mismatch; using greedy layout",
                );
                None
            }
        });
        let mut scan = if let Some(end) = planned_end {
            plan::selected(data, start, end, offset, indent, atomics, cx, &mut sat)
        } else {
            match cache::resolve(
                self,
                token,
                &options,
                &constraint,
                available,
                offset,
                indent,
                atomics,
                cx,
                &mut sat,
            ) {
                Ok(scan) => scan,
                Err((node, ordinal, position)) => {
                    cx.warnings.record_saturation(&sat);
                    return LineResult::FloatEncountered {
                        node,
                        line_start: token,
                        inline_position: position.to_f32(),
                        float_cursor: FloatCursor(ordinal),
                    };
                }
            }
        };
        // Select the break before reporting an anchor: floats do not create
        // opportunities, and a word containing one may belong to the next line.
        let mut float_pos = indent.add(decoration::width(data, start, true, &mut sat), &mut sat);
        for (u, w) in data.units[start..scan.end].iter().zip(&scan.widths) {
            if let UnitKind::Float { node, ordinal } = u.kind
                && constraint
                    .floats_placed_through
                    .is_none_or(|c| ordinal > c.0)
            {
                cx.warnings.record_saturation(&sat);
                return LineResult::FloatEncountered {
                    node,
                    line_start: token,
                    inline_position: float_pos.to_f32(),
                    float_cursor: FloatCursor(ordinal),
                };
            }
            float_pos = float_pos.add(*w, &mut sat);
        }
        let displaced: Vec<_> = data.units[scan.end..]
            .iter()
            .filter_map(|u| {
                if let UnitKind::Float { node, ordinal } = u.kind
                    && constraint
                        .floats_placed_through
                        .is_some_and(|c| ordinal <= c.0)
                {
                    Some((node, FloatCursor(ordinal)))
                } else {
                    None
                }
            })
            .collect();
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
        line.displaced = displaced;
        reshape::apply(&mut line, cx, &mut sat);
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
