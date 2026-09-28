//! Paragraph builders: record the inline content of one block container.

use std::collections::HashMap;
use std::ops::Range;

use crate::context::LayoutContext;
use crate::font::FontCollection;
use crate::limits::{LimitExceeded, LimitKind, Limits, WarningKind, WarningSink};
use crate::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use crate::paragraph::Paragraph;
use crate::style::{InlineStyle, ParagraphStyle};

/// Input as recorded, before white-space processing.
#[derive(Clone, Debug)]
pub(crate) enum RawItem {
    RubyBoundary {
        ruby: u32,
        boundary: crate::ruby::builder::Boundary,
        node: Option<NodeId>,
        style: u32,
    },
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
    /// Explicit alternatives, keyed by the normal/alternate pair's index.
    /// Sparse storage adds no alternate style allocations to legacy inputs.
    pub(crate) first_line_styles: HashMap<u32, InlineStyle>,
    style_index: HashMap<String, u32>,
    /// Index of the most recently interned or reused style.
    last_interned: u32,
    /// Style indices of the currently open inline boxes.
    pub(crate) stack: Vec<u32>,
    pub(crate) error: Option<LimitExceeded>,
    pub(crate) warnings: WarningSink,
    pub(crate) offset_mapping: bool,
    pub(crate) rubies: Vec<crate::ruby::builder::RubyInput>,
    pub(crate) ruby_cost: crate::ruby::builder::InputCost,
    pub(crate) ruby_annotation: bool,
}

impl ParagraphBuilder {
    pub fn new(style: &ParagraphStyle, limits: &Limits) -> Self {
        let mut builder = Self {
            style: style.clone(),
            limits: limits.clone(),
            text: String::new(),
            items: Vec::new(),
            styles: Vec::new(),
            first_line_styles: HashMap::new(),
            style_index: HashMap::new(),
            last_interned: 0,
            stack: Vec::new(),
            error: None,
            warnings: WarningSink::new(limits.max_warnings),
            offset_mapping: true,
            rubies: Vec::new(),
            ruby_cost: Default::default(),
            ruby_annotation: false,
        };
        builder.styles.push(style.root.clone());
        builder
            .style_index
            .insert(format!("{:?}", (&style.root, None::<&InlineStyle>)), 0);
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
        self.open_inline_styles(node, style, None, edges)
    }

    /// Open an inline with caller-resolved normal and first-line styles.
    /// The supported first-line font/language/line-height/spacing/transform/
    /// emphasis properties use the supplied values exactly, including values
    /// equal to the normal root. Other formatting stays in `normal`.
    /// Resolve inheritance and relative values in the caller's cascade.
    /// An explicit alternative activates first-line layout even without a
    /// `ParagraphStyle::first_line` root override. Legacy `open_inline` keeps
    /// its documented value-based fallback; resolve every inline to avoid it.
    pub fn open_inline_with_first_line(
        &mut self,
        node: NodeId,
        normal: &InlineStyle,
        first_line: &InlineStyle,
        edges: InlineEdges,
    ) -> &mut Self {
        self.open_inline_styles(node, normal, Some(first_line), edges)
    }

