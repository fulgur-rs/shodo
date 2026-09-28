//! Source-local queries over retained annotation lines, without reshaping.
use crate::hit::{Caret, HitResult, LineLayout};
use crate::{RubyAnnotationView, RubyVisibility};

/// A dedicated annotation hit. The position/source belong to `annotation.line()`.
/// `path()` lists the retained transforms from the caller's line to that lane.
#[derive(Clone, Debug)]
pub struct RubyHit<'a> {
    pub annotation: RubyAnnotationView<'a>,
    pub hit: HitResult,
    parent_line: usize,
    path: Vec<RubyAnnotationView<'a>>,
}
impl<'a> RubyHit<'a> {
    pub fn parent_line(&self) -> usize {
        self.parent_line
    }
    pub fn path(&self) -> &[RubyAnnotationView<'a>] {
        &self.path
    }
}

pub(crate) struct AnnotationIndex<'a> {
    pub(crate) parent_line: usize,
    annotation: RubyAnnotationView<'a>,
    parent_block_offset: f32,
    child: LineLayout<'a>,
    base_stops: Vec<Caret>,
}
impl<'a> AnnotationIndex<'a> {
    pub(crate) fn new(
        parent_line: usize,
        parent_block_offset: f32,
        annotation: RubyAnnotationView<'a>,
        base_stops: Vec<Caret>,
    ) -> Option<Self> {
        if annotation.visibility() != RubyVisibility::Visible {
            return None;
        }
        Some(Self {
            parent_line,
            annotation,
            parent_block_offset,
            child: LineLayout::new(std::slice::from_ref(annotation.line())),
            base_stops,
        })
    }
    pub(crate) fn base_caret(&self, inline: f32, block: f32) -> Option<Caret> {
        self.base_stops
            .iter()
            .min_by(|a, b| {
                let distance = |caret: &Caret| {
                    if caret.rect.block_size == 0.0 && caret.rect.inline_size > 0.0 {
                        (block - caret.rect.block_start).abs()
                    } else {
                        (inline - caret.rect.inline_start).abs()
                    }
                };
                distance(a).total_cmp(&distance(b)).then(
                    (a.position.affinity == crate::mapping::Affinity::Upstream)
                        .cmp(&(b.position.affinity == crate::mapping::Affinity::Upstream)),
                )
            })
            .copied()
    }

    pub(crate) fn hit(&self, inline: f32, block: f32) -> Option<RubyHit<'a>> {
        let t = self.annotation.transform();
        let determinant = t.inline_inline * t.block_block - t.inline_block * t.block_inline;
        if determinant == 0.0 {
            return None;
        }
        let x = inline - t.inline_offset;
        let y = block - self.parent_block_offset - t.block_offset;
        let x_local = (t.block_block * x - t.inline_block * y) / determinant;
        let y_local = (t.inline_inline * y - t.block_inline * x) / determinant;
        if let Some(mut nested) = self.child.hit_test_ruby(x_local, y_local) {
            nested.parent_line = self.parent_line;
            nested.path.insert(0, self.annotation);
            return Some(nested);
        }
        let hit = self.child.hit_test(x_local, y_local)?;
        hit.inside.then(|| RubyHit {
            annotation: self.annotation,
            hit,
            parent_line: self.parent_line,
            path: vec![self.annotation],
        })
    }
}
