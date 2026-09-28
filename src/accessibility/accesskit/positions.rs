use super::types;
use crate::accessibility::{
    AccessibleCharacterKind, AccessibleLayout, AccessiblePosition, AccessibleSelection,
};
use crate::mapping::Affinity;
use std::collections::HashMap;
use std::ops::Range;

pub(super) struct Span {
    pub node: types::NodeId,
    pub line: usize,
    pub characters: Range<usize>,
}

struct Boundary {
    upstream: AccessiblePosition,
    downstream: AccessiblePosition,
    upstream_at: types::TextPosition,
    downstream_at: types::TextPosition,
    logical_character: usize,
    after_hard_break: bool,
}

#[derive(Default)]
pub(super) struct PositionState {
    snapshot: u64,
    lines: Vec<Vec<Boundary>>,
    spans: HashMap<types::NodeId, Span>,
}

impl PositionState {
    pub fn new(layout: &AccessibleLayout<'_>, spans: Vec<Span>) -> Self {
        let mut lines = Vec::new();
        let mut begin = 0;
        let mut logical_start = 0;
        for line in layout.lines() {
            let end = spans.partition_point(|s| s.line <= line.index);
            let current = &spans[begin..end];
            let mut boundaries = Vec::new();
            let mut downstream_index = 0;
            let mut upstream_index = 0;
            for character in 0..=line.characters.len() {
                while downstream_index + 1 < current.len()
                    && current[downstream_index].characters.end <= character
                {
                    downstream_index += 1;
                }
                while upstream_index + 1 < current.len()
                    && current[upstream_index + 1].characters.start < character
                {
                    upstream_index += 1;
                }
                let downstream = &current[downstream_index];
                let upstream = if line
                    .characters
                    .get(character)
                    .is_some_and(|c| c.kind == AccessibleCharacterKind::HardBreak)
                {
                    downstream
                } else {
                    &current[upstream_index]
                };
                boundaries.push(Boundary {
                    upstream: layout
                        .position(line.index, character, Affinity::Upstream)
                        .expect("accepted boundary"),
                    downstream: layout
                        .position(line.index, character, Affinity::Downstream)
                        .expect("accepted boundary"),
                    upstream_at: types::TextPosition {
                        node: upstream.node,
                        character_index: character - upstream.characters.start,
                    },
                    downstream_at: types::TextPosition {
                        node: downstream.node,
                        character_index: character - downstream.characters.start,
                    },
                    logical_character: logical_start + character,
                    after_hard_break: character == line.characters.len()
                        && line
                            .characters
                            .last()
                            .is_some_and(|c| c.kind == AccessibleCharacterKind::HardBreak),
                });
            }
            lines.push(boundaries);
            begin = end;
            logical_start += line.characters.len();
        }
        Self {
            snapshot: layout.snapshot_id(),
            lines,
            spans: spans.into_iter().map(|s| (s.node, s)).collect(),
        }
    }
    fn boundary(&self, p: AccessiblePosition) -> Option<&Boundary> {
        if p.snapshot != self.snapshot {
            return None;
        }
        self.lines.get(p.line)?.get(p.character)
    }
    pub fn to_position(&self, p: AccessiblePosition) -> Option<types::TextPosition> {
        let b = self.boundary(p)?;
        let b = if b.after_hard_break {
            &self.lines[p.line][p.character - 1]
        } else {
            b
        };
        Some(match p.affinity {
            Affinity::Upstream => b.upstream_at,
            Affinity::Downstream => b.downstream_at,
        })
    }
    pub fn to_selection(&self, selection: AccessibleSelection) -> Option<types::TextSelection> {
        let anchor = self.boundary(selection.anchor)?;
        let focus = self.boundary(selection.focus)?;
        if anchor.logical_character == focus.logical_character {
            let caret = self.to_position(selection.focus)?;
            return Some(types::TextSelection {
                anchor: caret,
                focus: caret,
            });
        }
        let endpoint = |b: &Boundary, affinity| match affinity {
            Affinity::Upstream => b.upstream_at,
            Affinity::Downstream => b.downstream_at,
        };
        Some(types::TextSelection {
            anchor: endpoint(anchor, selection.anchor.affinity),
            focus: endpoint(focus, selection.focus.affinity),
        })
    }
    pub fn resolve(
        &self,
        p: types::TextPosition,
        affinity: Affinity,
    ) -> Option<AccessiblePosition> {
        let span = self.spans.get(&p.node)?;
        if p.character_index > span.characters.len() {
            return None;
        }
        let character = span.characters.start + p.character_index;
        let b = self.lines.get(span.line)?.get(character)?;
        Some(match affinity {
            Affinity::Upstream => b.upstream,
            Affinity::Downstream => b.downstream,
        })
    }
}
