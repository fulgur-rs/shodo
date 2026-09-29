use super::{LineLayout, TextPosition};
use crate::geometry::LogicalRect;
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
            let mut rects: Vec<_> = self.index[line]
                .segments
                .iter()
                .filter(|s| {
                    s.text.start < to
                        && s.text.end > from
                        && s.rect.inline_size > 0.0
                        && s.rect.block_size > 0.0
                })
                .map(|s| s.rect)
                .collect();
            rects.sort_by(|a, b| {
                a.block_start
                    .total_cmp(&b.block_start)
                    .then(a.block_size.total_cmp(&b.block_size))
                    .then(a.inline_start.total_cmp(&b.inline_start))
            });
            let mut merged: Vec<LogicalRect> = Vec::new();
            for rect in rects {
                if let Some(previous) = merged.last_mut()
                    && previous.block_start == rect.block_start
                    && previous.block_size == rect.block_size
                    && rect.inline_start <= previous.inline_start + previous.inline_size
                {
                    previous.inline_size = (rect.inline_start + rect.inline_size)
                        .max(previous.inline_start + previous.inline_size)
                        - previous.inline_start;
                } else {
                    merged.push(rect);
                }
            }
            merged.sort_by(|a, b| {
                a.inline_start
                    .total_cmp(&b.inline_start)
                    .then(a.inline_size.total_cmp(&b.inline_size))
                    .then(a.block_start.total_cmp(&b.block_start))
            });
            let mut final_rects: Vec<LogicalRect> = Vec::new();
            for rect in merged {
                if let Some(previous) = final_rects.last_mut()
                    && previous.inline_start == rect.inline_start
                    && previous.inline_size == rect.inline_size
                    && rect.block_start <= previous.block_start + previous.block_size
                {
                    previous.block_size = (rect.block_start + rect.block_size)
                        .max(previous.block_start + previous.block_size)
                        - previous.block_start;
                } else {
                    final_rects.push(rect);
                }
            }
            result.extend(final_rects);
        }
        result
    }
}
