//! Quirks-mode strut contributions for range queries (`line::quirk`). Kept
//! out of `Summary` so the shared trees stay compact when the quirk is off.
//!
//! Every unit that credits a strut on a line carries that strut in its leaf:
//! direct text its owner's (or the root's), an Open/Close with an inline
//! edge its own box's. A collapsible space or tab that trims at the line end
//! also carries its own glyph profile here instead of in the shared tree.
//! The trailing run start, forced break credit and ruby root credit are
//! answered from arrays precomputed in `new`.
use super::super::metrics::RecordProfile;
use super::super::quirk::{ContentCredit, CreditBox, content_credit};
use super::scalar::{Bounds, union};
use crate::LayoutContext;
use crate::analysis::units::UnitKind;
use crate::geometry::WritingMode;
use crate::paragraph::ParagraphData;
use crate::style::TextOrientation;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Side {
    pub(super) normal: Option<Bounds>,
    pub(super) raw: Option<Bounds>,
    /// Content that keeps a forced break from crediting its parent's strut.
    pub(super) content: ContentCredit,
}

impl Side {
    pub(super) fn join(self, o: Self) -> Self {
        Self {
            normal: union(self.normal, o.normal),
            raw: union(self.raw, o.raw),
            content: self.content.join(o.content),
        }
    }

    /// Mirrors `Summary::profile`: only ungrouped profiles size the root.
    pub(super) fn from(p: RecordProfile, content: ContentCredit) -> Self {
        let bounds = Some(Bounds {
            top: p.top,
            bottom: p.bottom,
        });
        Self {
            normal: (p.group.is_none() && p.own_group.is_none())
                .then_some(bounds)
                .flatten(),
            raw: bounds,
            content,
        }
    }
}

/// `all`: as if no unit were trimmed; `bare`: `all` without the unit's own
/// glyph, read inside edge windows whose glyphs come from the overlay;
/// `kept`: excluding units that trim when they end a line. A range query
/// reads `all`/`bare` before the trailing start and `kept` from it on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Leaf {
    pub(super) all: Side,
    pub(super) bare: Side,
    pub(super) kept: Side,
}

impl Leaf {
    fn join(self, o: Self) -> Self {
        Self {
            all: self.all.join(o.all),
            bare: self.bare.join(o.bare),
            kept: self.kept.join(o.kept),
        }
    }
}

#[derive(Debug)]
pub(super) struct QuirkIndex {
    tree: Vec<Leaf>,
    /// `stop[blocked][e]`: where the backward scan of `trailing_start(_, 0, e)`
    /// stops when entered in state `blocked` at `e`.
    stop: [Vec<u32>; 2],
    /// `whitespace::obstructed(data, e)` for every `e`.
    obstructed: Vec<bool>,
    /// The last ForcedBreak at or before `i`: `Struts::line` credits the
    /// last one of a line, wherever it lies.
    forced: Vec<Option<u32>>,
    /// Open unit of each box.
    opens: Vec<u32>,
    /// Static ancestor barriers, used by forced-break credit queries.
    credit_boxes: Vec<CreditBox>,
    /// Prefix count of units covered by a ruby container.
    ruby: Vec<u32>,
    root: RecordProfile,
}

/// The root inline box's strut, as `select` and `metrics::measure` size it.
pub(super) fn root_strut(data: &ParagraphData) -> RecordProfile {
    let root = &data.styles[0];
    let metrics = data.style_metrics[0];
    let upright = matches!(
        data.style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) && root.text_orientation != TextOrientation::Sideways;
    let (above, below) =
        super::super::metrics::extents(root, metrics.metrics, metrics.vertical_metrics, upright);
    RecordProfile {
        top: -above,
        bottom: below,
        shift: 0.0,
        group: None,
        own_group: None,
    }
}

