//! Text analysis: white-space processing and the data line breaking uses.

mod transform;
mod transform_context;
pub(crate) mod units;
mod whitespace;
mod whitespace_context;

use std::ops::Range;

use crate::node::{InlineEdges, NodeId, OutOfFlowKind};

pub(crate) use transform::transform;
pub(crate) use whitespace::process;

/// An item of the processed paragraph. `text` indexes the processed text.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    pub(crate) text: Range<u32>,
    pub(crate) style: u32,
    pub(crate) node: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    Text,
    OpenInline {
        edges: InlineEdges,
    },
    CloseInline,
    Atomic {
        // Retained for atomic decoration output when real box painting is wired.
        #[allow(dead_code)]
        edges: InlineEdges,
        parent_style: u32,
    },
    OutOfFlow {
        kind: OutOfFlowKind,
    },
    BlockInInline,
    ForcedBreak,
    Tab,
    BidiControl,
}
