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
            debug_assert_eq!(self.frontier.pop(), Some(self.active));
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
    scan: Scan,
    prefix: Vec<LayoutUnit>,
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
        std::mem::size_of::<Self>()
            + self.scan.widths.capacity() * std::mem::size_of::<LayoutUnit>()
            + self.scan.overlays.capacity() * std::mem::size_of::<super::reshape::EdgeOverlay>()
            + self.prefix.capacity() * 4
            + self.thresholds.capacity() * std::mem::size_of::<(usize, LayoutUnit)>()
            + self.breaks.bytes()
            + self.emergencies.bytes()
            + self.hyphens.bytes()
            + self.floats.capacity() * std::mem::size_of::<(usize, NodeId, u32)>()
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
        while end < self.scan.end
            && matches!(
                self.data.units[end].kind,
                UnitKind::Close { .. } | UnitKind::BidiControl
            )
        {
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
    let valid = cx.partial.as_ref().is_some_and(|p| {
        p.data.id == para.id()
            && p.token == token
            && p.revision == atomics.revision
            && p.options == *options
            && p.width >= available
            && constraint.floats_placed_through >= p.cursor
    });
    if !valid {
        cx.partial = None;
        let mut scanned = scan(
            &para.data, start, available, offset, indent, atomics, cx, sat,
        );
        #[cfg(test)]
        {
            cx.cache_visits += scanned.end - start;
        }
        let data = &para.data;
        let units = &data.units[start..scanned.end];
        let safe = indent >= LayoutUnit::ZERO
            && scanned.widths.iter().all(|w| *w >= LayoutUnit::ZERO)
            && !units.iter().any(|u| matches!(u.kind, UnitKind::Tab))
            && units
                .iter()
                .any(|u| matches!(u.kind, UnitKind::Float { .. }));
        if !safe {
            return Ok(scanned);
        }
        // A cached prefix always describes the unbroken text. The selected
        // discretionary glyph is materialized afresh after choosing an end.
        let natural_widths: Vec<_> = units
            .iter()
            .map(|u| {
                super::scan::unit_width_from(
                    data,
                    u,
                    data.units[start].text.start,
                    LayoutUnit::ZERO,
                    atomics,
                    cx,
                    sat,
                )
            })
            .collect();
        if natural_widths.iter().any(|w| *w < LayoutUnit::ZERO) {
            return Ok(scanned);
        }
        let mut hyphens = Candidates::default();
        let mut prefix = vec![decoration::width(data, start, true, sat)];
        let mut thresholds = Vec::new();
        let mut breaks = Candidates::default();
        let mut emergencies = Candidates::default();
        let mut floats = Vec::new();
        let mut max = LayoutUnit::ZERO;
        let mut hanging = LayoutUnit::ZERO;
        for (k, (u, w)) in units.iter().zip(&natural_widths).enumerate() {
            if let UnitKind::Float { node, ordinal } = u.kind {
                floats.push((start + k, node, ordinal));
            }
            let next = prefix[k].add(*w, sat);
            prefix.push(next);
            let (delta, viable) = super::windows::candidate(data, start, start + k + 1, cx, sat);
            let suffix = decoration::width(data, start + k + 1, false, sat);
            let transparent = matches!(
                u.kind,
                UnitKind::Close { .. }
                    | UnitKind::BidiControl
                    | UnitKind::Float { .. }
                    | UnitKind::Absolute { .. }
            );
            if k > 0
                && !matches!(
                    u.kind,
                    UnitKind::Cluster { space: true, .. } | UnitKind::ForcedBreak
                )
            {
                let extent = next.add(delta, sat).add(suffix, sat).sub(
                    if transparent {
                        hanging
                    } else {
                        LayoutUnit::ZERO
                    },
                    sat,
                );
                max = max.max(extent);
                thresholds.push((start + k, max));
            }
            match u.kind {
                UnitKind::Cluster { space: true, .. } => hanging = hanging.add(*w, sat),
                _ if transparent => {}
                _ => hanging = LayoutUnit::ZERO,
            }
            let required = next.add(delta, sat).sub(hanging, sat).add(suffix, sat);
            if u.break_after == BreakClass::Hyphen {
                if let Some(windows) = hyphen::line(data, start, start + k + 1, cx, sat) {
                    let required = next
                        .sub(hanging, sat)
                        .add(suffix, sat)
                        .add(super::windows::cost(&windows, start + k + 1, sat), sat);
                    hyphens.push(start + k + 1, required);
                }
            } else if viable && u.break_after == BreakClass::Allowed {
                breaks.push(start + k + 1, required);
            } else if viable && u.break_after == BreakClass::Emergency {
                emergencies.push(start + k + 1, required);
            }
        }
        if !sat.is_clean()
            || prefix.last().unwrap().raw() as i64 + indent.raw() as i64 > i32::MAX as i64
        {
            return Ok(scanned);
        }
        // Keep the cold result intact until all cache safety checks pass.
        scanned.widths = natural_widths;
        scanned.overlays.clear();
        scanned.prepared = false;
        cx.partial = Some(PartialLine {
            threshold_cursor: thresholds.len(),
            float_index: 0,
            data: Arc::clone(data),
            token,
            revision: atomics.revision,
            options: *options,
            cursor: constraint.floats_placed_through,
            width: available,
            scan: scanned,
            prefix,
            thresholds,
            breaks,
            emergencies,
            hyphens,
            floats,
        });
    }
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
        let position = indent.add(p.prefix[i - start], sat);
        let data = Arc::clone(&p.data);
        let windows = selected_hyphen
            .and_then(|h| hyphen::line(&data, start, h, cx, sat))
            .unwrap_or_else(|| super::windows::measure(&data, start, end, cx, sat));
        let delta = windows.iter().fold(LayoutUnit::ZERO, |sum, window| {
            sum.add(window.delta(i, sat), sat)
        });
        return Err((node, ordinal, position.add(delta, sat)));
    }
    let mut hang_start = end;
    let mut trailing = LayoutUnit::ZERO;
    for i in (start..end).rev() {
        match p.data.units[i].kind {
            UnitKind::Cluster { space: true, .. } => {
                trailing = trailing.add(p.scan.widths[i - start], sat);
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
    // Release the mutable cache borrow before accessing the context shaper.
    let data = Arc::clone(&p.data);
    let mut result = Scan {
        prepared: false,
        overlays: Vec::new(),
        end,
        reason: if end == p.scan.end {
            p.scan.reason
        } else {
            super::soft_break_reason(&p.data, start, end)
        },
        widths: p.scan.widths[..end - start].to_vec(),
        content: p.prefix[end - start]
            .sub(trailing, sat)
            .add(decoration::width(&p.data, end, false, sat), sat),
        hang_start,
    };
    if let Some(end) = selected_hyphen
        && let Some(windows) = hyphen::line(&data, start, end, cx, sat)
    {
        super::reshape::apply_windows(&data, start, &mut result, windows, cx, sat);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
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
}
