//! Text analysis: white-space processing and the data line breaking uses.

pub(crate) mod bidi;
pub(crate) mod breaks;
pub(crate) mod combine;
pub(crate) mod first_letter;
pub(crate) mod itemize;
mod transform;
mod transform_context;
pub(crate) mod units;
pub(crate) mod whitespace;
mod whitespace_context;
mod width;

use std::ops::Range;

use crate::node::{InlineEdges, NodeId, OutOfFlowKind};

#[cfg(test)]
pub(crate) use transform::transform;
pub(crate) use transform::transform_with_base_scopes;
#[cfg(test)]
pub(crate) use whitespace::process;

/// An item of the processed paragraph. `text` indexes the processed text.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    pub(crate) text: Range<u32>,
    pub(crate) style: u32,
    pub(crate) node: Option<NodeId>,
    /// A forced break supplied with its own style instead of its container's.
    pub(crate) own_break_style: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    RubyBoundary {
        ruby: u32,
        boundary: crate::ruby::builder::Boundary,
    },
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

mod scripts;

pub(crate) mod language;
