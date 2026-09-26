//! Mapping between caller text offsets (UTF-8 bytes within a node's text)
//! and offsets in the processed text shodo lays out. White-space collapsing
//! removes characters and text-transform can change lengths, so the mapping
//! is a list of runs.

use std::ops::Range;

use crate::node::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingKind {
    /// Same length on both sides.
    Identity,
    /// Removed by white-space collapsing; `text` is empty.
    Collapsed,
    /// Length changed (for example `ß` to `SS`); indivisible.
    Expanded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingUnit {
    pub kind: MappingKind,
    pub node: NodeId,
    pub dom: Range<u32>,
    pub text: Range<u32>,
}

/// Which side of a boundary a position belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOrigin {
    Dom {
        node: NodeId,
        offset: u32,
    },
    /// Generated content, atomics and control characters have no DOM offset.
    Generated {
        node: NodeId,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OffsetMapping {
    units: Vec<MappingUnit>,
    generated: Vec<(Range<u32>, NodeId)>,
}

/// A scalar transform, or a coalesced byte-preserving sequence. Expanded
/// spans also cover one-to-many scalar changes with equal UTF-8 byte lengths.
pub(crate) struct TransformSpan {
    pub(crate) old: Range<u32>,
    pub(crate) new: Range<u32>,
    pub(crate) kind: MappingKind,
}

impl TransformSpan {
    pub(crate) fn map_position(spans: &[Self], pos: u32) -> u32 {
        let index = spans.partition_point(|s| s.old.end <= pos);
        let Some(span) = spans.get(index) else {
            return spans.last().map_or(0, |s| s.new.end);
        };
        match span.kind {
            MappingKind::Identity => span.new.start + pos.saturating_sub(span.old.start),
            _ => span.new.start,
        }
    }
}

impl OffsetMapping {
    pub(crate) fn remap_text(&mut self, spans: &[TransformSpan]) {
        let units = std::mem::take(&mut self.units);
        let generated = std::mem::take(&mut self.generated);
        for unit in units {
            if unit.kind != MappingKind::Identity {
                self.push_unit(MappingUnit {
                    text: TransformSpan::map_position(spans, unit.text.start)
                        ..TransformSpan::map_position(spans, unit.text.end),
                    ..unit
                });
                continue;
            }
            let first = spans.partition_point(|s| s.old.end <= unit.text.start);
            for span in &spans[first..] {
                if span.old.start >= unit.text.end {
                    break;
                }
                let begin = span.old.start.max(unit.text.start);
                let end = span.old.end.min(unit.text.end);
                let new_text = if span.kind == MappingKind::Identity {
                    span.new.start + begin - span.old.start..span.new.start + end - span.old.start
                } else {
                    span.new.clone()
                };
                self.push_unit(MappingUnit {
                    kind: span.kind,
                    node: unit.node,
                    dom: unit.dom.start.saturating_add(begin - unit.text.start)
                        ..unit.dom.start.saturating_add(end - unit.text.start),
                    text: new_text,
                });
            }
        }
        for (range, node) in generated {
            self.push_generated(
                TransformSpan::map_position(spans, range.start)
                    ..TransformSpan::map_position(spans, range.end),
                node,
            );
        }
    }

    pub(crate) fn push_unit(&mut self, unit: MappingUnit) {
        if let Some(last) = self.units.last_mut()
            && last.kind == unit.kind
            && unit.kind != MappingKind::Expanded
            && last.node == unit.node
            && last.dom.end == unit.dom.start
            && last.text.end == unit.text.start
        {
            last.dom.end = unit.dom.end;
            last.text.end = unit.text.end;
            return;
        }
        self.units.push(unit);
    }

    pub(crate) fn push_generated(&mut self, text: Range<u32>, node: NodeId) {
        if let Some((last, last_node)) = self.generated.last_mut()
            && *last_node == node
            && last.end == text.start
        {
            last.end = text.end;
            return;
        }
        self.generated.push((text, node));
    }

    pub fn units(&self) -> &[MappingUnit] {
        &self.units
    }

    /// Processed-text offset of a caller offset. Offsets inside a collapsed
    /// run map to the end of the gap; offsets inside an expanded run round to
    /// its start.
    pub fn dom_to_text(&self, node: NodeId, offset: u32) -> Option<(u32, Affinity)> {
        let mut at_end = None;
        for u in self.units.iter().filter(|u| u.node == node) {
            if u.dom.start <= offset && offset < u.dom.end {
                // Saturate: `dom` came from a caller-supplied offset and may
                // already be clamped near `u32::MAX` (see `push_unit`).
                let text = match u.kind {
                    MappingKind::Identity => u.text.start.saturating_add(offset - u.dom.start),
                    MappingKind::Collapsed => u.text.end,
                    MappingKind::Expanded => u.text.start,
                };
                return Some((text, Affinity::Downstream));
            }
            if offset == u.dom.end {
                at_end = Some((u.text.end, Affinity::Upstream));
            }
        }
        at_end
    }

    /// Caller offset of a processed-text offset.
    pub fn text_to_dom(&self, offset: u32, affinity: Affinity) -> Option<TextOrigin> {
        let mut downstream = None;
        let mut upstream = None;
        for (r, node) in &self.generated {
            if r.start < offset && offset < r.end {
                return Some(TextOrigin::Generated { node: *node });
            }
            if r.start == offset && !r.is_empty() {
                downstream = Some(TextOrigin::Generated { node: *node });
            }
            if r.end == offset && !r.is_empty() {
                upstream = Some(TextOrigin::Generated { node: *node });
            }
        }
        for u in self
            .units
            .iter()
            .filter(|u| u.kind != MappingKind::Collapsed)
        {
            if downstream.is_none() && u.text.start <= offset && offset < u.text.end {
                // Saturate for the same reason as in `dom_to_text`: `u.dom.start`
                // may already be clamped near `u32::MAX`.
                let dom = match u.kind {
                    MappingKind::Identity => u.dom.start.saturating_add(offset - u.text.start),
                    _ => u.dom.start,
                };
                downstream = Some(TextOrigin::Dom {
                    node: u.node,
                    offset: dom,
                });
            }
            if u.text.end == offset {
                upstream = Some(TextOrigin::Dom {
                    node: u.node,
                    offset: u.dom.end,
                });
            }
        }
        match affinity {
            Affinity::Upstream => upstream.or(downstream),
            Affinity::Downstream => downstream.or(upstream),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(kind: MappingKind, dom: Range<u32>, text: Range<u32>) -> MappingUnit {
        MappingUnit {
            kind,
            node: NodeId(1),
            dom,
            text,
        }
    }

    #[test]
    fn affinity_selects_dom_or_generated_on_both_boundaries() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..1, 0..1));
        m.push_generated(1..3, NodeId(9));
        m.push_unit(unit(MappingKind::Identity, 1..2, 3..4));
        assert_eq!(
            m.text_to_dom(1, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(
            m.text_to_dom(3, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 1
            })
        );
        assert_eq!(
            m.text_to_dom(2, Affinity::Upstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
    }

    #[test]
    fn identity_units_merge_and_map_both_ways() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..4, 2..4));
        assert_eq!(m.units().len(), 1);
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(3, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 3
            })
        );
    }

    #[test]
    fn collapsed_offsets_map_to_the_end_of_the_gap() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2)); // "a "
        m.push_unit(unit(MappingKind::Collapsed, 2..4, 2..2)); // "  " removed
        m.push_unit(unit(MappingKind::Identity, 4..5, 2..3)); // "b"
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((2, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(2, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 4
            })
        );
        assert_eq!(
            m.text_to_dom(2, Affinity::Upstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 2
            })
        );
    }

    #[test]
    fn expanded_interiors_round_to_the_start() {
        let mut m = OffsetMapping::default();
        // "ß" (2 bytes) became "SS" (2 bytes) in one unit, then "x".
        m.push_unit(unit(MappingKind::Expanded, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..3, 2..3));
        assert_eq!(m.dom_to_text(NodeId(1), 1), Some((0, Affinity::Downstream)));
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Dom {
                node: NodeId(1),
                offset: 0
            })
        );
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Upstream)));
    }

    #[test]
    fn generated_text_has_no_dom_offset() {
        let mut m = OffsetMapping::default();
        m.push_generated(0..3, NodeId(9));
        assert_eq!(
            m.text_to_dom(1, Affinity::Downstream),
            Some(TextOrigin::Generated { node: NodeId(9) })
        );
        assert_eq!(m.dom_to_text(NodeId(9), 0), None);
    }
}
