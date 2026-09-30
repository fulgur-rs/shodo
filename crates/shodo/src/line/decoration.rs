use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::style::BoxDecorationBreak;

#[cfg(test)]
std::thread_local! {
    static CHAIN_VECS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn ancestors(data: &ParagraphData, boundary: usize) -> impl Iterator<Item = u32> + '_ {
    let mut parent = data.units.get(boundary).and_then(|u| match u.kind {
        UnitKind::Close { box_index } => Some(box_index),
        _ => u.parent_box,
    });
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
    ancestors(data, boundary)
        .filter(|b| cloned(data, *b))
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
        let style = ParagraphStyle::default();
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
            inline.box_decoration_break = if i % 2 == 0 {
                BoxDecorationBreak::Clone
            } else {
                BoxDecorationBreak::Slice
            };
            let mut edges = InlineEdges::default();
            edges.margin.inline_start = -0.1;
            edges.padding.inline_start = 0.2;
            edges.margin.inline_end = -0.2;
            edges.padding.inline_end = 0.3;
            builder.open_inline(NodeId(i as u64 + 1), &inline, edges);
        }
        builder.push_text(TextSource::Generated { node: NodeId(1000) }, "abc");
        for _ in 0..depth {
            builder.close_inline();
        }
        builder.build(&mut LayoutContext::new(), &fonts).unwrap()
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
