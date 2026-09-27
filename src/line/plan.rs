use super::{Scan, decoration};
use crate::LayoutContext;
use crate::analysis::units::{BreakClass, UnitKind};
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::BreakReason;
use crate::paragraph::{
    AtomicSizes, BreakPlan, BreakToken, LineConstraint, Paragraph, ParagraphData,
};
use crate::style::{LineOptions, TextWrapStyle};

pub(super) fn matches(
    p: &BreakPlan,
    para: &Paragraph,
    options: &LineOptions,
    c: &LineConstraint<'_>,
    atomics: &AtomicSizes,
) -> bool {
    p.para == para.id()
        && p.width == c.available_inline_size
        && p.options == *options
        && p.atomics_generation == atomics.generation()
        && p.atomics_revision == atomics.revision
        && c.inline_start_offset == 0.0
        && para.data.float_count.checked_sub(1).is_none_or(|last| {
            c.floats_placed_through
                .is_some_and(|cursor| cursor.0 >= last)
        })
}

fn hyphen_end(data: &ParagraphData, end: usize) -> Option<usize> {
    if end >= data.units.len() || matches!(data.units[end].kind, UnitKind::BlockInInline { .. }) {
        return None;
    }
    (0..end)
        .rev()
        .find(|i| {
            !matches!(
                data.units[*i].kind,
                UnitKind::Close { .. } | UnitKind::BidiControl
            )
        })
        .filter(|i| data.units[*i].break_after == BreakClass::Hyphen)
        .map(|i| i + 1)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn selected(
    data: &ParagraphData,
    start: usize,
    end: usize,
    offset: LayoutUnit,
    indent: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Scan {
    let prefix = decoration::width(data, start, true, sat);
    let mut pos = indent.add(prefix, sat);
    let hyphen =
        hyphen_end(data, end).and_then(|end| super::hyphen::line(data, start, end, cx, sat));
    let mut widths = Vec::with_capacity(end - start);
    let mut spacing = super::spacing_summary::Cursor::default();
    for (i, u) in data.units[start..end].iter().enumerate() {
        let w = super::scan::unit_width_from(
            data,
            u,
            data.units[start].text.start,
            offset.add(pos, sat).add(spacing.summary().width(sat), sat),
            atomics,
            cx,
            sat,
        )
        .add(data.unit_spacing[start + i].word, sat);
        super::spacing::push(data, &mut spacing, start + i);
        pos = pos.add(w, sat);
        widths.push(w);
    }
    let mut trailing = LayoutUnit::ZERO;
    let mut hang_start = end;
    for i in (start..end).rev() {
        match data.units[i].kind {
            UnitKind::Cluster { space: true, .. } => {
                trailing = trailing.add(widths[i - start], sat);
                hang_start = i;
            }
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak => {}
            _ => break,
        }
    }
    let reason = if end > start && matches!(data.units[end - 1].kind, UnitKind::ForcedBreak) {
        BreakReason::Forced
    } else if end == data.units.len() {
        BreakReason::End
    } else if matches!(data.units[end].kind, UnitKind::BlockInInline { .. }) {
        BreakReason::BlockInInline
    } else {
        super::soft_break_reason(data, start, end)
    };
    let mut scan = Scan {
        prepared: false,
        overlays: Vec::new(),
        end,
        reason,
        widths,
        leading: None,
        content: pos
            .sub(indent, sat)
            .sub(trailing, sat)
            .add(decoration::width(data, end, false, sat), sat),
        hang_start,
    };
    if let Some(windows) = hyphen {
        super::reshape::apply_windows(data, start, &mut scan, windows, cx, sat);
    } else {
        super::reshape::prepare(data, start, &mut scan, cx, sat);
    }
    let visible_hyphen = scan
        .overlays
        .iter()
        .find_map(|w| w.hyphen.as_ref().map(|t| t.start));
    scan.content = scan.content.add(
        super::spacing::width(data, start, hang_start, visible_hyphen, sat),
        sat,
    );
    scan
}

fn line_start(para: &Paragraph, line: &crate::Line) -> usize {
    if std::sync::Arc::ptr_eq(&para.data, &line.data) {
        line.units.start as usize
    } else {
        0
    }
}

fn span_data(para: &Paragraph, begin: usize, end: usize) -> Option<(&ParagraphData, usize, usize)> {
    if begin == 0
        && let Some(first) = &para.data.first_line
    {
        Some((&first.data, 0, first.alternate_cursor(end as u32)?))
    } else {
        Some((&para.data, begin, end))
    }
}

impl Paragraph {
    /// Computes fixed-width breaks. Floats are treated as zero-width anchors;
    /// float insets and unhandled floats make a plan inapplicable at layout.
    /// Balance keeps the greedy line count, while Pretty reduces squared
    /// raggedness within the configured window and never crosses a block or
    /// forced boundary. Zero search limits warn and retain greedy breaks.
    pub fn plan_breaks(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        width: f32,
        atomics: &AtomicSizes,
    ) -> BreakPlan {
        cx.warnings.set_max(self.data.limits.max_warnings);
        let mut sat = Saturation::default();
        let width = crate::sanitize::layout_length(width, true, &mut cx.warnings, &mut sat);
        let mut options = *options;
        options.text_indent.length = crate::sanitize::layout_length(
            options.text_indent.length,
            false,
            &mut cx.warnings,
            &mut sat,
        );
        let greedy = self.break_all(cx, &options, width, atomics);
        let mut ends: Vec<_> = greedy.iter().map(|l| l.break_token().unit).collect();
        match options.text_wrap_style {
            TextWrapStyle::Balance => {
                let limit = self.data.limits.max_balance_iterations.unwrap_or(32);
                if limit == 0 {
                    cx.warnings.push(
                        WarningKind::Unsupported,
                        "balance search disabled; using greedy breaks",
                    );
                } else {
                    let mut lo = 0_i32;
                    let mut hi = LayoutUnit::from_f32_round(width, &mut sat).raw();
                    for _ in 0..limit {
                        if lo >= hi {
                            break;
                        }
                        let mid = lo + (hi - lo) / 2;
                        let lines = self.break_all(
                            cx,
                            &options,
                            LayoutUnit::from_raw(mid).to_f32(),
                            atomics,
                        );
                        if lines.len() <= greedy.len() {
                            hi = mid;
                            ends = lines.iter().map(|l| l.break_token().unit).collect();
                        } else {
                            lo = mid + 1;
                        }
                    }
                }
            }
            TextWrapStyle::Pretty => {
                let window = self
                    .data
                    .limits
                    .max_pretty_window_lines
                    .unwrap_or(greedy.len() as u64)
                    .min(greedy.len() as u64) as usize;
                if window == 0 {
                    cx.warnings.push(
                        WarningKind::Unsupported,
                        "pretty search disabled; using greedy breaks",
                    );
                } else {
                    // Two candidates per line, two predecessor states: the
                    // search stays linear in input size, even without a limit.
                    let mut chunk = 0;
                    while chunk < greedy.len() {
                        let mut stop = (chunk + window).min(greedy.len());
                        if let Some(k) = (chunk..stop).find(|i| greedy[*i].is_last()) {
                            stop = k + 1;
                        }
                        let start = line_start(self, &greedy[chunk]);
                        let mut previous = vec![(start, 0.0_f64)];
                        let mut layers: Vec<Vec<(usize, f64, usize)>> = Vec::new();
                        for (i, line) in greedy.iter().enumerate().take(stop).skip(chunk) {
                            let end = line.break_token().unit as usize;
                            let actual_end = line.units.end as usize;
                            let candidate_data = &line.data;
                            let candidate_start = line.units.start as usize;
                            let mut candidates = vec![end];
                            if i + 1 < stop
                                && let Some(j) = (candidate_start..actual_end.saturating_sub(1))
                                    .rev()
                                    .find(|j| {
                                        let unit = &candidate_data.units[*j];
                                        unit.break_after == BreakClass::Allowed
                                            || unit.break_after == BreakClass::Hyphen
                                                && super::hyphen::line(
                                                    candidate_data,
                                                    candidate_start,
                                                    *j + 1,
                                                    cx,
                                                    &mut sat,
                                                )
                                                .is_some()
                                    })
                            {
                                let mut alt = j + 1;
                                while alt < actual_end
                                    && matches!(
                                        candidate_data.units[alt].kind,
                                        UnitKind::Close { .. } | UnitKind::BidiControl
                                    )
                                {
                                    alt += 1;
                                }
                                if alt < actual_end {
                                    let normal =
                                        if std::sync::Arc::ptr_eq(candidate_data, &self.data) {
                                            Some(alt as u32)
                                        } else {
                                            self.data.first_line.as_ref().unwrap().normal_cursors
                                                [alt]
                                        };
                                    if let Some(normal) = normal {
                                        candidates.push(normal as usize);
                                    }
                                }
                            }
                            candidates.sort_unstable();
                            candidates.dedup();
                            let mut layer = Vec::new();
                            for end in candidates {
                                let mut best = (f64::INFINITY, 0);
                                for (k, &(begin, cost)) in previous.iter().enumerate() {
                                    if begin >= end || !cost.is_finite() {
                                        continue;
                                    }
                                    let Some((data, actual_begin, actual_end)) =
                                        span_data(self, begin, end)
                                    else {
                                        continue;
                                    };
                                    let viable = if let Some(hyphen) = hyphen_end(data, actual_end)
                                    {
                                        super::hyphen::line(
                                            data,
                                            actual_begin,
                                            hyphen,
                                            cx,
                                            &mut sat,
                                        )
                                        .is_some()
                                    } else {
                                        super::windows::candidate(
                                            data,
                                            actual_begin,
                                            actual_end,
                                            cx,
                                            &mut sat,
                                        )
                                        .1
                                    };
                                    if !viable {
                                        continue;
                                    }
                                    let flags = if begin == 0 {
                                        BreakToken::FIRST_LINE
                                    } else {
                                        0
                                    };
                                    let indent = super::text_indent(&options, flags, &mut sat);
                                    let scan = selected(
                                        data,
                                        actual_begin,
                                        actual_end,
                                        LayoutUnit::ZERO,
                                        indent,
                                        atomics,
                                        cx,
                                        &mut sat,
                                    );
                                    let used = scan.content.add(indent, &mut sat).to_f32();
                                    if used > width
                                        && (begin != line_start(self, &greedy[i])
                                            || end != greedy[i].break_token().unit as usize)
                                    {
                                        continue;
                                    }
                                    let score = cost + f64::from((width - used).max(0.0)).powi(2);
                                    if score <= best.0 {
                                        best = (score, k);
                                    }
                                }
                                layer.push((end, best.0, best.1));
                            }
                            previous = layer.iter().map(|(end, cost, _)| (*end, *cost)).collect();
                            layers.push(layer);
                        }
                        let mut state = layers
                            .last()
                            .unwrap()
                            .iter()
                            .enumerate()
                            .min_by(|(_, a), (_, b)| {
                                a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0))
                            })
                            .map(|(i, _)| i)
                            .unwrap();
                        if layers.last().unwrap()[state].1.is_finite() {
                            for (row, layer) in layers.iter().enumerate().rev() {
                                ends[chunk + row] = layer[state].0 as u32;
                                state = layer[state].2;
                            }
                        }
                        chunk = stop;
                    }
                }
            }
            _ => {}
        }
        cx.warnings.record_saturation(&sat);
        BreakPlan {
            para: self.id(),
            width,
            options,
            atomics_generation: atomics.generation(),
            atomics_revision: atomics.revision,
            ends,
        }
    }
}
