//! Fragment records: the compact per-line table behind the public views.

use std::ops::Range;

use unicode_bidi::{BidiInfo, Level};

use crate::analysis::units::{Unit, UnitKind};
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

/// Builds the records of one line in visual order (UAX #9 L2). Units from
/// `hang_start` on are the line's hanging trailing spaces and what follows
/// them (see `Scan::hang_start`).
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    hang_start: usize,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    if data.units[units.clone()].iter().all(|u| u.level == base) {
        return build_logical(data, units, widths, origin, atomics);
    }
    build_bidi(data, units, hang_start, widths, origin, atomics)
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

/// Appends a cluster to `list`, extending the last glyph piece when it
/// continues the same run, item, level and owner.
fn push_cluster(
    list: &mut Vec<Piece>,
    unit: &Unit,
    width: LayoutUnit,
    level: u8,
    owner: Option<u32>,
) {
    let UnitKind::Cluster { run, glyphs, .. } = &unit.kind else {
        return;
    };
    let (run, item, text) = (*run, unit.item, &unit.text);
    if let Some(last) = list.last_mut()
        && last.level == level
        && last.owner == owner
        && let Some(FragmentRecord {
            kind:
                RecordKind::Glyphs {
                    run: r,
                    glyphs: g,
                    item: it,
                    text: t,
                },
            inline_size,
            ..
        }) = &mut last.record
        && *r == run
        && *it == item
        && g.end == glyphs.start
    {
        g.end = glyphs.end;
        t.end = text.end;
        *inline_size = *inline_size + width;
        last.width = last.width + width;
        return;
    }
    list.push(Piece {
        record: Some(FragmentRecord {
            kind: RecordKind::Glyphs {
                run,
                glyphs: glyphs.clone(),
                item,
                text: text.clone(),
            },
            inline_start: LayoutUnit::ZERO,
            inline_size: width,
            level,
        }),
        width,
        level,
        owner,
        edge: None,
    });
}

