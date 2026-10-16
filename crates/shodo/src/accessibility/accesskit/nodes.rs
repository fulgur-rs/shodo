use super::positions::Span;
use super::{AccessKitError, NodeSemantics, types};
use crate::accessibility::{
    AccessibleCharacterKind, AccessibleLayout, AccessibleLine, AccessibleRun,
};
use crate::geometry::{LogicalRect, PhysicalConverter, PhysicalRect, PhysicalSize};
use crate::mapping::{Affinity, TextOrigin};
use crate::node::NodeId;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Anchor {
    Dom(NodeId, u32),
    Generated(NodeId, u32),
    Anonymous(u64, u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Text,
    Wrapper,
    Ruby {
        container: NodeId,
        node: Option<NodeId>,
        level: usize,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    anchor: Anchor,
    kind: Kind,
    occurrence: usize,
}

pub(super) struct NodeBuilder<'a, F> {
    root: types::NodeId,
    old: &'a HashMap<Key, types::NodeId>,
    occupied: HashSet<types::NodeId>,
    issued: HashSet<types::NodeId>,
    occurrences: HashMap<(Anchor, Kind), usize>,
    allocate: F,
    pub registry: HashMap<Key, types::NodeId>,
    pub nodes: Vec<(types::NodeId, types::Node)>,
    pub children: Vec<types::NodeId>,
    pub spans: Vec<Span>,
}
impl<'a, F: FnMut() -> types::NodeId> NodeBuilder<'a, F> {
    pub fn new(root: types::NodeId, old: &'a HashMap<Key, types::NodeId>, allocate: F) -> Self {
        Self {
            root,
            old,
            occupied: old.values().copied().collect(),
            issued: HashSet::new(),
            occurrences: HashMap::new(),
            allocate,
            registry: HashMap::new(),
            nodes: Vec::new(),
            children: Vec::new(),
            spans: Vec::new(),
        }
    }
    fn id(&mut self, anchor: Anchor, kind: Kind) -> Result<types::NodeId, AccessKitError> {
        let occurrence = self.occurrences.entry((anchor, kind)).or_default();
        let key = Key {
            anchor,
            kind,
            occurrence: *occurrence,
        };
        *occurrence += 1;
        let id = if let Some(id) = self.old.get(&key) {
            *id
        } else {
            let id = (self.allocate)();
            if self.occupied.contains(&id) {
                return Err(AccessKitError::DuplicateNodeId);
            }
            id
        };
        if id == self.root || !self.issued.insert(id) {
            return Err(AccessKitError::DuplicateNodeId);
        }
        self.registry.insert(key, id);
        Ok(id)
    }
    pub fn build(
        &mut self,
        layout: &AccessibleLayout<'_>,
        frame: PhysicalRect,
        semantics: impl Fn(NodeId) -> NodeSemantics,
    ) -> Result<(), AccessKitError> {
        for line in layout.lines() {
            let text_nodes_on_line = line.runs.iter().fold(0usize, |count, run| {
                count.saturating_add(run.character_range.len().div_ceil(255).max(1))
            });
            let line_node_capacity = text_nodes_on_line.saturating_add(line.runs.len());
            self.nodes.reserve(line_node_capacity);
            self.registry.reserve(line_node_capacity);
            self.issued.reserve(line_node_capacity);
            self.occurrences.reserve(line_node_capacity);
            self.children.reserve(text_nodes_on_line);
            self.spans.reserve(text_nodes_on_line);
            let converter = PhysicalConverter::new(
                line.writing_mode,
                line.direction,
                PhysicalSize {
                    width: frame.width,
                    height: frame.height,
                },
            );
            let mut on_line = Vec::with_capacity(text_nodes_on_line);
            for run in &line.runs {
                let semantic = run.node.map(&semantics).unwrap_or_default();
                let atomic = run
                    .character_range
                    .clone()
                    .any(|i| matches!(line.characters[i].kind, AccessibleCharacterKind::Atomic(_)));
                let wrapper = atomic
                    || semantic.role != types::Role::GenericContainer
                    || semantic.label.is_some()
                    || semantic.description.is_some();
                let text_node_count = run.character_range.len().div_ceil(255).max(1);
                let mut chunks = Vec::with_capacity(text_node_count);
                let mut start = run.character_range.start;
                loop {
                    let end = (start + 255).min(run.character_range.end);
                    let range = start..end;
                    let anchor = anchor(layout, line, run, start);
                    let id = self.id(anchor, Kind::Text)?;
                    let node = text_node(
                        line,
                        run,
                        range.clone(),
                        atomic.then_some(semantic.label.as_deref().unwrap_or("\u{fffc}")),
                        converter,
                        frame,
                    )?;
                    let node_index = self.nodes.len();
                    self.nodes.push((id, node));
                    chunks.push(id);
                    on_line.push(node_index);
                    self.spans.push(Span {
                        node: id,
                        line: line.index,
                        characters: range,
                    });
                    if end == run.character_range.end {
                        break;
                    }
                    start = end;
                }
                if wrapper {
                    let id = self.id(
                        anchor(layout, line, run, run.character_range.start),
                        Kind::Wrapper,
                    )?;
                    let mut node = types::Node::new(semantic.role);
                    if let Some(label) = semantic.label {
                        node.set_label(label);
                    }
                    if let Some(description) = semantic.description {
                        node.set_description(description);
                    }
                    node.set_children(chunks);
                    node.set_bounds(bounds(converter, frame, run.bounds));
                    self.nodes.push((id, node));
                    self.children.push(id);
                } else {
                    self.children.extend(chunks);
                }
            }
            for pair in on_line.windows(2) {
                let previous = self.nodes[pair[0]].0;
                let next = self.nodes[pair[1]].0;
                self.nodes[pair[0]].1.set_next_on_line(next);
                self.nodes[pair[1]].1.set_previous_on_line(previous);
            }
        }
        // Readings are explicit metadata, not TextRuns in the document's text
        // or position table. The details relation stays on the paired runs.
        if layout.ruby_annotations().next().is_none() {
            return Ok(());
        }
        let node_indices: HashMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        for relationship in layout.ruby_annotations() {
            let line = &layout.lines()[relationship.parent_line];
            let a = relationship.annotation;
            let range = a.base_text_range();
            let mut paired = Vec::new();
            let begin = line
                .characters
                .partition_point(|c| (c.text_range.end as usize) <= range.start);
            let end = line
                .characters
                .partition_point(|c| (c.text_range.start as usize) < range.end);
            let first = self.spans.partition_point(|s| {
                s.line < relationship.parent_line
                    || (s.line == relationship.parent_line && s.characters.end <= begin)
            });
            for span in self.spans[first..]
                .iter()
                .take_while(|s| s.line == relationship.parent_line && s.characters.start < end)
            {
                paired.push(node_indices[&span.node]);
            }
            let transform = crate::RubyTransform {
                inline_inline: 1.0,
                inline_block: 0.0,
                block_inline: 0.0,
                block_block: 1.0,
                inline_offset: 0.0,
                block_offset: layout.accepted[relationship.parent_line].block_offset(),
            };
            let converter = PhysicalConverter::new(
                line.writing_mode,
                line.direction,
                PhysicalSize {
                    width: frame.width,
                    height: frame.height,
                },
            );
            let id = self.reading(a, transform, converter, frame, false)?;
            for index in paired {
                self.nodes[index].1.push_detail(id);
            }
            self.children.push(id);
        }
        Ok(())
    }

    fn reading(
        &mut self,
        a: crate::RubyAnnotationView<'_>,
        parent: crate::RubyTransform,
        converter: PhysicalConverter,
        frame: PhysicalRect,
        parent_hidden: bool,
    ) -> Result<types::NodeId, AccessKitError> {
        let start = a
            .line()
            .fragments()
            .find_map(|f| match f {
                crate::Fragment::GlyphRun(r) => Some(r.text_range().start as u32),
                _ => None,
            })
            .unwrap_or(a.text_range().start as u32);
        let anchor = match a
            .line()
            .offset_mapping()
            .and_then(|m| m.text_to_dom(start, Affinity::Downstream))
        {
            Some(TextOrigin::Dom { node, offset }) => Anchor::Dom(node, offset),
            Some(TextOrigin::Generated { node }) => Anchor::Generated(node, start),
            None => Anchor::Anonymous(a.line().data.id, start),
        };
        let id = self.id(
            anchor,
            Kind::Ruby {
                container: a.container(),
                node: a.node(),
                level: a.level(),
            },
        )?;
        let transform = compose(parent, a.transform());
        let hidden = parent_hidden || a.visibility() != crate::RubyVisibility::Visible;
        let mut node = types::Node::new(types::Role::RubyAnnotation);
        let range = a.line().text_range();
        node.set_value(&a.line().text()[range]);
        node.set_bounds(bounds(
            converter,
            frame,
            transformed(transform, a.line().overflow_rect()),
        ));
        if hidden {
            node.set_hidden();
        }
        let mut children = Vec::new();
        for child in a.line().ruby_annotations() {
            children.push(self.reading(child, transform, converter, frame, hidden)?);
        }
        node.set_children(children);
        self.nodes.push((id, node));
        Ok(id)
    }
}

fn compose(p: crate::RubyTransform, c: crate::RubyTransform) -> crate::RubyTransform {
    crate::RubyTransform {
        inline_inline: p.inline_inline * c.inline_inline + p.inline_block * c.block_inline,
        inline_block: p.inline_inline * c.inline_block + p.inline_block * c.block_block,
        block_inline: p.block_inline * c.inline_inline + p.block_block * c.block_inline,
        block_block: p.block_inline * c.inline_block + p.block_block * c.block_block,
        inline_offset: p.inline_inline * c.inline_offset
            + p.inline_block * c.block_offset
            + p.inline_offset,
        block_offset: p.block_inline * c.inline_offset
            + p.block_block * c.block_offset
            + p.block_offset,
    }
}

fn transformed(t: crate::RubyTransform, r: LogicalRect) -> LogicalRect {
    let points = [
        (r.inline_start, r.block_start),
        (r.inline_start + r.inline_size, r.block_start),
        (r.inline_start, r.block_start + r.block_size),
        (r.inline_start + r.inline_size, r.block_start + r.block_size),
    ]
    .map(|(x, y)| {
        (
            t.inline_inline * x + t.inline_block * y + t.inline_offset,
            t.block_inline * x + t.block_block * y + t.block_offset,
        )
    });
    let x0 = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
    let x1 = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
    let y0 = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
    let y1 = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
    LogicalRect {
        inline_start: x0,
        block_start: y0,
        inline_size: x1 - x0,
        block_size: y1 - y0,
    }
}

fn anchor(
    layout: &AccessibleLayout<'_>,
    line: &AccessibleLine<'_>,
    run: &AccessibleRun<'_>,
    character: usize,
) -> Anchor {
    let offset = line
        .characters
        .get(character)
        .map_or(line.text_range.end, |c| c.text_range.start);
    if let Some(p) = layout.position(line.index, character, Affinity::Downstream)
        && let Some(source) = layout.to_source(p)
    {
        return match source.origin {
            TextOrigin::Dom { node, offset } => Anchor::Dom(node, offset),
            TextOrigin::Generated { node } => Anchor::Generated(node, offset),
        };
    }
    if let Some(node) = run.node {
        Anchor::Generated(node, offset)
    } else {
        Anchor::Anonymous(layout.accepted[line.index].data.id, offset)
    }
}

fn bounds(converter: PhysicalConverter, frame: PhysicalRect, rect: LogicalRect) -> types::Rect {
    let r = converter.rect(rect);
    types::Rect::new(
        f64::from(frame.x) + f64::from(r.x),
        f64::from(frame.y) + f64::from(r.y),
        f64::from(frame.x) + f64::from(r.x) + f64::from(r.width),
        f64::from(frame.y) + f64::from(r.y) + f64::from(r.height),
    )
}
fn color([red, green, blue, alpha]: [u8; 4]) -> types::Color {
    types::Color {
        red,
        green,
        blue,
        alpha,
    }
}

fn text_node(
    line: &AccessibleLine<'_>,
    run: &AccessibleRun<'_>,
    range: Range<usize>,
    alternative: Option<&str>,
    converter: PhysicalConverter,
    frame: PhysicalRect,
) -> Result<types::Node, AccessKitError> {
    let characters = &line.characters[range.clone()];
    let rect = characters
        .iter()
        .map(|c| c.rect)
        .reduce(crate::accessibility::output::union)
        .unwrap_or(run.bounds);
    let rect = bounds(converter, frame, rect);
    let mut node = types::Node::new(types::Role::TextRun);
    node.set_bounds(rect);
    let mut value_capacity = 0;
    for c in characters {
        let text = if let Some(alternative) = alternative {
            alternative
        } else if c.kind == AccessibleCharacterKind::HardBreak {
            "\n"
        } else {
            c.text
        };
        u8::try_from(text.len()).map_err(|_| AccessKitError::CharacterTooLong)?;
        value_capacity += text.len();
    }
    let mut value = String::with_capacity(value_capacity);
    let mut lengths = Vec::with_capacity(characters.len());
    let mut positions = Vec::with_capacity(characters.len());
    let mut widths = Vec::with_capacity(characters.len());
    let direction = characters
        .iter()
        .find_map(|c| {
            let a = converter.point(c.leading.0, c.leading.1);
            let b = converter.point(c.trailing.0, c.trailing.1);
            if b.0 > a.0 {
                Some(types::TextDirection::LeftToRight)
            } else if b.0 < a.0 {
                Some(types::TextDirection::RightToLeft)
            } else if b.1 > a.1 {
                Some(types::TextDirection::TopToBottom)
            } else if b.1 < a.1 {
                Some(types::TextDirection::BottomToTop)
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            let sign = if run.bidi_level % 2 == layout_base_level(line) % 2 {
                1.0
            } else {
                -1.0
            };
            let (x, y) = converter.vector(sign, 0.0);
            if x > 0.0 {
                types::TextDirection::LeftToRight
            } else if x < 0.0 {
                types::TextDirection::RightToLeft
            } else if y > 0.0 {
                types::TextDirection::TopToBottom
            } else {
                types::TextDirection::BottomToTop
            }
        });
    for c in characters {
        let text = if let Some(alternative) = alternative {
            alternative
        } else if c.kind == AccessibleCharacterKind::HardBreak {
            "\n"
        } else {
            c.text
        };
        lengths.push(u8::try_from(text.len()).map_err(|_| AccessKitError::CharacterTooLong)?);
        value.push_str(text);
        let c_rect = bounds(converter, frame, c.rect);
        let (position, width) = match direction {
            types::TextDirection::LeftToRight => (c_rect.x0 - rect.x0, c_rect.width()),
            types::TextDirection::RightToLeft => (rect.x1 - c_rect.x1, c_rect.width()),
            types::TextDirection::TopToBottom => (c_rect.y0 - rect.y0, c_rect.height()),
            types::TextDirection::BottomToTop => (rect.y1 - c_rect.y1, c_rect.height()),
        };
        positions.push(position as f32);
        widths.push(width as f32);
    }
    node.set_value(value);
    node.set_character_lengths(lengths);
    node.set_character_positions(positions);
    node.set_character_widths(widths);
    node.set_text_direction(direction);
    // Output construction sorts and deduplicates word starts. Locate this
    // chunk's interval without scanning the other chunks' boundaries.
    let first_word = line.word_starts.partition_point(|&i| {
        #[cfg(test)]
        word_work::visit();
        i < range.start
    });
    let last_word = line.word_starts.partition_point(|&i| {
        #[cfg(test)]
        word_work::visit();
        i < range.end
    });
    let mut word_starts = Vec::with_capacity(last_word - first_word);
    word_starts.extend(line.word_starts[first_word..last_word].iter().map(|i| {
        #[cfg(test)]
        word_work::visit();
        (i - range.start) as u8
    }));
    node.set_word_starts(word_starts);
    node.set_font_size(run.font_size);
    node.set_font_weight(run.style.font_weight);
    if let Some(lang) = &run.style.lang {
        node.set_language(lang.clone());
    }
    if !matches!(run.style.font_style, crate::style::FontStyle::Normal) {
        node.set_italic();
    }
    node.set_foreground_color(color(run.style.paint.color));
    if let Some(decoration) = run.style.paint.underline
        && decoration.thickness != Some(0.0)
    {
        node.set_underline(types::TextDecoration {
            style: types::TextDecorationStyle::Solid,
            color: color(decoration.color.unwrap_or(run.style.paint.color)),
        });
    }
    if let Some(decoration) = run.style.paint.strikethrough
        && decoration.thickness != Some(0.0)
    {
        node.set_strikethrough(types::TextDecoration {
            style: types::TextDecorationStyle::Solid,
            color: color(decoration.color.unwrap_or(run.style.paint.color)),
        });
    }
    Ok(node)
}

fn layout_base_level(line: &AccessibleLine<'_>) -> u8 {
    if line.direction == crate::geometry::Direction::Ltr {
        0
    } else {
        1
    }
}

#[cfg(test)]
pub(super) mod word_work {
    std::thread_local! {
        static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(super) fn visit() {
        VISITS.with(|v| v.set(v.get() + 1));
    }
    pub(crate) fn reset() {
        VISITS.with(|v| v.set(0));
    }
    pub(crate) fn visits() -> usize {
        VISITS.with(std::cell::Cell::get)
    }
}
