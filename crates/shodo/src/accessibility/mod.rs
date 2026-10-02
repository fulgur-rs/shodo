//! Retained logical text, selectable units and source positions for assistive technology.
//!
//! Positions belong to one snapshot; preserve source anchors before reflow.
//! Text uses each accepted line's own processed dataset, including nonpainting
//! characters. DOM semantics, atomic alternatives and platform events belong
//! to the caller. See `docs/accessibility.md` for the integration contract.
mod output;
#[cfg(test)]
mod source_tests;

#[cfg(feature = "accesskit")]
pub mod accesskit;

use crate::font::FontId;
use crate::geometry::{Direction, LogicalRect, WritingMode};
use crate::hit::{LineLayout, TextPosition};
use crate::mapping::{Affinity, OffsetMapping, TextOrigin};
use crate::node::NodeId;
use crate::style::InlineStyle;
use crate::{BreakReason, GlyphOrientation, Line};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessibleCharacterKind {
    Text,
    HardBreak,
    Atomic(NodeId),
}

#[derive(Clone, Debug)]
pub struct AccessibleCharacter<'a> {
    pub text: &'a str,
    pub text_range: Range<u32>,
    pub kind: AccessibleCharacterKind,
    pub rect: LogicalRect,
    pub leading: (f32, f32),
    pub trailing: (f32, f32),
}

#[derive(Clone, Debug)]
pub struct AccessibleRun<'a> {
    pub character_range: Range<usize>,
    pub text_range: Range<u32>,
    pub node: Option<NodeId>,
    pub style: &'a InlineStyle,
    pub font: Option<FontId>,
    pub font_size: f32,
    pub bidi_level: u8,
    pub orientation: GlyphOrientation,
    pub bounds: LogicalRect,
}

#[derive(Clone, Debug)]
pub struct AccessibleLine<'a> {
    pub index: usize,
    pub text: &'a str,
    pub text_range: Range<u32>,
    pub characters: Vec<AccessibleCharacter<'a>>,
    pub runs: Vec<AccessibleRun<'a>>,
    pub word_starts: Vec<usize>,
    pub writing_mode: WritingMode,
    pub direction: Direction,
    pub break_reason: BreakReason,
    pub bounds: LogicalRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessiblePosition {
    pub snapshot: u64,
    pub line: usize,
    pub character: usize,
    pub affinity: Affinity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessibleSelection {
    pub anchor: AccessiblePosition,
    pub focus: AccessiblePosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub origin: TextOrigin,
    pub affinity: Affinity,
}

/// Explicit reading relationship; annotation offsets remain in its retained Line.
#[derive(Clone, Copy, Debug)]
pub struct AccessibleRuby<'a> {
    pub parent_line: usize,
    pub annotation: crate::RubyAnnotationView<'a>,
}

pub struct AccessibleLayout<'a> {
    snapshot: u64,
    accepted: &'a [Line],
    hit: LineLayout<'a>,
    lines: Vec<AccessibleLine<'a>>,
}

