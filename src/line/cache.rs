use super::{Scan, decoration, hyphen, scan::scan};
use crate::analysis::units::{BreakClass, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::node::NodeId;
use crate::paragraph::{
    AtomicSizes, BreakToken, FloatCursor, LineConstraint, Paragraph, ParagraphData,
};
use crate::style::LineOptions;
use std::sync::Arc;

/// Candidate costs need not increase with text position (fonts can change).
/// The frontier retains the latest candidate for each increasing cost. Undo
/// records restore dominated candidates when a shrinking scan drops its tail.
#[derive(Default)]
struct Candidates {
    candidates: Vec<(usize, LayoutUnit)>,
    frontier: Vec<usize>,
    undo: Vec<std::ops::Range<usize>>,
    popped: Vec<usize>,
    active: usize,
    fitting: usize,
    #[cfg(test)]
    visits: usize,
}

impl Candidates {
    fn push(&mut self, end: usize, required: LayoutUnit) {
        let begin = self.popped.len();
        while self
            .frontier
            .last()
            .is_some_and(|i| self.candidates[*i].1 >= required)
        {
            self.popped.push(self.frontier.pop().unwrap());
            #[cfg(test)]
            {
                self.visits += 1;
            }
        }
        self.undo.push(begin..self.popped.len());
        self.frontier.push(self.candidates.len());
        self.candidates.push((end, required));
        self.active = self.candidates.len();
        self.fitting = self.frontier.len();
    }

    fn select(&mut self, through: usize, limit: LayoutUnit) -> Option<usize> {
        while self.active > 0 && self.candidates[self.active - 1].0 > through {
            self.active -= 1;
            #[cfg(test)]
            {
                self.visits += 1;
            }
            let removed = self.frontier.pop();
            debug_assert_eq!(removed, Some(self.active));
            for i in self.popped[self.undo[self.active].clone()].iter().rev() {
                self.frontier.push(*i);
                #[cfg(test)]
                {
                    self.visits += 1;
                }
            }
        }
        self.fitting = self.fitting.min(self.frontier.len());
        while self.fitting > 0 && self.candidates[self.frontier[self.fitting - 1]].1 > limit {
            self.fitting -= 1;
            #[cfg(test)]
            {
                self.visits += 1;
            }
        }
        while self.fitting < self.frontier.len()
            && self.candidates[self.frontier[self.fitting]].1 <= limit
        {
            self.fitting += 1;
            #[cfg(test)]
            {
                self.visits += 1;
            }
        }
        self.fitting
            .checked_sub(1)
            .map(|i| self.candidates[self.frontier[i]].0)
    }

    fn bytes(&self) -> usize {
        self.candidates.capacity() * std::mem::size_of::<(usize, LayoutUnit)>()
            + (self.frontier.capacity() + self.popped.capacity()) * std::mem::size_of::<usize>()
            + self.undo.capacity() * std::mem::size_of::<std::ops::Range<usize>>()
    }
}

pub(crate) struct PartialLine {
    data: Arc<ParagraphData>,
    token: BreakToken,
    revision: u64,
    options: LineOptions,
    cursor: Option<FloatCursor>,
    width: LayoutUnit,
    offset: LayoutUnit,
    indent: LayoutUnit,
    scan: Scan,
    prefix: Vec<LayoutUnit>,
    tracking: Vec<LayoutUnit>,
    thresholds: Vec<(usize, LayoutUnit)>,
    breaks: Candidates,
    emergencies: Candidates,
    hyphens: Candidates,
    floats: Vec<(usize, NodeId, u32)>,
    threshold_cursor: usize,
    float_index: usize,
}

impl std::fmt::Debug for PartialLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartialLine")
            .field("token", &self.token)
            .field("units", &self.scan.widths.len())
            .finish()
    }
}

