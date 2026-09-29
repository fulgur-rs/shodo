#![cfg(feature = "accesskit")]

use shodo::accessibility::accesskit::{
    AccessKitAdapter, AccessKitError, NodeSemantics, types as ak,
};
use shodo::accessibility::{AccessibleLayout, AccessibleSelection};
use shodo::geometry::{PhysicalRect, WritingMode};
use shodo::mapping::Affinity;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{
    FontFamily, InlineStyle, OverflowWrap, ParagraphStyle, TextCombineUpright, TextTransform,
    WhiteSpaceCollapse,
};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, Line, ParagraphBuilder};
use shodo_fixtures::{FONTS, load_fonts};

fn root_style() -> InlineStyle {
    InlineStyle {
        font_families: FONTS
            .iter()
            .map(|f| FontFamily::Named(f.family.into()))
            .collect(),
        font_size: 16.0,
        ..Default::default()
    }
}
fn style() -> ParagraphStyle {
    ParagraphStyle {
        root: root_style(),
        ..Default::default()
    }
}
fn lines(text: &str, s: ParagraphStyle, width: f32) -> Vec<Line> {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut b = ParagraphBuilder::new(&s, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(7),
            offset: 0,
        },
        text,
    );
    b.build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        )
}
fn frame() -> PhysicalRect {
    PhysicalRect {
        x: 50.0,
        y: 30.0,
        width: 1000.0,
        height: 500.0,
    }
}
fn root() -> ak::Node {
    let mut n = ak::Node::new(ak::Role::Document);
    n.set_read_only();
    n.add_action(ak::Action::SetTextSelection);
    n
}
fn allocator(counter: &mut u64) -> impl FnMut() -> ak::NodeId + '_ {
    move || {
        *counter += 1;
        ak::NodeId(*counter)
    }
}
fn export(
    a: &mut AccessKitAdapter,
    l: &AccessibleLayout<'_>,
    selection: Option<AccessibleSelection>,
    counter: &mut u64,
) -> ak::TreeUpdate {
    a.update(
        l,
        root(),
        frame(),
        selection,
        |_| NodeSemantics::default(),
        allocator(counter),
    )
    .unwrap()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 0.025, "{a} != {b}");
}

#[test]
fn consumer_reads_logical_text_geometry_and_selection() {
    let ls = lines("ffi سلام b", style(), 1000.0);
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let selection = AccessibleSelection {
        anchor: l.position(0, 1, Affinity::Downstream).unwrap(),
        focus: l.position(0, 3, Affinity::Upstream).unwrap(),
    };
    let update = export(&mut a, &l, Some(selection), &mut counter);
    let first = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == ak::Role::TextRun && n.value() == Some("ffi "))
        .unwrap();
    assert_eq!(first.1.character_lengths(), [1, 1, 1, 1]);
    assert_eq!(first.1.font_size(), Some(16.0));
    assert_eq!(first.1.font_weight(), Some(400.0));
    let tree = accesskit_consumer::Tree::new(update, true);
    let state = tree.state();
    let document = state.root();
    assert_eq!(document.document_range().text(), "ffi سلام b");
    let selected = document.text_selection().unwrap();
    assert_eq!(selected.text(), "fi");
    let boxes = selected.bounding_boxes();
    assert_eq!(boxes.len(), 1);
    close(boxes[0].x0, 55.04);
    // The retained font's ffi advance is 946/1000em, before fixed-point rounding.
    close(boxes[0].x1, 65.136);
    assert_eq!(
        a.from_position(
            a.to_position(selection.anchor).unwrap(),
            Affinity::Downstream
        ),
        Some(selection.anchor)
    );
    let focus = a
        .from_position(a.to_position(selection.focus).unwrap(), Affinity::Upstream)
        .unwrap();
    assert_eq!(
        l.to_source(focus).unwrap().origin,
        shodo::mapping::TextOrigin::Dom {
            node: NodeId(7),
            offset: 3
        }
    );
}

