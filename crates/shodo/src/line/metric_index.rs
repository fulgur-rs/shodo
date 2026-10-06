//! Real scalar line profiles, with bounded selected edge replacements.
//! Top/bottom groups are disjoint; only clipped or replaced groups need a
//! fresh range query. Every other group reuses its cached complete height.
mod content;
mod quirk;
mod scalar;

use content::{ContentBounds, ContentSummary, content_bounds, record_content_bounds};
// Keep the existing crate-visible type path even when callers infer it.
#[allow(unused_imports)]
pub(crate) use content::{
    ContainerNote, ContentGeometry, ProfileShare, SelectionDigest, content, content_shared,
};
use scalar::{Bounds, Summary, union};
pub(crate) use scalar::{ScalarMetrics, measure};

use super::fragments::{FragmentRecord, GlyphSource, RecordKind};
use super::metrics::{ProfileResolver, RecordProfile};
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::paragraph::ParagraphData;
use crate::style::{TextOrientation, VerticalAlign};
use crate::{AtomicSizes, LayoutContext};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

struct Selection {
    metrics: ScalarMetrics,
    height: f32,
    above: f32,
    replacements: BTreeMap<usize, ContentSummary>,
    removed: Vec<Range<usize>>,
    partial: crate::hashing::FastMap<usize, Bounds>,
}

#[derive(Debug)]
struct Group {
    units: Range<usize>,
    /// Members that size the line; `None` under the quirk when no member
    /// does, so the group bakes no height.
    bounds: Option<Bounds>,
    /// Every member's profile: positions a group that sizes nothing.
    ghost: Bounds,
    bottom: bool,
}

impl Group {
    /// Bounds that place the group's content.
    fn placed(&self) -> Bounds {
        self.bounds.unwrap_or(self.ghost)
    }
}

#[derive(Debug)]
pub(super) struct MetricIndex {
    tree: Vec<Summary>,
    nonglyph: Vec<Summary>,
    contents: Vec<ContentSummary>,
    nonglyph_contents: Vec<ContentSummary>,
    box_contents: Vec<ContentBounds>,
    box_paints: Vec<ContentBounds>,
    groups: Vec<Group>,
    group_keys: crate::hashing::FastMap<u64, usize>,
    group_at: Vec<Option<u64>>,
    /// Prefix counts of units that belong to a top/bottom group.
    grouped: Vec<u32>,
    boxes: Vec<RecordProfile>,
    resolver: ProfileResolver,
    /// Quirks-mode strut contributions; `None` unless `line_height_quirk`.
    quirk: Option<Box<quirk::QuirkIndex>>,
}

pub(super) fn unit_record(
    data: &ParagraphData,
    i: usize,
    atomics: &AtomicSizes,
    sat: &mut Saturation,
) -> Option<FragmentRecord> {
    let u = &data.units[i];
    let kind = match &u.kind {
        UnitKind::Cluster { run, glyphs, .. } => RecordKind::Glyphs {
            source: GlyphSource::Shared,
            run: *run,
            glyphs: glyphs.clone(),
            item: u.item,
            text: u.text.clone(),
        },
        UnitKind::Open { box_index } | UnitKind::Close { box_index } => RecordKind::InlineBox {
            box_index: *box_index,
            start_edge: matches!(u.kind, UnitKind::Open { .. }),
            end_edge: matches!(u.kind, UnitKind::Close { .. }),
            slice_offset: None,
            parent: None,
            reversed: false,
        },
        UnitKind::Atomic { node } => RecordKind::Atomic {
            node: *node,
            size: crate::sanitize::atomic(
                atomics.get(*node).copied().unwrap_or_default(),
                &mut crate::limits::WarningSink::new(Some(0)),
                sat,
            ),
            unit: i as u32,
        },
        _ => return None,
    };
    Some(FragmentRecord {
        kind,
        inline_start: LayoutUnit::ZERO,
        inline_size: LayoutUnit::ZERO,
        level: u.level,
    })
}

