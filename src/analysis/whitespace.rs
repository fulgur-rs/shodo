//! White-space processing (CSS Text 3 §4.1.1 phase I) and bidi control
//! insertion (CSS Writing Modes 4 §2.4), producing the processed text,
//! the item list and the offset mapping.
//!
//! Segment breaks use whole-IFC neighbor context. The input is DOM text:
//! LF is a segment break and CR is a CSS space, not an HTML source newline.

use super::whitespace_context::{REMOVE, ignorable, whitespace_flags};
use super::{Item, ItemKind};
use crate::builder::RawItem;
use crate::geometry::Direction;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::mapping::{MappingKind, MappingUnit, OffsetMapping};
use crate::node::{NodeId, TextSource};
use crate::style::{InlineStyle, UnicodeBidi, WhiteSpaceCollapse};

const OBJECT_REPLACEMENT: char = '\u{FFFC}';
const PARAGRAPH_SEPARATOR: char = '\u{2029}';

pub(crate) struct Processed {
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) mapping: Option<OffsetMapping>,
    pub(crate) indivisible: Vec<std::ops::Range<u32>>,
    /// Correspondence with the common pre-transform text, independent of DOM mapping.
    pub(crate) source_spans: Vec<crate::mapping::TransformSpan>,
    pub(crate) width_origins: Vec<super::transform::WidthOrigin>,
}

pub(crate) fn process(
    raw_text: &str,
    raw: &[RawItem],
    styles: &[InlineStyle],
    with_mapping: bool,
    limits: &Limits,
) -> Result<Processed, LimitExceeded> {
    let flags = whitespace_flags(raw_text, raw, styles);
    let mut p = Processor {
        out: String::with_capacity(
            raw_text.len().min(
                limits
                    .max_text_bytes
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
            ),
        ),
        items: Vec::with_capacity(
            raw.len().min(
                limits
                    .max_items
                    .and_then(|n| usize::try_from(n).ok())
                    .unwrap_or(usize::MAX),
            ),
        ),
        mapping: with_mapping.then(OffsetMapping::default),
        // Collapsible spaces at the start of the paragraph are removed.
        after_space: true,
        open: Vec::new(),
        limits,
        styles,
    };
    for item in raw {
        match item {
            RawItem::RubyBoundary {
                ruby,
                boundary,
                node,
                style,
            } => {
                p.check_item()?;
                let at = p.pos();
                p.items.push(Item {
                    kind: ItemKind::RubyBoundary {
                        ruby: *ruby,
                        boundary: *boundary,
                    },
                    text: at..at,
                    style: *style,
                    node: *node,
                });
            }
            RawItem::Text {
                source,
                range,
                style,
            } => {
                let text = &raw_text[range.start as usize..range.end as usize];
                p.text(
                    text,
                    *source,
                    *style,
                    &flags[range.start as usize..range.end as usize],
                )?;
            }
            RawItem::Open { node, style, edges } => {
                p.marker(ItemKind::OpenInline { edges: *edges }, *style, *node)?;
                for &c in bidi_open(&styles[*style as usize]) {
                    p.generated(ItemKind::BidiControl, c, *style, *node)?;
                }
                p.open.push((*node, *style));
            }
            RawItem::Close => {
                if let Some((node, style)) = p.open.pop() {
                    for &c in bidi_close(&styles[style as usize]) {
                        p.generated(ItemKind::BidiControl, c, style, node)?;
                    }
                    p.marker(ItemKind::CloseInline, style, node)?;
                }
            }
            RawItem::Atomic {
                node,
                style,
                parent_style,
                edges,
            } => {
                let kind = ItemKind::Atomic {
                    edges: *edges,
                    parent_style: *parent_style,
                };
                p.generated(kind, OBJECT_REPLACEMENT, *style, *node)?;
                p.after_space = false;
            }
            RawItem::OutOfFlow { node, kind, style } => {
                // Out-of-flow boxes are transparent to white-space collapsing.
                p.generated(
                    ItemKind::OutOfFlow { kind: *kind },
                    OBJECT_REPLACEMENT,
                    *style,
                    *node,
                )?;
            }
            RawItem::BlockInInline { node, style } => {
                p.close_bidi_scopes()?;
                p.generated(ItemKind::BlockInInline, PARAGRAPH_SEPARATOR, *style, *node)?;
                p.open_bidi_scopes()?;
                p.after_space = true;
            }
            RawItem::ForcedBreak { node, style } => {
                p.close_bidi_scopes()?;
                p.generated(ItemKind::ForcedBreak, '\n', *style, *node)?;
                p.open_bidi_scopes()?;
                p.after_space = true;
            }
        }
    }
    Ok(Processed {
        width_origins: Vec::new(),
        source_spans: vec![crate::mapping::TransformSpan {
            old: 0..p.out.len() as u32,
            new: 0..p.out.len() as u32,
            kind: MappingKind::Identity,
        }],
        text: p.out,
        items: p.items,
        mapping: p.mapping,
        indivisible: Vec::new(),
    })
}