impl<'a> AccessibleLayout<'a> {
    pub fn new(lines: &'a [Line]) -> Self {
        let hit = LineLayout::new(lines);
        let output = output::build(lines, &hit);
        Self {
            snapshot: NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed),
            accepted: lines,
            hit,
            lines: output,
        }
    }
    pub fn ruby_annotations(&self) -> impl Iterator<Item = AccessibleRuby<'a>> + '_ {
        self.accepted
            .iter()
            .enumerate()
            .flat_map(|(parent_line, line)| {
                line.ruby_annotations()
                    .map(move |annotation| AccessibleRuby {
                        parent_line,
                        annotation,
                    })
            })
    }
    pub fn snapshot_id(&self) -> u64 {
        self.snapshot
    }
    pub fn lines(&self) -> &[AccessibleLine<'a>] {
        &self.lines
    }
    pub fn logical_text(&self) -> String {
        self.lines.iter().map(|l| l.text).collect()
    }
    pub fn position(
        &self,
        line: usize,
        character: usize,
        affinity: Affinity,
    ) -> Option<AccessiblePosition> {
        let l = self.lines.get(line)?;
        let offset = if character == l.characters.len() {
            l.text_range.end
        } else {
            l.characters.get(character)?.text_range.start
        };
        self.from_text_position(TextPosition {
            line,
            offset,
            affinity,
        })
    }
    pub fn to_text_position(&self, position: AccessiblePosition) -> Option<TextPosition> {
        if position.snapshot != self.snapshot {
            return None;
        }
        let l = self.lines.get(position.line)?;
        let offset = if position.character == l.characters.len() {
            l.text_range.end
        } else {
            l.characters.get(position.character)?.text_range.start
        };
        Some(
            self.hit
                .caret(TextPosition {
                    line: position.line,
                    offset,
                    affinity: position.affinity,
                })?
                .position,
        )
    }
    pub fn from_text_position(&self, position: TextPosition) -> Option<AccessiblePosition> {
        let caret = self.hit.caret(position)?.position;
        let l = self.lines.get(caret.line)?;
        let character = if caret.offset == l.text_range.end {
            l.characters.len()
        } else {
            l.characters
                .binary_search_by_key(&caret.offset, |c| c.text_range.start)
                .ok()?
        };
        Some(AccessiblePosition {
            snapshot: self.snapshot,
            line: caret.line,
            character,
            affinity: caret.affinity,
        })
    }
    pub fn to_source(&self, position: AccessiblePosition) -> Option<SourcePosition> {
        let p = self.to_text_position(position)?;
        Some(SourcePosition {
            origin: self.accepted[p.line]
                .offset_mapping()?
                .text_to_dom(p.offset, p.affinity)?,
            affinity: p.affinity,
        })
    }
    /// Source inverses may return several boundaries, particularly for generated
    /// content and repeated sources. Collapsed/expanded DOM offsets normalize.
    pub fn from_source(&self, source: SourcePosition) -> Vec<AccessiblePosition> {
        let mut result = Vec::new();
        // First-line styling can give equal data ids distinct source mappings.
        // Cache by the actual shared dataset, preserving repeated accepted lines.
        let mut datasets: BTreeMap<*const OffsetMapping, Vec<(u32, Affinity)>> = BTreeMap::new();
        for (line, accepted) in self.accepted.iter().enumerate() {
            let Some(mapping) = accepted.offset_mapping() else {
                continue;
            };
            match source.origin {
                TextOrigin::Dom { node, offset } => {
                    let candidates = datasets
                        .entry(mapping as *const OffsetMapping)
                        .or_insert_with(|| {
                            mapping.source_candidates(node, offset, source.affinity)
                        });
                    let range = &self.lines[line].text_range;
                    let first = candidates.partition_point(|&(text, _)| text < range.start);
                    let last = candidates.partition_point(|&(text, _)| text <= range.end);
                    let begin = result.len();
                    for &(offset, affinity) in &candidates[first..last] {
                        if let Some(p) = self.from_text_position(TextPosition {
                            line,
                            offset,
                            affinity,
                        }) {
                            result.push(p);
                        }
                    }
                    result[begin..]
                        .sort_by_key(|p| (p.character, p.affinity == Affinity::Downstream));
                }
                TextOrigin::Generated { .. } => {
                    for character in 0..=self.lines[line].characters.len() {
                        if let Some(p) = self.position(line, character, source.affinity)
                            && self.to_source(p).is_some_and(|s| s.origin == source.origin)
                            && result.last() != Some(&p)
                        {
                            result.push(p);
                        }
                    }
                }
            }
        }
        result.dedup();
        result
    }
    pub fn selection_rects(&self, selection: AccessibleSelection) -> Vec<LogicalRect> {
        let (Some(a), Some(b)) = (
            self.to_text_position(selection.anchor),
            self.to_text_position(selection.focus),
        ) else {
            return Vec::new();
        };
        self.hit.selection_rects(a, b)
    }
    pub fn hit_test(&self, inline: f32, block: f32) -> Option<AccessiblePosition> {
        self.from_text_position(self.hit.hit_test(inline, block)?.position)
    }
}
