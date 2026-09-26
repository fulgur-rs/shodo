mod common;
use common::*;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::LineOptions;
use shodo::{AtomicSizes, LayoutContext, LineConstraint, LineResult};

fn text(b: &mut shodo::ParagraphBuilder, s: &str) {
    b.push_text(TextSource::Generated { node: NodeId(1) }, s);
}
fn float(b: &mut shodo::ParagraphBuilder, n: u64) {
    b.push_out_of_flow(NodeId(n), OutOfFlowKind::Float);
}

#[test]
fn reports_once_at_indent_without_double_counting_inset() {
    let p = build(&style(), |b| {
        float(b, 2);
        text(b, "a");
    });
    let mut cx = LayoutContext::new();
    let mut o = LineOptions::default();
    o.text_indent.length = 5.0;
    let mut c = LineConstraint::new(100.0);
    c.inline_start_offset = 10.0;
    let LineResult::FloatEncountered {
        node,
        line_start,
        inline_position,
        float_cursor,
    } = p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
    else {
        panic!("expected float")
    };
    assert_eq!(node, NodeId(2));
    assert_eq!(line_start, p.start_token());
    assert_eq!(inline_position, 5.0);
    c.floats_placed_through = Some(float_cursor);
    assert!(matches!(
        p.next_line(&mut cx, line_start, &o, &c, &AtomicSizes::EMPTY),
        LineResult::Line(_)
    ));
}

#[test]
fn word_float_is_not_reported_before_its_word_fits() {
    let p = build(&style(), |b| {
        text(b, "aa b");
        float(b, 2);
        text(b, "bbbb");
    });
    let mut cx = LayoutContext::new();
    let c = LineConstraint::new(60.0);
    let LineResult::Line(l) = p.next_line(
        &mut cx,
        p.start_token(),
        &LineOptions::default(),
        &c,
        &AtomicSizes::EMPTY,
    ) else {
        panic!()
    };
    assert_eq!(l.text_range(), 0..3);
    assert!(matches!(
        p.next_line(
            &mut cx,
            l.break_token(),
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY
        ),
        LineResult::FloatEncountered {
            node: NodeId(2),
            inline_position: 10.0,
            ..
        }
    ));
}

#[test]
fn withdrawal_is_ordered_and_cursor_survives_line_boundaries() {
    let p = build(&style(), |b| {
        float(b, 2);
        text(b, "a b ");
        float(b, 3);
        text(b, "c");
    });
    let mut cx = LayoutContext::new();
    let o = LineOptions::default();
    let mut c = LineConstraint::new(100.0);
    let LineResult::FloatEncountered {
        float_cursor: first,
        ..
    } = p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    c.floats_placed_through = Some(first);
    let LineResult::FloatEncountered {
        float_cursor: second,
        ..
    } = p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    assert_eq!(second.before(), Some(first));
    c.floats_placed_through = Some(second);
    c.available_inline_size = 20.0;
    let LineResult::Line(l) = p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY)
    else {
        panic!()
    };
    assert_eq!(l.displaced_floats(), &[(NodeId(3), second)]);
    c.floats_placed_through = second.before();
    c.available_inline_size = 80.0;
    assert!(matches!(
        p.next_line(&mut cx, p.start_token(), &o, &c, &AtomicSizes::EMPTY),
        LineResult::FloatEncountered {
            node: NodeId(3),
            ..
        }
    ));
    c.floats_placed_through = Some(second);
    c.available_inline_size = 20.0;
    assert!(matches!(
        p.next_line(&mut cx, l.break_token(), &o, &c, &AtomicSizes::EMPTY),
        LineResult::Line(_)
    ));
}

#[derive(Clone, Debug, Default, PartialEq)]
struct DriverState {
    cursor: Option<shodo::FloatCursor>,
    placed: Vec<(NodeId, f32)>,
    deferred: Vec<NodeId>,
    withdrawn: Vec<NodeId>,
}

