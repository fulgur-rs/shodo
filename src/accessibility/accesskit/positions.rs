use super::types;
use crate::accessibility::{AccessibleCharacterKind, AccessibleLayout, AccessiblePosition};
use crate::mapping::Affinity;
use std::collections::HashMap;
use std::ops::Range;

pub(super) struct Span {
    pub node: types::NodeId,
    pub line: usize,
    pub characters: Range<usize>,
    pub hard_break: Option<usize>,
}

struct Boundary {
    upstream: AccessiblePosition,
    downstream: AccessiblePosition,
    upstream_at: types::TextPosition,
    downstream_at: types::TextPosition,
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
        for line in layout.lines() {
            let end = spans.partition_point(|s| s.line <= line.index);
            let current = &spans[begin..end];
            let mut boundaries = Vec::new();
            let mut downstream_index = 0;
            let mut upstream_index = 0;
            for character in 0..=line.characters.len() {
                let canonical = if character == line.characters.len()
                    && character > 0
                    && line.characters[character - 1].kind == AccessibleCharacterKind::HardBreak
                {
                    character - 1
                } else {
                    character
                };
                while downstream_index + 1 < current.len()
                    && current[downstream_index].characters.end <= canonical
                {
                    downstream_index += 1;
                }
                while upstream_index + 1 < current.len()
                    && current[upstream_index + 1].characters.start < canonical
                {
                    upstream_index += 1;
                }
                let downstream = &current[downstream_index];
                let upstream = if line
                    .characters
                    .get(canonical)
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
                        character_index: canonical - upstream.characters.start,
                    },
                    downstream_at: types::TextPosition {
                        node: downstream.node,
                        character_index: canonical - downstream.characters.start,
                    },
                });
            }
            lines.push(boundaries);
            begin = end;
        }
        Self {
            snapshot: layout.snapshot_id(),
            lines,
            spans: spans.into_iter().map(|s| (s.node, s)).collect(),
        }
    }
    pub fn to_position(&self, p: AccessiblePosition) -> Option<types::TextPosition> {
        if p.snapshot != self.snapshot {
            return None;
        }
        let b = self.lines.get(p.line)?.get(p.character)?;
        Some(match p.affinity {
            Affinity::Upstream => b.upstream_at,
            Affinity::Downstream => b.downstream_at,
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
        let mut character = span.characters.start + p.character_index;
        if p.character_index == span.characters.len()
            && let Some(hard_break) = span.hard_break
        {
            character = hard_break;
        }
        let b = self.lines.get(span.line)?.get(character)?;
        Some(match affinity {
            Affinity::Upstream => b.upstream,
            Affinity::Downstream => b.downstream,
        })
    }
}
