use super::*;
use crate::font::{FontCollection, FontFaceDescriptor};
use crate::limits::Limits;
use crate::node::{InlineEdges, NodeId, TextSource};
use crate::style::{
    FontFamily, InlineStyle, OverflowWrap, ParagraphStyle, TabSize, WhiteSpaceCollapse,
};
use crate::{AtomicSize, ParagraphBuilder};

#[test]
fn cold_and_alternating_short_lines_do_not_index_unvisited_units() {
    let fonts = FontCollection::new(&Limits::default());
    let paragraphs: Vec<_> = (0..3)
        .map(|node| {
            let mut builder = ParagraphBuilder::new(&Default::default(), &Limits::default());
            builder.push_text(
                TextSource::Generated { node: NodeId(node) },
                &"a b ".repeat(2048),
            );
            builder.build(&mut LayoutContext::new(), &fonts).unwrap()
        })
        .collect();
    for paragraph_count in [2, 3] {
        for counted in [false, true] {
            let mut cx = LayoutContext::new();
            let mut tokens: Vec<_> = paragraphs.iter().map(|p| p.start_token()).collect();
            let mut constraint = crate::LineConstraint::new(20.0);
            constraint.max_graphemes = counted.then_some(1);
            for _ in 0..24 {
                for (p, token) in paragraphs[..paragraph_count].iter().zip(&mut tokens) {
                    let before = cx.ruby_range_build_units;
                    let visited = cx.width_unit_visits;
                    let crate::LineResult::Line(line) = p.next_line(
                        &mut cx,
                        *token,
                        &Default::default(),
                        &constraint,
                        &AtomicSizes::EMPTY,
                    ) else {
                        panic!("expected a short line")
                    };
                    let next = line.break_token();
                    assert!(next.unit > token.unit && next.unit - token.unit <= 8);
                    assert!(
                        cx.ruby_range_build_units - before <= 16,
                        "counted={counted}: {} unvisited units indexed for a {}-unit line",
                        cx.ruby_range_build_units - before,
                        next.unit - token.unit
                    );
                    assert!(
                        cx.width_unit_visits - visited <= 16,
                        "unvisited unit producers ran"
                    );
                    *token = next;
                }
            }
        }
    }
}

fn clipped_hyphen_paragraph() -> (crate::Paragraph, usize, usize) {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Latin".into())],
                overflow_wrap: OverflowWrap::Anywhere,
                ..Default::default()
            },
            ..Default::default()
        },
        &Limits::default(),
    );
    for (node, text) in [(1, "f"), (2, "f"), (3, "i"), (4, " ab\u{ad}"), (5, "cd")] {
        builder.push_text(TextSource::Generated { node: NodeId(node) }, text);
    }
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let start = p
        .data
        .units
        .iter()
        .position(|u| {
            u.shared_cluster
                .as_ref()
                .is_some_and(|shared| u.text.start > shared.text.start)
        })
        .unwrap();
    let end = p
        .data
        .units
        .iter()
        .position(|u| u.break_after == crate::analysis::units::BreakClass::Hyphen)
        .unwrap()
        + 1;
    (p, start, end)
}

