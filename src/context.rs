//! Per-thread layout state.

use crate::limits::{Warning, WarningSink};

/// Scratch state for layout. Create one per thread and reuse it; it is
/// `Send` but not meant to be shared.
#[derive(Debug, Default)]
pub struct LayoutContext {
    pub(crate) warnings: WarningSink,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Warnings recorded by line layout since the last call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        self.warnings.take()
    }

    /// Releases retained scratch memory above `bytes`. Nothing is retained
    /// yet, so this is currently a no-op.
    pub fn shrink_to(&mut self, _bytes: usize) {}
}
