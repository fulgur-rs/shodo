use crate::LayoutContext;
use crate::analysis::units::{BreakClass, UnitKind};
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::paragraph::{
    AtomicIntrinsics, AtomicSizes, BreakToken, FloatClear, FloatSide, IntrinsicSizes, Paragraph,
    ParagraphData,
};
use crate::style::LineOptions;

impl Paragraph {
    /// Measures intrinsic margin-box widths, including floats and clear.
    /// Missing caller dimensions collapse to zero and produce a warning.
    pub fn intrinsic_sizes(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        inputs: &AtomicIntrinsics,
    ) -> IntrinsicSizes {
        if self.data.first_line.is_some() {
            let min = self
                .measure_intrinsics(cx, options, inputs, true)
                .min_content;
            let max = self
                .measure_intrinsics(cx, options, inputs, false)
                .max_content;
            IntrinsicSizes {
                min_content: min,
                max_content: max.max(min),
            }
        } else {
            self.measure_intrinsics(cx, options, inputs, false)
        }
    }

    fn measure_intrinsics(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        inputs: &AtomicIntrinsics,
        switch_at_soft_break: bool,
    ) -> IntrinsicSizes {
        let first = self.data.first_line.as_ref();
        let mut alternate = first.is_some();
        let mut data: &ParagraphData = first.map_or(&self.data, |f| &f.data);
        cx.warnings.set_max(self.data.limits.max_warnings);
        let mut sat = Saturation::default();
        let mut options = *options;
        options.text_indent.length = crate::sanitize::layout_length(
            options.text_indent.length,
            false,
            &mut cx.warnings,
            &mut sat,
        );
        let rtl = data.base_level % 2 == 1;
        let mut indent = super::text_indent(&options, BreakToken::FIRST_LINE, &mut sat);
        let mut min = LayoutUnit::ZERO;
        let mut max = LayoutUnit::ZERO;
        let mut word = indent;
        let mut word_unit = 0;
        let mut word_start = data.units.first().map_or(0, |u| u.text.start);
        let mut total = indent;
        let mut trailing = LayoutUnit::ZERO;
        let mut left = LayoutUnit::ZERO;
        let mut right = LayoutUnit::ZERO;
        let mut i = 0;
        let mut word_spacing = super::spacing_summary::Cursor::default();
        let mut total_spacing = super::spacing_summary::Cursor::default();
        let mut kept_word_spacing = LayoutUnit::ZERO;
        let mut kept_total_spacing = LayoutUnit::ZERO;
        while let Some(u) = data.units.get(i) {
            if matches!(
                u.kind,
                UnitKind::ForcedBreak | UnitKind::BlockInInline { .. }
            ) {
                let suffix = super::decoration::width(data, i, false, &mut sat);
                min = min.max(
                    word.add(kept_word_spacing, &mut sat)
                        .sub(trailing, &mut sat)
                        .add(suffix, &mut sat),
                );
                max = max.max(
                    total
                        .add(kept_total_spacing, &mut sat)
                        .sub(trailing, &mut sat)
                        .add(left, &mut sat)
                        .add(right, &mut sat)
                        .add(suffix, &mut sat),
                );
                indent = super::text_indent(
                    &options,
                    if matches!(u.kind, UnitKind::ForcedBreak) {
                        BreakToken::AFTER_FORCED
                    } else {
                        0
                    },
                    &mut sat,
                );
                let next = i + 1;
                i = if alternate {
                    if let Some(normal) = first.unwrap().normal_cursors[next] {
                        alternate = false;
                        data = &self.data;
                        normal as usize
                    } else {
                        next
                    }
                } else {
                    next
                };
                let prefix = super::decoration::width(data, i, true, &mut sat);
                word = indent.add(prefix, &mut sat);
                word_unit = i;
                word_start = data
                    .units
                    .get(i)
                    .map_or(data.text.len() as u32, |u| u.text.start);
                total = word;
                trailing = LayoutUnit::ZERO;
                left = LayoutUnit::ZERO;
                right = LayoutUnit::ZERO;
                word_spacing = Default::default();
                total_spacing = Default::default();
                kept_word_spacing = LayoutUnit::ZERO;
                kept_total_spacing = LayoutUnit::ZERO;
                continue;
            }
            if let UnitKind::Float { node, .. } = u.kind {
                let value = inputs.floats.get(&node).copied().unwrap_or_else(|| {
                    cx.warnings.push(
                        WarningKind::MissingAtomicSize,
                        "missing float intrinsic widths",
                    );
                    Default::default()
                });
                let lo = crate::sanitize::layout_length(
                    value.min_content,
                    true,
                    &mut cx.warnings,
                    &mut sat,
                );
                let hi = crate::sanitize::layout_length(
                    value.max_content,
                    true,
                    &mut cx.warnings,
                    &mut sat,
                )
                .max(lo);
                let clear_left = matches!(value.clear, FloatClear::Left | FloatClear::Both)
                    || matches!(value.clear, FloatClear::InlineStart) && !rtl
                    || matches!(value.clear, FloatClear::InlineEnd) && rtl;
                let clear_right = matches!(value.clear, FloatClear::Right | FloatClear::Both)
                    || matches!(value.clear, FloatClear::InlineStart) && rtl
                    || matches!(value.clear, FloatClear::InlineEnd) && !rtl;
                if clear_left || clear_right {
                    max = max.max(
                        total
                            .add(kept_total_spacing, &mut sat)
                            .sub(trailing, &mut sat)
                            .add(left, &mut sat)
                            .add(right, &mut sat),
                    );
                    if clear_left {
                        left = LayoutUnit::ZERO;
                    }
                    if clear_right {
                        right = LayoutUnit::ZERO;
                    }
                }
                let is_left = matches!(value.side, FloatSide::Left)
                    || matches!(value.side, FloatSide::InlineStart) && !rtl
                    || matches!(value.side, FloatSide::InlineEnd) && rtl;
                let side = if is_left { &mut left } else { &mut right };
                *side = side.add(LayoutUnit::from_f32_round(hi, &mut sat), &mut sat);
                min = min.max(LayoutUnit::from_f32_round(lo, &mut sat));
            }
            let (lo, hi) = if matches!(u.kind, UnitKind::Float { .. }) {
                (LayoutUnit::ZERO, LayoutUnit::ZERO)
            } else if let UnitKind::Atomic { node } = u.kind {
                let a = inputs.atomics.get(&node).copied().unwrap_or_else(|| {
                    cx.warnings.push(
                        WarningKind::MissingAtomicSize,
                        "missing atomic intrinsic widths",
                    );
                    Default::default()
                });
                let lo =
                    crate::sanitize::layout_length(a.min_content, true, &mut cx.warnings, &mut sat);
                let hi =
                    crate::sanitize::layout_length(a.max_content, true, &mut cx.warnings, &mut sat)
                        .max(lo);
                (
                    LayoutUnit::from_f32_round(lo, &mut sat),
                    LayoutUnit::from_f32_round(hi, &mut sat),
                )
            } else {
                let width = super::scan::unit_width(
                    data,
                    u,
                    total.add(total_spacing.summary().width(&mut sat), &mut sat),
                    &AtomicSizes::EMPTY,
                    cx,
                    &mut sat,
                )
                .add(data.unit_spacing[i].word, &mut sat);
                let min_width = if u.shared_cluster.is_some() || matches!(u.kind, UnitKind::Tab) {
                    super::scan::unit_width_from(
                        data,
                        u,
                        word_start,
                        word.add(word_spacing.summary().width(&mut sat), &mut sat),
                        &AtomicSizes::EMPTY,
                        cx,
                        &mut sat,
                    )
                    .add(data.unit_spacing[i].word, &mut sat)
                } else {
                    width
                };
                (min_width, width)
            };
            word = word.add(lo, &mut sat);
            total = total.add(hi, &mut sat);
            super::spacing::push(data, &mut word_spacing, i);
            super::spacing::push(data, &mut total_spacing, i);
            match u.kind {
                UnitKind::Cluster { space: true, .. } => trailing = trailing.add(hi, &mut sat),
                UnitKind::Close { .. }
                | UnitKind::BidiControl
                | UnitKind::Absolute { .. }
                | UnitKind::Float { .. } => {}
                _ => {
                    trailing = LayoutUnit::ZERO;
                    kept_word_spacing = word_spacing.summary().width(&mut sat);
                    kept_total_spacing = total_spacing.summary().width(&mut sat);
                }
            }
            let hyphen = if u.break_after == BreakClass::Hyphen {
                super::hyphen::line(data, word_unit, i + 1, cx, &mut sat)
                    .map(|windows| super::windows::cost(&windows, i + 1, &mut sat))
            } else {
                None
            };
            if u.break_after == BreakClass::Allowed
                || hyphen.is_some()
                || u.break_after == BreakClass::Emergency && u.emergency_min_content
            {
                let (delta, viable) = if let Some(delta) = hyphen {
                    (delta, true)
                } else {
                    super::windows::candidate(data, word_unit, i + 1, cx, &mut sat)
                };
                if !viable {
                    i += 1;
                    continue;
                }
                let tracking = if hyphen.is_some() {
                    super::spacing::hyphen(data, &word_spacing, i, &mut sat)
                } else {
                    kept_word_spacing
                };
                let measured_word = word.add(tracking, &mut sat).add(delta, &mut sat);
                min = min.max(
                    measured_word
                        .sub(
                            if matches!(u.kind, UnitKind::Cluster { space: true, .. }) {
                                lo
                            } else {
                                LayoutUnit::ZERO
                            },
                            &mut sat,
                        )
                        .add(
                            super::decoration::width(data, i + 1, false, &mut sat),
                            &mut sat,
                        ),
                );
                let next = i + 1;
                let next_text = u.text.end;
                if alternate
                    && switch_at_soft_break
                    && let Some(normal) = first.unwrap().normal_cursors[next]
                {
                    alternate = false;
                    data = &self.data;
                    i = normal as usize;
                    word_unit = i;
                    word_start = data
                        .units
                        .get(i)
                        .map_or(data.text.len() as u32, |u| u.text.start);
                    word = super::text_indent(&options, 0, &mut sat)
                        .add(super::decoration::width(data, i, true, &mut sat), &mut sat);
                    trailing = LayoutUnit::ZERO;
                    word_spacing = Default::default();
                    kept_word_spacing = LayoutUnit::ZERO;
                    continue;
                }
                word_unit = next;
                word_start = next_text;
                word_spacing = Default::default();
                kept_word_spacing = LayoutUnit::ZERO;
                word = super::text_indent(&options, 0, &mut sat).add(
                    super::decoration::width(data, next, true, &mut sat),
                    &mut sat,
                );
            }
            i += 1;
        }
        let final_delta = super::windows::delta(data, word_unit, data.units.len(), cx, &mut sat);
        min = min
            .max(
                word.add(kept_word_spacing, &mut sat)
                    .add(final_delta, &mut sat)
                    .sub(trailing, &mut sat),
            )
            .max(LayoutUnit::ZERO);
        max = max
            .max(
                total
                    .add(kept_total_spacing, &mut sat)
                    .sub(trailing, &mut sat)
                    .add(left, &mut sat)
                    .add(right, &mut sat),
            )
            .max(min);
        cx.warnings.record_saturation(&sat);
        IntrinsicSizes {
            min_content: min.to_f32(),
            max_content: max.to_f32(),
        }
    }
}