#[test]
fn split_cluster_identity_preserves_marker_order_and_shared_groups() {
    use crate::analysis::units::UnitKind;
    use crate::style::ParagraphStyle;
    use std::sync::Arc;

    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("Latin".into())],
        overflow_wrap: OverflowWrap::Anywhere,
        ..Default::default()
    };
    let paragraph_style = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&paragraph_style, &Limits::default());
    builder
        .push_text(TextSource::Generated { node: NodeId(1) }, "f")
        .open_inline(NodeId(10), &style, InlineEdges::default())
        .open_inline(NodeId(11), &style, InlineEdges::default())
        .push_text(TextSource::Generated { node: NodeId(2) }, "f")
        .close_inline()
        .push_text(TextSource::Generated { node: NodeId(3) }, "i")
        .close_inline()
        .push_text(TextSource::Generated { node: NodeId(4) }, " ffi");
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();

    let mut owners = Vec::<usize>::new();
    for (index, unit) in paragraph.data.units.iter().enumerate() {
        let Some(shared) = &unit.shared_cluster else {
            continue;
        };
        if owners.iter().all(|&owner| {
            !Arc::ptr_eq(
                shared,
                paragraph.data.units[owner].shared_cluster.as_ref().unwrap(),
            )
        }) {
            owners.push(index);
        }
    }
    assert!(
        owners.len() >= 2,
        "the real Latin fixture should split the marker group and following ffi group"
    );
    let mut continuation_markers = Vec::new();
    for first_index in owners {
        let shared = paragraph.data.units[first_index]
            .shared_cluster
            .as_ref()
            .unwrap();
        assert!(shared.slices.len() >= 2);
        assert_eq!(shared.units.start, *shared.slices.first().unwrap());
        assert_eq!(shared.units.end, shared.slices.last().unwrap() + 1);
        let mut end = shared.text.start;
        for &index in &shared.slices {
            let slice = &paragraph.data.units[index];
            assert!(Arc::ptr_eq(slice.shared_cluster.as_ref().unwrap(), shared));
            assert_eq!(slice.text.start, end, "slice order changed");
            assert!(slice.text.end >= slice.text.start);
            if slice.text.start > shared.text.start {
                let markers: Vec<_> = paragraph
                    .data
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(_, marker)| {
                        !matches!(marker.kind, UnitKind::Cluster { .. })
                            && marker.text.start == slice.text.start
                    })
                    .collect();
                if !markers.is_empty() {
                    assert!(
                        markers
                            .iter()
                            .all(|(marker_index, _)| *marker_index < index),
                        "every continuation marker must precede its slice at {}",
                        slice.text.start
                    );
                    continuation_markers.push((
                        slice.text.start,
                        markers
                            .iter()
                            .map(|(_, marker)| marker.kind.clone())
                            .collect::<Vec<_>>(),
                    ));
                }
            }
            end = slice.text.end;
        }
        assert_eq!(end, shared.text.end);
        assert!(shared.glyphs.start < shared.glyphs.end);
    }
    assert_eq!(
        continuation_markers,
        [
            (
                1,
                vec![
                    UnitKind::Open { box_index: 0 },
                    UnitKind::Open { box_index: 1 }
                ],
            ),
            (2, vec![UnitKind::Close { box_index: 1 }]),
        ],
        "all inline markers at continuation starts must precede their slices in source order"
    );

    let mut stack = Vec::new();
    for unit in &paragraph.data.units {
        match unit.kind {
            UnitKind::Open { box_index } => {
                assert_eq!(unit.parent_box, stack.last().copied());
                stack.push(box_index);
            }
            UnitKind::Close { .. } => {
                stack.pop();
                assert_eq!(unit.parent_box, stack.last().copied());
            }
            _ => assert_eq!(unit.parent_box, stack.last().copied()),
        }
    }
}

