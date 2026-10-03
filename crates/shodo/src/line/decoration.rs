use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::style::BoxDecorationBreak;

#[cfg(test)]
std::thread_local! {
    static CHAIN_VECS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static WIDTH_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static COMMON_ANCESTOR_LINK_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn boundary_box(data: &ParagraphData, boundary: usize) -> Option<u32> {
    data.units.get(boundary).and_then(|unit| match unit.kind {
        UnitKind::Close { box_index } => Some(box_index),
        _ => unit.parent_box,
    })
}

fn ancestors(data: &ParagraphData, boundary: usize) -> impl Iterator<Item = u32> + '_ {
    let mut parent = boundary_box(data, boundary);
    std::iter::from_fn(move || {
        let b = parent?;
        parent = data.boxes[b as usize].parent;
        Some(b)
    })
}

pub(super) fn chain(data: &ParagraphData, boundary: usize) -> Vec<u32> {
    #[cfg(test)]
    CHAIN_VECS.with(|count| count.set(count.get() + 1));
    ancestors(data, boundary).collect()
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AncestorPath {
    first: Option<u32>,
    depth: Option<usize>,
}

pub(super) fn path(data: &ParagraphData, boundary: usize) -> AncestorPath {
    AncestorPath {
        first: boundary_box(data, boundary),
        depth: None,
    }
}

pub(super) fn add_shared_width(
    data: &ParagraphData,
    outer: &mut AncestorPath,
    boundary: &mut AncestorPath,
    start: bool,
    total: &mut LayoutUnit,
    sat: &mut Saturation,
) {
    let common = if outer.first == boundary.first {
        outer.first
    } else {
        let mut outer_box = outer.first;
        let mut boundary_box = boundary.first;
        let mut outer_depth = path_depth(data, outer);
        let mut boundary_depth = path_depth(data, boundary);

        while outer_depth > boundary_depth {
            outer_box = outer_box.and_then(|b| parent_box(data, b));
            outer_depth -= 1;
        }
        while boundary_depth > outer_depth {
            boundary_box = boundary_box.and_then(|b| parent_box(data, b));
            boundary_depth -= 1;
        }
        while outer_box != boundary_box {
            outer_box = outer_box.and_then(|b| parent_box(data, b));
            boundary_box = boundary_box.and_then(|b| parent_box(data, b));
        }
        outer_box
    };

    let Some(common) = common else {
        return;
    };
    let mut next = data.boxes[common as usize].nearest_clone;
    while let Some(b) = next {
        #[cfg(test)]
        WIDTH_VISITS.with(|count| count.set(count.get() + 1));

        let edges = data.boxes[b as usize].edges;
        *total = total.add(
            LayoutUnit::from_f32_round(
                if start {
                    edges.inline_start_total()
                } else {
                    edges.inline_end_total()
                },
                sat,
            ),
            sat,
        );
        next = parent_box(data, b).and_then(|parent| data.boxes[parent as usize].nearest_clone);
    }
}

fn parent_box(data: &ParagraphData, box_index: u32) -> Option<u32> {
    #[cfg(test)]
    COMMON_ANCESTOR_LINK_READS.with(|count| count.set(count.get() + 1));
    data.boxes[box_index as usize].parent
}

fn path_depth(data: &ParagraphData, path: &mut AncestorPath) -> usize {
    if let Some(depth) = path.depth {
        return depth;
    }
    let mut depth = 0;
    let mut current = path.first;
    while let Some(box_index) = current {
        depth += 1;
        current = parent_box(data, box_index);
    }
    path.depth = Some(depth);
    depth
}

pub(super) fn cloned(data: &ParagraphData, b: u32) -> bool {
    data.styles[data.boxes[b as usize].style as usize].box_decoration_break
        == BoxDecorationBreak::Clone
}

pub(super) fn width(
    data: &ParagraphData,
    boundary: usize,
    start: bool,
    sat: &mut Saturation,
) -> LayoutUnit {
    let mut next = data
        .units
        .get(boundary)
        .and_then(|unit| match unit.kind {
            UnitKind::Close { box_index } => Some(box_index),
            _ => unit.parent_box,
        })
        .and_then(|b| data.boxes[b as usize].nearest_clone);
    std::iter::from_fn(move || {
        let b = next?;
        next = data.boxes[b as usize]
            .parent
            .and_then(|parent| data.boxes[parent as usize].nearest_clone);
        Some(b)
    })
    .inspect(|_| {
        #[cfg(test)]
        WIDTH_VISITS.with(|count| count.set(count.get() + 1));
    })
    .fold(LayoutUnit::ZERO, |w, b| {
        let e = data.boxes[b as usize].edges;
        w.add(
            LayoutUnit::from_f32_round(
                if start {
                    e.inline_start_total()
                } else {
                    e.inline_end_total()
                },
                sat,
            ),
            sat,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::ParagraphStyle;
    use crate::{LayoutContext, Paragraph, ParagraphBuilder};

    fn nested(depth: usize) -> Paragraph {
        nested_with(depth, Some(2))
    }

    fn nested_with(depth: usize, clone_every: Option<usize>) -> Paragraph {
        nested_first_line(depth, clone_every, None)
    }

    fn nested_first_line(
        depth: usize,
        clone_every: Option<usize>,
        first_break: Option<BoxDecorationBreak>,
    ) -> Paragraph {
        let mut style = ParagraphStyle::default();
        style.first_line = first_break.map(|decoration| crate::style::InlineStyle {
            font_size: 20.0,
            box_decoration_break: decoration,
            ..style.root.clone()
        });
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut builder = ParagraphBuilder::new(&style, &limits);
        for i in 0..depth {
            let mut inline = style.root.clone();
            inline.box_decoration_break = if clone_every.is_some_and(|every| i % every == 0) {
                BoxDecorationBreak::Clone
            } else {
                BoxDecorationBreak::Slice
            };
            let mut edges = InlineEdges::default();
            edges.margin.inline_start = -0.1;
            edges.padding.inline_start = 0.2;
            edges.margin.inline_end = -0.2;
            edges.padding.inline_end = 0.3;
            if let Some(decoration) = first_break {
                let alternate = crate::style::InlineStyle {
                    font_size: 20.0,
                    box_decoration_break: decoration,
                    ..inline.clone()
                };
                builder.open_inline_with_first_line(
                    NodeId(i as u64 + 1),
                    &inline,
                    &alternate,
                    edges,
                );
            } else {
                builder.open_inline(NodeId(i as u64 + 1), &inline, edges);
            }
        }
        builder.push_text(TextSource::Generated { node: NodeId(1000) }, "abc");
        for _ in 0..depth {
            builder.close_inline();
        }
        builder.build(&mut LayoutContext::new(), &fonts).unwrap()
    }

    fn legacy_shared_width(
        data: &ParagraphData,
        scope_start: usize,
        boundary: usize,
        start: bool,
        sat: &mut Saturation,
    ) -> (LayoutUnit, usize) {
        let mut width = LayoutUnit::ZERO;
        let work = legacy_shared_width_into(data, scope_start, boundary, start, &mut width, sat);
        (width, work)
    }

    fn legacy_shared_width_into(
        data: &ParagraphData,
        scope_start: usize,
        boundary: usize,
        start: bool,
        width: &mut LayoutUnit,
        sat: &mut Saturation,
    ) -> usize {
        let outer = chain(data, scope_start);
        let boundary = chain(data, boundary);
        let mut work = outer.len() + boundary.len();
        for b in boundary {
            let common = outer.iter().any(|outer| {
                work += 1;
                *outer == b
            });
            if common && cloned(data, b) {
                let edges = data.boxes[b as usize].edges;
                *width = width.add(
                    LayoutUnit::from_f32_round(
                        if start {
                            edges.inline_start_total()
                        } else {
                            edges.inline_end_total()
                        },
                        sat,
                    ),
                    sat,
                );
            }
        }
        work
    }

    fn shared_width(
        data: &ParagraphData,
        outer: &mut AncestorPath,
        boundary: &mut AncestorPath,
        start: bool,
        sat: &mut Saturation,
    ) -> LayoutUnit {
        let mut total = LayoutUnit::ZERO;
        add_shared_width(data, outer, boundary, start, &mut total, sat);
        total
    }

    fn nested_boundaries(data: &ParagraphData, depth: usize) -> (usize, usize, usize) {
        let scope_start = data
            .units
            .iter()
            .position(|unit| matches!(unit.kind, UnitKind::Open { box_index } if box_index as usize == depth / 2))
            .unwrap();
        let text = data
            .units
            .iter()
            .position(|unit| matches!(unit.kind, UnitKind::Cluster { .. }))
            .unwrap();
        let close = data
            .units
            .iter()
            .position(|unit| matches!(unit.kind, UnitKind::Close { .. }))
            .unwrap();
        (scope_start, text, close)
    }

    #[test]
    fn shared_width_matches_legacy_for_nested_mixed_and_first_line_paths() {
        for (depth, clone_every) in [
            (1, None),
            (16, None),
            (64, None),
            (16, Some(1)),
            (64, Some(2)),
            (64, Some(64)),
        ] {
            let p = nested_first_line(depth, clone_every, Some(BoxDecorationBreak::Clone));
            let mut datasets = vec![&*p.data];
            if let Some(first_line) = &p.data.first_line {
                datasets.push(&first_line.data);
            }
            for data in datasets {
                let (scope_start, text, close) = nested_boundaries(data, depth);
                for boundary in [text, close] {
                    let mut outer = path(data, scope_start);
                    let mut selected = path(data, boundary);
                    for start in [true, false] {
                        let mut expected_sat = Saturation::default();
                        let (expected, _) = legacy_shared_width(
                            data,
                            scope_start,
                            boundary,
                            start,
                            &mut expected_sat,
                        );
                        let mut actual_sat = Saturation::default();
                        let actual =
                            shared_width(data, &mut outer, &mut selected, start, &mut actual_sat);
                        assert_eq!(
                            actual, expected,
                            "depth={depth} clone_every={clone_every:?}"
                        );
                        assert_eq!(actual_sat, expected_sat);
                    }
                }
            }
        }
    }

    #[test]
    fn shared_width_keeps_close_ownership_and_signed_saturation_order() {
        let mut p = nested_with(6, Some(2));
        let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
        for index in [2, 4] {
            data.boxes[index].edges.margin.inline_start = 40_000_000.0;
            data.boxes[index].edges.padding.inline_start = 0.0;
        }
        data.boxes[0].edges.margin.inline_start = -40_000_000.0;
        data.boxes[0].edges.padding.inline_start = 0.0;
        let (_, text, close) = nested_boundaries(data, 6);
        // The base starts inside the deepest wrapper, so all three Clone boxes
        // are common to the base scope and selected boundary.
        let scope_start = text;

        for boundary in [text, close] {
            let mut expected_sat = Saturation::default();
            let (expected, _) =
                legacy_shared_width(data, scope_start, boundary, true, &mut expected_sat);
            let mut actual_sat = Saturation::default();
            let actual = shared_width(
                data,
                &mut path(data, scope_start),
                &mut path(data, boundary),
                true,
                &mut actual_sat,
            );
            assert_eq!(actual, expected);
            assert_eq!(actual.raw(), -1);
            assert_eq!(actual_sat, expected_sat);
            assert_eq!(actual_sat.saturated, 4);
            assert_eq!(actual_sat.non_finite, 0);
        }
    }

    #[test]
    fn shared_width_preserves_saturation_order_across_both_selected_edges() {
        let mut p = nested_with(6, Some(2));
        let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
        for index in [2, 4] {
            data.boxes[index].edges.margin.inline_start = 40_000_000.0;
            data.boxes[index].edges.padding.inline_start = 0.0;
            data.boxes[index].edges.margin.inline_end = 40_000_000.0;
            data.boxes[index].edges.padding.inline_end = 0.0;
        }
        data.boxes[0].edges.margin.inline_start = -40_000_000.0;
        data.boxes[0].edges.padding.inline_start = 0.0;
        data.boxes[0].edges.margin.inline_end = -40_000_000.0;
        data.boxes[0].edges.padding.inline_end = 0.0;
        let (_, text, close) = nested_boundaries(data, 6);
        let scope_start = text;
        let mut expected = LayoutUnit::ZERO;
        let mut expected_sat = Saturation::default();
        for (boundary, start) in [(text, true), (close, false)] {
            legacy_shared_width_into(
                data,
                scope_start,
                boundary,
                start,
                &mut expected,
                &mut expected_sat,
            );
        }

        let mut actual = LayoutUnit::ZERO;
        let mut actual_sat = Saturation::default();
        let mut outer = path(data, scope_start);
        for (boundary, start) in [(text, true), (close, false)] {
            let mut selected = path(data, boundary);
            add_shared_width(
                data,
                &mut outer,
                &mut selected,
                start,
                &mut actual,
                &mut actual_sat,
            );
        }
        assert_eq!(expected.raw(), -1);
        assert_eq!(expected_sat.saturated, 8);
        assert_eq!(actual, expected);
        assert_eq!(actual_sat, expected_sat);
    }

    #[test]
    fn shared_width_visits_fewer_parent_links_than_legacy_membership_scans() {
        let depth = 64;
        let p = nested_with(depth, Some(2));
        let (scope_start, text, close) = nested_boundaries(&p.data, depth);
        let mut outer = path(&p.data, scope_start);
        let mut legacy_total = 0;
        let mut new_link_reads = 0;
        for boundary in [text, close] {
            for start in [true, false] {
                let mut expected_sat = Saturation::default();
                let (expected, work) =
                    legacy_shared_width(&p.data, scope_start, boundary, start, &mut expected_sat);
                legacy_total += work;
                COMMON_ANCESTOR_LINK_READS.with(|count| count.set(0));
                let actual = shared_width(
                    &p.data,
                    &mut outer,
                    &mut path(&p.data, boundary),
                    start,
                    &mut Saturation::default(),
                );
                assert_eq!(actual, expected);
                COMMON_ANCESTOR_LINK_READS.with(|count| {
                    new_link_reads += count.get();
                    assert!(
                        count.get() < work,
                        "new link reads {} should be below legacy work {work}",
                        count.get()
                    )
                });
            }
        }
        eprintln!("depth={depth} parent-link reads={new_link_reads}, legacy work={legacy_total}");
        assert!(new_link_reads * 10 < legacy_total);
    }

    #[test]
    fn shared_width_equal_paths_skip_depth_scans() {
        let depth = 64;
        let p = nested_with(depth, Some(2));
        let boundary = p
            .data
            .units
            .iter()
            .position(|unit| matches!(unit.kind, UnitKind::Cluster { .. }))
            .unwrap();
        let mut outer = path(&p.data, boundary);
        let mut selected = path(&p.data, boundary);
        COMMON_ANCESTOR_LINK_READS.with(|count| count.set(0));
        WIDTH_VISITS.with(|count| count.set(0));
        let actual = shared_width(
            &p.data,
            &mut outer,
            &mut selected,
            true,
            &mut Saturation::default(),
        );
        assert_eq!(actual.raw(), 32 * 6);
        COMMON_ANCESTOR_LINK_READS.with(|count| assert_eq!(count.get(), 32));
        WIDTH_VISITS.with(|count| assert_eq!(count.get(), 32));
    }

    #[test]
    fn width_visits_only_clone_ancestors_in_deep_slice_and_mixed_inputs() {
        // Reverting to filtering the full parent chain makes Slice and mixed
        // inputs pay for every enclosing box, even when only one contributes.
        for (depth, every, visits) in [
            (16, None, 0),
            (64, None, 0),
            (16, Some(1), 16),
            (64, Some(1), 64),
            (16, Some(2), 8),
            (64, Some(2), 32),
            (64, Some(64), 1),
        ] {
            let p = nested_with(depth, every);
            let text = p
                .data
                .units
                .iter()
                .position(|u| matches!(u.kind, UnitKind::Cluster { .. }))
                .unwrap();
            let close = p
                .data
                .units
                .iter()
                .position(|u| matches!(u.kind, UnitKind::Close { .. }))
                .unwrap();
            for boundary in [text, close] {
                for start in [true, false] {
                    WIDTH_VISITS.with(|count| count.set(0));
                    let mut sat = Saturation::default();
                    assert_eq!(width(&p.data, boundary, start, &mut sat).raw(), visits * 6);
                    assert!(sat.is_clean());
                    WIDTH_VISITS.with(|count| {
                        assert_eq!(
                            count.get(),
                            visits as usize,
                            "depth={depth} every={every:?} boundary={boundary}"
                        )
                    });
                }
            }
        }
    }

    #[test]
    fn first_line_clone_queries_use_the_resolved_decoration_styles() {
        // box-decoration-break is outside the first-line property subset:
        // caller-supplied alternate values must not change either width.
        for (normal, alternate, raw, visits) in [
            (None, BoxDecorationBreak::Clone, 0, 0),
            (Some(1), BoxDecorationBreak::Slice, 12, 2),
        ] {
            let p = nested_first_line(2, normal, Some(alternate));
            for data in [&*p.data, &*p.data.first_line.as_ref().unwrap().data] {
                let boundary = data
                    .units
                    .iter()
                    .position(|u| matches!(u.kind, UnitKind::Cluster { .. }))
                    .unwrap();
                WIDTH_VISITS.with(|count| count.set(0));
                assert_eq!(
                    width(data, boundary, true, &mut Saturation::default()).raw(),
                    raw
                );
                WIDTH_VISITS.with(|count| assert_eq!(count.get(), visits));
            }
        }
    }

    #[test]
    fn clone_ancestor_queries_do_not_leak_across_sibling_boxes() {
        let seed = nested(0);
        let style = ParagraphStyle::default();
        let limits = Limits::default();
        let mut builder = ParagraphBuilder::new(&style, &limits);
        for (id, decoration, text) in [
            (1, BoxDecorationBreak::Clone, "a"),
            (2, BoxDecorationBreak::Slice, "b"),
        ] {
            let inline = crate::style::InlineStyle {
                box_decoration_break: decoration,
                ..style.root.clone()
            };
            let mut edges = InlineEdges::default();
            edges.padding.inline_start = 0.1;
            builder.open_inline(NodeId(id), &inline, edges);
            builder.push_text(
                TextSource::Generated {
                    node: NodeId(id + 10),
                },
                text,
            );
            builder.close_inline();
        }
        let p = builder
            .build(&mut LayoutContext::new(), &seed.data.fonts)
            .unwrap();
        let clusters: Vec<_> = p
            .data
            .units
            .iter()
            .enumerate()
            .filter(|(_, unit)| matches!(unit.kind, UnitKind::Cluster { .. }))
            .map(|(index, _)| index)
            .collect();
        for start in [true, false] {
            let mut expected_sat = Saturation::default();
            let (expected, _) =
                legacy_shared_width(&p.data, clusters[0], clusters[1], start, &mut expected_sat);
            let mut actual_sat = Saturation::default();
            let actual = shared_width(
                &p.data,
                &mut path(&p.data, clusters[0]),
                &mut path(&p.data, clusters[1]),
                start,
                &mut actual_sat,
            );
            assert_eq!(actual, expected);
            assert_eq!(actual, LayoutUnit::ZERO);
            assert_eq!(actual_sat, expected_sat);
        }
        for (boundary, unit) in p
            .data
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| matches!(u.kind, UnitKind::Cluster { .. }))
        {
            let mut sat = Saturation::default();
            assert_eq!(
                width(&p.data, boundary, true, &mut sat).raw(),
                if unit.text.start == 0 { 6 } else { 0 }
            );
            assert!(sat.is_clean());
        }
    }

    #[test]
    fn width_does_not_materialize_ancestor_vectors_at_any_depth() {
        for depth in [1, 16, 64, 256, 512] {
            let p = nested(depth);
            CHAIN_VECS.with(|count| count.set(0));
            for boundary in 0..=p.data.units.len() {
                for start in [false, true] {
                    width(&p.data, boundary, start, &mut Saturation::default());
                }
            }
            CHAIN_VECS.with(|count| assert_eq!(count.get(), 0, "depth {depth}"));
        }
    }

    #[test]
    fn width_preserves_per_edge_rounding_and_close_ownership() {
        let p = nested(3);
        for (i, unit) in p.data.units.iter().enumerate() {
            let parent = match unit.kind {
                UnitKind::Close { box_index } => Some(box_index),
                _ => unit.parent_box,
            };
            let clones = match parent {
                Some(2) => 2,
                Some(0 | 1) => 1,
                None => 0,
                _ => unreachable!(),
            };
            for start in [false, true] {
                let mut sat = Saturation::default();
                // Each clone's 0.1 px rounds separately to 6/64 px.
                assert_eq!(width(&p.data, i, start, &mut sat).raw(), clones * 6);
                assert!(sat.is_clean());
            }
        }
        assert_eq!(
            width(
                &p.data,
                p.data.units.len(),
                true,
                &mut Saturation::default()
            ),
            LayoutUnit::ZERO
        );
    }

    #[test]
    fn width_keeps_inner_to_outer_order_when_signed_edges_saturate() {
        let mut p = nested(5);
        let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
        // Three Clone edges, separated by Slice edges, must accumulate inside-out.
        for index in [2, 4] {
            data.boxes[index].edges.margin.inline_start = 40_000_000.0;
            data.boxes[index].edges.padding.inline_start = 0.0;
        }
        data.boxes[0].edges.margin.inline_start = -40_000_000.0;
        data.boxes[0].edges.padding.inline_start = 0.0;
        let boundary = data
            .units
            .iter()
            .position(|u| matches!(u.kind, UnitKind::Cluster { .. }))
            .unwrap();
        let mut sat = Saturation::default();
        assert_eq!(width(data, boundary, true, &mut sat).raw(), -1);
        assert_eq!(sat.saturated, 4);
        assert_eq!(sat.non_finite, 0);
    }
}