impl QuirkIndex {
    /// `boxes`: each box's strut profile; `trimmed`: the glyph profile of
    /// every `line::quirk::trims` unit, which the shared tree omits.
    pub(super) fn new(
        data: &ParagraphData,
        boxes: &[RecordProfile],
        trimmed: &[Option<RecordProfile>],
    ) -> Self {
        use super::super::quirk::{end_edge, start_edge, trims};
        use super::super::whitespace::{hangable, preserved, transparent};
        let n = data.units.len();
        let root = root_strut(data);
        let size = n.max(1).next_power_of_two();
        let mut tree = vec![Leaf::default(); size * 2];
        let mut opens = vec![0u32; data.boxes.len()];
        // Boxes are in preorder (build_units pushes parents before children).
        let mut credit_boxes: Vec<CreditBox> = Vec::with_capacity(data.boxes.len());
        for b in &data.boxes {
            let parent = b
                .parent
                .map_or(CreditBox::ROOT, |p| credit_boxes[p as usize]);
            credit_boxes.push(parent.child(data.styles[b.style as usize].vertical_align));
        }
        let mut forced = Vec::with_capacity(n);
        let mut covered = vec![0i32; n + 1];
        for ruby in &data.ruby.containers {
            covered[ruby.units.start] += 1;
            covered[ruby.units.end] -= 1;
        }
        let mut ruby = Vec::with_capacity(n + 1);
        ruby.push(0u32);
        let mut depth = 0;
        for (i, u) in data.units.iter().enumerate() {
            depth += covered[i];
            ruby.push(ruby[i] + u32::from(depth > 0));
            forced.push(match u.kind {
                UnitKind::ForcedBreak => Some(i as u32),
                _ => forced.last().copied().flatten(),
            });
            let owner = match u.kind {
                UnitKind::Open { box_index } | UnitKind::Close { box_index } => {
                    credit_boxes[box_index as usize]
                }
                _ => u
                    .parent_box
                    .map_or(CreditBox::ROOT, |b| credit_boxes[b as usize]),
            };
            let credit = content_credit(data, i, owner);
            let content = Side {
                content: credit,
                ..Default::default()
            };
            tree[size + i] = match u.kind {
                UnitKind::Cluster { .. } | UnitKind::Tab => {
                    let strut =
                        Side::from(u.parent_box.map_or(root, |b| boxes[b as usize]), credit);
                    let all = trimmed[i].map_or(strut, |p| strut.join(Side::from(p, credit)));
                    Leaf {
                        all,
                        bare: strut,
                        kept: if trims(data, i) {
                            Side::default()
                        } else {
                            strut
                        },
                    }
                }
                UnitKind::Atomic { .. } => {
                    let side = content;
                    Leaf {
                        all: side,
                        bare: side,
                        kept: side,
                    }
                }
                UnitKind::Open { box_index } => {
                    opens[box_index as usize] = i as u32;
                    if start_edge(data, box_index) {
                        let side = Side::from(boxes[box_index as usize], credit);
                        Leaf {
                            all: side,
                            bare: side,
                            kept: side,
                        }
                    } else {
                        // Empty pending children credit ancestors, but never
                        // contribute their own strut through this Open.
                        Leaf {
                            all: content,
                            bare: content,
                            kept: content,
                        }
                    }
                }
                UnitKind::Close { box_index } if end_edge(data, box_index) => {
                    let side = Side::from(boxes[box_index as usize], credit);
                    Leaf {
                        all: side,
                        bare: side,
                        kept: side,
                    }
                }
                _ => Leaf::default(),
            };
        }
        for i in (1..size).rev() {
            tree[i] = tree[i * 2].join(tree[i * 2 + 1]);
        }
        let end_blocks = |b: u32| {
            let e = data.boxes[b as usize].edges;
            e.padding.inline_end != 0.0 || e.border.inline_end != 0.0
        };
        // The backward scan of `whitespace::trailing_start`, for every end
        // and entry state.
        let mut stop = [vec![0u32; n + 1], vec![0u32; n + 1]];
        for e in 1..=n {
            let u = e - 1;
            for s in [false, true] {
                stop[usize::from(s)][e] = match data.units[u].kind {
                    UnitKind::Close { box_index } => {
                        stop[usize::from(s || end_blocks(box_index))][u]
                    }
                    _ if (hangable(data, u) && (!preserved(data, u) || !s))
                        || transparent(data, u) =>
                    {
                        stop[usize::from(s)][u]
                    }
                    _ => e as u32,
                };
            }
        }
        // `whitespace::obstructed`: a forward scan over end edges and
        // transparent units, then the cloned ancestors of the boundary box.
        let mut chained: Vec<Option<bool>> = vec![None; data.boxes.len()];
        let mut chain = |b: u32| {
            let mut path = Vec::new();
            let mut cursor = Some(b);
            let mut value = false;
            while let Some(b) = cursor {
                if let Some(v) = chained[b as usize] {
                    value = v;
                    break;
                }
                path.push(b);
                cursor = data.boxes[b as usize].parent;
            }
            for b in path.into_iter().rev() {
                value |= super::super::decoration::cloned(data, b) && end_blocks(b);
                chained[b as usize] = Some(value);
            }
            value
        };
        let mut obstructed = vec![false; n + 1];
        let mut scan = false;
        for e in (0..n).rev() {
            scan = match data.units[e].kind {
                UnitKind::Close { box_index } => end_blocks(box_index) || scan,
                UnitKind::BidiControl | UnitKind::Float { .. } | UnitKind::Absolute { .. } => scan,
                UnitKind::Cluster { .. } if transparent(data, e) => scan,
                _ => false,
            };
            let boundary = match data.units[e].kind {
                UnitKind::Close { box_index } => Some(box_index),
                _ => data.units[e].parent_box,
            };
            obstructed[e] = scan || boundary.is_some_and(&mut chain);
        }
        Self {
            tree,
            stop,
            obstructed,
            forced,
            opens,
            credit_boxes,
            ruby,
            root,
        }
    }