#[test]
fn split_cluster_identity_preserves_first_line_cursor_slices() {
    use crate::analysis::units::BreakClass;
    use crate::style::{FontVariantLigatures, ParagraphStyle};

    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let normal = InlineStyle {
        font_families: vec![FontFamily::Named("Latin".into())],
        ..Default::default()
    };
    let first = InlineStyle {
        font_variant_ligatures: FontVariantLigatures {
            none: true,
            ..Default::default()
        },
        ..normal.clone()
    };
    let style = ParagraphStyle {
        root: normal,
        first_line: Some(first),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(&style, &Limits::default());
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
    let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let shared = paragraph
        .data
        .units
        .iter()
        .find_map(|unit| unit.shared_cluster.as_ref())
        .expect("normal run should shape ffi as a shared ligature");
    let cursor_cuts: Vec<_> = shared
        .slices
        .iter()
        .filter_map(|&index| {
            let unit = &paragraph.data.units[index];
            unit.first_line_cursor.then_some(unit.text.end)
        })
        .collect();
    assert_eq!(cursor_cuts, [1, 2]);
    for &index in &shared.slices {
        let unit = &paragraph.data.units[index];
        if unit.first_line_cursor {
            assert_eq!(unit.break_after, BreakClass::Prohibited);
            assert!(unit.text.end < shared.text.end);
        }
    }
    assert_eq!(
        shared.units,
        shared.slices[0]..shared.slices.last().unwrap() + 1
    );
}

#[test]
fn equal_text_ranges_keep_storage_split_owners_separate() {
    use crate::analysis::units::UnitKind;
    use crate::style::ParagraphStyle;
    use std::sync::Arc;

    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("Latin".into())],
        overflow_wrap: OverflowWrap::Anywhere,
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style,
            ..Default::default()
        },
        &Limits::default(),
    );
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffia");
    let mut paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let data = Arc::get_mut(&mut paragraph.data).unwrap();
    let (slice_index, text, other_glyphs) = {
        let shared = data
            .units
            .iter()
            .find_map(|unit| unit.shared_cluster.as_ref())
            .unwrap();
        let other_glyphs = data
            .units
            .iter()
            .find_map(|unit| {
                (unit.shared_cluster.is_none())
                    .then_some(&unit.kind)
                    .and_then(|kind| match kind {
                        UnitKind::Cluster { glyphs, .. } => Some(glyphs.clone()),
                        _ => None,
                    })
            })
            .unwrap();
        (shared.slices[0], shared.text.clone(), other_glyphs)
    };
    let mut first = data.units[slice_index].clone();
    first.text = text;
    first.shared_cluster = None;
    first.slice_advance = crate::geometry::LayoutUnit::ZERO;
    let mut second = first.clone();
    if let UnitKind::Cluster { glyphs, .. } = &mut second.kind {
        *glyphs = other_glyphs.clone();
    }
    let first_glyphs = match &first.kind {
        UnitKind::Cluster { glyphs, .. } => glyphs.clone(),
        _ => unreachable!(),
    };
    assert_ne!(first_glyphs, other_glyphs);
    data.units = vec![first, second];

    let mut cx = LayoutContext::new();
    let mut warnings = crate::limits::WarningSink::new(None);
    super::super::reshape::initialize_slices(
        data,
        None,
        &mut cx,
        &mut warnings,
        &mut Saturation::default(),
    );
    assert_eq!(data.units.len(), 2);
    assert_eq!(data.units[0].text, data.units[1].text);
    assert!(data.units.iter().all(|unit| unit.shared_cluster.is_none()));
    let glyph_range = |unit: &crate::analysis::units::Unit| match &unit.kind {
        UnitKind::Cluster { glyphs, .. } => glyphs.clone(),
        _ => unreachable!(),
    };
    assert_ne!(glyph_range(&data.units[0]), glyph_range(&data.units[1]));
}

#[test]
fn scalar_clipped_hyphen_keeps_clipped_warning_before_hyphen_warning() {
    let (mut p, start, end) = clipped_hyphen_paragraph();
    let limits = &mut std::sync::Arc::get_mut(&mut p.data).unwrap().limits;
    limits.max_reshape_window_bytes = Some(1);
    limits.max_shaped_glyphs = Some(0);
    limits.max_warnings = Some(1);
    let mut cx = LayoutContext::new();
    cx.warnings.set_max(Some(1));
    width(
        &p.data,
        start..end,
        &AtomicSizes::EMPTY,
        &mut cx,
        &mut Saturation::default(),
    );
    let warnings = cx.take_warnings();
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0].kind, crate::limits::WarningKind::Unsupported);
    assert_eq!(
        warnings[0].message,
        "line edge glyph budget exceeded; keeping shared glyphs"
    );
    assert_eq!(warnings[1].kind, crate::limits::WarningKind::Suppressed);
}

#[test]
fn planned_clipped_hyphen_keeps_reference_warning_order() {
    let (mut p, start, end) = clipped_hyphen_paragraph();
    for window in [1, 2, 4096] {
        std::sync::Arc::get_mut(&mut p.data)
            .unwrap()
            .limits
            .max_reshape_window_bytes = Some(window);
        for glyphs in [0, 1, 2] {
            std::sync::Arc::get_mut(&mut p.data)
                .unwrap()
                .limits
                .max_shaped_glyphs = Some(glyphs);
            for warning_cap in [1, 2, 16] {
                let mut cx = LayoutContext::new();
                let mut oracle_cx = LayoutContext::new();
                cx.warnings.set_max(Some(warning_cap));
                oracle_cx.warnings.set_max(Some(warning_cap));
                let actual = super::super::plan::selected(
                    &p.data,
                    start,
                    end,
                    LayoutUnit::ZERO,
                    LayoutUnit::ZERO,
                    0,
                    None,
                    LayoutUnit::MAX,
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut Saturation::default(),
                );
                let expected = super::super::plan::reference_selected_raw(
                    &p.data,
                    start,
                    end,
                    LayoutUnit::ZERO,
                    LayoutUnit::ZERO,
                    0,
                    None,
                    LayoutUnit::MAX,
                    &AtomicSizes::EMPTY,
                    &mut oracle_cx,
                    &mut Saturation::default(),
                );
                let warnings = cx.take_warnings();
                assert!(!warnings.is_empty());
                assert_eq!(
                    warnings,
                    oracle_cx.take_warnings(),
                    "glyphs={glyphs} cap={warning_cap}"
                );
                assert_eq!(
                    (actual.content, actual.widths),
                    (expected.content, expected.widths)
                );
            }
        }
    }
}

