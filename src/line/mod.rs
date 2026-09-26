//! Line breaking.

pub(crate) mod fragments;

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
}

impl Paragraph {
    /// Lays out the line starting at `token`. Pure: the same inputs always
    /// give the same result, so a token can be retried with other
    /// constraints.
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
                flags: BreakToken::AFTER_FORCED,
            };
            return LineResult::BlockInInline { node, token_after };
        }
        let mut sat = Saturation::default();
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
        let indent = text_indent(options, token.flags, &mut sat);
        let scan = scan(
            data, start, available, offset, indent, atomics, cx, &mut sat,
        );
        let origin = offset.add(indent, &mut sat);
        let line = Line::new(
            self,
            token,
            scan,
            origin,
            constraint.block_offset,
            atomics,
            &mut sat,
        );
        cx.warnings.record_saturation(&sat);
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

#[allow(clippy::too_many_arguments)]
fn scan(
    data: &ParagraphData,
    start: usize,
    available: LayoutUnit,
    offset: LayoutUnit,
    indent: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Scan {
    let units = &data.units;
    let mut widths = Vec::new();
    let mut pos = indent;
    let mut last_break: Option<usize> = None;
    let mut overflowing = false;
    let mut i = start;
    let reason = loop {
        let Some(unit) = units.get(i) else {
            break BreakReason::End;
        };
        match unit.kind {
            UnitKind::ForcedBreak => {
                widths.push(LayoutUnit::ZERO);
                i += 1;
                break BreakReason::Forced;
            }
            UnitKind::BlockInInline { .. } => break BreakReason::BlockInInline,
            _ => {}
        }
        let w = unit_width(data, unit, offset.add(pos, sat), atomics, cx, sat);
        // Trailing spaces hang and never cause a break (CSS Text 3 §4.1.3).
        let hangs = matches!(unit.kind, UnitKind::Cluster { space: true, .. });
        if !hangs && !overflowing && i > start && pos.add(w, sat) > available {
            if let Some(b) = last_break {
                widths.truncate(b - start);
                i = b;
                break BreakReason::Regular;
            }
            // No opportunity yet: the unbreakable run overflows
            // (`overflow-wrap: normal`) and the line ends at the next one.
            overflowing = true;
        }
        widths.push(w);
        pos = pos.add(w, sat);
        i += 1;
        if unit.break_after == BreakClass::Allowed {
            if overflowing {
                break BreakReason::Regular;
            }
            last_break = Some(i);
        }
    };
    // Inline box ends right after a soft break stay on the line that ends
    // there, together with the zero-width bidi controls (PDI, PDF) that
    // precede a box's end. Out-of-flow anchors are not pulled.
    if reason == BreakReason::Regular {
        while let Some(unit) = units.get(i)
            && matches!(unit.kind, UnitKind::Close { .. } | UnitKind::BidiControl)
        {
            widths.push(unit_width(data, unit, LayoutUnit::ZERO, atomics, cx, sat));
            i += 1;
        }
    }
    let total = widths
        .iter()
        .fold(LayoutUnit::ZERO, |acc, w| acc.add(*w, sat));
    let mut trailing = LayoutUnit::ZERO;
    for (k, unit) in units[start..i].iter().enumerate().rev() {
        match unit.kind {
            UnitKind::Cluster { space: true, .. } => trailing = trailing.add(widths[k], sat),
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak => {}
            _ => break,
        }
    }
    Scan {
        end: i,
        reason,
        widths,
        content: total.sub(trailing, sat),
    }
}

/// Inline advance of one unit. `content_pos` is the unit's position from the
/// content edge of the block container (tab stops are measured from it).
fn unit_width(
    data: &ParagraphData,
    unit: &Unit,
    content_pos: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    match &unit.kind {
        UnitKind::Cluster { glyphs, .. } => glyphs.clone().fold(LayoutUnit::ZERO, |acc, g| {
            acc.add(data.glyphs.advance[g as usize], sat)
        }),
        UnitKind::Open { box_index } => LayoutUnit::from_f32_round(
            data.boxes[*box_index as usize].edges.inline_start_total(),
            sat,
        ),
        UnitKind::Close { box_index } => LayoutUnit::from_f32_round(
            data.boxes[*box_index as usize].edges.inline_end_total(),
            sat,
        ),
        UnitKind::Atomic { node } => match atomics.get(*node) {
            Some(size) => {
                let inline = if size.inline_size < 0.0 {
                    cx.warnings.push(
                        WarningKind::NegativeInput,
                        "negative atomic inline size replaced with 0",
                    );
                    0.0
                } else {
                    size.inline_size
                };
                LayoutUnit::from_f32_round(inline + size.margins.inline_sum(), sat)
            }
            None => {
                cx.warnings.push(
                    WarningKind::MissingAtomicSize,
                    format!("no size for atomic inline {node:?}"),
                );
                LayoutUnit::ZERO
            }
        },
        UnitKind::Tab => tab_width(data, unit, content_pos, sat),
        _ => LayoutUnit::ZERO,
    }
}

/// Distance to the next tab stop (CSS Text 3 §4.2). The placeholder shaper
/// gives the space character a 1em advance.
fn tab_width(
    data: &ParagraphData,
    unit: &Unit,
    content_pos: LayoutUnit,
    sat: &mut Saturation,
) -> LayoutUnit {
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    let interval = match style.tab_size {
        TabSize::Spaces(n) => n * style.font_size,
        TabSize::Px(v) => v,
    };
    let interval = i64::from(LayoutUnit::from_f32_round(interval, sat).raw());
    if interval <= 0 {
        return LayoutUnit::ZERO;
    }
    let x = i64::from(content_pos.raw());
    let next = (x.div_euclid(interval) + 1) * interval;
    LayoutUnit::from_raw((next - x).clamp(0, i64::from(i32::MAX)) as i32)
}
