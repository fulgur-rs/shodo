//! Quirks-mode line height calculation (Quirks Mode Standard §3.3-3.4,
//! CSS Inline 3 §5.3), decided per line from units so that retained line
//! metrics and the ruby metric index share one definition.
use crate::analysis::units::UnitKind;
use crate::paragraph::ParagraphData;
use crate::style::WhiteSpaceCollapse;
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
pub(crate) fn text(data: &ParagraphData, i: usize, t: usize) -> bool {
    matches!(data.units[i].kind, UnitKind::Cluster { .. } | UnitKind::Tab)
        && !(i >= t && trims(data, i))
}

/// Content that keeps a forced break from contributing its parent's strut.
pub(crate) fn content(data: &ParagraphData, i: usize, t: usize) -> bool {
    match data.units[i].kind {
        UnitKind::Atomic { .. } => true,
        UnitKind::Open { box_index } => start_edge(data, box_index),
        UnitKind::Close { box_index } => end_edge(data, box_index),
        _ => text(data, i, t),
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
        let covered = data.units[first.min(data.units.len())..]
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
        for i in units.clone() {
            let u = &data.units[i];
            match u.kind {
                UnitKind::Open { box_index } if start_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                UnitKind::Close { box_index } if end_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                UnitKind::ForcedBreak => forced = Some(i),
                _ if text(data, i, t) => s.mark(u.parent_box),
                _ => {}
            }
        }
        if let Some(k) = forced {
            let p = data.units[k].parent_box;
            let lo = p.map_or(units.start, |b| {
                (units.start..k)
                    .rev()
                    .find(|i| matches!(data.units[*i].kind, UnitKind::Open { box_index } if box_index == b))
                    .map_or(units.start, |open| open + 1)
            });
            if !(lo..k).any(|i| content(data, i, t)) {
                s.mark(p);
            }
        }
        if !s.root && ruby_on_line(data, &units) {
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
