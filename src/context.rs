//! Per-thread layout state.

use crate::limits::{Warning, WarningSink};

/// Scratch state for layout. Create one per thread and reuse it; it is
/// `Send` but not meant to be shared.
#[derive(Debug, Default)]
pub struct LayoutContext {
    pub(crate) warnings: WarningSink,
    pub(crate) plans: crate::shape::cache::PlanCache,
    pub(crate) scratch: Option<harfrust::UnicodeBuffer>,
    pub(crate) scratch_bytes: usize,
    // A context is moved between threads, never concurrently shared.
    _not_sync: std::marker::PhantomData<std::cell::Cell<()>>,
    pub(crate) partial: Option<crate::line::cache::PartialLine>,
    #[cfg(test)]
    pub(crate) cache_visits: usize,
    #[cfg(test)]
    pub(crate) float_search_visits: usize,
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
        // Harfrust does not expose a plan's heap size; dropping the bounded
        // cache conservatively releases all of it on an explicit shrink.
        self.plans.clear();
        if bytes == 0 || self.scratch_bytes > bytes {
            self.scratch = None;
            self.scratch_bytes = 0;
        }
        if self.partial.as_ref().is_some_and(|p| p.bytes() > bytes) {
            self.partial = None;
        }
    }
}
