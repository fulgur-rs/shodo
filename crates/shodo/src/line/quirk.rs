//! Quirks-mode line height calculation (Quirks Mode Standard §3.3-3.4,
//! CSS Inline 3 §5.3), decided per line from units so that retained line
//! metrics and the ruby metric index share one definition.
use crate::analysis::units::UnitKind;
use crate::paragraph::ParagraphData;
use crate::style::{VerticalAlign, WhiteSpaceCollapse};
use std::ops::Range;

/// A collapsible space or tab that is removed when it ends a line. Whether
/// it actually ends the line is decided by `whitespace::trailing_start`.
pub(crate) fn trims(data: &ParagraphData, i: usize) -> bool {
    let unit = &data.units[i];
    unit.combine.is_none()
        && matches!(
            unit.kind,
            UnitKind::Cluster { space: true, .. } | UnitKind::Tab
        )
        && matches!(
            data.styles[data.items[unit.item as usize].style as usize].white_space_collapse,
            WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
        )
}

/// Inline-start border or padding; margins never keep the strut.
pub(crate) fn start_edge(data: &ParagraphData, b: u32) -> bool {
    let e = data.boxes[b as usize].edges;
    e.border.inline_start + e.padding.inline_start != 0.0
}

pub(crate) fn end_edge(data: &ParagraphData, b: u32) -> bool {
    let e = data.boxes[b as usize].edges;
    e.border.inline_end + e.padding.inline_end != 0.0
}

/// Text the unit's `parent_box` directly contains on a line whose trailing
/// run starts at `t`.
fn text(data: &ParagraphData, i: usize, t: usize) -> bool {
    matches!(data.units[i].kind, UnitKind::Cluster { .. } | UnitKind::Tab)
        && !(i >= t && trims(data, i))
}

/// Inclusive box depth and the depth of its nearest top/bottom ancestor.
/// The root is depth zero and always receives pending top/bottom children.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CreditBox {
    depth: u32,
    top_bottom: u32,
    boundary: bool,
}

impl CreditBox {
    pub(crate) const ROOT: Self = Self {
        depth: 0,
        top_bottom: 0,
        boundary: true,
    };

    pub(crate) fn child(self, align: VerticalAlign) -> Self {
        let depth = self.depth + 1;
        let boundary = matches!(align, VerticalAlign::Top | VerticalAlign::Bottom);
        Self {
            depth,
            top_bottom: if boundary { depth } else { self.top_bottom },
            boundary,
        }
    }

    /// Baseline ancestors cannot receive metrics across a top/bottom box.
    /// Root and top/bottom boxes also receive pending descendants, even empty
    /// ones. This mirrors Blink's HasMetrics / ApplyBaselineShift.
    pub(crate) fn credited(self, content: ContentCredit) -> bool {
        content.0 < if self.boundary { u32::MAX } else { self.depth }
    }
}

/// Minimum top/bottom depth of content units; MAX denotes no content.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentCredit(u32);

impl Default for ContentCredit {
    fn default() -> Self {
        Self(u32::MAX)
    }
}

impl ContentCredit {
    pub(crate) fn join(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }
}

/// Content credit only: a pending Open does not add the child's own strut.
/// `owner` includes the box itself for Open/Close, and the parent otherwise.
/// Trimming is decided by the caller, independently of this static credit.
pub(crate) fn content_credit(data: &ParagraphData, i: usize, owner: CreditBox) -> ContentCredit {
    let pending = |align| {
        matches!(
            align,
            VerticalAlign::Top
                | VerticalAlign::Bottom
                | VerticalAlign::TextTop
                | VerticalAlign::TextBottom
        )
    };
    match data.units[i].kind {
        UnitKind::Cluster { .. } | UnitKind::Tab => ContentCredit(owner.top_bottom),
        UnitKind::Atomic { .. } => {
            let align =
                data.styles[data.items[data.units[i].item as usize].style as usize].vertical_align;
            ContentCredit(owner.child(align).top_bottom)
        }
        UnitKind::Open { box_index }
            if start_edge(data, box_index)
                || pending(
                    data.styles[data.boxes[box_index as usize].style as usize].vertical_align,
                ) =>
        {
            ContentCredit(owner.top_bottom)
        }
        UnitKind::Close { box_index } if end_edge(data, box_index) => {
            ContentCredit(owner.top_bottom)
        }
        _ => ContentCredit::default(),
    }
}

/// Inline boxes whose strut contributes to one line.
#[derive(Debug, Default)]
pub(crate) struct Struts {
    pub(crate) root: bool,
    pub(crate) boxes: crate::hashing::FastSet<u32>,
    /// First unit of the line's trailing run (`whitespace::trailing_start`).
    pub(crate) trailing: usize,
}

impl Struts {
    pub(crate) fn contributes(&self, b: Option<u32>) -> bool {
        b.map_or(self.root, |b| self.boxes.contains(&b))
    }