#[test]
fn consumer_preserves_hard_breaks_and_first_line_datasets() {
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let mut first = s.root.clone();
    first.text_transform = TextTransform::Uppercase;
    s.first_line = Some(first);
    let ls = lines("ſ\nb", s, 1000.0);
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let selection = AccessibleSelection {
        anchor: l.position(0, 0, Affinity::Downstream).unwrap(),
        focus: l.position(1, 1, Affinity::Upstream).unwrap(),
    };
    let tree =
        accesskit_consumer::Tree::new(export(&mut a, &l, Some(selection), &mut counter), true);
    let state = tree.state();
    assert_eq!(state.root().document_range().text(), "S\nb");
    assert_eq!(state.root().text_selection().unwrap().text(), "S\nb");
    let end = l.position(0, 2, Affinity::Upstream).unwrap();
    let end = a.to_position(end).unwrap();
    let canonical = a.from_position(end, Affinity::Upstream).unwrap();
    assert_eq!(l.to_text_position(canonical).unwrap().offset, 1);
    let mut s = style();
    s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
    let ls = lines("a\u{2028}b", s, 1000.0);
    let l = AccessibleLayout::new(&ls);
    let tree = accesskit_consumer::Tree::new(export(&mut a, &l, None, &mut counter), true);
    assert_eq!(tree.state().root().document_range().text(), "a\nb");
    assert_eq!(l.logical_text(), "a\u{2028}b");
}

#[test]
fn consumer_select_all_round_trips_trailing_hard_break() {
    for text in ["a\n", "\n"] {
        let mut s = style();
        s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let ls = lines(text, s, 1000.0);
        let layout = AccessibleLayout::new(&ls);
        let mut adapter = AccessKitAdapter::new(ak::NodeId(1));
        let mut counter = 10;
        let tree =
            accesskit_consumer::Tree::new(export(&mut adapter, &layout, None, &mut counter), true);
        let state = tree.state();
        let document = state.root();
        assert_eq!(document.document_range().text(), text);
        let action = ak::ActionRequest {
            action: ak::Action::SetTextSelection,
            target_tree: ak::TreeId::ROOT,
            target_node: ak::NodeId(1),
            data: Some(ak::ActionData::SetTextSelection(ak::TextSelection {
                anchor: document.document_start().to_raw(),
                focus: document.document_end().to_raw(),
            })),
        };
        let Some(ak::ActionData::SetTextSelection(raw)) = action.data else {
            unreachable!()
        };
        let selection = AccessibleSelection {
            anchor: adapter
                .from_position(raw.anchor, Affinity::Downstream)
                .unwrap(),
            focus: adapter
                .from_position(raw.focus, Affinity::Upstream)
                .unwrap(),
        };
        assert_eq!(
            layout.to_text_position(selection.focus).unwrap().offset,
            text.len() as u32
        );
        for selection in [
            selection,
            AccessibleSelection {
                anchor: selection.focus,
                focus: selection.anchor,
            },
        ] {
            let tree = accesskit_consumer::Tree::new(
                export(&mut adapter, &layout, Some(selection), &mut counter),
                true,
            );
            assert_eq!(tree.state().root().text_selection().unwrap().text(), text);
        }
    }
}

