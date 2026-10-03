#![cfg(feature = "allocation-counting")]

use shodo::hit::LineLayout;
use shodo::limits::Limits;
use shodo::mapping::TextOrigin;
use shodo::node::NodeId;
use shodo::ruby::RubyVisibility;
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[path = "../examples/support/ruby_hit_fixture.rs"]
mod fixture;

fn body_point(line: &shodo::Line) -> (f32, f32) {
    let layout = LineLayout::new(std::slice::from_ref(line));
    let caret = layout
        .caret(shodo::hit::TextPosition {
            line: 0,
            offset: line.text_range().start as u32,
            affinity: shodo::mapping::Affinity::Downstream,
        })
        .unwrap();
    (
        line.inline_size() / 2.0,
        caret.rect.block_start + caret.rect.block_size / 2.0,
    )
}

fn deepest_ruby_point(lines: &[shodo::Line], depth: usize) -> (f32, f32, Vec<u64>) {
    let mut line = &lines[0];
    let mut chain = Vec::with_capacity(depth);
    for _ in 0..depth {
        let annotation = line
            .ruby_annotations()
            .find(|annotation| annotation.visibility() == RubyVisibility::Visible)
            .unwrap();
        chain.push((
            annotation.transform(),
            line.block_offset(),
            annotation.node().unwrap().0,
        ));
        line = annotation.line();
    }

    let mut point = body_point(line);
    for (transform, parent_block_offset, _) in chain.iter().rev() {
        point = (
            transform.inline_inline * point.0
                + transform.inline_block * point.1
                + transform.inline_offset,
            transform.block_inline * point.0
                + transform.block_block * point.1
                + transform.block_offset
                + parent_block_offset,
        );
    }
    let path = chain.into_iter().map(|(_, _, node)| node).collect();
    (point.0, point.1, path)
}

#[test]
fn hit_test_skips_ruby_path_allocations_but_public_ruby_hit_keeps_parent_first_path() {
    for depth in [1, 2, 4] {
        let (lines, warnings) = fixture::fixture(depth, &Limits::default()).unwrap();
        assert_eq!(warnings, "[]/[]");
        assert_eq!(fixture::actual_depth(&lines), depth);
        fixture::assert_real_glyphs(&lines);

        let (inline, block, expected_path) = deepest_ruby_point(&lines, depth);
        let layout = LineLayout::new(&lines);
        let ruby_hit = layout.hit_test_ruby(inline, block).unwrap();
        assert_eq!(
            ruby_hit
                .path()
                .iter()
                .map(|a| a.node().unwrap().0)
                .collect::<Vec<_>>(),
            expected_path,
            "the public API keeps annotation paths parent-to-child"
        );
        assert_eq!(ruby_hit.path().len(), depth);
        assert_eq!(ruby_hit.parent_line(), 0);
        drop(ruby_hit);

        let hit = layout.hit_test(inline, block).unwrap();
        assert!(hit.inside);
        assert_eq!(hit.position.line, 0);
        assert!(
            matches!(
                hit.origin,
                Some(TextOrigin::Dom { node, offset })
                    if node == NodeId(100 + depth as u64) && offset <= 1
            ),
            "the main hit should still map to the outer base source: {:?}",
            hit.origin
        );

        let scope = ALLOCATOR.begin().unwrap();
        for _ in 0..32 {
            black_box(layout.hit_test(black_box(inline), black_box(block)));
        }
        let main_hit = scope.finish();
        assert_eq!(
            main_hit.calls, 0,
            "ordinary hit_test should allocate nothing at depth {depth}: {main_hit:?}"
        );
        assert_eq!(main_hit.allocated_bytes, 0);
        assert_eq!(main_hit.peak_extra_bytes, 0);

        let scope = ALLOCATOR.begin().unwrap();
        black_box(layout.hit_test_ruby(inline, block));
        let ruby_hit = scope.finish();
        assert!(
            ruby_hit.calls > 0,
            "the public RubyHit API still owns its returned path at depth {depth}"
        );
    }
}