/// Move the dataset's index out of its value slot, building it when the slot
/// is missing or empty. The key stays in the map, so the matching `put_index`
/// neither removes nor rehashes an entry.
fn take_index(
    data: &ParagraphData,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
) -> Box<MetricIndex> {
    let key = (data.id, data as *const ParagraphData as usize);
    match cx.ruby_ranges.metrics.get_mut(&key).and_then(Option::take) {
        Some(index) => index,
        None => Box::new(MetricIndex::new(data, atomics, cx)),
    }
}

fn put_index(data: &ParagraphData, index: Box<MetricIndex>, cx: &mut LayoutContext) {
    let key = (data.id, data as *const ParagraphData as usize);
    *cx.ruby_ranges.metrics.entry(key).or_default() = Some(index);
}

impl MetricIndex {
    fn new(data: &ParagraphData, atomics: &AtomicSizes, cx: &mut LayoutContext) -> Self {
        let mut resolver = ProfileResolver::new(data, 0..data.units.len());
        let mut sat = Saturation::default();
        let size = data.units.len().max(1).next_power_of_two();
        let mut tree = vec![Summary::default(); size * 2];
        let mut nonglyph = tree.clone();
        let mut groups: crate::hashing::FastMap<u64, Group> = crate::hashing::FastMap::default();
        let mut group_at = vec![None; data.units.len()];
        let mut boxes = vec![None; data.boxes.len()];
        let mut record_content = vec![None; data.units.len()];
        let quirk = data.style.line_height_quirk;
        // Glyph profiles of trimmable units, which size a line only when it
        // does not end with them: they live in the quirk tree instead.
        let mut trimmed = if quirk {
            vec![None; data.units.len()]
        } else {
            Vec::new()
        };
        for (i, u) in data.units.iter().enumerate() {
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 2;
            }
            let active = match u.kind {
                UnitKind::Cluster { .. }
                | UnitKind::Atomic { .. }
                | UnitKind::Tab
                | UnitKind::ForcedBreak => true,
                UnitKind::Open { box_index } | UnitKind::Close { box_index } => {
                    let e = data.boxes[box_index as usize].edges;
                    (e.padding.block_start + e.padding.block_end) != 0.0
                        || (e.border.block_start + e.border.block_end) != 0.0
                        || matches!(u.kind, UnitKind::Open { .. }) && e.inline_start_total() != 0.0
                        || matches!(u.kind, UnitKind::Close { .. }) && e.inline_end_total() != 0.0
                }
                _ => u.combine.is_some(),
            };
            tree[size + i].active = active;
            let record = unit_record(data, i, atomics, &mut sat);
            let profile = record.as_ref().and_then(|r| resolver.record(data, r, &[]));
            record_content[i] = record
                .as_ref()
                .zip(profile)
                .and_then(|(r, p)| record_content_bounds(data, p, r, &[], &mut sat));
            let combination = resolver.combination(data, u);
            let is_box = matches!(u.kind, UnitKind::Open { .. } | UnitKind::Close { .. });
            let trims = quirk && super::quirk::trims(data, i);
            for (p, record) in profile
                .map(|p| (p, true))
                .into_iter()
                .chain(combination.map(|p| (p, false)))
            {
                // Under the quirk a box strut sizes only the lines that credit
                // it (`quirk::QuirkIndex`), as does a trimmable unit's glyph.
                let sizes = !(quirk && record && (is_box || trims));
                if sizes {
                    tree[size + i] = tree[size + i].join(Summary::profile(p, active));
                } else if trims {
                    trimmed[i] = Some(p);
                }
                if let UnitKind::Open { box_index } | UnitKind::Close { box_index } = u.kind {
                    boxes[box_index as usize] = Some(p);
                }
                let key = p
                    .group
                    .map(u64::from)
                    .or_else(|| p.own_group.map(|_| (1u64 << 32) | i as u64));
                if let Some(key) = key {
                    group_at[i] = Some(key);
                    let bottom = p.group.map_or(p.own_group.unwrap_or(false), |b| {
                        data.styles[data.boxes[b as usize].style as usize].vertical_align
                            == VerticalAlign::Bottom
                    });
                    let bounds = Bounds {
                        top: p.top,
                        bottom: p.bottom,
                    };
                    let group = groups.entry(key).or_insert(Group {
                        units: i..i + 1,
                        bounds: None,
                        ghost: bounds,
                        bottom,
                    });
                    group.units.end = i + 1;
                    group.ghost = union(Some(group.ghost), Some(bounds)).unwrap();
                    if sizes {
                        group.bounds = union(group.bounds, Some(bounds));
                    }
                }
            }
            nonglyph[size + i] = if matches!(u.kind, UnitKind::Cluster { .. }) {
                combination.map_or(Summary::default(), |p| Summary::profile(p, true))
            } else {
                tree[size + i]
            };
        }
        let mut grouped = Vec::with_capacity(data.units.len() + 1);
        grouped.push(0u32);
        for key in &group_at {
            grouped.push(grouped.last().unwrap() + u32::from(key.is_some()));
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_unstable_by_key(|(_, g)| g.units.start);
        let group_keys: crate::hashing::FastMap<u64, usize> = groups
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (*key, i))
            .collect();
        let mut groups: Vec<_> = groups.into_iter().map(|(_, g)| g).collect();
        let boxes: Vec<_> = boxes.into_iter().map(Option::unwrap).collect();
        let quirk = quirk.then(|| {
            let index = quirk::QuirkIndex::new(data, &boxes, &trimmed);
            // Credited struts and trimmable glyphs of a wholly selected group
            // size it on every line that does not end inside it.
            for (i, u) in data.units.iter().enumerate() {
                let owner = match u.kind {
                    UnitKind::Open { box_index } | UnitKind::Close { box_index } => Some(box_index),
                    _ => u.parent_box,
                };
                let raw = index.leaf(i).all.raw;
                if let Some(g) = owner.and_then(|b| boxes[b as usize].group)
                    && raw.is_some()
                {
                    let group = &mut groups[group_keys[&u64::from(g)]];
                    group.bounds = union(group.bounds, raw);
                }
            }
            Box::new(index)
        });
        for group in &groups {
            let Some(bounds) = group.bounds else {
                continue;
            };
            let leaf = &mut tree[size + group.units.end - 1];
            let height = bounds.bottom - bounds.top;
            nonglyph[size + group.units.end - 1].height =
                nonglyph[size + group.units.end - 1].height.max(height);
            leaf.height = leaf.height.max(height);
            if group.bottom {
                nonglyph[size + group.units.end - 1].bottom_height = nonglyph
                    [size + group.units.end - 1]
                    .bottom_height
                    .max(height);
                leaf.bottom_height = leaf.bottom_height.max(height);
            }
        }
        for i in (1..size).rev() {
            tree[i] = tree[i * 2].join(tree[i * 2 + 1]);
            nonglyph[i] = nonglyph[i * 2].join(nonglyph[i * 2 + 1]);
        }
        let mut contents = vec![ContentSummary::default(); size * 2];
        for (i, raw) in record_content.into_iter().enumerate() {
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 1;
            }
            let mut leaf = ContentSummary {
                raw,
                ..Default::default()
            };
            if let Some(key) = group_at[i] {
                let g = &groups[group_keys[&key]];
                let sign = if data.style.writing_mode == WritingMode::VerticalLr {
                    -1.0
                } else {
                    1.0
                };
                if g.bottom {
                    leaf.bottom = raw.map(|r| r.shift(-sign * f64::from(g.placed().bottom)));
                } else {
                    leaf.top = raw.map(|r| r.shift(-sign * f64::from(g.placed().top)));
                }
            } else {
                leaf.normal = raw;
            }
            contents[size + i] = leaf;
        }
        let mut nonglyph_contents = contents.clone();
        for (i, u) in data.units.iter().enumerate() {
            if matches!(u.kind, UnitKind::Cluster { .. }) {
                nonglyph_contents[size + i] = ContentSummary::default();
            }
        }
        for i in (1..size).rev() {
            contents[i] = contents[i * 2].join(contents[i * 2 + 1]);
            nonglyph_contents[i] = nonglyph_contents[i * 2].join(nonglyph_contents[i * 2 + 1]);
        }
        let box_contents = boxes
            .iter()
            .enumerate()
            .map(|(b, p)| {
                let (a, d) =
                    crate::ruby::geometry::font_extents(data, data.boxes[b].style, &mut sat);
                content_bounds(data, *p, a, d)
            })
            .collect();
        let box_paints = boxes
            .iter()
            .enumerate()
            .map(|(b, p)| {
                let b = &data.boxes[b];
                let (a, d) = crate::ruby::geometry::font_extents(data, b.style, &mut sat);
                let a = a.add(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_start + b.edges.border.block_start,
                        &mut sat,
                    ),
                    &mut sat,
                );
                let d = d.add(
                    LayoutUnit::from_f32_round(
                        b.edges.padding.block_end + b.edges.border.block_end,
                        &mut sat,
                    ),
                    &mut sat,
                );
                content_bounds(data, *p, a, d)
            })
            .collect();
        let _ = cx;
        Self {
            tree,
            nonglyph,
            contents,
            nonglyph_contents,
            box_contents,
            box_paints,
            groups,
            group_keys,
            group_at,
            grouped,
            boxes,
            resolver,
            quirk,
        }
    }

    /// Whether any unit of `range` belongs to a top/bottom group: its content
    /// moves with the selected profile's height and above.
    fn grouped(&self, range: &Range<usize>) -> bool {
        self.grouped[range.end] > self.grouped[range.start]
    }

    fn select(
        &mut self,
        data: &ParagraphData,
        range: &Range<usize>,
        windows: &[super::windows::Window],
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Selection {
        // First unit of the trailing run that a quirks-mode line trims.
        let trailing = self
            .quirk
            .as_ref()
            .map(|q| q.trailing_start(range.start, range.end));
        let mut replacements = BTreeMap::<usize, Summary>::new();
        let mut content_replacements = BTreeMap::<usize, ContentSummary>::new();
        let mut removed = Vec::new();
        // Only edge-window owners are replaced. Box/atomic leaves keep their
        // real profiles, and unaffected glyphs reuse the indexed summaries.
        for window in windows {
            if let (Some((first, _, _)), Some((last, _, _))) =
                (window.changes.first(), window.changes.last())
            {
                removed.push(*first..last + 1);
            }
            for run in &window.overlay.runs {
                if run.glyphs.is_empty() {
                    continue;
                }
                let at = window.changes.partition_point(|(i, _, _)| {
                    #[cfg(test)]
                    {
                        cx.ruby_measure_visits += 1;
                    }
                    data.units[*i].text.end <= run.text.start
                });
                let Some(owner) = window
                    .changes
                    .get(at)
                    .map(|(i, _, _)| *i)
                    .filter(|i| data.units[*i].text.start < run.text.end)
                else {
                    continue;
                };
                let record = FragmentRecord {
                    kind: RecordKind::Glyphs {
                        source: GlyphSource::Overlay {
                            glyphs: (run.glyphs.start, run.glyphs.end),
                            clusters: (0, 0),
                            run: Some(0),
                        },
                        run: 0,
                        glyphs: run.glyphs.clone(),
                        item: run.item,
                        text: run.text.clone(),
                    },
                    inline_start: LayoutUnit::ZERO,
                    inline_size: LayoutUnit::ZERO,
                    level: data.units[owner].level,
                };
                if let Some(profile) =
                    self.resolver
                        .record(data, &record, std::slice::from_ref(run))
                {
                    // The overlay glyph of a trimmed trailing space sizes
                    // nothing, as its original glyph would not.
                    if !trailing.is_some_and(|t| owner >= t && super::quirk::trims(data, owner)) {
                        let entry = replacements.entry(owner).or_default();
                        *entry = entry.join(Summary::profile(profile, true));
                    }
                    if let Some(bounds) = record_content_bounds(
                        data,
                        profile,
                        &record,
                        std::slice::from_ref(run),
                        sat,
                    ) {
                        let raw = Some(bounds);
                        let normal = if profile.group.is_none() && profile.own_group.is_none() {
                            raw
                        } else {
                            None
                        };
                        let entry = content_replacements.entry(owner).or_default();
                        *entry = entry.join(ContentSummary {
                            raw,
                            normal,
                            ..Default::default()
                        });
                    }
                }
            }
        }
        let mut affected = crate::hashing::FastSet::default();
        let first = self.groups.partition_point(|g| g.units.end <= range.start);
        let last = self.groups.partition_point(|g| g.units.start < range.end);
        for i in [first, last.saturating_sub(1)] {
            if let Some(g) = self.groups.get(i)
                && g.units.start < range.end
                && range.start < g.units.end
                && (g.units.start < range.start || range.end < g.units.end)
            {
                affected.insert(i);
            }
        }
        for r in &removed {
            let first = self.groups.partition_point(|g| g.units.end <= r.start);
            let last = self.groups.partition_point(|g| g.units.start < r.end);
            for i in first..last {
                #[cfg(test)]
                {
                    cx.ruby_measure_visits += 1;
                }
                affected.insert(i);
            }
        }
        for unit in replacements.keys() {
            if let Some(key) = self.group_at[*unit] {
                affected.insert(self.group_keys[&key]);
            }
        }
        let mut extra = Summary::default();
        let mut group_extra = crate::hashing::FastMap::default();
        let mut side = quirk::Side::default();
        if let (Some(q), Some(t)) = (&self.quirk, trailing) {
            side = q.side(range, t, &removed, cx);
            // Groups that the trailing run clips. The run holds no Open unit,
            // so at most the group around its start intersects it.
            let first = self.groups.partition_point(|g| g.units.end <= t);
            for (i, g) in self.groups.iter().enumerate().skip(first) {
                if g.units.start >= range.end {
                    break;
                }
                // Constant time relies on no group starting inside the run.
                debug_assert!(i - first < 2, "trailing run starts groups");
                affected.insert(i);
            }
            if let Some((k, lo)) = q.forced(data, range) {
                let content = q
                    .query(&(lo..k.min(t)), |l| l.all, cx)
                    .join(q.query(&(lo.max(t)..k), |l| l.kept, cx))
                    .content;
                if !q.credited(data.units[k].parent_box, content) {
                    let p = data.units[k]
                        .parent_box
                        .map_or(q.root(), |b| self.boxes[b as usize]);
                    side = side.join(quirk::Side::from(p, Default::default()));
                    if let Some(group) = p.group {
                        let i = self.group_keys[&u64::from(group)];
                        affected.insert(i);
                        group_extra.insert(
                            i,
                            Some(Bounds {
                                top: p.top,
                                bottom: p.bottom,
                            }),
                        );
                    }
                }
            }
            if q.ruby(range) {
                side = side.join(quirk::Side::from(q.root(), Default::default()));
            }
        }
        let mut ancestors = crate::hashing::FastSet::default();
        for unit in [range.start, range.end - 1] {
            let mut cursor = data.units[unit].parent_box;
            while let Some(b) = cursor {
                if !ancestors.insert(b) {
                    break;
                }
                #[cfg(test)]
                {
                    cx.ruby_measure_visits += 1;
                }
                let p = self.boxes[b as usize];
                let e = data.boxes[b as usize].edges;
                let active = (e.padding.block_start + e.padding.block_end) != 0.0
                    || (e.border.block_start + e.border.block_end) != 0.0;
                if self.quirk.is_some() {
                    // Ancestors size the line only through `side` credits.
                    extra.active |= active;
                    cursor = data.boxes[b as usize].parent;
                    continue;
                }
                extra = extra.join(Summary::profile(p, active));
                if let Some(group) = p.group {
                    let i = self.group_keys[&u64::from(group)];
                    if affected.contains(&i) {
                        group_extra
                            .entry(i)
                            .and_modify(|v| {
                                *v = union(
                                    *v,
                                    Some(Bounds {
                                        top: p.top,
                                        bottom: p.bottom,
                                    }),
                                )
                            })
                            .or_insert(Some(Bounds {
                                top: p.top,
                                bottom: p.bottom,
                            }));
                    }
                }
                cursor = data.boxes[b as usize].parent;
            }
        }
        let excluded: BTreeSet<_> = affected
            .iter()
            .map(|i| self.groups[*i].units.end - 1)
            .collect();
        let mut summary = self
            .query(range, &replacements, &excluded, &removed, cx)
            .join(extra);
        let mut partial = crate::hashing::FastMap::default();
        for i in affected {
            let group = &self.groups[i];
            let selected = range.start.max(group.units.start)..range.end.min(group.units.end);
            let mut raw = union(
                self.query(&selected, &replacements, &BTreeSet::new(), &removed, cx)
                    .raw,
                group_extra.get(&i).copied().flatten(),
            );
            if let (Some(q), Some(t)) = (&self.quirk, trailing) {
                raw = union(raw, q.side(&selected, t, &removed, cx).raw);
                if raw.is_none() {
                    // No member sizes the line: place the content only.
                    partial.insert(i, group.ghost);
                }
            }
            if let Some(bounds) = raw {
                partial.insert(i, bounds);
                let height = bounds.bottom - bounds.top;
                summary.height = summary.height.max(height);
                if group.bottom {
                    summary.bottom_height = summary.bottom_height.max(height);
                }
            }
        }
        let root = &data.styles[0];
        let metrics = data.style_metrics[0];
        let upright = matches!(
            data.style.writing_mode,
            WritingMode::VerticalRl | WritingMode::VerticalLr
        ) && root.text_orientation != TextOrientation::Sideways;
        let (above, below) =
            super::metrics::extents(root, metrics.metrics, metrics.vertical_metrics, upright);
        let bounds = if self.quirk.is_some() {
            // Root text and credited struts already joined `side`; a line
            // nothing sizes has a zero-height root.
            union(side.normal, summary.normal).unwrap_or(Bounds {
                top: 0.0,
                bottom: 0.0,
            })
        } else {
            union(
                Some(Bounds {
                    top: -above,
                    bottom: below,
                }),
                summary.normal,
            )
            .unwrap()
        };
        let height = (bounds.bottom - bounds.top).max(0.0).max(summary.height);
        let mut above = -bounds.top;
        if height > bounds.bottom - bounds.top {
            above = above.max(summary.bottom_height - bounds.bottom);
        }
        let block_size = LayoutUnit::from_f32_ceil(if summary.active { height } else { 0.0 }, sat);
        let over = LayoutUnit::from_f32_round(above, sat);
        Selection {
            metrics: ScalarMetrics {
                baseline: if data.style.writing_mode == WritingMode::VerticalLr {
                    block_size.sub(over, sat)
                } else {
                    over
                },
                block_size,
                empty: !summary.active,
            },
            height,
            above,
            replacements: content_replacements,
            removed,
            partial,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParagraphBuilder;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{FontFamily, InlineStyle, LineHeight, ParagraphStyle};

    #[test]
    fn quirk_tree_is_absent_without_the_flag() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        for quirk in [false, true] {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    line_height_quirk: quirk,
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "a b ");
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            let index = MetricIndex::new(&p.data, &AtomicSizes::EMPTY, &mut LayoutContext::new());
            assert_eq!(index.quirk.is_some(), quirk);
        }
    }

    #[test]
    fn selected_content_profiles_match_actual_retained_top_bottom_geometry() {
        let fonts = FontCollection::with_options(
            &Limits::default(),
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::test_support::fonts::CJK.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture CJK".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = |size| InlineStyle {
            font_size: size,
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            ..Default::default()
        };
        for mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    writing_mode: mode,
                    root: InlineStyle {
                        line_height: LineHeight::Px(73.125),
                        ..style(21.3)
                    },
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
            for (i, align, size, height) in [
                (0, VerticalAlign::Top, 47.7, 83.625),
                (1, VerticalAlign::Bottom, 28.3, 33.9),
            ] {
                b.open_inline(
                    NodeId(10 + i),
                    &InlineStyle {
                        vertical_align: align,
                        line_height: LineHeight::Px(height),
                        ..style(size)
                    },
                    Default::default(),
                );
                b.push_text(
                    TextSource::Generated {
                        node: NodeId(20 + i),
                    },
                    "日",
                );
                b.open_inline(
                    NodeId(30 + i),
                    &InlineStyle {
                        vertical_align: VerticalAlign::Length(-3.317),
                        ..style(8.13)
                    },
                    InlineEdges {
                        padding: Sides {
                            block_start: 4.4375,
                            block_end: 2.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );
                b.push_text(
                    TextSource::Generated {
                        node: NodeId(40 + i),
                    },
                    "日本",
                );
                b.close_inline();
                b.push_atomic(NodeId(50 + i), &style(13.7), Default::default());
                b.close_inline();
            }
            b.push_text(TextSource::Generated { node: NodeId(60) }, "語");
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            let mut atomics = AtomicSizes::new();
            for node in [NodeId(50), NodeId(51)] {
                atomics.insert(
                    node,
                    crate::AtomicSize {
                        inline_size: 34.0,
                        block_size: 27.3,
                        baseline: Some(9.13),
                        ..Default::default()
                    },
                );
            }
            let owners: Vec<_> = p
                .data
                .units
                .iter()
                .enumerate()
                .filter(|(_, u)| {
                    matches!(u.kind, UnitKind::Cluster { .. } | UnitKind::Atomic { .. })
                })
                .map(|(i, _)| i)
                .collect();
            let mut cx = LayoutContext::new();
            cx.ruby_ranges.begin(&p.data, &atomics);
            for (at, start) in owners.iter().enumerate() {
                for end in owners[at..].iter().map(|i| i + 1) {
                    let range = *start..end;
                    let line = p.ruby_line(
                        &mut LayoutContext::new(),
                        range.clone(),
                        10000.0,
                        &atomics,
                        crate::ruby::align::AnnotationAlign::Policy(crate::RubyAlign::Start),
                    );
                    let mut sat = Saturation::default();
                    let frame = crate::ruby::geometry::Frame::new(
                        &p.data,
                        range.clone(),
                        &line.fragments,
                        &line.overlay_runs,
                        LayoutUnit::ZERO,
                        &line.block_shifts,
                        line.block_size,
                    );
                    let root = frame.box_content(None, &mut sat);
                    let expected = line
                        .fragments
                        .iter()
                        .enumerate()
                        .filter_map(|(i, _)| frame.record_bounds(i, &mut sat))
                        .fold(root, |a, b| a.union(b));
                    let result = content(
                        &p.data,
                        range.clone(),
                        std::slice::from_ref(&range),
                        &[None],
                        &atomics,
                        &mut cx,
                        &mut sat,
                    );
                    let actual =
                        result.areas[0].map_or(result.contents[0], |b| b.union(result.contents[0]));
                    assert_eq!(
                        (actual.top, actual.bottom),
                        (expected.top, expected.bottom),
                        "{mode:?}/{range:?}"
                    );
                    let metrics = measure(&p.data, range.clone(), &atomics, &mut cx, &mut sat);
                    assert_eq!(metrics.baseline, line.baseline, "{mode:?}/{range:?}");
                    assert_eq!(metrics.block_size, line.block_size, "{mode:?}/{range:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod emphasis_tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::style::{
        InlineStyle, LineHeight, ParagraphStyle, TextEmphasis, TextEmphasisPosition,
        TextEmphasisShape,
    };
    use crate::{
        ParagraphBuilder, Ruby, RubyAnnotation, RubyBase, RubyContent, RubyLevel, RubyPosition,
        RubySpan, RubyStyle, RubyVisibility,
    };

    #[test]
    fn emphasis_ruby_probes_match_retained_ranges_and_warm_queries_stay_bounded() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        for mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
        ] {
            for position in [
                TextEmphasisPosition::OverRight,
                TextEmphasisPosition::UnderLeft,
            ] {
                let style = InlineStyle {
                    font_size: 20.0,
                    line_height: LineHeight::Px(20.0),
                    text_emphasis: Some(TextEmphasis {
                        shape: TextEmphasisShape::Dot,
                        filled: true,
                        position,
                    }),
                    ..Default::default()
                };
                let reading = InlineStyle {
                    font_size: 10.0,
                    line_height: LineHeight::Px(40.0),
                    ..Default::default()
                };
                let mut builder = ParagraphBuilder::new(
                    &ParagraphStyle {
                        root: style.clone(),
                        writing_mode: mode,
                        ..Default::default()
                    },
                    &limits,
                );
                for i in 0..4 {
                    let base_content = if i % 2 == 0 {
                        let large = InlineStyle {
                            font_size: 40.0,
                            line_height: LineHeight::Px(40.0),
                            ..Default::default()
                        };
                        let mut base = ParagraphBuilder::new(
                            &ParagraphStyle {
                                root: large,
                                writing_mode: mode,
                                ..Default::default()
                            },
                            &limits,
                        );
                        let mut marked = style.clone();
                        if i == 2 {
                            marked.vertical_align = crate::style::VerticalAlign::Top;
                        }
                        base.open_inline(NodeId(i + 50), &marked, Default::default())
                            .push_text(
                                TextSource::Generated {
                                    node: NodeId(i + 10),
                                },
                                "a",
                            )
                            .close_inline();
                        base.push_text(
                            TextSource::Generated {
                                node: NodeId(i + 60),
                            },
                            "b",
                        );
                        RubyContent::from_builder(base)
                    } else {
                        RubyContent::text(
                            TextSource::Generated {
                                node: NodeId(i + 10),
                            },
                            "ab",
                            &style,
                            &limits,
                        )
                    };
                    let ruby = Ruby::new(
                        vec![RubyBase {
                            node: NodeId(i + 10),
                            content: base_content,
                            align: crate::RubyAlign::Start,
                        }],
                        vec![RubyLevel {
                            annotations: vec![RubyAnnotation {
                                node: NodeId(i + 20),
                                content: RubyContent::text(
                                    TextSource::Generated {
                                        node: NodeId(i + 20),
                                    },
                                    "cd",
                                    &reading,
                                    &limits,
                                ),
                                span: RubySpan::All,
                                visibility: RubyVisibility::Visible,
                            }],
                            style: RubyStyle {
                                position: if i % 2 == 0 {
                                    RubyPosition::Over
                                } else {
                                    RubyPosition::Under
                                },
                                ..Default::default()
                            },
                        }],
                    )
                    .unwrap();
                    builder.push_ruby(NodeId(i + 30), &style, ruby);
                }
                let paragraph = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                let data = &paragraph.data;
                let mut cx = LayoutContext::new();
                cx.ruby_ranges.begin(data, &AtomicSizes::EMPTY);
                let owners: Vec<_> = data
                    .units
                    .iter()
                    .enumerate()
                    .filter_map(|(i, u)| matches!(u.kind, UnitKind::Cluster { .. }).then_some(i))
                    .collect();
                for (at, start) in owners.iter().enumerate() {
                    for end in owners[at..].iter().map(|i| i + 1) {
                        let range = *start..end;
                        let mut sat = Saturation::default();
                        let ruby = crate::ruby::measure::candidate(
                            data,
                            range.start,
                            range.end,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut sat,
                        );
                        let probed = crate::line::range::block_size(
                            data,
                            range.clone(),
                            &ruby,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut sat,
                        );
                        let retained = paragraph.ruby_line(
                            &mut LayoutContext::new(),
                            range.clone(),
                            10000.0,
                            &AtomicSizes::EMPTY,
                            crate::ruby::align::AnnotationAlign::Policy(crate::RubyAlign::Start),
                        );
                        assert_eq!(
                            probed, retained.block_size,
                            "{mode:?}/{position:?}/{range:?}"
                        );
                        let before = cx.ruby_measure_visits;
                        for _ in 0..8 {
                            assert_eq!(
                                crate::line::range::block_size(
                                    data,
                                    range.clone(),
                                    &ruby,
                                    &AtomicSizes::EMPTY,
                                    &mut cx,
                                    &mut sat
                                ),
                                probed
                            );
                        }
                        assert!(
                            cx.ruby_measure_visits - before <= 8,
                            "warm block probes must reuse the exact cached geometry"
                        );
                    }
                }
            }
        }
    }
}
