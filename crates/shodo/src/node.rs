//! Identities and box edges supplied by the caller.

/// Opaque caller-defined identifier: a DOM node for CSS engines, or any span
/// tag (for example an ECS entity) for other users.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

/// Where a piece of text came from, for mapping offsets back to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSource {
    /// Text of a DOM text node, starting at `offset` (UTF-8 bytes) within it.
    Dom { node: NodeId, offset: u32 },
    /// Generated content (`::before`, list markers) with no DOM offsets.
    Generated { node: NodeId },
}

impl TextSource {
    pub fn node(self) -> NodeId {
        match self {
            Self::Dom { node, .. } | Self::Generated { node } => node,
        }
    }
}

/// Four logical sides, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides {
    pub inline_start: f32,
    pub inline_end: f32,
    pub block_start: f32,
    pub block_end: f32,
}

impl Sides {
    pub fn inline_sum(&self) -> f32 {
        self.inline_start + self.inline_end
    }

    /// The four sides in a fixed order, for per-side updates.
    pub(crate) fn values_mut(&mut self) -> [&mut f32; 4] {
        [
            &mut self.inline_start,
            &mut self.inline_end,
            &mut self.block_start,
            &mut self.block_end,
        ]
    }

    /// The four sides in the order of [`Self::values_mut`].
    pub(crate) fn values(&self) -> [f32; 4] {
        [
            self.inline_start,
            self.inline_end,
            self.block_start,
            self.block_end,
        ]
    }
}

/// Margin, border and padding of an inline box or atomic inline. The inline
/// sides take space in the line; the block sides do not affect the line box
/// height (CSS 2.1 §10.8.1) but are part of the reported border box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InlineEdges {
    pub margin: Sides,
    pub border: Sides,
    pub padding: Sides,
}

impl InlineEdges {
    pub fn inline_start_total(&self) -> f32 {
        self.margin.inline_start + self.border.inline_start + self.padding.inline_start
    }

    pub fn inline_end_total(&self) -> f32 {
        self.margin.inline_end + self.border.inline_end + self.padding.inline_end
    }
}

/// Percentage parts of an inline box's margin and padding, as fractions of
/// the percentage basis (`0.5` is `50%`).
///
/// CSS resolves margin and padding percentages, on every side, against the
/// inline size of the containing block, which is only known at line layout.
/// Record them with [`crate::ParagraphBuilder::set_inline_edge_percents`]
/// next to the px parts in [`InlineEdges`]; a `calc(10% + 4px)` padding is
/// `0.1` here and `4.0` in [`InlineEdges::padding`]. The built paragraph
/// resolves every percentage against `0`, as intrinsic sizing requires (the
/// cyclic percentage rules of CSS Sizing 3 §5.2.1); use
/// [`crate::Paragraph::with_edge_basis`] to obtain the paragraph for a
/// definite basis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InlineEdgePercents {
    pub margin: Sides,
    pub padding: Sides,
}

impl InlineEdgePercents {
    /// Whether every percentage is zero.
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// Logical inline sides that carry a nonzero percentage:
    /// [`Self::INLINE_START`] and/or [`Self::INLINE_END`].
    pub(crate) fn inline_sides(&self) -> u8 {
        let mut sides = 0;
        if self.margin.inline_start != 0.0 || self.padding.inline_start != 0.0 {
            sides |= Self::INLINE_START;
        }
        if self.margin.inline_end != 0.0 || self.padding.inline_end != 0.0 {
            sides |= Self::INLINE_END;
        }
        sides
    }

    pub(crate) const INLINE_START: u8 = 1;
    pub(crate) const INLINE_END: u8 = 2;
}

/// Out-of-flow boxes anchored in the inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutOfFlowKind {
    Float,
    Absolute,
}
