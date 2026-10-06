use super::{LineLayout, TextPosition};
use crate::geometry::LogicalRect;

pub(super) fn inline_order(a: &LogicalRect, b: &LogicalRect) -> std::cmp::Ordering {
    a.block_start
        .total_cmp(&b.block_start)
        .then(a.block_size.total_cmp(&b.block_size))
        .then(a.inline_start.total_cmp(&b.inline_start))
}

fn merge_inline(previous: &mut LogicalRect, rect: &LogicalRect) -> bool {
    if previous.block_start == rect.block_start
        && previous.block_size == rect.block_size
        && rect.inline_start <= previous.inline_start + previous.inline_size
    {
        previous.inline_size = (rect.inline_start + rect.inline_size)
            .max(previous.inline_start + previous.inline_size)
            - previous.inline_start;
        true
    } else {
        false
    }
}

impl LineLayout<'_> {
    /// Select source order across the supplied line datasets. Reverse endpoints
    /// are normalized. Bidi gaps remain separate; only touching equal-height
    /// regions on the same line are merged.
    pub fn selection_rects(&self, start: TextPosition, end: TextPosition) -> Vec<LogicalRect> {
        let (Some(a), Some(b)) = (self.caret(start), self.caret(end)) else {
            return Vec::new();
        };
        let (mut a, mut b) = (a.position, b.position);
        if (a.line, a.offset) > (b.line, b.offset) {
            std::mem::swap(&mut a, &mut b);
        }
        if (a.line, a.offset) == (b.line, b.offset) {
            return Vec::new();
        }
        let mut result = Vec::new();
        for line in a.line..=b.line {
            let from = if line == a.line { a.offset } else { 0 };
            let to = if line == b.line { b.offset } else { u32::MAX };
            let mut rects = Vec::new();
            let index = &self.index[line];
            index.source.for_each(from, to, &index.segments, |s| {
                #[cfg(test)]
                tests::visit();
                if s.text.start < to
                    && s.text.end > from
                    && s.rect.inline_size > 0.0
                    && s.rect.block_size > 0.0
                    && (!index.selection_sorted
                        || !rects
                            .last_mut()
                            .is_some_and(|previous| merge_inline(previous, &s.rect)))
                {
                    rects.push(s.rect);
                }
            });
            if !index.selection_sorted {
                rects.sort_by(inline_order);
                rects.dedup_by(|rect, previous| merge_inline(previous, rect));
            }
            rects.sort_by(|a, b| {
                a.inline_start
                    .total_cmp(&b.inline_start)
                    .then(a.inline_size.total_cmp(&b.inline_size))
                    .then(a.block_start.total_cmp(&b.block_start))
            });
            rects.dedup_by(|rect, previous| {
                if previous.inline_start == rect.inline_start
                    && previous.inline_size == rect.inline_size
                    && rect.block_start <= previous.block_start + previous.block_size
                {
                    previous.block_size = (rect.block_start + rect.block_size)
                        .max(previous.block_start + previous.block_size)
                        - previous.block_start;
                    true
                } else {
                    false
                }
            });
            result.extend(rects);
        }
        result
    }
}

#[cfg(test)]
pub(super) mod tests;
