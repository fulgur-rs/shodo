//! Fail-closed bound on ruby line-measurement work (shodo-mc0).
//!
//! The through memo (`super::memo`) and the container accumulator
//! (`super::accumulate`) make the common fit scans linear, but some inputs
//! still re-measure most containers at every probe: a line profile that
//! changes at every step, a walk beyond the accumulator's container cap,
//! measurements that warn (never stored) under `max_warnings: None`, and
//! the other shapes listed in `docs/records/shodo-mc0-ruby-line-work.md`.
//!
//! Each `next_line` or `intrinsic_sizes` call therefore gets an allowance of
//! `Limits::max_ruby_line_work` (a factor) times the units its fit probes
//! cover (the last end minus the first start; `intrinsic_sizes` with a
//! first line mixes the unit indices of its two datasets, which can at most
//! double the span) plus the widest container walk it has made. Every live
//! container measurement (`measure::measure_one`), every container a walk
//! (`memo::advance`) re-applies or visits (a restarted walk visits them all
//! again) and every position an accumulator adds one at a time costs one
//! unit. A probe is admitted only while the spent units are
//! below the allowance; once one is refused, the rest of the call answers
//! every adjustment-only probe with zero (sticky, so no later probe resumes
//! a walk that skipped steps) and the sink takes one warning.
//!
//! A walk wider than an accumulator keeps (`accumulate::MAX_CONTAINERS`,
//! 16,384) is measured for at most one look-ahead end (`through`) per start
//! and atomic revision in an operation (asking for that same end again
//! measures it again): the through memo alone would measure every container
//! of such a walk again at each step, so a probe with another end is
//! refused the same way.
//!
//! A probe admitted just below the allowance measures at most the
//! containers of its walk and the annotation-lane containers nested in
//! them, so the fit probes of a call spend at most the allowance plus one
//! such walk. An accepted line then measures its own containers once more,
//! and the float placement probe after `PartialLine::index` is measured
//! outside the allowance (one probe per call; linear either way).
//!
//! The refusal is a function of the call's own probe sequence and of the
//! state the exact-replay gates read (`edge_reshape_spent`, sink
//! suppression), not of the contents of caches left by earlier calls: the
//! per-operation memo and accumulators start empty, range cache hits charge
//! like misses (shodo-tj5), and a scan that warned is never retained as a
//! `PartialLine`. A retained line changes the probe sequence only through
//! `PartialLine::index`. Its probes are a superset of a fresh narrower
//! scan's (repeated probes add work but never allowance: it uses maxima),
//! so an index that is not refused means the fresh scan would not be
//! either; a failed index restores this state and clears the memo before
//! scanning again. One input remains shared with main's reshape budget: a
//! failed index keeps its reshape charges (`edge_reshape_spent` is not
//! restored, as before this limit), and the replay gates of the rescan read
//! them.
//!
//! Accepted lines measure their ruby in full (`measure::candidate` through
//! `apply`), so the ruby geometry of a placed line is exact; only the fit
//! decision (and intrinsic sizes) ignore annotation overflow after a refusal.
use crate::LayoutContext;
use crate::limits::WarningKind;
use crate::paragraph::ParagraphData;

/// Work state of the current operation, reset by
/// `LayoutContext::begin_reshape_operation`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineWork {
    /// Units spent: live container measurements and sequentially added
    /// positions.
    spent: u64,
    /// Units the admitted probes cover: the last end minus the first start
    /// over all of them (`intrinsic_sizes` probes many starts).
    span: u64,
    /// Smallest admitted start.
    first: Option<usize>,
    /// Largest admitted end.
    last: usize,
    /// Most containers one admitted probe walked.
    walk: u64,
    /// `(start, atomic revision, through)` of the first walk per start and
    /// revision that was wider than an accumulator keeps and measured on a
    /// memo miss (`admit_walk`). Each entry stands for more than
    /// `MAX_CONTAINERS` spent units, so few exist.
    wide: Vec<(usize, u64, usize)>,
    /// A probe was refused: every later probe of the operation is too.
    exhausted: bool,
    /// Admit without reading or updating the state
    /// (`measure::candidate_adjustment_exempt`).
    pub(crate) exempt: bool,
}

