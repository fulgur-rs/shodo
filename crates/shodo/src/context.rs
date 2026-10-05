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
    pub(crate) completed: Option<Box<crate::line::completed::CompletedLine>>,
    pub(crate) ruby_ranges: crate::line::range::RangeCache,
    pub(crate) edge_shapes: crate::line::windows::EdgeShapeCache,
    /// Bytes of edge reshape windows requested by the current `next_line` or
    /// `intrinsic_sizes` call. First-line intrinsic passes share one budget.
    pub(crate) edge_reshape_spent: u64,
    /// Reshape charges of the measurements currently being recorded for
    /// exact replay.
    pub(crate) reshape_log: crate::line::replay::ReshapeLog,
    /// Adjustment-only ruby candidates measured in the current operation.
    pub(crate) ruby_memo: crate::ruby::memo::RubyMemo,
    /// Ruby line-measurement work of the current operation (shodo-mc0).
    pub(crate) ruby_line_work: crate::ruby::line_work::LineWork,
    /// The ruby line-measurement work state of every operation that charged
    /// any, as it was when the next one began.
    /// Admit every ruby probe: tests that sweep every range of a paragraph
    /// in one operation (not a layout call) check reuse exactness alone.
    #[cfg(test)]
    pub(crate) ruby_line_work_disabled: bool,
    #[cfg(test)]
    pub(crate) ruby_line_work_log: Vec<crate::ruby::line_work::LineWork>,
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
    /// Measure every ruby candidate in full: the reference path that each
    /// reuse path must match exactly.
    #[cfg(test)]
    pub(crate) ruby_reference: bool,
    /// Calls of `line::range::width` (ruby range widths).
    #[cfg(test)]
    pub(crate) ruby_width_calls: usize,
    /// Calls of `line::metric_index::scalar::measure`.
    #[cfg(test)]
    pub(crate) ruby_scalar_calls: usize,
    /// Selected line profiles measured by `metric_index::content_shared`.
    #[cfg(test)]
    pub(crate) ruby_profile_selects: usize,
    /// `line::replay::replay` calls refused by the gate (not counting
    /// measurements that never recorded effects because they warned).
    #[cfg(test)]
    pub(crate) ruby_replay_refusals: usize,
    /// Ruby candidate cores answered from `ruby_memo`.
    #[cfg(test)]
    pub(crate) ruby_memo_hits: usize,
    /// Containers measured by `ruby::measure::measure_one`.
    #[cfg(test)]
    pub(crate) ruby_container_measures: usize,
    /// Step-oracle mismatches (clean containers whose live measurement
    /// differed from their entry).
    #[cfg(test)]
    pub(crate) ruby_oracle_misses: Vec<String>,
    /// Containers answered by an accumulator segment replay.
    #[cfg(test)]
    pub(crate) ruby_replayed_containers: usize,
    /// Accumulator dirty marks by reason (`ruby::accumulate::Dirty`).
    #[cfg(test)]
    pub(crate) ruby_dirty: [usize; 7],
    /// Accumulators reset at the end of a step that could not keep them.
    #[cfg(test)]
    pub(crate) ruby_accumulator_resets: usize,
    /// Use the per-through memo alone (the shodo-d77 path).
    #[cfg(test)]
    pub(crate) ruby_accumulate_disabled: bool,
    /// Step oracle: measure clean accumulator positions live and compare
    /// them with their entries (`ruby_oracle_misses`).
    #[cfg(test)]
    pub(crate) ruby_accumulate_verify: bool,
    /// Override of `ruby::accumulate::MAX_CONTAINERS`.
    #[cfg(test)]
    pub(crate) ruby_accumulate_cap: Option<usize>,
    /// Step-oracle comparisons (clean entries that would have replayed,
    /// measured live and compared).
    #[cfg(test)]
    pub(crate) ruby_oracle_checks: usize,
    /// Accumulator replays whose saturation guard failed, so the run's
    /// adjustments were added one at a time.
    #[cfg(test)]
    pub(crate) ruby_sequential_replays: usize,
    /// Accumulator replays of a run with more profile calls than positions
    /// while the step's profile charged reshape bytes (the `m > 1` case of
    /// the detached profile contract).
    #[cfg(test)]
    pub(crate) ruby_repeated_profile_replays: usize,
    /// Reads of completed descendants by an enclosing container: one per
    /// read the accumulator's segment tree answers, one per position or
    /// fragment iterated otherwise (its fallback loop, and the reference
    /// path's `measure::Completed`). Counted apart from general visits.
    #[cfg(test)]
    pub(crate) ruby_descendant_reads: usize,
    /// Detached profile measurements (`metric_index::content_shared`) that
    /// warned, so the step kept no fixed profile.
    #[cfg(test)]
    pub(crate) ruby_profile_warnings: usize,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Warnings recorded by line layout since the last call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        self.warnings.take()
    }

    /// Start one `next_line` or `intrinsic_sizes` operation: reset the edge
    /// reshape budget, the ruby line-measurement work and every
    /// per-operation reuse state bounded by them.
    pub(crate) fn begin_reshape_operation(&mut self) {
        self.edge_reshape_spent = 0;
        self.reshape_log.clear();
        self.ruby_memo.clear();
        #[cfg(test)]
        if self.ruby_line_work != Default::default() {
            self.ruby_line_work_log.push(self.ruby_line_work.clone());
        }
        self.ruby_line_work.clear();
    }

    /// Whether measurements may be reused within an operation. Tests switch
    /// reuse off to obtain the reference path.
    pub(crate) fn reuse_enabled(&self) -> bool {
        #[cfg(test)]
        {
            !self.ruby_reference
        }
        #[cfg(not(test))]
        {
            true
        }
    }

    /// Whether adjustment-only candidates may use the container accumulator
    /// (`ruby::accumulate`). Tests switch it off to obtain the through-memo
    /// path alone.
    pub(crate) fn accumulate_enabled(&self) -> bool {
        #[cfg(test)]
        {
            self.reuse_enabled() && !self.ruby_accumulate_disabled
        }
        #[cfg(not(test))]
        {
            true
        }
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
    /// Completed rejected lines are also released on every explicit shrink.
    /// `shrink_to(0)` also releases any partial line's paragraph reference.
    pub fn shrink_to(&mut self, bytes: usize) {
        // Harfrust does not expose a plan's heap size; dropping the bounded
        // cache conservatively releases all of it on an explicit shrink.
        self.plans.clear();
        // A rejected completed line can prolong shared paragraph/font owners.
        // Release it conservatively on any explicit shrink.
        self.completed = None;
        self.ruby_ranges = Default::default();
        self.ruby_memo = Default::default();
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
