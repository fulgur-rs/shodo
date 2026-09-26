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

    /// Drops scratch that exceeds the current shaping window's conservative cap.
    pub(crate) fn bound_shaping_scratch(&mut self, limits: &crate::limits::Limits) {
        if let Some(bytes) = limits.max_shaping_run_bytes {
            let cap = usize::try_from(bytes)
                .unwrap_or(usize::MAX)
                .max(1)
                .saturating_mul(128);
            if self.scratch_bytes > cap {
                self.scratch = None;
                self.scratch_bytes = 0;
            }
        }
    }

    /// Releases retained shaping plans and buffers exceeding `bytes`.
    /// `shrink_to(0)` also releases any partial line's paragraph reference.
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
