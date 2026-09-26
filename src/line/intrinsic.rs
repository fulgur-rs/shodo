use crate::LayoutContext;
use crate::analysis::units::{BreakClass, UnitKind};
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::paragraph::{
    AtomicIntrinsics, AtomicSizes, BreakToken, FloatClear, FloatSide, IntrinsicSizes, Paragraph,
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
        cx.warnings.set_max(self.data.limits.max_warnings);
        let mut sat = Saturation::default();
        let mut options = *options;
        options.text_indent.length = crate::sanitize::layout_length(
            options.text_indent.length,
            false,
            &mut cx.warnings,
            &mut sat,
        );
        let rtl = self.data.base_level % 2 == 1;
        let mut indent = super::text_indent(&options, BreakToken::FIRST_LINE, &mut sat);
        let mut min = LayoutUnit::ZERO;
        let mut max = LayoutUnit::ZERO;
        let mut word = indent;
        let mut word_start = self.data.units.first().map_or(0, |u| u.text.start);
        let mut total = indent;
        let mut trailing = LayoutUnit::ZERO;
        let mut left = LayoutUnit::ZERO;
        let mut right = LayoutUnit::ZERO;
        for (i, u) in self.data.units.iter().enumerate() {
            if matches!(
                u.kind,
                UnitKind::ForcedBreak | UnitKind::BlockInInline { .. }
            ) {
                let suffix = super::decoration::width(&self.data, i, false, &mut sat);
                min = min.max(word.sub(trailing, &mut sat).add(suffix, &mut sat));
                max = max.max(
                    total
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
                let prefix = super::decoration::width(&self.data, i + 1, true, &mut sat);
                word = indent.add(prefix, &mut sat);
                word_start = self
                    .data
                    .units
                    .get(i + 1)
                    .map_or(u.text.end, |next| next.text.start);
                total = word;
                trailing = LayoutUnit::ZERO;
                left = LayoutUnit::ZERO;
                right = LayoutUnit::ZERO;
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
                continue;
            }
            let (lo, hi) = if let UnitKind::Atomic { node } = u.kind {
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
                    &self.data,
                    u,
                    total,
                    &AtomicSizes::EMPTY,
                    cx,
                    &mut sat,
                );
                let min_width = if u.shared_cluster.is_some() {
                    super::scan::unit_width_from(
                        &self.data,
                        u,
                        word_start,
                        word,
                        &AtomicSizes::EMPTY,
                        cx,
                        &mut sat,
                    )
                } else {
                    width
                };
                (min_width, width)
            };
            word = word.add(lo, &mut sat);
            total = total.add(hi, &mut sat);
            match u.kind {
                UnitKind::Cluster { space: true, .. } => trailing = trailing.add(hi, &mut sat),
                UnitKind::Close { .. } | UnitKind::BidiControl | UnitKind::Absolute { .. } => {}
                _ => trailing = LayoutUnit::ZERO,
            }
            let hyphen = if u.break_after == BreakClass::Hyphen {
                super::hyphen::shape(&self.data, u, cx, &mut sat)
                    .map(|edge| super::hyphen::width(&edge, &mut sat).sub(lo, &mut sat))
            } else {
                None
            };
            if u.break_after == BreakClass::Allowed
                || hyphen.is_some()
                || u.break_after == BreakClass::Emergency && u.emergency_min_content
            {
                let measured_word = word.add(hyphen.unwrap_or(LayoutUnit::ZERO), &mut sat);
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
                            super::decoration::width(&self.data, i + 1, false, &mut sat),
                            &mut sat,
                        ),
                );
                word_start = u.text.end;
                word = super::text_indent(&options, 0, &mut sat).add(
                    super::decoration::width(&self.data, i + 1, true, &mut sat),
                    &mut sat,
                );
            }
        }
        min = min.max(word.sub(trailing, &mut sat)).max(LayoutUnit::ZERO);
        max = max
            .max(
                total
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
