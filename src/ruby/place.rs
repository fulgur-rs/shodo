//! Materialize each accepted source lane once, then use shared block geometry.
use super::align::AnnotationAlign;
use super::geometry::Frame;
use super::measure::RubyMeasure;
use crate::geometry::{Direction, LayoutUnit, Saturation};
use crate::output::ruby::RubyAnnotationRecord;
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext, Line, RubyTransform};
use std::collections::HashMap;
use std::ops::Range;

fn selected_columns(
    fragment: &super::measure::RubyFragmentMeasure,
    columns: Range<usize>,
) -> Range<usize> {
    let first = fragment
        .bases
        .iter()
        .position(|b| !b.is_empty())
        .unwrap_or(columns.start);
    let last = fragment
        .bases
        .iter()
        .rposition(|b| !b.is_empty())
        .map_or(columns.end, |i| i + 1);
    columns.start.max(first)..columns.end.min(last)
}

fn span_width(
    data: &ParagraphData,
    fragment: &super::measure::RubyFragmentMeasure,
    columns: &Range<usize>,
    sat: &mut Saturation,
) -> LayoutUnit {
    let base = columns.clone().fold(LayoutUnit::ZERO, |w, i| {
        w.add(fragment.base_columns[i], sat)
    });
    base.add(
        super::measure::internal_cross(
            data,
            &data.ruby.containers[fragment.container],
            &fragment.cross_columns,
            columns,
            sat,
        ),
        sat,
    )
}

