use png_example::flow::*;
use shodo::font::{FontCollection, FontOptions};
use shodo::limits::Limits;
use shodo::node::{NodeId, OutOfFlowKind, TextSource};
use shodo::style::{LineOptions, ParagraphStyle, TabSize, WhiteSpaceCollapse};
use shodo::{AtomicSizes, LayoutContext, Line};

fn paragraph(parts: &[Part], tab: bool) -> shodo::Paragraph {
    let mut style = ParagraphStyle::default();
    style.root.font_size = 10.0;
    if tab {
        style.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        style.root.tab_size = TabSize::Px(80.0);
    }
    let mut builder = shodo::ParagraphBuilder::new(&style, &Limits::default());
    for part in parts {
        match part {
            Part::Text(text) => {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
            }
            Part::Float(id) => {
                builder.push_out_of_flow(NodeId(*id), OutOfFlowKind::Float);
            }
            Part::Atomic(id) => {
                builder.push_atomic(NodeId(*id), &style.root, Default::default());
            }
            Part::Block(id) => {
                builder.push_block_in_inline(NodeId(*id));
            }
        }
    }
    builder
        .build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap()
}
enum Part<'a> {
    Text(&'a str),
    Float(u64),
    Atomic(u64),
    Block(u64),
}
fn spec(id: u64, side: Side, clear: Clear, width: f32, height: f32) -> FloatSpec {
    FloatSpec {
        node: NodeId(id),
        side,
        clear,
        width,
        height,
    }
}
fn line(trial: Trial) -> (Line, Checkpoint, Vec<Event>, usize) {
    let Outcome::Line(line) = trial.outcome else {
        panic!("{:?}", trial.outcome)
    };
    (line, trial.state, trial.events, trial.calls)
}
#[test]
fn opposing_head_floats_use_real_geometry_and_retry_same_token() {
    let p = paragraph(
        &[Part::Float(2), Part::Float(3), Part::Text("aa bb")],
        false,
    );
    let driver = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Right, Clear::None, 30.0, 20.0),
    ])
    .unwrap();
    let start = Checkpoint::new(&p, 100.0).unwrap();
    let (l, end, events, calls) = line(
        driver
            .trial(
                &p,
                &mut LayoutContext::new(),
                &start,
                &LineOptions::default(),
                &AtomicSizes::EMPTY,
                None,
            )
            .unwrap(),
    );
    assert_eq!(p.text(), "\u{fffc}\u{fffc}aa bb");
    assert_eq!(l.text_range(), 0..11);
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(end.placed()[0].rect.inline_start, 0.0);
    assert_eq!(end.placed()[1].rect.inline_start, 70.0);
    assert_eq!(end.slot(10.0), (20.0, 50.0));
    assert_eq!(calls, 3);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Reported { .. }))
            .count(),
        2
    );
    assert_eq!(start.token(), p.start_token());
    assert!(start.placed().is_empty());
}
fn trial(driver: &Driver, p: &shodo::Paragraph, state: &Checkpoint) -> Trial {
    driver
        .trial(
            p,
            &mut LayoutContext::new(),
            state,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None,
        )
        .unwrap()
}
#[test]
fn clear_all_sides_preserves_real_taffy_clearance() {
    for side in [Side::Left, Side::Right] {
        for (clear, y) in [
            (Clear::None, 0.0),
            (Clear::Left, 30.0),
            (Clear::Right, 50.0),
            (Clear::Both, 50.0),
        ] {
            let p = paragraph(
                &[
                    Part::Float(2),
                    Part::Float(3),
                    Part::Float(4),
                    Part::Text("a"),
                ],
                false,
            );
            let d = Driver::new(vec![
                spec(2, Side::Left, Clear::None, 20.0, 30.0),
                spec(3, Side::Right, Clear::None, 20.0, 50.0),
                spec(4, side, clear, 10.0, 10.0),
            ])
            .unwrap();
            let (_, s, _, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
            let third = s.placed().iter().find(|p| p.node == NodeId(4)).unwrap();
            let x = match (side, clear) {
                (Side::Left, Clear::None) => 20.0,
                (Side::Right, Clear::None | Clear::Left) => 70.0,
                (Side::Right, _) => 90.0,
                (Side::Left, _) => 0.0,
            };
            assert_eq!(
                (third.rect.inline_start, third.rect.block_start),
                (x, y),
                "{side:?}/{clear:?}: {:?}",
                s.placed()
            );
            assert_eq!(calls, 4);
        }
    }
}
#[test]
fn midline_remaining_width_and_pending_order() {
    let p = paragraph(
        &[
            Part::Text("aa "),
            Part::Float(2),
            Part::Float(3),
            Part::Text("bb"),
        ],
        false,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 80.0, 20.0),
        spec(3, Side::Left, Clear::None, 10.0, 10.0),
    ])
    .unwrap();
    let (l, s, events, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    assert_eq!(l.text_range(), 0..11);
    assert_eq!(
        s.placed()
            .iter()
            .map(|p| (p.node, p.rect.inline_start, p.rect.block_start))
            .collect::<Vec<_>>(),
        [(NodeId(2), 0.0, 10.0), (NodeId(3), 80.0, 10.0)]
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|e| if let Event::Deferred { node } = e {
                Some(*node)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        [NodeId(2), NodeId(3)]
    );
    assert_eq!(calls, 3);
}
#[test]
fn tab_withdraws_latest_only_keeps_first_float_and_forces_rereport_defer() {
    let p = paragraph(
        &[
            Part::Float(2),
            Part::Text("a a \ta "),
            Part::Float(3),
            Part::Text("b"),
        ],
        true,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Left, Clear::None, 50.0, 30.0),
    ])
    .unwrap();
    let (l, s, events, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 160.0).unwrap()));
    assert_eq!(l.text_range(), 0..14);
    assert_eq!(
        s.placed()
            .iter()
            .map(|p| (p.node, p.rect.block_start))
            .collect::<Vec<_>>(),
        [(NodeId(2), 0.0), (NodeId(3), 10.0)]
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|e| if let Event::Withdrawn { node } = e {
                Some(*node)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        [NodeId(3)]
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                Event::Reported {
                    node: NodeId(3),
                    ..
                }
            ))
            .count(),
        2
    );
    assert!(s.withdrawn_nodes().is_empty());
    assert_eq!(calls, 5);
    assert!(calls <= 3 * 2 + 1);
}
#[test]
fn indivisible_word_reports_float_on_its_anchor_line() {
    let p = paragraph(
        &[Part::Text("aa b"), Part::Float(2), Part::Text("bbbb")],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let (first, s, events, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(first.text_range(), 0..3);
    assert_eq!(calls, 1);
    assert!(!events.iter().any(|e| matches!(e, Event::Reported { .. })));
    let (second, s, events, calls) = line(trial(&d, &p, &s));
    assert_eq!(second.text_range(), 3..11);
    assert_eq!(second.block_offset(), 10.0);
    assert_eq!(s.placed()[0].rect.block_start, 20.0);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Reported { .. }))
            .count(),
        1
    );
    assert_eq!(calls, 3);
}
fn same(a: &Checkpoint, b: &Checkpoint) {
    assert_eq!(a.token(), b.token());
    assert_eq!(a.cursor(), b.cursor());
    assert_eq!(a.block(), b.block());
    assert_eq!(a.fragment(), b.fragment());
    assert_eq!(a.pending_nodes(), b.pending_nodes());
    assert_eq!(a.withdrawn_nodes(), b.withdrawn_nodes());
    assert_eq!(
        a.placed()
            .iter()
            .map(|p| (p.node, p.rect))
            .collect::<Vec<_>>(),
        b.placed()
            .iter()
            .map(|p| (p.node, p.rect))
            .collect::<Vec<_>>()
    );
    for h in [0.0, 10.0, 30.0, 50.0] {
        assert_eq!(a.slot(h), b.slot(h));
    }
    for clear in [Clear::None, Clear::Left, Clear::Right, Clear::Both] {
        assert_eq!(a.clearance(clear), b.clearance(clear));
    }
}
#[test]
fn height_rejection_restores_every_field_and_replays_float_reports() {
    let p = paragraph(&[Part::Float(2), Part::Text("a")], false);
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 20.0, 30.0)]).unwrap();
    let start = Checkpoint::new(&p, 100.0).unwrap();
    let failed = d
        .trial(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            Some(5.0),
        )
        .unwrap();
    assert!(matches!(
        failed.outcome,
        Outcome::HeightRejected { needed: 10.0 }
    ));
    assert_eq!(failed.calls, 2);
    assert!(
        failed
            .events
            .iter()
            .any(|e| matches!(e, Event::Placed { .. }))
    );
    same(&start, &failed.state);
    let good = trial(&d, &p, &failed.state);
    assert_eq!(failed.events[0], good.events[0]);
    assert_eq!(good.state.placed()[0].rect.block_start, 0.0);
}
#[test]
fn nonempty_pending_and_withdrawn_checkpoint_can_be_rejected_then_resumed() {
    let p = paragraph(
        &[
            Part::Float(2),
            Part::Text("a a \ta "),
            Part::Float(3),
            Part::Text("b"),
        ],
        true,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Left, Clear::None, 50.0, 30.0),
    ])
    .unwrap();
    let full = trial(&d, &p, &Checkpoint::new(&p, 160.0).unwrap());
    let saved = full
        .attempts
        .iter()
        .find(|s| !s.withdrawn_nodes().is_empty() && !s.pending_nodes().is_empty())
        .unwrap()
        .clone();
    assert_eq!(saved.pending_nodes(), [NodeId(3)]);
    assert_eq!(saved.withdrawn_nodes(), [NodeId(3)]);
    let rejected = d
        .trial(
            &p,
            &mut LayoutContext::new(),
            &saved,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            Some(5.0),
        )
        .unwrap();
    assert!(matches!(rejected.outcome, Outcome::HeightRejected { .. }));
    same(&saved, &rejected.state);
    let resumed = trial(&d, &p, &rejected.state);
    same(&full.state, &resumed.state);
    assert_eq!(line(full).0.text_range(), line(resumed).0.text_range());
}
#[test]
fn preview_prefix_commit_and_widow_orphan_discard_restore_whole_tuple() {
    let p = paragraph(
        &[
            Part::Text("aa bb "),
            Part::Float(2),
            Part::Text("cc dd ee ff"),
        ],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 20.0, 40.0)]).unwrap();
    let start = Checkpoint::new(&p, 60.0).unwrap();
    let preview = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    assert_eq!(preview.lines.len(), 5);
    assert_eq!(
        preview
            .lines
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>(),
        [0..6, 6..12, 12..15, 15..18, 18..20]
    );
    // A page can hold 4 lines; widows=2 leaves only one on the next page.
    // Commit 3 rather than 4, preserving orphans=2 on the current page.
    let keep = widow_prefix(4, preview.lines.len(), 2, 2, false).unwrap();
    assert_eq!(keep, 3);
    let selected = preview.select(keep).unwrap();
    assert_eq!(selected.token(), preview.lines[2].break_token());
    let rest = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &selected,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    assert_eq!(
        rest.lines
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>(),
        preview.lines[3..]
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>()
    );
    // Only 1 line fits in occupied page, fewer than orphans=2: discard all.
    assert_eq!(
        widow_prefix(1, preview.lines.len(), 2, 2, false).unwrap(),
        0
    );
    same(&start, &preview.select(0).unwrap());
    let again = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &preview.select(0).unwrap(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    assert_eq!(again.events, preview.events);
    // A fresh page that cannot satisfy the limits accepts progress anyway.
    assert_eq!(widow_prefix(1, 5, 2, 2, true).unwrap(), 1);
    assert!(preview.select(6).is_err());
}
#[test]
fn width_changing_page_carries_remaining_right_float_and_cursor() {
    let p = paragraph(&[Part::Float(2), Part::Text("aa bb cc dd ee ff")], false);
    let d = Driver::new(vec![spec(2, Side::Right, Clear::None, 20.0, 50.0)]).unwrap();
    let (first, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 80.0).unwrap()));
    assert_eq!(first.text_range(), 0..9);
    let page = s.next_fragment(10.0, 100.0).unwrap();
    assert_eq!(page.block(), 0.0);
    assert_eq!(page.fragment(), 1);
    assert_eq!(page.cursor(), s.cursor());
    assert_eq!(
        page.placed()[0].rect,
        Rect {
            inline_start: 80.0,
            block_start: 0.0,
            inline_size: 20.0,
            block_size: 40.0
        }
    );
    let (l, _, events, _) = line(trial(&d, &p, &page));
    assert_eq!(l.text_range(), 9..18);
    assert!(!events.iter().any(|e| matches!(e, Event::Reported { .. })));
}
#[test]
fn paragraph_handoff_resets_cursor_but_retains_previous_bfc() {
    let first = paragraph(&[Part::Float(2), Part::Text("a")], false);
    let second = paragraph(&[Part::Float(3), Part::Text("bb")], false);
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 50.0),
        spec(3, Side::Right, Clear::None, 30.0, 30.0),
    ])
    .unwrap();
    let (_, s, _, _) = line(trial(&d, &first, &Checkpoint::new(&first, 100.0).unwrap()));
    let handoff = s
        .begin_paragraph(
            &first,
            &second,
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        )
        .unwrap();
    assert_eq!(handoff.cursor(), None);
    assert_eq!(handoff.slot(10.0), (20.0, 80.0));
    let (l, s, events, _) = line(trial(&d, &second, &handoff));
    assert_eq!(l.block_offset(), 10.0);
    assert_eq!(s.placed().len(), 2);
    assert_eq!(s.placed()[1].rect.inline_start, 70.0);
    assert_eq!(s.placed()[1].rect.block_start, 10.0);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Reported { .. }))
            .count(),
        1
    );
    assert!(
        d.trial(
            &first,
            &mut LayoutContext::new(),
            &handoff,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None
        )
        .is_err()
    );
}
#[test]
fn taller_line_checks_the_entire_float_band_then_moves_to_next_bottom() {
    let p = paragraph(&[Part::Float(2), Part::Float(3), Part::Atomic(9)], false);
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Right, Clear::Left, 20.0, 30.0),
    ])
    .unwrap();
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(9),
        shodo::AtomicSize {
            inline_size: 70.0,
            block_size: 40.0,
            baseline: Some(8.0),
            ..Default::default()
        },
    );
    let t = d
        .trial(
            &p,
            &mut LayoutContext::new(),
            &Checkpoint::new(&p, 100.0).unwrap(),
            &LineOptions::default(),
            &atomics,
            None,
        )
        .unwrap();
    assert!(t.events.contains(&Event::BandRetry { height: 40.0 }));
    assert!(t.events.contains(&Event::PositionRetry { block: 30.0 }));
    let (l, s, _, calls) = line(t);
    assert_eq!((l.block_offset(), l.block_size()), (30.0, 40.0));
    assert_eq!(s.block(), 70.0);
    assert_eq!(calls, 5);
}
#[test]
fn tab_long_span_float_never_precedes_the_line_containing_its_anchor() {
    let p = paragraph(
        &[
            Part::Text("\ta a a a a a a a a a a a a a "),
            Part::Float(2),
            Part::Text("b"),
        ],
        true,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 90.0, 50.0)]).unwrap();
    let preview = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &Checkpoint::new(&p, 120.0).unwrap(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    assert_eq!(
        preview
            .lines
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>(),
        [0..5, 5..17, 17..29, 29..33]
    );
    assert_eq!(
        preview.select(4).unwrap().placed()[0].rect.block_start,
        30.0
    );
    assert_eq!(preview.lines[3].block_offset(), 30.0);
    assert!(preview.select(3).unwrap().placed().is_empty());
}
#[test]
fn oversized_float_and_line_have_explicit_finite_progress() {
    let p = paragraph(&[Part::Float(2), Part::Text("a b")], false);
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 120.0, 30.0)]).unwrap();
    let (first, s, _, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    assert_eq!(first.text_range(), 0..6);
    assert_eq!(s.placed()[0].rect.inline_size, 120.0);
    assert_eq!(s.placed()[0].rect.block_start, 10.0);
    assert_eq!(calls, 2);
    // A following paragraph skips the fully obstructed band in one retry.
    let next = paragraph(&[Part::Text("cc")], false);
    let handoff = s
        .begin_paragraph(
            &p,
            &next,
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        )
        .unwrap();
    let (l, _, events, calls) = line(trial(&d, &next, &handoff));
    assert_eq!(l.block_offset(), 40.0);
    assert!(events.contains(&Event::PositionRetry { block: 40.0 }));
    assert_eq!(calls, 2, "{events:?}");
    // A page shorter than one line rejects the bounded trial. Explicitly
    // remove the limit once on a fresh page, then accept the advancing token.
    let empty = Driver::new(vec![]).unwrap();
    let start = Checkpoint::new(&next, 100.0).unwrap();
    let failed = empty
        .trial(
            &next,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            Some(2.0),
        )
        .unwrap();
    assert!(matches!(
        failed.outcome,
        Outcome::HeightRejected { needed: 10.0 }
    ));
    let (l, accepted, _, calls) = line(trial(&empty, &next, &failed.state));
    assert_eq!(l.block_size(), 10.0);
    assert_ne!(accepted.token(), start.token());
    assert_eq!(calls, 1);
}
#[test]
fn block_boundary_caller_handoff_keeps_float_exclusions_and_uses_measured_height() {
    let p = paragraph(
        &[
            Part::Float(2),
            Part::Text("a "),
            Part::Block(9),
            Part::Text("b"),
        ],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 20.0, 50.0)]).unwrap();
    let (_, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    let boundary = trial(&d, &p, &s);
    assert!(matches!(
        boundary.outcome,
        Outcome::BlockBoundary {
            node: NodeId(9),
            ..
        }
    ));
    let after = d
        .commit_block(
            &p,
            &mut LayoutContext::new(),
            &boundary.state,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            20.0,
        )
        .unwrap();
    assert_eq!(after.block(), 30.0);
    assert_eq!(after.slot(10.0), (20.0, 80.0));
    let (l, _, events, _) = line(trial(&d, &p, &after));
    assert_eq!(l.block_offset(), 30.0);
    assert!(!events.iter().any(|e| matches!(e, Event::Reported { .. })));
}
#[test]
fn validates_geometry_missing_specs_and_illegal_handoffs() {
    let p = paragraph(&[Part::Float(2), Part::Text("a")], false);
    assert!(Checkpoint::new(&p, f32::NAN).is_err());
    assert!(Checkpoint::new(&p, 0.0).is_err());
    assert!(Driver::new(vec![spec(2, Side::Left, Clear::None, -1.0, 2.0)]).is_err());
    assert!(Driver::new(vec![spec(2, Side::Left, Clear::None, 1.0, f32::INFINITY)]).is_err());
    assert!(Driver::new(vec![spec(2, Side::Left, Clear::None, 1.0, 2.0); 2]).is_err());
    let d = Driver::new(vec![]).unwrap();
    let s = Checkpoint::new(&p, 100.0).unwrap();
    assert!(matches!(
        d.trial(
            &p,
            &mut LayoutContext::new(),
            &s,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None
        ),
        Err(FlowError::MissingFloat)
    ));
    assert!(
        s.begin_paragraph(
            &p,
            &p,
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY
        )
        .is_err()
    );
    assert!(s.next_fragment(0.0, -10.0).is_err());
    assert!(widow_prefix(1, 2, 0, 1, true).is_err());
}
#[test]
fn retained_band_insets_prevent_taffy_opposite_side_overlap() {
    // Upstream 0.14 reproducer, kept separate from caller geometry:
    // the right float extends past the left float's bottom at y=30.
    let mut native = taffy::compute::FloatContext::new();
    native.set_width(100.0);
    native.place_floated_box(
        taffy::Size {
            width: 20.0,
            height: 30.0,
        },
        0.0,
        [0.0, 0.0],
        Side::Left,
        Clear::None,
    );
    native.place_floated_box(
        taffy::Size {
            width: 20.0,
            height: 50.0,
        },
        0.0,
        [0.0, 0.0],
        Side::Right,
        Clear::None,
    );
    let unguarded = native.place_floated_box(
        taffy::Size {
            width: 10.0,
            height: 10.0,
        },
        0.0,
        [0.0, 0.0],
        Side::Right,
        Clear::Left,
    );
    assert_eq!(
        (unguarded.x, unguarded.y),
        (90.0, 30.0),
        "recheck the caller workaround if Taffy changes"
    );
    let p = paragraph(
        &[
            Part::Float(2),
            Part::Float(3),
            Part::Float(4),
            Part::Text("a"),
        ],
        false,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Right, Clear::None, 20.0, 50.0),
        spec(4, Side::Right, Clear::Left, 10.0, 10.0),
    ])
    .unwrap();
    let (_, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    assert_eq!(
        (
            s.placed()[2].rect.inline_start,
            s.placed()[2].rect.block_start
        ),
        (70.0, 30.0)
    );
    assert!(s.placed()[2].rect.inline_start + 10.0 <= s.placed()[1].rect.inline_start);
}
#[test]
fn zero_width_float_still_contributes_clearance() {
    let p = paragraph(&[Part::Float(2), Part::Float(3), Part::Text("a")], false);
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 0.0, 30.0),
        spec(3, Side::Right, Clear::Left, 10.0, 10.0),
    ])
    .unwrap();
    let (_, s, _, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    assert_eq!(s.placed()[1].rect.block_start, 30.0);
    assert_eq!(s.clearance(Clear::Left), Some(30.0));
    assert_eq!(calls, 3);
}
#[allow(dead_code)]
#[path = "../examples/float_png.rs"]
mod png_example;
#[test]
fn real_font_sample_paints_accepted_glyphs_and_float_boxes_deterministically() {
    let (lines, state) = png_example::sample().unwrap();
    assert!(lines.len() >= 4);
    assert_eq!(state.placed().len(), 2);
    assert_eq!(state.placed()[0].rect.inline_start, 0.0);
    assert_eq!(state.placed()[0].rect.block_start, 0.0);
    assert_eq!(state.placed()[1].rect.inline_start, 170.0);
    let first = png_example::paint(&lines, &state).unwrap();
    let second = png_example::paint(&lines, &state).unwrap();
    assert_eq!(first.encode_png().unwrap(), second.encode_png().unwrap());
    let pixel = |x: usize, y: usize| {
        &first.data()
            [(y * first.width() as usize + x) * 4..(y * first.width() as usize + x + 1) * 4]
    };
    assert_eq!(pixel(20, 20), [0, 80, 220, 255]);
    let y = state.placed()[1].rect.block_start as usize + 15;
    assert_eq!(pixel(185, y), [0, 150, 80, 255]);
    assert_eq!(pixel(5, 5), [255, 255, 255, 255]);
    assert!(first.data().as_chunks::<4>().0.contains(&[0, 0, 0, 255]));
    assert!(lines.iter().all(|l| l.displaced_floats().is_empty()));
}
#[test]
fn float_matrix_respects_css_nonoverlap_order_clearance_and_bounds() {
    for widths in [[20.0, 20.0, 10.0], [60.0, 50.0, 70.0], [0.0, 40.0, 20.0]] {
        for heights in [[30.0, 50.0, 10.0], [50.0, 10.0, 40.0], [0.0, 20.0, 30.0]] {
            for sides in [
                [Side::Left, Side::Right, Side::Right],
                [Side::Right, Side::Left, Side::Left],
                [Side::Left; 3],
                [Side::Right; 3],
            ] {
                for clear in [Clear::None, Clear::Left, Clear::Right, Clear::Both] {
                    let p = paragraph(
                        &[
                            Part::Float(2),
                            Part::Float(3),
                            Part::Float(4),
                            Part::Text("a"),
                        ],
                        false,
                    );
                    let specs = (0..3)
                        .map(|i| {
                            spec(
                                i as u64 + 2,
                                sides[i],
                                if i == 2 { clear } else { Clear::None },
                                widths[i],
                                heights[i],
                            )
                        })
                        .collect();
                    let d = Driver::new(specs).unwrap();
                    let (_, s, _, calls) =
                        line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
                    assert!(calls <= 10);
                    assert_eq!(s.placed().len(), 3);
                    let rects = s.placed().iter().map(|p| p.rect).collect::<Vec<_>>();
                    for (i, r) in rects.iter().enumerate() {
                        assert!(r.inline_start >= 0.0 && r.inline_start + r.inline_size <= 100.0);
                        for (j, earlier) in rects[..i].iter().enumerate() {
                            assert!(r.block_start >= earlier.block_start);
                            if i == 2
                                && (clear == Clear::Both
                                    || clear == Clear::Left && sides[j] == Side::Left
                                    || clear == Clear::Right && sides[j] == Side::Right)
                            {
                                assert!(r.block_start >= earlier.block_start + earlier.block_size);
                            }
                            if r.inline_size > 0.0
                                && earlier.inline_size > 0.0
                                && r.block_size > 0.0
                                && earlier.block_size > 0.0
                            {
                                let overlap = r.inline_start
                                    < earlier.inline_start + earlier.inline_size
                                    && earlier.inline_start < r.inline_start + r.inline_size
                                    && r.block_start < earlier.block_start + earlier.block_size
                                    && earlier.block_start < r.block_start + r.block_size;
                                assert!(!overlap, "overlap {rects:?}; {sides:?}/{clear:?}");
                            }
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn reused_paragraph_cursor_withdrawal_preserves_previous_placement_epoch() {
    let p = paragraph(
        &[
            Part::Float(2),
            Part::Text("a a \ta "),
            Part::Float(3),
            Part::Text("b"),
        ],
        true,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Left, Clear::None, 50.0, 30.0),
    ])
    .unwrap();
    let (_, previous, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 160.0).unwrap()));
    let again = previous
        .begin_paragraph(
            &p,
            &p,
            &mut LayoutContext::new(),
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
        )
        .unwrap();
    let output = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &again,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    assert!(matches!(output.stop, Some(Stop::Done)));
    let end = output.select(output.lines.len()).unwrap();
    assert_eq!(end.placed().len(), 4);
    assert_eq!(end.placed()[0].rect, previous.placed()[0].rect);
    assert_eq!(end.placed()[1].rect, previous.placed()[1].rect);
}
#[test]
fn widow_selected_page_checkpoint_and_height_limited_preview_replay_actual_content() {
    let p = paragraph(
        &[
            Part::Text("aa bb "),
            Part::Float(2),
            Part::Text("cc dd ee ff"),
        ],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 20.0, 40.0)]).unwrap();
    let start = Checkpoint::new(&p, 60.0).unwrap();
    let all = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: None,
            },
        )
        .unwrap();
    let page = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: Some(40.0),
            },
        )
        .unwrap();
    assert_eq!(page.lines.len(), 4);
    assert!(matches!(page.stop, Some(Stop::Height { needed: 10.0 })));
    let accepted = widow_prefix(page.lines.len(), all.lines.len(), 2, 2, false).unwrap();
    assert_eq!(accepted, 3);
    let next = page
        .select(accepted)
        .unwrap()
        .next_fragment(40.0, 60.0)
        .unwrap();
    assert_eq!(next.placed()[0].rect.block_size, 10.0);
    let rest = d
        .preview(
            &p,
            &mut LayoutContext::new(),
            &next,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 20,
                height: Some(40.0),
            },
        )
        .unwrap();
    assert_eq!(
        rest.lines
            .iter()
            .map(|l| l.text_range())
            .collect::<Vec<_>>(),
        [15..18, 18..20]
    );
    assert_eq!(
        rest.lines
            .iter()
            .map(|l| l.block_offset())
            .collect::<Vec<_>>(),
        [0.0, 10.0]
    );
    assert!(
        !rest
            .events
            .iter()
            .any(|e| matches!(e, Event::Reported { .. }))
    );
    assert!(matches!(rest.stop, Some(Stop::Done)));
    assert!(
        d.preview(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            PreviewLimit {
                lines: 0,
                height: Some(f32::NAN)
            }
        )
        .is_err()
    );
}
#[test]
fn head_float_can_stay_high_when_unbreakable_text_moves_below_it() {
    let p = paragraph(&[Part::Float(2), Part::Text("aaaaa")], false);
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let (l, s, _, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(l.block_offset(), 30.0);
    assert_eq!(s.placed()[0].rect.block_start, 0.0);
    assert_eq!(calls, 3);
    let mut options = LineOptions::default();
    options.text_indent.length = 5.0;
    let (l, s, _, _) = line(
        d.trial(
            &p,
            &mut LayoutContext::new(),
            &Checkpoint::new(&p, 60.0).unwrap(),
            &options,
            &AtomicSizes::EMPTY,
            None,
        )
        .unwrap(),
    );
    assert_eq!(l.block_offset(), 30.0);
    assert_eq!(s.placed()[0].rect.block_start, 0.0);
}
#[test]
fn atomic_prefix_prevents_trial_float_from_remaining_above_its_line() {
    let p = paragraph(
        &[Part::Atomic(9), Part::Float(2), Part::Text("aaaaa")],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let mut a = AtomicSizes::new();
    a.insert(
        NodeId(9),
        shodo::AtomicSize {
            inline_size: 0.0,
            block_size: 10.0,
            baseline: Some(8.0),
            ..Default::default()
        },
    );
    let (l, s, _, _) = line(
        d.trial(
            &p,
            &mut LayoutContext::new(),
            &Checkpoint::new(&p, 60.0).unwrap(),
            &LineOptions::default(),
            &a,
            None,
        )
        .unwrap(),
    );
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(s.placed()[0].rect.block_start, 10.0);
}
#[test]
fn source_order_handles_zero_advance_prefix_and_float_only_prefix() {
    let p = paragraph(
        &[Part::Text("\u{0301}"), Part::Float(2), Part::Text("aaaaa")],
        false,
    );
    let d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let (l, s, events, _) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(s.placed()[0].rect.block_start, 10.0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Reported { position: 0.0, .. }))
    );
    let p = paragraph(
        &[Part::Float(2), Part::Float(3), Part::Text("aaaaa")],
        false,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 10.0, 30.0),
        spec(3, Side::Right, Clear::None, 20.0, 30.0),
    ])
    .unwrap();
    let (l, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(l.block_offset(), 30.0);
    assert!(s.placed().iter().all(|p| p.rect.block_start == 0.0));
}
fn transform_paragraph(mapping: bool, first_line: bool) -> shodo::Paragraph {
    let mut style = ParagraphStyle::default();
    style.root.font_size = 10.0;
    if first_line {
        let mut first = style.root.clone();
        first.text_transform = shodo::style::TextTransform::Uppercase;
        style.first_line = Some(first);
    }
    let mut b = shodo::ParagraphBuilder::new(&style, &Limits::default());
    b.with_offset_mapping(mapping)
        .push_text(TextSource::Generated { node: NodeId(1) }, "\u{fb00}")
        .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(1) }, "aaaaa");
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        ),
    )
    .unwrap()
}
#[test]
fn first_line_uses_its_actual_processed_source_set_and_mapping_off_metadata() {
    for mapping in [true, false] {
        let p = transform_paragraph(mapping, true);
        let mut d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
        if !mapping {
            // Register the first-line set, whose marker starts at byte2,
            // rather than the normal Paragraph set's marker at byte3.
            d.register_source_order(
                p.id(),
                "FF\u{fffc}AAAAA",
                SourceOrder {
                    floats: vec![(NodeId(2), 2)],
                    atomics: vec![],
                    edges: Some(vec![]),
                },
            )
            .unwrap();
        }
        let (l, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 80.0).unwrap()));
        assert_eq!(p.text(), "\u{fb00}\u{fffc}aaaaa");
        assert_eq!(l.text(), "FF\u{fffc}AAAAA");
        assert_eq!(l.block_offset(), 0.0);
        assert_eq!(s.placed()[0].rect.block_start, 10.0);
    }
    let p = transform_paragraph(false, false);
    let mut d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let start = Checkpoint::new(&p, 70.0).unwrap();
    // No source metadata means a clear error, not a position-based guess.
    assert!(matches!(
        d.trial(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None
        ),
        Err(FlowError::MissingSourceOrder)
    ));
    assert!(
        d.register_source_order(
            p.id(),
            p.text(),
            SourceOrder {
                floats: vec![(NodeId(2), 0)],
                atomics: vec![],
                edges: Some(vec![])
            }
        )
        .is_err()
    );
    d.register_source_order(
        p.id(),
        p.text(),
        SourceOrder {
            floats: vec![(NodeId(2), 3)],
            atomics: vec![],
            edges: Some(vec![]),
        },
    )
    .unwrap();
    let (l, s, _, _) = line(trial(&d, &p, &start));
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(s.placed()[0].rect.block_start, 10.0);
}
#[test]
fn geometry_defers_latest_placements_one_at_a_time_then_flushes_source_order() {
    let p = paragraph(
        &[
            Part::Text("b"),
            Part::Float(2),
            Part::Float(3),
            Part::Text("aaaaa"),
        ],
        false,
    );
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 30.0, 30.0),
        spec(3, Side::Right, Clear::None, 10.0, 30.0),
    ])
    .unwrap();
    let (l, s, events, calls) = line(trial(&d, &p, &Checkpoint::new(&p, 80.0).unwrap()));
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(
        s.placed()
            .iter()
            .map(|p| (p.node, p.rect.block_start))
            .collect::<Vec<_>>(),
        [(NodeId(2), 10.0), (NodeId(3), 10.0)]
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|e| if let Event::Deferred { node } = e {
                Some(*node)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        [NodeId(3), NodeId(2)]
    );
    assert_eq!(calls, 5);
}
#[test]
fn source_order_remains_active_after_first_geometry_position_retry() {
    let mut style = ParagraphStyle::default();
    style.root.font_size = 10.0;
    style.root.line_height = shodo::style::LineHeight::Px(40.0);
    let mut b = shodo::ParagraphBuilder::new(&style, &Limits::default());
    b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(1) }, "a")
        .push_out_of_flow(NodeId(3), OutOfFlowKind::Float)
        .push_text(TextSource::Generated { node: NodeId(1) }, "aaaaaaaa");
    let p = b
        .build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    let d = Driver::new(vec![
        spec(2, Side::Left, Clear::None, 20.0, 30.0),
        spec(3, Side::Right, Clear::Left, 20.0, 30.0),
    ])
    .unwrap();
    let (l, s, events, _) = line(trial(&d, &p, &Checkpoint::new(&p, 100.0).unwrap()));
    assert_eq!(l.block_offset(), 30.0);
    assert_eq!(s.placed()[0].rect.block_start, 0.0);
    assert_eq!(s.placed()[1].rect.block_start, 70.0);
    assert!(events.contains(&Event::PositionRetry { block: 30.0 }));
    assert!(events.contains(&Event::Deferred { node: NodeId(3) }));
}
fn empty_inline_paragraph(float_first: bool) -> shodo::Paragraph {
    let mut style = ParagraphStyle::default();
    style.root.font_size = 10.0;
    let mut b = shodo::ParagraphBuilder::new(&style, &Limits::default());
    if float_first {
        b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
    }
    let mut edges = shodo::node::InlineEdges::default();
    edges.padding.inline_start = 5.0;
    edges.border.inline_end = 5.0;
    b.open_inline(NodeId(8), &style.root, edges).close_inline();
    if !float_first {
        b.push_out_of_flow(NodeId(2), OutOfFlowKind::Float);
    }
    b.push_text(TextSource::Generated { node: NodeId(1) }, "aaaaa");
    b.build(
        &mut LayoutContext::new(),
        &FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        ),
    )
    .unwrap()
}
#[test]
fn empty_inline_edges_before_float_remain_on_the_same_or_higher_line() {
    let p = empty_inline_paragraph(false);
    let mut d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    let start = Checkpoint::new(&p, 60.0).unwrap();
    assert!(matches!(
        d.trial(
            &p,
            &mut LayoutContext::new(),
            &start,
            &LineOptions::default(),
            &AtomicSizes::EMPTY,
            None
        ),
        Err(FlowError::MissingSourceOrder)
    ));
    d.register_source_order(
        p.id(),
        p.text(),
        SourceOrder {
            floats: vec![(NodeId(2), 0)],
            atomics: vec![],
            edges: Some(vec![
                SourceEdge {
                    node: NodeId(8),
                    offset: 0,
                    start: true,
                },
                SourceEdge {
                    node: NodeId(8),
                    offset: 0,
                    start: false,
                },
            ]),
        },
    )
    .unwrap();
    let (l, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(l.block_offset(), 0.0);
    assert_eq!(s.placed()[0].rect.block_start, 10.0);
}

#[test]
fn inline_edges_after_head_float_do_not_force_it_below_later_content() {
    let p = empty_inline_paragraph(true);
    let mut d = Driver::new(vec![spec(2, Side::Left, Clear::None, 30.0, 30.0)]).unwrap();
    d.register_source_order(
        p.id(),
        p.text(),
        SourceOrder {
            floats: vec![(NodeId(2), 0)],
            atomics: vec![],
            edges: Some(vec![
                SourceEdge {
                    node: NodeId(8),
                    offset: 3,
                    start: true,
                },
                SourceEdge {
                    node: NodeId(8),
                    offset: 3,
                    start: false,
                },
            ]),
        },
    )
    .unwrap();
    let (l, s, _, _) = line(trial(&d, &p, &Checkpoint::new(&p, 60.0).unwrap()));
    assert_eq!(l.block_offset(), 30.0);
    assert_eq!(s.placed()[0].rect.block_start, 0.0);
}