#[test]
fn consumer_selects_hard_break_alone_and_normalizes_collapsed_caret() {
    for text in ["a\n", "\n", "a\nb"] {
        let mut s = style();
        s.root.white_space_collapse = WhiteSpaceCollapse::Preserve;
        let ls = lines(text, s, 1000.0);
        let layout = AccessibleLayout::new(&ls);
        let mut adapter = AccessKitAdapter::new(ak::NodeId(1));
        let mut counter = 10;
        let update = export(&mut adapter, &layout, None, &mut counter);
        let newline = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == ak::Role::TextRun && n.value() == Some("\n"))
            .unwrap()
            .0;
        let begin = ak::TextPosition {
            node: newline,
            character_index: 0,
        };
        let end = ak::TextPosition {
            node: newline,
            character_index: 1,
        };
        let selection = AccessibleSelection {
            anchor: adapter.from_position(begin, Affinity::Downstream).unwrap(),
            focus: adapter.from_position(end, Affinity::Upstream).unwrap(),
        };
        for selection in [
            selection,
            AccessibleSelection {
                anchor: selection.focus,
                focus: selection.anchor,
            },
        ] {
            let tree = accesskit_consumer::Tree::new(
                export(&mut adapter, &layout, Some(selection), &mut counter),
                true,
            );
            assert_eq!(tree.state().root().text_selection().unwrap().text(), "\n");
        }
        let caret = AccessibleSelection {
            anchor: adapter.from_position(end, Affinity::Downstream).unwrap(),
            focus: adapter.from_position(end, Affinity::Upstream).unwrap(),
        };
        let update = export(&mut adapter, &layout, Some(caret), &mut counter);
        let selection = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ak::NodeId(1))
            .unwrap()
            .1
            .text_selection()
            .unwrap();
        assert_eq!(selection.anchor, begin);
        assert_eq!(selection.focus, begin);
        let tree = accesskit_consumer::Tree::new(update, true);
        assert_eq!(tree.state().root().text_selection().unwrap().text(), "");
    }
}

#[test]
fn atomic_alternatives_and_caller_roles_are_exposed() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let s = style();
    let mut b = ParagraphBuilder::new(&s, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(7),
            offset: 0,
        },
        "a",
    )
    .push_atomic(NodeId(2), &s.root, InlineEdges::default())
    .push_text(
        TextSource::Dom {
            node: NodeId(7),
            offset: 1,
        },
        "b",
    );
    let mut atomics = AtomicSizes::new();
    atomics.insert(
        NodeId(2),
        AtomicSize {
            inline_size: 20.0,
            block_size: 12.0,
            baseline: Some(8.0),
            margins: Default::default(),
        },
    );
    let ls = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap()
        .break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &atomics,
        );
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let update = a
        .update(
            &l,
            root(),
            frame(),
            None,
            |node| {
                if node == NodeId(2) {
                    NodeSemantics {
                        role: ak::Role::Image,
                        label: Some("写真".into()),
                        description: Some("long description".repeat(100)),
                    }
                } else {
                    NodeSemantics {
                        role: ak::Role::Link,
                        ..Default::default()
                    }
                }
            },
            allocator(&mut counter),
        )
        .unwrap();
    let wrapper = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == ak::Role::Image)
        .unwrap();
    assert_eq!(wrapper.1.label(), Some("写真"));
    assert!(wrapper.1.description().unwrap().len() > 255);
    let alternative = update
        .nodes
        .iter()
        .find(|(_, n)| n.role() == ak::Role::TextRun && n.value() == Some("写真"))
        .unwrap();
    assert_eq!(alternative.1.character_lengths(), [6]);
    let bounds = alternative.1.bounds().unwrap();
    close(bounds.width(), 20.0);
    close(bounds.height(), 12.0);
    for (at, want) in [(0, 1), (1, 4)] {
        let p = a
            .from_position(
                ak::TextPosition {
                    node: alternative.0,
                    character_index: at,
                },
                Affinity::Downstream,
            )
            .unwrap();
        assert_eq!(l.to_text_position(p).unwrap().offset, want);
    }
    assert!(update.nodes.iter().any(|(_, n)| n.role() == ak::Role::Link));
    let tree = accesskit_consumer::Tree::new(update, true);
    assert_eq!(tree.state().root().document_range().text(), "a写真b");
}

#[test]
fn adapter_reuses_ids_and_rejects_removed_positions_after_reflow() {
    let wide = lines("a b c", style(), 1000.0);
    let wide = AccessibleLayout::new(&wide);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let old = wide.position(0, 0, Affinity::Downstream).unwrap();
    let mut tree = accesskit_consumer::Tree::new(export(&mut a, &wide, None, &mut counter), true);
    let first = a.to_position(old).unwrap().node;
    let narrow = lines("a b c", style(), 16.0);
    let narrow = AccessibleLayout::new(&narrow);
    let update = export(&mut a, &narrow, None, &mut counter);
    tree.update_and_process_changes(update, &mut IgnoreChanges);
    assert_eq!(
        a.to_position(narrow.position(0, 0, Affinity::Downstream).unwrap())
            .unwrap()
            .node,
        first
    );
    assert!(a.to_position(old).is_none());
    let removed = a
        .to_position(narrow.position(1, 0, Affinity::Downstream).unwrap())
        .unwrap();
    assert_ne!(removed.node, first);
    tree.update_and_process_changes(
        export(&mut a, &wide, None, &mut counter),
        &mut IgnoreChanges,
    );
    assert!(a.from_position(removed, Affinity::Downstream).is_none());
    assert_eq!(tree.state().root().document_range().text(), "a b c");
}

