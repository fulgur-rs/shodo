use super::fragments::{FragmentRecord, RecordKind};
use crate::analysis::units::UnitKind;
use crate::geometry::{BaselineKind, LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::style::{InlineStyle, LineHeight, VerticalAlign};
use std::collections::HashMap;
use std::ops::Range;

pub(crate) struct LineMetrics {
    pub(crate) baseline: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) shifts: Vec<LayoutUnit>,
    pub(crate) empty: bool,
}

fn extents(s: &InlineStyle) -> (f32, f32) {
    let a = 0.8 * s.font_size;
    let d = 0.2 * s.font_size;
    let h = match s.line_height {
        LineHeight::Normal => a + d,
        LineHeight::Px(v) => v,
        LineHeight::Number(n) => n * s.font_size,
    };
    let lead = (h - a - d) / 2.0;
    (a + lead, d + lead)
}

fn shift(s: &InlineStyle, parent: &InlineStyle, a: f32, d: f32) -> f32 {
    match s.vertical_align {
        VerticalAlign::Length(v) => -v,
        VerticalAlign::Sub => parent.font_size * 0.2,
        VerticalAlign::Super => -parent.font_size * 0.3,
        VerticalAlign::TextTop => a - parent.font_size * 0.8,
        VerticalAlign::TextBottom => parent.font_size * 0.2 - d,
        VerticalAlign::Middle => (a - d) / 2.0 - parent.font_size * 0.25,
        _ => 0.0,
    }
}

// Return the baseline displacement and the top/bottom-aligned ancestor.
fn box_shift(
    data: &ParagraphData,
    box_: u32,
    cache: &mut HashMap<u32, (f32, Option<u32>)>,
) -> (f32, Option<u32>) {
    if let Some(v) = cache.get(&box_) {
        return *v;
    }
    let b = &data.boxes[box_ as usize];
    let s = &data.styles[b.style as usize];
    let (base, group) = b.parent.map_or((0.0, None), |p| box_shift(data, p, cache));
    let parent = b.parent.map_or(&data.styles[0], |p| {
        &data.styles[data.boxes[p as usize].style as usize]
    });
    let (a, d) = extents(s);
    let group = group.or_else(|| {
        matches!(s.vertical_align, VerticalAlign::Top | VerticalAlign::Bottom).then_some(box_)
    });
    let value = (base + shift(s, parent, a, d), group);
    cache.insert(box_, value);
    value
}

