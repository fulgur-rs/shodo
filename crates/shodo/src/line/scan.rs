use super::Scan;
use crate::analysis::units::{BreakClass, Unit, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::BreakReason;
use crate::paragraph::{AtomicSizes, ParagraphData};
use crate::style::TabSize;

/// The suffix and spacing of a candidate include the closing units that will
/// be pulled onto its line. The live scan/index cursor stays at the text cut.
fn candidate_end(
    data: &ParagraphData,
    mut end: usize,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (usize, LayoutUnit) {
    let mut pulled = LayoutUnit::ZERO;
    while data.units.get(end).is_some() && super::pulls_after_break(data, end) {
        pulled = pulled.add(
            unit_width(data, &data.units[end], LayoutUnit::ZERO, atomics, cx, sat),
            sat,
        );
        end += 1;
    }
    (
        end,
        pulled.add(super::decoration::width(data, end, false, sat), sat),
    )
}

/// Trailing whitespace that hangs past the end of the line being scanned, and
/// the spacing kept before it.
#[derive(Clone, Copy, Default)]
pub(super) struct Tail {
    hanging: LayoutUnit,
    kept_spacing: LayoutUnit,
}

/// One unit's widths as a line end, shared by the eager scan and the cached
/// index so both apply the same edge rules. Edge reshape deltas are left out:
/// the scan measures them only where it must, the index everywhere.
pub(super) struct CandidateMeasure {
    /// Unit index the line would end at.
    end: usize,
    /// Whether the current unit hangs during overflow detection.
    hangs: bool,
    paragraph_end: bool,
    last: bool,
    /// Extent of the visible content through this unit, for overflow.
    shared_extent: LayoutUnit,
    /// Width the line needs when it ends here.
    required: LayoutUnit,
    /// Spacing through `break_end` when a break pulls closing units.
    summary: super::spacing_summary::Summary,
    hyphen: Option<HyphenMeasure>,
    /// Spacing width of the live line through this unit.
    pub(super) tracking: LayoutUnit,
}

impl CandidateMeasure {
    /// Measure unit `at` of advance `w` placed at `pos`, and advance the live
    /// `spacing` and `tail` past it. `hanging_tail` is `None` when the line
    /// cannot break here (grapheme counting), which also skips pulling
    /// closing units.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new<'a>(
        data: &'a ParagraphData,
        start: usize,
        at: usize,
        pos: LayoutUnit,
        w: LayoutUnit,
        indexed: &super::range::Cursor,
        indent: LayoutUnit,
        spacing: &mut super::spacing_summary::Cursor<'a>,
        tail: &mut Tail,
        hanging_tail: Option<&mut super::punctuation::HangingTail>,
        atomics: &AtomicSizes,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Self {
        let unit = &data.units[at];
        let end = at + 1;
        // Fit excludes eligible trailing space/tab advances (CSS Text 3 §4.1.2).
        let hangs = super::whitespace::fits_hanging(data, at);
        // `transparent` excludes forced breaks from the hanging run; the
        // unmasked value still keeps a forced break's hanging state below.
        let raw_transparent = super::whitespace::transparent(data, at);
        let transparent = raw_transparent && !matches!(unit.kind, UnitKind::ForcedBreak);
        let candidates = hanging_tail.is_some();
        if let Some(hanging_tail) = hanging_tail {
            hanging_tail.observe(data, at, spacing, hangs, transparent);
        }
        super::spacing::push(data, spacing, at);
        let tracking = spacing.summary(Some(data)).width(sat);
        // A selected break also pulls adjacent closing units. Reserve their
        // actual edges now, and measure Clone decoration after them so those
        // edges are not counted twice. The candidate summary includes their
        // punctuation-hanging barriers, while the live spacing stays unchanged.
        let (break_end, suffix) = if candidates
            && matches!(
                unit.break_after,
                BreakClass::Allowed | BreakClass::Emergency | BreakClass::Hyphen
            ) {
            candidate_end(data, end, atomics, cx, sat)
        } else {
            (end, super::decoration::width(data, end, false, sat))
        };
        let ruby_delta =
            crate::ruby::measure::candidate_adjustment(data, start, end, atomics, cx, sat);
        let paragraph_end = end == data.units.len();
        let Tail {
            hanging,
            kept_spacing,
        } = *tail;
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
        // A hanging final space or tab must not hide overflow of the visible
        // content. Its advance and trailing tracking are excluded, while the
        // line-start reshape still contributes to the fit width.
        let shared_extent = if paragraph_end && hangs {
            shared_extent
                .sub(hanging.add(w, sat), sat)
                .sub(tracking.sub(kept_spacing, sat), sat)
        } else {
            shared_extent
        };
        match unit.kind {
            _ if hangs => tail.hanging = tail.hanging.add(w, sat),
            _ if raw_transparent => {}
            _ => {
                tail.hanging = LayoutUnit::ZERO;
                tail.kept_spacing = tracking;
            }
        }
        if tail.hanging == LayoutUnit::ZERO {
            tail.kept_spacing = tracking;
        }
        let natural = indexed.measure(data, break_end, None, &cx.ruby_ranges, sat);
        let required = natural.content.add(indent, sat).add(ruby_delta, sat);
        let hyphen = (unit.break_after == BreakClass::Hyphen).then(|| {
            let leaf = super::spacing::hyphen_unit_summary(data, end - 1, sat);
            let natural =
                indexed.measure(data, break_end, Some((end - 1, leaf)), &cx.ruby_ranges, sat);
            HyphenMeasure {
                end,
                natural: natural.content.add(indent, sat).add(ruby_delta, sat),
                summary: natural.summary,
            }
        });
        Self {
            end,
            hangs,
            paragraph_end,
            last: super::punctuation::last_edge(data, end),
            shared_extent,
            required,
            summary: natural.summary,
            hyphen,
            tracking,
        }
    }

    /// The spacing summary of the line ending here.
    fn summary<'a>(
        &self,
        _data: &'a ParagraphData,
        _live: &super::spacing_summary::Cursor<'a>,
    ) -> super::spacing_summary::Summary {
        self.summary
    }

    /// The candidate's widths, to test or defer its fit.
    fn pending<'a>(
        &self,
        data: &'a ParagraphData,
        live: &super::spacing_summary::Cursor<'a>,
    ) -> Pending {
        Pending {
            end: self.end,
            hangs: self.hangs,
            shared_extent: self.shared_extent,
            required: self.required,
            summary: self.summary(data, live),
            last: self.last,
        }
    }

    /// Whether overflow of this unit is tested at all: hanging whitespace
    /// cannot overflow, except at the paragraph end.
    pub(super) fn tests_overflow(&self) -> bool {
        !self.hangs || self.paragraph_end
    }

    /// The extent of the visible content through this unit with its edge
    /// delta, after punctuation at its edges. A hanging tail exposes the
    /// punctuation before it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn overflow_width<'a>(
        &self,
        data: &'a ParagraphData,
        live: &super::spacing_summary::Cursor<'a>,
        hanging_tail: &mut super::punctuation::HangingTail,
        edge_delta: LayoutUnit,
        flags: u8,
        options: &crate::style::LineOptions,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        let summary = hanging_tail
            .summary(data, self.end)
            .unwrap_or_else(|| self.summary(data, live));
        let extent = self.shared_extent.add(edge_delta, sat);
        fit_width(data, summary, flags, self.last, options, extent, sat)
    }

    /// Whether this unit overflows once its edge delta is added. Edge
    /// adjustments only ever remove width (see `EdgeAdjustment`), so a
    /// candidate that already fits is not adjusted.
    #[allow(clippy::too_many_arguments)]
    fn overflows<'a>(
        &self,
        data: &'a ParagraphData,
        live: &super::spacing_summary::Cursor<'a>,
        hanging_tail: &mut super::punctuation::HangingTail,
        edge_delta: LayoutUnit,
        flags: u8,
        options: &crate::style::LineOptions,
        available: LayoutUnit,
        sat: &mut Saturation,
    ) -> bool {
        self.tests_overflow()
            && self.shared_extent.add(edge_delta, sat) > available
            && self.overflow_width(data, live, hanging_tail, edge_delta, flags, options, sat)
                > available
    }

    /// The width the line needs to end here, given its edge delta.
    pub(super) fn required_width<'a>(
        &self,
        data: &'a ParagraphData,
        live: &super::spacing_summary::Cursor<'a>,
        edge_delta: LayoutUnit,
        flags: u8,
        options: &crate::style::LineOptions,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        self.pending(data, live)
            .required_width(data, edge_delta, flags, options, sat)
    }

    /// The width the line needs to end here with a discretionary hyphen.
    pub(super) fn hyphen<'a>(
        &self,
        _data: &'a ParagraphData,
        _spacing: &super::spacing_summary::Cursor<'a>,
        _sat: &mut Saturation,
    ) -> HyphenMeasure {
        self.hyphen.expect("a discretionary hyphen candidate")
    }
}