#[test]
fn source_ids_survive_reflow_and_shifted_repeated_transformed_text() {
    // Using processed offsets as DOM anchors would lose these IDs when the
    // prefix is removed. Collapsing the two source occurrences would lose one
    // ID. Ignoring remapped offsets would misidentify the omitted LF and SS.
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style();
    s.root.text_transform = TextTransform::Uppercase;
    s.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut adapter = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let mut original_ids = None;
    let mut previous_position = None;
    for (prefix, width) in [(true, 1000.0), (false, 16.0), (false, 1000.0)] {
        let mut builder = ParagraphBuilder::new(&s, &limits);
        if prefix {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(99),
                    offset: 0,
                },
                "X",
            );
        }
        for occurrence in 0..2 {
            if occurrence == 1 {
                builder.push_text(TextSource::Generated { node: NodeId(42) }, "!");
            }
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(7),
                    offset: 0,
                },
                "水\n水ß",
            );
        }
        let paragraph = builder
            .build(&mut LayoutContext::new(), &fonts.collection)
            .unwrap();
        let lines = paragraph.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        let layout = AccessibleLayout::new(&lines);
        let at = |offset, affinity| {
            layout
                .lines()
                .iter()
                .find_map(|line| {
                    line.characters
                        .iter()
                        .position(|character| character.text_range.start == offset)
                        .map(|character| layout.position(line.index, character, affinity).unwrap())
                })
                .expect("literal processed boundary")
        };
        let prefix_len = u32::from(prefix);
        let first = at(prefix_len, Affinity::Downstream);
        let second = at(prefix_len + 9, Affinity::Downstream);
        for (offset, source_offset) in [(0, 0), (3, 4), (6, 7), (9, 0), (12, 4), (15, 7)] {
            assert_eq!(
                layout
                    .to_source(at(prefix_len + offset, Affinity::Downstream))
                    .unwrap()
                    .origin,
                shodo::mapping::TextOrigin::Dom {
                    node: NodeId(7),
                    offset: source_offset,
                }
            );
        }
        let selection = AccessibleSelection {
            anchor: first,
            focus: at(prefix_len + 8, Affinity::Upstream),
        };
        let tree = accesskit_consumer::Tree::new(
            export(&mut adapter, &layout, Some(selection), &mut counter),
            true,
        );
        let state = tree.state();
        assert_eq!(
            state.root().document_range().text(),
            if prefix {
                "X水水SS!水水SS"
            } else {
                "水水SS!水水SS"
            }
        );
        assert_eq!(state.root().text_selection().unwrap().text(), "水水SS");
        let ids = [
            adapter.to_position(first).unwrap().node,
            adapter.to_position(second).unwrap().node,
        ];
        assert_ne!(ids[0], ids[1]);
        if let Some(original) = original_ids {
            assert_eq!(ids, original);
        }
        original_ids = Some(ids);
        if let Some(previous) = previous_position {
            assert!(adapter.to_position(previous).is_none());
        }
        previous_position = Some(first);
        for position in [first, second, selection.focus] {
            assert_eq!(
                adapter.from_position(adapter.to_position(position).unwrap(), position.affinity),
                Some(position)
            );
        }
    }
}