impl PartialLine {
    pub(crate) fn bytes(&self) -> usize {
        let overlay_bytes: usize = self
            .scan
            .overlays
            .iter()
            .map(|overlay| {
                let store = &overlay.store;
                (store.id.capacity() + store.cluster.capacity()) * std::mem::size_of::<u32>()
                    + (store.advance.capacity()
                        + store.pen.capacity()
                        + store.offset_inline.capacity()
                        + store.offset_block.capacity()
                        + store.spacing.as_ref().map_or(0, Vec::capacity)
                        + store.leading.as_ref().map_or(0, Vec::capacity))
                        * std::mem::size_of::<LayoutUnit>()
                    + store.flags.capacity() * std::mem::size_of::<u8>()
                    + overlay.runs.capacity() * std::mem::size_of::<crate::shape::ShapedRun>()
            })
            .sum();
        std::mem::size_of::<Self>()
            + self.scan.widths.capacity() * std::mem::size_of::<LayoutUnit>()
            + self.scan.leading.as_ref().map_or(0, Vec::capacity)
                * std::mem::size_of::<LayoutUnit>()
            + self.scan.autospace_gaps.capacity() * std::mem::size_of::<super::autospace::Gap>()
            + self.scan.ruby_caret_gaps.capacity()
                * std::mem::size_of::<crate::ruby::align::CaretGap>()
            + self.scan.overlays.capacity() * std::mem::size_of::<super::reshape::EdgeOverlay>()
            + overlay_bytes
            + self.prefix.capacity() * std::mem::size_of::<LayoutUnit>()
            + self.tracking.capacity() * std::mem::size_of::<LayoutUnit>()
            + self.thresholds.capacity() * std::mem::size_of::<(usize, LayoutUnit)>()
            + self.breaks.bytes()
            + self.emergencies.bytes()
            + self.hyphens.bytes()
            + self.floats.capacity() * std::mem::size_of::<(usize, NodeId, u32)>()
    }
    /// Prepare prefix/frontier searches only when a float or a narrower
    /// retry needs them. On failure the original scan remains untouched.
    fn index(
        &mut self,
        atomics: &AtomicSizes,
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> bool {
        let data = &self.data;
        let token = self.token;
        let start = token.unit as usize;
        let units = &data.units[start..self.scan.end];
        let options = &self.options;
        let indent = self.indent;
        let warning_checkpoint = cx.warnings.checkpoint();
        // A cached prefix always describes the unbroken text. The selected
        // discretionary glyph is materialized afresh after choosing an end.
        let natural_widths: Vec<_> = units
            .iter()
            .enumerate()
            .map(|(k, u)| {
                super::scan::unit_width_from(
                    data,
                    u,
                    data.units[start].text.start,
                    LayoutUnit::ZERO,
                    atomics,
                    cx,
                    sat,
                )
                .add(data.unit_spacing[start + k].word, sat)
            })
            .collect();
        if natural_widths.iter().any(|w| *w < LayoutUnit::ZERO) {
            return false;
        }
        let mut hyphens = Candidates::default();
        let mut prefix = vec![decoration::width(data, start, true, sat)];
        let mut thresholds = Vec::new();
        let mut breaks = Candidates::default();
        let mut emergencies = Candidates::default();
        let mut floats = Vec::new();
        let mut max = LayoutUnit::ZERO;
        let mut hanging = LayoutUnit::ZERO;
        let mut spacing = super::spacing_summary::Cursor::default();
        let mut tracking = vec![LayoutUnit::ZERO];
        let mut kept_spacing = LayoutUnit::ZERO;
        for (k, (u, w)) in units.iter().zip(&natural_widths).enumerate() {
            #[cfg(test)]
            {
                cx.cache_prepare_visits += 1;
            }
            super::spacing::push(data, &mut spacing, start + k);
            let spacing_width = spacing.summary(Some(data)).width(sat);
            tracking.push(spacing_width);
            if let UnitKind::Float { node, ordinal } = u.kind {
                floats.push((start + k, node, ordinal));
            }
            let next = prefix[k].add(*w, sat);
            prefix.push(next);
            let (delta, viable) = super::windows::candidate(data, start, start + k + 1, cx, sat);
            let delta = delta.add(
                crate::ruby::measure::candidate(data, start, start + k + 1, atomics, cx, sat)
                    .adjustment,
                sat,
            );
            let suffix = decoration::width(data, start + k + 1, false, sat);
            let transparent = super::whitespace::transparent(data, start + k)
                && !matches!(u.kind, UnitKind::ForcedBreak);
            if k > 0
                && !super::whitespace::fits_hanging(data, start + k)
                && !matches!(u.kind, UnitKind::ForcedBreak)
            {
                let extent = next
                    .add(spacing_width, sat)
                    .add(delta, sat)
                    .add(suffix, sat)
                    .sub(
                        if transparent {
                            hanging.add(spacing_width.sub(kept_spacing, sat), sat)
                        } else {
                            LayoutUnit::ZERO
                        },
                        sat,
                    );
                let adjustment = super::punctuation::edges(
                    data,
                    spacing.summary(Some(data)),
                    token.flags,
                    super::punctuation::last_edge(data, start + k + 1),
                    options,
                    LayoutUnit::ZERO,
                    extent,
                    sat,
                );
                max = max.max(extent.sub(adjustment.removed(sat), sat));
                thresholds.push((start + k, max));
            }
            match u.kind {
                _ if super::whitespace::fits_hanging(data, start + k) => {
                    hanging = hanging.add(*w, sat)
                }
                _ if super::whitespace::transparent(data, start + k) => {}
                _ => {
                    hanging = LayoutUnit::ZERO;
                    kept_spacing = spacing_width;
                }
            }
            if hanging == LayoutUnit::ZERO {
                kept_spacing = spacing_width;
            }
            let required = next
                .add(kept_spacing, sat)
                .add(delta, sat)
                .sub(hanging, sat)
                .add(suffix, sat);
            let adjustment = super::punctuation::edges(
                data,
                spacing.summary(Some(data)),
                token.flags,
                super::punctuation::last_edge(data, start + k + 1),
                options,
                LayoutUnit::ZERO,
                required.add(indent, sat),
                sat,
            );
            let required = required.sub(adjustment.removed(sat), sat);
            if u.break_after == BreakClass::Hyphen {
                if let Some(windows) = hyphen::line(data, start, start + k + 1, cx, sat) {
                    let summary = super::spacing::hyphen_summary(data, &spacing, start + k, sat);
                    let required = next
                        .add(summary.width(sat), sat)
                        .sub(hanging, sat)
                        .add(suffix, sat)
                        .add(super::windows::cost(&windows, start + k + 1, sat), sat);
                    let required = required.add(
                        crate::ruby::measure::candidate(
                            data,
                            start,
                            start + k + 1,
                            atomics,
                            cx,
                            sat,
                        )
                        .adjustment,
                        sat,
                    );
                    let adjustment = super::punctuation::edges(
                        data,
                        summary,
                        token.flags,
                        false,
                        options,
                        LayoutUnit::ZERO,
                        required.add(indent, sat),
                        sat,
                    );
                    let required = required.sub(adjustment.removed(sat), sat);
                    hyphens.push(start + k + 1, required);
                }
            } else if viable && u.break_after == BreakClass::Allowed {
                breaks.push(start + k + 1, required);
            } else if viable && u.break_after == BreakClass::Emergency {
                emergencies.push(start + k + 1, required);
            }
        }
        if !sat.is_clean()
            || warning_checkpoint.is_none()
            || cx.warnings.checkpoint() != warning_checkpoint
            || prefix.last().unwrap().raw() as i64 + indent.raw() as i64 > i32::MAX as i64
        {
            return false;
        }
        self.scan.widths = natural_widths;
        self.scan.overlays.clear();
        self.scan.prepared = false;
        self.threshold_cursor = thresholds.len();
        self.prefix = prefix;
        self.tracking = tracking;
        self.thresholds = thresholds;
        self.breaks = breaks;
        self.emergencies = emergencies;
        self.hyphens = hyphens;
        self.floats = floats;
        true
    }

    fn end(
        &mut self,
        available: LayoutUnit,
        indent: LayoutUnit,
        sat: &mut Saturation,
    ) -> (usize, Option<usize>) {
        let limit = available.sub(indent, sat);
        while self.threshold_cursor > 0 && self.thresholds[self.threshold_cursor - 1].1 > limit {
            self.threshold_cursor -= 1;
        }
        while self.threshold_cursor < self.thresholds.len()
            && self.thresholds[self.threshold_cursor].1 <= limit
        {
            self.threshold_cursor += 1;
        }
        let bad = self.threshold_cursor;
        let Some(&(i, _)) = self.thresholds.get(bad) else {
            let hyphen = (self.scan.reason == crate::output::BreakReason::Regular)
                .then(|| self.hyphens.candidates.last().map(|c| c.0))
                .flatten()
                .filter(|end| {
                    (*end..self.scan.end).all(|at| {
                        matches!(
                            self.data.units[at].kind,
                            UnitKind::Close { .. } | UnitKind::BidiControl
                        )
                    })
                });
            return (self.scan.end, hyphen);
        };
        let normal = self.breaks.select(i, limit);
        let emergency = self.emergencies.select(i, limit);
        let fitting_hyphen = self.hyphens.select(i, limit);
        let preferred = normal.into_iter().chain(fitting_hyphen).max();
        let mut end = preferred.or(emergency).unwrap_or_else(|| {
            self.breaks
                .candidates
                .first()
                .map(|c| c.0)
                .into_iter()
                .chain(self.emergencies.candidates.first().map(|c| c.0))
                .chain(self.hyphens.candidates.first().map(|c| c.0))
                .min()
                .unwrap_or(self.scan.end)
        });
        let hyphen = self
            .hyphens
            .candidates
            .binary_search_by_key(&end, |c| c.0)
            .ok()
            .map(|_| end);
        while end < self.scan.end && super::pulls_after_break(&self.data, end) {
            end += 1;
        }
        (end, hyphen)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve(
    para: &Paragraph,
    token: BreakToken,
    options: &LineOptions,
    constraint: &LineConstraint<'_>,
    available: LayoutUnit,
    offset: LayoutUnit,
    indent: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Result<Scan, (NodeId, u32, LayoutUnit)> {
    let start = token.unit as usize;
    let valid = sat.is_clean()
        && cx.partial.as_ref().is_some_and(|p| {
            Arc::ptr_eq(&p.data, &para.data)
                && p.token == token
                && p.revision == atomics.revision
                && p.options == *options
                && p.offset == offset
                && p.indent == indent
                && p.width >= available
                && constraint.floats_placed_through >= p.cursor
        });
    let mut cached = if valid {
        cx.partial.take().unwrap()
    } else {
        cx.partial = None;
        let warning_checkpoint = cx.warnings.checkpoint();
        let scanned = scan(
            &para.data,
            start,
            available,
            offset,
            indent,
            token.flags,
            options,
            atomics,
            cx,
            sat,
        );
        #[cfg(test)]
        {
            cx.cache_visits += scanned.end - start;
        }
        let units = &para.data.units[start..scanned.end];
        let safe = indent >= LayoutUnit::ZERO
            && scanned.widths.iter().all(|w| *w >= LayoutUnit::ZERO)
            && !units.iter().any(|u| matches!(u.kind, UnitKind::Tab))
            && sat.is_clean()
            && warning_checkpoint.is_some()
            && cx.warnings.checkpoint() == warning_checkpoint;
        if !safe {
            return Ok(scanned);
        }
        let has_float = units
            .iter()
            .any(|u| matches!(u.kind, UnitKind::Float { .. }));
        let cached = PartialLine {
            data: Arc::clone(&para.data),
            token,
            revision: atomics.revision,
            options: *options,
            cursor: constraint.floats_placed_through,
            width: available,
            offset,
            indent,
            scan: scanned,
            prefix: Vec::new(),
            tracking: Vec::new(),
            thresholds: Vec::new(),
            breaks: Candidates::default(),
            emergencies: Candidates::default(),
            hyphens: Candidates::default(),
            floats: Vec::new(),
            threshold_cursor: 0,
            float_index: 0,
        };
        if !has_float {
            // Ordinary first calls retain one raw result. They do not repeat
            // unit measurement or build candidate searches until a retry
            // actually needs a smaller width.
            let result = cached.scan.clone();
            cx.partial = Some(cached);
            return Ok(result);
        }
        cached
    };
    if cached.prefix.is_empty() {
        if valid && cached.width == available {
            cached.cursor = constraint.floats_placed_through;
            let result = cached.scan.clone();
            cx.partial = Some(cached);
            return Ok(result);
        }
        let warnings_before = cx.warnings.clone();
        let saturation_before = *sat;
        if !cached.index(atomics, cx, sat) {
            // Speculative index preparation must not add diagnostics to the
            // requested line. A failed narrower retry needs a fresh scan at
            // its own width, rather than the retained wider result.
            cx.warnings = warnings_before;
            *sat = saturation_before;
            if valid {
                return resolve(
                    para, token, options, constraint, available, offset, indent, atomics, cx, sat,
                );
            }
            return Ok(cached.scan);
        }
    }
    cx.partial = Some(cached);
    let p = cx.partial.as_mut().unwrap();
    p.cursor = constraint.floats_placed_through;
    p.width = available;
    let (end, selected_hyphen) = p.end(available, indent, sat);
    while p.floats.get(p.float_index).is_some_and(|(_, _, ordinal)| {
        constraint
            .floats_placed_through
            .is_some_and(|c| *ordinal <= c.0)
    }) {
        p.float_index += 1;
    }
    if let Some(&(i, node, ordinal)) = p.floats.get(p.float_index)
        && i < end
    {
        let position = indent
            .add(p.prefix[i - start], sat)
            .add(p.tracking[i - start], sat);
        let data = Arc::clone(&p.data);
        let windows = selected_hyphen
            .and_then(|h| hyphen::line(&data, start, h, cx, sat))
            .unwrap_or_else(|| super::windows::measure(&data, start, end, cx, sat));
        let delta = windows.iter().fold(LayoutUnit::ZERO, |sum, window| {
            sum.add(window.delta(i, sat), sat)
        });
        let ruby_delta =
            crate::ruby::measure::candidate(&data, start, i, atomics, cx, sat).adjustment;
        return Err((node, ordinal, position.add(delta, sat).add(ruby_delta, sat)));
    }
    let (hang_start, trailing) =
        super::whitespace::trailing(&p.data, start, end, &p.scan.widths, sat);
    // Release the mutable cache borrow before accessing the context shaper.
    let data = Arc::clone(&p.data);
    let mut result = Scan {
        ruby: None,
        ruby_caret_gaps: Vec::new(),
        prepared: false,
        overlays: Vec::new(),
        end,
        reason: if end == p.scan.end {
            p.scan.reason
        } else {
            super::soft_break_reason(&p.data, start, end)
        },
        widths: p.scan.widths[..end - start].to_vec(),
        leading: None,
        autospace_gaps: Vec::new(),
        content: p.prefix[end - start]
            .sub(trailing, sat)
            .add(p.tracking[hang_start - start], sat)
            .add(decoration::width(&p.data, end, false, sat), sat),
        hang_start,
        hanging_end: LayoutUnit::ZERO,
        punctuation_edges: Default::default(),
    };
    if let Some(end) = selected_hyphen
        && let Some(windows) = hyphen::line(&data, start, end, cx, sat)
    {
        super::reshape::apply_windows(&data, start, &mut result, windows, cx, sat);
        let visible = result
            .overlays
            .iter()
            .find_map(|w| w.hyphen.as_ref().map(|t| t.start));
        result.content = result
            .content
            .sub(
                super::spacing::width(&data, start, result.hang_start, None, sat),
                sat,
            )
            .add(
                super::spacing::width(&data, start, result.hang_start, visible, sat),
                sat,
            );
    }
    super::punctuation::prepare(
        &data,
        start,
        &mut result,
        token.flags,
        options,
        available,
        indent,
        sat,
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    fn plain_paragraph(text: &str) -> crate::Paragraph {
        plain_paragraph_with_style(text, &crate::style::ParagraphStyle::default())
    }

    fn plain_paragraph_with_style(
        text: &str,
        style: &crate::style::ParagraphStyle,
    ) -> crate::Paragraph {
        use crate::font::{FontCollection, FontOptions};
        use crate::limits::Limits;
        use crate::node::{NodeId, TextSource};
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register(include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec())
            .unwrap();
        let mut builder = crate::ParagraphBuilder::new(style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        builder
            .build(&mut crate::LayoutContext::new(), &fonts)
            .unwrap()
    }

    fn line_signature(line: &crate::Line) -> (String, Vec<crate::output::Glyph>) {
        let glyphs = line
            .fragments()
            .flat_map(|fragment| match fragment {
                crate::Fragment::GlyphRun(run) => run.glyphs().collect::<Vec<_>>(),
                _ => Vec::new(),
            })
            .collect();
        (
            format!(
                "{:?}/{:?}/{:?}/{:?}/{:?}",
                line.text_range(),
                line.break_reason(),
                (
                    line.block_offset(),
                    line.inline_size(),
                    line.block_size(),
                    line.baseline(crate::geometry::BaselineKind::Alphabetic)
                ),
                (line.hang_start(), line.hang_end()),
                line.fragments().collect::<Vec<_>>()
            ),
            glyphs,
        )
    }

    #[test]
    fn plain_height_rejection_retry_reuses_scan_without_a_float() {
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let text = "ab ".repeat(63) + "ab";
        let p = plain_paragraph(&text);
        let mut cx = LayoutContext::new();
        let mut constraint = LineConstraint::new(10000.);
        constraint.max_block_size = Some(0.);
        assert!(
            matches!(p.next_line(&mut cx, p.start_token(), &Default::default(),
            &constraint, &AtomicSizes::EMPTY), LineResult::BlockSizeExceeded { needed_block_size } if needed_block_size > 0.)
        );
        assert!(cx.cache_visits > 100);
        assert_eq!(
            cx.cache_prepare_visits, 0,
            "cold ordinary lines need no candidate preparation"
        );
        cx.cache_visits = 0;
        constraint.max_block_size = None;
        let LineResult::Line(actual) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("restored height")
        };
        assert_eq!(actual.text_range(), 0..text.len());
        let LineResult::Line(fresh) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("fresh")
        };
        assert_eq!(line_signature(&actual), line_signature(&fresh));
        assert!(
            !line_signature(&actual).1.is_empty(),
            "real fixture glyph output"
        );
        assert_eq!(
            cx.cache_prepare_visits, 0,
            "same-width retry needs no candidate preparation"
        );
        assert_eq!(
            cx.cache_visits, 0,
            "the first restored-height retry must not scan again"
        );
    }

    #[test]
    fn plain_justified_width_retries_reuse_scan_and_match_fresh_lines() {
        use crate::style::{LineOptions, TextAlign};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let p = plain_paragraph(&("alpha beta gamma delta ".repeat(15) + "alpha"));
        let options = LineOptions {
            text_align: TextAlign::JustifyAll,
            ..Default::default()
        };
        let mut cx = LayoutContext::new();
        let first = LineConstraint::new(10000.);
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &first,
                &AtomicSizes::EMPTY
            ),
            LineResult::Line(_)
        ));
        let mut ends = Vec::new();
        assert_eq!(cx.cache_prepare_visits, 0);
        let mut prepared = 0;
        for width in [512., 320., 192., 96., 48.] {
            cx.cache_visits = 0;
            let constraint = LineConstraint::new(width);
            let LineResult::Line(actual) = p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("retry width {width}")
            };
            let LineResult::Line(fresh) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("fresh width {width}")
            };
            assert_eq!(
                line_signature(&actual),
                line_signature(&fresh),
                "width {width}"
            );
            assert_eq!(
                cx.cache_visits, 0,
                "retry width {width} must reuse the original scan"
            );
            ends.push(actual.text_range().end);
            if prepared == 0 {
                prepared = cx.cache_prepare_visits;
                assert!(prepared > 0 && prepared <= p.data.units.len());
            } else {
                assert_eq!(
                    cx.cache_prepare_visits, prepared,
                    "prepare candidate searches once"
                );
            }
        }
        assert!(
            ends.windows(2).any(|pair| pair[0] != pair[1]),
            "actual different line cuts"
        );
    }

    #[test]
    fn plain_atomic_size_revisions_change_geometry_before_cache_reuse() {
        use crate::limits::Limits;
        use crate::node::{InlineEdges, NodeId};
        use crate::style::ParagraphStyle;
        use crate::{
            AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let style = ParagraphStyle::default();
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_atomic(NodeId(1), &style.root, InlineEdges::default());
        let p = builder
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        let mut small = AtomicSizes::new();
        small.insert(
            NodeId(1),
            AtomicSize {
                inline_size: 10.,
                ..Default::default()
            },
        );
        let mut large = AtomicSizes::new();
        large.insert(
            NodeId(1),
            AtomicSize {
                inline_size: 50.,
                ..Default::default()
            },
        );
        assert_eq!(small.generation(), large.generation());
        let mut cx = LayoutContext::new();
        let constraint = LineConstraint::new(1000.);
        for (sizes, want) in [(&small, 10.), (&large, 50.), (&small, 10.)] {
            for attempt in 0..2 {
                cx.cache_visits = 0;
                let crate::LineResult::Line(actual) = p.next_line(
                    &mut cx,
                    p.start_token(),
                    &Default::default(),
                    &constraint,
                    sizes,
                ) else {
                    panic!("atomic")
                };
                let LineResult::Line(fresh) = p.next_line(
                    &mut LayoutContext::new(),
                    p.start_token(),
                    &Default::default(),
                    &constraint,
                    sizes,
                ) else {
                    panic!("fresh atomic")
                };
                assert_eq!(actual.inline_size(), want);
                assert_eq!(line_signature(&actual), line_signature(&fresh));
                if attempt == 1 {
                    assert_eq!(cx.cache_visits, 0);
                }
            }
        }
    }

    #[test]
    fn plain_missing_atomic_warnings_are_repeated_on_each_call() {
        use crate::limits::{Limits, WarningKind};
        use crate::node::{InlineEdges, NodeId};
        use crate::style::ParagraphStyle;
        use crate::{AtomicSizes, LayoutContext, LineConstraint, ParagraphBuilder};
        let style = ParagraphStyle::default();
        let mut builder = ParagraphBuilder::new(&style, &Limits::default());
        builder.push_atomic(NodeId(1), &style.root, InlineEdges::default());
        let p = builder
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        let mut cx = LayoutContext::new();
        for _ in 0..3 {
            cx.cache_visits = 0;
            p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &LineConstraint::new(1000.),
                &AtomicSizes::EMPTY,
            );
            assert!(
                cx.take_warnings()
                    .iter()
                    .any(|warning| warning.kind == WarningKind::MissingAtomicSize)
            );
            assert!(
                cx.cache_visits > 0,
                "warning-producing scans must run again"
            );
        }
    }

    #[test]
    fn lazy_index_fallback_rescans_current_width_and_preserves_suppression() {
        use crate::limits::WarningKind;
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let p = plain_paragraph(&("alpha beta gamma delta ".repeat(15) + "alpha"));
        let mut cx = LayoutContext::new();
        p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(10000.),
            &AtomicSizes::EMPTY,
        );
        // A suppressed sink cannot prove speculative preparation warning-free.
        // Force that real sink state while retaining the clean, wider scan.
        cx.warnings.set_max(Some(0));
        cx.warnings
            .push(WarningKind::Unsupported, "existing diagnostic");
        assert!(cx.warnings.is_suppressed());
        cx.cache_visits = 0;
        let constraint = LineConstraint::new(96.);
        let LineResult::Line(actual) = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("fallback")
        };
        let LineResult::Line(fresh) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("fresh")
        };
        assert_eq!(line_signature(&actual), line_signature(&fresh));
        assert!(
            actual.text_range().end < p.text().len(),
            "must not return the retained wide line"
        );
        assert!(cx.cache_visits > 0);
        assert_eq!(
            cx.take_warnings()
                .iter()
                .map(|warning| warning.kind)
                .collect::<Vec<_>>(),
            vec![WarningKind::Suppressed]
        );
    }

    #[test]
    fn plain_tabs_remain_position_dependent_and_uncached() {
        use crate::style::{ParagraphStyle, WhiteSpaceCollapse};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let mut style = ParagraphStyle::default();
        style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let p = plain_paragraph_with_style("a\tb\tc", &style);
        let mut cx = LayoutContext::new();
        let mut signatures = Vec::new();
        for offset in [0., 12., 4., 0.] {
            cx.cache_visits = 0;
            let mut constraint = LineConstraint::new(1000.);
            constraint.inline_start_offset = offset;
            let LineResult::Line(actual) = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("tab")
            };
            let LineResult::Line(fresh) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("fresh tab")
            };
            assert_eq!(line_signature(&actual), line_signature(&fresh));
            assert!(cx.cache_visits > 0);
            signatures.push(line_signature(&actual));
        }
        assert_ne!(signatures[0], signatures[1]);
        assert_eq!(signatures[0], signatures[3]);
    }

    #[test]
    fn plain_raw_cache_releases_its_paragraph_on_shrink() {
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let p = plain_paragraph("alpha beta gamma");
        let weak = std::sync::Arc::downgrade(&p.data);
        let mut cx = LayoutContext::new();
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &LineConstraint::new(1000.),
                &AtomicSizes::EMPTY
            ),
            LineResult::Line(_)
        ));
        drop(p);
        assert!(weak.upgrade().is_some());
        cx.shrink_to(0);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn plain_first_line_ligature_retries_keep_alternate_glyphs_and_source_cuts() {
        use crate::style::ParagraphStyle;
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let mut style = ParagraphStyle::default();
        let mut first = style.root.clone();
        first.font_size = 32.;
        style.first_line = Some(first);
        let p = plain_paragraph_with_style("ffi office ffi office ffi office", &style);
        let mut cx = LayoutContext::new();
        let constraint = LineConstraint::new(1000.);
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY
            ),
            LineResult::Line(_)
        ));
        for width in [1000., 320., 160., 96., 80.] {
            cx.cache_visits = 0;
            let constraint = LineConstraint::new(width);
            let LineResult::Line(actual) = p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("alternate retry")
            };
            let LineResult::Line(fresh) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("fresh alternate")
            };
            assert_eq!(line_signature(&actual), line_signature(&fresh));
            assert_eq!(cx.cache_visits, 0);
            assert_eq!(actual.break_token(), fresh.break_token());
            let continuation = p.next_line(
                &mut LayoutContext::new(),
                actual.break_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            );
            let fresh_continuation = p.next_line(
                &mut LayoutContext::new(),
                fresh.break_token(),
                &Default::default(),
                &constraint,
                &AtomicSizes::EMPTY,
            );
            match (continuation, fresh_continuation) {
                (LineResult::Line(actual), LineResult::Line(fresh)) => {
                    assert_eq!(line_signature(&actual), line_signature(&fresh))
                }
                (LineResult::Done, LineResult::Done) => (),
                pair => panic!("unexpected continuation: {pair:?}"),
            }
        }
    }

    #[test]
    fn plain_discretionary_forced_and_bidi_retries_match_fresh_glyphs() {
        use crate::style::{ParagraphStyle, WhiteSpaceCollapse};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult};
        let mut style = ParagraphStyle::default();
        style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        for text in [
            "ab\u{ad}cdef\u{ad}ghijkl\u{ad}mn",
            "ab cd\n12 ef\ngh",
            "ab \u{202b}12 cd\u{202c} ef 34",
        ] {
            let p = plain_paragraph_with_style(text, &style);
            let mut saw_visible_hyphen = false;
            for first_width in [1000., 96.] {
                let mut cx = LayoutContext::new();
                p.next_line(
                    &mut cx,
                    p.start_token(),
                    &Default::default(),
                    &LineConstraint::new(first_width),
                    &AtomicSizes::EMPTY,
                );
                for width in [
                    first_width,
                    first_width * 0.75,
                    first_width * 0.5,
                    first_width * 0.5,
                ] {
                    cx.cache_visits = 0;
                    let constraint = LineConstraint::new(width);
                    let LineResult::Line(actual) = p.next_line(
                        &mut cx,
                        p.start_token(),
                        &Default::default(),
                        &constraint,
                        &AtomicSizes::EMPTY,
                    ) else {
                        panic!("retry")
                    };
                    let LineResult::Line(fresh) = p.next_line(
                        &mut LayoutContext::new(),
                        p.start_token(),
                        &Default::default(),
                        &constraint,
                        &AtomicSizes::EMPTY,
                    ) else {
                        panic!("fresh")
                    };
                    assert_eq!(
                        line_signature(&actual),
                        line_signature(&fresh),
                        "{text:?} at {width}"
                    );
                    assert_eq!(cx.cache_visits, 0, "eligible retry {text:?} at {width}");
                    saw_visible_hyphen |= actual.fragments().any(|fragment| match fragment {
                        crate::Fragment::GlyphRun(run) => run.clusters().any(|cluster| {
                            cluster.source_char == Some('\u{ad}') && cluster.advance > 0.
                        }),
                        _ => false,
                    });
                }
            }
            if text.contains('\u{ad}') {
                assert!(
                    saw_visible_hyphen,
                    "exercise a selected discretionary glyph"
                );
            }
        }
    }

    #[test]
    fn shrinking_frontier_discards_candidates_beyond_current_end() {
        use super::Candidates;
        use crate::geometry::LayoutUnit;

        let mut candidates = Candidates::default();
        for (end, cost) in [(1, 7), (2, 4), (3, 2)] {
            candidates.push(end, LayoutUnit::from_raw(cost));
        }
        // Later, cheaper candidates dominate earlier ones. Shrinking must
        // remove them even when none of the restored candidates fits.
        for (through, limit, expected) in [
            (3, 3, Some(3)),
            (2, 3, None),
            (2, 4, Some(2)),
            (1, 6, None),
            (1, 7, Some(1)),
            (0, 7, None),
        ] {
            assert_eq!(
                candidates.select(through, LayoutUnit::from_raw(limit)),
                expected,
                "through={through}, limit={limit}"
            );
        }
    }

    #[test]
    fn varying_hyphen_costs_choose_latest_fitting_break_with_linear_shrink_work() {
        use super::Candidates;
        use crate::geometry::LayoutUnit;
        for count in [128, 1024, 4096] {
            let mut hyphens = Candidates::default();
            for end in 1..=count {
                // A varying font/size can make a later hyphen cheaper.
                let cost = (end * 37 % 101 + end / 4) as i32;
                hyphens.push(end, LayoutUnit::from_raw(cost));
            }
            for through in (0..=count).rev() {
                let limit = LayoutUnit::from_raw((through / 3) as i32);
                let expected = hyphens
                    .candidates
                    .iter()
                    .filter(|(end, cost)| *end <= through && *cost <= limit)
                    .map(|(end, _)| *end)
                    .max();
                assert_eq!(hyphens.select(through, limit), expected);
            }
            assert!(
                hyphens.visits <= count * 6,
                "{} for {count}",
                hyphens.visits
            );
        }
    }

    #[test]
    fn cache_matches_cold_layout_across_widths_options_atomics_and_rewinds() {
        use crate::limits::Limits;
        use crate::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
        use crate::style::{LineOptions, ParagraphStyle, TextAlign};
        use crate::{
            AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let style = ParagraphStyle::default();
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "a b ")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_atomic(NodeId(3), &style.root, InlineEdges::default())
            .push_text(TextSource::Generated { node: NodeId(1) }, "c d")
            .push_out_of_flow(NodeId(4), OutOfFlowKind::Float);
        let p = b
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        let mut small = AtomicSizes::new();
        small.insert(
            NodeId(3),
            AtomicSize {
                inline_size: 10.0,
                ..AtomicSize::default()
            },
        );
        let mut large = AtomicSizes::new();
        large.insert(
            NodeId(3),
            AtomicSize {
                inline_size: 50.0,
                ..AtomicSize::default()
            },
        );
        assert_eq!(small.generation(), large.generation());
        let mut cx = LayoutContext::new();
        for sizes in [&small, &large, &small] {
            for align in [TextAlign::Start, TextAlign::Center] {
                let options = LineOptions {
                    text_align: align,
                    ..LineOptions::default()
                };
                let mut c = LineConstraint::new(500.0);
                for width in [500.0, 80.0, 40.0, 100.0, 30.0] {
                    c.available_inline_size = width;
                    let actual = p.next_line(&mut cx, p.start_token(), &options, &c, sizes);
                    let expected = p.next_line(
                        &mut LayoutContext::new(),
                        p.start_token(),
                        &options,
                        &c,
                        sizes,
                    );
                    fn dump(r: &LineResult) -> String {
                        match r {
                            LineResult::Line(l) => format!(
                                "{:?}/{:?}/{:?}",
                                l.text_range(),
                                l.break_reason(),
                                l.fragments().collect::<Vec<_>>()
                            ),
                            _ => format!("{r:?}"),
                        }
                    }
                    assert_eq!(dump(&actual), dump(&expected));
                    if let LineResult::FloatEncountered { float_cursor, .. } = actual {
                        c.floats_placed_through = Some(float_cursor);
                    }
                }
                c.floats_placed_through = None;
                assert!(matches!(
                    p.next_line(&mut cx, p.start_token(), &options, &c, sizes),
                    LineResult::Line(_) | LineResult::FloatEncountered { .. }
                ));
            }
        }
        let mut root = style.clone();
        root.root.white_space_collapse = crate::style::WhiteSpaceCollapse::Preserve;
        let mut b = ParagraphBuilder::new(&root, &Limits::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "\ta")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
        let other = b
            .build(
                &mut LayoutContext::new(),
                &crate::font::FontCollection::new(&Limits::default()),
            )
            .unwrap();
        other.next_line(
            &mut cx,
            other.start_token(),
            &LineOptions::default(),
            &LineConstraint::new(500.0),
            &AtomicSizes::EMPTY,
        );
        assert!(cx.partial.is_none());
    }

    #[test]
    fn retries_reuse_one_entry_and_shrink_releases_paragraph() {
        use crate::limits::Limits;
        use crate::node::{NodeId, OutOfFlowKind, TextSource};
        use crate::style::{LineOptions, ParagraphStyle};
        use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};
        for count in [32, 64] {
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
            for n in 0..count {
                b.push_text(TextSource::Generated { node: NodeId(1) }, "a")
                    .push_out_of_flow(NodeId(n + 2), OutOfFlowKind::Float);
            }
            let p = b
                .build(
                    &mut LayoutContext::new(),
                    &crate::font::FontCollection::new(&Limits::default()),
                )
                .unwrap();
            let weak = std::sync::Arc::downgrade(&p.data);
            let mut cx = LayoutContext::new();
            let mut c = LineConstraint::new(10000.0);
            for _ in 0..count {
                let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
                    &mut cx,
                    p.start_token(),
                    &LineOptions::default(),
                    &c,
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!()
                };
                c.floats_placed_through = Some(float_cursor);
            }
            assert!(cx.partial.is_some());
            assert!(cx.cache_visits <= count as usize * 2);
            drop(p);
            assert!(weak.upgrade().is_some());
            cx.shrink_to(0);
            assert!(weak.upgrade().is_none());
        }
    }

    #[test]
    fn punctuation_float_retries_keep_candidate_work_linear() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::node::{NodeId, OutOfFlowKind, TextSource};
        use crate::style::{
            FontFamily, HangingPunctuation, LineOptions, ParagraphStyle, TextSpacingTrim,
        };
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named("CJK".into())];
        style.root.font_size = 16.;
        style.root.lang = Some("ja".into());
        style.root.text_spacing_trim = TextSpacingTrim::TrimAll;
        let options = LineOptions {
            hanging_punctuation: HangingPunctuation {
                first: true,
                allow_end: true,
                ..Default::default()
            },
            ..Default::default()
        };
        for count in [32u64, 128, 512] {
            let mut builder = ParagraphBuilder::new(&style, &limits);
            for i in 0..count {
                builder
                    .push_text(
                        TextSource::Generated {
                            node: NodeId(i * 2 + 1),
                        },
                        "「日」、",
                    )
                    .push_out_of_flow(NodeId(i * 2 + 2), OutOfFlowKind::Float);
            }
            let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
            let mut cx = LayoutContext::new();
            let mut constraint = LineConstraint::new(count as f32 * 40.);
            let line = loop {
                match p.next_line(
                    &mut cx,
                    p.start_token(),
                    &options,
                    &constraint,
                    &AtomicSizes::EMPTY,
                ) {
                    LineResult::FloatEncountered { float_cursor, .. } => {
                        constraint.floats_placed_through = Some(float_cursor);
                        constraint.available_inline_size -= 4.;
                    }
                    LineResult::Line(line) => break line,
                    other => panic!("{other:?}"),
                }
            };
            assert!(cx.cache_visits <= p.data.units.len() * 2);
            let partial = cx.partial.as_ref().expect("retained float cache");
            let visits =
                partial.breaks.visits + partial.emergencies.visits + partial.hyphens.visits;
            assert!(visits <= p.data.units.len() * 8, "{count}: {visits}");
            let LineResult::Line(fresh) = p.next_line(
                &mut LayoutContext::new(),
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("fresh")
            };
            assert_eq!(line.text_range(), fresh.text_range());
            assert_eq!(
                (line.inline_size(), line.hang_start(), line.hang_end()),
                (fresh.inline_size(), fresh.hang_start(), fresh.hang_end())
            );
            let glyphs = |line: &crate::Line| {
                line.fragments()
                    .filter_map(|f| {
                        if let Fragment::GlyphRun(r) = f {
                            Some(r.glyphs())
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .map(|g| (g.id, g.inline_position))
                    .collect::<Vec<_>>()
            };
            assert_eq!(glyphs(&line), glyphs(&fresh));
        }
    }
}
