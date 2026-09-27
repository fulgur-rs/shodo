//! Units: the sequence line breaking walks. There is one unit per cluster
//! (a base character with its combining marks) and one per non-text item.

use std::ops::Range;
use std::sync::Arc;

use super::breaks::BreakAnalysis;
use super::{Item, ItemKind};
use crate::node::{InlineEdges, NodeId, OutOfFlowKind};
use crate::shape::{GlyphStore, ShapedRun};

/// CSS-tailored line break opportunity after a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BreakClass {
    Prohibited,
    Allowed,
    Mandatory,
    Emergency,
    Hyphen,
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

#[derive(Debug)]
pub(crate) struct SharedCluster {
    pub(crate) text: Range<u32>,
    pub(crate) glyphs: Range<u32>,
    pub(crate) units: Range<usize>,
    /// Text slices only; transparent markers can occur between them.
    pub(crate) slices: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    /// Selectable source slices share one unbroken shaping-cluster owner.
    pub(crate) shared_cluster: Option<Arc<SharedCluster>>,
    pub(crate) slice_advance: crate::geometry::LayoutUnit,
    pub(crate) unsafe_to_break: bool,
    pub(crate) unsafe_to_concat: bool,
    pub(crate) kind: UnitKind,
    pub(crate) item: u32,
    pub(crate) text: Range<u32>,
    pub(crate) break_after: BreakClass,
    pub(crate) emergency_min_content: bool,
    pub(crate) level: u8,
    /// Innermost inline box containing the unit (for `Open`/`Close`, the
    /// box's parent).
    pub(crate) parent_box: Option<u32>,
}

impl Unit {
    pub(crate) fn shares_cluster(&self, other: &Self) -> bool {
        self.shared_cluster
            .as_ref()
            .zip(other.shared_cluster.as_ref())
            .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
    }

