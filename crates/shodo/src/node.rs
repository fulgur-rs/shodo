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

/// Out-of-flow boxes anchored in the inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutOfFlowKind {
    Float,
    Absolute,
}