struct Processor<'a> {
    out: String,
    items: Vec<Item>,
    mapping: Option<OffsetMapping>,
    after_space: bool,
    open: Vec<(NodeId, u32)>,
    limits: &'a Limits,
    styles: &'a [InlineStyle],
}

impl Processor<'_> {
    fn close_bidi_scopes(&mut self) -> Result<(), LimitExceeded> {
        for i in (0..self.open.len()).rev() {
            let (node, style) = self.open[i];
            for &c in bidi_close(&self.styles[style as usize]) {
                self.generated(ItemKind::BidiControl, c, style, node)?;
            }
        }
        Ok(())
    }

    fn open_bidi_scopes(&mut self) -> Result<(), LimitExceeded> {
        for i in 0..self.open.len() {
            let (node, style) = self.open[i];
            for &c in bidi_open(&self.styles[style as usize]) {
                self.generated(ItemKind::BidiControl, c, style, node)?;
            }
        }
        Ok(())
    }

    fn check_item(&self) -> Result<(), LimitExceeded> {
        Limits::check(
            Some(u64::from(u32::MAX)),
            LimitKind::Items,
            self.items.len() as u64 + 1,
        )?;
        Limits::check(
            self.limits.max_items,
            LimitKind::Items,
            self.items.len() as u64 + 1,
        )
    }
    fn append(&mut self, c: char) -> Result<(), LimitExceeded> {
        let len = (self.out.len() as u64).saturating_add(c.len_utf8() as u64);
        Limits::check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, len)?;
        Limits::check(self.limits.max_text_bytes, LimitKind::TextBytes, len)?;
        self.out.push(c);
        Ok(())
    }
    fn pos(&self) -> u32 {
        self.out.len() as u32
    }

    fn marker(&mut self, kind: ItemKind, style: u32, node: NodeId) -> Result<(), LimitExceeded> {
        self.check_item()?;
        let at = self.pos();
        self.items.push(Item {
            kind,
            text: at..at,
            style,
            node: Some(node),
        });
        Ok(())
    }

    fn generated(
        &mut self,
        kind: ItemKind,
        c: char,
        style: u32,
        node: NodeId,
    ) -> Result<(), LimitExceeded> {
        self.check_item()?;
        let start = self.pos();
        self.append(c)?;
        let text = start..self.pos();
        if let Some(m) = &mut self.mapping {
            m.push_generated(text.clone(), node);
        }
        self.items.push(Item {
            kind,
            text,
            style,
            node: Some(node),
        });
        Ok(())
    }

    fn map(
        &mut self,
        kind: MappingKind,
        node: NodeId,
        dom: Option<u32>,
        len: u32,
        text: std::ops::Range<u32>,
    ) {
        let Some(m) = &mut self.mapping else { return };
        match dom {
            // `dom` is a caller-supplied offset, not covered by any
            // resource limit; saturate instead of overflowing so no input
            // can panic.
            Some(dom) => m.push_unit(MappingUnit {
                kind,
                node,
                dom: dom..dom.saturating_add(len),
                text,
            }),
            None if !text.is_empty() => m.push_generated(text, node),
            None => {}
        }
    }

    fn text(
        &mut self,
        s: &str,
        source: TextSource,
        style_index: u32,
        flags: &[u8],
    ) -> Result<(), LimitExceeded> {
        use WhiteSpaceCollapse::*;
        let style = &self.styles[style_index as usize];
        let collapse_spaces = matches!(style.white_space_collapse, Collapse | PreserveBreaks);
        let preserve_breaks = !matches!(style.white_space_collapse, Collapse);
        // `preserve-spaces` keeps every space uncollapsed but, unlike
        // `preserve`, tabs and segment breaks lose their special meaning and
        // become an ordinary space character (CSS Text 4, `white-space-collapse`).
        let convert_to_space = matches!(style.white_space_collapse, PreserveSpaces);
        let node = source.node();
        let dom_base = match source {
            TextSource::Dom { offset, .. } => Some(offset),
            TextSource::Generated { .. } => None,
        };
        let mut segment: Option<u32> = None;
        for (i, raw_c) in s.char_indices() {
            let dom = dom_base.map(|b| b.saturating_add(i as u32));
            let len = raw_c.len_utf8() as u32;
            if flags[i] & REMOVE != 0 {
                let at = self.pos();
                self.map(MappingKind::Collapsed, node, dom, len, at..at);
                continue;
            }
            let c = if raw_c == '\r' || convert_to_space && matches!(raw_c, '\t' | '\n') {
                ' '
            } else {
                raw_c
            };
            let control = match c {
                '\u{2028}' | '\u{2029}' | '\u{0085}' => Some(ItemKind::ForcedBreak),
                '\n' if preserve_breaks => Some(ItemKind::ForcedBreak),
                '\t' if !collapse_spaces => Some(ItemKind::Tab),
                _ => None,
            };
            if let Some(kind) = control {
                self.flush(&mut segment, style_index, node)?;
                if matches!(kind, ItemKind::ForcedBreak) {
                    self.close_bidi_scopes()?;
                }
                self.check_item()?;
                let start = self.pos();
                self.append(c)?;
                self.map(MappingKind::Identity, node, dom, len, start..self.pos());
                self.items.push(Item {
                    kind: kind.clone(),
                    text: start..self.pos(),
                    style: style_index,
                    node: Some(node),
                });
                self.after_space = matches!(kind, ItemKind::ForcedBreak);
                if matches!(kind, ItemKind::ForcedBreak) {
                    self.open_bidi_scopes()?;
                }
                continue;
            }
            let collapsible = collapse_spaces && matches!(c, ' ' | '\t' | '\n');
            if collapsible && self.after_space {
                let at = self.pos();
                self.map(MappingKind::Collapsed, node, dom, len, at..at);
                continue;
            }
            if segment.is_none() {
                self.check_item()?;
            }
            segment.get_or_insert(self.pos());
            let start = self.pos();
            // A collapsible tab or segment break is kept as one space (same length).
            self.append(if collapsible { ' ' } else { c })?;
            self.map(MappingKind::Identity, node, dom, len, start..self.pos());
            if !ignorable(c) {
                self.after_space = collapsible;
            }
        }
        self.flush(&mut segment, style_index, node)
    }

    fn flush(
        &mut self,
        segment: &mut Option<u32>,
        style: u32,
        node: NodeId,
    ) -> Result<(), LimitExceeded> {
        if let Some(start) = segment.take() {
            let end = self.pos();
            if end > start {
                self.check_item()?;
                self.items.push(Item {
                    kind: ItemKind::Text,
                    text: start..end,
                    style,
                    node: Some(node),
                });
            }
        }
        Ok(())
    }
}

