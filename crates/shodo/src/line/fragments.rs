//! Fragment records: the compact per-line table behind the public views.

use std::ops::Range;

use unicode_bidi::{BidiInfo, Level};

use crate::analysis::units::{Unit, UnitKind};
use crate::geometry::{BaselineKind, LayoutUnit};
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{AtomicSize, AtomicSizes, ParagraphData};

fn normalized_size(atomics: &AtomicSizes, node: NodeId) -> AtomicSize {
    crate::sanitize::atomic(
        atomics.get(node).copied().unwrap_or_default(),
        &mut crate::limits::WarningSink::new(Some(0)),
        &mut crate::geometry::Saturation::default(),
    )
}

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
        source: GlyphSource,
        run: u32,
        glyphs: Range<u32>,
        item: u32,
        text: Range<u32>,
    },
    Atomic {
        node: NodeId,
        size: AtomicSize,
        unit: u32,
        baseline_kind: BaselineKind,
    },
    InlineBox {
        box_index: u32,
        start_edge: bool,
        end_edge: bool,
        slice_offset: Option<f32>,
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

#[derive(Clone, Copy, Debug)]
pub(crate) enum GlyphSource {
    Shared,
    Overlay {
        glyphs: (u32, u32),
        clusters: (u32, u32),
        run: Option<u32>,
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

#[derive(Clone, Debug)]
pub(crate) struct TabSlot {
    pub(crate) unit: u32,
    pub(crate) start: LayoutUnit,
    pub(crate) width: LayoutUnit,
}
type Built = (Vec<FragmentRecord>, Vec<TabSlot>);

/// Collapsible terminal spaces retain their source/glyph advance, but are
/// removed from inline background and border geometry (CSS Text 3 §4.1.2).
fn trims_box(data: &ParagraphData, i: usize, hang_start: usize) -> bool {
    i >= hang_start && super::quirk::trims(data, i)
}

/// Builds the records of one line in logical order.
fn build_logical(
    data: &ParagraphData,
    units: Range<usize>,
    hang_start: usize,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
    visible_hyphen: Option<u32>,
) -> Built {
    let hidden_hyphen = |unit: &Unit| {
        data.text
            .get(unit.text.start as usize..unit.text.end as usize)
            == Some("\u{ad}")
            && visible_hyphen != Some(unit.text.start)
    };
    let mut out: Vec<FragmentRecord> = Vec::new();
    let mut tabs = Vec::new();
    let mut open: Vec<(usize, LayoutUnit)> = Vec::new();
    let mut shared_record: Option<(usize, usize)> = None;
    let mut pos = origin;
    let mut trimmed = LayoutUnit::ZERO;
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
        let cloned = super::decoration::cloned(data, box_index);
        let parent = open.last().map(|&(r, _)| r as u32);
        out.push(FragmentRecord {
            kind: RecordKind::InlineBox {
                box_index,
                start_edge: cloned,
                end_edge: false,
                slice_offset: None,
                parent,
                reversed: box_reversed(data, box_index),
            },
            inline_start: pos,
            inline_size: LayoutUnit::ZERO,
            level: level_at_start,
        });
        open.push((out.len() - 1, trimmed));
        if cloned {
            pos = pos
                + LayoutUnit::from_f32_round(
                    data.boxes[box_index as usize].edges.inline_start_total(),
                    &mut Default::default(),
                );
        }
    }

    for (k, i) in units.enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        if trims_box(data, i, hang_start) {
            trimmed = trimmed + w;
        }
        match &unit.kind {
            UnitKind::Open { box_index } => {
                let parent = open.last().map(|&(r, _)| r as u32);
                out.push(FragmentRecord {
                    kind: RecordKind::InlineBox {
                        box_index: *box_index,
                        start_edge: true,
                        end_edge: false,
                        slice_offset: None,
                        parent,
                        reversed: box_reversed(data, *box_index),
                    },
                    inline_start: pos,
                    inline_size: LayoutUnit::ZERO,
                    level: unit.level,
                });
                open.push((out.len() - 1, trimmed));
                pos = pos + w;
            }
            UnitKind::Close { .. } => {
                pos = pos + w;
                if let Some((r, before)) = open.pop() {
                    out[r].inline_size = pos - out[r].inline_start - (trimmed - before);
                    if let RecordKind::InlineBox { end_edge, .. } = &mut out[r].kind {
                        *end_edge = true;
                    }
                }
            }
            UnitKind::Cluster { run, glyphs, .. } => {
                if let Some((previous, record)) = shared_record
                    && unit.shares_cluster(&data.units[previous])
                {
                    out[record].inline_size = out[record].inline_size + w;
                    if let RecordKind::Glyphs { text, .. } = &mut out[record].kind {
                        text.end = unit.text.end;
                    }
                    pos = pos + w;
                    continue;
                }
                if hidden_hyphen(unit) {
                    continue;
                }
                if let Some(last) = out.last_mut()
                    && last.level == unit.level
                    && let RecordKind::Glyphs {
                        run: r,
                        glyphs: g,
                        item,
                        text,
                        ..
                    } = &mut last.kind
                    && *r == *run
                    && *item == unit.item
                    && (g.end == glyphs.start
                        || unit.shared_cluster.as_ref().is_some_and(|shared| {
                            g.end == shared.glyphs.end && g.start <= shared.glyphs.start
                        }))
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
                            source: GlyphSource::Shared,
                        },
                        inline_start: pos,
                        inline_size: w,
                        level: unit.level,
                    });
                }
                shared_record = unit.shared_cluster.as_ref().map(|_| (i, out.len() - 1));
                pos = pos + w;
            }
            UnitKind::Atomic { node } => {
                let size = normalized_size(atomics, *node);
                out.push(FragmentRecord {
                    kind: RecordKind::Atomic {
                        node: *node,
                        size,
                        unit: i as u32,
                        baseline_kind: data
                            .baseline_kind(*node)
                            .unwrap_or(BaselineKind::Alphabetic),
                    },
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
            UnitKind::Tab => {
                tabs.push(TabSlot {
                    unit: i as u32,
                    start: pos,
                    width: w,
                });
                pos = pos + w;
            }
            // Anonymous ruby bases reserve space on their source-free marker.
            // Ordinary bidi controls retain their zero width.
            UnitKind::BidiControl => pos = pos + w,
            UnitKind::ForcedBreak | UnitKind::BlockInInline { .. } => {}
        }
    }
    // Boxes that continue on the next line end here without their end edge.
    for (r, before) in open.into_iter().rev() {
        if let RecordKind::InlineBox {
            box_index,
            end_edge,
            ..
        } = &mut out[r].kind
            && super::decoration::cloned(data, *box_index)
        {
            *end_edge = true;
            pos = pos
                + LayoutUnit::from_f32_round(
                    data.boxes[*box_index as usize].edges.inline_end_total(),
                    &mut Default::default(),
                );
        }
        out[r].inline_size = pos - out[r].inline_start - (trimmed - before);
    }
    (out, tabs)
}

