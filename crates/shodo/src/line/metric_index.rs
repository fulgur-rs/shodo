//! Real scalar line profiles, with bounded selected edge replacements.
//! Top/bottom groups are disjoint; only clipped or replaced groups need a
//! fresh range query. Every other group reuses its cached complete height.
mod content;
mod scalar;

use content::{ContentBounds, ContentSummary, content_bounds};
// Keep the existing crate-visible type path even when callers infer it.
#[allow(unused_imports)]
pub(crate) use content::{ContentGeometry, content};
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
    bounds: Bounds,
    bottom: bool,
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
    boxes: Vec<RecordProfile>,
    resolver: ProfileResolver,
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
            record_content[i] = record.as_ref().zip(profile).and_then(|(r, p)| {
                crate::ruby::geometry::record_extents(data, r, &[], &mut sat)
                    .map(|(a, d)| content_bounds(data, p, a, d))
            });
            let combination = resolver.combination(data, u);
            for p in profile.into_iter().chain(combination) {
                tree[size + i] = tree[size + i].join(Summary::profile(p, active));
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
                    let group = groups.entry(key).or_insert(Group {
                        units: i..i + 1,
                        bounds: Bounds {
                            top: p.top,
                            bottom: p.bottom,
                        },
                        bottom,
                    });
                    group.units.end = i + 1;
                    group.bounds.top = group.bounds.top.min(p.top);
                    group.bounds.bottom = group.bounds.bottom.max(p.bottom);
                }
            }
            nonglyph[size + i] = if matches!(u.kind, UnitKind::Cluster { .. }) {
                combination.map_or(Summary::default(), |p| Summary::profile(p, true))
            } else {
                tree[size + i]
            };
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_unstable_by_key(|(_, g)| g.units.start);
        let group_keys: crate::hashing::FastMap<u64, usize> = groups
            .iter()
            .enumerate()
            .map(|(i, (key, _))| (*key, i))
            .collect();
        let groups: Vec<_> = groups.into_iter().map(|(_, g)| g).collect();
        for group in &groups {
            let leaf = &mut tree[size + group.units.end - 1];
            let height = group.bounds.bottom - group.bounds.top;
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
        let boxes: Vec<_> = boxes.into_iter().map(Option::unwrap).collect();
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
                    leaf.bottom = raw.map(|r| r.shift(-sign * f64::from(g.bounds.bottom)));
                } else {
                    leaf.top = raw.map(|r| r.shift(-sign * f64::from(g.bounds.top)));
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
            boxes,
            resolver,
        }
    }

    fn select(
        &mut self,
        data: &ParagraphData,
        range: &Range<usize>,
        windows: &[super::windows::Window],
        cx: &mut LayoutContext,
        sat: &mut Saturation,
    ) -> Selection {
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
                    let entry = replacements.entry(owner).or_default();
                    *entry = entry.join(Summary::profile(profile, true));
                    if let Some((a, d)) = crate::ruby::geometry::record_extents(
                        data,
                        &record,
                        std::slice::from_ref(run),
                        sat,
                    ) {
                        let raw = Some(content_bounds(data, profile, a, d));
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
            if let Some(bounds) = union(
                self.query(&selected, &replacements, &BTreeSet::new(), &removed, cx)
                    .raw,
                group_extra.get(&i).copied().flatten(),
            ) {
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
        let bounds = union(
            Some(Bounds {
                top: -above,
                bottom: below,
            }),
            summary.normal,
        )
        .unwrap();
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
