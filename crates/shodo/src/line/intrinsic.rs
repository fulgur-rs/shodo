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
    /// Edge reshaping is budgeted per call, with first-line min/max passes
    /// sharing the same budget.
    pub fn intrinsic_sizes(
        &self,
        cx: &mut LayoutContext,
        options: &LineOptions,
        inputs: &AtomicIntrinsics,
    ) -> IntrinsicSizes {
        // Intrinsic work can replace the caches used by a retained trial.
        cx.completed = None;
        cx.begin_reshape_operation();
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
        let (ruby_min_atomics, ruby_max_atomics) = if data.ruby.containers.is_empty() {
            (AtomicSizes::new(), AtomicSizes::new())
        } else {
            ruby_atomics(inputs, cx, &mut sat)
        };
        let mut options = *options;
        options.text_indent.length = crate::sanitize::layout_length(
            options.text_indent.length,
            false,
            &mut cx.warnings,
            &mut sat,
        );
        let rtl = data.base_level % 2 == 1;
        let mut indent = super::text_indent(&options, BreakToken::FIRST_LINE, &mut sat);
        let mut word_flags = BreakToken::FIRST_LINE;
        let mut total_flags = BreakToken::FIRST_LINE;
        let mut min = LayoutUnit::ZERO;
        let mut max = LayoutUnit::ZERO;
        let mut word = indent;
        let mut word_unit = 0;
        let mut word_start = data.units.first().map_or(0, |u| u.text.start);
        let mut total = indent;
        let mut total_unit = 0;
        let mut trailing = LayoutUnit::ZERO;
        let mut word_trailing = LayoutUnit::ZERO;
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
                let natural_min = word
                    .add(
                        crate::ruby::measure::candidate_adjustment(
                            data,
                            word_unit,
                            i,
                            &ruby_min_atomics,
                            cx,
                            &mut sat,
                        ),
                        &mut sat,
                    )
                    .add(kept_word_spacing, &mut sat)
                    .sub(word_trailing, &mut sat)
                    .add(suffix, &mut sat);
                min = min.max(super::punctuation::intrinsic(
                    data,
                    word_spacing.summary(Some(data)),
                    word_flags,
                    false,
                    &options,
                    natural_min,
                    true,
                    &mut sat,
                ));
                let natural_max = total
                    .add(
                        crate::ruby::measure::candidate_adjustment(
                            data,
                            total_unit,
                            i,
                            &ruby_max_atomics,
                            cx,
                            &mut sat,
                        ),
                        &mut sat,
                    )
                    .add(kept_total_spacing, &mut sat)
                    .sub(trailing, &mut sat)
                    .add(left, &mut sat)
                    .add(right, &mut sat)
                    .add(suffix, &mut sat);
                max = max.max(super::punctuation::intrinsic(
                    data,
                    total_spacing.summary(Some(data)),
                    total_flags,
                    false,
                    &options,
                    natural_max,
                    false,
                    &mut sat,
                ));
                word_flags = BreakToken::AFTER_FORCED;
                total_flags = word_flags;
                indent = super::text_indent(&options, BreakToken::AFTER_FORCED, &mut sat);
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
                total_unit = i;
                trailing = LayoutUnit::ZERO;
                word_trailing = LayoutUnit::ZERO;
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
                            .add(
                                crate::ruby::measure::candidate_adjustment(
                                    data,
                                    total_unit,
                                    i,
                                    &ruby_max_atomics,
                                    cx,
                                    &mut sat,
                                ),
                                &mut sat,
                            )
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
                    total.add(total_spacing.summary(Some(data)).width(&mut sat), &mut sat),
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
                        word.add(word_spacing.summary(Some(data)).width(&mut sat), &mut sat),
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
                _ if super::whitespace::fits_hanging(data, i) => {
                    word_trailing = word_trailing.add(lo, &mut sat);
                    if super::whitespace::preserved(data, i) {
                        // Conditional trailing whitespace contributes to max-content,
                        // including its tracking, even though min-content excludes it.
                        kept_total_spacing = total_spacing.summary(Some(data)).width(&mut sat);
                    } else {
                        trailing = trailing.add(hi, &mut sat);
                    }
                }
                _ if super::whitespace::transparent(data, i) => {}
                _ => {
                    trailing = LayoutUnit::ZERO;
                    word_trailing = LayoutUnit::ZERO;
                    kept_word_spacing = word_spacing.summary(Some(data)).width(&mut sat);
                    kept_total_spacing = total_spacing.summary(Some(data)).width(&mut sat);
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
                    super::spacing::hyphen_summary(data, &word_spacing, i, &mut sat).width(&mut sat)
                } else {
                    kept_word_spacing
                };
                let measured_word = word.add(tracking, &mut sat).add(delta, &mut sat);
                let measured_word = measured_word.add(
                    crate::ruby::measure::candidate_adjustment(
                        data,
                        word_unit,
                        i + 1,
                        &ruby_min_atomics,
                        cx,
                        &mut sat,
                    ),
                    &mut sat,
                );
                let natural_min = measured_word.sub(word_trailing, &mut sat).add(
                    super::decoration::width(data, i + 1, false, &mut sat),
                    &mut sat,
                );
                min = min.max(super::punctuation::intrinsic(
                    data,
                    word_spacing.summary(Some(data)),
                    word_flags,
                    super::punctuation::last_edge(data, i + 1),
                    &options,
                    natural_min,
                    true,
                    &mut sat,
                ));
                word_flags = 0;
                let next = i + 1;
                let next_text = u.text.end;
                if alternate
                    && switch_at_soft_break
                    && let Some(normal) = first.unwrap().normal_cursors[next]
                {
                    alternate = false;
                    data = &self.data;
                    i = normal as usize;
                    total_unit = i;
                    word_unit = i;
                    word_start = data
                        .units
                        .get(i)
                        .map_or(data.text.len() as u32, |u| u.text.start);
                    word = super::text_indent(&options, 0, &mut sat)
                        .add(super::decoration::width(data, i, true, &mut sat), &mut sat);
                    trailing = LayoutUnit::ZERO;
                    word_trailing = LayoutUnit::ZERO;
                    word_spacing = Default::default();
                    kept_word_spacing = LayoutUnit::ZERO;
                    continue;
                }
                word_unit = next;
                word_start = next_text;
                word_spacing = Default::default();
                kept_word_spacing = LayoutUnit::ZERO;
                word_trailing = LayoutUnit::ZERO;
                word = super::text_indent(&options, 0, &mut sat).add(
                    super::decoration::width(data, next, true, &mut sat),
                    &mut sat,
                );
            }
            i += 1;
        }
        let final_delta = super::windows::delta(data, word_unit, data.units.len(), cx, &mut sat);
        let natural_min = word
            .add(
                crate::ruby::measure::candidate_adjustment(
                    data,
                    word_unit,
                    data.units.len(),
                    &ruby_min_atomics,
                    cx,
                    &mut sat,
                ),
                &mut sat,
            )
            .add(kept_word_spacing, &mut sat)
            .add(final_delta, &mut sat)
            .sub(word_trailing, &mut sat);
        min = min
            .max(super::punctuation::intrinsic(
                data,
                word_spacing.summary(Some(data)),
                word_flags,
                true,
                &options,
                natural_min,
                true,
                &mut sat,
            ))
            .max(LayoutUnit::ZERO);
        let natural_max = total
            .add(
                crate::ruby::measure::candidate_adjustment(
                    data,
                    total_unit,
                    data.units.len(),
                    &ruby_max_atomics,
                    cx,
                    &mut sat,
                ),
                &mut sat,
            )
            .add(kept_total_spacing, &mut sat)
            .sub(trailing, &mut sat)
            .add(left, &mut sat)
            .add(right, &mut sat);
        max = max
            .max(super::punctuation::intrinsic(
                data,
                total_spacing.summary(Some(data)),
                total_flags,
                true,
                &options,
                natural_max,
                false,
                &mut sat,
            ))
            .max(min);
        cx.warnings.record_saturation(&sat);
        IntrinsicSizes {
            min_content: min.to_f32(),
            max_content: max.to_f32(),
        }
    }
}

/// Min- and max-content atomic sizes for ruby measurement. Every `insert`
/// takes a fresh revision from a global counter, so with at least one atomic
/// the two sets never share a revision (or a ruby memo key); without atomics
/// both are empty and equal, and sharing is exact.
pub(crate) fn ruby_atomics(
    inputs: &AtomicIntrinsics,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (AtomicSizes, AtomicSizes) {
    let mut min = AtomicSizes::new();
    let mut max = AtomicSizes::new();
    for (node, value) in &inputs.atomics {
        let lo = crate::sanitize::layout_length(value.min_content, true, &mut cx.warnings, sat);
        let hi =
            crate::sanitize::layout_length(value.max_content, true, &mut cx.warnings, sat).max(lo);
        min.insert(
            *node,
            crate::AtomicSize {
                inline_size: lo,
                ..Default::default()
            },
        );
        max.insert(
            *node,
            crate::AtomicSize {
                inline_size: hi,
                ..Default::default()
            },
        );
    }
    (min, max)
}
