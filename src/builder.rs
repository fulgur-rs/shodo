//! Paragraph builders: record the inline content of one block container.

use std::collections::HashMap;
use std::ops::Range;

use crate::limits::{LimitExceeded, LimitKind, Limits, WarningKind, WarningSink};
use crate::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use crate::style::{InlineStyle, ParagraphStyle};

/// Input as recorded, before white-space processing.
#[derive(Clone, Debug)]
pub(crate) enum RawItem {
    Text {
        source: TextSource,
        range: Range<u32>,
        style: u32,
    },
    Open {
        node: NodeId,
        style: u32,
        edges: InlineEdges,
    },
    Close,
    Atomic {
        node: NodeId,
        style: u32,
        parent_style: u32,
        edges: InlineEdges,
    },
    OutOfFlow {
        node: NodeId,
        kind: OutOfFlowKind,
        style: u32,
    },
    BlockInInline {
        node: NodeId,
        style: u32,
    },
    ForcedBreak {
        node: NodeId,
        style: u32,
    },
}

/// Builds a [`crate::Paragraph`] from the inline content of a block
/// container.
///
/// Limits are checked on every call. After the first violation the builder
/// ignores further input without allocating, and `build` returns the error.
pub struct ParagraphBuilder {
    pub(crate) style: ParagraphStyle,
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<RawItem>,
    /// Interned styles; index 0 is the paragraph's root style.
    pub(crate) styles: Vec<InlineStyle>,
    style_index: HashMap<String, u32>,
    /// Style indices of the currently open inline boxes.
    pub(crate) stack: Vec<u32>,
    pub(crate) error: Option<LimitExceeded>,
    pub(crate) warnings: WarningSink,
    pub(crate) offset_mapping: bool,
}

impl ParagraphBuilder {
    pub fn new(style: &ParagraphStyle, limits: &Limits) -> Self {
        let mut builder = Self {
            style: style.clone(),
            limits: limits.clone(),
            text: String::new(),
            items: Vec::new(),
            styles: Vec::new(),
            style_index: HashMap::new(),
            stack: Vec::new(),
            error: None,
            warnings: WarningSink::new(limits.max_warnings),
            offset_mapping: true,
        };
        builder.styles.push(style.root.clone());
        builder.style_index.insert(format!("{:?}", style.root), 0);
        builder
    }

    /// The first limit violation, if any.
    pub fn error(&self) -> Option<LimitExceeded> {
        self.error
    }

    /// Whether to build an [`crate::mapping::OffsetMapping`] (default true).
    /// Disable it when offsets are never mapped back, for example for PDF.
    pub fn with_offset_mapping(&mut self, enabled: bool) -> &mut Self {
        self.offset_mapping = enabled;
        self
    }

    pub fn open_inline(
        &mut self,
        node: NodeId,
        style: &InlineStyle,
        edges: InlineEdges,
    ) -> &mut Self {
        let depth = self.stack.len() as u64 + 1;
        if self.check(
            self.limits.max_nesting_depth,
            LimitKind::NestingDepth,
            depth,
        ) && self.reserve_item()
            && let Some(style) = self.intern(style)
        {
            self.items.push(RawItem::Open { node, style, edges });
            self.stack.push(style);
        }
        self
    }

