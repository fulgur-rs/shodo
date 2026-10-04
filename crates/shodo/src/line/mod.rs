//! Line breaking.

mod align;
pub(crate) mod autospace;
pub(crate) mod cache;
pub(crate) mod completed;
mod decoration;
pub(crate) mod font_metrics;
pub(crate) mod fragments;
mod hyphen;
mod intrinsic;
mod iter;
pub(crate) mod metric_index;
pub(crate) mod metrics;
mod plan;
pub(crate) mod punctuation;
pub(crate) mod range;
pub(crate) mod replay;
pub(crate) mod reshape;
mod scan;
pub(crate) mod spacing;
pub(crate) mod spacing_summary;
mod whitespace;
pub(crate) mod windows;

pub(crate) use scan::tab_advance;
pub(crate) use whitespace::trailing_advance;

/// Use the same bounded edge/spacing machinery as accepted selected fragments.
/// This measures units, not a rebuilt paragraph or retained child Line.
pub(crate) fn ruby_range_width(
    data: &crate::paragraph::ParagraphData,
    units: std::ops::Range<usize>,
    atomics: &crate::AtomicSizes,
    cx: &mut crate::LayoutContext,
    sat: &mut crate::geometry::Saturation,
) -> crate::geometry::LayoutUnit {
    if units.is_empty() {
        return crate::geometry::LayoutUnit::ZERO;
    }
    if let Some(width) = range::width(data, units.clone(), atomics, cx, sat) {
        return width;
    }
    #[cfg(test)]
    {
        cx.ruby_measure_visits += units.len();
    }
    plan::selected_raw(
        data,
        units.start,
        units.end,
        crate::geometry::LayoutUnit::ZERO,
        crate::geometry::LayoutUnit::ZERO,
        0,
        &crate::style::LineOptions::default(),
        crate::geometry::LayoutUnit::MAX,
        atomics,
        cx,
        sat,
    )
    .content
}

/// A base's own continuation decorations contribute to its width. Cloned
/// ancestors outside that base belong to the parent line, not to the column
/// constraint against which the annotation's deficit is calculated.
pub(crate) fn ruby_base_width(
    data: &crate::paragraph::ParagraphData,
    scope: std::ops::Range<usize>,
    selected: std::ops::Range<usize>,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    if selected.is_empty() {
        return LayoutUnit::ZERO;
    }
    let width = ruby_range_width(data, selected.clone(), atomics, cx, sat);
    let mut outer = decoration::path(data, scope.start);
    let mut excluded = LayoutUnit::ZERO;
    for (boundary, start) in [(selected.start, true), (selected.end, false)] {
        let mut boundary = decoration::path(data, boundary);
        decoration::add_shared_width(data, &mut outer, &mut boundary, start, &mut excluded, sat);
    }
    width.sub(excluded, sat)
}

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
    pub(crate) ruby: Option<crate::ruby::measure::RubyMeasure>,
    pub(crate) ruby_caret_gaps: Vec<crate::ruby::align::CaretGap>,
    pub(crate) prepared: bool,
    pub(crate) end: usize,
    pub(crate) reason: BreakReason,
    /// Width of every unit in the line, in order.
    pub(crate) widths: Vec<LayoutUnit>,
    pub(crate) leading: Option<Vec<LayoutUnit>>,
    pub(crate) autospace_gaps: Vec<autospace::Gap>,
    pub(crate) overlays: Vec<reshape::EdgeOverlay>,
    /// Content width, excluding text-indent and hanging trailing whitespace.
    pub(crate) content: LayoutUnit,
    /// Index of the first eligible trailing whitespace of the line (`end` when
    /// there is none). Units from here on are trailing spaces/tabs, inline box
    /// ends, bidi controls, out-of-flow anchors or a forced break.
    pub(crate) hang_start: usize,
    pub(crate) hanging_end: LayoutUnit,
    pub(super) punctuation_edges: punctuation::EdgeAdjustment,
}

