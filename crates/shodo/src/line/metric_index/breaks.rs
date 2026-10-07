//! Styled-only profiles and incrementally prepared eligibility summaries.
//! A growing range with a fixed start decides each break at most once per
//! trimming mode. The ordinary zero/one-break query needs no cache allocation.
use super::super::metrics::RecordProfile;
use super::quirk::QuirkIndex;
use super::scalar::Summary;
use crate::LayoutContext;
use crate::paragraph::ParagraphData;
use std::ops::Range;

#[derive(Debug)]
struct Profile {
    unit: usize,
    summary: Summary,
}

#[derive(Debug)]
struct ParentRun {
    end: usize,
    parent: Option<u32>,
}

#[derive(Debug)]
struct Prepared {
    start: usize,
    first: usize,
    through: usize,
    all: Vec<Summary>,
    kept: Vec<Summary>,
}

#[derive(Debug, Default)]
pub(super) struct Breaks {
    profiles: Vec<Profile>,
    parents: Vec<ParentRun>,
    prepared: Option<Prepared>,
}

impl Breaks {
    pub(super) fn push(&mut self, data: &ParagraphData, unit: usize, profile: RecordProfile) {
        let mut summary = Summary::profile(profile, true);
        if let Some(bottom) = profile.own_group {
            summary.height = profile.bottom - profile.top;
            if bottom {
                summary.bottom_height = summary.height;
            }
        }
        self.profiles.push(Profile { unit, summary });
        let parent = data.units[unit].parent_box;
        if let Some(run) = self.parents.last_mut()
            && run.parent == parent
        {
            run.end = self.profiles.len();
        } else {
            self.parents.push(ParentRun {
                end: self.profiles.len(),
                parent,
            });
        }
    }

    fn window(&self, range: &Range<usize>) -> Range<usize> {
        self.profiles.partition_point(|p| p.unit < range.start)
            ..self.profiles.partition_point(|p| p.unit < range.end)
    }

    /// Prepare only newly selected styled breaks. Stale leaves outside this
    /// start's prepared window cannot enter a clipped summary query.
    pub(super) fn prepare(
        &mut self,
        data: &ParagraphData,
        q: &QuirkIndex,
        range: &Range<usize>,
        cx: &mut LayoutContext,
    ) {
        let selected = self.window(range);
        if selected.len() < 2 {
            return;
        }
        let size = self.profiles.len().next_power_of_two();
        let prepared = self.prepared.get_or_insert_with(|| Prepared {
            start: range.start,
            first: selected.start,
            through: selected.start,
            all: vec![Summary::default(); size * 2],
            kept: vec![Summary::default(); size * 2],
        });
        if prepared.start != range.start {
            prepared.start = range.start;
            prepared.first = selected.start;
            prepared.through = selected.start;
        }
        for i in prepared.through..selected.end {
            let p = &self.profiles[i];
            let all = eligible(data, q, range.start, p.unit, None, cx);
            let kept = eligible(data, q, range.start, p.unit, Some(range.start), cx);
            update(
                &mut prepared.all,
                i,
                if all { p.summary } else { Summary::default() },
            );
            update(
                &mut prepared.kept,
                i,
                if kept { p.summary } else { Summary::default() },
            );
        }
        prepared.through = prepared.through.max(selected.end);
    }

    pub(super) fn query(
        &self,
        data: &ParagraphData,
        q: &QuirkIndex,
        range: &Range<usize>,
        start: usize,
        trailing: usize,
        cx: &mut LayoutContext,
    ) -> Summary {
        let selected = self.window(range);
        if selected.is_empty() {
            return Summary::default();
        }
        let Some(prepared) = self
            .prepared
            .as_ref()
            .filter(|p| p.start == start && p.first <= selected.start && selected.end <= p.through)
        else {
            // The full selection prepares every multi-break window first.
            debug_assert_eq!(selected.len(), 1);
            let p = &self.profiles[selected.start];
            return if eligible(data, q, start, p.unit, Some(trailing), cx) {
                p.summary
            } else {
                Summary::default()
            };
        };
        let split = self
            .profiles
            .partition_point(|p| p.unit < trailing)
            .clamp(selected.start, selected.end);
        let mut summary = query(&prepared.all, selected.start..split, cx);
        // A trailing run contains no Open unit. Its styled-break parents can
        // only move up the boundary ancestry as Close units are passed, so
        // this loop visits parent segments, never each trailing break.
        let mut first = split;
        let mut run = self.parents.partition_point(|r| r.end <= first);
        while first < selected.end {
            let end = self.parents[run].end.min(selected.end);
            let unit = self.profiles[first].unit;
            let (_, lo) = q
                .ending_break(data, &(start..unit + 1))
                .expect("styled break window");
            let content = q.query(&(lo..trailing.min(unit)), |l| l.all, cx).content;
            if !q.credited(self.parents[run].parent, content) {
                summary = summary.join(query(&prepared.kept, first..end, cx));
            }
            first = end;
            run += 1;
        }
        summary
    }
}

fn eligible(
    data: &ParagraphData,
    q: &QuirkIndex,
    start: usize,
    unit: usize,
    trailing: Option<usize>,
    cx: &mut LayoutContext,
) -> bool {
    #[cfg(test)]
    {
        cx.ruby_break_credit_queries += 1;
    }
    let range = start..unit + 1;
    let (_, lo) = q.ending_break(data, &range).expect("styled break window");
    if q.break_parent_edge(data, &range, unit) {
        return false;
    }
    let t = trailing.unwrap_or(unit);
    let content = q
        .query(&(lo..unit.min(t)), |l| l.all, cx)
        .join(q.query(&(lo.max(t)..unit), |l| l.kept, cx))
        .content;
    !q.credited(data.units[unit].parent_box, content)
}

fn update(tree: &mut [Summary], index: usize, summary: Summary) {
    let mut i = tree.len() / 2 + index;
    tree[i] = summary;
    while i > 1 {
        i /= 2;
        tree[i] = tree[i * 2].join(tree[i * 2 + 1]);
    }
}

fn query(tree: &[Summary], range: Range<usize>, _cx: &mut LayoutContext) -> Summary {
    let size = tree.len() / 2;
    let (mut lo, mut hi) = (size + range.start, size + range.end);
    let mut summary = Summary::default();
    while lo < hi {
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if lo & 1 != 0 {
            summary = summary.join(tree[lo]);
            lo += 1;
        }
        if hi & 1 != 0 {
            hi -= 1;
            summary = summary.join(tree[hi]);
        }
        lo /= 2;
        hi /= 2;
    }
    summary
}