fn bidi_open(style: &InlineStyle) -> &'static [char] {
    let rtl = style.direction == Direction::Rtl;
    match (style.unicode_bidi, rtl) {
        (UnicodeBidi::Normal, _) => &[],
        (UnicodeBidi::Embed, false) => &['\u{202A}'],
        (UnicodeBidi::Embed, true) => &['\u{202B}'],
        (UnicodeBidi::Isolate, false) => &['\u{2066}'],
        (UnicodeBidi::Isolate, true) => &['\u{2067}'],
        (UnicodeBidi::BidiOverride, false) => &['\u{202D}'],
        (UnicodeBidi::BidiOverride, true) => &['\u{202E}'],
        (UnicodeBidi::IsolateOverride, false) => &['\u{2066}', '\u{202D}'],
        (UnicodeBidi::IsolateOverride, true) => &['\u{2067}', '\u{202E}'],
        (UnicodeBidi::Plaintext, _) => &['\u{2068}'],
    }
}

fn bidi_close(style: &InlineStyle) -> &'static [char] {
    match style.unicode_bidi {
        UnicodeBidi::Normal => &[],
        UnicodeBidi::Embed | UnicodeBidi::BidiOverride => &['\u{202C}'],
        UnicodeBidi::Isolate | UnicodeBidi::Plaintext => &['\u{2069}'],
        UnicodeBidi::IsolateOverride => &['\u{202C}', '\u{2069}'],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ParagraphBuilder;
    use crate::limits::Limits;
    use crate::mapping::{Affinity, TextOrigin};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};

    fn run(b: &ParagraphBuilder) -> Processed {
        process(&b.text, &b.items, &b.styles, true, &b.limits).unwrap()
    }

    fn dom(node: u64) -> TextSource {
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        }
    }

    fn builder() -> ParagraphBuilder {
        ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
    }

    #[test]
    fn collapses_across_element_boundaries_and_trims_the_start() {
        let mut b = builder();
        b.push_text(dom(1), "  a  ")
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a b");
        let m = p.mapping.unwrap();
        // The space in node 3 was collapsed into the one kept in node 1.
        assert_eq!(m.dom_to_text(NodeId(3), 0), Some((2, Affinity::Downstream)));
        assert_eq!(m.dom_to_text(NodeId(3), 1), Some((2, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(3),
                offset: 1
            })
        );
        // Round trip for every kept DOM offset.
        for (node, offset) in [(1, 2), (1, 3), (3, 1)] {
            let (t, a) = m.dom_to_text(NodeId(node), offset).unwrap();
            assert_eq!(
                m.text_to_dom(t, a),
                Some(TextOrigin::Dom {
                    node: NodeId(node),
                    offset
                })
            );
        }
    }

    #[test]
    fn preserved_tabs_and_newlines_become_control_items() {
        let pre = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &pre, InlineEdges::default())
            .push_text(dom(2), "a\tb\nc")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a\tb\nc");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            Tab,
            Text,
            ForcedBreak,
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn preserve_spaces_converts_tabs_and_breaks_to_space_without_collapsing() {
        let style = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::PreserveSpaces,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &style, InlineEdges::default())
            .push_text(dom(2), "a \t\nb")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a   b");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(
            kinds, expected,
            "no Tab or ForcedBreak items under preserve-spaces"
        );
    }

    #[test]
    fn break_spaces_still_produces_tab_and_forced_break_items() {
        let style = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::BreakSpaces,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &style, InlineEdges::default())
            .push_text(dom(2), "a\tb\nc")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a\tb\nc");
        let kinds: Vec<_> = p
            .items
            .iter()
            .map(|i| std::mem::discriminant(&i.kind))
            .collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline {
                edges: InlineEdges::default(),
            },
            Text,
            Tab,
            Text,
            ForcedBreak,
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn dom_offsets_near_u32_max_do_not_panic() {
        let mut b = builder();
        b.push_text(
            TextSource::Dom {
                node: NodeId(1),
                offset: u32::MAX - 1,
            },
            "abc",
        );
        let p = process(&b.text, &b.items, &b.styles, true, &b.limits).unwrap();
        assert_eq!(p.text, "abc");
        // The mapping itself must not panic either, in both directions.
        let m = p.mapping.unwrap();
        assert!(m.text_to_dom(2, Affinity::Downstream).is_some());
        assert!(m.dom_to_text(NodeId(1), u32::MAX - 1).is_some());
    }

    #[test]
    fn atomics_stop_collapsing_and_are_object_replacement_characters() {
        let mut b = builder();
        b.push_text(dom(1), "a ")
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b");
        let p = run(&b);
        assert_eq!(p.text, "a \u{FFFC} b");
        let m = p.mapping.unwrap();
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(2) })
        );
    }

    #[test]
    fn isolates_insert_bidi_controls() {
        let iso = InlineStyle {
            unicode_bidi: UnicodeBidi::Isolate,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &iso, InlineEdges::default())
            .push_text(dom(2), "x")
            .close_inline();
        assert_eq!(run(&b).text, "\u{2066}x\u{2069}");
    }

    #[test]
    fn mapping_can_be_disabled() {
        let mut b = builder();
        b.push_text(dom(1), "a");
        assert!(
            process(&b.text, &b.items, &b.styles, false, &b.limits)
                .unwrap()
                .mapping
                .is_none()
        );
    }
    #[test]
    fn segment_break_neighbors_cross_nodes() {
        let mut b = builder();
        b.push_text(dom(1), "日 \t\n")
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_out_of_flow(NodeId(9), crate::node::OutOfFlowKind::Absolute)
            .push_text(dom(3), " \t本")
            .close_inline();
        assert_eq!(run(&b).text, "日\u{FFFC}本");
        // An actual inline box is a context barrier, unlike an OOF marker.
        let mut b = builder();
        b.push_text(dom(1), "日\n")
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), "本");
        assert_eq!(run(&b).text, "日 \u{FFFC}本");
    }

    #[test]
    fn hangul_keeps_segment_space() {
        let mut b = builder();
        b.push_text(dom(1), "한\n글 日\n本 a\n\nb");
        assert_eq!(run(&b).text, "한 글 日本 a b");
    }

    #[test]
    fn zero_width_space_removes_adjacent_break() {
        for s in ["a\u{200B}\nb", "a\n\u{200B}b", "日\u{FE0F} \n \u{200E}本"] {
            let mut b = builder();
            b.push_text(dom(1), s);
            let expected = s.replace(['\n', ' '], "");
            assert_eq!(run(&b).text, expected);
        }
    }

    #[test]
    fn preserve_breaks_removes_surrounding_collapsible_spaces() {
        let pre_line = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::PreserveBreaks,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &pre_line, InlineEdges::default())
            .push_text(dom(2), "a \t\n \tb")
            .close_inline();
        assert_eq!(run(&b).text, "a\nb");
    }

    #[test]
    fn dom_carriage_return_is_space() {
        for (mode, expected) in [
            (WhiteSpaceCollapse::Preserve, "a \nb"),
            (WhiteSpaceCollapse::Collapse, "a b"),
            (WhiteSpaceCollapse::PreserveSpaces, "a  b"),
        ] {
            let style = ParagraphStyle {
                root: InlineStyle {
                    white_space_collapse: mode,
                    ..InlineStyle::default()
                },
                ..ParagraphStyle::default()
            };
            let mut b = ParagraphBuilder::new(&style, &Limits::default());
            b.push_text(dom(1), "a\r\nb");
            assert_eq!(run(&b).text, expected);
        }
    }

    #[test]
    fn bidi_scopes_restart_at_paragraph_boundaries() {
        let outer = InlineStyle {
            unicode_bidi: UnicodeBidi::Isolate,
            ..InlineStyle::default()
        };
        let inner = InlineStyle {
            unicode_bidi: UnicodeBidi::Embed,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..InlineStyle::default()
        };
        let mut b = builder();
        b.open_inline(NodeId(1), &outer, InlineEdges::default())
            .open_inline(NodeId(2), &inner, InlineEdges::default())
            .push_text(dom(3), "a\nb")
            .push_block_in_inline(NodeId(4))
            .push_text(dom(5), "c")
            .push_forced_break(NodeId(6))
            .push_text(dom(7), "d")
            .close_inline()
            .close_inline();
        let p = run(&b);
        let start = "\u{2066}\u{202A}";
        let end = "\u{202C}\u{2069}";
        assert_eq!(
            p.text,
            format!("{start}a{end}\n{start}b{end}\u{2029}{start}c{end}\n{start}d{end}")
        );
        let m = p.mapping.unwrap();
        let at = p.text.find('\n').unwrap() as u32;
        assert_eq!(
            m.text_to_dom(at, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(3),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(at + 1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(1) })
        );
    }

    #[test]
    fn segment_mapping_and_limits() {
        let mut b = builder();
        b.push_text(dom(1), "日 \n 本");
        let p = run(&b);
        assert_eq!(p.text, "日本");
        let m = p.mapping.unwrap();
        assert_eq!(m.dom_to_text(NodeId(1), 4), Some((3, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 6
            })
        );
        let style = ParagraphStyle {
            root: InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..InlineStyle::default()
            },
            ..ParagraphStyle::default()
        };
        for limits in [
            Limits {
                max_text_bytes: Some(10),
                ..Limits::default()
            },
            Limits {
                max_items: Some(7),
                ..Limits::default()
            },
        ] {
            let iso = InlineStyle {
                unicode_bidi: UnicodeBidi::Isolate,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..InlineStyle::default()
            };
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.open_inline(NodeId(1), &iso, InlineEdges::default())
                .push_text(dom(2), "a\nb")
                .close_inline();
            assert!(process(&b.text, &b.items, &b.styles, true, &limits).is_err());
        }
    }

    #[test]
    fn transparent_boundaries_are_linear() {
        let mut b = builder();
        b.push_text(dom(0), "日");
        for i in 1..=4096 {
            b.open_inline(NodeId(i), &InlineStyle::default(), InlineEdges::default())
                .push_text(dom(10000 + i), " \n ")
                .push_out_of_flow(NodeId(20000 + i), crate::node::OutOfFlowKind::Absolute)
                .close_inline();
        }
        b.push_text(dom(50000), "本");
        let p = run(&b);
        assert_eq!(p.text, format!("日{}本", "\u{FFFC}".repeat(4096)));
        assert!(p.items.len() <= 3 * 4096 + 2);
    }
}
