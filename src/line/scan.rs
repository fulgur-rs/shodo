use super::Scan;
use crate::analysis::units::{BreakClass, Unit, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::BreakReason;
use crate::paragraph::{AtomicSizes, ParagraphData};
use crate::style::TabSize;

#[allow(clippy::too_many_arguments)]
pub(super) fn scan(
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
    let prefix = super::decoration::width(data, start, true, sat);
    let mut pos = indent.add(prefix, sat);
    let mut last_break: Option<(usize, bool)> = None;
    let mut taken_hyphen = None;
    let mut first_hyphen = None;
    let mut last_emergency: Option<usize> = None;
    let mut overflowing = false;
    let mut hanging = LayoutUnit::ZERO;
    let mut spacing = super::spacing_summary::Cursor::default();
    let mut kept_spacing = LayoutUnit::ZERO;
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
        let w = unit_width_from(
            data,
            unit,
            units[start].text.start,
            offset
                .add(pos, sat)
                .add(spacing.summary(Some(data)).width(sat), sat),
            atomics,
            cx,
            sat,
        )
        .add(data.unit_spacing[i].word, sat);
        super::spacing::push(data, &mut spacing, i);
        let tracking = spacing.summary(Some(data)).width(sat);
        // Trailing spaces hang and never cause a break (CSS Text 3 §4.1.3).
        let hangs = matches!(unit.kind, UnitKind::Cluster { space: true, .. });
        let suffix = super::decoration::width(data, i + 1, false, sat);
        let (edge_delta, viable) = super::windows::candidate(data, start, i + 1, cx, sat);
        let transparent = matches!(
            unit.kind,
            UnitKind::Close { .. }
                | UnitKind::BidiControl
                | UnitKind::Float { .. }
                | UnitKind::Absolute { .. }
        );
        let extent = pos
            .add(w, sat)
            .add(tracking, sat)
            .add(edge_delta, sat)
            .add(suffix, sat)
            .sub(
                if transparent {
                    hanging.add(tracking.sub(kept_spacing, sat), sat)
                } else {
                    LayoutUnit::ZERO
                },
                sat,
            );
        if !hangs && !overflowing && extent > available {
            if let Some((b, edge)) = last_break.take() {
                taken_hyphen = edge.then_some(b);
                widths.truncate(b - start);
                i = b;
                break BreakReason::Regular;
            }
            if let Some(b) = last_emergency {
                widths.truncate(b - start);
                i = b;
                break BreakReason::Emergency;
            }
            if let Some(b) = first_hyphen.take() {
                taken_hyphen = Some(b);
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
        match unit.kind {
            UnitKind::Cluster { space: true, .. } => hanging = hanging.add(w, sat),
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. } => {}
            _ => {
                hanging = LayoutUnit::ZERO;
                kept_spacing = tracking;
            }
        }
        if hanging == LayoutUnit::ZERO {
            kept_spacing = tracking;
        }
        let required = pos
            .add(kept_spacing, sat)
            .add(edge_delta, sat)
            .sub(hanging, sat)
            .add(suffix, sat);
        i += 1;
        match unit.break_after {
            BreakClass::Mandatory => break BreakReason::Forced,
            BreakClass::Allowed if viable => {
                if overflowing {
                    break BreakReason::Regular;
                }
                if required <= available {
                    last_break = Some((i, false));
                }
            }
            BreakClass::Hyphen => {
                if let Some(windows) = super::hyphen::line(data, start, i, cx, sat) {
                    let required = pos
                        .add(super::spacing::hyphen(data, &spacing, i - 1, sat), sat)
                        .sub(hanging, sat)
                        .add(suffix, sat)
                        .add(super::windows::cost(&windows, i, sat), sat);
                    if overflowing {
                        taken_hyphen = Some(i);
                        break BreakReason::Regular;
                    }
                    if required <= available {
                        last_break = Some((i, true));
                    } else if first_hyphen.is_none() {
                        first_hyphen = Some(i);
                    }
                }
            }
            BreakClass::Emergency if viable => {
                if overflowing {
                    break BreakReason::Emergency;
                }
                if required <= available {
                    last_emergency = Some(i);
                }
            }
            _ => {}
        }
    };
    // Inline box ends right after a soft break stay on the line that ends
    // there, together with the zero-width bidi controls (PDI, PDF) that
    // precede a box's end. Out-of-flow anchors are not pulled.
    if matches!(reason, BreakReason::Regular | BreakReason::Emergency) {
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
    let mut hang_start = i;
    for (k, unit) in units[start..i].iter().enumerate().rev() {
        match unit.kind {
            UnitKind::Cluster { space: true, .. } => {
                trailing = trailing.add(widths[k], sat);
                hang_start = start + k;
            }
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak => {}
            _ => break,
        }
    }
    let mut result = Scan {
        prepared: false,
        overlays: Vec::new(),
        end: i,
        reason,
        widths,
        leading: None,
        autospace_gaps: Vec::new(),
        content: total
            .sub(trailing, sat)
            .add(prefix, sat)
            .add(super::decoration::width(data, i, false, sat), sat),
        hang_start,
    };
    if let Some(end) = taken_hyphen
        && let Some(windows) = super::hyphen::line(data, start, end, cx, sat)
    {
        super::reshape::apply_windows(data, start, &mut result, windows, cx, sat);
    }
    let visible_hyphen = result
        .overlays
        .iter()
        .find_map(|w| w.hyphen.as_ref().map(|t| t.start));
    result.content = result.content.add(
        super::spacing::width(data, start, result.hang_start, visible_hyphen, sat),
        sat,
    );
    result
}

/// A continuation inside a shaping cluster must use its own exact prefix
/// shapes, rather than subtracting advances measured from the old line start.
#[allow(clippy::too_many_arguments)]
pub(super) fn unit_width_from(
    data: &ParagraphData,
    unit: &Unit,
    line_start: u32,
    content_pos: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    if let Some(shared) = &unit.shared_cluster
        && line_start > shared.text.start
        && line_start < shared.text.end
    {
        let measure = |end: u32, cx: &mut LayoutContext, sat: &mut Saturation| {
            if end <= line_start {
                return Some(LayoutUnit::ZERO);
            }
            let mut prefix = unit.clone();
            prefix.text = line_start..end;
            crate::shape::shape_line_edge(data, &prefix, cx, sat).map(|store| {
                store
                    .advance
                    .iter()
                    .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat))
            })
        };
        if let Some(end) = measure(unit.text.end, cx, sat)
            && let Some(begin) = measure(unit.text.start, cx, sat)
        {
            return end.sub(begin, sat);
        }
    }
    unit_width(data, unit, content_pos, atomics, cx, sat)
}

