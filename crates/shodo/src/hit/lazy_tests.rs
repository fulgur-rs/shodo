use super::*;

mod fixture {
    use crate as shodo;
    include!("../../../../dev/bench/examples/support/ruby_base_caret_fixture.rs");
}

fn lines() -> Vec<Line> {
    let limits = Default::default();
    let fonts = crate::test_support::fonts::collection(&limits);
    let style = crate::style::ParagraphStyle::default();
    let mut builder = crate::ParagraphBuilder::new(&style, &limits);
    builder.push_text(
        crate::node::TextSource::Generated {
            node: crate::node::NodeId(1),
        },
        "ab cd",
    );
    let paragraph = builder
        .build(&mut crate::LayoutContext::new(), &fonts)
        .unwrap();
    paragraph.break_all(
        &mut crate::LayoutContext::new(),
        &Default::default(),
        100.0,
        &crate::AtomicSizes::EMPTY,
    )
}

#[test]
fn source_queries_do_not_construct_coordinate_trees() {
    let lines = lines();
    spatial::construction::reset();
    let layout = LineLayout::new(&lines);
    let start = TextPosition {
        line: 0,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    let end = TextPosition {
        line: 0,
        offset: 5,
        affinity: Affinity::Upstream,
    };
    assert_eq!(layout.caret(start).unwrap().position.offset, 0);
    assert_eq!(layout.caret(end).unwrap().position.offset, 5);
    assert!(!layout.selection_rects(start, end).is_empty());
    assert_eq!(
        layout
            .move_caret(start, CaretDirection::Forward, NavigationOrder::Logical)
            .unwrap()
            .offset,
        1
    );
    assert!(layout.hit_test(f32::NAN, 0.0).is_none());
    assert_eq!(
        spatial::construction::count(),
        0,
        "source queries must not pay for coordinate trees"
    );
}

#[test]
fn nested_annotation_bounds_do_not_construct_coordinate_trees() {
    let lines = fixture::nested(1, 16, false);
    spatial::construction::reset();
    let layout = LineLayout::new(&lines);
    assert!(layout.hit_bounds().is_some());
    let start = TextPosition {
        line: 0,
        offset: 0,
        affinity: Affinity::Downstream,
    };
    assert!(layout.caret(start).is_some());
    assert_eq!(
        spatial::construction::count(),
        0,
        "retained annotation bounds must be available without spatial trees"
    );
}

#[test]
fn coordinate_trees_are_constructed_once_and_reused() {
    let lines = lines();
    spatial::construction::reset();
    let layout = LineLayout::new(&lines);
    assert_eq!(spatial::construction::count(), 0);
    let hit = layout.hit_test(-100.0, 0.0).unwrap();
    assert_eq!(hit.position.offset, 0);
    assert!(!hit.inside);
    let builds = spatial::construction::count();
    assert!(
        builds > 0,
        "the first coordinate hit must initialize its spatial hierarchy"
    );
    for _ in 0..32 {
        assert_eq!(layout.hit_test(-100.0, 0.0), Some(hit));
    }
    assert_eq!(spatial::construction::count(), builds);
}

#[test]
fn annotation_bounds_remain_identical_after_coordinate_queries() {
    for depth in [0, 1, 3] {
        let lines = if depth == 0 {
            lines()
        } else {
            fixture::nested(depth, 16, false)
        };
        spatial::construction::reset();
        let layout = LineLayout::new(&lines);
        let before = layout.hit_bounds().unwrap();
        assert_eq!(spatial::construction::count(), 0);
        for inline in [-100.0, 0.0, 10.0, 100.0] {
            for block in [-100.0, 0.0, 10.0, 100.0] {
                let _ = layout.hit_test(inline, block);
            }
        }
        let after = layout.hit_bounds().unwrap();
        let bits = |r: LogicalRect| {
            [
                r.inline_start.to_bits(),
                r.inline_size.to_bits(),
                r.block_start.to_bits(),
                r.block_size.to_bits(),
            ]
        };
        assert_eq!(bits(before), bits(after));
    }
}

#[test]
fn concurrent_first_coordinate_queries_preserve_hits() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LineLayout<'static>>();
    let lines = lines();
    let layout = LineLayout::new(&lines);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| layout.hit_test(-100.0, 0.0).unwrap()))
            .collect();
        for worker in workers {
            let hit = worker.join().unwrap();
            assert_eq!(hit.position.offset, 0);
            assert!(!hit.inside);
        }
    });
}