    fn open_inline_styles(
        &mut self,
        node: NodeId,
        style: &InlineStyle,
        first_line: Option<&InlineStyle>,
        edges: InlineEdges,
    ) -> &mut Self {
        let depth = self.stack.len() as u64 + 1;
        if self.check(
            self.limits.max_nesting_depth,
            LimitKind::NestingDepth,
            depth,
        ) && self.reserve_item()
            && let Some(style) = self.intern_styles(style, first_line)
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
        let total = (self.text.len() as u64)
            .saturating_add(text.len() as u64)
            .saturating_add(self.ruby_cost.text);
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

    pub(crate) fn check(&mut self, limit: Option<u64>, kind: LimitKind, actual: u64) -> bool {
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

    pub(crate) fn reserve_item(&mut self) -> bool {
        let count = (self.items.len() as u64)
            .saturating_add(1)
            .saturating_add(self.ruby_cost.items);
        self.check(self.limits.max_items, LimitKind::Items, count)
    }

    fn intern(&mut self, style: &InlineStyle) -> Option<u32> {
        self.intern_styles(style, None)
    }

    pub(crate) fn intern_styles(
        &mut self,
        style: &InlineStyle,
        first_line: Option<&InlineStyle>,
    ) -> Option<u32> {
        // Consecutive and nested elements usually share a style; compare
        // with the enclosing box's style and the last interned one before
        // building the map key, whose cost grows with the style's size.
        for index in [self.current_style(), self.last_interned] {
            if self.styles.get(index as usize) == Some(style)
                && self.first_line_styles.get(&index) == first_line
            {
                self.last_interned = index;
                return Some(index);
            }
        }
        let key = format!("{:?}", (style, first_line));
        if let Some(&index) = self.style_index.get(&key) {
            self.last_interned = index;
            return Some(index);
        }
        let count = self.styles.len() as u64
            + self.first_line_styles.len() as u64
            + 1
            + u64::from(first_line.is_some())
            + self.ruby_cost.styles;
        if !self.check(self.limits.max_styles, LimitKind::Styles, count) {
            return None;
        }
        let index = self.styles.len() as u32;
        self.styles.push(style.clone());
        if let Some(first_line) = first_line {
            self.first_line_styles.insert(index, first_line.clone());
        }
        self.style_index.insert(key, index);
        self.last_interned = index;
        Some(index)
    }
}

impl ParagraphBuilder {
    /// Append a paired ruby container, preserving the original base sources.
    pub fn push_ruby(&mut self, node: NodeId, style: &InlineStyle, ruby: crate::Ruby) -> &mut Self {
        crate::ruby::builder::append(self, node, style, None, ruby);
        self
    }

    /// Append ruby with a caller-resolved first-line container style.
    pub fn push_ruby_with_first_line(
        &mut self,
        node: NodeId,
        normal: &InlineStyle,
        first_line: &InlineStyle,
        ruby: crate::Ruby,
    ) -> &mut Self {
        crate::ruby::builder::append(self, node, normal, Some(first_line), ruby);
        self
    }

    pub(crate) fn into_ruby_content(mut self) -> crate::ruby::input::ContentInput {
        self.close_unbalanced();
        let has_first_line = self.style.first_line.is_some()
            || !self.first_line_styles.is_empty()
            || self.has_ruby_first_line();
        crate::ruby::input::ContentInput {
            style: self.style,
            limits: self.limits,
            text: self.text,
            items: self.items,
            styles: self.styles,
            first_line_styles: self.first_line_styles,
            error: self.error,
            warnings: self.warnings.take(),
            offset_mapping: self.offset_mapping,
            rubies: self.rubies,
            ruby_cost: self.ruby_cost,
            has_first_line,
        }
    }

    pub(crate) fn has_ruby_first_line(&self) -> bool {
        crate::ruby::prepare::has_first_line(&self.rubies)
    }

    pub(crate) fn from_ruby_content(
        input: &crate::ruby::input::ContentInput,
        node: NodeId,
    ) -> Result<Self, LimitExceeded> {
        if let Some(error) = input.error {
            return Err(error);
        }
        // Restored items are balanced, so they are absent from builder.stack.
        // Validate the generated isolation wrapper around their real depth
        // before cloning the snapshot, including any nested ruby descendants.
        Limits::check(
            input.limits.max_nesting_depth,
            LimitKind::NestingDepth,
            crate::ruby::builder::InputCost::content(input)
                .depth
                .saturating_add(1),
        )?;
        let mut builder = Self::new(&input.style, &input.limits);
        builder.text = input.text.clone();
        builder.items = input.items.clone();
        builder.styles = input.styles.clone();
        builder.first_line_styles = input.first_line_styles.clone();
        builder.offset_mapping = input.offset_mapping;
        builder.rubies = input.rubies.clone();
        builder.ruby_cost = input.ruby_cost;
        builder.ruby_annotation = true;
        for warning in &input.warnings {
            builder.warnings.push(warning.kind, warning.message.clone());
        }
        crate::ruby::prepare::annotation_breaks(&mut builder)?;
        let normal = crate::ruby::builder::isolated(&builder.style.root);
        let first = builder
            .style
            .first_line
            .as_ref()
            .map(crate::ruby::builder::isolated);
        match first.as_ref() {
            Some(first) => {
                builder.open_inline_with_first_line(node, &normal, first, InlineEdges::default());
            }
            None => {
                builder.open_inline(node, &normal, InlineEdges::default());
            }
        }
        if let Some(error) = builder.error {
            return Err(error);
        }
        let open = builder
            .items
            .pop()
            .expect("successful open_inline records a marker");
        builder.items.insert(0, open);
        builder.close_inline();
        if let Some(error) = builder.error {
            return Err(error);
        }
        Ok(builder)
    }

    fn close_unbalanced(&mut self) {
        while !self.stack.is_empty() && self.error.is_none() {
            self.warnings.push(
                WarningKind::UnbalancedInline,
                "unclosed inline box closed at build",
            );
            self.close_inline();
        }
    }
    /// Analyzes and shapes the content. Fails only when a resource limit was
    /// exceeded; inline boxes left open are closed with a warning.
    pub fn build(
        mut self,
        cx: &mut LayoutContext,
        fonts: &FontCollection,
    ) -> Result<Paragraph, LimitExceeded> {
        self.close_unbalanced();
        if let Some(e) = self.error {
            return Err(e);
        }
        Paragraph::from_builder(self, cx, fonts)
    }
}

/// Convenience builder for plain rich text (no DOM). The n-th pushed span
/// gets `NodeId(n)`, starting at 0, and offsets within the pushed string map
/// through [`crate::mapping::OffsetMapping`].
pub struct RichText {
    builder: ParagraphBuilder,
    next_node: u64,
}

impl RichText {
    /// Append ruby, assigning its container through this builder's node counter.
    pub fn push_ruby(mut self, ruby: crate::Ruby, style: &InlineStyle) -> Self {
        let node = NodeId(self.next_node);
        self.next_node += 1;
        self.builder.push_ruby(node, style, ruby);
        self
    }
    pub fn new(style: &ParagraphStyle) -> Self {
        Self::with_limits(style, &Limits::default())
    }

    pub fn with_limits(style: &ParagraphStyle, limits: &Limits) -> Self {
        Self {
            builder: ParagraphBuilder::new(style, limits),
            next_node: 0,
        }
    }

    pub fn push(mut self, text: &str, style: &InlineStyle) -> Self {
        let node = NodeId(self.next_node);
        self.next_node += 1;
        self.builder
            .open_inline(node, style, InlineEdges::default())
            .push_text(TextSource::Dom { node, offset: 0 }, text)
            .close_inline();
        self
    }

    /// Append one span with caller-resolved normal and first-line styles.
    /// Uses the same property subset and fallback contract as
    /// [`ParagraphBuilder::open_inline_with_first_line`].
    pub fn push_with_first_line(
        mut self,
        text: &str,
        normal: &InlineStyle,
        first_line: &InlineStyle,
    ) -> Self {
        let node = NodeId(self.next_node);
        self.next_node += 1;
        self.builder
            .open_inline_with_first_line(node, normal, first_line, InlineEdges::default())
            .push_text(TextSource::Dom { node, offset: 0 }, text)
            .close_inline();
        self
    }

    pub fn build(
        self,
        cx: &mut LayoutContext,
        fonts: &FontCollection,
    ) -> Result<Paragraph, LimitExceeded> {
        self.builder.build(cx, fonts)
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
    fn repeated_and_nested_equal_styles_share_one_index() {
        let big = InlineStyle {
            font_families: vec![crate::style::FontFamily::Named("x".repeat(100 * 1024))],
            ..InlineStyle::default()
        };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::unlimited());
        for n in 0..20_000 {
            b.open_inline(NodeId(n), &big, InlineEdges::default())
                .close_inline();
        }
        // Nested inside a box of the same style, and alternating with another.
        b.open_inline(NodeId(1), &big, InlineEdges::default())
            .open_inline(NodeId(2), &big, InlineEdges::default())
            .push_atomic(NodeId(3), &big, InlineEdges::default())
            .open_inline(NodeId(4), &bold(), InlineEdges::default())
            .close_inline()
            .open_inline(NodeId(5), &big, InlineEdges::default())
            .close_inline()
            .open_inline(NodeId(6), &bold(), InlineEdges::default());
        assert_eq!(b.styles.len(), 3, "root, big and bold");
        assert_eq!(b.stack, [1, 1, 2]);
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
