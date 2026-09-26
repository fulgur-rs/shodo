//! Fragment records: the compact per-line table behind the public views.

use std::ops::Range;

use unicode_bidi::{BidiInfo, Level};

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
        /// Whether the box's own `direction` opposes the paragraph's, so
        /// its start edge is on the inline-end side.
        reversed: bool,
    },
    Anchor {
        node: NodeId,
        kind: OutOfFlowKind,
    },
}

/// Whether a box's own `direction` opposes the paragraph's, so its start
/// edge (in text order) is on the inline-end side of the rendered box (CSS
/// Writing Modes 4 §2.4.1). Bidi *levels* are not a reliable proxy: an
/// isolate or embedding initiator's unit takes the level in effect just
/// before the isolate (UAX #9), not the level of the box's own content.
fn box_reversed(data: &ParagraphData, box_index: u32) -> bool {
    let info = &data.boxes[box_index as usize];
    data.styles[info.style as usize].direction != data.style.direction
}

/// Builds the records of one line in logical order.
fn build_logical(
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
                reversed: box_reversed(data, box_index),
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
                        reversed: box_reversed(data, *box_index),
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

/// Builds the records of one line in visual order (UAX #9 L2).
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    if data.units[units.clone()].iter().all(|u| u.level == base) {
        return build_logical(data, units, widths, origin, atomics);
    }
    build_bidi(data, units, widths, origin, atomics)
}

/// A reorderable piece of a line: a glyph run segment, an atomic, an
/// anchor, a tab, or an inline box edge.
struct Piece {
    record: Option<FragmentRecord>,
    width: LayoutUnit,
    level: u8,
    /// Innermost inline box the piece belongs to (the box itself for edges).
    owner: Option<u32>,
    /// `(box, is_start)` for inline box edges.
    edge: Option<(u32, bool)>,
}

fn inside(data: &ParagraphData, mut b: Option<u32>, target: u32) -> bool {
    while let Some(x) = b {
        if x == target {
            return true;
        }
        b = data.boxes[x as usize].parent;
    }
    false
}