fn build_bidi(
    data: &ParagraphData,
    units: Range<usize>,
    hang_start: usize,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    let mut pieces: Vec<Piece> = Vec::new();
    // Hanging trailing spaces, placed after everything else on the line.
    let mut hanging: Vec<Piece> = Vec::new();
    // Level and owner of the last piece that is neither a hanging space nor
    // an out-of-flow anchor.
    let mut last_kept: Option<(u8, Option<u32>)> = None;
    for (k, i) in units.clone().enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        let trailing = i >= hang_start;
        // UAX #9 L1: trailing whitespace and segment separators (tabs) take
        // the paragraph embedding level. `BidiInfo` levels are resolved
        // before L1, so it is applied here, per line and per unit.
        let level = match &unit.kind {
            UnitKind::Cluster { space: true, .. } if trailing => base,
            UnitKind::Tab => base,
            // A box end after the hanging spaces stays with the box's
            // content: it takes the level of the box's last piece before
            // them rather than the pre-L1 level of the spaces.
            UnitKind::Close { box_index } if trailing => match last_kept {
                Some((level, owner)) if inside(data, owner, *box_index) => level,
                _ => unit.level,
            },
            _ => unit.level,
        };
        let record = |kind: RecordKind| FragmentRecord {
            kind,
            inline_start: LayoutUnit::ZERO,
            inline_size: w,
            level,
        };
        let piece = match &unit.kind {
            UnitKind::Open { box_index } => Piece {
                record: None,
                width: w,
                level,
                owner: Some(*box_index),
                edge: Some((*box_index, true)),
            },
            UnitKind::Close { box_index } => Piece {
                record: None,
                width: w,
                level,
                owner: Some(*box_index),
                edge: Some((*box_index, false)),
            },
            UnitKind::Cluster { space, .. } => {
                if trailing && *space {
                    // Hanging spaces belong to no box on this line, so box
                    // fragments end with the box's content and edges.
                    push_cluster(&mut hanging, unit, w, level, None);
                } else {
                    push_cluster(&mut pieces, unit, w, level, unit.parent_box);
                    last_kept = Some((level, unit.parent_box));
                }
                continue;
            }
            UnitKind::Atomic { node } => {
                let size = atomics.get(*node).copied().unwrap_or_default();
                let kind = RecordKind::Atomic { node: *node, size };
                Piece {
                    record: Some(record(kind)),
                    width: w,
                    level,
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
                pieces.push(Piece {
                    record: Some(r),
                    width: LayoutUnit::ZERO,
                    level,
                    owner: unit.parent_box,
                    edge: None,
                });
                continue;
            }
            UnitKind::Tab => Piece {
                record: None,
                width: w,
                level,
                owner: unit.parent_box,
                edge: None,
            },
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => {
                continue;
            }
        };
        last_kept = Some((piece.level, piece.owner));
        pieces.push(piece);
    }
    pieces.append(&mut hanging);

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

    // Inline boxes: one fragment per visually contiguous group of members,
    // found in a single pass over the visual order. `open` holds the groups
    // of the boxes enclosing the previous piece, outermost first; a piece
    // keeps the common prefix of its own box chain open, closes the rest
    // and opens groups for the boxes it enters.
    let mut groups: Vec<Group> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut chain: Vec<u32> = Vec::new();
    let mut end = origin;
    for &p in &order {
        let piece = &pieces[p];
        let top = open.last().map(|&g| groups[g].box_index);
        if piece.owner != top {
            chain.clear();
            let mut b = piece.owner;
            while let Some(x) = b {
                chain.push(x);
                b = data.boxes[x as usize].parent;
            }
            chain.reverse();
            let keep = open
                .iter()
                .zip(&chain)
                .take_while(|&(&g, &b)| groups[g].box_index == b)
                .count();
            for g in open.drain(keep..) {
                groups[g].size = end - groups[g].start;
            }
            for &b in &chain[keep..] {
                groups.push(Group {
                    box_index: b,
                    start: starts[p],
                    size: LayoutUnit::ZERO,
                    start_edge: false,
                    end_edge: false,
                    level: piece.level,
                    parent: open.last().copied(),
                    depth: open.len() as u32,
                });
                open.push(groups.len() - 1);
            }
        }
        if let Some((b, is_start)) = piece.edge
            && let Some(&g) = open.last()
            && groups[g].box_index == b
        {
            if is_start {
                groups[g].start_edge = true;
            } else {
                groups[g].end_edge = true;
            }
        }
        end = starts[p] + piece.width;
    }
    for g in open.drain(..) {
        groups[g].size = end - groups[g].start;
    }

    // Output: boxes and content sorted by position; a box precedes the
    // content it starts with, and an outer box precedes an inner one. Among
    // boxes at the same position and depth, the lower box index comes first.
    let mut group_order: Vec<usize> = (0..groups.len()).collect();
    group_order.sort_by_key(|&g| groups[g].box_index);
    let mut out: Vec<(FragmentRecord, Option<usize>, u32)> =
        Vec::with_capacity(groups.len() + pieces.len());
    for g in group_order {
        let group = &groups[g];
        let kind = RecordKind::InlineBox {
            box_index: group.box_index,
            start_edge: group.start_edge,
            end_edge: group.end_edge,
            parent: None,
            reversed: box_reversed(data, group.box_index),
        };
        let record = FragmentRecord {
            kind,
            inline_start: group.start,
            inline_size: group.size,
            level: group.level,
        };
        out.push((record, Some(g), group.depth));
    }
    for &p in &order {
        if let Some(mut r) = pieces[p].record.clone() {
            r.inline_start = starts[p];
            out.push((r, None, u32::MAX));
        }
    }
    out.sort_by_key(|(r, _, d)| (r.inline_start, *d));

    // Parent links: the fragment of the group that was open around a group
    // when it started.
    let mut index_of_group = vec![0usize; groups.len()];
    for (i, (_, g, _)) in out.iter().enumerate() {
        if let Some(g) = g {
            index_of_group[*g] = i;
        }
    }
    for (r, g, _) in &mut out {
        let Some(g) = *g else { continue };
        if let RecordKind::InlineBox { parent: slot, .. } = &mut r.kind {
            *slot = groups[g].parent.map(|p| index_of_group[p] as u32);
        }
    }
    out.into_iter().map(|(r, _, _)| r).collect()
}

/// A visually contiguous part of an inline box on one reordered line.
struct Group {
    box_index: u32,
    start: LayoutUnit,
    size: LayoutUnit,
    start_edge: bool,
    end_edge: bool,
    level: u8,
    /// The group of the enclosing box that was open when this one started.
    parent: Option<usize>,
    /// Nesting depth of the box among the boxes on the line.
    depth: u32,
}
