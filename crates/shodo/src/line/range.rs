//! Actual retained unit advances and visual spacing indexed by source range.
//! Tabs extend a prefix cache for their actual selected start; caller atomics
//! are indexed under their revision. Source-clipped shared clusters and
//! discretionary hyphens use bounded selected windows and a replaced spacing
//! leaf. A source-clipped shaping cluster corrects its own slices, keeping the
//! remainder of the range indexed.
use super::spacing_summary::{RangeIndex, raw};
use crate::LayoutContext;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::{AtomicSizes, ParagraphData};
use std::ops::Range;

#[derive(Debug, Default)]
pub(crate) struct RangeCache {
    root: Option<(u64, usize, u64)>,
    sets: crate::hashing::FastMap<(u64, usize), Costs>,
    /// Value slots: a query moves the index out and back without removing the
    /// key, so the map is not rehashed per query. An empty slot is rebuilt.
    pub(super) metrics:
        crate::hashing::FastMap<(u64, usize), Option<Box<super::metric_index::MetricIndex>>>,
    blocks: crate::hashing::FastMap<(u64, usize, usize, usize), LayoutUnit>,
    pub(crate) neighbors:
        crate::hashing::FastMap<(u64, usize), Option<Box<crate::ruby::overhang::NeighborIndex>>>,
    /// Bumped whenever a cache fills or clears in a way that changes the side
    /// effects of later queries: a `blocks` hit skips the reshape charges and
    /// saturation of measuring the block, a `sets` hit skips the build's
    /// saturation, and a covered tab step skips its saturation. Tab steps
    /// whose saturation was clean change no effects and do not count, nor do
    /// the metric and neighbor index slots.
    /// Per-operation reuse (`ruby::memo`) records and replays a measurement
    /// only while this is unchanged, so replayed effects always equal those of
    /// measuring again against the same cache state. Every bump is either a
    /// fill (`fills`) or an invalidation (`epoch`).
    generation: u64,
    /// Monotone fills: a `blocks` or `sets` insertion, and a tab step with
    /// saturation computed for the current prefix. A fill only turns later
    /// misses into hits, so a measurement recorded while no fill and no
    /// invalidation happened stays exact until the next invalidation
    /// (`ruby::accumulate`).
    fills: u64,
    /// Invalidations: caches cleared (`begin` with a new root,
    /// `vacate_slots`) or a tab prefix holding a step with saturation
    /// replaced for another start.
    epoch: u64,
}

impl RangeCache {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn fills(&self) -> u64 {
        self.fills
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    fn fill(&mut self) {
        self.fills = self.fills.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
    }

    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn begin(&mut self, data: &ParagraphData, atomics: &AtomicSizes) {
        let root = (
            data.id,
            data as *const ParagraphData as usize,
            atomics.revision,
        );
        // Retained nested annotation layout revisits datasets already indexed
        // as children of this root. Keep their scalar tables together, without
        // treating child materialization as a new independent paragraph. A new
        // dataset or changed atomic revision still replaces the whole cache.
        let key = (root.0, root.1);
        if self.root.is_some_and(|owner| owner.2 == root.2)
            && (self.sets.contains_key(&key)
                || self.metrics.contains_key(&key)
                || self.neighbors.contains_key(&key))
        {
            return;
        }
        if self.root != Some(root) {
            self.sets.clear();
            self.blocks.clear();
            self.metrics.clear();
            self.neighbors.clear();
            self.root = Some(root);
            self.invalidate();
        }
    }

    /// Empty every index slot while keeping its key. The scalar caches are
    /// cleared too, so a repeated query must go through the vacated slots
    /// instead of being answered from a cached result.
    #[cfg(test)]
    pub(crate) fn vacate_slots(&mut self) {
        self.sets.clear();
        self.blocks.clear();
        self.metrics.values_mut().for_each(|slot| *slot = None);
        self.neighbors.values_mut().for_each(|slot| *slot = None);
        self.invalidate();
    }