impl LineWork {
    pub(crate) fn clear(&mut self) {
        self.spent = 0;
        self.span = 0;
        self.first = None;
        self.last = 0;
        self.walk = 0;
        self.exhausted = false;
        self.exempt = false;
        self.wide.clear();
    }

    #[cfg(test)]
    pub(crate) fn spent(&self) -> u64 {
        self.spent
    }

    #[cfg(test)]
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted
    }

    #[cfg(test)]
    pub(crate) fn walk(&self) -> u64 {
        self.walk
    }

    /// `span + walk`, the extent the allowance multiplies.
    #[cfg(test)]
    pub(crate) fn extent(&self) -> u64 {
        self.span + self.walk
    }
}

pub(crate) const WARNING: &str =
    "ruby line measurement budget exceeded; fitting without ruby adjustments";

/// Charge `units` of work to the current operation.
pub(crate) fn charge(cx: &mut LayoutContext, units: u64) {
    cx.ruby_line_work.spent = cx.ruby_line_work.spent.saturating_add(units);
}

/// Whether the adjustment-only probe `start..end` may measure. Refusing the
/// first time warns once and makes the operation's later probes refuse too.
pub(crate) fn admit(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
) -> bool {
    let Some(factor) = data.limits.max_ruby_line_work else {
        return true;
    };
    #[cfg(test)]
    if cx.ruby_line_work_disabled {
        return true;
    }
    let work = &mut cx.ruby_line_work;
    if work.exempt {
        return true;
    }
    if !work.exhausted {
        let first = work.first.map_or(start, |first| first.min(start));
        work.first = Some(first);
        work.last = work.last.max(end);
        work.span = (work.last - first) as u64;
        let allowance = factor.saturating_mul(work.span.saturating_add(work.walk));
        if work.spent < allowance {
            return true;
        }
        refuse(cx);
    }
    false
}

/// Record the container walk of an admitted probe.
pub(crate) fn walked(cx: &mut LayoutContext, containers: usize) {
    let work = &mut cx.ruby_line_work;
    work.walk = work.walk.max(containers as u64);
}

/// Whether an admitted probe of `start..through` that missed the memo may
/// measure its walk of `containers`. A walk wider than an accumulator keeps
/// is measured by the through memo alone, every container at every new
/// `through`: per start and atomic revision, only the first such `through`
/// is measured (again if asked again: intrinsic sizes ask for the end of a
/// row with the min and the max atomics, which may share a revision); a
/// probe with another `through` is refused (warning once, like `admit`, and
/// refusing every later probe).
pub(crate) fn admit_walk(
    data: &ParagraphData,
    atomics: &crate::AtomicSizes,
    start: usize,
    through: usize,
    containers: usize,
    cx: &mut LayoutContext,
) -> bool {
    if data.limits.max_ruby_line_work.is_none()
        || containers <= super::accumulate::max_containers(cx)
    {
        return true;
    }
    #[cfg(test)]
    if cx.ruby_line_work_disabled {
        return true;
    }
    let work = &mut cx.ruby_line_work;
    if work.exempt {
        return true;
    }
    match work
        .wide
        .iter()
        .find(|(s, revision, _)| *s == start && *revision == atomics.revision)
    {
        None => {
            work.wide.push((start, atomics.revision, through));
            true
        }
        Some(&(_, _, first)) if first == through => true,
        Some(_) => {
            refuse(cx);
            false
        }
    }
}

fn refuse(cx: &mut LayoutContext) {
    cx.ruby_line_work.exhausted = true;
    cx.warnings.push(WarningKind::Unsupported, WARNING);
}