impl Paragraph {
    /// Lays out the line starting at `token`. Pure: the same inputs always
    /// give the same result, so a token can be retried with other
    /// constraints.
    /// If a line cannot fit even at the top of a page, retry the same token
    /// with `max_block_size: None` to accept the overflowing line.
    ///
    /// Start with [`Self::start_token`]. Accept a [`LineResult::Line`] before
    /// advancing to [`Line::break_token`] and updating the logical block
    /// offset. [`LineResult::BlockSizeExceeded`] leaves the token unchanged.
    /// Its limit is the space available to this line, so supply the remaining
    /// page extent rather than the full page extent after every line.
    ///
    /// Float placement requires additional caller state: retain the returned
    /// cursor, retry from `line_start`, and inspect [`Line::displaced_floats`]
    /// before accepting a line. Withdraw only the last displaced float, then
    /// retry and reevaluate before withdrawing another; do not batch withdrawals.
    /// Defer re-reported withdrawn floats for that line. Save tokens,
    /// cursors, placements and withdrawal/defer records together at page
    /// checkpoints. The [float integration guide] describes the complete loop.
    ///
    /// # Examples
    ///
    /// Paginate plain text without floats or block-in-inline content. A height
    /// rejection retries the current token at the next page's start; a line
    /// taller than a whole page is accepted with the height limit removed:
    ///
    /// ```
    /// use shodo::font::{FontCollection, FontOptions};
    /// use shodo::limits::Limits;
    /// use shodo::style::{LineOptions, ParagraphStyle};
    /// use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult, RichText};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let style = ParagraphStyle::default();
    /// let fonts = FontCollection::with_options(&Limits::default(), FontOptions {
    ///     system_fonts: false, ..Default::default()
    /// });
    /// let mut cx = LayoutContext::new();
    /// let paragraph = RichText::new(&style)
    ///     .push("A paragraph long enough to wrap across several lines and pages.", &style.root)
    ///     .build(&mut cx, &fonts)?;
    /// let options = LineOptions::default();
    /// let page_extent = 48.0;
    /// let mut page = 1;
    /// let mut token = paragraph.start_token();
    /// let mut constraint = LineConstraint::new(80.0);
    /// constraint.max_block_size = Some(page_extent);
    /// loop {
    ///     match paragraph.next_line(
    ///         &mut cx, token, &options, &constraint, &AtomicSizes::EMPTY,
    ///     ) {
    ///         LineResult::Line(line) => {
    ///             println!("page {page}: {}", &line.text()[line.text_range()]);
    ///             token = line.break_token();
    ///             constraint.block_offset += line.block_size();
    ///             constraint.max_block_size = Some(
    ///                 (page_extent - constraint.block_offset).max(0.0),
    ///             );
    ///         }
    ///         LineResult::BlockSizeExceeded { .. } => {
    ///             if constraint.block_offset > 0.0 {
    ///                 page += 1;
    ///                 constraint.block_offset = 0.0;
    ///                 constraint.max_block_size = Some(page_extent);
    ///             } else {
    ///                 constraint.max_block_size = None;
    ///             }
    ///             // Keep the same token until this candidate line is accepted.
    ///         }
    ///         LineResult::Done => break,
    ///         other => return Err(format!("unexpected layout result: {other:?}").into()),
    ///     }
    /// }
    /// assert!(page > 1);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// These examples use deterministic missing-glyph output. Register real
    /// font data before rendering; read layout diagnostics through
    /// [`LayoutContext::take_warnings`].
    ///
    /// [float integration guide]: https://github.com/fulgur-rs/shodo/blob/adf02f0dda2cb41837f371eef5b70b7389e28eea/docs/guides/float-integration-harness.md
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
        if cx
            .completed
            .as_ref()
            .is_some_and(|entry| !entry.same_start(self, token))
        {
            cx.completed = None;
        }
        if token.para != data.id || token.unit as usize > data.units.len() {
            return LineResult::InvalidToken;
        }
        if token.flags & BreakToken::FIRST_LINE != 0
            && let Some(first) = &data.first_line
        {
            cx.completed = None;
            let mut sat = Saturation::default();
            let constraint = crate::sanitize::constraint(*constraint, &mut cx.warnings, &mut sat);
            let mut options = *options;
            options.text_indent.length = crate::sanitize::layout_length(
                options.text_indent.length,
                false,
                &mut cx.warnings,
                &mut sat,
            );
            cx.warnings.record_saturation(&sat);
            let alternate = Paragraph {
                data: std::sync::Arc::clone(&first.data),
            };
            let mut alternate_token = token;
            alternate_token.unit = 0;
            let planned_end = constraint.break_plan.and_then(|p| {
                if plan::matches(p, self, &options, &constraint, atomics) {
                    let normal_end = *p.ends.first()?;
                    first.alternate_cursor(normal_end)
                } else {
                    cx.warnings.push(
                        WarningKind::Unsupported,
                        "break plan inputs mismatch; using greedy layout",
                    );
                    None
                }
            });
            let mut alternate_constraint = constraint;
            alternate_constraint.break_plan = None;
            let mut result = alternate.next_line_in_set(
                cx,
                alternate_token,
                &options,
                &alternate_constraint,
                atomics,
                planned_end,
                None,
                Some(&first.normal_cursors),
            );
            match &mut result {
                LineResult::Line(line) => {
                    let Some(unit) = first.normal_cursors[line.break_token.unit as usize] else {
                        // Unavailable resource-limited cuts are excluded at build.
                        return LineResult::InvalidToken;
                    };
                    line.break_token.unit = unit;
                }
                LineResult::FloatEncountered { line_start, .. } => *line_start = token,
                LineResult::BlockInInline { token_after, .. } => {
                    let Some(unit) = first.normal_cursors[token_after.unit as usize] else {
                        return LineResult::InvalidToken;
                    };
                    token_after.unit = unit;
                }
                _ => {}
            }
            return result;
        }
        self.next_line_in_set(cx, token, options, constraint, atomics, None, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn next_line_in_set(
        &self,
        cx: &mut LayoutContext,
        token: BreakToken,
        options: &LineOptions,
        constraint: &LineConstraint<'_>,
        atomics: &AtomicSizes,
        planned_end_override: Option<usize>,
        annotation_align: Option<crate::ruby::align::AnnotationAlign<'_>>,
        normal_cursors: Option<&[Option<u32>]>,
    ) -> LineResult {
        let data = &*self.data;
        let start = token.unit as usize;
        if start == data.units.len() {
            cx.completed = None;
            return LineResult::Done;
        }
        if let UnitKind::BlockInInline { node } = data.units[start].kind {
            cx.completed = None;
            let token_after = BreakToken {
                para: data.id,
                unit: token.unit + 1,
                flags: BreakToken::AFTER_FORCED,
            };
            return LineResult::BlockInInline { node, token_after };
        }
        cx.begin_reshape_operation();
        let warning_checkpoint = cx.warnings.checkpoint();
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
        let planned_end = planned_end_override.or_else(|| {
            constraint.break_plan.and_then(|p| {
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
            })
        });
        let eligible = planned_end_override.is_none()
            && annotation_align.is_none()
            && normal_cursors.is_none()
            && constraint.break_plan.is_none()
            && constraint.max_graphemes.is_none()
            && data.warnings.is_empty()
            && sat.is_clean()
            && warning_checkpoint.is_some()
            && cx.warnings.checkpoint() == warning_checkpoint;
        // Ordinary acceptance with no retained trial does not construct a key.
        let key = (eligible && (cx.completed.is_some() || constraint.max_block_size.is_some()))
            .then(|| completed::Key::new(self, token, options, &constraint, atomics));
        if let Some(entry) = cx.completed.take()
            && key.as_ref() == Some(&entry.key)
        {
            let needed = entry.line.block_size();
            if constraint.max_block_size.is_some_and(|max| needed > max) {
                cx.completed = Some(entry);
                return LineResult::BlockSizeExceeded {
                    needed_block_size: needed,
                };
            }
            return LineResult::Line(entry.line);
        }
        let mut scan = if let Some(end) = planned_end {
            plan::selected(
                data,
                start,
                end,
                offset,
                indent,
                token.flags,
                &options,
                available,
                atomics,
                cx,
                &mut sat,
            )
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
                normal_cursors,
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
        reshape::prepare(data, start, &mut scan, cx, &mut sat);
        crate::ruby::measure::apply(data, start, &mut scan, atomics, cx, &mut sat);
        spacing::apply(data, start, &mut scan, &mut sat);
        punctuation::apply(data, start, &mut scan, &mut sat);
        crate::ruby::align::bases(data, start, &mut scan, &mut sat);
        if let Some(align) = annotation_align {
            crate::ruby::align::annotation(data, start, &mut scan, available, align, &mut sat);
        }
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
        let mut displaced = Vec::new();
        if let Some(cursor) = constraint.floats_placed_through
            && !data.floats.is_empty()
        {
            let begin = data
                .floats
                .partition_point(|(unit, _)| *unit < scan.end as u32);
            let handled = (u64::from(cursor.0) + 1).min(data.floats.len() as u64) as usize;
            if begin < handled {
                for (index, (_, node)) in data.floats[begin..handled].iter().enumerate() {
                    #[cfg(test)]
                    {
                        cx.float_search_visits += 1;
                    }
                    displaced.push((*node, FloatCursor((begin + index) as u32)));
                }
            }
        }
        whitespace::finalize(data, start, &mut scan, available, indent, &mut sat);
        scan.hanging_end = scan
            .hanging_end
            .add(scan.punctuation_edges.hang_end, &mut sat);
        let alignment = align::apply(
            data, start, &mut scan, &options, available, indent, &mut sat,
        );
        let (positions, glyph_spacing) =
            spacing::positions(data, start, &mut scan, alignment.justified, &mut sat);
        let origin = offset
            .add(indent, &mut sat)
            .add(alignment.shift, &mut sat)
            .sub(scan.punctuation_edges.hang_start, &mut sat);
        let ruby_measure = scan.ruby.take();
        let mut line = Line::new(
            self,
            token,
            scan,
            origin,
            constraint.block_offset,
            atomics,
            &mut sat,
        );
        line.positions = positions;
        line.glyph_spacing = glyph_spacing;
        line.displaced = displaced;
        reshape::apply(&mut line, cx, &mut sat);
        line.measure_metrics(&mut sat);
        if let Some(measure) = ruby_measure {
            crate::ruby::place::format(data, &measure, &mut line, atomics, cx, &mut sat);
        }
        cx.warnings.record_saturation(&sat);
        if constraint
            .max_block_size
            .is_some_and(|max| line.block_size() > max)
        {
            let needed_block_size = line.block_size();
            if sat.is_clean()
                && cx.warnings.checkpoint() == warning_checkpoint
                && cx
                    .partial
                    .as_ref()
                    .is_some_and(|p| p.retains_trial(self, token))
                && let Some(key) = key
                && key == completed::Key::new(self, token, options, &constraint, atomics)
            {
                cx.completed = completed::CompletedLine::retain(key, line);
            }
            return LineResult::BlockSizeExceeded { needed_block_size };
        }
        LineResult::Line(line)
    }

    /// Format the prepared lane's selected dataset and immutable unit range.
    /// Bypass the root first-line selection and the parent's partial cache.
    pub(crate) fn ruby_line(
        &self,
        cx: &mut LayoutContext,
        units: std::ops::Range<usize>,
        width: f32,
        atomics: &AtomicSizes,
        align: crate::ruby::align::AnnotationAlign<'_>,
    ) -> Line {
        let token = BreakToken {
            para: self.data.id,
            unit: units.start as u32,
            flags: 0,
        };
        let mut constraint = LineConstraint::new(width);
        constraint.floats_placed_through = Some(FloatCursor(u32::MAX));
        match self.next_line_in_set(
            cx,
            token,
            &LineOptions::default(),
            &constraint,
            atomics,
            Some(units.end),
            Some(align),
            None,
        ) {
            LineResult::Line(line) => line,
            _ => unreachable!("prepared annotation ranges contain no block boundaries"),
        }
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

#[cfg(test)]
mod iter_tests;
#[cfg(test)]
mod tests;

pub(super) fn soft_break_reason(
    data: &crate::paragraph::ParagraphData,
    start: usize,
    end: usize,
) -> BreakReason {
    let last = data.units[start..end].iter().rev().find(|u| {
        u.break_after != crate::analysis::units::BreakClass::Prohibited
            || !matches!(u.kind, UnitKind::Close { .. } | UnitKind::BidiControl)
    });
    if last.is_some_and(|u| u.break_after == crate::analysis::units::BreakClass::Emergency) {
        BreakReason::Emergency
    } else {
        BreakReason::Regular
    }
}

/// Closing controls may accompany a cut; opening a new paired box must stay
/// with its continuation so its parent cursor keeps the annotation ranges.
pub(super) fn pulls_after_break(data: &crate::paragraph::ParagraphData, i: usize) -> bool {
    let unit = &data.units[i];
    matches!(unit.kind, UnitKind::Close { .. } | UnitKind::BidiControl)
        && !matches!(
            data.items[unit.item as usize].kind,
            crate::analysis::ItemKind::RubyBoundary {
                boundary: crate::ruby::builder::Boundary::ContainerOpen
                    | crate::ruby::builder::Boundary::BaseOpen(_),
                ..
            }
        )
}