    /// Both index maps are populated and every slot holds an index.
    #[cfg(test)]
    pub(crate) fn slots_filled(&self) -> bool {
        !self.metrics.is_empty()
            && !self.neighbors.is_empty()
            && self.metrics.values().all(Option::is_some)
            && self.neighbors.values().all(Option::is_some)
    }
}

/// Cache only scalar block measurements, never child Lines or lane cursors.
/// Actual edge-window run instances supply fallback font metrics, matching
/// retained output without constructing glyph buffers for each fit probe.
pub(crate) fn block_size(
    data: &ParagraphData,
    range: Range<usize>,
    ruby: &crate::ruby::measure::RubyMeasure,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    let key = (
        data.id,
        data as *const ParagraphData as usize,
        range.start,
        range.end,
    );
    if let Some(height) = cx.ruby_ranges.blocks.get(&key) {
        return *height;
    }
    let metrics = super::metric_index::measure(data, range, atomics, cx, sat);
    let mut bounds = crate::ruby::geometry::Bounds {
        top: LayoutUnit::ZERO.sub(metrics.baseline, sat),
        bottom: metrics.block_size.sub(metrics.baseline, sat),
    };
    for fragment in &ruby.fragments {
        if fragment.has_content {
            bounds = bounds.union(fragment.contribution);
        }
    }
    let height = bounds.height(sat);
    cx.ruby_ranges.blocks.insert(key, height);
    cx.ruby_ranges.fill();
    height
}

#[derive(Debug)]
struct Costs {
    advances: Vec<i64>,
    spacing: RangeIndex,
    hanging_advances: Vec<i64>,
    hanging_units: Vec<usize>,
    last_content: Vec<usize>,
    last_preserved: Vec<usize>,
    last_blocked_preserved: Vec<usize>,
    bad_atomics: Vec<usize>,
    tabs: Vec<usize>,
    tab_prefix: Option<TabPrefix>,
}

/// Extra advances of the tabs after one range start, grown as later ends
/// are asked for. Only tab units are stored, so a new start costs work in
/// the tabs it covers, not in every unit.
#[derive(Debug)]
struct TabPrefix {
    start: usize,
    /// Every tab below `through` is covered.
    through: usize,
    /// Index into `Costs::tabs` of the first tab at or after `start`.
    first: usize,
    /// `extra[k]`: the extra advance of the first `k + 1` covered tabs.
    extra: Vec<i64>,
    /// Whether computing any covered tab step changed the saturation
    /// counters: an OR of per-step flags, since the counters wrap.
    effects: bool,
}

impl TabPrefix {
    /// Extra advance of the covered tabs before unit `end` (`end <= through`).
    fn before(&self, tabs: &[usize], end: usize) -> i64 {
        let covered = &tabs[self.first..self.first + self.extra.len()];
        match covered.partition_point(|t| *t < end) {
            0 => 0,
            k => self.extra[k - 1],
        }
    }
}

/// Add one tab step's saturation to the caller's counters. Counters only
/// ever increment; wrapping matches release `+=` and `line::replay`.
fn absorb(sat: &mut Saturation, step: Saturation) {
    sat.saturated = sat.saturated.wrapping_add(step.saturated);
    sat.non_finite = sat.non_finite.wrapping_add(step.non_finite);
}

fn build(
    data: &ParagraphData,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Costs {
    let mut advances = vec![0_i64];
    let mut hanging_advances = vec![0_i64];
    let mut hanging_units = Vec::new();
    let mut last_content = vec![0];
    let mut last_preserved = vec![0];
    let mut last_blocked_preserved = vec![0];
    let mut bad_atomics = Vec::new();
    let mut tabs = Vec::new();
    let mut local_warnings = crate::limits::WarningSink::new(Some(0));
    for (i, unit) in data.units.iter().enumerate() {
        #[cfg(test)]
        {
            cx.ruby_measure_visits += 1;
        }
        let width = if let UnitKind::Atomic { node } = unit.kind {
            let mut local_sat = Saturation::default();
            let value = atomics
                .get(node)
                .map(|value| crate::sanitize::atomic(*value, &mut local_warnings, &mut local_sat));
            let width = value.map_or(LayoutUnit::ZERO, |value| {
                LayoutUnit::from_f32_round(
                    value.inline_size + value.margins.inline_sum(),
                    &mut local_sat,
                )
            });
            if value.is_none() || !local_warnings.take().is_empty() || !local_sat.is_clean() {
                bad_atomics.push(i);
            }
            width
        } else if matches!(unit.kind, UnitKind::Tab) && unit.combine.is_none() {
            tabs.push(i);
            LayoutUnit::ZERO
        } else {
            super::scan::unit_width(data, unit, LayoutUnit::ZERO, atomics, cx, sat)
        };
        let width = width.add(data.unit_spacing[i].word, sat);
        advances.push(advances.last().unwrap() + i64::from(width.raw()));
        let hanging = super::whitespace::hangable(data, i);
        hanging_advances.push(
            hanging_advances.last().unwrap() + if hanging { i64::from(width.raw()) } else { 0 },
        );
        if hanging {
            hanging_units.push(i);
        }
        last_content.push(if !hanging && !super::whitespace::transparent(data, i) {
            i + 1
        } else {
            *last_content.last().unwrap()
        });
        last_preserved.push(if hanging && super::whitespace::preserved(data, i) {
            i + 1
        } else {
            *last_preserved.last().unwrap()
        });
        let blocks = if let UnitKind::Close { box_index } = unit.kind {
            let e = data.boxes[box_index as usize].edges;
            e.padding.inline_end != 0.0 || e.border.inline_end != 0.0
        } else {
            false
        };
        last_blocked_preserved.push(if blocks {
            *last_preserved.last().unwrap()
        } else {
            *last_blocked_preserved.last().unwrap()
        });
    }
    let spacing = RangeIndex::new(
        data.units.iter().enumerate().map(|(i, u)| {
            (
                if matches!(u.kind, UnitKind::Tab) && u.combine.is_none() {
                    data.base_level
                } else {
                    u.level
                },
                data.unit_spacing[i].summary,
            )
        }),
        Some(data),
    );
    Costs {
        advances,
        spacing,
        hanging_advances,
        hanging_units,
        last_content,
        last_preserved,
        last_blocked_preserved,
        bad_atomics,
        tabs,
        tab_prefix: None,
    }
}

pub(super) fn width(
    data: &ParagraphData,
    range: Range<usize>,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<LayoutUnit> {
    #[cfg(test)]
    {
        cx.ruby_width_calls += 1;
    }
    let first = data
        .selectable_clusters
        .partition_point(|u| (*u as usize) < range.start);
    let shared = data
        .selectable_clusters
        .get(first)
        .map(|i| &data.units[*i as usize])
        .and_then(|u| u.shared_cluster.as_ref())
        .filter(|c| {
            data.units[range.start].text.start > c.text.start
                && data.units[range.start].text.start < c.text.end
        });
    let mut corrections = Vec::new();
    if let Some(shared) = shared {
        let begin = shared.slices.partition_point(|i| *i < range.start);
        let finish = shared.slices.partition_point(|i| *i < range.end);
        for &i in &shared.slices[begin..finish] {
            #[cfg(test)]
            {
                cx.ruby_measure_visits += 1;
            }
            let actual = super::scan::unit_width_from(
                data,
                &data.units[i],
                data.units[range.start].text.start,
                LayoutUnit::ZERO,
                atomics,
                cx,
                sat,
            );
            corrections.push((i, actual.sub(data.units[i].slice_advance, sat)));
        }
    }
    let clipped_delta = corrections
        .iter()
        .fold(LayoutUnit::ZERO, |w, (_, delta)| w.add(*delta, sat));
    let hyphen_end = super::plan::hyphen_end(data, range.end).filter(|end| *end > range.start);
    let hyphen_windows =
        hyphen_end.and_then(|end| super::hyphen::line(data, range.start, end, cx, sat));
    let hyphen_leaf = hyphen_windows.as_ref().and(hyphen_end).map(|end| {
        (
            end - 1,
            super::spacing::hyphen_unit_summary(data, end - 1, sat),
        )
    });
    if cx
        .ruby_ranges
        .root
        .is_none_or(|root| root.2 != atomics.revision)
    {
        cx.ruby_ranges.begin(data, atomics);
    }
    let key = (data.id, data as *const ParagraphData as usize);
    if !cx.ruby_ranges.sets.contains_key(&key) {
        let costs = build(data, atomics, cx, sat);
        cx.ruby_ranges.sets.insert(key, costs);
        cx.ruby_ranges.fill();
    }
    // Each tab step's value and saturation depend only on the paragraph,
    // the range start and the tab (`clipped_delta` is the same for every
    // end past the tab: a preserved tab is its own item, so no shared
    // cluster slice lies after it). A step charges the caller's `sat` only
    // when it is computed, as before. Discarding or computing a step whose
    // saturation was clean therefore changes no later query's effects and
    // moves no generation; an effectful step keeps the old rule: computing
    // it fills, discarding it for another start invalidates.
    let discards_effects = cx.ruby_ranges.sets.get(&key).is_some_and(|costs| {
        costs
            .tab_prefix
            .as_ref()
            .is_some_and(|p| p.start != range.start && p.effects)
    });
    if discards_effects {
        cx.ruby_ranges.invalidate();
    }
    let mut computed_effects = false;
    let costs = cx.ruby_ranges.sets.get_mut(&key)?;
    let mut tab_total = 0_i64;
    if !costs.tabs.is_empty() {
        if costs
            .tab_prefix
            .as_ref()
            .is_none_or(|p| p.start != range.start)
        {
            costs.tab_prefix = Some(TabPrefix {
                start: range.start,
                through: range.start,
                first: costs.tabs.partition_point(|t| *t < range.start),
                extra: Vec::new(),
                effects: false,
            });
        }
        let prefix = costs.tab_prefix.as_mut().unwrap();
        if prefix.through < range.end {
            // The start's decoration is the same for every step: measure it
            // once and charge its saturation per step, as calling it per
            // step did.
            let mut decoration: Option<(LayoutUnit, Saturation)> = None;
            let from = prefix.first + prefix.extra.len();
            let to = costs.tabs.partition_point(|t| *t < range.end);
            for &i in &costs.tabs[from..to] {
                #[cfg(test)]
                {
                    cx.ruby_measure_visits += 1;
                }
                let previous = prefix.extra.last().copied().unwrap_or(0);
                let mut step = Saturation::default();
                let (start_decoration, decoration_sat) = *decoration.get_or_insert_with(|| {
                    let mut s = Saturation::default();
                    let w = super::decoration::width(data, range.start, true, &mut s);
                    (w, s)
                });
                let pos = raw(
                    costs.advances[i] - costs.advances[range.start] + previous,
                    &mut step,
                )
                .add(clipped_delta, &mut step)
                .add(start_decoration, &mut step)
                .add(
                    costs
                        .spacing
                        .query(range.start..i, Some(data))
                        .width(&mut step),
                    &mut step,
                );
                absorb(&mut step, decoration_sat);
                let extra =
                    i64::from(super::scan::tab_width(data, &data.units[i], pos, &mut step).raw());
                prefix.extra.push(previous + extra);
                if !step.is_clean() {
                    prefix.effects = true;
                    computed_effects = true;
                }
                absorb(sat, step);
            }
            prefix.through = range.end;
        }
        tab_total = prefix.before(&costs.tabs, range.end);
    }
    if computed_effects {
        cx.ruby_ranges.fill();
    }
    let costs = cx.ruby_ranges.sets.get(&key)?;
    let mut stop = range
        .start
        .max(costs.last_content[range.end])
        .max(costs.last_blocked_preserved[range.end]);
    if super::whitespace::obstructed(data, range.end) {
        stop = stop.max(costs.last_preserved[range.end]);
    }
    let next = costs.hanging_units.partition_point(|u| *u < stop);
    let hang = costs
        .hanging_units
        .get(next)
        .copied()
        .filter(|i| *i < range.end)
        .unwrap_or(range.end);
    let trailing_tabs = costs
        .tab_prefix
        .as_ref()
        .filter(|p| p.start == range.start)
        .map_or(0, |p| {
            p.before(&costs.tabs, range.end) - p.before(&costs.tabs, hang)
        });
    let trailing = costs.hanging_advances[range.end] - costs.hanging_advances[hang] + trailing_tabs;
    let natural = raw(
        costs.advances[range.end] - costs.advances[range.start] + tab_total - trailing,
        sat,
    );
    let natural = natural.add(
        corrections
            .iter()
            .filter(|(i, _)| *i < hang)
            .fold(LayoutUnit::ZERO, |w, (_, delta)| w.add(*delta, sat)),
        sat,
    );
    let summary = costs
        .spacing
        .query_replace(range.start..hang, hyphen_leaf, Some(data));
    let width = natural
        .add(summary.width(sat), sat)
        .add(super::decoration::width(data, range.start, true, sat), sat)
        .add(super::decoration::width(data, range.end, false, sat), sat);
    #[cfg(test)]
    {
        cx.ruby_measure_visits += costs.spacing.take_visits();
    }
    let bad_start = costs.bad_atomics.partition_point(|i| *i < range.start);
    let bad_end = costs.bad_atomics.partition_point(|i| *i < range.end);
    // Only selected invalid caller values report warnings; the sink's cap also
    // bounds work on repeated hostile candidates after its suppression marker.
    let bad: Vec<_> = costs.bad_atomics[bad_start..bad_end]
        .iter()
        .copied()
        .take(if cx.warnings.is_suppressed() {
            0
        } else {
            data.limits
                .max_warnings
                .unwrap_or(u64::MAX)
                .saturating_add(1)
                .min(usize::MAX as u64) as usize
        })
        .collect();
    for i in bad {
        if cx.warnings.is_suppressed() {
            break;
        }
        super::scan::unit_width(data, &data.units[i], LayoutUnit::ZERO, atomics, cx, sat);
    }
    let delta = match hyphen_windows {
        Some(windows) => super::windows::cost(&windows, range.end, sat),
        None => super::windows::delta(data, range.start, range.end, cx, sat),
    };
    let width = width.add(delta, sat);
    let adjustment = super::punctuation::edges(
        data,
        summary,
        0,
        super::punctuation::last_edge(data, range.end),
        None,
        LayoutUnit::MAX,
        width,
        sat,
    );
    Some(width.sub(adjustment.removed(sat), sat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{
        BoxDecorationBreak, FontFamily, InlineStyle, ParagraphStyle, UnicodeBidi,
        WhiteSpaceCollapse,
    };
    use crate::{Paragraph, ParagraphBuilder};

    fn fonts() -> FontCollection {
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
        fonts
    }

    #[test]
    fn indexed_discretionary_hyphens_match_actual_bidi_spacing_and_edge_windows() {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            let root = InlineStyle {
                font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
                font_size: 12.0,
                letter_spacing: 1.5,
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction,
                    root: root.clone(),
                    ..Default::default()
                },
                &Limits::default(),
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "ab\u{ad}");
            b.open_inline(
                NodeId(2),
                &InlineStyle {
                    unicode_bidi: UnicodeBidi::Isolate,
                    letter_spacing: 2.5,
                    box_decoration_break: BoxDecorationBreak::Clone,
                    ..root
                },
                InlineEdges {
                    padding: Sides {
                        inline_start: 3.0,
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            b.push_text(TextSource::Generated { node: NodeId(3) }, "cd\u{ad}");
            b.close_inline();
            b.push_text(TextSource::Generated { node: NodeId(4) }, "ef");
            let mut cx = LayoutContext::new();
            let p = b.build(&mut cx, &fonts).unwrap();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .expect("manual hyphen uses actual indexed costs");
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "{direction:?}: {start}..{end}");
                }
            }
        }
    }

    #[test]
    fn indexed_source_clipped_shared_cluster_keeps_actual_remaining_glyph_width() {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture Latin".into())],
            font_size: 12.0,
            overflow_wrap: crate::style::OverflowWrap::Anywhere,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            for suffix in ["WWWW", "\tWWWW "] {
                let mut b = ParagraphBuilder::new(
                    &ParagraphStyle {
                        root: root.clone(),
                        direction,
                        ..Default::default()
                    },
                    &Limits::default(),
                );
                for (node, text) in [(1, "f"), (2, "f"), (3, "i"), (4, suffix)] {
                    b.push_text(
                        TextSource::Dom {
                            node: NodeId(node),
                            offset: 40,
                        },
                        text,
                    );
                }
                let mut cx = LayoutContext::new();
                let p = b.build(&mut cx, &fonts).unwrap();
                assert!(p.data.units.iter().any(|u| {
                    u.shared_cluster
                        .as_ref()
                        .is_some_and(|c| c.slices.len() == 3)
                }));
                cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
                for start in 0..p.data.units.len() {
                    for end in start + 1..=p.data.units.len() {
                        let actual = width(
                            &p.data,
                            start..end,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut Saturation::default(),
                        )
                        .expect("only the clipped source cluster needs bounded correction");
                        let expected = super::super::plan::selected_raw(
                            &p.data,
                            start,
                            end,
                            LayoutUnit::ZERO,
                            LayoutUnit::ZERO,
                            0,
                            None,
                            LayoutUnit::MAX,
                            &AtomicSizes::EMPTY,
                            &mut cx,
                            &mut Saturation::default(),
                        )
                        .content;
                        assert_eq!(
                            actual, expected,
                            "{direction:?}, {suffix:?}: source clipped {start}..{end}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn indexed_tabs_and_atomic_ranges_match_actual_selected_windows_after_revision() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            letter_spacing: 1.5,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t");
        b.push_atomic(NodeId(9), &root, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(2) }, "本\t語 ");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let mut atomics = AtomicSizes::new();
        for size in [17.0, 31.0] {
            atomics.insert(
                NodeId(9),
                crate::AtomicSize {
                    inline_size: size,
                    margins: Sides {
                        inline_start: 2.0,
                        inline_end: 3.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            cx.ruby_ranges.begin(&p.data, &atomics);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &atomics,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .expect("dynamic range must use the index");
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &atomics,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "atomic{size}: {start}..{end}");
                }
            }
        }
    }

    #[test]
    fn indexing_a_missing_atomic_warns_only_when_its_range_is_selected() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日");
        b.push_atomic(NodeId(9), &root, InlineEdges::default());
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        cx.take_warnings();
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        assert_eq!(
            width(
                &p.data,
                0..1,
                &AtomicSizes::EMPTY,
                &mut cx,
                &mut Saturation::default()
            )
            .unwrap()
            .to_f32(),
            24.0
        );
        assert!(cx.take_warnings().is_empty());
        width(
            &p.data,
            0..p.data.units.len(),
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        )
        .unwrap();
        assert!(
            cx.take_warnings()
                .iter()
                .any(|w| w.kind == crate::limits::WarningKind::MissingAtomicSize)
        );
    }

    #[test]
    fn indexed_actual_ranges_match_selected_edge_spacing_and_cloned_boxes() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
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
        for direction in [
            crate::geometry::Direction::Ltr,
            crate::geometry::Direction::Rtl,
        ] {
            let root = InlineStyle {
                font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
                font_size: 24.0,
                letter_spacing: 1.5,
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            };
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    direction,
                    root: root.clone(),
                    ..Default::default()
                },
                &limits,
            );
            b.push_text(TextSource::Generated { node: NodeId(1) }, "日本 ");
            b.open_inline(
                NodeId(2),
                &InlineStyle {
                    font_size: 12.0,
                    unicode_bidi: UnicodeBidi::Isolate,
                    box_decoration_break: BoxDecorationBreak::Clone,
                    ..root
                },
                InlineEdges {
                    padding: Sides {
                        inline_start: 3.0,
                        inline_end: 5.0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            b.push_text(TextSource::Generated { node: NodeId(3) }, "にほん ");
            b.close_inline();
            b.push_text(TextSource::Generated { node: NodeId(4) }, "語ご ");
            let mut cx = LayoutContext::new();
            let p = b.build(&mut cx, &fonts).unwrap();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for start in 0..p.data.units.len() {
                for end in start + 1..=p.data.units.len() {
                    let actual = width(
                        &p.data,
                        start..end,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .unwrap();
                    let expected = super::super::plan::selected_raw(
                        &p.data,
                        start,
                        end,
                        LayoutUnit::ZERO,
                        LayoutUnit::ZERO,
                        0,
                        None,
                        LayoutUnit::MAX,
                        &AtomicSizes::EMPTY,
                        &mut cx,
                        &mut Saturation::default(),
                    )
                    .content;
                    assert_eq!(actual, expected, "{direction:?}: units{start}..{end}");
                }
            }
        }
    }

    #[test]
    fn generation_splits_into_monotone_fills_and_invalidating_epochs() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t本\t語");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let n = p.data.units.len();
        assert!(n >= 4, "{n}");
        let state = |cx: &LayoutContext| {
            (
                cx.ruby_ranges.generation(),
                cx.ruby_ranges.fills(),
                cx.ruby_ranges.epoch(),
            )
        };
        let query = |cx: &mut LayoutContext, range: Range<usize>| {
            width(
                &p.data,
                range,
                &AtomicSizes::EMPTY,
                cx,
                &mut Saturation::default(),
            )
            .unwrap();
        };
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let (g, f, e) = state(&cx);
        // Building the costs fills; zero-effect tab steps do not.
        query(&mut cx, 0..2);
        assert_eq!(state(&cx), (g + 1, f + 1, e));
        // Extending the same start's prefix without effects changes nothing.
        query(&mut cx, 0..n);
        assert_eq!(state(&cx), (g + 1, f + 1, e));
        // A covered range changes nothing.
        query(&mut cx, 0..3);
        assert_eq!(state(&cx), (g + 1, f + 1, e));
        // Replacing a prefix without effects changes nothing.
        query(&mut cx, 1..n);
        assert_eq!(state(&cx), (g + 1, f + 1, e));
        // A block measurement fills.
        block_size(
            &p.data,
            0..2,
            &Default::default(),
            &AtomicSizes::EMPTY,
            &mut cx,
            &mut Saturation::default(),
        );
        assert_eq!(state(&cx), (g + 2, f + 2, e));
        // A new atomic revision clears every cache.
        let mut atomics = AtomicSizes::new();
        atomics.insert(NodeId(9), crate::AtomicSize::default());
        cx.ruby_ranges.begin(&p.data, &atomics);
        assert_eq!(state(&cx), (g + 3, f + 2, e + 1));
        // Vacating the slots invalidates.
        cx.ruby_ranges.vacate_slots();
        assert_eq!(state(&cx), (g + 4, f + 2, e + 2));
    }

    /// Tab steps that saturate keep the old rule: computing one fills,
    /// discarding a prefix that holds one invalidates.
    #[test]
    fn effectful_tab_steps_fill_and_their_discard_invalidates() {
        let root = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(1.0e12),
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: root.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t本\t語");
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let n = p.data.units.len();
        let state = |cx: &LayoutContext| (cx.ruby_ranges.fills(), cx.ruby_ranges.epoch());
        let query = |cx: &mut LayoutContext, range: Range<usize>| {
            let mut sat = Saturation::default();
            width(&p.data, range, &AtomicSizes::EMPTY, cx, &mut sat).unwrap();
            sat
        };
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let (f, e) = state(&cx);
        // Costs build fills once; the saturating first tab fills again.
        assert!(!query(&mut cx, 0..2).is_clean());
        assert_eq!(state(&cx), (f + 2, e));
        // A covered range neither charges nor fills.
        assert!(query(&mut cx, 0..2).is_clean());
        assert_eq!(state(&cx), (f + 2, e));
        // Another start discards the effectful prefix: an invalidation, and
        // its own saturating steps fill.
        assert!(!query(&mut cx, 1..n).is_clean());
        assert_eq!(state(&cx), (f + 3, e + 1));
    }

    /// A prefix whose early tab steps are clean and a later one saturates:
    /// the clean extension moves nothing, the saturating extension fills,
    /// and the latched `effects` flag makes the next start invalidate.
    #[test]
    fn mixed_tab_prefix_latches_effects_when_a_later_step_saturates() {
        let clean = InlineStyle {
            font_families: vec![FontFamily::Named("Shodo Fixture CJK".into())],
            font_size: 24.0,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size: crate::style::TabSize::Px(40.0),
            ..Default::default()
        };
        let huge = InlineStyle {
            tab_size: crate::style::TabSize::Px(1.0e12),
            ..clean.clone()
        };
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: clean.clone(),
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\t本");
        b.open_inline(NodeId(2), &huge, InlineEdges::default());
        b.push_text(TextSource::Generated { node: NodeId(3) }, "\t語");
        b.close_inline();
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts()).unwrap();
        let tabs: Vec<usize> = p
            .data
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| matches!(u.kind, UnitKind::Tab))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(tabs.len(), 2, "{tabs:?}");
        let (first, second) = (tabs[0], tabs[1]);
        let n = p.data.units.len();
        let state = |cx: &LayoutContext| (cx.ruby_ranges.fills(), cx.ruby_ranges.epoch());
        let query = |cx: &mut LayoutContext, range: Range<usize>| {
            let mut sat = Saturation::default();
            width(&p.data, range, &AtomicSizes::EMPTY, cx, &mut sat).unwrap();
            sat
        };
        cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
        let (f, e) = state(&cx);
        // Building the costs fills once; the clean first tab adds nothing.
        assert!(query(&mut cx, 0..first + 1).is_clean());
        assert_eq!(state(&cx), (f + 1, e));
        // A clean extension of the same start moves nothing.
        assert!(query(&mut cx, 0..second).is_clean());
        assert_eq!(state(&cx), (f + 1, e));
        // The extension whose new step saturates fills.
        assert!(!query(&mut cx, 0..n).is_clean());
        assert_eq!(state(&cx), (f + 2, e));
        // Covered again: no step is computed, so nothing moves (the total
        // itself may still saturate the width).
        query(&mut cx, 0..n);
        assert_eq!(state(&cx), (f + 2, e));
        // Another start discards the prefix whose flag latched: invalidate,
        // and its own saturating step fills.
        assert!(!query(&mut cx, 1..n).is_clean());
        assert_eq!(state(&cx), (f + 3, e + 1));
    }

    /// One paragraph per golden fixture: preserved tabs under different
    /// tab sizes, a ligature clipped by the range start, hanging trailing
    /// tabs and a tab interval that saturates.
    fn tab_golden_paragraphs() -> Vec<(&'static str, Paragraph)> {
        let fonts = fonts();
        fonts
            .register_face(
                crate::test_support::fonts::LATIN.to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Shodo Fixture Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = |family: &str, tab_size: crate::style::TabSize| InlineStyle {
            font_families: vec![FontFamily::Named(family.into())],
            font_size: 24.0,
            letter_spacing: 1.5,
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            tab_size,
            ..Default::default()
        };
        let build = |root: InlineStyle, texts: &[(u64, &str)]| {
            let mut b = ParagraphBuilder::new(
                &ParagraphStyle {
                    root: root.clone(),
                    ..Default::default()
                },
                &Limits::default(),
            );
            for (node, text) in texts {
                b.push_text(
                    TextSource::Dom {
                        node: NodeId(*node),
                        offset: 40,
                    },
                    text,
                );
            }
            b.build(&mut LayoutContext::new(), &fonts).unwrap()
        };
        use crate::style::TabSize;
        vec![
            (
                "cjk",
                build(
                    style("Shodo Fixture CJK", TabSize::Px(40.0)),
                    &[(1, "日\t本\t\t語\t \t")],
                ),
            ),
            (
                "spaces",
                build(
                    style("Shodo Fixture CJK", TabSize::Spaces(3.0)),
                    &[(1, "日本\t語\t日")],
                ),
            ),
            (
                "ligature",
                build(
                    InlineStyle {
                        font_size: 12.0,
                        letter_spacing: 0.0,
                        overflow_wrap: crate::style::OverflowWrap::Anywhere,
                        ..style("Shodo Fixture Latin", TabSize::Px(40.0))
                    },
                    &[(1, "f"), (2, "f"), (3, "i"), (4, "\tWW\tW ")],
                ),
            ),
            (
                "huge",
                build(
                    style("Shodo Fixture CJK", TabSize::Px(1.0e12)),
                    &[(1, "日\t本\t語\t")],
                ),
            ),
        ]
    }

    /// Value and saturation of every query of a fixed history, one context
    /// per fixture (see `tab_golden_paragraphs`).
    fn tab_golden() -> String {
        let mut out = String::new();
        for (name, p) in tab_golden_paragraphs() {
            if name == "ligature" {
                assert!(p.data.units.iter().any(|u| {
                    u.shared_cluster
                        .as_ref()
                        .is_some_and(|c| c.slices.len() == 3)
                }));
            }
            let n = p.data.units.len();
            let mut history = vec![0..2, 0..n, 0..3, 1..n, 0..n, 2..n, n - 2..n, 1..3, 0..n];
            for start in 0..n {
                for end in start + 1..=n {
                    history.push(start..end);
                }
            }
            for end in (1..=n).rev() {
                history.push(0..end);
            }
            let mut cx = LayoutContext::new();
            cx.ruby_ranges.begin(&p.data, &AtomicSizes::EMPTY);
            for range in history {
                let mut sat = Saturation::default();
                let value = width(
                    &p.data,
                    range.clone(),
                    &AtomicSizes::EMPTY,
                    &mut cx,
                    &mut sat,
                )
                .map(|w| w.raw());
                out.push_str(&format!(
                    "{name} {range:?} {value:?} {} {}\n",
                    sat.saturated, sat.non_finite
                ));
            }
        }
        out
    }

    /// Byte-identity with the previous tab prefix: the golden file was
    /// captured on the code before shodo-b7d. Regenerate only on purpose
    /// with `SHODO_UPDATE_GOLDEN=1`.
    #[test]
    fn tab_width_queries_match_the_golden_record() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/line/testdata/b7d_tab_golden.txt"
        );
        let got = tab_golden();
        if std::env::var_os("SHODO_UPDATE_GOLDEN").is_some() {
            std::fs::write(path, &got).unwrap();
            panic!("golden updated; rerun without SHODO_UPDATE_GOLDEN");
        }
        let want = std::fs::read_to_string(path).expect("golden file");
        assert_eq!(got, want);
        // The record must exercise saturation, a clipped ligature and tabs.
        assert!(
            want.lines()
                .any(|l| l.starts_with("huge ") && !l.ends_with(" 0 0")),
            "the huge tab size must saturate"
        );
    }
}