    /// `whitespace::trailing_start(data, start, end)` in constant time.
    pub(super) fn trailing_start(&self, start: usize, end: usize) -> usize {
        start.max(self.stop[usize::from(self.obstructed[end])][end] as usize)
    }

    pub(super) fn leaf(&self, i: usize) -> &Leaf {
        &self.tree[self.tree.len() / 2 + i]
    }

    pub(super) fn root(&self) -> RecordProfile {
        self.root
    }

    /// Whether a ruby container covers a unit of `range`.
    pub(super) fn ruby(&self, range: &Range<usize>) -> bool {
        self.ruby[range.end] > self.ruby[range.start]
    }

    pub(super) fn credited(&self, parent: Option<u32>, content: ContentCredit) -> bool {
        parent
            .map_or(CreditBox::ROOT, |b| self.credit_boxes[b as usize])
            .credited(content)
    }

    /// The last forced break of `range` and the first unit whose content
    /// keeps it from crediting its parent's strut.
    pub(super) fn forced(
        &self,
        data: &ParagraphData,
        range: &Range<usize>,
    ) -> Option<(usize, usize)> {
        let k = self.forced[range.end - 1]? as usize;
        if k < range.start {
            return None;
        }
        if data.items[data.units[k].item as usize].own_break_style
            && data.units[k].parent_box.is_some()
        {
            return None;
        }
        let lo = data.units[k].parent_box.map_or(range.start, |b| {
            range.start.max(self.opens[b as usize] as usize + 1)
        });
        Some((k, lo))
    }

    /// Strut contributions of `range` on a line whose trailing run starts at
    /// `t`; units inside `removed` edge windows omit their own glyph.
    pub(super) fn side(
        &self,
        range: &Range<usize>,
        t: usize,
        removed: &[Range<usize>],
        cx: &mut LayoutContext,
    ) -> Side {
        let t = t.clamp(range.start, range.end);
        self.lead(range.start..t, removed, cx)
            .join(self.query(&(t..range.end), |l| l.kept, cx))
    }