#[test]
fn page_trial_restores_float_reports_and_replayed_line() {
    let p = build(&style(), |b| {
        float(b, 2);
        text(b, "a");
        float(b, 3);
        text(b, "b");
    });
    let mut cx = LayoutContext::new();
    let options = LineOptions::default();
    let mut state = DriverState::default();
    let mut token = p.start_token();
    let checkpoint = (token, state.clone());
    let mut reports = Vec::new();
    for pass in 0..2 {
        for _ in 0..2 {
            let mut c = LineConstraint::new(100.0);
            c.floats_placed_through = state.cursor;
            c.max_block_size = (pass == 0).then_some(5.0);
            let LineResult::FloatEncountered {
                node,
                line_start,
                inline_position,
                float_cursor,
            } = p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY)
            else {
                panic!("expected replayable float report")
            };
            reports.push((node, line_start, inline_position, float_cursor));
            state.cursor = Some(float_cursor);
            state.placed.push((node, 0.0));
        }
        let mut c = LineConstraint::new(100.0);
        c.floats_placed_through = state.cursor;
        if pass == 0 {
            c.max_block_size = Some(5.0);
            assert!(matches!(
                p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY),
                LineResult::BlockSizeExceeded { .. }
            ));
            (token, state) = checkpoint.clone();
        } else {
            let lookahead = (token, state.clone());
            let LineResult::Line(first) =
                p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY)
            else {
                panic!("expected line after unlimited retry")
            };
            token = first.break_token();
            assert!(matches!(
                p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY),
                LineResult::Done
            ));
            (token, state) = lookahead;
            c.floats_placed_through = state.cursor;
            let LineResult::Line(replayed) =
                p.next_line(&mut cx, token, &options, &c, &AtomicSizes::EMPTY)
            else {
                panic!("expected replayed line")
            };
            assert_eq!(first.text_range(), replayed.text_range());
            assert_eq!(first.block_size(), replayed.block_size());
            assert_eq!(first.inline_size(), replayed.inline_size());
            assert_eq!(first.break_token(), replayed.break_token());
            assert_eq!(first.displaced_floats(), replayed.displaced_floats());
        }
    }
    assert_eq!(reports[..2], reports[2..]);
}

fn drive(p: &shodo::Paragraph, width: f32, sizes: &[(NodeId, f32)]) -> Vec<shodo::Line> {
    let mut state = DriverState::default();
    let mut token = p.start_token();
    let mut lines = Vec::new();
    let mut cx = LayoutContext::new();
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls <= 3 * sizes.len() + 1, "retry bound: {state:?}");
        let inset: f32 = state.placed.iter().map(|(_, w)| w).sum();
        let mut c = LineConstraint::new(width - inset);
        c.inline_start_offset = inset;
        c.floats_placed_through = state.cursor;
        match p.next_line(
            &mut cx,
            token,
            &LineOptions::default(),
            &c,
            &AtomicSizes::EMPTY,
        ) {
            LineResult::FloatEncountered {
                node, float_cursor, ..
            } => {
                assert!(!state.placed.iter().any(|(n, _)| *n == node));
                let w = sizes.iter().find(|(n, _)| *n == node).unwrap().1;
                if state.withdrawn.contains(&node) || w > c.available_inline_size {
                    state.deferred.push(node);
                } else {
                    state.placed.push((node, w));
                }
                state.cursor = Some(float_cursor);
            }
            LineResult::Line(l) => {
                if let Some(&(node, cursor)) = l.displaced_floats().last() {
                    state.placed.retain(|(n, _)| *n != node);
                    state.deferred.retain(|n| *n != node);
                    state.withdrawn.push(node);
                    state.cursor = cursor.before();
                } else {
                    // Lookahead/page rollback restores the complete external
                    // state, not only the token or only the placed boxes.
                    let checkpoint = (token, state.clone());
                    let saved = state.clone();
                    state.placed.clear();
                    state.cursor = None;
                    state.deferred.clear();
                    state.withdrawn.clear();
                    let (restored_token, restored_state) = checkpoint;
                    assert_eq!(restored_token, token);
                    state = restored_state;
                    assert_eq!(state, saved);
                    token = l.break_token();
                    lines.push(l);
                    state.withdrawn.clear();
                    calls = 0;
                }
            }
            LineResult::Done => break,
            other => panic!("{other:?}"),
        }
    }
    lines
}

#[test]
fn tab_retry_defers_float_until_anchor_line_and_withdraws_one_at_a_time() {
    let mut root = style();
    root.root.white_space_collapse = shodo::style::WhiteSpaceCollapse::Preserve;
    root.root.tab_size = shodo::style::TabSize::Px(80.0);
    let p = build(&root, |b| {
        text(b, "\ta a a a a a a a a a a a a a ");
        float(b, 2);
        text(b, "b");
    });
    let lines = drive(&p, 120.0, &[(NodeId(2), 90.0)]);
    assert!(lines.len() >= 3);
    assert!(lines.iter().all(|l| l.displaced_floats().is_empty()));
    let p = build(&root, |b| {
        float(b, 2);
        text(b, "\ta a a ");
        float(b, 3);
        text(b, "b");
    });
    let lines = drive(&p, 160.0, &[(NodeId(2), 20.0), (NodeId(3), 50.0)]);
    assert!(lines.iter().all(|l| l.displaced_floats().is_empty()));
    assert_eq!(
        lines[0]
            .fragments()
            .filter(|f| matches!(f, shodo::Fragment::OutOfFlowAnchor(a) if a.node == NodeId(2)))
            .count(),
        1
    );
}