/// Builds the records of one line in visual order (UAX #9 L2). Units from
/// `hang_start` on are the line's hanging trailing spaces and what follows
/// them (see `Scan::hang_start`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    hang_start: usize,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
    visible_hyphen: Option<u32>,
    leading: Option<&[LayoutUnit]>,
) -> Built {
    let base = data.base_level;
    let (mut records, mut tabs) = if data.units[units.clone()].iter().all(|u| u.level == base) {
        build_logical(
            data,
            units.clone(),
            hang_start,
            widths,
            origin,
            atomics,
            visible_hyphen,
        )
    } else {
        build_bidi(
            data,
            units.clone(),
            hang_start,
            widths,
            origin,
            atomics,
            visible_hyphen,
        )
    };
    // Ruby alignment can pad a nested container as one typographic group.
    // Edge-unit advances move all descendants; remove these external gaps
    // from the wrapper's own geometry so its annotation shares that move.
    if let Some(leading) = leading {
        let mut gaps: crate::hashing::FastMap<u32, (LayoutUnit, LayoutUnit)> =
            crate::hashing::FastMap::default();
        for (k, i) in units.clone().enumerate() {
            if leading[k] == LayoutUnit::ZERO {
                continue;
            }
            match data.units[i].kind {
                UnitKind::Open { box_index } => {
                    gaps.entry(box_index)
                        .or_insert((LayoutUnit::ZERO, LayoutUnit::ZERO))
                        .0 = leading[k]
                }
                UnitKind::Close { box_index } => {
                    gaps.entry(box_index)
                        .or_insert((LayoutUnit::ZERO, LayoutUnit::ZERO))
                        .1 = leading[k]
                }
                _ => {}
            }
        }
        for record in &mut records {
            if let RecordKind::InlineBox {
                box_index,
                reversed,
                ..
            } = record.kind
                && let Some((before, after)) = gaps.get(&box_index)
            {
                record.inline_start = record.inline_start + if reversed { *after } else { *before };
                record.inline_size = record.inline_size - *before - *after;
            }
        }
    }
    if data.combine_spans.is_empty() {
        return (records, tabs);
    }
    // Selectable source slices paint from a single composition origin.
    // Before-spacing belongs ahead of that square; after-spacing belongs
    // after its last unit, regardless of source fragmentation or bidi order.
    let mut starts = crate::hashing::FastMap::default();
    let mut before = crate::hashing::FastMap::default();
    for (k, i) in units.enumerate() {
        if let Some(span) = data.combine_at_text(data.units[i].text.start) {
            let amount = leading.map_or(LayoutUnit::ZERO, |values| values[k]);
            let value = before.entry(span.text.start).or_insert(LayoutUnit::ZERO);
            *value = *value + amount;
        }
    }
    for record in &records {
        if let RecordKind::Glyphs { text, .. } = &record.kind
            && let Some(span) = data.combine_at_text(text.start)
        {
            let start = starts.entry(span.text.start).or_insert(record.inline_start);
            *start = (*start).min(record.inline_start);
        }
    }
    for tab in &tabs {
        if let Some(span) = data.combine_at_text(data.units[tab.unit as usize].text.start) {
            let start = starts.entry(span.text.start).or_insert(tab.start);
            *start = (*start).min(tab.start);
        }
    }
    for record in &mut records {
        if let RecordKind::Glyphs { text, .. } = &record.kind
            && let Some(span) = data.combine_at_text(text.start)
        {
            record.inline_start = starts[&span.text.start] + before[&span.text.start];
        }
    }
    for tab in &mut tabs {
        if let Some(span) = data.combine_at_text(data.units[tab.unit as usize].text.start) {
            tab.start = starts[&span.text.start] + before[&span.text.start];
        }
    }
    (records, tabs)
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
    tab: Option<u32>,
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
/// continues the same run, item, level and owner. Returns the piece receiving
/// this unit's advance (a shared cluster continuation can be a separate piece).
fn push_cluster(
    list: &mut Vec<Piece>,
    unit: &Unit,
    width: LayoutUnit,
    level: u8,
    owner: Option<u32>,
    shared_record: &mut Option<(std::sync::Arc<crate::analysis::units::SharedCluster>, usize)>,
    continuations: &mut Vec<(usize, usize)>,
) -> usize {
    let UnitKind::Cluster { run, glyphs, .. } = &unit.kind else {
        unreachable!("cluster piece requires a cluster unit");
    };
    let (run, item, text) = (*run, unit.item, &unit.text);
    if let Some(shared) = &unit.shared_cluster
        && let Some((previous, record)) = shared_record.as_ref()
        && std::sync::Arc::ptr_eq(shared, previous)
    {
        let record = *record;
        if let Some(glyph) = &mut list[record].record {
            glyph.inline_size = glyph.inline_size + width;
            if let RecordKind::Glyphs { text, .. } = &mut glyph.kind {
                text.end = unit.text.end;
            }
        }
        continuations.push((record, list.len()));
        list.push(Piece {
            record: None,
            width,
            level,
            owner,
            edge: None,
            tab: None,
        });
        return list.len() - 1;
    }
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
                    ..
                },
            inline_size,
            ..
        }) = &mut last.record
        && *r == run
        && *it == item
        && (g.end == glyphs.start
            || unit
                .shared_cluster
                .as_ref()
                .is_some_and(|shared| g.end == shared.glyphs.end && g.start <= shared.glyphs.start))
    {
        g.end = glyphs.end;
        t.end = text.end;
        *inline_size = *inline_size + width;
        last.width = last.width + width;
        *shared_record = unit
            .shared_cluster
            .as_ref()
            .map(|c| (std::sync::Arc::clone(c), list.len() - 1));
        return list.len() - 1;
    }
    list.push(Piece {
        record: Some(FragmentRecord {
            kind: RecordKind::Glyphs {
                run,
                glyphs: glyphs.clone(),
                item,
                text: text.clone(),
                source: GlyphSource::Shared,
            },
            inline_start: LayoutUnit::ZERO,
            inline_size: width,
            level,
        }),
        width,
        level,
        owner,
        edge: None,
        tab: None,
    });
    *shared_record = unit
        .shared_cluster
        .as_ref()
        .map(|c| (std::sync::Arc::clone(c), list.len() - 1));
    list.len() - 1
}

