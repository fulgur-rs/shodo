//! Source-local queries over retained annotation lines, without reshaping.
use crate::geometry::LogicalRect;
use crate::hit::{Caret, HitResult, LineLayout};
use crate::{RubyAnnotationView, RubyTransform, RubyVisibility};

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
    bounds: Option<LogicalRect>,
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
        let child = LineLayout::new(std::slice::from_ref(annotation.line()));
        let bounds = child
            .hit_bounds()
            .map(|bounds| transformed_bounds(annotation.transform(), parent_block_offset, bounds));
        Some(Self {
            parent_line,
            annotation,
            parent_block_offset,
            child,
            base_stops,
            bounds,
        })
    }
    pub(crate) fn bounds(&self) -> Option<LogicalRect> {
        self.bounds
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

fn transformed_bounds(
    transform: RubyTransform,
    parent_block_offset: f32,
    bounds: LogicalRect,
) -> LogicalRect {
    let inline_end = bounds.inline_start + bounds.inline_size;
    let block_end = bounds.block_start + bounds.block_size;
    let corners = [
        (bounds.inline_start, bounds.block_start),
        (bounds.inline_start, block_end),
        (inline_end, bounds.block_start),
        (inline_end, block_end),
    ];
    let mut min_inline = f32::INFINITY;
    let mut max_inline = f32::NEG_INFINITY;
    let mut min_block = f32::INFINITY;
    let mut max_block = f32::NEG_INFINITY;
    for (inline, block) in corners {
        let parent_inline = transform.inline_inline * inline
            + transform.inline_block * block
            + transform.inline_offset;
        let parent_block = transform.block_inline * inline
            + transform.block_block * block
            + transform.block_offset
            + parent_block_offset;
        if !parent_inline.is_finite() || !parent_block.is_finite() {
            return invalid_bounds();
        }
        min_inline = min_inline.min(parent_inline);
        max_inline = max_inline.max(parent_inline);
        min_block = min_block.min(parent_block);
        max_block = max_block.max(parent_block);
    }
    let result = LogicalRect {
        inline_start: min_inline,
        inline_size: max_inline - min_inline,
        block_start: min_block,
        block_size: max_block - min_block,
    };
    if [
        result.inline_start,
        result.inline_size,
        result.block_start,
        result.block_size,
    ]
    .into_iter()
    .all(f32::is_finite)
    {
        result
    } else {
        invalid_bounds()
    }
}

fn invalid_bounds() -> LogicalRect {
    LogicalRect {
        inline_start: f32::NAN,
        inline_size: f32::NAN,
        block_start: f32::NAN,
        block_size: f32::NAN,
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
    fn nested_miss_outside_all_annotation_bounds_visits_none() {
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
        assert_eq!(visits, [(4, 0, 0), (8, 0, 0), (12, 0, 0), (16, 0, 0)]);
    }

    fn spread_siblings(lines: &mut [crate::Line], count: usize) {
        let seed = lines[0].ruby[0].clone();
        lines[0].ruby = (0..count)
            .map(|index| {
                let mut annotation = seed.clone();
                annotation.container = crate::node::NodeId(5000 + index as u64);
                annotation.transform.inline_offset = index as f32 * 100.0;
                annotation
            })
            .collect();
    }

    #[test]
    fn sparse_sibling_hit_miss_and_body_queries_visit_only_spatial_candidates() {
        for count in [16, 64, 256, 1024] {
            let (mut lines, _) = fixture::fixture(1, &Default::default()).unwrap();
            spread_siblings(&mut lines, count);
            let layout = LineLayout::new(&lines);
            let first = lines[0].ruby_annotations().next().unwrap();
            let mut point = parent_point(first, body_point(first.line()));
            point.1 += lines[0].block_offset();

            VISITS.with(|visits| visits.set(0));
            let hit = layout.hit_test_ruby(point.0, point.1).unwrap();
            assert_eq!(hit.annotation.container().0, 5000);
            assert_eq!(
                VISITS.with(|visits| visits.get()),
                1,
                "one separated annotation candidate should be tested for R={count}"
            );

            VISITS.with(|visits| visits.set(0));
            assert!(layout.hit_test_ruby(1_000_000.0, 1_000_000.0).is_none());
            assert_eq!(
                VISITS.with(|visits| visits.get()),
                0,
                "a distant miss should test no annotation for R={count}"
            );

            let body = body_point(&lines[0]);
            VISITS.with(|visits| visits.set(0));
            let result = layout.hit_test(body.0, body.1).unwrap();
            assert!(result.inside);
            assert_eq!(
                VISITS.with(|visits| visits.get()),
                0,
                "a body hit outside ruby bounds should test no annotation for R={count}"
            );
        }
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

    #[test]
    fn hidden_and_non_finite_annotation_bounds_keep_exact_hit_behavior() {
        for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut lines, _) = fixture::fixture(1, &Default::default()).unwrap();
            lines[0].ruby[0].transform.inline_offset = offset;
            let layout = LineLayout::new(&lines);
            assert!(layout.hit_test_ruby(10.0, 10.0).is_none());
            assert!(layout.hit_test_ruby(f32::INFINITY, 10.0).is_none());
            assert!(layout.hit_test_ruby(10.0, f32::NAN).is_none());
            assert!(layout.hit_test(f32::NAN, 10.0).is_none());
            assert!(!layout.hit_test(1_000_000.0, 1_000_000.0).unwrap().inside);
        }

        let (mut lines, _) = fixture::fixture(1, &Default::default()).unwrap();
        lines[0].ruby[0].visibility = RubyVisibility::Hidden;
        let layout = LineLayout::new(&lines);
        assert!(layout.hit_test_ruby(10.0, 10.0).is_none());
        assert!(!layout.hit_test(10.0, 10.0).unwrap().inside);
    }
}
