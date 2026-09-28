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
pub(super) struct Key {
    anchor: Anchor,
    wrapper: bool,
    occurrence: usize,
}

pub(super) struct NodeBuilder<'a, F> {
    root: types::NodeId,
    old: &'a HashMap<Key, types::NodeId>,
    occupied: HashSet<types::NodeId>,
    issued: HashSet<types::NodeId>,
    occurrences: HashMap<(Anchor, bool), usize>,
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
    fn id(&mut self, anchor: Anchor, wrapper: bool) -> Result<types::NodeId, AccessKitError> {
        let occurrence = self.occurrences.entry((anchor, wrapper)).or_default();
        let key = Key {
            anchor,
            wrapper,
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
            let line_node_begin = self.nodes.len();
            let converter = PhysicalConverter::new(
                line.writing_mode,
                line.direction,
                PhysicalSize {
                    width: frame.width,
                    height: frame.height,
                },
            );
            let mut on_line = Vec::new();
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
                let mut chunks = Vec::new();
                let mut start = run.character_range.start;
                loop {
                    let end = (start + 255).min(run.character_range.end);
                    let range = start..end;
                    let anchor = anchor(layout, line, run, start);
                    let id = self.id(anchor, false)?;
                    let node = text_node(
                        line,
                        run,
                        range.clone(),
                        atomic.then_some(semantic.label.as_deref().unwrap_or("\u{fffc}")),
                        converter,
                        frame,
                    )?;
                    self.nodes.push((id, node));
                    chunks.push(id);
                    on_line.push(id);
                    let hard_break = range
                        .clone()
                        .find(|&i| line.characters[i].kind == AccessibleCharacterKind::HardBreak);
                    self.spans.push(Span {
                        node: id,
                        line: line.index,
                        characters: range,
                        hard_break,
                    });
                    if end == run.character_range.end {
                        break;
                    }
                    start = end;
                }
                if wrapper {
                    let id = self.id(anchor(layout, line, run, run.character_range.start), true)?;
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
            // The current nodes for this line are contiguous, except wrappers.
            // Index IDs once instead of searching the whole tree per link.
            let links: HashMap<_, _> = on_line
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    (
                        *id,
                        (
                            i.checked_sub(1).map(|j| on_line[j]),
                            on_line.get(i + 1).copied(),
                        ),
                    )
                })
                .collect();
            for (id, node) in &mut self.nodes[line_node_begin..] {
                if let Some((previous, next)) = links.get(id) {
                    if let Some(previous) = previous {
                        node.set_previous_on_line(*previous);
                    }
                    if let Some(next) = next {
                        node.set_next_on_line(*next);
                    }
                }
            }
        }
        Ok(())
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
    let mut value = String::new();
    let mut lengths = Vec::new();
    let mut positions = Vec::new();
    let mut widths = Vec::new();
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
    node.set_word_starts(
        line.word_starts
            .iter()
            .filter(|&&i| range.contains(&i))
            .map(|i| (i - range.start) as u8)
            .collect::<Vec<_>>(),
    );
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