    /// Whether every unit of a glyph record's text is trimmed at the line end.
    pub(crate) fn trimmed(&self, data: &ParagraphData, text: &Range<u32>) -> bool {
        let first = data.units.partition_point(|u| u.text.end <= text.start);
        let covered = data.units[first..]
            .iter()
            .take_while(|u| u.text.start < text.end)
            .count();
        covered > 0 && first >= self.trailing && (first..first + covered).all(|i| trims(data, i))
    }

    fn mark(&mut self, b: Option<u32>) {
        match b {
            Some(b) => {
                self.boxes.insert(b);
            }
            None => self.root = true,
        }
    }

    pub(crate) fn line(data: &ParagraphData, units: Range<usize>) -> Self {
        let mut s = Self::default();
        let t = super::whitespace::trailing_start(data, units.start, units.end);
        s.trailing = t;
        let mut forced = None;
        // Seed continuation ancestors once. Each Open/Close then updates the
        // stack in O(1), so credit calculation is linear in the line and its
        // boundary ancestry, without rescanning descendant subtrees.
        let mut ancestors = Vec::new();
        let mut cursor = units.clone().next().and_then(|i| match data.units[i].kind {
            UnitKind::Close { box_index } => Some(box_index),
            _ => data.units[i].parent_box,
        });
        while let Some(b) = cursor {
            ancestors.push(b);
            cursor = data.boxes[b as usize].parent;
        }
        let mut stack = vec![(CreditBox::ROOT, ContentCredit::default())];
        for b in ancestors.into_iter().rev() {
            let align = data.styles[data.boxes[b as usize].style as usize].vertical_align;
            let owner = stack.last().expect("root credit frame").0.child(align);
            stack.push((owner, ContentCredit::default()));
        }
        for i in units.clone() {
            let u = &data.units[i];
            match u.kind {
                UnitKind::Open { box_index } => {
                    let parent = stack.last_mut().expect("root credit frame");
                    let owner = parent.0.child(
                        data.styles[data.boxes[box_index as usize].style as usize].vertical_align,
                    );
                    parent.1 = parent.1.join(content_credit(data, i, owner));
                    stack.push((owner, ContentCredit::default()));
                }
                UnitKind::Close { .. } => {
                    let (owner, content) = stack.pop().expect("closing credit frame");
                    let parent = stack.last_mut().expect("root credit frame");
                    parent.1 = parent.1.join(content).join(content_credit(data, i, owner));
                }
                UnitKind::ForcedBreak => {
                    let (owner, content) = *stack.last().expect("root credit frame");
                    // Explicit break profiles size themselves, not empty
                    // ancestor boxes. Direct root breaks keep its usual rule.
                    let inherited = !data.items[u.item as usize].own_break_style;
                    forced = Some((
                        u.parent_box,
                        (inherited || u.parent_box.is_none()) && !owner.credited(content),
                    ));
                }
                _ if !(i >= t && trims(data, i)) => {
                    let parent = stack.last_mut().expect("root credit frame");
                    parent.1 = parent.1.join(content_credit(data, i, parent.0));
                }
                _ => {}
            }
            match u.kind {
                UnitKind::Open { box_index } if start_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                UnitKind::Close { box_index } if end_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                _ if text(data, i, t) => s.mark(u.parent_box),
                _ => {}
            }
        }
        if let Some((p, true)) = forced {
            s.mark(p);
        }
        if data.style.force_root_strut || (!s.root && ruby_on_line(data, &units)) {
            s.root = true;
        }
        s
    }
}

