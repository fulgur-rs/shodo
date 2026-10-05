//! Content and paint geometry for the selected line profile.
use super::super::metrics::RecordProfile;
use super::{MetricIndex, Selection};
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext};
use std::collections::BTreeMap;
use std::ops::Range;

// Store real displacements separately from quantized font edges. Group
// translation happens only after solving the actual selected line metrics.
#[derive(Clone, Copy, Debug)]
pub(super) struct ContentBounds {
    top: f64,
    bottom: f64,
}
impl ContentBounds {
    fn join(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Self {
                top: a.top.min(b.top),
                bottom: a.bottom.max(b.bottom),
            }),
            (a, None) | (None, a) => a,
        }
    }
    pub(super) fn shift(self, delta: f64) -> Self {
        Self {
            top: self.top + delta,
            bottom: self.bottom + delta,
        }
    }
    fn fixed(self, sat: &mut Saturation) -> crate::ruby::geometry::Bounds {
        crate::ruby::geometry::Bounds {
            top: LayoutUnit::from_f32_round(self.top as f32, sat),
            bottom: LayoutUnit::from_f32_round(self.bottom as f32, sat),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ContentSummary {
    pub(super) normal: Option<ContentBounds>,
    pub(super) top: Option<ContentBounds>,
    pub(super) bottom: Option<ContentBounds>,
    pub(super) raw: Option<ContentBounds>,
}
impl ContentSummary {
    pub(super) fn join(self, other: Self) -> Self {
        Self {
            normal: ContentBounds::join(self.normal, other.normal),
            top: ContentBounds::join(self.top, other.top),
            bottom: ContentBounds::join(self.bottom, other.bottom),
            raw: ContentBounds::join(self.raw, other.raw),
        }
    }
}
pub(super) fn content_bounds(
    data: &ParagraphData,
    profile: RecordProfile,
    a: LayoutUnit,
    d: LayoutUnit,
) -> ContentBounds {
    let sign = if data.style.writing_mode == WritingMode::VerticalLr {
        -1.0
    } else {
        1.0
    };
    let center = sign * f64::from(profile.shift);
    ContentBounds {
        top: center - f64::from(a.to_f32()),
        bottom: center + f64::from(d.to_f32()),
    }
}

impl MetricIndex {
    fn content_query(
        &self,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, ContentSummary>,
        excluded: &[Range<usize>],
        removed: &[Range<usize>],
        cx: &mut LayoutContext,
    ) -> ContentSummary {
        self.content_node(
            1,
            0..self.contents.len() / 2,
            range,
            replacements,
            excluded,
            removed,
            cx,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn content_node(
        &self,
        node: usize,
        source: Range<usize>,
        range: &Range<usize>,
        replacements: &BTreeMap<usize, ContentSummary>,
        excluded: &[Range<usize>],
        removed: &[Range<usize>],
        _cx: &mut LayoutContext,
    ) -> ContentSummary {
        #[cfg(test)]
        {
            _cx.ruby_measure_visits += 1;
        }
        if source.end <= range.start
            || range.end <= source.start
            || excluded
                .iter()
                .any(|r| r.start <= source.start && source.end <= r.end)
        {
            return ContentSummary::default();
        }
        if range.start <= source.start
            && source.end <= range.end
            && replacements.range(source.clone()).next().is_none()
            && !excluded
                .iter()
                .any(|r| r.start < source.end && source.start < r.end)
        {
            if removed
                .iter()
                .any(|r| r.start <= source.start && source.end <= r.end)
            {
                return self.nonglyph_contents[node];
            }
            if !removed
                .iter()
                .any(|r| r.start < source.end && source.start < r.end)
            {
                return self.contents[node];
            }
        }
        if source.end - source.start == 1 {
            return replacements.get(&source.start).copied().unwrap_or(
                if removed.iter().any(|r| r.contains(&source.start)) {
                    self.nonglyph_contents[node]
                } else {
                    self.contents[node]
                },
            );
        }
        let mid = (source.start + source.end) / 2;
        self.content_node(
            node * 2,
            source.start..mid,
            range,
            replacements,
            excluded,
            removed,
            _cx,
        )
        .join(self.content_node(
            node * 2 + 1,
            mid..source.end,
            range,
            replacements,
            excluded,
            removed,
            _cx,
        ))
    }
    fn group_delta(&self, data: &ParagraphData, selection: &Selection, i: usize) -> f64 {
        let g = &self.groups[i];
        let bounds = selection.partial.get(&i).copied().unwrap_or(g.bounds);
        // Preserve the accepted solver's floating-point displacement before
        // quantizing the final font-content edge.
        let delta = if g.bottom {
            selection.height - selection.above - bounds.bottom
        } else {
            -selection.above - bounds.top
        };
        f64::from(delta)
            * if data.style.writing_mode == WritingMode::VerticalLr {
                -1.0
            } else {
                1.0
            }
    }
    fn box_delta(&self, data: &ParagraphData, selection: &Selection, b: u32) -> f64 {
        self.boxes[b as usize].group.map_or(0.0, |g| {
            self.group_delta(data, selection, self.group_keys[&u64::from(g)])
        })
    }
    fn selected_content(
        &self,
        data: &ParagraphData,
        selection: &Selection,
        range: &Range<usize>,
        cx: &mut LayoutContext,
    ) -> Option<ContentBounds> {
        let excluded: Vec<_> = selection
            .partial
            .keys()
            .map(|i| self.groups[*i].units.clone())
            .collect();
        let summary = self.content_query(
            range,
            &selection.replacements,
            &excluded,
            &selection.removed,
            cx,
        );
        let sign = if data.style.writing_mode == WritingMode::VerticalLr {
            -1.0
        } else {
            1.0
        };
        let mut result = ContentBounds::join(
            summary.normal,
            summary
                .top
                .map(|r| r.shift(-sign * f64::from(selection.above))),
        );
        result = ContentBounds::join(
            result,
            summary
                .bottom
                .map(|r| r.shift(sign * f64::from(selection.height - selection.above))),
        );
        for i in selection.partial.keys() {
            let g = &self.groups[*i];
            let selected = range.start.max(g.units.start)..range.end.min(g.units.end);
            if selected.start < selected.end {
                let raw = self
                    .content_query(
                        &selected,
                        &selection.replacements,
                        &[],
                        &selection.removed,
                        cx,
                    )
                    .raw;
                result = ContentBounds::join(
                    result,
                    raw.map(|r| r.shift(self.group_delta(data, selection, *i))),
                );
            }
        }
        result
    }

    /// The parts of `selection` that container geometry reads.
    fn digest(&self, selection: &Selection) -> SelectionDigest {
        let bits = |b: Option<ContentBounds>| b.map(|b| (b.top.to_bits(), b.bottom.to_bits()));
        let mut partial: Vec<_> = selection
            .partial
            .iter()
            .map(|(i, b)| {
                (
                    self.groups[*i].units.clone(),
                    b.top.to_bits(),
                    b.bottom.to_bits(),
                )
            })
            .collect();
        partial.sort_unstable_by_key(|p| (p.0.start, p.0.end));
        SelectionDigest {
            height: selection.height.to_bits(),
            above: selection.above.to_bits(),
            partial,
            removed: selection.removed.clone(),
            replacements: selection
                .replacements
                .iter()
                .map(|(u, s)| {
                    (
                        *u,
                        [bits(s.normal), bits(s.top), bits(s.bottom), bits(s.raw)],
                    )
                })
                .collect(),
        }
    }
}

pub(crate) struct ContentGeometry {
    pub(crate) areas: Vec<Option<crate::ruby::geometry::Bounds>>,
    pub(crate) leaf_areas: Vec<Option<crate::ruby::geometry::Bounds>>,
    pub(crate) paints: Vec<crate::ruby::geometry::Bounds>,
    pub(crate) contents: Vec<crate::ruby::geometry::Bounds>,
}

/// Edge windows and selected line profile of one `selected` range, shared by
/// the containers of one candidate. Reuse replays the recorded reshape
/// charges and saturation exactly (`line::replay`), or measures afresh.
///
/// A detached share (`ruby::accumulate`) keeps the profile's effects out of
/// the current container's recording: its charges count for the frame below
/// it, and `ContainerNote` records how many calls the container made and
/// their Saturation.
///
/// Contract: a container recorded under a detached share has own effects
/// that EXCLUDE the profile's reshape charges (and, once `without_sat` takes
/// out `ContainerNote::sat`, its Saturation). Whoever replays such a
/// container MUST also replay `ContainerNote::calls` copies of the current
/// selection's profile effects (`effects().times(calls)`), in sequence with
/// the container's own effects (`Effects::then`), or the profile's charges,
/// warnings and Saturation are silently dropped. The composition is exact
/// because charges are an order-free aggregate and Saturation is counters;
/// the replay gate then decides the whole sequence at once, refusing exactly
/// where measuring afresh would cross the reshape budget.
#[derive(Default)]
pub(crate) struct ProfileShare {
    entry: Option<SharedSelection>,
    detached: Option<Detached>,
}

#[cfg_attr(not(test), allow(dead_code))]
struct SharedSelection {
    owner: (u64, usize),
    selected: Range<usize>,
    effects: crate::line::replay::Effects,
    selection: Selection,
    /// Kept by detached shares only.
    digest: Option<SelectionDigest>,
}

#[cfg_attr(not(test), allow(dead_code))]
struct Detached {
    /// Frame receiving the profile's charges (the one below the container
    /// recording), or none without an enclosing recording.
    parent: Option<usize>,
    /// Fresh profile measurements over the whole candidate.
    fresh: u32,
    note: ContainerNote,
}

/// What a detached share observed while one container was measured.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ContainerNote {
    /// `content_shared` calls, each measuring or replaying the profile. A
    /// replay of the container MUST add this many copies of the profile
    /// effects to its own effects (see `ProfileShare`).
    pub(crate) calls: u32,
    /// Saturation of those profile measurements and replays.
    pub(crate) sat: Saturation,
    /// A call read a top/bottom group, whose position follows the profile's
    /// height and above.
    pub(crate) profile: bool,
    /// Visual neighbours `(before, after)` whose allowance was computed
    /// (`ruby::overhang::allowances`).
    pub(crate) neighbours: [Option<usize>; 2],
    /// Some allowance read a visual neighbour (possibly none at a line edge).
    pub(crate) neighbour_dependent: bool,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ProfileShare {
    pub(crate) fn detached() -> Self {
        Self {
            entry: None,
            detached: Some(Detached {
                parent: None,
                fresh: 0,
                note: ContainerNote::default(),
            }),
        }
    }

    /// Start a container whose recording opens above frame `parent`.
    pub(crate) fn begin_container(&mut self, parent: Option<usize>) {
        if let Some(d) = &mut self.detached {
            d.parent = parent;
            d.note = ContainerNote::default();
        }
    }

    pub(crate) fn note(&self) -> ContainerNote {
        self.detached
            .as_ref()
            .map(|d| d.note.clone())
            .unwrap_or_default()
    }

    /// The profile was measured more than once: a replay was refused or a
    /// measurement warned, so its effects are not one fixed `P`.
    pub(crate) fn refreshed(&self) -> bool {
        self.detached.as_ref().is_some_and(|d| d.fresh > 1)
    }

    pub(crate) fn effects(&self) -> Option<crate::line::replay::Effects> {
        self.entry.as_ref().map(|e| e.effects)
    }

    pub(crate) fn digest(&self) -> Option<SelectionDigest> {
        self.entry.as_ref().and_then(|e| e.digest.clone())
    }

    /// Record the neighbours an allowance computation read (`Some(side)`),
    /// each possibly absent at a line edge.
    pub(crate) fn note_neighbours(
        &mut self,
        before: Option<Option<usize>>,
        after: Option<Option<usize>>,
    ) {
        if let Some(d) = &mut self.detached {
            for (slot, side) in d.note.neighbours.iter_mut().zip([before, after]) {
                if let Some(unit) = side {
                    d.note.neighbour_dependent = true;
                    *slot = unit;
                }
            }
        }
    }
}

/// The parts of a selected line profile that container geometry reads,
/// compared between accumulator steps (`ruby::accumulate`). Floats are
/// compared by their bits, unquantized.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectionDigest {
    pub(crate) height: u32,
    pub(crate) above: u32,
    /// Units and bounds (bits) of every partially selected group, by start.
    pub(crate) partial: Vec<(Range<usize>, u32, u32)>,
    /// Units whose glyphs an edge window removed.
    pub(crate) removed: Vec<Range<usize>>,
    /// Owner unit and content bounds (bits) of every edge-window replacement.
    pub(crate) replacements: Vec<(usize, SummaryBits)>,
}

/// `ContentSummary` bounds (normal, top, bottom, raw) as `(top, bottom)` bits.
pub(crate) type SummaryBits = [Option<(u64, u64)>; 4];

#[cfg_attr(not(test), allow(dead_code))]
impl SelectionDigest {
    /// The profile's height or above changed: every group moves.
    pub(crate) fn profile_changed(&self, other: &Self) -> bool {
        self.height != other.height || self.above != other.above
    }

    /// Source ranges whose partial group, edge-window removal or replacement
    /// is not the same in both digests.
    pub(crate) fn changed_ranges(&self, other: &Self) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        for (a, b) in [(self, other), (other, self)] {
            out.extend(
                a.partial
                    .iter()
                    .filter(|p| !b.partial.contains(p))
                    .map(|p| p.0.clone()),
            );
            out.extend(a.removed.iter().filter(|r| !b.removed.contains(r)).cloned());
            out.extend(
                a.replacements
                    .iter()
                    .filter(|r| !b.replacements.contains(r))
                    .map(|r| r.0..r.0 + 1),
            );
        }
        out
    }
}

fn add_sat(total: &mut Saturation, part: Saturation) {
    total.saturated = total.saturated.wrapping_add(part.saturated);
    total.non_finite = total.non_finite.wrapping_add(part.non_finite);
}

/// Resolve all requested columns against one actual selected line profile.
/// Production callers share the profile through `content_shared`.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn content(
    data: &ParagraphData,
    selected: Range<usize>,
    ranges: &[Range<usize>],
    boxes: &[Option<u32>],
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> ContentGeometry {
    content_shared(
        data,
        selected,
        ranges,
        boxes,
        atomics,
        &mut ProfileShare::default(),
        cx,
        sat,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn content_shared(
    data: &ParagraphData,
    selected: Range<usize>,
    ranges: &[Range<usize>],
    boxes: &[Option<u32>],
    atomics: &AtomicSizes,
    share: &mut ProfileShare,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> ContentGeometry {
    let owner = (data.id, data as *const ParagraphData as usize);
    let mut index = super::take_index(data, atomics, cx);
    // `Some(frame)` for a detached share: profile charges skip the
    // container's recording and count for `frame`.
    let parent = share.detached.as_ref().map(|d| d.parent);
    let shared = share
        .entry
        .as_ref()
        .filter(|e| cx.reuse_enabled() && e.owner == owner && e.selected == selected)
        .map(|e| e.effects);
    let replayed = shared.is_some_and(|effects| match parent {
        Some(frame) => crate::line::replay::replay_to(cx, &effects, sat, frame),
        None => crate::line::replay::replay(cx, &effects, sat),
    });
    if replayed && let Some(d) = share.detached.as_mut() {
        d.note.calls += 1;
        add_sat(&mut d.note.sat, shared.unwrap().sat());
    }
    let fresh;
    let selection: &Selection = if replayed {
        &share.entry.as_ref().unwrap().selection
    } else {
        let recording = crate::line::replay::begin(cx, sat);
        let end =
            super::super::plan::hyphen_end(data, selected.end).filter(|e| *e > selected.start);
        let windows = end
            .and_then(|end| super::super::hyphen::line(data, selected.start, end, cx, sat))
            .unwrap_or_else(|| {
                super::super::windows::measure(data, selected.start, selected.end, cx, sat)
            });
        let selection = index.select(data, &selected, &windows, cx, sat);
        #[cfg(test)]
        {
            cx.ruby_profile_selects += 1;
        }
        let finished = match parent {
            Some(frame) => crate::line::replay::finish_to(cx, recording, sat, frame),
            None => crate::line::replay::finish(cx, recording, sat),
        };
        if let Some(d) = share.detached.as_mut() {
            d.fresh += 1;
            d.note.calls += 1;
            if let Some(effects) = &finished {
                add_sat(&mut d.note.sat, effects.sat());
            }
        }
        match finished {
            Some(effects) => {
                let digest = parent.is_some().then(|| index.digest(&selection));
                &share
                    .entry
                    .insert(SharedSelection {
                        owner,
                        selected: selected.clone(),
                        effects,
                        selection,
                        digest,
                    })
                    .selection
            }
            None => {
                share.entry = None;
                fresh = selection;
                &fresh
            }
        }
    };
    if let Some(d) = share.detached.as_mut() {
        d.note.profile |= boxes
            .iter()
            .flatten()
            .any(|b| index.boxes[*b as usize].group.is_some())
            || ranges.iter().any(|r| !r.is_empty() && index.grouped(r));
    }
    let contents: Vec<crate::ruby::geometry::Bounds> = boxes
        .iter()
        .map(|b| {
            if let Some(b) = b {
                index.box_contents[*b as usize]
                    .shift(index.box_delta(data, selection, *b))
                    .fixed(sat)
            } else {
                let (a, d) = crate::ruby::geometry::font_extents(data, 0, sat);
                crate::ruby::geometry::Bounds {
                    top: LayoutUnit::ZERO.sub(a, sat),
                    bottom: d,
                }
            }
        })
        .collect();
    let paints = boxes
        .iter()
        .zip(&contents)
        .map(|(b, content)| {
            if let Some(b) = b {
                index.box_paints[*b as usize]
                    .shift(index.box_delta(data, selection, *b))
                    .fixed(sat)
            } else {
                *content
            }
        })
        .collect();
    let leaf_areas: Vec<_> = ranges
        .iter()
        .map(|range| {
            if range.is_empty() {
                None
            } else {
                index.selected_content(data, selection, range, cx)
            }
        })
        .collect();
    let mut grouped_ancestor = false;
    let areas = ranges
        .iter()
        .enumerate()
        .map(|(i, range)| {
            if range.is_empty() {
                return None;
            }
            let mut area = leaf_areas[i];
            let boundary = boxes.get(i).copied().flatten();
            let mut seen = crate::hashing::FastSet::default();
            for unit in [range.start, range.end - 1] {
                let mut owner = data.units[unit].parent_box;
                while let Some(b) = owner {
                    if !seen.insert(b) {
                        break;
                    }
                    #[cfg(test)]
                    {
                        cx.ruby_measure_visits += 1;
                    }
                    // A column includes its own descendant inlines, but does not
                    // inherit an independent containing inline's painted area.
                    if let Some(limit) = boundary {
                        let mut cursor = Some(b);
                        while let Some(parent) = cursor {
                            if parent == limit {
                                break;
                            }
                            cursor = data.boxes[parent as usize].parent;
                        }
                        if cursor.is_none() {
                            break;
                        }
                    }
                    grouped_ancestor |= index.boxes[b as usize].group.is_some();
                    area = ContentBounds::join(
                        area,
                        Some(
                            index.box_paints[b as usize].shift(index.box_delta(data, selection, b)),
                        ),
                    );
                    if Some(b) == boundary {
                        break;
                    }
                    owner = data.boxes[b as usize].parent;
                }
            }
            area.map(|a| a.fixed(sat))
        })
        .collect();
    if let Some(d) = share.detached.as_mut() {
        d.note.profile |= grouped_ancestor;
    }
    let leaf_areas = leaf_areas
        .into_iter()
        .map(|b| b.map(|b| b.fixed(sat)))
        .collect();
    super::put_index(data, index, cx);
    ContentGeometry {
        areas,
        leaf_areas,
        contents,
        paints,
    }
}