#[test]
fn failed_exports_preserve_last_successful_state() {
    let ls = lines("a", style(), 1000.0);
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    export(&mut a, &l, None, &mut counter);
    let p = l.position(0, 0, Affinity::Downstream).unwrap();
    let previous = a.to_position(p);
    let huge = lines(&format!("a{}", "\u{0301}".repeat(200)), style(), 1000.0);
    let huge = AccessibleLayout::new(&huge);
    assert_eq!(huge.lines()[0].characters.len(), 1);
    assert!(matches!(
        a.update(
            &huge,
            root(),
            frame(),
            None,
            |_| NodeSemantics::default(),
            allocator(&mut counter)
        ),
        Err(AccessKitError::CharacterTooLong)
    ));
    assert_eq!(a.to_position(p), previous);
    assert!(matches!(
        a.update(
            &l,
            root(),
            PhysicalRect {
                x: f32::NAN,
                ..frame()
            },
            None,
            |_| NodeSemantics::default(),
            allocator(&mut counter)
        ),
        Err(AccessKitError::InvalidFrame)
    ));
    assert_eq!(a.to_position(p), previous);
    let other = AccessibleLayout::new(&ls);
    let selection = AccessibleSelection {
        anchor: other.position(0, 0, Affinity::Downstream).unwrap(),
        focus: other.position(0, 1, Affinity::Upstream).unwrap(),
    };
    assert!(matches!(
        a.update(
            &l,
            root(),
            frame(),
            Some(selection),
            |_| NodeSemantics::default(),
            allocator(&mut counter)
        ),
        Err(AccessKitError::InvalidSelection)
    ));
    assert_eq!(a.to_position(p), previous);
    let fresh = lines("b", style(), 1000.0);
    let fresh = AccessibleLayout::new(&fresh);
    let mut new_adapter = AccessKitAdapter::new(ak::NodeId(1));
    assert!(matches!(
        new_adapter.update(
            &fresh,
            root(),
            frame(),
            None,
            |_| NodeSemantics::default(),
            || ak::NodeId(1)
        ),
        Err(AccessKitError::DuplicateNodeId)
    ));
    assert!(
        a.from_position(
            ak::TextPosition {
                node: ak::NodeId(999),
                character_index: 0
            },
            Affinity::Downstream
        )
        .is_none()
    );
}

#[test]
fn adapter_chunks_long_runs_without_splitting_characters() {
    let ls = lines(&"a".repeat(600), style(), 10000.0);
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let update = export(&mut a, &l, None, &mut counter);
    let chunks: Vec<_> = update
        .nodes
        .iter()
        .filter(|(_, n)| n.role() == ak::Role::TextRun)
        .collect();
    assert_eq!(
        chunks
            .iter()
            .map(|(_, n)| n.character_lengths().len())
            .collect::<Vec<_>>(),
        [255, 255, 90]
    );
    assert_eq!(chunks[0].1.word_starts(), [0]);
    assert!(chunks[1].1.word_starts().is_empty());
    assert!(chunks[2].1.word_starts().is_empty());
    assert_eq!(chunks[0].1.next_on_line(), Some(chunks[1].0));
    assert_eq!(chunks[1].1.previous_on_line(), Some(chunks[0].0));
    let p = l.position(0, 255, Affinity::Downstream).unwrap();
    let at = a.to_position(p).unwrap();
    assert_eq!((at.node, at.character_index), (chunks[1].0, 0));
    assert_eq!(a.from_position(at, Affinity::Downstream), Some(p));
    let tree = accesskit_consumer::Tree::new(update, true);
    assert_eq!(tree.state().root().document_range().text(), "a".repeat(600));
}

