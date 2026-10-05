//! Shape independent annotation paragraphs and retain their source/font owners.
pub(crate) use super::budget::RubyBudget;
use super::builder::{Boundary, RubyInput};
use super::cuts::PairedCut;
use super::{RubyAlign, RubyPosition, RubyStyle, RubyVisibility};
use crate::analysis::ItemKind;
use crate::builder::{ParagraphBuilder, RawItem};
use crate::font::FontCollection;
use crate::geometry::WritingMode;
use crate::limits::{LimitExceeded, LimitKind, Limits};
use crate::node::{NodeId, TextSource};
use crate::paragraph::{Paragraph, ParagraphData};
use crate::style::{LineHeight, TextOrientation, TextWrapMode};
use std::ops::Range;

#[derive(Default)]
pub(crate) struct RubyData {
    pub(crate) containers: Vec<PreparedRuby>,
    pub(crate) intervals: super::index::ContainerIndex,
}

// Measurement/placement consume these records in Tasks 3/4.
pub(crate) struct PreparedRuby {
    pub(crate) node: NodeId,
    pub(crate) box_index: Option<u32>,
    pub(crate) units: Range<usize>,
    pub(crate) columns: Vec<PreparedBase>,
    pub(crate) levels: Vec<RubyStyle>,
    pub(crate) lanes: Vec<PreparedLane>,
    pub(crate) cuts: Vec<PairedCut>,
}

pub(crate) struct PreparedBase {
    pub(crate) node: Option<NodeId>,
    pub(crate) box_index: Option<u32>,
    pub(crate) align: RubyAlign,
    pub(crate) units: Range<usize>,
    pub(crate) text: Range<u32>,
}

#[derive(Clone)]
pub(crate) struct PreparedLane {
    pub(crate) node: Option<NodeId>,
    pub(crate) level: usize,
    pub(crate) columns: Range<usize>,
    pub(crate) visibility: RubyVisibility,
    pub(crate) paragraph: Paragraph,
}

pub(crate) fn has_first_line(inputs: &[RubyInput]) -> bool {
    inputs
        .iter()
        .flat_map(|r| &r.normalized.levels)
        .flat_map(|l| &l.annotations)
        .any(|a| {
            !a.auto_hidden
                && a.visibility != RubyVisibility::Collapse
                && a.content.as_ref().is_some_and(|c| c.0.has_first_line)
        })
}

/// Represent explicit forced-break inputs as generated segment breaks before
/// annotation-specific whitespace processing. Original DOM text ranges shift,
/// while their TextSource offsets and styles remain caller-owned.
pub(crate) fn annotation_breaks(builder: &mut ParagraphBuilder) -> Result<(), LimitExceeded> {
    let extra = builder
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                RawItem::ForcedBreak { .. } | RawItem::BlockInInline { .. }
            )
        })
        .count();
    if extra == 0 {
        return Ok(());
    }
    let bytes = (builder.text.len() as u64).saturating_add(extra as u64);
    Limits::check(Some(u32::MAX as u64), LimitKind::TextBytes, bytes)?;
    Limits::check(builder.limits.max_text_bytes, LimitKind::TextBytes, bytes)?;
    let mut text = String::with_capacity(bytes as usize);
    for item in &mut builder.items {
        match item {
            RawItem::Text { range, .. } => {
                let start = text.len() as u32;
                text.push_str(&builder.text[range.start as usize..range.end as usize]);
                *range = start..text.len() as u32;
            }
            RawItem::ForcedBreak { node, style } | RawItem::BlockInInline { node, style } => {
                let start = text.len() as u32;
                text.push('\n');
                *item = RawItem::Text {
                    source: TextSource::Generated { node: *node },
                    range: start..text.len() as u32,
                    style: *style,
                };
            }
            _ => {}
        }
    }
    builder.text = text;
    Ok(())
}

/// Index marker offsets in one pass; markers have their own transparent units.
fn containers(data: &ParagraphData, inputs: &[RubyInput]) -> Vec<PreparedRuby> {
    let mut result: Vec<_> = inputs
        .iter()
        .map(|input| PreparedRuby {
            node: input.node,
            box_index: None,
            units: 0..0,
            columns: input
                .normalized
                .bases
                .iter()
                .map(|base| PreparedBase {
                    node: base.node,
                    box_index: None,
                    align: base.align,
                    units: 0..0,
                    text: 0..0,
                })
                .collect(),
            levels: input
                .normalized
                .levels
                .iter()
                .map(|level| level.style)
                .collect(),
            lanes: Vec::new(),
            cuts: Vec::new(),
        })
        .collect();
    let mut cursor = 0;
    for (index, item) in data.items.iter().enumerate() {
        while cursor < data.units.len() && (data.units[cursor].item as usize) < index {
            cursor += 1;
        }
        if let ItemKind::RubyBoundary { ruby, boundary } = item.kind {
            let container = &mut result[ruby as usize];
            match boundary {
                Boundary::ContainerOpen => {
                    let mut start = cursor;
                    while start > 0
                        && matches!(
                            data.items[data.units[start - 1].item as usize].kind,
                            ItemKind::BidiControl
                        )
                    {
                        start -= 1;
                    }
                    if start > 0
                        && let crate::analysis::units::UnitKind::Open { box_index } =
                            data.units[start - 1].kind
                        && data.boxes[box_index as usize].node == container.node
                    {
                        start -= 1;
                    }
                    container.units.start = start;
                }
                Boundary::ContainerClose => {
                    let mut end = cursor + 1;
                    while end < data.units.len()
                        && matches!(
                            data.items[data.units[end].item as usize].kind,
                            ItemKind::BidiControl
                        )
                    {
                        end += 1;
                    }
                    if end < data.units.len()
                        && let crate::analysis::units::UnitKind::Close { box_index } =
                            data.units[end].kind
                        && data.boxes[box_index as usize].node == container.node
                    {
                        end += 1;
                    }
                    container.units.end = end;
                }
                Boundary::BaseOpen(column) => {
                    container.columns[column].units.start = cursor;
                    container.columns[column].text.start = item.text.start;
                }
                Boundary::BaseClose(column) => {
                    container.columns[column].units.end = cursor + 1;
                    container.columns[column].text.end = item.text.end;
                }
            }
        }
    }
    for ruby in &mut result {
        let first_box = |mut range: Range<usize>| {
            range.find_map(|i| match data.units[i].kind {
                crate::analysis::units::UnitKind::Open { box_index } => Some(box_index),
                _ => None,
            })
        };
        ruby.box_index = first_box(ruby.units.clone());
        for column in &mut ruby.columns {
            column.box_index = first_box(column.units.clone());
        }
    }
    result
}

