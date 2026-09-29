mod common;
#[test]
fn break_all_does_not_claim_unplaced_floats_are_displaced() {
    let p = build(&style(), |b| {
        b.push_text(TextSource::Generated { node: NodeId(1) }, "aa bb cc ")
            .push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
    });
    let lines = p.break_all(
        &mut LayoutContext::new(),
        &LineOptions::default(),
        20.0,
        &AtomicSizes::EMPTY,
    );
    assert!(lines.iter().all(|l| l.displaced_floats().is_empty()));
}
use common::*;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::LineOptions;
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult};

#[test]
fn break_all_matches_manual_at_all_widths() {
    let p = paragraph("aa bb cc");
    let o = LineOptions::default();
    for w in [0.0, 30.0, 100.0, f32::NAN] {
        let all = p.break_all(&mut LayoutContext::new(), &o, w, &AtomicSizes::EMPTY);
        let mut t = p.start_token();
        let mut ranges = Vec::new();
        loop {
            match p.next_line(
                &mut LayoutContext::new(),
                t,
                &o,
                &LineConstraint::new(w),
                &AtomicSizes::EMPTY,
            ) {
                LineResult::Line(l) => {
                    t = l.break_token();
                    ranges.push(l.text_range());
                }
                LineResult::Done => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            all.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
            ranges
        );
    }
}

#[test]
fn iterator_handles_retry_results_internally_and_yields_boundaries() {
    let atomics = AtomicSizes::EMPTY;
    let p = build(&style(), |b| {
        b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
            .push_text(TextSource::Generated { node: NodeId(1) }, "a")
            .push_block_in_inline(NodeId(3))
            .push_text(TextSource::Generated { node: NodeId(1) }, "b");
    });
    let mut cursor = None;
    let mut limit = Some(5.0);
    let mut seen_float = false;
    let mut seen_height = false;
    let mut offsets = Vec::new();
    let mut cx = LayoutContext::new();
    let o = LineOptions::default();
    let mut it = p.lines(
        &mut cx,
        p.start_token(),
        &o,
        |previous, offset| {
            if let Some(LineResult::FloatEncountered { float_cursor, .. }) = previous {
                cursor = Some(*float_cursor);
                seen_float = true;
            }
            if matches!(previous, Some(LineResult::BlockSizeExceeded { .. })) {
                limit = None;
                seen_height = true;
            }
            offsets.push(offset);
            let mut c = LineConstraint::new(100.0);
            c.floats_placed_through = cursor;
            c.max_block_size = limit;
            c.block_offset = offset;
            c
        },
        &atomics,
    );
    assert!(matches!(it.next(), Some(LineResult::Line(_))));
    assert!(matches!(it.next(), Some(LineResult::BlockInInline { .. })));
    assert!(matches!(it.next(), Some(LineResult::Line(_))));
    assert!(matches!(it.next(), Some(LineResult::Done)));
    assert!(it.next().is_none());
    drop(it);
    assert!(seen_float && seen_height);
    assert!(offsets.contains(&10.0));
    assert_eq!(
        p.break_all(&mut cx, &o, 100.0, &AtomicSizes::EMPTY).len(),
        2
    );
}

#[test]
fn invalid_token_yields_once_and_stops() {
    let atomics = AtomicSizes::EMPTY;
    let p = paragraph("a");
    let q = paragraph("b");
    let mut cx = LayoutContext::new();
    let o = LineOptions::default();
    let mut it = p.lines(
        &mut cx,
        q.start_token(),
        &o,
        |_, _| LineConstraint::new(100.0),
        &atomics,
    );
    assert!(matches!(it.next(), Some(LineResult::InvalidToken)));
    assert!(it.next().is_none());
}

#[test]
fn iterator_accepts_tall_atomic_after_unlimited_retry() {
    let root = style();
    let p = build(&root, |b| {
        b.push_atomic(NodeId(2), &root.root, shodo::node::InlineEdges::default());
    });
    let mut sizes = AtomicSizes::new();
    sizes.insert(
        NodeId(2),
        shodo::AtomicSize {
            inline_size: 10.0,
            block_size: 30.0,
            baseline: Some(20.0),
            ..shodo::AtomicSize::default()
        },
    );
    let mut cx = LayoutContext::new();
    let mut limit = Some(20.0);
    let mut it = p.lines(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        |prev, _| {
            if matches!(
                prev,
                Some(LineResult::BlockSizeExceeded {
                    needed_block_size: 30.0
                })
            ) {
                limit = None;
            }
            let mut c = LineConstraint::new(5.0);
            c.max_block_size = limit;
            c
        },
        &sizes,
    );
    let Some(LineResult::Line(l)) = it.next() else {
        panic!()
    };
    assert_eq!(l.block_size(), 30.0);
    assert!(matches!(it.next(), Some(LineResult::Done)));
}
