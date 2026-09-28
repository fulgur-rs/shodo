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

/// Borrows finalized lines and builds their caret index once. No paragraph or
/// font bytes are copied, and queries do not reshape or rebuild glyph data.
pub struct LineLayout<'a> {
    lines: &'a [Line],
    index: Vec<index::LineIndex>,
    block_tree: spatial::Tree,
}
impl<'a> LineLayout<'a> {
    pub fn new(lines: &'a [Line]) -> Self {
        Self {
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
            index: lines
                .iter()
                .enumerate()
                .map(|(i, l)| index::LineIndex::new(i, l))
                .collect(),
        }
    }
    /// Snap an interior byte/grapheme/indivisible transform according to
    /// affinity. Out-of-range line indices and offsets return `None`.
    pub fn caret(&self, position: TextPosition) -> Option<Caret> {
        self.index.get(position.line)?.caret(position)
    }
    /// Outside the layout, clamp to a nearest stop with `inside=false`.
    /// Empty layouts and NaN inputs return `None`; infinities clamp to edges.
    pub fn hit_test(&self, inline: f32, block: f32) -> Option<HitResult> {
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