/// The width left of a line of natural width `natural` after punctuation
/// trimming and hanging at its edges. Every fit test compares this with the
/// available width.
#[allow(clippy::too_many_arguments)]
fn fit_width(
    data: &ParagraphData,
    summary: super::spacing_summary::Summary,
    flags: u8,
    last: bool,
    options: &crate::style::LineOptions,
    natural: LayoutUnit,
    sat: &mut Saturation,
) -> LayoutUnit {
    super::range::finish_width(
        data,
        summary,
        flags,
        last,
        Some(options),
        LayoutUnit::ZERO,
        natural,
        LayoutUnit::ZERO,
        sat,
    )
    .0
}

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
    let mut indexed = super::range::Cursor::new(data, start, offset.add(indent, sat), atomics, cx);
    let prefix = super::decoration::width(data, start, true, sat);
    let mut pos = indent.add(prefix, sat);
    let mut last_break: Option<(usize, bool)> = None;
    let mut taken_hyphen = None;
    let mut first_hyphen = None;
    let mut last_emergency: Option<usize> = None;
    let mut overflowing = false;
    let mut tail = Tail::default();
    let mut spacing = super::spacing_summary::Cursor::default();
    let mut hanging_tail = super::punctuation::HangingTail::default();
    let mut i = start;
    let mut graphemes_seen = 0usize;
    let mut counted_through = units[start].text.start;
    let max_graphemes = max_graphemes.map(|limit| limit.max(1));
    let count_mode = max_graphemes.is_some();
    // Break candidates after the latest verified break whose measurement
    // waits until the line overflows, in line order.
    let mut pending: std::collections::VecDeque<Deferred> = Default::default();
    let reason = loop {
        // A line that ends here without overflowing still has to fit at its
        // last opportunity, whose edge reshape may have been deferred. Box
        // ends and bidi controls scanned after that opportunity belong to the
        // cut (`pulls_after_break`), so the line still ends there logically.
        let terminating = units.get(i).is_none_or(|unit| match unit.kind {
            UnitKind::ForcedBreak => !count_mode,
            UnitKind::BlockInInline { .. } => true,
            _ => false,
        });
        if terminating
            && let Some(last) = pending.back().map(Deferred::plain)
            && last.end <= i
            && super::pulled_end(data, last.end, i) == i
            && !last.hangs
        {
            let (edge_delta, _) = super::windows::candidate(data, start, last.end, cx, sat);
            if last.overflows(data, edge_delta, flags, options, available, sat) {
                let last = pending.pop_back().expect("checked above");
                if let Some(b) = settle(
                    data,
                    start,
                    &mut pending,
                    &mut first_hyphen,
                    flags,
                    options,
                    available,
                    cx,
                    sat,
                ) {
                    last_break = Some(b);
                }
                if let Some(reason) = take_fallback(
                    start,
                    &mut last_break,
                    last_emergency,
                    &mut first_hyphen,
                    &mut taken_hyphen,
                    &mut widths,
                    &mut i,
                ) {
                    break reason;
                }
                // The run overflows with no earlier break, so it ends here,
                // hyphenated when that is possible.
                if let Deferred::Hyphen(hyphen) = &last
                    && hyphen
                        .hyphen
                        .required(data, start, flags, options, cx, sat)
                        .is_some()
                {
                    taken_hyphen = Some(i);
                    break BreakReason::Regular;
                }
            }
        }
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
        let w = indexed.advance(data, i, atomics, cx, sat);
        let measure = CandidateMeasure::new(
            data,
            start,
            i,
            pos,
            w,
            &indexed,
            indent,
            &mut spacing,
            &mut tail,
            (!count_mode).then_some(&mut hanging_tail),
            atomics,
            cx,
            sat,
        );
        let (hangs, paragraph_end) = (measure.hangs, measure.paragraph_end);
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
        // get the real measurement. A slice cut only to give the first line
        // a resume cursor is not an end the normal set can take, so it
        // defers that confirmation to the cluster's own last slice. A
        // discretionary hyphen position is the same for its unhyphenated
        // edge: the line can end there only with the hyphen, which is
        // measured separately, so the plain edge only confirms overflow.
        // The paragraph end must include the line-start reshape even when
        // its final glyph or closing controls prohibit a break. Measure it
        // once here rather than probing every prohibited position.
        let need_edge = if count_mode {
            limited_cut
        } else {
            !matches!(
                unit.break_after,
                BreakClass::Prohibited | BreakClass::Hyphen
            ) || paragraph_end
                || unit.shared_cluster.is_some() && !unit.first_line_cursor
                || (!hangs && !overflowing && measure.shared_extent > available)
        };
        // Once a fitting break is known, an `Allowed` opportunity whose shared
        // advances fit only matters if no later one fits. With kerning fonts
        // most word ends are unsafe to break, so measuring the edge reshape
        // of every opportunity costs a shaping call per word. Defer it: when
        // the line overflows, `settle` measures the deferred opportunities
        // from the last one backwards and takes the first that really fits,
        // like a browser that reshapes only the end of the line it chose.
        let defer = !count_mode
            && !paragraph_end
            && !overflowing
            && last_break.is_some()
            && unit.break_after == BreakClass::Allowed
            && unit.shared_cluster.is_none()
            && (hangs || measure.shared_extent <= available);
        let (edge_delta, viable) = if need_edge && !defer {
            super::windows::candidate(data, start, i + 1, cx, sat)
        } else {
            (LayoutUnit::ZERO, false)
        };
        if !count_mode
            && !overflowing
            && measure.overflows(
                data,
                &spacing,
                &mut hanging_tail,
                edge_delta,
                flags,
                options,
                available,
                sat,
            )
        {
            if let Some(b) = settle(
                data,
                start,
                &mut pending,
                &mut first_hyphen,
                flags,
                options,
                available,
                cx,
                sat,
            ) {
                last_break = Some(b);
            }
            if let Some(reason) = take_fallback(
                start,
                &mut last_break,
                last_emergency,
                &mut first_hyphen,
                &mut taken_hyphen,
                &mut widths,
                &mut i,
            ) {
                break reason;
            }
            // No opportunity yet: the unbreakable run overflows
            // (`overflow-wrap: normal`) and the line ends at the next one.
            overflowing = true;
        }
        widths.push(w);
        pos = pos.add(w, sat);
        i += 1;
        if count_mode {
            if limited_cut && viable {
                break BreakReason::Regular;
            }
            continue;
        }
        match unit.break_after {
            BreakClass::Mandatory => break BreakReason::Forced,
            BreakClass::Allowed if defer => defer_candidate(
                Deferred::Opportunity(measure.pending(data, &spacing)),
                data,
                start,
                &mut pending,
                &mut last_break,
                &mut first_hyphen,
                flags,
                options,
                available,
                cx,
                sat,
            ),
            BreakClass::Allowed if viable => {
                if overflowing {
                    break BreakReason::Regular;
                }
                if measure.required_width(data, &spacing, edge_delta, flags, options, sat)
                    <= available
                {
                    last_break = Some((i, false));
                    pending.clear();
                }
            }
            BreakClass::Hyphen => {
                let hyphen = PendingHyphen {
                    plain: measure.pending(data, &spacing),
                    hyphen: measure.hyphen(data, &spacing, sat),
                };
                if overflowing {
                    if hyphen
                        .hyphen
                        .required(data, start, flags, options, cx, sat)
                        .is_some()
                    {
                        taken_hyphen = Some(i);
                        break BreakReason::Regular;
                    }
                } else {
                    defer_candidate(
                        Deferred::Hyphen(hyphen),
                        data,
                        start,
                        &mut pending,
                        &mut last_break,
                        &mut first_hyphen,
                        flags,
                        options,
                        available,
                        cx,
                        sat,
                    );
                }
            }
            BreakClass::Emergency if viable => {
                if overflowing {
                    break BreakReason::Emergency;
                }
                if measure.required_width(data, &spacing, edge_delta, flags, options, sat)
                    <= available
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
        while units.get(i).is_some() && super::pulls_after_break(data, i) {
            widths.push(indexed.advance(data, i, atomics, cx, sat));
            i += 1;
        }
    }
    let mut result = indexed.selected(
        data,
        i,
        widths,
        indent,
        flags,
        Some(options),
        available,
        taken_hyphen,
        cx,
        sat,
    );
    result.reason = reason;
    result
}

/// The break an overflowing line falls back to, in priority order: the last
/// verified break (hyphenated when its edge says so), the last emergency
/// break, then the first measurable hyphen. The chosen break truncates
/// `widths` and moves `i` back to it. Only the regular and hyphen arms set
/// `taken_hyphen`; an emergency break leaves it unchanged. `None` changes
/// nothing.
#[allow(clippy::too_many_arguments)]
fn take_fallback(
    start: usize,
    last_break: &mut Option<(usize, bool)>,
    last_emergency: Option<usize>,
    first_hyphen: &mut Option<usize>,
    taken_hyphen: &mut Option<usize>,
    widths: &mut Vec<LayoutUnit>,
    i: &mut usize,
) -> Option<BreakReason> {
    let (b, hyphen, reason) = if let Some((b, edge)) = last_break.take() {
        (b, Some(edge.then_some(b)), BreakReason::Regular)
    } else if let Some(b) = last_emergency {
        (b, None, BreakReason::Emergency)
    } else {
        let b = first_hyphen.take()?;
        (b, Some(Some(b)), BreakReason::Regular)
    };
    if let Some(hyphen) = hyphen {
        *taken_hyphen = hyphen;
    }
    widths.truncate(b - start);
    *i = b;
    Some(reason)
}

/// A break candidate's widths without its edge reshape, kept while that
/// reshape is deferred.
struct Pending {
    /// Unit index the line would end at.
    end: usize,
    hangs: bool,
    shared_extent: LayoutUnit,
    /// The line's required width at `end` without the edge reshape delta.
    required: LayoutUnit,
    summary: super::spacing_summary::Summary,
    last: bool,
}

impl Pending {
    /// Whether the unit before `end` overflows once its edge delta is added.
    fn overflows(
        &self,
        data: &ParagraphData,
        edge_delta: LayoutUnit,
        flags: u8,
        options: &crate::style::LineOptions,
        available: LayoutUnit,
        sat: &mut Saturation,
    ) -> bool {
        let extent = self.shared_extent.add(edge_delta, sat);
        !self.hangs
            && extent > available
            && fit_width(data, self.summary, flags, self.last, options, extent, sat) > available
    }

    /// The width the line needs to end at `end`, given its edge delta.
    fn required_width(
        &self,
        data: &ParagraphData,
        edge_delta: LayoutUnit,
        flags: u8,
        options: &crate::style::LineOptions,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        let required = self.required.add(edge_delta, sat);
        fit_width(data, self.summary, flags, self.last, options, required, sat)
    }

    /// Whether the line can end at `end`, given the measured edge reshape.
    fn fits(
        &self,
        data: &ParagraphData,
        (edge_delta, viable): (LayoutUnit, bool),
        flags: u8,
        options: &crate::style::LineOptions,
        available: LayoutUnit,
        sat: &mut Saturation,
    ) -> bool {
        viable
            && !self.overflows(data, edge_delta, flags, options, available, sat)
            && self.required_width(data, edge_delta, flags, options, sat) <= available
    }
}

/// A break candidate whose measurement is deferred until the line overflows.
enum Deferred {
    Opportunity(Pending),
    Hyphen(PendingHyphen),
}

impl Deferred {
    /// The candidate's position as an unhyphenated line end.
    fn plain(&self) -> &Pending {
        match self {
            Self::Opportunity(opportunity) => opportunity,
            Self::Hyphen(hyphen) => &hyphen.plain,
        }
    }

    /// The verified break this candidate gives, if it fits. A measurable
    /// hyphen that does not fit is reported through `unfit_hyphen`.
    #[allow(clippy::too_many_arguments)]
    fn verify(
        &self,
        data: &ParagraphData,
        start: usize,
        unfit_hyphen: &mut Option<usize>,
        flags: u8,
        options: &crate::style::LineOptions,
        available: LayoutUnit,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Option<(usize, bool)> {
        match self {
            Self::Opportunity(opportunity) => {
                let measured = super::windows::candidate(data, start, opportunity.end, cx, sat);
                opportunity
                    .fits(data, measured, flags, options, available, sat)
                    .then_some((opportunity.end, false))
            }
            Self::Hyphen(hyphen) => {
                let hyphen = &hyphen.hyphen;
                let required = hyphen.required(data, start, flags, options, cx, sat)?;
                if required <= available {
                    return Some((hyphen.end, true));
                }
                *unfit_hyphen = Some(hyphen.end);
                None
            }
        }
    }
}

/// Deferred candidates kept per scan before the oldest is measured.
pub(super) const DEFERRED_MAX: usize = 256;

/// Defer a candidate. On very wide lines, keep the retained state bounded by
/// measuring the oldest candidate now; it is earlier than every candidate
/// still deferred, so a later fitting one still takes precedence.
#[allow(clippy::too_many_arguments)]
fn defer_candidate(
    candidate: Deferred,
    data: &ParagraphData,
    start: usize,
    pending: &mut std::collections::VecDeque<Deferred>,
    last_break: &mut Option<(usize, bool)>,
    first_hyphen: &mut Option<usize>,
    flags: u8,
    options: &crate::style::LineOptions,
    available: LayoutUnit,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    if pending.len() == DEFERRED_MAX
        && let Some(oldest) = pending.pop_front()
    {
        let mut unfit = None;
        if let Some(b) = oldest.verify(data, start, &mut unfit, flags, options, available, cx, sat)
        {
            *last_break = Some(b);
        }
        if first_hyphen.is_none() {
            *first_hyphen = unfit;
        }
    }
    pending.push_back(candidate);
}

/// Measure deferred candidates from the last one backwards and return the
/// first that fits, which is the last fitting one in line order. All of them
/// are consumed. `None` keeps the verified break found before them; when no
/// hyphen fits, the first measurable one becomes the overflow fallback, as an
/// eager scan would have recorded it.
#[allow(clippy::too_many_arguments)]
fn settle(
    data: &ParagraphData,
    start: usize,
    pending: &mut std::collections::VecDeque<Deferred>,
    first_hyphen: &mut Option<usize>,
    flags: u8,
    options: &crate::style::LineOptions,
    available: LayoutUnit,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<(usize, bool)> {
    let mut unfit = None;
    while let Some(candidate) = pending.pop_back() {
        if let Some(b) =
            candidate.verify(data, start, &mut unfit, flags, options, available, cx, sat)
        {
            pending.clear();
            return Some(b);
        }
    }
    if first_hyphen.is_none() {
        *first_hyphen = unfit;
    }
    None
}

/// A discretionary hyphen break candidate, with the scan state needed to
/// measure it later. Shaping the hyphenated edge is the costly part, and a
/// line can only end at its last fitting candidate, so the measurement waits
/// until the line overflows.
struct PendingHyphen {
    /// The same position ending without a hyphen. Its edge is measured only
    /// when the line ends there, where it must still fit.
    plain: Pending,
    hyphen: HyphenMeasure,
}

/// The widths of a line ending at a discretionary hyphen, before the
/// hyphenated edge is shaped.
#[derive(Clone, Copy)]
pub(super) struct HyphenMeasure {
    end: usize,
    natural: LayoutUnit,
    summary: super::spacing_summary::Summary,
}

impl HyphenMeasure {
    /// The width needed to end the line here with a hyphen, or `None` when
    /// the hyphenated edge cannot be shaped.
    pub(super) fn required(
        &self,
        data: &ParagraphData,
        start: usize,
        flags: u8,
        options: &crate::style::LineOptions,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Option<LayoutUnit> {
        let windows = super::hyphen::line(data, start, self.end, cx, sat)?;
        let required = self
            .natural
            .add(super::windows::cost(&windows, self.end, sat), sat);
        Some(fit_width(
            data,
            self.summary,
            flags,
            false,
            options,
            required,
            sat,
        ))
    }
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
                cx.warnings.push_lazy(WarningKind::MissingAtomicSize, || {
                    format!("no size for atomic inline {node:?}")
                });
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
        TabSize::Spaces(n) => {
            n * (metrics.space + block.letter_spacing + block.used_word_spacing(metrics.space))
        }
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