#[test]
fn transparent_suffix_obstruction_work_stays_linear() {
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            },
            ..Default::default()
        },
        &Limits::default(),
    );
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "a ");
    for node in 2..1026 {
        builder.push_out_of_flow(NodeId(node), crate::node::OutOfFlowKind::Absolute);
    }
    let fonts = FontCollection::new(&Limits::default());
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    super::super::whitespace::OBSTRUCTION_VISITS.with(|visits| visits.set(0));
    let mut cx = LayoutContext::new();
    let crate::LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(1000.0),
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected a line")
    };
    assert_eq!(line.break_token().unit as usize, p.data.units.len());
    let visits = super::super::whitespace::OBSTRUCTION_VISITS.with(std::cell::Cell::get);
    assert!(
        visits <= p.data.units.len() * 4,
        "{visits} obstruction visits"
    );
}

#[test]
fn planned_selections_release_previous_paragraph_indexes() {
    let mut cx = LayoutContext::new();
    let fonts = FontCollection::new(&Limits::default());
    let mut atomics = AtomicSizes::new();
    for node in 0..16 {
        atomics.insert(
            NodeId(node),
            AtomicSize {
                inline_size: 10.0,
                ..Default::default()
            },
        );
    }
    for node in 0..16 {
        let mut builder = ParagraphBuilder::new(&Default::default(), &Limits::default());
        builder.push_atomic(NodeId(node), &Default::default(), InlineEdges::default());
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let scan = super::super::plan::selected(
            &p.data,
            0,
            p.data.units.len(),
            LayoutUnit::ZERO,
            LayoutUnit::ZERO,
            0,
            None,
            LayoutUnit::MAX,
            &atomics,
            &mut cx,
            &mut Saturation::default(),
        );
        assert_eq!(scan.content.to_f32(), 10.0);
        assert_eq!(
            width(
                &p.data,
                0..p.data.units.len(),
                &atomics,
                &mut cx,
                &mut Saturation::default()
            )
            .to_f32(),
            10.0
        );
        assert_eq!(
            cx.ruby_ranges.caches.sets.len(),
            1,
            "previous paragraph retained"
        );
    }
}

#[test]
fn retained_float_cursor_survives_range_index_shrink() {
    let fonts = FontCollection::new(&Limits::default());
    let mut builder = ParagraphBuilder::new(&Default::default(), &Limits::default());
    builder.push_text(TextSource::Generated { node: NodeId(1) }, "a ");
    builder.push_out_of_flow(NodeId(2), crate::node::OutOfFlowKind::Float);
    builder.push_text(TextSource::Generated { node: NodeId(3) }, "b ");
    builder.push_out_of_flow(NodeId(4), crate::node::OutOfFlowKind::Float);
    builder.push_text(TextSource::Generated { node: NodeId(5) }, "c");
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut cx = LayoutContext::new();
    let mut constraint = crate::LineConstraint::new(1000.0);
    for _ in 0..2 {
        let crate::LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
            &mut cx,
            p.start_token(),
            &Default::default(),
            &constraint,
            &AtomicSizes::EMPTY,
        ) else {
            panic!("expected a float")
        };
        constraint.floats_placed_through = Some(float_cursor);
    }
    assert!(cx.partial.is_some());
    cx.shrink_to(usize::MAX);
    assert!(cx.partial.is_some());
    let crate::LineResult::Line(actual) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected a line")
    };
    let crate::LineResult::Line(expected) = p.next_line(
        &mut LayoutContext::new(),
        p.start_token(),
        &Default::default(),
        &constraint,
        &AtomicSizes::EMPTY,
    ) else {
        panic!("expected a fresh line")
    };
    assert_eq!(
        (actual.text_range(), actual.inline_size()),
        (expected.text_range(), expected.inline_size())
    );
}