fn build_bidi(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    let mut pieces: Vec<Piece> = Vec::new();
    for (k, i) in units.clone().enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        let record = |kind: RecordKind| FragmentRecord {
            kind,
            inline_start: LayoutUnit::ZERO,
            inline_size: w,
            level: unit.level,
        };
        let piece = match &unit.kind {
            UnitKind::Open { box_index } => Piece {
                record: None,
                width: w,
                level: unit.level,
                owner: Some(*box_index),
                edge: Some((*box_index, true)),
            },
            UnitKind::Close { box_index } => Piece {
                record: None,
                width: w,
                level: unit.level,
                owner: Some(*box_index),
                edge: Some((*box_index, false)),
            },
            UnitKind::Cluster { run, glyphs, .. } => {
                if let Some(last) = pieces.last_mut()
                    && last.level == unit.level
                    && last.owner == unit.parent_box
                    && let Some(FragmentRecord {
                        kind:
                            RecordKind::Glyphs {
                                run: r,
                                glyphs: g,
                                item,
                                text,
                            },
                        inline_size,
                        ..
                    }) = &mut last.record
                    && *r == *run
                    && *item == unit.item
                    && g.end == glyphs.start
                {
                    g.end = glyphs.end;
                    text.end = unit.text.end;
                    *inline_size = *inline_size + w;
                    last.width = last.width + w;
                    continue;
                }
                let kind = RecordKind::Glyphs {
                    run: *run,
                    glyphs: glyphs.clone(),
                    item: unit.item,
                    text: unit.text.clone(),
                };
                Piece {
                    record: Some(record(kind)),
                    width: w,
                    level: unit.level,
                    owner: unit.parent_box,
                    edge: None,
                }
            }
            UnitKind::Atomic { node } => {
                let size = atomics.get(*node).copied().unwrap_or_default();
                let kind = RecordKind::Atomic { node: *node, size };
                Piece {
                    record: Some(record(kind)),
                    width: w,
                    level: unit.level,
                    owner: unit.parent_box,
                    edge: None,
                }
            }
            UnitKind::Float { node, .. } | UnitKind::Absolute { node } => {
                let kind = if matches!(unit.kind, UnitKind::Float { .. }) {
                    OutOfFlowKind::Float
                } else {
                    OutOfFlowKind::Absolute
                };
                let mut r = record(RecordKind::Anchor { node: *node, kind });
                r.inline_size = LayoutUnit::ZERO;
                Piece {
                    record: Some(r),
                    width: LayoutUnit::ZERO,
                    level: unit.level,
                    owner: unit.parent_box,
                    edge: None,
                }
            }
            UnitKind::Tab => Piece {
                record: None,
                width: w,
                level: unit.level,
                owner: unit.parent_box,
                edge: None,
            },
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => {
                continue;
            }
        };
        pieces.push(piece);
    }

    // Visual order, left to right; from the inline-start edge that is the
    // reverse order when the paragraph is right-to-left.
    let levels: Vec<Level> = pieces
        .iter()
        .map(|p| Level::new(p.level).unwrap_or_else(|_| Level::ltr()))
        .collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if base % 2 == 1 {
        order.reverse();
    }
    let mut starts = vec![LayoutUnit::ZERO; pieces.len()];
    let mut pos = origin;
    for &p in &order {
        starts[p] = pos;
        pos = pos + pieces[p].width;
    }

    // Inline boxes: one fragment per visually contiguous group of members.
    let mut box_ids: Vec<u32> = Vec::new();
    for piece in &pieces {
        let mut b = piece.owner;
        while let Some(x) = b {
            if !box_ids.contains(&x) {
                box_ids.push(x);
            }
            b = data.boxes[x as usize].parent;
        }
    }
    box_ids.sort_unstable();
    let mut boxes: Vec<(FragmentRecord, u32)> = Vec::new();
    for &b in &box_ids {
        let reversed = box_reversed(data, b);
        let mut group: Option<(LayoutUnit, LayoutUnit, bool, bool, u8)> = None;
        let flush = |group: &mut Option<(LayoutUnit, LayoutUnit, bool, bool, u8)>,
                     boxes: &mut Vec<(FragmentRecord, u32)>| {
            if let Some((start, size, start_edge, end_edge, level)) = group.take() {
                let kind = RecordKind::InlineBox {
                    box_index: b,
                    start_edge,
                    end_edge,
                    parent: None,
                    reversed,
                };
                boxes.push((
                    FragmentRecord {
                        kind,
                        inline_start: start,
                        inline_size: size,
                        level,
                    },
                    b,
                ));
            }
        };
        for &p in &order {
            let piece = &pieces[p];
            if inside(data, piece.owner, b) {
                let g =
                    group.get_or_insert((starts[p], LayoutUnit::ZERO, false, false, piece.level));
                g.1 = g.1 + piece.width;
                g.2 |= piece.edge == Some((b, true));
                g.3 |= piece.edge == Some((b, false));
            } else {
                flush(&mut group, &mut boxes);
            }
        }
        flush(&mut group, &mut boxes);
    }

    // Output: boxes and content sorted by position; a box precedes the
    // content it starts with, and an outer box precedes an inner one.
    let depth = |b: u32| {
        let mut d = 0;
        let mut x = data.boxes[b as usize].parent;
        while let Some(p) = x {
            d += 1;
            x = data.boxes[p as usize].parent;
        }
        d
    };
    let mut out: Vec<(FragmentRecord, Option<u32>, u32)> = boxes
        .into_iter()
        .map(|(r, b)| (r, Some(b), depth(b)))
        .collect();
    for &p in &order {
        if let Some(mut r) = pieces[p].record.clone() {
            r.inline_start = starts[p];
            out.push((r, None, u32::MAX));
        }
    }
    out.sort_by_key(|(r, _, d)| (r.inline_start, *d));

    // Parent links: the enclosing box fragment of the parent box.
    let spans: Vec<(Option<u32>, LayoutUnit, LayoutUnit)> = out
        .iter()
        .map(|(r, b, _)| (*b, r.inline_start, r.inline_start + r.inline_size))
        .collect();
    for (r, b, _) in &mut out {
        let Some(b) = *b else { continue };
        let Some(parent_box) = data.boxes[b as usize].parent else {
            continue;
        };
        let (start, end) = (r.inline_start, r.inline_start + r.inline_size);
        let parent = spans
            .iter()
            .position(|(pb, ps, pe)| *pb == Some(parent_box) && *ps <= start && end <= *pe);
        if let RecordKind::InlineBox { parent: slot, .. } = &mut r.kind {
            *slot = parent.map(|p| p as u32);
        }
    }
    out.into_iter().map(|(r, _, _)| r).collect()
}
