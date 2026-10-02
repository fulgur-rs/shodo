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
        #[cfg(test)]
        tests::VISITS.with(|visits| visits.set(visits.get() + 1));
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
        let hit = self.child.hit_test_body(x_local, y_local)?;
        hit.inside.then(|| RubyHit {
            annotation: self.annotation,
            hit,
            parent_line: self.parent_line,
            path: vec![self.annotation],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as shodo;
    mod fixture {
        use super::shodo;
        include!("../../../../dev/bench/examples/support/ruby_hit_fixture.rs");
    }
    thread_local! {
        pub(super) static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn nested_miss_visits_each_visible_annotation_once() {
        let mut visits = Vec::new();
        for depth in [4, 8, 12, 16] {
            let (lines, _) = fixture::fixture(depth, &Default::default()).unwrap();
            assert_eq!(fixture::actual_depth(&lines), depth);
            fixture::assert_real_glyphs(&lines);
            let layout = LineLayout::new(&lines);
            VISITS.with(|v| v.set(0));
            assert!(layout.hit_test_ruby(1_000_000.0, 1_000_000.0).is_none());
            let ruby_visits = VISITS.with(|v| v.get());
            VISITS.with(|v| v.set(0));
            assert!(!layout.hit_test(1_000_000.0, 1_000_000.0).unwrap().inside);
            visits.push((depth, ruby_visits, VISITS.with(|v| v.get())));
        }
        assert_eq!(visits, [(4, 4, 4), (8, 8, 8), (12, 12, 12), (16, 16, 16)]);
    }

    fn body_point(line: &crate::Line) -> (f32, f32) {
        let layout = LineLayout::new(std::slice::from_ref(line));
        let caret = layout
            .caret(crate::hit::TextPosition {
                line: 0,
                offset: 0,
                affinity: crate::mapping::Affinity::Downstream,
            })
            .unwrap();
        (
            line.inline_size() / 2.0,
            caret.rect.block_start + caret.rect.block_size / 2.0,
        )
    }

    fn parent_point(a: RubyAnnotationView<'_>, point: (f32, f32)) -> (f32, f32) {
        let t = a.transform();
        (
            t.inline_inline * point.0 + t.inline_block * point.1 + t.inline_offset,
            t.block_inline * point.0 + t.block_block * point.1 + t.block_offset,
        )
    }

    #[test]
    fn protruding_nested_hit_inverts_transforms_and_keeps_outer_base_source() {
        let (mut lines, _) = fixture::fixture(2, &Default::default()).unwrap();
        // Exercise retained hit geometry independently of ruby placement policy.
        lines[0].block_offset = 37.0;
        lines[0].ruby[0].transform = crate::RubyTransform {
            inline_inline: 0.0,
            inline_block: 2.0,
            block_inline: -3.0,
            block_block: 0.0,
            inline_offset: 50.0,
            block_offset: 60.0,
        };
        lines[0].ruby[0].line.ruby[0].transform.inline_offset = 200.0;
        lines[0].ruby[0].line.ruby[0].transform.block_offset = 300.0;
        let outer = lines[0].ruby_annotations().next().unwrap();
        let inner = outer.line().ruby_annotations().next().unwrap();
        let in_outer = parent_point(inner, body_point(inner.line()));
        let mut body = outer.line().clone();
        body.ruby.clear();
        let outer_body = LineLayout::new(std::slice::from_ref(&body));
        assert!(!outer_body.hit_test(in_outer.0, in_outer.1).unwrap().inside);
        let mut point = parent_point(outer, in_outer);
        point.1 += lines[0].block_offset();
        let layout = LineLayout::new(&lines);
        let hit = layout.hit_test_ruby(point.0, point.1).unwrap();
        assert_eq!(hit.path().len(), 2);
        assert_eq!(hit.annotation.node(), inner.node());
        assert_eq!(hit.parent_line(), 0);
        assert_eq!(
            hit.hit.origin,
            Some(crate::mapping::TextOrigin::Dom {
                node: crate::node::NodeId(1000),
                offset: 0,
            })
        );
        let main = layout.hit_test(point.0, point.1).unwrap();
        assert!(main.inside);
        assert_eq!(main.position.line, 0);
        assert!(matches!(
            main.origin,
            Some(crate::mapping::TextOrigin::Dom {
                node: crate::node::NodeId(102),
                ..
            })
        ));
    }

    #[test]
    fn overlapping_hits_prefer_nested_then_last_sibling() {
        let (mut lines, _) = fixture::fixture(2, &Default::default()).unwrap();
        let outer_line = &mut lines[0].ruby[0].line;
        let outer_point = body_point(outer_line);
        let inner_point = body_point(&outer_line.ruby[0].line);
        outer_line.ruby[0].transform.inline_offset = outer_point.0 - inner_point.0;
        outer_line.ruby[0].transform.block_offset = outer_point.1 - inner_point.1;
        let outer = lines[0].ruby_annotations().next().unwrap();
        let point = parent_point(outer, outer_point);
        let layout = LineLayout::new(&lines);
        let hit = layout.hit_test_ruby(point.0, point.1).unwrap();
        assert_eq!(hit.path().len(), 2, "nested wins over its parent's body");
        let mut sibling = lines[0].ruby[0].clone();
        sibling.container = crate::node::NodeId(9000);
        sibling.line.ruby[0].container = crate::node::NodeId(9001);
        lines[0].ruby.push(sibling);
        let layout = LineLayout::new(&lines);
        let hit = layout.hit_test_ruby(point.0, point.1).unwrap();
        assert_eq!(
            hit.path()
                .iter()
                .map(|a| a.container().0)
                .collect::<Vec<_>>(),
            [9000, 9001]
        );
    }
}