#[test]
fn tab_positions_use_indexed_prefix_after_saturation_and_negative_margins() {
    let root = InlineStyle {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        text_wrap_mode: crate::style::TextWrapMode::NoWrap,
        tab_size: TabSize::Px(40.0),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: root.clone(),
            ..Default::default()
        },
        &Limits::default(),
    );
    for node in 1..=3 {
        builder.push_atomic(NodeId(node), &root, InlineEdges::default());
    }
    builder.push_text(TextSource::Generated { node: NodeId(4) }, "\t");
    let fonts = FontCollection::new(&Limits::default());
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let mut atomics = AtomicSizes::new();
    for node in 1..=2 {
        atomics.insert(
            NodeId(node),
            AtomicSize {
                inline_size: 10_000_000.0,
                margins: crate::node::Sides {
                    inline_start: 10_000_000.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
    }
    atomics.insert(
        NodeId(3),
        AtomicSize {
            margins: crate::node::Sides {
                inline_start: -10_000_000.0,
                inline_end: -10_000_000.0,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut cx = LayoutContext::new();
    let crate::LineResult::Line(line) = p.next_line(
        &mut cx,
        p.start_token(),
        &Default::default(),
        &crate::LineConstraint::new(f32::MAX),
        &atomics,
    ) else {
        panic!("expected a line")
    };
    assert_eq!(line.inline_size(), 20_000_040.0);
    assert_eq!(
        width(
            &p.data,
            0..p.data.units.len(),
            &atomics,
            &mut cx,
            &mut Saturation::default()
        )
        .to_f32(),
        20_000_040.0
    );
}

#[test]
fn scanned_clipped_cluster_selection_keeps_original_shaping_effects() {
    let fonts = crate::test_support::fonts::collection(&Limits::default());
    fonts
        .register_face(
            crate::test_support::fonts::LATIN.to_vec(),
            0,
            FontFaceDescriptor {
                family: "Latin".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("Latin".into())],
        overflow_wrap: OverflowWrap::Anywhere,
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..Default::default()
    };
    let mut builder = ParagraphBuilder::new(
        &ParagraphStyle {
            root: style,
            ..Default::default()
        },
        &Limits::default(),
    );
    for (node, text) in [(1, "f"), (2, "f"), (3, "i"), (4, "\tAV x")] {
        builder.push_text(TextSource::Generated { node: NodeId(node) }, text);
    }
    let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
    let start = p
        .data
        .units
        .iter()
        .position(|u| {
            u.shared_cluster
                .as_ref()
                .is_some_and(|shared| u.text.start > shared.text.start)
        })
        .unwrap();
    let end = p.data.units.len();
    let mut cx = LayoutContext::new();
    let mut sat = Saturation::default();
    let mut indexed = Cursor::new(
        &p.data,
        start,
        LayoutUnit::ZERO,
        &AtomicSizes::EMPTY,
        &mut cx,
    );
    let mut widths = Vec::new();
    for at in start..end {
        let w = indexed.advance(&p.data, at, &AtomicSizes::EMPTY, &mut cx, &mut sat);
        widths.push(w);
    }
    let scan = indexed.selected(
        &p.data,
        end,
        widths,
        LayoutUnit::ZERO,
        0,
        Some(&Default::default()),
        LayoutUnit::MAX,
        None,
        &mut cx,
        &mut sat,
    );
    let mut oracle_cx = LayoutContext::new();
    let mut oracle_sat = Saturation::default();
    let oracle = super::super::plan::reference_selected_raw(
        &p.data,
        start,
        end,
        LayoutUnit::ZERO,
        LayoutUnit::ZERO,
        0,
        Some(&Default::default()),
        LayoutUnit::MAX,
        &AtomicSizes::EMPTY,
        &mut oracle_cx,
        &mut oracle_sat,
    );
    assert_eq!(
        (scan.content, scan.widths, scan.hang_start),
        (oracle.content, oracle.widths, oracle.hang_start)
    );
    assert_eq!(cx.edge_reshape_spent, oracle_cx.edge_reshape_spent);
    assert_eq!(cx.take_warnings(), oracle_cx.take_warnings());
    assert_eq!(sat, oracle_sat);
}
