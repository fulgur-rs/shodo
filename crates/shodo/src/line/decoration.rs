use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::style::BoxDecorationBreak;

pub(super) fn chain(data: &ParagraphData, boundary: usize) -> Vec<u32> {
    let mut parent = data.units.get(boundary).and_then(|u| match u.kind {
        UnitKind::Close { box_index } => Some(box_index),
        _ => u.parent_box,
    });
    let mut chain = Vec::new();
    while let Some(b) = parent {
        chain.push(b);
        parent = data.boxes[b as usize].parent;
    }
    chain
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
    chain(data, boundary)
        .into_iter()
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