#[test]
fn consumer_geometry_follows_rtl_vertical_and_tcy() {
    for (text, mode, combine, direction) in [
        (
            "سلام",
            WritingMode::HorizontalTb,
            false,
            ak::TextDirection::RightToLeft,
        ),
        (
            "水水",
            WritingMode::VerticalRl,
            false,
            ak::TextDirection::TopToBottom,
        ),
        (
            "12",
            WritingMode::VerticalRl,
            true,
            ak::TextDirection::LeftToRight,
        ),
        (
            "12",
            WritingMode::VerticalLr,
            true,
            ak::TextDirection::LeftToRight,
        ),
    ] {
        let mut s = style();
        s.writing_mode = mode;
        if combine {
            s.root.text_combine_upright = TextCombineUpright::All;
        }
        let ls = lines(text, s, 1000.0);
        let l = AccessibleLayout::new(&ls);
        let mut a = AccessKitAdapter::new(ak::NodeId(1));
        let mut counter = 10;
        let selection = AccessibleSelection {
            anchor: l.position(0, 0, Affinity::Downstream).unwrap(),
            focus: l.position(0, 1, Affinity::Upstream).unwrap(),
        };
        let update = export(&mut a, &l, Some(selection), &mut counter);
        let run = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == ak::Role::TextRun)
            .unwrap();
        assert_eq!(run.1.text_direction(), Some(direction));
        let c = &l.lines()[0].characters[0];
        let converter = shodo::geometry::PhysicalConverter::new(
            mode,
            l.lines()[0].direction,
            shodo::geometry::PhysicalSize {
                width: 1000.0,
                height: 500.0,
            },
        );
        let expected = converter.rect(c.rect);
        let tree = accesskit_consumer::Tree::new(update, true);
        let state = tree.state();
        let boxes = state.root().text_selection().unwrap().bounding_boxes();
        assert_eq!(boxes.len(), 1);
        close(boxes[0].x0, f64::from(expected.x + 50.0));
        close(boxes[0].y0, f64::from(expected.y + 30.0));
        close(boxes[0].width(), f64::from(expected.width));
        close(boxes[0].height(), f64::from(expected.height));
    }
}

#[test]
fn consumer_word_navigation_crosses_chunks_and_soft_wraps() {
    let limits = Default::default();
    let fonts = load_fonts(&limits).unwrap();
    let mut s = style();
    s.root.overflow_wrap = OverflowWrap::Anywhere;
    let mut b = ParagraphBuilder::new(&s, &limits);
    b.push_text(
        TextSource::Dom {
            node: NodeId(7),
            offset: 0,
        },
        "hel",
    );
    let mut inline = root_style();
    inline.paint.color = [255, 0, 0, 255];
    b.open_inline(NodeId(9), &inline, InlineEdges::default())
        .push_text(
            TextSource::Dom {
                node: NodeId(8),
                offset: 0,
            },
            "lo world",
        )
        .close_inline();
    let p = b
        .build(&mut LayoutContext::new(), &fonts.collection)
        .unwrap();
    for width in [1000.0, 24.0] {
        let ls = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        let l = AccessibleLayout::new(&ls);
        let mut a = AccessKitAdapter::new(ak::NodeId(1));
        let mut counter = 10;
        let tree = accesskit_consumer::Tree::new(export(&mut a, &l, None, &mut counter), true);
        let state = tree.state();
        let root = state.root();
        assert_eq!(root.document_range().text(), "hello world");
        let next = root.document_start().forward_to_word_start();
        assert_eq!(next.to_global_usv_index(), 6);
    }
    let text = format!("{} world", "a".repeat(600));
    let ls = lines(&text, style(), 10000.0);
    let l = AccessibleLayout::new(&ls);
    let mut a = AccessKitAdapter::new(ak::NodeId(1));
    let mut counter = 10;
    let tree = accesskit_consumer::Tree::new(export(&mut a, &l, None, &mut counter), true);
    let state = tree.state();
    assert_eq!(
        state
            .root()
            .document_start()
            .forward_to_word_start()
            .to_global_usv_index(),
        601
    );
}

struct IgnoreChanges;
impl accesskit_consumer::TreeChangeHandler for IgnoreChanges {
    fn node_added(&mut self, _node: &accesskit_consumer::Node) {}
    fn node_updated(&mut self, _old: &accesskit_consumer::Node, _new: &accesskit_consumer::Node) {}
    fn focus_moved(
        &mut self,
        _old: Option<&accesskit_consumer::Node>,
        _new: Option<&accesskit_consumer::Node>,
    ) {
    }
    fn node_removed(&mut self, _node: &accesskit_consumer::Node) {}
}