/// Chromium forces the root strut on a line holding a ruby column.
fn ruby_on_line(data: &ParagraphData, units: &Range<usize>) -> bool {
    let containers = &data.ruby.containers;
    if containers.is_empty() {
        return false;
    }
    let mut found = false;
    let mut through = units.end;
    data.ruby
        .intervals
        .intersecting(containers, units.start, &mut through, |_, _| found = true);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle, WhiteSpaceCollapse};
    use crate::{LayoutContext, Paragraph, ParagraphBuilder};

    fn build(input: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                line_height_quirk: true,
                ..Default::default()
            },
            &Limits::default(),
        );
        input(&mut b);
        b.build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap()
    }

    fn text(b: &mut ParagraphBuilder, s: &str) {
        b.push_text(TextSource::Generated { node: NodeId(1) }, s);
    }

    /// `trailing()` before `finalize`, recomputed with real zero widths.
    fn reference(data: &crate::paragraph::ParagraphData, start: usize, end: usize) -> usize {
        let widths = vec![crate::geometry::LayoutUnit::ZERO; end - start];
        let mut sat = Default::default();
        crate::line::whitespace::trailing(data, start, end, &widths, &mut sat).0
    }

    #[test]
    fn trailing_start_matches_trailing_on_every_range() {
        let pre = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        };
        let padded = InlineEdges {
            padding: Sides {
                inline_end: 1.0,
                ..Sides::default()
            },
            ..InlineEdges::default()
        };
        let p = build(|b| {
            text(b, "a b ");
            b.open_inline(NodeId(2), &pre, padded);
            text(b, "  ");
            b.close_inline();
            text(b, "\u{200e} c ");
            b.push_forced_break(NodeId(3));
            text(b, "d");
        });
        let n = p.data.units.len();
        for start in 0..n {
            for end in start + 1..=n {
                let begin = reference(&p.data, start, end);
                let t = crate::line::whitespace::trailing_start(&p.data, start, end);
                // Every trimmed unit at or after `begin` is at or after `t`
                // and vice versa: the trimmed sets coincide.
                for i in start..end {
                    assert_eq!(
                        i >= begin && trims(&p.data, i),
                        i >= t && trims(&p.data, i),
                        "{start}..{end} unit {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn trailing_start_ignores_zero_width_finalize_reset() {
        let zero = InlineStyle {
            font_size: 0.0,
            ..Default::default()
        };
        let p = build(|b| {
            text(b, "x");
            b.open_inline(NodeId(2), &zero, InlineEdges::default());
            text(b, " ");
            b.close_inline();
        });
        let n = p.data.units.len();
        let space = (0..n)
            .find(|i| {
                matches!(
                    p.data.units[*i].kind,
                    crate::analysis::units::UnitKind::Cluster { space: true, .. }
                )
            })
            .unwrap();
        assert!(crate::line::whitespace::trailing_start(&p.data, 0, n) <= space);
        let struts = Struts::line(&p.data, 0..n);
        // The zero-size span's only text is a trailing collapsible space.
        assert!(!struts.contributes(Some(0)));
        assert!(struts.contributes(None));
    }

    #[test]
    fn br_credits_its_parent_only_without_other_parent_content() {
        // <span><img><br></span>: the atomic is content of the span.
        let p = build(|b| {
            b.open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
            b.push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default());
            b.push_forced_break(NodeId(4));
            b.close_inline();
        });
        let n = p.data.units.len();
        let s = Struts::line(&p.data, 0..n);
        assert!(!s.contributes(Some(0)) && !s.contributes(None));
        // x<span><br></span>: root text does not stop the span's br.
        let p = build(|b| {
            text(b, "x");
            b.open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
            b.push_forced_break(NodeId(4));
            b.close_inline();
        });
        let n = p.data.units.len();
        let s = Struts::line(&p.data, 0..n);
        assert!(s.contributes(Some(0)) && s.contributes(None));
    }

    #[test]
    fn trailing_space_record_is_trimmed() {
        let space = |p: &Paragraph| {
            let n = p.data.units.len();
            let s = Struts::line(&p.data, 0..n);
            let i = n - 1;
            s.trimmed(&p.data, &p.data.units[i].text)
        };
        assert!(space(&build(|b| text(b, "x "))));
        assert!(!space(&build(|b| text(b, "x"))));
    }

    #[test]
    fn margins_are_not_quirk_edges() {
        let p = build(|b| {
            b.open_inline(
                NodeId(2),
                &InlineStyle::default(),
                InlineEdges {
                    margin: Sides {
                        inline_start: 1.0,
                        inline_end: 1.0,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            );
            b.push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default());
            b.close_inline();
        });
        let n = p.data.units.len();
        assert!(!Struts::line(&p.data, 0..n).contributes(Some(0)));
    }

    #[test]
    fn ruby_column_on_the_line_credits_the_root() {
        use crate::{
            Ruby, RubyAlign, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubySpan, RubyStyle,
            RubyVisibility,
        };
        let limits = Limits::default();
        let style = InlineStyle::default();
        let p = build(|b| {
            let ruby = Ruby::new(
                vec![RubyBase {
                    node: NodeId(11),
                    content: RubyContent::text(
                        TextSource::Generated { node: NodeId(12) },
                        "b",
                        &style,
                        &limits,
                    ),
                    align: RubyAlign::default(),
                }],
                vec![RubyLevel {
                    annotations: vec![RubyAnnotation {
                        node: NodeId(13),
                        content: RubyContent::text(
                            TextSource::Generated { node: NodeId(14) },
                            "a",
                            &style,
                            &limits,
                        ),
                        span: RubySpan::Auto,
                        visibility: RubyVisibility::Visible,
                    }],
                    style: RubyStyle::default(),
                }],
            )
            .unwrap();
            b.push_ruby(NodeId(10), &style, ruby);
        });
        let n = p.data.units.len();
        assert!(ruby_on_line(&p.data, &(0..n)));
        assert!(Struts::line(&p.data, 0..n).contributes(None));
    }
}
