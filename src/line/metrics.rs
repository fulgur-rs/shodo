use super::font_metrics::StyleMetrics;
use super::fragments::{FragmentRecord, RecordKind};
use crate::analysis::units::UnitKind;
use crate::font::FontMetrics;
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

fn extents(
    s: &InlineStyle,
    m: FontMetrics,
    vertical: Option<crate::font::VerticalFontMetrics>,
    upright: bool,
) -> (f32, f32) {
    let (a, d, gap) = if upright {
        vertical.map_or((s.font_size / 2.0, s.font_size / 2.0, 0.0), |v| {
            (v.ascent, v.descent, v.line_gap)
        })
    } else {
        (m.ascent, m.descent, m.line_gap)
    };
    let h = match s.line_height {
        LineHeight::Normal => a + d + gap,
        LineHeight::Px(v) => v,
        LineHeight::Number(n) => n * s.font_size,
    };
    let lead = (h - a - d) / 2.0;
    (a + lead, d + lead)
}

fn shift(s: &InlineStyle, parent: StyleMetrics, parent_upright: bool, a: f32, d: f32) -> f32 {
    let (parent_over, parent_under) = if parent_upright {
        parent
            .vertical_metrics
            .map_or((parent.size / 2.0, parent.size / 2.0), |v| {
                (v.ascent, v.descent)
            })
    } else {
        (parent.metrics.ascent, parent.metrics.descent)
    };
    match s.vertical_align {
        VerticalAlign::Length(v) => -v,
        VerticalAlign::Sub => parent.metrics.subscript_offset,
        VerticalAlign::Super => -parent.metrics.superscript_offset,
        VerticalAlign::TextTop => a - parent_over,
        VerticalAlign::TextBottom => parent_under - d,
        VerticalAlign::Middle => (a - d) / 2.0 - parent.metrics.x_height / 2.0,
        _ => 0.0,
    }
}

// Return the baseline displacement and the top/bottom-aligned ancestor.
fn box_shift(
    data: &ParagraphData,
    box_: u32,
    cache: &mut HashMap<u32, (f32, Option<u32>)>,
) -> (f32, Option<u32>) {
    let mut path = Vec::new();
    let mut cursor = Some(box_);
    let mut value = (0.0, None);
    while let Some(index) = cursor {
        if let Some(cached) = cache.get(&index) {
            value = *cached;
            break;
        }
        path.push(index);
        cursor = data.boxes[index as usize].parent;
    }
    for index in path.into_iter().rev() {
        let b = &data.boxes[index as usize];
        let s = &data.styles[b.style as usize];
        let parent_style = b.parent.map_or(0, |p| data.boxes[p as usize].style) as usize;
        let parent_upright = matches!(
            data.style.writing_mode,
            crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
        ) && data.styles[parent_style].text_orientation
            != crate::style::TextOrientation::Sideways;
        let style_metrics = data.style_metrics[b.style as usize];
        let upright = matches!(
            data.style.writing_mode,
            crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
        ) && s.text_orientation != crate::style::TextOrientation::Sideways;
        let (a, d) = extents(
            s,
            style_metrics.metrics,
            style_metrics.vertical_metrics,
            upright,
        );
        let group = value.1.or_else(|| {
            matches!(s.vertical_align, VerticalAlign::Top | VerticalAlign::Bottom).then_some(index)
        });
        value = (
            value.0 + shift(s, data.style_metrics[parent_style], parent_upright, a, d),
            group,
        );
        cache.insert(index, value);
    }
    value
}