    pub fn close_inline(&mut self) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        if self.stack.is_empty() {
            self.warnings.push(
                WarningKind::UnbalancedInline,
                "close_inline without open_inline ignored",
            );
            return self;
        }
        if self.reserve_item() {
            self.items.push(RawItem::Close);
            self.stack.pop();
        }
        self
    }

    pub fn push_text(&mut self, source: TextSource, text: &str) -> &mut Self {
        if self.error.is_some() || text.is_empty() {
            return self;
        }
        let total = self.text.len() as u64 + text.len() as u64;
        // Offsets are stored as u32, so that is a hard ceiling as well.
        let limit = self
            .limits
            .max_text_bytes
            .map_or(u64::from(u32::MAX), |l| l.min(u64::from(u32::MAX)));
        if self.check(Some(limit), LimitKind::TextBytes, total) && self.reserve_item() {
            let start = self.text.len() as u32;
            self.text.push_str(text);
            let style = self.current_style();
            self.items.push(RawItem::Text {
                source,
                range: start..self.text.len() as u32,
                style,
            });
        }
        self
    }

    pub fn push_atomic(
        &mut self,
        node: NodeId,
        style: &InlineStyle,
        edges: InlineEdges,
    ) -> &mut Self {
        if self.reserve_item()
            && let Some(style) = self.intern(style)
        {
            let parent_style = self.current_style();
            self.items.push(RawItem::Atomic {
                node,
                style,
                parent_style,
                edges,
            });
        }
        self
    }

    pub fn push_out_of_flow(&mut self, node: NodeId, kind: OutOfFlowKind) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::OutOfFlow { node, kind, style });
        }
        self
    }

    /// A block-level box inside inline content (CSS 2.1 §9.2.1.1).
    pub fn push_block_in_inline(&mut self, node: NodeId) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::BlockInInline { node, style });
        }
        self
    }

    /// A forced line break such as `<br>`.
    pub fn push_forced_break(&mut self, node: NodeId) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::ForcedBreak { node, style });
        }
        self
    }

    pub(crate) fn current_style(&self) -> u32 {
        self.stack.last().copied().unwrap_or(0)
    }

    fn check(&mut self, limit: Option<u64>, kind: LimitKind, actual: u64) -> bool {
        if self.error.is_some() {
            return false;
        }
        match Limits::check(limit, kind, actual) {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn reserve_item(&mut self) -> bool {
        let count = self.items.len() as u64 + 1;
        self.check(self.limits.max_items, LimitKind::Items, count)
    }

    fn intern(&mut self, style: &InlineStyle) -> Option<u32> {
        let key = format!("{style:?}");
        if let Some(&index) = self.style_index.get(&key) {
            return Some(index);
        }
        let count = self.styles.len() as u64 + 1;
        if !self.check(self.limits.max_styles, LimitKind::Styles, count) {
            return None;
        }
        let index = self.styles.len() as u32;
        self.styles.push(style.clone());
        self.style_index.insert(key, index);
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{LimitKind, Limits, WarningKind};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle};

    fn dom(node: u64) -> TextSource {
        TextSource::Dom {
            node: NodeId(node),
            offset: 0,
        }
    }

    fn bold() -> InlineStyle {
        InlineStyle {
            font_weight: 700.0,
            ..InlineStyle::default()
        }
    }

    #[test]
    fn records_items_and_shares_equal_styles() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), "ab")
            .open_inline(NodeId(2), &bold(), InlineEdges::default())
            .push_text(dom(3), "cd")
            .close_inline()
            .open_inline(NodeId(4), &bold(), InlineEdges::default())
            .close_inline();
        assert_eq!(b.text, "abcd");
        assert_eq!(b.items.len(), 6);
        assert_eq!(b.styles.len(), 2, "root + one shared bold style");
        assert!(b.stack.is_empty());
        assert_eq!(b.error(), None);
    }

    #[test]
    fn empty_text_is_not_recorded() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), "");
        assert!(b.items.is_empty());
    }

    #[test]
    fn text_limit_stops_further_allocation() {
        let limits = Limits {
            max_text_bytes: Some(5),
            ..Limits::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_text(dom(1), "abc")
            .push_text(dom(1), "def")
            .push_text(dom(1), "x");
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::TextBytes));
        assert_eq!(b.text, "abc");
        assert_eq!(b.items.len(), 1);
    }

    #[test]
    fn nesting_items_and_style_limits() {
        let limits = Limits {
            max_nesting_depth: Some(1),
            ..Limits::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.open_inline(NodeId(1), &InlineStyle::default(), InlineEdges::default())
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::NestingDepth));
        assert_eq!(b.stack.len(), 1);

        let limits = Limits {
            max_items: Some(2),
            ..Limits::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_text(dom(1), "a")
            .push_text(dom(1), "b")
            .push_text(dom(1), "c");
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::Items));
        assert_eq!(b.items.len(), 2);

        let limits = Limits {
            max_styles: Some(1),
            ..Limits::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.open_inline(NodeId(1), &bold(), InlineEdges::default());
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::Styles));
        assert_eq!(b.styles.len(), 1);
    }

    #[test]
    fn unbalanced_close_is_ignored_with_a_warning() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.close_inline();
        assert!(b.items.is_empty());
        assert_eq!(b.warnings.as_slice()[0].kind, WarningKind::UnbalancedInline);
    }

    #[test]
    fn atomics_remember_their_parent_style() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.open_inline(NodeId(1), &bold(), InlineEdges::default())
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default());
        match &b.items[1] {
            RawItem::Atomic {
                parent_style,
                style,
                ..
            } => {
                assert_eq!(*parent_style, 1);
                assert_eq!(*style, 0, "default style is shared with the root");
            }
            _ => panic!("expected an atomic"),
        }
    }
}