/// Inline advance of one unit. `content_pos` is the unit's position from the
/// content edge of the block container (tab stops are measured from it).
pub(super) fn unit_width(
    data: &ParagraphData,
    unit: &Unit,
    content_pos: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    match &unit.kind {
        UnitKind::Cluster { .. } if unit.shared_cluster.is_some() => unit.slice_advance,
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
                let size = crate::sanitize::atomic(*size, &mut cx.warnings, sat);
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

/// Tab-size numbers use the nearest block's actual space and spacing.
/// A stop closer than half a ch is skipped (CSS Text 3 §4.1.2/4.2).
fn tab_width(
    data: &ParagraphData,
    unit: &Unit,
    content_pos: LayoutUnit,
    sat: &mut Saturation,
) -> LayoutUnit {
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    let block = &data.styles[0];
    let metrics = data.style_metrics[0];
    let interval = match style.tab_size {
        TabSize::Spaces(n) => n * (metrics.space + block.letter_spacing + block.word_spacing),
        TabSize::Px(v) => v,
    };
    let interval = i64::from(LayoutUnit::from_f32_round(interval, sat).raw());
    if interval <= 0 {
        return LayoutUnit::ZERO;
    }
    let x = i64::from(content_pos.raw());
    let mut next = (x.div_euclid(interval) + 1) * interval;
    let min_gap = i64::from(LayoutUnit::from_f32_round(metrics.ch * 0.5, sat).raw());
    if next - x < min_gap {
        next += interval;
    }
    LayoutUnit::from_raw((next - x).clamp(0, i64::from(i32::MAX)) as i32)
}