fn build_bidi(
    data: &ParagraphData,
    units: Range<usize>,
    hang_start: usize,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
    visible_hyphen: Option<u32>,
) -> Built {
    let base = data
        .bidi_paragraph_at_unit(units.start)
        .map_or(data.base_level, |p| p.base_level);
    let bidi_start = super::whitespace::bidi_trailing(data, units.start, units.end);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut box_trims: crate::hashing::FastMap<usize, LayoutUnit> =
        crate::hashing::FastMap::default();
    // Hanging trailing spaces, placed after everything else on the line.
    let mut hanging: Vec<Piece> = Vec::new();
    // Level and owner of the last piece that is neither a hanging space nor
    // an out-of-flow anchor.
    let mut last_kept: Option<(u8, Option<u32>)> = None;
    for b in super::decoration::chain(data, units.start)
        .into_iter()
        .rev()
        .filter(|b| super::decoration::cloned(data, *b))
    {
        pieces.push(Piece {
            record: None,
            width: LayoutUnit::from_f32_round(
                data.boxes[b as usize].edges.inline_start_total(),
                &mut Default::default(),
            ),
            level: data.units[units.start].level,
            owner: Some(b),
            edge: Some((b, true)),
            tab: None,
        });
    }
    let mut shared_record = None;
    let mut hanging_shared = None;
    let mut continuations = Vec::new();
    let mut hanging_continuations = Vec::new();
    for (k, i) in units.clone().enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        let trailing = i >= hang_start;
        // UAX #9 L1: trailing whitespace and segment separators (tabs) take
        // the paragraph embedding level. `BidiInfo` levels are resolved
        // before L1, so it is applied here, per line and per unit.
        let level = match &unit.kind {
            UnitKind::Cluster { .. } if i >= bidi_start => base,
            UnitKind::Tab if data.combine_at_text(unit.text.start).is_none() => base,
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
                tab: None,
            },
            UnitKind::Close { box_index } => Piece {
                record: None,
                width: w,
                level,
                owner: Some(*box_index),
                edge: Some((*box_index, false)),
                tab: None,
            },
            UnitKind::Cluster { space, .. } => {
                if data
                    .text
                    .get(unit.text.start as usize..unit.text.end as usize)
                    == Some("\u{ad}")
                    && visible_hyphen != Some(unit.text.start)
                {
                    continue;
                }
                if trailing && *space && unit.level != base {
                    // L1 moves hanging spaces to the paragraph level.
                    // Collapsible spaces no longer paint their box and can
                    // leave its owner to avoid splitting it around the space.
                    // Preserved spaces still belong to the painted box.
                    push_cluster(
                        &mut hanging,
                        unit,
                        w,
                        level,
                        if trims_box(data, i, hang_start) {
                            None
                        } else {
                            unit.parent_box
                        },
                        &mut hanging_shared,
                        &mut hanging_continuations,
                    );
                } else {
                    let piece = push_cluster(
                        &mut pieces,
                        unit,
                        w,
                        level,
                        unit.parent_box,
                        &mut shared_record,
                        &mut continuations,
                    );
                    if trims_box(data, i, hang_start) {
                        let amount = box_trims.entry(piece).or_insert(LayoutUnit::ZERO);
                        *amount = *amount + w;
                    }
                    last_kept = Some((level, unit.parent_box));
                }
                continue;
            }
            UnitKind::Atomic { node } => {
                let size = normalized_size(atomics, *node);
                let kind = RecordKind::Atomic {
                    node: *node,
                    size,
                    unit: i as u32,
                    baseline_kind: data
                        .baseline_kind(*node)
                        .unwrap_or(BaselineKind::Alphabetic),
                };
                Piece {
                    record: Some(record(kind)),
                    width: w,
                    level,
                    owner: unit.parent_box,
                    edge: None,
                    tab: None,
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
                    tab: None,
                });
                continue;
            }
            UnitKind::Tab => Piece {
                record: None,
                width: w,
                level,
                owner: unit.parent_box,
                edge: None,
                tab: Some(i as u32),
            },
            UnitKind::BidiControl if w != LayoutUnit::ZERO => Piece {
                record: None,
                width: w,
                level,
                owner: unit.parent_box,
                edge: None,
                tab: None,
            },
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => {
                continue;
            }
        };
        last_kept = Some((piece.level, piece.owner));
        pieces.push(piece);
    }
    for b in super::decoration::chain(data, units.end)
        .into_iter()
        .filter(|b| super::decoration::cloned(data, *b))
    {
        pieces.push(Piece {
            record: None,
            width: LayoutUnit::from_f32_round(
                data.boxes[b as usize].edges.inline_end_total(),
                &mut Default::default(),
            ),
            level: last_kept.map_or(base, |v| v.0),
            owner: Some(b),
            edge: Some((b, false)),
            tab: None,
        });
    }
    continuations.extend(
        hanging_continuations
            .into_iter()
            .map(|(a, b)| (a + pieces.len(), b + pieces.len())),
    );
    pieces.append(&mut hanging);

    // Visual order, left to right; from the inline-start edge that is the
    // reverse order when the paragraph is right-to-left.
    let levels: Vec<Level> = pieces
        .iter()
        .map(|p| Level::new(p.level).unwrap_or_else(|_| Level::ltr()))
        .collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if data.base_level % 2 == 1 {
        order.reverse();
    }
    let mut starts = vec![LayoutUnit::ZERO; pieces.len()];
    let mut pos = origin;
    for &p in &order {
        starts[p] = pos;
        pos = pos + pieces[p].width;
    }

    let mut glyph_starts = starts.clone();
    for (owner, continuation) in continuations {
        glyph_starts[owner] = glyph_starts[owner].min(starts[continuation]);
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
    let mut trimmed = LayoutUnit::ZERO;
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
                groups[g].size = end - groups[g].start - (trimmed - groups[g].trimmed_before);
            }
            for &b in &chain[keep..] {
                groups.push(Group {
                    box_index: b,
                    start: starts[p],
                    trimmed_before: trimmed,
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
        trimmed = trimmed + box_trims.get(&p).copied().unwrap_or(LayoutUnit::ZERO);
        end = starts[p] + piece.width;
    }
    for g in open.drain(..) {
        groups[g].size = end - groups[g].start - (trimmed - groups[g].trimmed_before);
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
            slice_offset: None,
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
            r.inline_start = glyph_starts[p];
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
    let tabs = pieces
        .iter()
        .enumerate()
        .filter_map(|(p, piece)| {
            piece.tab.map(|unit| TabSlot {
                unit,
                start: starts[p],
                width: piece.width,
            })
        })
        .collect();
    (out.into_iter().map(|(r, _, _)| r).collect(), tabs)
}

/// A visually contiguous part of an inline box on one reordered line.
struct Group {
    box_index: u32,
    start: LayoutUnit,
    trimmed_before: LayoutUnit,
    size: LayoutUnit,
    start_edge: bool,
    end_edge: bool,
    level: u8,
    /// The group of the enclosing box that was open when this one started.
    parent: Option<usize>,
    /// Nesting depth of the box among the boxes on the line.
    depth: u32,
}