    pub(crate) fn shaping_text(&self) -> &Range<u32> {
        self.shared_cluster
            .as_ref()
            .map_or(&self.text, |cluster| &cluster.text)
    }
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

/// Whether a unit's innermost enclosing box (`b`) is `target` or nested
/// inside it.
fn inside(boxes: &[InlineBoxInfo], mut b: Option<u32>, target: u32) -> bool {
    while let Some(x) = b {
        if x == target {
            return true;
        }
        b = boxes[x as usize].parent;
    }
    false
}

pub(crate) fn build_units(
    text: &str,
    items: &[Item],
    runs: &[ShapedRun],
    glyphs: &GlyphStore,
    levels: &[u8],
    base_level: u8,
    breaks: &BreakAnalysis,
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
                shared_cluster: None,
                slice_advance: crate::geometry::LayoutUnit::ZERO,
                unsafe_to_break: false,
                unsafe_to_concat: false,
                kind,
                item: index,
                text: item.text.clone(),
                break_after,
                emergency_min_content: false,
                level: base_level,
                parent_box,
            });
        };
        match &item.kind {
            ItemKind::Text => {
                while run_index < runs.len() && runs[run_index].item == index {
                    let run = &runs[run_index];
                    let mut cluster_ends =
                        vec![run.text.end; (run.glyphs.end - run.glyphs.start) as usize];
                    let mut end = run.text.end;
                    for g in run.glyphs.clone().rev() {
                        if g + 1 < run.glyphs.end
                            && glyphs.cluster[(g + 1) as usize] > glyphs.cluster[g as usize]
                        {
                            end = glyphs.cluster[(g + 1) as usize];
                        }
                        cluster_ends[(g - run.glyphs.start) as usize] = end;
                    }
                    for g in run.glyphs.clone() {
                        let cluster = glyphs.cluster[g as usize];
                        let c = text[cluster as usize..].chars().next().unwrap_or(' ');
                        let end = cluster_ends[(g - run.glyphs.start) as usize];
                        if (breaks.graphemes.binary_search(&cluster).is_err()
                            || units.last().is_some_and(|u| u.text.start == cluster))
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
                            last.break_after = breaks.at(end).class;
                            last.emergency_min_content = breaks.at(end).min_content;
                            continue;
                        }
                        let space = c == ' ';
                        units.push(Unit {
                            shared_cluster: None,
                            slice_advance: crate::geometry::LayoutUnit::ZERO,
                            unsafe_to_break: false,
                            unsafe_to_concat: false,
                            kind: UnitKind::Cluster {
                                run: run_index as u32,
                                glyphs: g..g + 1,
                                space,
                            },
                            item: index,
                            text: cluster..end,
                            break_after: breaks.at(end).class,
                            emergency_min_content: breaks.at(end).min_content,
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
                units.push(Unit {
                    shared_cluster: None,
                    slice_advance: crate::geometry::LayoutUnit::ZERO,
                    unsafe_to_break: false,
                    unsafe_to_concat: false,
                    kind: UnitKind::Atomic { node },
                    item: index,
                    text: item.text.clone(),
                    break_after: breaks.at(item.text.end).class,
                    emergency_min_content: breaks.at(item.text.end).min_content,
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
            ItemKind::Tab => push(UnitKind::Tab, breaks.at(item.text.end).class, parent_box),
            ItemKind::BidiControl => {
                push(UnitKind::BidiControl, BreakClass::Prohibited, parent_box)
            }
        }
    }
    let glyphs_store_flags = |g: u32| glyphs.flags[g as usize];
    let mut previous_cluster: Option<usize> = None;
    for i in 0..units.len() {
        if let UnitKind::Cluster { ref glyphs, .. } = units[i].kind {
            let flags = glyphs
                .clone()
                .fold(0, |flags, g| flags | glyphs_store_flags(g));
            units[i].unsafe_to_concat = flags & 2 != 0;
            if let Some(previous) = previous_cluster {
                units[previous].unsafe_to_break = flags & 1 != 0;
                if units[previous].text.start == units[i].text.start {
                    // A resource split inside one shaping cluster is storage
                    // only; it must never create a selectable line break.
                    units[previous].break_after = BreakClass::Prohibited;
                    units[previous].emergency_min_content = false;
                }
            }
            previous_cluster = Some(i);
        } else if matches!(
            units[i].kind,
            UnitKind::ForcedBreak
                | UnitKind::Atomic { .. }
                | UnitKind::BlockInInline { .. }
                | UnitKind::Tab
        ) {
            previous_cluster = None;
        }
    }
    let level_at = |pos: u32| levels.get(pos as usize).copied();

    // An opening or closing box's own unit sits at the text position of an
    // injected bidi control (LRE/RLE/LRI/RLI/FSI on open, PDF/PDI on
    // close), which UAX #9 gives the level in effect *outside* the isolate
    // or embedding, not the level established just inside it (X5a-c, X6a).
    // Skip past bidi controls (and the box's own matching edge, which
    // means it has no content) to the next unit and use its level instead:
    // a following unit's control, if any, sits at the position of *its
    // own* boundary character, which UAX #9 gives the level in effect just
    // outside it — exactly the level established inside this box. A box
    // with no content of its own falls back to the level at the control's
    // text position, as before.
    //
    // A found unit that is itself a nested box's `Close` needs its
    // boundary character's position, one byte before its marker (built
    // after the character, unlike `Open`'s marker, built before) — the
    // same adjustment the fallback below makes for this box's own `Close`.
    let mut resolved: Vec<u8> = Vec::with_capacity(units.len());
    for (i, unit) in units.iter().enumerate() {
        let level = match unit.kind {
            UnitKind::Open { box_index } => {
                let mut content = None;
                for u in &units[i + 1..] {
                    match &u.kind {
                        UnitKind::BidiControl => continue,
                        UnitKind::Close { box_index: b } if *b == box_index => break,
                        _ => {
                            if inside(&boxes, u.parent_box, box_index) {
                                content = level_at(u.text.start);
                            }
                            break;
                        }
                    }
                }
                content.or_else(|| level_at(unit.text.start))
            }
            UnitKind::Close { box_index } => {
                let mut content = None;
                for u in units[..i].iter().rev() {
                    match &u.kind {
                        UnitKind::BidiControl => continue,
                        UnitKind::Open { box_index: b } if *b == box_index => break,
                        UnitKind::Close { .. } => {
                            if inside(&boxes, u.parent_box, box_index) {
                                content = u.text.start.checked_sub(1).and_then(level_at);
                            }
                            break;
                        }
                        _ => {
                            if inside(&boxes, u.parent_box, box_index) {
                                content = level_at(u.text.start);
                            }
                            break;
                        }
                    }
                }
                content.or_else(|| unit.text.start.checked_sub(1).and_then(level_at))
            }
            _ => level_at(unit.text.start),
        };
        resolved.push(level.unwrap_or(base_level));
    }
    for (unit, level) in units.iter_mut().zip(resolved) {
        unit.level = level;
    }
    UnitList {
        units,
        boxes,
        float_count,
    }
}
