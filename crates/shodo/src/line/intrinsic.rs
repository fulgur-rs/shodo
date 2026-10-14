use crate::LayoutContext;
use crate::analysis::units::{BreakClass, UnitKind};
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::paragraph::{
    AtomicIntrinsics, AtomicSizes, BreakToken, FloatClear, FloatSide, IntrinsicSizes, Paragraph,
    ParagraphData,
};
use crate::style::LineOptions;

#[cfg(test)]
std::thread_local! {
    pub(crate) static MEASURE_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static FORCE_TWO_PASSES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static INTRINSIC_UNIT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn single_pass_options_safe(options: &LineOptions) -> bool {
    options.text_indent.length.is_finite()
        && options.text_indent.length.abs() <= 1.0e7
        && options.text_indent.length == 0.0
        && !options.text_indent.hanging
        && !options.text_indent.each_line
}

pub(crate) fn single_pass_safe(
    normal: &ParagraphData,
    first: &crate::paragraph::FirstLineData,
    options: &LineOptions,
) -> bool {
    let alternate: &ParagraphData = &first.data;
    // Spacing summaries can add terms after the glyph-width pre-bound below;
    // keep them on the two-pass path so saturation and warning counts match.
    if !single_pass_options_safe(options)
        || !normal.ruby.containers.is_empty()
        || !alternate.ruby.containers.is_empty()
        || normal.needs_spacing
        || alternate.needs_spacing
        || normal.units.len() != alternate.units.len()
        || first.normal_cursors.len() != alternate.units.len() + 1
        || normal.text != alternate.text
        || !normal.boxes.is_empty()
        || !alternate.boxes.is_empty()
        || !normal.combine_spans.is_empty()
        || !alternate.combine_spans.is_empty()
        || !normal.floats.is_empty()
        || !alternate.floats.is_empty()
    {
        return false;
    }

    // This path is for ordinary, space-delimited text with safe shaping
    // boundaries. Unsafe edges may reshape windows, consume a shared budget,
    // or emit warnings; keep their existing two-pass behavior unchanged.
    for (index, (a, b)) in normal.units.iter().zip(&alternate.units).enumerate() {
        #[cfg(test)]
        INTRINSIC_UNIT_VISITS.with(|visits| visits.set(visits.get() + 2));
        let kind_break_pair_is_supported = match a.kind {
            UnitKind::Cluster { .. } => {
                matches!(a.break_after, BreakClass::Prohibited | BreakClass::Allowed)
            }
            UnitKind::ForcedBreak => a.break_after == BreakClass::Mandatory,
            _ => false,
        };
        if first.normal_cursors[index] != Some(index as u32)
            || a.kind != b.kind
            || a.text != b.text
            || a.break_after != b.break_after
            || a.unsafe_to_break
            || b.unsafe_to_break
            || a.unsafe_to_concat
            || b.unsafe_to_concat
            || !matches!(a.kind, UnitKind::Cluster { .. } | UnitKind::ForcedBreak)
            || !matches!(
                a.break_after,
                BreakClass::Prohibited | BreakClass::Allowed | BreakClass::Mandatory
            )
            || !kind_break_pair_is_supported
        {
            return false;
        }
        if a.break_after == BreakClass::Allowed {
            if !matches!(a.kind, UnitKind::Cluster { space: true, .. })
                || normal.text.get(a.text.start as usize..a.text.end as usize) != Some(" ")
                || alternate
                    .text
                    .get(b.text.start as usize..b.text.end as usize)
                    != Some(" ")
                || [normal, alternate].into_iter().any(|data| {
                    let boundary = index + 1;
                    [data.units.get(index), data.units.get(boundary)]
                        .into_iter()
                        .flatten()
                        .any(|unit| {
                            unit.shared_cluster.as_ref().is_some_and(|cluster| {
                                cluster.units.start < boundary && boundary < cluster.units.end
                            })
                        })
                })
            {
                return false;
            }
        } else if matches!(a.kind, UnitKind::ForcedBreak) && a.break_after != BreakClass::Mandatory
        {
            return false;
        }
    }
    if first.normal_cursors[alternate.units.len()] != Some(normal.units.len() as u32) {
        return false;
    }

    // Bound every width term well below LayoutUnit's saturation point. That
    // keeps the optimized pass warning-free, so it does not change the
    // warning count or suppression behavior of the old two-pass path.
    let mut bound = 0_i64;
    for data in [normal, alternate] {
        for width in data
            .glyphs
            .advance
            .iter()
            .chain(data.glyphs.spacing.iter().flatten())
            .chain(data.glyphs.leading.iter().flatten())
            .chain(data.unit_spacing.iter().map(|spacing| &spacing.word))
            .chain(data.units.iter().map(|unit| &unit.slice_advance))
        {
            // Negative advances make `natural - LayoutUnit::MAX` saturate in
            // punctuation edge calculations even when their magnitude is
            // small. Keep them on the reference path.
            if width.raw() < 0 {
                return false;
            }
            bound = match bound.checked_add(i64::from(width.raw())) {
                Some(bound) if bound < i64::from(i32::MAX) / 16 => bound,
                _ => return false,
            };
        }
    }
    let mut sat = Saturation::default();
    let indent = super::text_indent(options, BreakToken::FIRST_LINE, &mut sat);
    sat.is_clean() && bound + i64::from(indent.raw()).abs() < i64::from(i32::MAX) / 16
}

fn simple_cluster_width(data: &ParagraphData, unit: &crate::analysis::units::Unit) -> Option<i64> {
    if unit.shared_cluster.is_some() {
        return Some(i64::from(unit.slice_advance.raw()));
    }
    let UnitKind::Cluster { glyphs, .. } = &unit.kind else {
        return matches!(unit.kind, UnitKind::ForcedBreak).then_some(0);
    };
    let mut width = LayoutUnit::ZERO;
    let mut sat = Saturation::default();
    for glyph in glyphs.clone() {
        width = width.add(*data.glyphs.advance.get(glyph as usize)?, &mut sat);
        if !sat.is_clean() {
            return None;
        }
    }
    Some(i64::from(width.raw()))
}

fn simple_intrinsic_spacing_is_zero(data: &ParagraphData, index: usize) -> bool {
    let summary = data.spacing_summaries.get(index);
    data.unit_spacing
        .get(index)
        .is_some_and(|spacing| spacing.word.raw() == 0)
        && summary.cost == 0
        && summary.first.is_none_or(|edge| edge.tracking == 0)
        && summary.last.is_none_or(|edge| edge.tracking == 0)
}

/// Fuses the min- and max-content walks for plain first-line text. It only
/// accepts segment boundaries where edge shaping is a no-op, so a rejected
/// input can safely fall back without having changed warnings or budgets.
fn simple_first_line_intrinsics(
    normal: &ParagraphData,
    first: &crate::paragraph::FirstLineData,
    options: &LineOptions,
) -> Option<IntrinsicSizes> {
    let alternate: &ParagraphData = &first.data;
    if *options != LineOptions::default()
        || normal.base_level != 0
        || alternate.base_level != 0
        || normal.style.writing_mode != crate::geometry::WritingMode::HorizontalTb
        || alternate.style.writing_mode != crate::geometry::WritingMode::HorizontalTb
        || normal.style.direction != crate::geometry::Direction::Ltr
        || alternate.style.direction != crate::geometry::Direction::Ltr
        || !normal.ruby.containers.is_empty()
        || !alternate.ruby.containers.is_empty()
        || !normal.boxes.is_empty()
        || !alternate.boxes.is_empty()
        || !normal.combine_spans.is_empty()
        || !alternate.combine_spans.is_empty()
        || !normal.floats.is_empty()
        || !alternate.floats.is_empty()
        || normal.text != alternate.text
        || normal.units.len() != alternate.units.len()
        || first.normal_cursors.len() != alternate.units.len() + 1
        || first.normal_cursors[alternate.units.len()] != Some(normal.units.len() as u32)
        || normal.text.starts_with(' ')
        || normal.text.ends_with(' ')
    {
        return None;
    }

    let mut sat = Saturation::default();
    let mut exact_min = LayoutUnit::ZERO;
    let mut exact_max = LayoutUnit::ZERO;
    let mut normal_word = LayoutUnit::ZERO;
    let mut alternate_word = LayoutUnit::ZERO;
    let mut normal_word_trailing = LayoutUnit::ZERO;
    let mut alternate_word_trailing = LayoutUnit::ZERO;
    let mut normal_total = LayoutUnit::ZERO;
    let mut alternate_total = LayoutUnit::ZERO;
    let mut normal_trailing = LayoutUnit::ZERO;
    let mut alternate_trailing = LayoutUnit::ZERO;
    let mut min_switched = false;
    let mut max_switched = false;
    let mut text_end = 0_usize;

    for (index, (normal_unit, alternate_unit)) in
        normal.units.iter().zip(&alternate.units).enumerate()
    {
        #[cfg(test)]
        INTRINSIC_UNIT_VISITS.with(|visits| visits.set(visits.get() + 2));
        if first.normal_cursors[index] != Some(index as u32)
            || normal_unit.shared_cluster.is_some()
            || alternate_unit.shared_cluster.is_some()
            || normal_unit.unsafe_to_break
            || alternate_unit.unsafe_to_break
            || normal_unit.unsafe_to_concat
            || alternate_unit.unsafe_to_concat
            || normal_unit.text != alternate_unit.text
            || normal_unit.text.start as usize != text_end
            || normal_unit.break_after != alternate_unit.break_after
            || normal_unit.combine.is_some()
            || alternate_unit.combine.is_some()
            || !simple_intrinsic_spacing_is_zero(normal, index)
            || !simple_intrinsic_spacing_is_zero(alternate, index)
        {
            return None;
        }
        let unit_text = normal
            .text
            .get(normal_unit.text.start as usize..normal_unit.text.end as usize)?;
        if !unit_text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'\n'))
        {
            return None;
        }
        text_end = normal_unit.text.end as usize;
        let (is_space, is_forced_break) = match (&normal_unit.kind, &alternate_unit.kind) {
            (UnitKind::Cluster { space: a, .. }, UnitKind::Cluster { space: b, .. })
                if a == b
                    && matches!(
                        normal_unit.break_after,
                        BreakClass::Prohibited | BreakClass::Allowed
                    ) =>
            {
                (*a, false)
            }
            (UnitKind::ForcedBreak, UnitKind::ForcedBreak)
                if normal_unit.break_after == BreakClass::Mandatory =>
            {
                (false, true)
            }
            _ => return None,
        };

        if is_forced_break {
            let (word, word_trailing) = if min_switched {
                (normal_word, normal_word_trailing)
            } else {
                (alternate_word, alternate_word_trailing)
            };
            exact_min = exact_min.max(word.sub(word_trailing, &mut sat));
            let (total, trailing) = if max_switched {
                (normal_total, normal_trailing)
            } else {
                (alternate_total, alternate_trailing)
            };
            exact_max = exact_max.max(total.sub(trailing, &mut sat));
            min_switched = true;
            max_switched = true;
            normal_word = LayoutUnit::ZERO;
            alternate_word = LayoutUnit::ZERO;
            normal_word_trailing = LayoutUnit::ZERO;
            alternate_word_trailing = LayoutUnit::ZERO;
            normal_total = LayoutUnit::ZERO;
            alternate_total = LayoutUnit::ZERO;
            normal_trailing = LayoutUnit::ZERO;
            alternate_trailing = LayoutUnit::ZERO;
            continue;
        }

        if is_space != (unit_text == " ") {
            return None;
        }
        if is_space
            && (normal_unit.break_after != BreakClass::Allowed
                || !super::whitespace::fits_hanging(normal, index)
                || !super::whitespace::fits_hanging(alternate, index)
                || super::whitespace::preserved(normal, index)
                || super::whitespace::preserved(alternate, index))
        {
            return None;
        }

        let normal_width = simple_cluster_width(normal, normal_unit)?;
        let alternate_width = simple_cluster_width(alternate, alternate_unit)?;
        if normal_width < 0 || alternate_width < 0 {
            return None;
        }
        let normal_width = LayoutUnit::from_raw(i32::try_from(normal_width).ok()?);
        let alternate_width = LayoutUnit::from_raw(i32::try_from(alternate_width).ok()?);
        normal_word = normal_word.add(normal_width, &mut sat);
        alternate_word = alternate_word.add(alternate_width, &mut sat);
        normal_total = normal_total.add(normal_width, &mut sat);
        alternate_total = alternate_total.add(alternate_width, &mut sat);
        if is_space {
            normal_word_trailing = normal_word_trailing.add(normal_width, &mut sat);
            alternate_word_trailing = alternate_word_trailing.add(alternate_width, &mut sat);
            normal_trailing = normal_trailing.add(normal_width, &mut sat);
            alternate_trailing = alternate_trailing.add(alternate_width, &mut sat);
        } else {
            normal_word_trailing = LayoutUnit::ZERO;
            alternate_word_trailing = LayoutUnit::ZERO;
            normal_trailing = LayoutUnit::ZERO;
            alternate_trailing = LayoutUnit::ZERO;
        }
        if normal_unit.break_after == BreakClass::Allowed {
            let (word, word_trailing) = if min_switched {
                (normal_word, normal_word_trailing)
            } else {
                (alternate_word, alternate_word_trailing)
            };
            exact_min = exact_min.max(word.sub(word_trailing, &mut sat));
            min_switched = true;
            normal_word = LayoutUnit::ZERO;
            alternate_word = LayoutUnit::ZERO;
            normal_word_trailing = LayoutUnit::ZERO;
            alternate_word_trailing = LayoutUnit::ZERO;
        }
    }

    if text_end != normal.text.len() || !sat.is_clean() {
        return None;
    }
    let (word, word_trailing) = if min_switched {
        (normal_word, normal_word_trailing)
    } else {
        (alternate_word, alternate_word_trailing)
    };
    exact_min = exact_min
        .max(word.sub(word_trailing, &mut sat))
        .max(LayoutUnit::ZERO);
    let (total, trailing) = if max_switched {
        (normal_total, normal_trailing)
    } else {
        (alternate_total, alternate_trailing)
    };
    exact_max = exact_max.max(total.sub(trailing, &mut sat)).max(exact_min);
    if !sat.is_clean() {
        return None;
    }

    #[cfg(test)]
    MEASURE_PASSES.with(|count| count.set(count.get() + 1));
    Some(IntrinsicSizes {
        min_content: exact_min.to_f32(),
        max_content: exact_max.to_f32(),
    })
}

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
        let first = self.data.first_line.as_ref();
        #[cfg(test)]
        let force_two_passes = FORCE_TWO_PASSES.with(std::cell::Cell::get);
        #[cfg(not(test))]
        let force_two_passes = false;
        if !force_two_passes
            && cx.warnings.checkpoint() == Some(0)
            && let Some(first) = first
            && let Some(sizes) = simple_first_line_intrinsics(&self.data, first, options)
        {
            return sizes;
        }
        let single_geometry = first.is_some_and(|first| {
            first.intrinsic_geometry_reused
                && cx.warnings.checkpoint() == Some(0)
                && single_pass_options_safe(options)
                && first.intrinsic_single_pass_safe
        });
        #[cfg(test)]
        let single_geometry = single_geometry && !FORCE_TWO_PASSES.with(std::cell::Cell::get);
        if first.is_some() && !single_geometry {
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
        } else if single_geometry {
            // The normal and first-line datasets have identical non-paint
            // styles. One walk can therefore switch at the min-content break
            // without changing the max-content result.
            self.measure_intrinsics(cx, options, inputs, true)
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
        #[cfg(test)]
        MEASURE_PASSES.with(|count| count.set(count.get() + 1));
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
        let options = crate::sanitize::line_options(options, &mut cx.warnings, &mut sat);
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
        cx.ruby_memo
            .retain_intrinsic_row(data, &ruby_max_atomics, total_unit);
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
            #[cfg(test)]
            INTRINSIC_UNIT_VISITS.with(|visits| visits.set(visits.get() + 1));
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
                cx.ruby_memo
                    .retain_intrinsic_row(data, &ruby_max_atomics, total_unit);
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
                    super::spacing::hyphen_summary(data, &word_spacing, i, i + 1, &mut sat)
                        .width(&mut sat)
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
                    cx.ruby_memo
                        .retain_intrinsic_row(data, &ruby_max_atomics, total_unit);
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