pub(crate) fn measure(
    data: &ParagraphData,
    units: Range<usize>,
    records: &[FragmentRecord],
    overlay_runs: &[crate::shape::ShapedRun],
    sat: &mut Saturation,
) -> LineMetrics {
    let root = &data.styles[0];
    let root_metrics = data.style_metrics[0];
    let root_upright = matches!(
        data.style.writing_mode,
        crate::geometry::WritingMode::VerticalRl | crate::geometry::WritingMode::VerticalLr
    ) && root.text_orientation != crate::style::TextOrientation::Sideways;
    let (mut above, mut below) = extents(
        root,
        root_metrics.metrics,
        root_metrics.vertical_metrics,
        root_upright,
    );
    let mut cache = HashMap::new();
    let mut parents = HashMap::new();
    let mut atomic_styles = HashMap::new();
    let mut empty = true;
    for u in &data.units[units] {
        empty &= !matches!(u.kind, UnitKind::Tab | UnitKind::ForcedBreak);
        parents.insert(u.item, u.parent_box);
        if let UnitKind::Atomic { node } = u.kind {
            atomic_styles.insert(node, (u.item, u.parent_box));
        }
    }
    let mut shifts = Vec::with_capacity(records.len());
    let mut groups: HashMap<u32, (f32, f32)> = HashMap::new();
    let mut own_groups: HashMap<usize, (f32, f32, bool)> = HashMap::new();
    let mut memberships = Vec::with_capacity(records.len());
    for (i, r) in records.iter().enumerate() {
        let (a, d, base, group, own_group) = match &r.kind {
            RecordKind::Glyphs {
                item, run, source, ..
            } => {
                empty = false;
                let s = &data.styles[data.items[*item as usize].style as usize];
                let shaped = match source {
                    super::fragments::GlyphSource::Overlay { run: Some(run), .. } => {
                        &overlay_runs[*run as usize]
                    }
                    _ => &data.runs[*run as usize],
                };
                let metrics = shaped
                    .instance
                    .metrics
                    .unwrap_or_else(|| data.fonts.metrics(shaped.font, shaped.font_size));
                let (a, d) = extents(
                    s,
                    metrics,
                    if shaped.orientation == crate::shape::orientation::RunOrientation::Combined {
                        None
                    } else {
                        shaped.instance.vertical_metrics
                    },
                    matches!(
                        shaped.orientation,
                        crate::shape::orientation::RunOrientation::Upright
                            | crate::shape::orientation::RunOrientation::Combined
                    ),
                );
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
                let style_metrics = data.style_metrics[b.style as usize];
                let upright = matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && s.text_orientation != crate::style::TextOrientation::Sideways;
                let (a, d) = extents(
                    s,
                    style_metrics.metrics,
                    style_metrics.vertical_metrics,
                    upright,
                );
                let (base, group) = box_shift(data, *box_index, &mut cache);
                (a, d, base, group, None)
            }
            RecordKind::Atomic { node, size, .. } => {
                empty = false;
                let (item, parent_box) = atomic_styles[node];
                let s = &data.styles[data.items[item as usize].style as usize];
                let parent_style = parent_box.map_or(0, |p| data.boxes[p as usize].style) as usize;
                let parent = data.style_metrics[parent_style];
                let parent_upright = matches!(
                    data.style.writing_mode,
                    crate::geometry::WritingMode::VerticalRl
                        | crate::geometry::WritingMode::VerticalLr
                ) && data.styles[parent_style].text_orientation
                    != crate::style::TextOrientation::Sideways;
                let (base, group) =
                    parent_box.map_or((0.0, None), |b| box_shift(data, b, &mut cache));
                let height = size.block_size + size.margins.block_start + size.margins.block_end;
                let central = data.baseline_kind(*node) == Some(BaselineKind::Central);
                let baseline = size
                    .baseline
                    .unwrap_or(if central { height / 2.0 } else { height });
                let dominant_shift = if central {
                    let (over, under) = parent
                        .vertical_metrics
                        .map_or((parent.size / 2.0, parent.size / 2.0), |v| {
                            (v.ascent, v.descent)
                        });
                    -(over - under) / 2.0
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
                            parent_upright,
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
    let block_size = LayoutUnit::from_f32_ceil(if empty { 0.0 } else { height }, sat);
    let over_baseline = LayoutUnit::from_f32_round(above, sat);
    // The over side is the physical right in vertical-lr, opposite block-start.
    // Reflect the line-relative solution into logical block coordinates once.
    let reverse_over = data.style.writing_mode == crate::geometry::WritingMode::VerticalLr;
    LineMetrics {
        baseline: if reverse_over {
            block_size.sub(over_baseline, sat)
        } else {
            over_baseline
        },
        block_size,
        shifts: shifts
            .into_iter()
            .map(|v| LayoutUnit::from_f32_round(if reverse_over { -v } else { v }, sat))
            .collect(),
        empty,
    }
}