    /// `all` outside the removed windows and `bare` inside them.
    fn lead(&self, range: Range<usize>, removed: &[Range<usize>], cx: &mut LayoutContext) -> Side {
        if range.is_empty() {
            return Side::default();
        }
        let Some(r) = removed
            .iter()
            .filter(|r| r.start < range.end && range.start < r.end)
            .min_by_key(|r| r.start)
        else {
            return self.query(&range, |l| l.all, cx);
        };
        let inside = range.start.max(r.start)..range.end.min(r.end);
        self.query(&(range.start..inside.start), |l| l.all, cx)
            .join(self.query(&inside, |l| l.bare, cx))
            .join(self.lead(inside.end..range.end, removed, cx))
    }

    pub(super) fn query(
        &self,
        range: &Range<usize>,
        pick: fn(&Leaf) -> Side,
        cx: &mut LayoutContext,
    ) -> Side {
        if range.is_empty() {
            return Side::default();
        }
        self.query_node(1, 0..self.tree.len() / 2, range, pick, cx)
    }

    fn query_node(
        &self,
        node: usize,
        source: Range<usize>,
        range: &Range<usize>,
        pick: fn(&Leaf) -> Side,
        _cx: &mut LayoutContext,
    ) -> Side {
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if source.end <= range.start || range.end <= source.start {
            return Side::default();
        }
        if range.start <= source.start && source.end <= range.end {
            return pick(&self.tree[node]);
        }
        let mid = (source.start + source.end) / 2;
        self.query_node(node * 2, source.start..mid, range, pick, _cx)
            .join(self.query_node(node * 2 + 1, mid..source.end, range, pick, _cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParagraphBuilder;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{BoxDecorationBreak, InlineStyle, ParagraphStyle, WhiteSpaceCollapse};

    #[test]
    fn precomputed_trailing_start_matches_the_scan() {
        let pre = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        };
        let cloned = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            box_decoration_break: BoxDecorationBreak::Clone,
            ..Default::default()
        };
        let padded = InlineEdges {
            padding: Sides {
                inline_end: 1.0,
                ..Sides::default()
            },
            ..InlineEdges::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                line_height_quirk: true,
                ..Default::default()
            },
            &Limits::default(),
        );
        let text = |b: &mut ParagraphBuilder, s: &str| {
            b.push_text(TextSource::Generated { node: NodeId(1) }, s);
        };
        text(&mut b, "a b ");
        b.open_inline(NodeId(2), &pre, padded);
        text(&mut b, "  ");
        b.close_inline();
        text(&mut b, "\u{200e} c ");
        b.push_forced_break(NodeId(3));
        text(&mut b, "d ");
        b.open_inline(NodeId(4), &cloned, padded);
        text(&mut b, "e  ");
        b.open_inline(NodeId(5), &pre, InlineEdges::default());
        text(&mut b, " f \u{200e} ");
        b.close_inline();
        text(&mut b, "  ");
        b.close_inline();
        text(&mut b, " g");
        let p = b
            .build(
                &mut LayoutContext::new(),
                &FontCollection::with_options(
                    &Limits::default(),
                    FontOptions {
                        system_fonts: false,
                        ..Default::default()
                    },
                ),
            )
            .unwrap();
        let data = &p.data;
        let n = data.units.len();
        let profile = super::root_strut(data);
        let index = QuirkIndex::new(data, &vec![profile; data.boxes.len()], &vec![None; n]);
        let mut obstructed = 0;
        for e in 0..=n {
            let expected = crate::line::whitespace::obstructed(data, e);
            obstructed += usize::from(expected);
            assert_eq!(index.obstructed[e], expected, "obstructed {e}");
            for s in 0..=e {
                assert_eq!(
                    index.trailing_start(s, e),
                    crate::line::whitespace::trailing_start(data, s, e),
                    "{s}..{e}"
                );
            }
        }
        // Both the forward scan and the cloned-ancestor chain obstruct.
        assert!(obstructed > 2);
    }
}
