//! Fragment records: the compact per-line table behind the public views.

use std::ops::Range;

use crate::analysis::units::UnitKind;
use crate::geometry::LayoutUnit;
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{AtomicSize, AtomicSizes, ParagraphData};

#[derive(Clone, Debug)]
pub(crate) struct FragmentRecord {
    pub(crate) kind: RecordKind,
    pub(crate) inline_start: LayoutUnit,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) level: u8,
}

#[derive(Clone, Debug)]
pub(crate) enum RecordKind {
    Glyphs {
        run: u32,
        glyphs: Range<u32>,
        item: u32,
        text: Range<u32>,
    },
    Atomic {
        node: NodeId,
        size: AtomicSize,
    },
    InlineBox {
        box_index: u32,
        start_edge: bool,
        end_edge: bool,
        parent: Option<u32>,
    },
    Anchor {
        node: NodeId,
        kind: OutOfFlowKind,
    },
}

/// Builds the records of one line in logical order.
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let mut out: Vec<FragmentRecord> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut pos = origin;
    let level_at_start = data
        .units
        .get(units.start)
        .map_or(data.base_level, |u| u.level);

    // Boxes that are still open from the previous line continue here,
    // without their start edge. A line can start on the `Close` of a box
    // that was still open at the previous line's end (for example after a
    // forced break inside it); that box's own index, not its parent's, is
    // where the continuation chain begins.
    let mut chain = Vec::new();
    let mut parent = data.units.get(units.start).and_then(|u| match &u.kind {
        UnitKind::Close { box_index } => Some(*box_index),
        _ => u.parent_box,
    });
    while let Some(b) = parent {
        chain.push(b);
        parent = data.boxes[b as usize].parent;
    }
    for &box_index in chain.iter().rev() {
        let parent = open.last().map(|&r| r as u32);
        out.push(FragmentRecord {
            kind: RecordKind::InlineBox {
                box_index,
                start_edge: false,
                end_edge: false,
                parent,
            },
            inline_start: pos,
            inline_size: LayoutUnit::ZERO,
            level: level_at_start,
        });
        open.push(out.len() - 1);
    }

    for (k, i) in units.enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        match &unit.kind {
            UnitKind::Open { box_index } => {
                let parent = open.last().map(|&r| r as u32);
                out.push(FragmentRecord {
                    kind: RecordKind::InlineBox {
                        box_index: *box_index,
                        start_edge: true,
                        end_edge: false,
                        parent,
                    },
                    inline_start: pos,
                    inline_size: LayoutUnit::ZERO,
                    level: unit.level,
                });
                open.push(out.len() - 1);
                pos = pos + w;
            }
            UnitKind::Close { .. } => {
                pos = pos + w;
                if let Some(r) = open.pop() {
                    out[r].inline_size = pos - out[r].inline_start;
                    if let RecordKind::InlineBox { end_edge, .. } = &mut out[r].kind {
                        *end_edge = true;
                    }
                }
            }
            UnitKind::Cluster { run, glyphs, .. } => {
                if let Some(last) = out.last_mut()
                    && last.level == unit.level
                    && let RecordKind::Glyphs {
                        run: r,
                        glyphs: g,
                        item,
                        text,
                    } = &mut last.kind
                    && *r == *run
                    && *item == unit.item
                    && g.end == glyphs.start
                {
                    g.end = glyphs.end;
                    text.end = unit.text.end;
                    last.inline_size = last.inline_size + w;
                } else {
                    out.push(FragmentRecord {
                        kind: RecordKind::Glyphs {
                            run: *run,
                            glyphs: glyphs.clone(),
                            item: unit.item,
                            text: unit.text.clone(),
                        },
                        inline_start: pos,
                        inline_size: w,
                        level: unit.level,
                    });
                }
                pos = pos + w;
            }
            UnitKind::Atomic { node } => {
                let size = atomics.get(*node).copied().unwrap_or_default();
                out.push(FragmentRecord {
                    kind: RecordKind::Atomic { node: *node, size },
                    inline_start: pos,
                    inline_size: w,
                    level: unit.level,
                });
                pos = pos + w;
            }
            UnitKind::Float { node, .. } | UnitKind::Absolute { node } => {
                let kind = if matches!(unit.kind, UnitKind::Float { .. }) {
                    OutOfFlowKind::Float
                } else {
                    OutOfFlowKind::Absolute
                };
                out.push(FragmentRecord {
                    kind: RecordKind::Anchor { node: *node, kind },
                    inline_start: pos,
                    inline_size: LayoutUnit::ZERO,
                    level: unit.level,
                });
            }
            UnitKind::Tab => pos = pos + w,
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => {}
        }
    }
    // Boxes that continue on the next line end here without their end edge.
    for r in open {
        out[r].inline_size = pos - out[r].inline_start;
    }
    out
}