pub(crate) fn format(
    data: &ParagraphData,
    measure: &RubyMeasure,
    line: &mut Line,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    let box_origins: HashMap<_, _> = line
        .fragments
        .iter()
        .filter_map(|r| match r.kind {
            crate::line::fragments::RecordKind::InlineBox { box_index, .. } => {
                Some((box_index, r.inline_start.to_f32()))
            }
            _ => None,
        })
        .collect();
    let frame = Frame::new(
        data,
        line.units.start as usize..line.units.end as usize,
        &line.fragments,
        &line.overlay_runs,
        line.baseline,
        &line.block_shifts,
        line.block_size,
    );
    let mut retained = Vec::with_capacity(measure.fragments.len());
    let mut heights = Vec::with_capacity(measure.fragments.len());
    for fragment in &measure.fragments {
        let ruby = &data.ruby.containers[fragment.container];
        let mut origins = Vec::with_capacity(ruby.columns.len());
        let mut fallback = ruby
            .box_index
            .and_then(|b| box_origins.get(&b).copied())
            .unwrap_or_else(|| {
                line.fragments
                    .first()
                    .map_or(0.0, |r| r.inline_start.to_f32())
            });
        let rtl = line.used_direction() == Direction::Rtl;
        for (i, (column, selected)) in ruby.columns.iter().zip(&fragment.bases).enumerate() {
            let raw_origin = column
                .box_index
                .and_then(|b| box_origins.get(&b).copied())
                .unwrap_or(fallback);
            let content_origin = raw_origin
                + if rtl {
                    fragment.cross_columns[i].to_f32()
                } else {
                    0.0
                };
            origins.push(content_origin);
            if !selected.is_empty() {
                fallback = raw_origin + fragment.columns[i].to_f32();
            }
        }
        let mut merged = HashMap::new();
        let mut level_lanes = vec![Vec::new(); ruby.levels.len()];
        for lane in &fragment.lanes {
            level_lanes[ruby.lanes[lane.lane].level].push(lane);
        }
        for (level, style) in ruby.levels.iter().enumerate() {
            if !super::measure::merging(data, *style) {
                continue;
            }
            let lanes = &level_lanes[level];
            let natural = lanes
                .iter()
                .fold(LayoutUnit::ZERO, |w, l| w.add(l.width, sat));
            let columns = selected_columns(fragment, 0..fragment.bases.len());
            let width = span_width(data, fragment, &columns, sat);
            let counts: Vec<_> = lanes
                .iter()
                .map(|l| super::align::count(&ruby.lanes[l.lane].paragraph.data, l.units.clone()))
                .collect();
            let gaps = super::align::gaps(
                style.align,
                counts.iter().sum(),
                width.sub(natural, sat).max(LayoutUnit::ZERO),
            );
            let mut pen = origins[columns.start];
            let mut cursor = 0;
            for (lane, count) in lanes.iter().zip(counts) {
                let slice = gaps[cursor..cursor + count].to_vec();
                cursor += count;
                let width = slice.iter().fold(lane.width, |w, (before, after)| {
                    w.add(*before, sat).add(*after, sat)
                });
                merged.insert(lane.lane, (pen, width.to_f32(), slice));
                pen += width.to_f32();
            }
        }
        let mut children = Vec::with_capacity(fragment.lanes.len());
        let mut child_heights = Vec::with_capacity(fragment.lanes.len());
        let mut cross_used = vec![LayoutUnit::ZERO; fragment.bases.len()];
        for measured in &fragment.lanes {
            let lane = &ruby.lanes[measured.lane];
            let style = ruby.levels[lane.level];
            let columns = selected_columns(fragment, lane.columns.clone());
            let cross = super::measure::inter_character(data, style);
            let right = super::measure::rightmost(data, ruby, &columns);
            let (origin, width, alignment) = if cross {
                let base = frame.column_content(fragment, right, sat);
                (
                    origins[right],
                    measured.width.max(base.height(sat)).to_f32(),
                    AnnotationAlign::Policy(style.align),
                )
            } else {
                let width = span_width(data, fragment, &columns, sat).to_f32();
                merged.remove(&measured.lane).map_or_else(
                    || {
                        (
                            origins[columns.start],
                            width,
                            AnnotationAlign::Policy(style.align),
                        )
                    },
                    |(origin, width, gaps)| (origin, width, AnnotationAlign::Gaps(gaps)),
                )
            };
            let child =
                lane.paragraph
                    .ruby_line(cx, measured.units.clone(), width, atomics, alignment);
            let child_height = child.block_size;
            child_heights.push(child_height);
            let inline = if cross {
                let prior = cross_used[right];
                cross_used[right] = prior.add(child_height, sat);
                if rtl {
                    origin - prior.to_f32() - child_height.to_f32()
                } else {
                    origin
                        + fragment.base_columns[right].to_f32()
                        + prior.to_f32()
                        + child_height.to_f32()
                }
            } else {
                origin + (width - child.inline_size()) / 2.0
            };
            let base_text = columns
                .clone()
                .flat_map(|i| fragment.bases[i].clone())
                .fold(None, |range, i| {
                    let text = &data.units[i].text;
                    Some(
                        range.map_or(text.start as usize..text.end as usize, |r: Range<usize>| {
                            r.start.min(text.start as usize)..r.end.max(text.end as usize)
                        }),
                    )
                })
                .unwrap_or(0..0);
            let transform = if cross {
                RubyTransform {
                    inline_inline: 0.0,
                    inline_block: if rtl { 1.0 } else { -1.0 },
                    block_inline: if child.used_direction() == Direction::Ltr {
                        1.0
                    } else {
                        -1.0
                    },
                    block_block: 0.0,
                    inline_offset: inline,
                    block_offset: 0.0,
                }
            } else {
                RubyTransform {
                    inline_inline: 1.0,
                    inline_block: 0.0,
                    block_inline: 0.0,
                    block_block: 1.0,
                    inline_offset: inline,
                    block_offset: 0.0,
                }
            };
            children.push(RubyAnnotationRecord {
                container: ruby.node,
                base_nodes: columns.filter_map(|i| ruby.columns[i].node).collect(),
                node: lane.node,
                level: lane.level,
                base_text,
                paragraph: lane.paragraph.clone(),
                transform,
                line: child,
                visibility: lane.visibility,
            });
        }
        retained.push(children);
        heights.push(child_heights);
    }
    let layout = super::geometry::layout(&frame, measure, &heights, sat);
    let mut annotations = Vec::new();
    for (children, blocks) in retained.into_iter().zip(layout.lanes) {
        for (mut child, block) in children.into_iter().zip(blocks) {
            let reverse_inline =
                block.inline_size.is_some() && child.line.used_direction() == Direction::Rtl;
            child.transform.block_offset = block.block.add(layout.shift, sat).to_f32()
                + if reverse_inline {
                    child.line.inline_size()
                } else {
                    0.0
                };
            annotations.push(child);
        }
    }
    line.baseline = line.baseline.add(layout.shift, sat);
    line.block_size = layout.advance;
    line.ruby = annotations;
}
