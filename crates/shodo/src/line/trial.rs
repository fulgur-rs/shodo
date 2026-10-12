//! Private Balance trial results; unsupported lines retain full finalization.
use crate::node::NodeId;
use crate::{BreakToken, FloatCursor, LineResult};

#[derive(Clone, Copy, Debug)]
pub(super) struct TrialSummary {
    pub(super) token: BreakToken,
    pub(super) block_size: f32,
}

// Preserve all non-line information at the formatter boundary. The fixed
// width driver consumes only progress; other event fields are intentionally
// retained rather than changing the meaning of a formatting result.
#[allow(dead_code)]
pub(super) enum TrialResult {
    Line(TrialSummary),
    FloatEncountered {
        node: NodeId,
        line_start: BreakToken,
        inline_position: f32,
        float_cursor: FloatCursor,
    },
    BlockInInline {
        node: NodeId,
        token_after: BreakToken,
    },
    BlockSizeExceeded {
        needed_block_size: f32,
    },
    Done,
    InvalidToken,
}

pub(super) trait Finalizer {
    type Output;
    fn from_full(result: LineResult) -> Self::Output;
}

pub(super) struct Full;
impl Finalizer for Full {
    type Output = LineResult;
    fn from_full(result: LineResult) -> Self::Output {
        result
    }
}

pub(super) struct Trial;
impl Finalizer for Trial {
    type Output = TrialResult;
    fn from_full(result: LineResult) -> Self::Output {
        match result {
            LineResult::Line(line) => TrialResult::Line(TrialSummary {
                token: line.break_token(),
                block_size: line.block_size(),
            }),
            LineResult::FloatEncountered {
                node,
                line_start,
                inline_position,
                float_cursor,
            } => TrialResult::FloatEncountered {
                node,
                line_start,
                inline_position,
                float_cursor,
            },
            LineResult::BlockInInline { node, token_after } => {
                TrialResult::BlockInInline { node, token_after }
            }
            LineResult::BlockSizeExceeded { needed_block_size } => {
                TrialResult::BlockSizeExceeded { needed_block_size }
            }
            LineResult::Done => TrialResult::Done,
            LineResult::InvalidToken => TrialResult::InvalidToken,
        }
    }
}

#[cfg(test)]
mod tests;
