//! Per-thread layout state.

use crate::limits::{Warning, WarningSink};

/// Scratch state for layout. Create one per thread and reuse it; it is
/// `Send` but not meant to be shared.
#[derive(Debug, Default)]
pub struct LayoutContext {
    pub(crate) warnings: WarningSink,
    pub(crate) partial: Option<crate::line::cache::PartialLine>,
    #[cfg(test)]
    pub(crate) cache_visits: usize,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Warnings recorded by line layout since the last call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        self.warnings.take()
    }

    /// Releases the retained partial line when its storage exceeds `bytes`.
    /// `shrink_to(0)` always releases its paragraph reference as well.
    pub fn shrink_to(&mut self, bytes: usize) {
        if self.partial.as_ref().is_some_and(|p| p.bytes() > bytes) {
            self.partial = None;
        }
    }
}
