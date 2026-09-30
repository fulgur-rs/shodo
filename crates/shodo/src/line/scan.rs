use super::Scan;
use super::punctuation::removed;
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
    flags: u8,
    options: &crate::style::LineOptions,
    atomics: &AtomicSizes,
    max_graphemes: Option<usize>,
    normal_cursors: Option<&[Option<u32>]>,
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
    let mut graphemes_seen = 0usize;
    let mut counted_through = units[start].text.start;
    let max_graphemes = max_graphemes.map(|limit| limit.max(1));
    let count_mode = max_graphemes.is_some();
    let reason = loop {
        let Some(unit) = units.get(i) else {
            break BreakReason::End;
        };
        match unit.kind {
            UnitKind::ForcedBreak if !count_mode => {
                widths.push(LayoutUnit::ZERO);
                i += 1;
                break BreakReason::Forced;
            }
            UnitKind::BlockInInline { .. } => break BreakReason::BlockInInline,
            _ => {}
        }
        if max_graphemes.is_some() {
            let count = match unit.kind {
                UnitKind::Cluster { .. } => {
                    let from = counted_through.max(unit.text.start);
                    counted_through = counted_through.max(unit.text.end);
                    data.breaks
                        .graphemes
                        .partition_point(|cut| *cut <= unit.text.end)
                        .saturating_sub(data.breaks.graphemes.partition_point(|cut| *cut <= from))
                }
                UnitKind::Atomic { .. } | UnitKind::Tab | UnitKind::ForcedBreak => {
                    counted_through = counted_through.max(unit.text.end);
                    1
                }
                _ => 0,
            };
            graphemes_seen = graphemes_seen.saturating_add(count);
        }
        let cut = unit.text.end;
        let combined = data
            .combine_spans
            .partition_point(|span| span.text.end <= cut);
        let character_cut = max_graphemes.is_some()
            && graphemes_seen > 0
            && i + 1 < units.len()
            && data.breaks.caret_cuts.binary_search(&cut).is_ok()
            && normal_cursors.is_none_or(|cursors| cursors.get(i + 1).is_some_and(Option::is_some))
            && data
                .combine_spans
                .get(combined)
                .is_none_or(|span| span.text.start >= cut);
        let limited_cut =
            character_cut && max_graphemes.is_some_and(|limit| graphemes_seen >= limit);
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
        // Fit excludes eligible trailing space/tab advances (CSS Text 3 §4.1.2).
        let hangs = super::whitespace::fits_hanging(data, i);
        let suffix = super::decoration::width(data, i + 1, false, sat);
        let ruby_delta =
            crate::ruby::measure::candidate(data, start, i + 1, atomics, cx, sat).adjustment;
        let transparent =
            super::whitespace::transparent(data, i) && !matches!(unit.kind, UnitKind::ForcedBreak);
        let shared_extent = pos
            .add(w, sat)
            .add(tracking, sat)
            .add(ruby_delta, sat)
            .add(suffix, sat)
            .sub(
                if transparent {
                    hanging.add(tracking.sub(kept_spacing, sat), sat)
                } else {
                    LayoutUnit::ZERO
                },
                sat,
            );
        // The line cannot end at a `Prohibited` position, so the only
        // reason to pay for a real edge-window reshape there is to confirm
        // genuine overflow of the current unbreakable run. Skip it while
        // shared (un-rejoined) advances already fit: cursive scripts mark
        // most units (and the spaces between words) unsafe to break/concat,
        // so probing every one of them would reshape the growing unsafe
        // window from scratch at each unit instead of once at the eventual
        // break opportunity. A `shared_cluster` unit's shared advance can
        // under-count a window that turns out unshapeable within budget
        // (the edge falls back to wider un-sliced glyphs), so those always
        // get the real measurement.
        let need_edge = if count_mode {
            limited_cut
        } else {
            unit.break_after != BreakClass::Prohibited
                || unit.shared_cluster.is_some()
                || (!hangs && !overflowing && shared_extent > available)
        };
        let (edge_delta, viable) = if need_edge {
            super::windows::candidate(data, start, i + 1, cx, sat)
        } else {
            (LayoutUnit::ZERO, false)
        };
        let extent = shared_extent.add(edge_delta, sat);
        // Edge adjustments only ever remove width (see `EdgeAdjustment`), so a
        // candidate that already fits cannot start to overflow.
        let extent = if !count_mode && !hangs && !overflowing && extent > available {
            let adjustment = super::punctuation::edges(
                data,
                spacing.summary(Some(data)),
                flags,
                super::punctuation::last_edge(data, i + 1),
                options,
                LayoutUnit::ZERO,
                extent,
                sat,
            );
            extent.sub(adjustment.removed(sat), sat)
        } else {
            extent
        };
        if !count_mode && !hangs && !overflowing && extent > available {
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
            _ if hangs => hanging = hanging.add(w, sat),
            _ if super::whitespace::transparent(data, i) => {}
            _ => {
                hanging = LayoutUnit::ZERO;
                kept_spacing = tracking;
            }
        }
        if hanging == LayoutUnit::ZERO {
            kept_spacing = tracking;
        }
        i += 1;
        if count_mode {
            if limited_cut && viable {
                break BreakReason::Regular;
            }
            continue;
        }
        let required = pos
            .add(kept_spacing, sat)
            .add(edge_delta, sat)
            .add(ruby_delta, sat)
            .sub(hanging, sat)
            .add(suffix, sat);
        // Only break candidates compare `required` against the width, so the
        // edge adjustment is computed for those alone.
        let last = super::punctuation::last_edge(data, i);
        match unit.break_after {
            BreakClass::Mandatory => break BreakReason::Forced,
            BreakClass::Allowed if viable => {
                if overflowing {
                    break BreakReason::Regular;
                }
                if required.sub(
                    removed(data, &mut spacing, flags, last, options, required, sat),
                    sat,
                ) <= available
                {
                    last_break = Some((i, false));
                }
            }
            BreakClass::Hyphen => {
                if let Some(windows) = super::hyphen::line(data, start, i, cx, sat) {
                    let summary = super::spacing::hyphen_summary(data, &spacing, i - 1, sat);
                    let required = pos
                        .add(summary.width(sat), sat)
                        .sub(hanging, sat)
                        .add(suffix, sat)
                        .add(super::windows::cost(&windows, i, sat), sat);
                    let required = required.add(ruby_delta, sat);
                    let adjustment = super::punctuation::edges(
                        data,
                        summary,
                        flags,
                        false,
                        options,
                        LayoutUnit::ZERO,
                        required,
                        sat,
                    );
                    let required = required.sub(adjustment.removed(sat), sat);
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
                if required.sub(
                    removed(data, &mut spacing, flags, last, options, required, sat),
                    sat,
                ) <= available
                {
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
            && super::pulls_after_break(data, i)
        {
            widths.push(unit_width(data, unit, LayoutUnit::ZERO, atomics, cx, sat));
            i += 1;
        }
    }
    let total = widths
        .iter()
        .fold(LayoutUnit::ZERO, |acc, w| acc.add(*w, sat));
    let (hang_start, trailing) = super::whitespace::trailing(data, start, i, &widths, sat);
    let mut result = Scan {
        ruby: None,
        ruby_caret_gaps: Vec::new(),
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
        hanging_end: LayoutUnit::ZERO,
        punctuation_edges: Default::default(),
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
    super::punctuation::prepare(
        data,
        start,
        &mut result,
        flags,
        options,
        available,
        indent,
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
    if unit.combine.is_some() {
        return unit.slice_advance;
    }
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
pub(super) fn tab_width(
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
    tab_advance(
        content_pos,
        LayoutUnit::from_f32_round(interval, sat),
        LayoutUnit::from_f32_round(metrics.ch * 0.5, sat),
    )
}

pub(crate) fn tab_advance(
    content_pos: LayoutUnit,
    interval: LayoutUnit,
    min_gap: LayoutUnit,
) -> LayoutUnit {
    let interval = i64::from(interval.raw());
    if interval <= 0 {
        return LayoutUnit::ZERO;
    }
    let x = i64::from(content_pos.raw());
    let mut next = (x.div_euclid(interval) + 1) * interval;
    let min_gap = i64::from(min_gap.raw());
    if next - x < min_gap {
        next += interval;
    }
    LayoutUnit::from_raw((next - x).clamp(0, i64::from(i32::MAX)) as i32)
}