pub(crate) fn measure(
    data: &ParagraphData,
    units: Range<usize>,
    records: &[FragmentRecord],
    sat: &mut Saturation,
) -> LineMetrics {
    let root = &data.styles[0];
    let (mut above, mut below) = extents(root);
    let mut cache = HashMap::new();
    let mut parents = HashMap::new();
    let mut atomic_styles = HashMap::new();
    for u in &data.units[units] {
        parents.insert(u.item, u.parent_box);
        if let UnitKind::Atomic { node } = u.kind {
            atomic_styles.insert(node, (u.item, u.parent_box));
        }
    }
    let mut shifts = Vec::with_capacity(records.len());
    let mut groups: HashMap<u32, (f32, f32)> = HashMap::new();
    let mut own_groups: HashMap<usize, (f32, f32, bool)> = HashMap::new();
    let mut memberships = Vec::with_capacity(records.len());
    let mut empty = true;
    for (i, r) in records.iter().enumerate() {
        let (a, d, base, group, own_group) = match &r.kind {
            RecordKind::Glyphs { item, .. } => {
                empty = false;
                let s = &data.styles[data.items[*item as usize].style as usize];
                let (a, d) = extents(s);
                let (base, group) = parents
                    .get(item)
                    .copied()
                    .flatten()
                    .map_or((0.0, None), |b| box_shift(data, b, &mut cache));
                (a, d, base, group, None)
            }
            RecordKind::InlineBox {
                box_index,
                start_edge,
                end_edge,
                ..
            } => {
                let b = &data.boxes[*box_index as usize];
                let s = &data.styles[b.style as usize];
                let e = b.edges;
                empty &= !((*start_edge && (e.inline_start_total() != 0.0))
                    || (*end_edge && (e.inline_end_total() != 0.0))
                    || e.border.block_start != 0.0
                    || e.border.block_end != 0.0
                    || e.padding.block_start != 0.0
                    || e.padding.block_end != 0.0);
                let (a, d) = extents(s);
                let (base, group) = box_shift(data, *box_index, &mut cache);
                (a, d, base, group, None)
            }
            RecordKind::Atomic { node, size } => {
                empty = false;
                let (item, parent_box) = atomic_styles[node];
                let s = &data.styles[data.items[item as usize].style as usize];
                let parent = parent_box.map_or(root, |p| {
                    &data.styles[data.boxes[p as usize].style as usize]
                });
                let (base, group) =
                    parent_box.map_or((0.0, None), |b| box_shift(data, b, &mut cache));
                let height = size.block_size + size.margins.block_start + size.margins.block_end;
                let central = data
                    .baselines
                    .iter()
                    .any(|(n, k)| n == node && *k == BaselineKind::Central);
                let baseline = size
                    .baseline
                    .unwrap_or(if central { height / 2.0 } else { height });
                let dominant_shift = if central {
                    -0.3 * parent.font_size
                } else {
                    0.0
                };
                let own = if group.is_none() {
                    match s.vertical_align {
                        VerticalAlign::Top => Some(false),
                        VerticalAlign::Bottom => Some(true),
                        _ => None,
                    }
                } else {
                    None
                };
                (
                    baseline,
                    height - baseline,
                    base + dominant_shift
                        + shift(
                            s,
                            parent,
                            baseline - dominant_shift,
                            height - baseline + dominant_shift,
                        ),
                    group,
                    own,
                )
            }
            RecordKind::Anchor { .. } => {
                shifts.push(0.0);
                memberships.push((None, None));
                continue;
            }
        };
        let top = base - a;
        let bottom = base + d;
        if let Some(g) = group {
            let v = groups.entry(g).or_insert((top, bottom));
            v.0 = v.0.min(top);
            v.1 = v.1.max(bottom);
        } else if let Some(bottom_align) = own_group {
            own_groups.insert(i, (top, bottom, bottom_align));
        } else {
            above = above.max(-top);
            below = below.max(bottom);
        }
        shifts.push(base);
        memberships.push((group, own_group));
    }
    // Negative half-leading is meaningful: a zero-height strut can still
    // have a positive ascent and an equally negative descent.
    let mut height = (above + below).max(0.0);
    for (top, bottom) in groups.values() {
        height = height.max(bottom - top);
    }
    for (top, bottom, _) in own_groups.values() {
        height = height.max(bottom - top);
    }
    // Bottom aligned content may require room above the root strut.
    let bottom_height = groups
        .iter()
        .filter(|(b, _)| {
            data.styles[data.boxes[**b as usize].style as usize].vertical_align
                == VerticalAlign::Bottom
        })
        .map(|(_, v)| v.1 - v.0)
        .chain(own_groups.values().filter(|v| v.2).map(|v| v.1 - v.0))
        .fold(0.0_f32, f32::max);
    if height > above + below {
        above = above.max(bottom_height - below);
    }
    let mut deltas = HashMap::new();
    for (g, (top, bottom)) in groups {
        let align = data.styles[data.boxes[g as usize].style as usize].vertical_align;
        deltas.insert(
            g,
            if align == VerticalAlign::Bottom {
                height - above - bottom
            } else {
                -above - top
            },
        );
    }
    for (i, (g, own)) in memberships.into_iter().enumerate() {
        if let Some(g) = g {
            shifts[i] += deltas[&g];
        }
        if own.is_some() {
            let (top, bottom, is_bottom) = own_groups[&i];
            shifts[i] += if is_bottom {
                height - above - bottom
            } else {
                -above - top
            };
        }
    }
    LineMetrics {
        baseline: LayoutUnit::from_f32_round(above, sat),
        block_size: LayoutUnit::from_f32_ceil(if empty { 0.0 } else { height }, sat),
        shifts: shifts
            .into_iter()
            .map(|v| LayoutUnit::from_f32_round(v, sat))
            .collect(),
        empty,
    }
}