pub(crate) fn prepare(
    data: &mut ParagraphData,
    inputs: &[RubyInput],
    cx: &mut crate::LayoutContext,
    fonts: &FontCollection,
    budget: &mut RubyBudget,
    bases: &mut super::base_budget::BaseScopes,
) -> Result<(), LimitExceeded> {
    if inputs.is_empty() {
        return Ok(());
    }
    for (index, input) in inputs.iter().enumerate() {
        bases.container(index, input.normalized.metadata_items)?;
        budget.charge(LimitKind::Items, input.normalized.metadata_items)?;
    }
    let mut result = containers(data, inputs);
    for (index, (container, input)) in result.iter_mut().zip(inputs).enumerate() {
        for (level, normalized) in input.normalized.levels.iter().enumerate() {
            for annotation in &normalized.annotations {
                if annotation.auto_hidden || annotation.visibility == RubyVisibility::Collapse {
                    continue;
                }
                let Some(content) = &annotation.content else {
                    continue;
                };
                let Some(node) = annotation.node else {
                    continue;
                };
                let scopes = bases.enter_container(index, budget);
                // Check ancestor remaining bytes before cloning a lane snapshot.
                let bytes = super::builder::InputCost::content(&content.0).style_bytes;
                budget.check(LimitKind::StyleBytes, bytes)?;
                let mut builder = ParagraphBuilder::from_ruby_content(&content.0, node)?;
                // Annotation line-height does not apply (CSS Ruby §3.3).
                for style in &mut builder.styles {
                    style.line_height = LineHeight::Normal;
                }
                builder.style.root.line_height = LineHeight::Normal;
                for style in builder.first_line_styles.values_mut() {
                    style.line_height = LineHeight::Normal;
                }
                if let Some(first) = builder.style.first_line.as_mut() {
                    first.line_height = LineHeight::Normal;
                }
                // Annotation boxes are inline content of the same document.
                builder.style.line_height_quirk = data.style.line_height_quirk;
                if normalized.style.position == RubyPosition::InterCharacter
                    && data.style.writing_mode == WritingMode::HorizontalTb
                {
                    builder.style.writing_mode = WritingMode::VerticalRl;
                    for style in &mut builder.styles {
                        style.text_orientation = TextOrientation::Upright;
                        style.text_wrap_mode = TextWrapMode::NoWrap;
                    }
                    builder.style.root.text_orientation = TextOrientation::Upright;
                    builder.style.root.text_wrap_mode = TextWrapMode::NoWrap;
                } else {
                    builder.style.writing_mode = data.style.writing_mode;
                }
                let paragraph =
                    Paragraph::from_builder_with_ruby_budget(builder, cx, fonts, budget);
                bases.leave_container(scopes, budget);
                let paragraph = paragraph?;
                container.lanes.push(PreparedLane {
                    node: annotation.node,
                    level,
                    columns: annotation.columns.clone(),
                    visibility: annotation.visibility,
                    paragraph,
                });
            }
        }
    }
    super::index::prepare_cuts(data, &mut result, budget, None, bases)?;
    data.ruby.intervals = super::index::ContainerIndex::new(&result, budget, bases)?;
    data.ruby.containers = result;
    Ok(())
}

/// Alternate parents share the corresponding child data, not duplicate shapes.
pub(crate) fn prepare_alternate(
    data: &mut ParagraphData,
    inputs: &[RubyInput],
    normal: &RubyData,
    budget: &mut RubyBudget,
    parent_cursors: &[Option<u32>],
    bases: &mut super::base_budget::BaseScopes,
) -> Result<(), LimitExceeded> {
    if inputs.is_empty() {
        return Ok(());
    }
    for (index, input) in inputs.iter().enumerate() {
        bases.container(index, input.normalized.metadata_items)?;
        budget.charge(LimitKind::Items, input.normalized.metadata_items)?;
    }
    let mut result = containers(data, inputs);
    for (alternate, normal) in result.iter_mut().zip(&normal.containers) {
        for lane in &normal.lanes {
            let mut lane = lane.clone();
            if let Some(first) = &lane.paragraph.data.first_line {
                lane.paragraph = Paragraph {
                    data: std::sync::Arc::clone(&first.data),
                };
            }
            alternate.lanes.push(lane);
        }
    }
    super::index::prepare_cuts(
        data,
        &mut result,
        budget,
        Some((normal, parent_cursors)),
        bases,
    )?;
    data.ruby.intervals = super::index::ContainerIndex::new(&result, budget, bases)?;
    data.ruby.containers = result;
    Ok(())
}
