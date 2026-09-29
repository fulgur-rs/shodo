//! Carets and coordinate queries over accepted, finalized lines.
//!
//! Positions use UTF-8 byte offsets in the specified [`Line::text`] dataset.
//! `::first-line` can have a different dataset from following lines. Geometry
//! uses the same logical coordinates as glyphs, with line block offsets added.
//! Unsupported or absent GDEF ligature carets use proportional grapheme stops.
mod index;
mod navigation;
mod selection;
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
    index::LineIndex::new(0, line)
        .segments
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
        let ruby = lines
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
        Self {
            index,
            ruby,
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
        self.ruby
            .iter()
            .rev()
            .find_map(|entry| entry.hit(inline, block))
    }

    /// Outside the layout, clamp to a nearest stop with `inside=false`.
    /// Empty layouts and NaN inputs return `None`; infinities clamp to edges.
    pub fn hit_test(&self, inline: f32, block: f32) -> Option<HitResult> {
        if inline.is_nan() || block.is_nan() {
            return None;
        }
        for entry in self.ruby.iter().rev() {
            if entry.hit(inline, block).is_some() {
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
