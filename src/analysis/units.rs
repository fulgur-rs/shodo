//! Units: the sequence line breaking walks. There is one unit per cluster
//! (a base character with its combining marks) and one per non-text item.

use std::ops::Range;

use unicode_bidi::{BidiClass, BidiInfo, Level, bidi_class};

use super::{Item, ItemKind};
use crate::geometry::Direction;
use crate::node::{InlineEdges, NodeId, OutOfFlowKind};
use crate::shape::{GlyphStore, ShapedRun, is_mark};

/// Line break opportunity after a unit. Only spaces, tabs and atomic inlines
/// provide soft opportunities for now; UAX #14 comes later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BreakClass {
    Prohibited,
    Allowed,
    Mandatory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnitKind {
    Cluster {
        run: u32,
        glyphs: Range<u32>,
        space: bool,
    },
    Open {
        box_index: u32,
    },
    Close {
        box_index: u32,
    },
    Atomic {
        node: NodeId,
    },
    Float {
        node: NodeId,
        ordinal: u32,
    },
    Absolute {
        node: NodeId,
    },
    BlockInInline {
        node: NodeId,
    },
    ForcedBreak,
    Tab,
    BidiControl,
}

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub(crate) kind: UnitKind,
    pub(crate) item: u32,
    pub(crate) text: Range<u32>,
    pub(crate) break_after: BreakClass,
    pub(crate) level: u8,
    /// Innermost inline box containing the unit (for `Open`/`Close`, the
    /// box's parent).
    pub(crate) parent_box: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct InlineBoxInfo {
    pub(crate) node: NodeId,
    pub(crate) style: u32,
    pub(crate) edges: InlineEdges,
    pub(crate) parent: Option<u32>,
}

pub(crate) struct UnitList {
    pub(crate) units: Vec<Unit>,
    pub(crate) boxes: Vec<InlineBoxInfo>,
    pub(crate) float_count: u32,
}

/// Bidi embedding level of every byte of `text` (UAX #9). Left-to-right
/// paragraphs without right-to-left characters or bidi controls skip the
/// algorithm.
pub(crate) fn bidi_levels(text: &str, direction: Direction, plaintext: bool) -> Vec<u8> {
    use BidiClass::*;
    let needs_bidi = plaintext
        || direction == Direction::Rtl
        || text.chars().any(|c| {
            matches!(
                bidi_class(c),
                R | AL | RLE | RLO | RLI | LRE | LRO | LRI | FSI | PDF | PDI
            )
        });
    if !needs_bidi {
        return vec![0; text.len()];
    }
    let base = match (plaintext, direction) {
        (true, _) => None,
        (false, Direction::Rtl) => Some(Level::rtl()),
        (false, Direction::Ltr) => Some(Level::ltr()),
    };
    BidiInfo::new(text, base)
        .levels
        .iter()
        .map(|l| l.number())
        .collect()
}

