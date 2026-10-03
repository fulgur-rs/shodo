//! Carets and coordinate queries over accepted, finalized lines.
//!
//! Positions use UTF-8 byte offsets in the specified [`Line::text`] dataset.
//! `::first-line` can have a different dataset from following lines. Geometry
//! uses the same logical coordinates as glyphs, with line block offsets added.
//! Unsupported or absent GDEF ligature carets use proportional grapheme stops.
mod index;
mod navigation;
mod selection;
mod source;
mod spatial;
use crate::Line;
use crate::geometry::LogicalRect;
use crate::mapping::{Affinity, TextOrigin};
pub use crate::ruby::hit::RubyHit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPosition {
    pub line: usize,
    pub offset: u32,
    pub affinity: Affinity,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitResult {
    pub position: TextPosition,
    pub origin: Option<TextOrigin>,
    pub inside: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    pub position: TextPosition,
    pub rect: LogicalRect,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretDirection {
    Backward,
    Forward,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationOrder {
    Logical,
    Visual,
}

/// Crate-private shared source geometry; paint must not duplicate GDEF logic.
pub(crate) fn paint_segments(line: &Line) -> Vec<(std::ops::Range<u32>, LogicalRect)> {
    index::paint_segments(line)
        .into_iter()
        .map(|s| (s.text, s.rect))
        .collect()
}

/// Borrows finalized lines and builds their caret index once. No paragraph or
/// font bytes are copied, and queries do not reshape or rebuild glyph data.
pub struct LineLayout<'a> {
    lines: &'a [Line],
    index: Vec<index::LineIndex>,
    block_tree: spatial::Tree,
    ruby: Vec<crate::ruby::hit::AnnotationIndex<'a>>,
    ruby_spatial: spatial::Tree,
    ruby_unindexed: Vec<usize>,
}
impl<'a> LineLayout<'a> {
    /// Accessibility shares this finalized index rather than reinterpreting
    /// grapheme cuts or rebuilding source geometry for each character.
    pub(crate) fn accepted_stops(&self, line: usize) -> &[Caret] {
        &self.index[line].stops
    }
    pub(crate) fn accepted_segments(
        &self,
        line: usize,
    ) -> impl Iterator<Item = (&std::ops::Range<u32>, LogicalRect)> {
        self.index[line].segments.iter().map(|s| (&s.text, s.rect))
    }
    pub fn new(lines: &'a [Line]) -> Self {
        let index: Vec<_> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| index::LineIndex::new(i, line))
            .collect();
        let ruby: Vec<_> = lines
            .iter()
            .enumerate()
            .flat_map(|(parent, line)| {
                let index = &index[parent];
                line.ruby_annotations().filter_map(move |a| {
                    let range = a.base_text_range();
                    let begin = index
                        .stops
                        .partition_point(|c| (c.position.offset as usize) < range.start);
                    let end = index
                        .stops
                        .partition_point(|c| (c.position.offset as usize) <= range.end);
                    crate::ruby::hit::AnnotationIndex::new(
                        parent,
                        line.block_offset(),
                        a,
                        index.stops[begin..end].to_vec(),
                    )
                })
            })
            .collect();
        let mut ruby_rects = Vec::with_capacity(ruby.len());
        let mut ruby_unindexed = Vec::new();
        for (i, entry) in ruby.iter().enumerate() {
            match entry.bounds() {
                Some(rect) if bounds_are_finite(rect) => ruby_rects.push((i, rect)),
                Some(_) => ruby_unindexed.push(i),
                None => {}
            }
        }
        let ruby_spatial = spatial::Tree::new(ruby_rects.into_iter(), false);
        Self {
            index,
            ruby,
            ruby_spatial,
            ruby_unindexed,
            lines,
            block_tree: spatial::Tree::new(
                lines.iter().enumerate().map(|(i, l)| {
                    (
                        i,
                        LogicalRect {
                            inline_start: 0.0,
                            inline_size: 0.0,
                            block_start: l.block_offset(),
                            block_size: l.block_size(),
                        },
                    )
                }),
                true,
            ),
        }
    }
    /// Snap an interior byte/grapheme/indivisible transform according to
    /// affinity. Out-of-range line indices and offsets return `None`.
    pub fn caret(&self, position: TextPosition) -> Option<Caret> {
        self.index.get(position.line)?.caret(position)
    }
    /// Hit a visible retained lane, descending through nested annotations.
    /// The result's offsets are local to its annotation, never the main text.
    pub fn hit_test_ruby(&self, inline: f32, block: f32) -> Option<RubyHit<'a>> {
        if !inline.is_finite() || !block.is_finite() {
            return None;
        }
        self.ruby_hit(inline, block).map(|(_, hit)| hit)
    }

    /// Outside the layout, clamp to a nearest stop with `inside=false`.
    /// Empty layouts and NaN inputs return `None`; infinities clamp to edges.
    pub fn hit_test(&self, inline: f32, block: f32) -> Option<HitResult> {
        if inline.is_nan() || block.is_nan() {
            return None;
        }
        if let Some((entry_index, _)) = self.ruby_hit(inline, block) {
            let entry = &self.ruby[entry_index];
            let stop = entry.base_caret(inline, block)?;
            return Some(HitResult {
                position: stop.position,
                origin: self.lines[entry.parent_line]
                    .offset_mapping()
                    .and_then(|mapping| {
                        mapping.text_to_dom(stop.position.offset, stop.position.affinity)
                    }),
                inside: true,
            });
        }
        self.hit_test_body(inline, block)
    }

    fn ruby_hit(&self, inline: f32, block: f32) -> Option<(usize, RubyHit<'a>)> {
        let indexed = self
            .ruby_spatial
            .best_containing_by(inline, block, |i| self.ruby[i].hit(inline, block));
        for &i in self.ruby_unindexed.iter().rev() {
            if let Some(hit) = self.ruby[i].hit(inline, block) {
                if indexed.as_ref().is_some_and(|(best, _)| *best > i) {
                    return indexed;
                }
                return Some((i, hit));
            }
        }
        indexed
    }

    pub(crate) fn hit_bounds(&self) -> Option<LogicalRect> {
        let body = self
            .index
            .iter()
            .filter_map(index::LineIndex::hit_bounds)
            .reduce(union_bounds);
        let ruby = if self.ruby_unindexed.is_empty() {
            self.ruby_spatial.bounds()
        } else {
            Some(invalid_bounds())
        };
        match (body, ruby) {
            (Some(a), Some(b)) => Some(union_bounds(a, b)),
            (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
            (None, None) => None,
        }
    }

    /// Query only main text after this layout's annotations have been searched.
    pub(crate) fn hit_test_body(&self, inline: f32, block: f32) -> Option<HitResult> {
        // An annotation's inverse transform can produce NaN from infinities.
        if inline.is_nan() || block.is_nan() {
            return None;
        }
        let line = self.block_tree.nearest_y(block)?;
        let index = &self.index[line];
        let stop = index.hit(inline, block)?;
        Some(HitResult {
            position: stop.position,
            origin: self.lines[line]
                .offset_mapping()
                .and_then(|m| m.text_to_dom(stop.position.offset, stop.position.affinity)),
            inside: index.inside(inline, block),
        })
    }
}

fn bounds_are_finite(rect: LogicalRect) -> bool {
    [
        rect.inline_start,
        rect.inline_start + rect.inline_size,
        rect.inline_size,
        rect.block_start,
        rect.block_start + rect.block_size,
        rect.block_size,
    ]
    .into_iter()
    .all(f32::is_finite)
}

fn invalid_bounds() -> LogicalRect {
    LogicalRect {
        inline_start: f32::NAN,
        inline_size: f32::NAN,
        block_start: f32::NAN,
        block_size: f32::NAN,
    }
}

fn union_bounds(a: LogicalRect, b: LogicalRect) -> LogicalRect {
    if !bounds_are_finite(a) || !bounds_are_finite(b) {
        return invalid_bounds();
    }
    let a_right = a.inline_start + a.inline_size;
    let b_right = b.inline_start + b.inline_size;
    let a_bottom = a.block_start + a.block_size;
    let b_bottom = b.block_start + b.block_size;
    let inline_start = a.inline_start.min(b.inline_start);
    let block_start = a.block_start.min(b.block_start);
    let inline_end = a_right.max(b_right);
    let block_end = a_bottom.max(b_bottom);
    let bounds = LogicalRect {
        inline_start,
        inline_size: inline_end - inline_start,
        block_start,
        block_size: block_end - block_start,
    };
    if bounds_are_finite(bounds) {
        bounds
    } else {
        invalid_bounds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_hits_and_navigation_do_not_rebuild_glyph_indexes() {
        let limits = Default::default();
        let fonts = crate::font::FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let style = crate::style::ParagraphStyle::default();
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"a".repeat(10_000),
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        let lines = p.break_all(
            &mut crate::LayoutContext::new(),
            &Default::default(),
            1_000_000.0,
            &crate::AtomicSizes::EMPTY,
        );
        let layout = LineLayout::new(&lines);
        let before = p
            .data
            .cluster_queries
            .load(std::sync::atomic::Ordering::Relaxed);
        for _ in 0..1000 {
            let hit = layout.hit_test(1.0, 8.0).unwrap();
            let next = layout
                .move_caret(
                    hit.position,
                    CaretDirection::Forward,
                    NavigationOrder::Logical,
                )
                .unwrap();
            assert_eq!(next.offset, 1);
            let next = layout
                .move_caret(
                    hit.position,
                    CaretDirection::Forward,
                    NavigationOrder::Visual,
                )
                .unwrap();
            assert_eq!(next.offset, 1);
        }
        assert_eq!(
            p.data
                .cluster_queries
                .load(std::sync::atomic::Ordering::Relaxed),
            before
        );
    }
}
