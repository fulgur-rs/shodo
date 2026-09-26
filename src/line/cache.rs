use super::{Scan, decoration, scan::scan};
use crate::analysis::units::{BreakClass, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::node::NodeId;
use crate::output::BreakReason;
use crate::paragraph::{
    AtomicSizes, BreakToken, FloatCursor, LineConstraint, Paragraph, ParagraphData,
};
use crate::style::LineOptions;
use std::sync::Arc;

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
    breaks: Vec<usize>,
    floats: Vec<(usize, NodeId, u32)>,
    threshold_cursor: usize,
    break_cursor: usize,
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
            + self.scan.widths.capacity() * 4
            + self.prefix.capacity() * 4
            + self.thresholds.capacity() * std::mem::size_of::<(usize, LayoutUnit)>()
            + self.breaks.capacity() * std::mem::size_of::<usize>()
            + self.floats.capacity() * std::mem::size_of::<(usize, NodeId, u32)>()
    }
    fn end(&mut self, available: LayoutUnit, indent: LayoutUnit, sat: &mut Saturation) -> usize {
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
            return self.scan.end;
        };
        while self.break_cursor > 0 && self.breaks[self.break_cursor - 1] > i {
            self.break_cursor -= 1;
        }
        while self.break_cursor < self.breaks.len() && self.breaks[self.break_cursor] <= i {
            self.break_cursor += 1;
        }
        let before = self.break_cursor;
        let mut end = if before > 0 {
            self.breaks[before - 1]
        } else {
            self.breaks.first().copied().unwrap_or(self.scan.end)
        };
        while end < self.scan.end
            && matches!(
                self.data.units[end].kind,
                UnitKind::Close { .. } | UnitKind::BidiControl
            )
        {
            end += 1;
        }
        end
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
        let scanned = scan(
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
        let mut prefix = vec![decoration::width(data, start, true, sat)];
        let mut thresholds = Vec::new();
        let mut breaks = Vec::new();
        let mut floats = Vec::new();
        let mut max = LayoutUnit::ZERO;
        for (k, (u, w)) in units.iter().zip(&scanned.widths).enumerate() {
            if let UnitKind::Float { node, ordinal } = u.kind {
                floats.push((start + k, node, ordinal));
            }
            let next = prefix[k].add(*w, sat);
            prefix.push(next);
            if k > 0
                && !matches!(
                    u.kind,
                    UnitKind::Cluster { space: true, .. } | UnitKind::ForcedBreak
                )
            {
                max = max.max(next.add(decoration::width(data, start + k + 1, false, sat), sat));
                thresholds.push((start + k, max));
            }
            if u.break_after == BreakClass::Allowed {
                breaks.push(start + k + 1);
            }
        }
        if !sat.is_clean()
            || prefix.last().unwrap().raw() as i64 + indent.raw() as i64 > i32::MAX as i64
        {
            return Ok(scanned);
        }
        cx.partial = Some(PartialLine {
            threshold_cursor: thresholds.len(),
            break_cursor: breaks.len(),
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
            floats,
        });
    }
    let p = cx.partial.as_mut().unwrap();
    p.cursor = constraint.floats_placed_through;
    let end = p.end(available, indent, sat);
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
        return Err((node, ordinal, indent.add(p.prefix[i - start], sat)));
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
    Ok(Scan {
        end,
        reason: if end == p.scan.end {
            p.scan.reason
        } else {
            BreakReason::Regular
        },
        widths: p.scan.widths[..end - start].to_vec(),
        content: p.prefix[end - start]
            .sub(trailing, sat)
            .add(decoration::width(&p.data, end, false, sat), sat),
        hang_start,
    })
}

#[cfg(test)]
mod tests {
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
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        for n in 0..32 {
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
        for _ in 0..32 {
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
        assert!(cx.cache_visits <= 64);
        drop(p);
        assert!(weak.upgrade().is_some());
        cx.shrink_to(0);
        assert!(weak.upgrade().is_none());
    }
}