pub(crate) fn build_units(
    text: &str,
    items: &[Item],
    runs: &[ShapedRun],
    glyphs: &GlyphStore,
    levels: &[u8],
    base_level: u8,
) -> UnitList {
    let mut units: Vec<Unit> = Vec::with_capacity(glyphs.len() + items.len());
    let mut boxes: Vec<InlineBoxInfo> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut float_count = 0u32;
    let mut run_index = 0usize;
    for (index, item) in items.iter().enumerate() {
        let index = index as u32;
        let parent_box = stack.last().copied();
        let node = item.node.unwrap_or(NodeId(0));
        let mut push = |kind: UnitKind, break_after: BreakClass, parent_box: Option<u32>| {
            units.push(Unit {
                kind,
                item: index,
                text: item.text.clone(),
                break_after,
                level: base_level,
                parent_box,
            });
        };
        match &item.kind {
            ItemKind::Text => {
                while run_index < runs.len() && runs[run_index].item == index {
                    let run = &runs[run_index];
                    for g in run.glyphs.clone() {
                        let cluster = glyphs.cluster[g as usize];
                        let c = text[cluster as usize..].chars().next().unwrap_or(' ');
                        let end = cluster + c.len_utf8() as u32;
                        if is_mark(c)
                            && let Some(last) = units.last_mut()
                            && let UnitKind::Cluster {
                                run: r,
                                glyphs: range,
                                ..
                            } = &mut last.kind
                            && *r == run_index as u32
                        {
                            range.end = g + 1;
                            last.text.end = end;
                            continue;
                        }
                        let space = c == ' ';
                        units.push(Unit {
                            kind: UnitKind::Cluster {
                                run: run_index as u32,
                                glyphs: g..g + 1,
                                space,
                            },
                            item: index,
                            text: cluster..end,
                            break_after: if space {
                                BreakClass::Allowed
                            } else {
                                BreakClass::Prohibited
                            },
                            level: base_level,
                            parent_box,
                        });
                    }
                    run_index += 1;
                }
            }
            ItemKind::OpenInline { edges } => {
                let box_index = boxes.len() as u32;
                boxes.push(InlineBoxInfo {
                    node,
                    style: item.style,
                    edges: *edges,
                    parent: parent_box,
                });
                push(
                    UnitKind::Open { box_index },
                    BreakClass::Prohibited,
                    parent_box,
                );
                stack.push(box_index);
            }
            ItemKind::CloseInline => {
                if let Some(box_index) = stack.pop() {
                    push(
                        UnitKind::Close { box_index },
                        BreakClass::Prohibited,
                        stack.last().copied(),
                    );
                }
            }
            ItemKind::Atomic { .. } => {
                // UAX #14 class CB: break opportunities before and after.
                if let Some(prev) = units.last_mut()
                    && prev.break_after == BreakClass::Prohibited
                {
                    prev.break_after = BreakClass::Allowed;
                }
                units.push(Unit {
                    kind: UnitKind::Atomic { node },
                    item: index,
                    text: item.text.clone(),
                    break_after: BreakClass::Allowed,
                    level: base_level,
                    parent_box,
                });
            }
            ItemKind::OutOfFlow { kind } => {
                let unit = match kind {
                    OutOfFlowKind::Float => {
                        float_count += 1;
                        UnitKind::Float {
                            node,
                            ordinal: float_count - 1,
                        }
                    }
                    OutOfFlowKind::Absolute => UnitKind::Absolute { node },
                };
                push(unit, BreakClass::Prohibited, parent_box);
            }
            ItemKind::BlockInInline => push(
                UnitKind::BlockInInline { node },
                BreakClass::Prohibited,
                parent_box,
            ),
            ItemKind::ForcedBreak => push(UnitKind::ForcedBreak, BreakClass::Mandatory, parent_box),
            ItemKind::Tab => push(UnitKind::Tab, BreakClass::Allowed, parent_box),
            ItemKind::BidiControl => {
                push(UnitKind::BidiControl, BreakClass::Prohibited, parent_box)
            }
        }
    }
    let level_at = |pos: u32| levels.get(pos as usize).copied();
    for unit in &mut units {
        let level = match unit.kind {
            // An opening box takes the level of what follows it, a closing
            // box the level of what precedes it.
            UnitKind::Open { .. } => level_at(unit.text.start),
            UnitKind::Close { .. } => unit.text.start.checked_sub(1).and_then(level_at),
            _ => level_at(unit.text.start),
        };
        unit.level = level.unwrap_or(base_level);
    }
    UnitList {
        units,
        boxes,
        float_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_to_right_text_skips_the_algorithm() {
        assert_eq!(bidi_levels("abc", Direction::Ltr, false), vec![0, 0, 0]);
    }

    #[test]
    fn hebrew_gets_odd_levels() {
        let levels = bidi_levels("a \u{5D0}", Direction::Ltr, false);
        assert_eq!(levels[0], 0);
        assert_eq!(*levels.last().unwrap(), 1);
        assert!(
            bidi_levels("abc", Direction::Rtl, false)
                .iter()
                .all(|&l| l == 2)
        );
    }
}
