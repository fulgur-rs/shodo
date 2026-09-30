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
    pub(crate) ruby_ranges: crate::line::range::RangeCache,
    pub(crate) edge_shapes: crate::line::windows::EdgeShapeCache,
    /// Bytes of edge reshape windows requested by the current `next_line` or
    /// `intrinsic_sizes` call. First-line intrinsic passes share one budget.
    pub(crate) edge_reshape_spent: u64,
    #[cfg(test)]
    pub(crate) cache_visits: usize,
    #[cfg(test)]
    pub(crate) cache_prepare_visits: usize,
    #[cfg(test)]
    pub(crate) float_search_visits: usize,
    #[cfg(test)]
    pub(crate) ruby_measure_visits: usize,
    #[cfg(test)]
    pub(crate) ruby_lane_visits: usize,
    #[cfg(test)]
    pub(crate) ruby_column_visits: usize,
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

    /// Releases shaping plans and bounds the combined accounted storage of
    /// shaping scratch and partial-line buffers by `bytes`. Ruby range indexes
    /// are released conservatively on every explicit shrink, as is the cache of
    /// reshaped line-edge windows.
    /// `shrink_to(0)` also releases any partial line's paragraph reference.
    pub fn shrink_to(&mut self, bytes: usize) {
        // Harfrust does not expose a plan's heap size; dropping the bounded
        // cache conservatively releases all of it on an explicit shrink.
        self.plans.clear();
        self.ruby_ranges = Default::default();
        self.edge_shapes.clear();
        if bytes == 0 || self.scratch_bytes > bytes {
            self.scratch = None;
            self.scratch_bytes = 0;
        }
        let remaining = bytes.saturating_sub(self.scratch_bytes);
        if self.partial.as_ref().is_some_and(|p| p.bytes() > remaining) {
            self.partial = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, OutOfFlowKind, TextSource};
    use crate::style::ParagraphStyle;
    use crate::{AtomicSizes, LineConstraint, LineResult, ParagraphBuilder};

    #[test]
    fn shrink_budget_is_shared_by_real_shaping_and_partial_line_buffers() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register(crate::test_support::fonts::LATIN.to_vec())
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.first_line = Some(style.root.clone());
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi before ")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(3) }, "after");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts).unwrap();
        assert!(matches!(
            p.next_line(
                &mut cx,
                p.start_token(),
                &Default::default(),
                &LineConstraint::new(1000.0),
                &AtomicSizes::EMPTY
            ),
            LineResult::FloatEncountered { .. }
        ));
        assert!(cx.scratch.is_some());
        assert!(cx.plans.len() > 0);
        let partial = cx.partial.as_ref().unwrap().bytes();
        assert!(partial > 0 && cx.scratch_bytes > 0);
        let cap = partial.max(cx.scratch_bytes);
        cx.shrink_to(cap);
        let retained = cx.scratch_bytes + cx.partial.as_ref().map_or(0, |p| p.bytes());
        assert!(
            retained <= cap,
            "retained {retained} exceeds aggregate cap {cap}"
        );
        cx.shrink_to(0);
        assert!(cx.scratch.is_none() && cx.partial.is_none());
        assert_eq!(cx.scratch_bytes, 0);
        assert_eq!(cx.plans.len(), 0);
    }
}
