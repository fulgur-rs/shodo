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
#[derive(Default)]
pub(crate) struct ProfileShare {
    entry: Option<SharedSelection>,
}

struct SharedSelection {
    owner: (u64, usize),
    selected: Range<usize>,
    effects: crate::line::replay::Effects,
    selection: Selection,
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
    let replayed = cx.reuse_enabled()
        && share
            .entry
            .as_ref()
            .is_some_and(|e| e.owner == owner && e.selected == selected)
        && crate::line::replay::replay(cx, &share.entry.as_ref().unwrap().effects, sat);
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
        match crate::line::replay::finish(cx, recording, sat) {
            Some(effects) => {
                &share
                    .entry
                    .insert(SharedSelection {
                        owner,
                        selected: selected.clone(),
                        effects,
                        selection,
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
