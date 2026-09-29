//! Bounded edge windows shared by candidate measurements and materialization.
use super::reshape::EdgeOverlay;
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::{WarningKind, WarningSink};
use crate::paragraph::ParagraphData;
use crate::{AtomicSizes, LayoutContext};
use std::ops::Range;
use std::sync::Arc;

pub(super) struct Window {
    pub(super) overlay: EdgeOverlay,
    /// Glyph contributions only: box edges and anchors keep their widths.
    pub(super) changes: Vec<(usize, LayoutUnit, LayoutUnit)>,
}
impl Window {
    pub(super) fn delta(&self, through: usize, sat: &mut Saturation) -> LayoutUnit {
        self.changes
            .iter()
            .filter(|(i, _, _)| *i < through)
            .fold(LayoutUnit::ZERO, |p, (_, old, new)| {
                p.add(new.sub(*old, sat), sat)
            })
    }
}

pub(super) fn hyphen(
    data: &ParagraphData,
    start: usize,
    end: usize,
    replacement: &crate::shape::Replacement,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<Vec<Window>> {
    if !remaining_valid(data, start, end, cx, sat) {
        return None;
    }
    let windows = measure_edit(data, start, end, Some(replacement), cx, sat);
    let generated = windows.iter().any(|w| {
        w.overlay.text.start <= replacement.text.start && replacement.text.end <= w.overlay.text.end
    });
    if !generated || !covers_partial_edges(data, start, end, &windows) {
        return None;
    }
    Some(windows)
}

pub(super) fn cost(windows: &[Window], end: usize, sat: &mut Saturation) -> LayoutUnit {
    windows
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat))
}

fn first(data: &ParagraphData, at: usize, end: usize) -> Option<usize> {
    if at >= end {
        return None;
    }
    if matches!(data.units[at].kind, UnitKind::Cluster { .. }) {
        return Some(at);
    }
    let i = data
        .selectable_clusters
        .partition_point(|i| (*i as usize) < at);
    data.selectable_clusters
        .get(i)
        .map(|i| *i as usize)
        .filter(|i| *i < end)
}
fn last(data: &ParagraphData, start: usize, end: usize) -> Option<usize> {
    if start >= end {
        return None;
    }
    if matches!(data.units[end - 1].kind, UnitKind::Cluster { .. }) {
        return Some(end - 1);
    }
    let i = data
        .selectable_clusters
        .partition_point(|i| (*i as usize) < end);
    let at = *data.selectable_clusters.get(i.checked_sub(1)?)? as usize;
    (at >= start).then_some(at)
}
fn group(data: &ParagraphData, at: usize) -> Range<usize> {
    data.units[at]
        .shared_cluster
        .as_ref()
        .map_or(at..at + 1, |c| c.units.clone())
}
fn clipped_group(data: &ParagraphData, at: usize, line: &Range<usize>) -> Range<usize> {
    let group = group(data, at);
    let start = first(data, group.start.max(line.start), at + 1).unwrap();
    let end = last(data, at, group.end.min(line.end)).unwrap() + 1;
    start..end
}
fn compatible(data: &ParagraphData, a: usize, b: usize) -> bool {
    if data.units[a].combine.is_some() || data.units[b].combine.is_some() {
        return false;
    }
    let (UnitKind::Cluster { run: a_run, .. }, UnitKind::Cluster { run: b_run, .. }) =
        (&data.units[a].kind, &data.units[b].kind)
    else {
        return false;
    };
    let (a_run, b_run) = (&data.runs[*a_run as usize], &data.runs[*b_run as usize]);
    data.units[a].level == data.units[b].level
        && a_run.font == b_run.font
        && Arc::ptr_eq(&a_run.instance, &b_run.instance)
}
fn unsafe_join(data: &ParagraphData, a: usize, b: usize) -> bool {
    data.units[a].unsafe_to_break || data.units[b].unsafe_to_concat
}
fn storage_split(data: &ParagraphData, at: usize) -> bool {
    if data.units[at].shared_cluster.is_some() {
        return false;
    }
    [last(data, 0, at), first(data, at + 1, data.units.len())]
        .into_iter()
        .flatten()
        .any(|i| data.units[i].text == data.units[at].text)
}
fn partial(data: &ParagraphData, range: &Range<usize>) -> bool {
    data.units[range.start]
        .shared_cluster
        .as_ref()
        .is_some_and(|c| {
            data.units[range.start].text.start != c.text.start
                || data.units[range.end - 1].text.end != c.text.end
        })
}

/// Edge windows are a pure function of the paragraph, unit range and glyph
/// budget, and a line scan revisits the same few windows at every break
/// candidate. Keep results for the most recent paragraph only, so memory stays
/// bounded by the entry cap regardless of how many paragraphs a context lays
/// out. Only clean, unedited results are retained.
#[derive(Debug, Default)]
pub(crate) struct EdgeShapeCache {
    owner: Option<(u64, usize)>,
    entries: std::collections::HashMap<(usize, usize, Option<u64>), ShapedWindow>,
}

type ShapedWindow = (crate::shape::GlyphStore, Vec<crate::shape::ShapedRun>);

const EDGE_SHAPE_CACHE_ENTRIES: usize = 256;

impl EdgeShapeCache {
    fn begin(&mut self, data: &ParagraphData) {
        let owner = (data.id, data as *const ParagraphData as usize);
        if self.owner != Some(owner) {
            self.entries.clear();
            self.owner = Some(owner);
        }
    }
}

fn shape(
    data: &ParagraphData,
    range: &Range<usize>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
    budget: Option<u64>,
    replacement: Option<&crate::shape::Replacement>,
) -> Option<ShapedWindow> {
    let key = (range.start, range.end, budget);
    if replacement.is_none() {
        cx.edge_shapes.begin(data);
        if let Some(hit) = cx.edge_shapes.entries.get(&key) {
            return Some(hit.clone());
        }
    }
    let mut unit = data.units[range.start].clone();
    unit.text = unit.text.start..data.units[range.end - 1].text.end;
    #[cfg(test)]
    {
        data.edge_shape_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        data.edge_shape_bytes.fetch_add(
            (unit.text.end - unit.text.start) as usize,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let mut warnings = WarningSink::new(data.limits.max_warnings);
    let saturation_before = *sat;
    let result =
        crate::shape::shape_window_edit(data, &unit, budget, replacement, cx, &mut warnings, sat);
    let warned = warnings.take();
    let clean = warned.is_empty() && *sat == saturation_before;
    if replacement.is_none()
        && clean
        && let Some(shaped) = &result
    {
        if cx.edge_shapes.entries.len() >= EDGE_SHAPE_CACHE_ENTRIES {
            cx.edge_shapes.entries.clear();
        }
        cx.edge_shapes.entries.insert(key, shaped.clone());
    }
    for w in warned {
        cx.warnings.push(w.kind, w.message);
    }
    result
}

fn within_window_budget(data: &ParagraphData, range: &Range<usize>) -> bool {
    data.limits.max_reshape_window_bytes.is_none_or(|max| {
        u64::from(data.units[range.end - 1].text.end - data.units[range.start].text.start) <= max
    })
}

fn expand_original(data: &ParagraphData, line: &Range<usize>, range: &mut Range<usize>) -> bool {
    if !within_window_budget(data, range) {
        return false;
    }
    while let Some(previous) = last(data, line.start, range.start)
        && compatible(data, previous, range.start)
        && unsafe_join(data, previous, range.start)
    {
        #[cfg(test)]
        data.window_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        range.start = clipped_group(data, previous, line).start;
        if !within_window_budget(data, range) {
            return false;
        }
    }
    while let Some(next) = first(data, range.end, line.end)
        && compatible(data, range.end - 1, next)
        && unsafe_join(data, range.end - 1, next)
    {
        #[cfg(test)]
        data.window_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        range.end = clipped_group(data, next, line).end;
        if !within_window_budget(data, range) {
            return false;
        }
    }
    true
}

fn window_budget_warning(cx: &mut LayoutContext) {
    cx.warnings.push(
        crate::limits::WarningKind::Unsupported,
        "unsafe edge exceeds reshape window budget; retaining shared glyphs",
    );
}

/// Re-shape to original safe boundaries, then check new concatenation flags.
/// A right-hand probe detects a changed join without submitting raw markers.
fn materialize(
    data: &ParagraphData,
    line: &Range<usize>,
    mut range: Range<usize>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
    budget: Option<u64>,
    replacement: Option<&crate::shape::Replacement>,
) -> Option<Window> {
    if !expand_original(data, line, &mut range) {
        window_budget_warning(cx);
        return None;
    }
    loop {
        if storage_split(data, range.start) || storage_split(data, range.end - 1) {
            cx.warnings.push(
                WarningKind::Unsupported,
                "resource-split shaping cluster retained whole at line edge",
            );
            return None;
        }
        let (mut store, mut runs) = shape(data, &range, cx, sat, budget, replacement)?;
        if store.flags.first().is_some_and(|f| f & 2 != 0)
            && let Some(previous) = last(data, line.start, range.start)
            && compatible(data, previous, range.start)
        {
            range.start = clipped_group(data, previous, line).start;
            if !expand_original(data, line, &mut range) {
                window_budget_warning(cx);
                return None;
            }
            continue;
        }
        if let Some(next) = first(data, range.end, line.end)
            && compatible(data, range.end - 1, next)
        {
            let probe = range.start..clipped_group(data, next, line).end;
            // Keep only one temporary SoA at a time during validation.
            drop(store);
            drop(runs);
            let (tested, _) = shape(data, &probe, cx, sat, budget, replacement)?;
            let boundary = data.units[next].text.start;
            let unsafe_probe = tested
                .cluster
                .iter()
                .position(|c| *c >= boundary)
                .is_none_or(|i| tested.flags[i] & 3 != 0);
            drop(tested);
            if unsafe_probe {
                range.end = probe.end;
                if !expand_original(data, line, &mut range) {
                    window_budget_warning(cx);
                    return None;
                }
                continue;
            }
            (store, runs) = shape(data, &range, cx, sat, budget, replacement)?;
        }
        let mut changes = Vec::new();
        let mut at = range.start;
        while let Some(i) = first(data, at, range.end) {
            let end = group(data, i).end.min(range.end);
            let begin = data
                .selectable_clusters
                .partition_point(|u| (*u as usize) < i);
            let finish = data
                .selectable_clusters
                .partition_point(|u| (*u as usize) < end);
            for index in data.selectable_clusters[begin..finish]
                .iter()
                .map(|i| *i as usize)
            {
                let old = super::scan::unit_width_from(
                    data,
                    &data.units[index],
                    data.units[line.start].text.start,
                    LayoutUnit::ZERO,
                    &AtomicSizes::EMPTY,
                    cx,
                    sat,
                );
                changes.push((index, old, LayoutUnit::ZERO));
            }
            at = end;
        }
        let mut unit = 0;
        for (cluster, advance) in store.cluster.iter().zip(&store.advance) {
            while unit + 1 < changes.len() && data.units[changes[unit + 1].0].text.start <= *cluster
            {
                unit += 1;
            }
            changes[unit].2 = changes[unit].2.add(*advance, sat);
        }
        let UnitKind::Cluster { glyphs: begin, .. } = &data.units[range.start].kind else {
            unreachable!()
        };
        let UnitKind::Cluster { glyphs: end, .. } = &data.units[range.end - 1].kind else {
            unreachable!()
        };
        return Some(Window {
            overlay: EdgeOverlay {
                glyphs: begin.start..end.end,
                text: data.units[range.start].text.start..data.units[range.end - 1].text.end,
                store,
                runs,
                hyphen: replacement
                    .filter(|r| {
                        data.units[range.start].text.start <= r.text.start
                            && r.text.end <= data.units[range.end - 1].text.end
                    })
                    .map(|r| r.text.clone()),
            },
            changes,
        });
    }
}

pub(super) fn measure(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<Window> {
    measure_edit(data, start, end, None, cx, sat)
}

fn measure_edit(
    data: &ParagraphData,
    start: usize,
    end: usize,
    replacement: Option<&crate::shape::Replacement>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Vec<Window> {
    let line = start..end;
    let (Some(first_at), Some(last_at)) = (first(data, start, end), last(data, start, end)) else {
        return Vec::new();
    };
    let first_range = clipped_group(data, first_at, &line);
    let mut last_range = clipped_group(data, last_at, &line);
    if replacement.is_some()
        && let Some(previous) = last(data, start, last_range.start)
        && data.units[previous].level == data.units[last_at].level
        && data
            .shaping_barriers
            .get(
                data.shaping_barriers
                    .partition_point(|i| (*i as usize) <= previous),
            )
            .is_none_or(|i| (*i as usize) >= last_range.start)
    {
        last_range.start = clipped_group(data, previous, &line).start;
    }
    let before = last(data, 0, start);
    let needs_first = data.units[first_at].combine.is_none()
        && (partial(data, &first_range)
            || start > 0
                && (data.units[first_at].unsafe_to_concat
                    || before.is_some_and(|i| data.units[i].unsafe_to_break)));
    let needs_last = data.units[last_at].combine.is_none()
        && (replacement.is_some()
            || partial(data, &last_range)
            || data.units[last_at].unsafe_to_break);
    let mut ranges = Vec::new();
    if needs_first {
        ranges.push(first_range);
    }
    if needs_last {
        ranges.push(last_range);
    }
    ranges.retain_mut(|range| {
        let fits = expand_original(data, &line, range);
        if !fits {
            window_budget_warning(cx);
        }
        fits
    });
    if ranges.len() == 2 && ranges[0].end > ranges[1].start {
        ranges[0].end = ranges[0].end.max(ranges[1].end);
        ranges.pop();
    }
    let mut windows: Vec<Window> = Vec::new();
    for range in ranges {
        let retained = windows
            .iter()
            .map(|w| w.overlay.store.len() as u64)
            .sum::<u64>();
        let budget = data
            .limits
            .max_shaped_glyphs
            .map(|max| max.saturating_sub(retained));
        if let Some(window) = materialize(data, &line, range, cx, sat, budget, replacement) {
            if let Some(previous) = windows.last()
                && previous.overlay.glyphs.end > window.overlay.glyphs.start
            {
                // Revalidation may grow two formerly disjoint windows together.
                let merged = first_at..last_at + 1;
                windows.clear();
                if let Some(window) = materialize(
                    data,
                    &line,
                    merged,
                    cx,
                    sat,
                    data.limits.max_shaped_glyphs,
                    replacement,
                ) {
                    windows.push(window);
                }
                break;
            }
            windows.push(window);
        }
    }
    windows
}

pub(super) fn delta(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    measure(data, start, end, cx, sat)
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(w.delta(end, sat), sat))
}

fn covers_partial_edges(
    data: &ParagraphData,
    start: usize,
    end: usize,
    windows: &[Window],
) -> bool {
    [first(data, start, end), last(data, start, end)]
        .into_iter()
        .flatten()
        .all(|at| {
            let range = clipped_group(data, at, &(start..end));
            !partial(data, &range)
                || windows.iter().any(|window| {
                    window.overlay.text.start <= data.units[range.start].text.start
                        && window.overlay.text.end >= data.units[range.end - 1].text.end
                })
        })
}

/// A shared ligature may only be sliced when the selected source and its
/// remaining suffix both have owned glyphs. Ordinary whole-cluster edges
/// can still use the documented warned shared fallback.
pub(super) fn candidate(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> (LayoutUnit, bool) {
    let windows = measure(data, start, end, cx, sat);
    let delta = cost(&windows, end, sat);
    let covered = covers_partial_edges(data, start, end, &windows);
    drop(windows);
    let valid = covered && remaining_valid(data, start, end, cx, sat);
    (delta, valid)
}

fn remaining_valid(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> bool {
    let remainder = last(data, start, end).is_some_and(|at| group(data, at).end > end);
    !remainder || {
        let windows = measure(data, end, data.units.len(), cx, sat);
        covers_partial_edges(data, end, data.units.len(), &windows)
    }
}

#[cfg(test)]
mod tests {
    fn ligature_font(substitutions: &[(&str, char)]) -> Vec<u8> {
        ligature_font_for(
            include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf"),
            *b"latn",
            substitutions,
        )
    }

    fn ligature_font_for(bytes: &[u8], script: [u8; 4], substitutions: &[(&str, char)]) -> Vec<u8> {
        use skrifa::MetadataProvider;
        let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
        let glyph = |c| {
            font.charmap()
                .map(c)
                .or_else(|| {
                    (script == *b"kana" && c == 'ｶ')
                        .then(|| font.charmap().map('カ'))
                        .flatten()
                })
                .unwrap()
                .to_u32() as u16
        };
        let mut sets = std::collections::BTreeMap::<u16, Vec<Vec<u16>>>::new();
        for (text, target) in substitutions {
            let mut components = text.chars().map(glyph);
            let first = components.next().unwrap();
            let rest: Vec<_> = components.collect();
            let mut lig = vec![glyph(*target), rest.len() as u16 + 1];
            lig.extend(rest);
            sets.entry(first).or_default().push(lig);
        }
        let mut offsets = Vec::new();
        let mut data = Vec::new();
        let header = 3 + sets.len();
        for ligatures in sets.values() {
            offsets.push(((header + data.len()) * 2) as u16);
            let mut offset = 1 + ligatures.len();
            data.push(ligatures.len() as u16);
            for lig in ligatures {
                data.push((offset * 2) as u16);
                offset += lig.len();
            }
            for lig in ligatures {
                data.extend(lig);
            }
        }
        let mut subtable = vec![1, ((header + data.len()) * 2) as u16, sets.len() as u16];
        subtable.extend(offsets);
        subtable.extend(data);
        subtable.extend([1, sets.len() as u16]);
        subtable.extend(sets.keys());
        let mut gsub = Vec::new();
        for word in [1u16, 0, 10, 30, 44, 1] {
            gsub.extend(word.to_be_bytes());
        }
        gsub.extend(script);
        for word in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
            gsub.extend(word.to_be_bytes());
        }
        gsub.extend(b"liga");
        for word in [8u16, 0, 1, 0, 1, 4, 4, 0, 1, 8]
            .into_iter()
            .chain(subtable)
        {
            gsub.extend(word.to_be_bytes());
        }
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
            if tag == *b"cmap" && script == *b"kana" {
                // The subset has no halfwidth kana; give that source scalar
                // the existing fullwidth glyph before forcing their ligature.
                let mut chars: Vec<_> = substitutions
                    .iter()
                    .flat_map(|(s, c)| s.chars().chain(std::iter::once(*c)))
                    .collect();
                chars.sort_unstable();
                chars.dedup();
                let mut cmap = Vec::new();
                for word in [0u16, 1, 3, 10] {
                    cmap.extend(word.to_be_bytes());
                }
                cmap.extend(12u32.to_be_bytes());
                cmap.extend(12u16.to_be_bytes());
                cmap.extend(0u16.to_be_bytes());
                for word in [16 + 12 * chars.len() as u32, 0, chars.len() as u32] {
                    cmap.extend(word.to_be_bytes());
                }
                for ch in chars {
                    for word in [ch as u32, ch as u32, glyph(ch) as u32] {
                        cmap.extend(word.to_be_bytes());
                    }
                }
                tables.push((tag, cmap));
            } else if tag != *b"GSUB" {
                tables.push((tag, bytes[offset..offset + len].to_vec()));
            }
        }
        tables.push((*b"GSUB", gsub));
        tables.sort_by_key(|(tag, _)| *tag);
        let mut result = crate::font::sfnt::build_sfnt(&tables);
        result[..4].copy_from_slice(&bytes[..4]);
        result
    }

    #[test]
    fn mixed_width_kana_whole_cluster_uses_each_source_box_for_autospace() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::node::{InlineEdges, NodeId, TextSource};
        use crate::style::{FontFamily, OverflowWrap, ParagraphStyle, TextAutospace};
        use crate::{AtomicSizes, Fragment, LayoutContext, ParagraphBuilder};
        use skrifa::{
            MetadataProvider,
            instance::{LocationRef, Size},
        };
        let bytes = ligature_font_for(
            include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf"),
            *b"kana",
            &[("カｶ", '水')],
        );
        let font = skrifa::FontRef::from_index(&bytes, 0).unwrap();
        let metrics = font.glyph_metrics(Size::new(20.0), LocationRef::default());
        let water = font.charmap().map('水').unwrap();
        let natural = metrics.advance_width(water).unwrap();
        let hf = harfrust::FontRef::from_index(&bytes, 0).unwrap();
        let hd = harfrust::ShaperData::new(&hf);
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("カｶ");
        buffer.guess_segment_properties();
        let direct = hd.shaper(&hf).build().shape(buffer, Default::default());
        assert_eq!(
            direct.len(),
            1,
            "the oracle must force a shared GSUB cluster"
        );
        assert_eq!(direct.glyph_infos()[0].glyph_id, water.to_u32());
        let limits = Limits {
            max_reshape_window_bytes: Some(0),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                bytes,
                0,
                FontFaceDescriptor {
                    family: "Kana provenance".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named("Kana provenance".into())];
        style.root.font_size = 20.0;
        style.root.overflow_wrap = OverflowWrap::Anywhere;
        let mut child = style.root.clone();
        child.text_autospace = TextAutospace::NoAutospace;
        let mut builder = ParagraphBuilder::new(&style, &limits);
        for (node, text) in [(1, "カ"), (2, "ｶ")] {
            builder
                .open_inline(NodeId(node), &child, InlineEdges::default())
                .push_text(TextSource::Generated { node: NodeId(node) }, text)
                .close_inline();
        }
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        for width in [0.0, 100.0] {
            let lines = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                width,
                &AtomicSizes::EMPTY,
            );
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].text_range(), 0..6);
            assert!((lines[0].inline_size() - (natural + natural / 8.0)).abs() < 0.04);
            assert_eq!(
                lines[0]
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                        _ => None,
                    })
                    .sum::<usize>(),
                1
            );
            let child_width: f32 = lines[0]
                .fragments()
                .filter_map(|f| match f {
                    Fragment::InlineBox(b) => Some(b.rect.inline_size),
                    _ => None,
                })
                .sum();
            assert!(
                (child_width - natural).abs() < 0.04,
                "parent-owned spacing leaked into descendants: {child_width}"
            );
        }
    }

    fn expanded_hyphen_font(count: u16) -> Vec<u8> {
        use skrifa::MetadataProvider;
        let bytes = include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf");
        let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
        let dash = font.charmap().map('-').unwrap().to_u32() as u16;
        let w = font.charmap().map('W').unwrap().to_u32() as u16;
        let mut gsub = Vec::new();
        for word in [1u16, 0, 10, 30, 44, 1] {
            gsub.extend(word.to_be_bytes());
        }
        gsub.extend(b"latn");
        for word in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
            gsub.extend(word.to_be_bytes());
        }
        gsub.extend(b"ccmp");
        for word in [
            8u16,
            0,
            1,
            0,
            1,
            4,
            2,
            0,
            1,
            8,
            1,
            10 + 2 * count,
            1,
            8,
            count,
        ]
        .into_iter()
        .chain(std::iter::repeat_n(w, count as usize))
        .chain([1, 1, dash])
        {
            gsub.extend(word.to_be_bytes());
        }
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
            if tag != *b"GSUB" {
                tables.push((tag, bytes[offset..offset + len].to_vec()));
            }
        }
        tables.push((*b"GSUB", gsub));
        tables.sort_by_key(|(tag, _)| *tag);
        crate::font::sfnt::build_sfnt(&tables)
    }

    #[test]
    fn generated_hyphen_and_opposite_partial_edge_share_one_glyph_budget() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::{Limits, WarningKind};
        use crate::node::{NodeId, OutOfFlowKind, TextSource};
        use crate::style::{FontFamily, OverflowWrap, ParagraphStyle};
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let expanded = expanded_hyphen_font(9);
        let font = harfrust::FontRef::from_index(&expanded, 0).unwrap();
        let direct = harfrust::ShaperData::new(&font);
        let shaper = direct.shaper(&font).build();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("-");
        buffer.guess_segment_properties();
        assert_eq!(
            shaper.shape(buffer, Default::default()).len(),
            9,
            "real generated GSUB expansion"
        );
        for budget in [11, 12] {
            let limits = Limits {
                max_shaped_glyphs: Some(budget),
                ..Default::default()
            };
            let fonts = FontCollection::with_options(
                &limits,
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            fonts
                .register_face(
                    include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                    0,
                    FontFaceDescriptor {
                        family: "Letters".into(),
                        unicode_ranges: vec![(32, 32), (65, 90), (97, 122), (173, 173)],
                        ..Default::default()
                    },
                )
                .unwrap();
            fonts
                .register_face(
                    include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                    0,
                    FontFaceDescriptor {
                        family: "Other".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let dash = fonts
                .register_face(
                    expanded.clone(),
                    0,
                    FontFaceDescriptor {
                        family: "Dash".into(),
                        unicode_ranges: vec![(45, 45)],
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut style = ParagraphStyle::default();
            style.root.font_families = vec![
                FontFamily::Named("Letters".into()),
                FontFamily::Named("Dash".into()),
            ];
            style.root.overflow_wrap = OverflowWrap::Anywhere;
            let mut large = style.root.clone();
            large.font_size = 1000.0;
            let mut prefix = style.root.clone();
            prefix.font_families = vec![FontFamily::Named("Other".into())];
            let mut b = ParagraphBuilder::new(&style, &limits);
            b.open_inline(NodeId(5), &prefix, Default::default())
                .push_text(TextSource::Generated { node: NodeId(1) }, "x ffi ")
                .close_inline()
                .push_out_of_flow(NodeId(2), OutOfFlowKind::Float)
                .push_text(TextSource::Generated { node: NodeId(1) }, "T\u{ad}")
                .open_inline(NodeId(3), &large, Default::default())
                .push_text(TextSource::Generated { node: NodeId(4) }, "ZZZZ")
                .close_inline();
            let p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            assert_eq!(p.data.glyphs.len(), 10);
            let prefixes = p.break_all(
                &mut LayoutContext::new(),
                &Default::default(),
                1.0,
                &AtomicSizes::EMPTY,
            );
            let token = prefixes
                .iter()
                .find(|l| l.text_range().end == 3)
                .unwrap()
                .break_token();
            let mut warm = LayoutContext::new();
            let LineResult::FloatEncountered { float_cursor, .. } = p.next_line(
                &mut warm,
                token,
                &Default::default(),
                &LineConstraint::new(10000.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            let mut c = LineConstraint::new(200.0);
            c.floats_placed_through = Some(float_cursor);
            let LineResult::Line(line) = p.next_line(
                &mut warm,
                token,
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            let LineResult::Line(cold) = p.next_line(
                &mut LayoutContext::new(),
                token,
                &Default::default(),
                &c,
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            assert_eq!(line.text_range(), cold.text_range());
            assert_eq!(line.inline_size(), cold.inline_size());
            let runs: Vec<_> = line
                .fragments()
                .filter_map(|f| match f {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .collect();
            if budget == 11 {
                assert_eq!(line.text_range(), 3..6);
                assert!(runs.iter().all(|r| r.font() != dash));
                assert!(
                    warm.warnings
                        .as_slice()
                        .iter()
                        .any(|w| w.kind == WarningKind::Unsupported)
                );
            } else {
                assert_eq!(line.text_range(), 3..12);
                let generated = runs.iter().find(|r| r.font() == dash).unwrap();
                assert_eq!(generated.glyphs().len(), 9);
                assert_eq!(generated.clusters().next().unwrap().text_range, 10..12);
                let start = p.data.units.iter().position(|u| u.text.start == 3).unwrap();
                let end = p.data.units.iter().position(|u| u.text.end == 12).unwrap() + 1;
                let windows = crate::line::hyphen::line(
                    &p.data,
                    start,
                    end,
                    &mut LayoutContext::new(),
                    &mut crate::geometry::Saturation::default(),
                )
                .unwrap();
                assert_eq!(windows.len(), 2, "both distinct font edges retained");
                assert_eq!(
                    windows.iter().map(|w| w.overlay.store.len()).sum::<usize>(),
                    12,
                    "the safe prefix and generated tail share the limit"
                );
            }
            assert!(
                line.overlay
                    .as_ref()
                    .is_none_or(|g| g.len() as u64 <= budget)
            );
        }
    }

    #[test]
    fn unshapeable_interior_slice_is_skipped_without_duplicate_glyphs() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::node::{NodeId, TextSource};
        use crate::style::{FontFamily, LineOptions, OverflowWrap, ParagraphStyle, TextWrapStyle};
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let bytes = ligature_font(&[
            ("ffii", 'f'),
            ("ffi", 'W'),
            ("ff", 'W'),
            ("fii", 'W'),
            ("ii", 'f'),
        ]);
        let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
        let data = harfrust::ShaperData::new(&font);
        let shaper = data.shaper(&font).build();
        for (text, expected) in [
            ("ffii", 1),
            ("f", 1),
            ("ff", 1),
            ("ffi", 1),
            ("fii", 1),
            ("ii", 1),
            ("i", 1),
            ("fi", 2),
        ] {
            let mut buffer = harfrust::UnicodeBuffer::new();
            buffer.push_str(text);
            buffer.guess_segment_properties();
            assert_eq!(
                shaper.shape(buffer, Default::default()).len(),
                expected,
                "direct oracle {text}"
            );
        }
        let limits = Limits {
            max_shaped_glyphs: Some(1),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                bytes.clone(),
                0,
                FontFaceDescriptor {
                    family: "Budget".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named("Budget".into())];
        style.root.overflow_wrap = OverflowWrap::Anywhere;
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffii");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let wide = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        let width = wide[0].inline_size() + 0.01;
        let lines = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            width,
            &AtomicSizes::EMPTY,
        );
        assert_eq!(
            lines.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
            vec![0..1, 1..2, 2..4]
        );
        for wrap in [
            TextWrapStyle::Auto,
            TextWrapStyle::Balance,
            TextWrapStyle::Pretty,
        ] {
            let options = LineOptions {
                text_wrap_style: wrap,
                ..Default::default()
            };
            let mut cx = LayoutContext::new();
            let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
            let mut constraint = LineConstraint::new(width);
            constraint.break_plan = Some(&plan);
            let mut token = p.start_token();
            for expected in [0..1, 1..2, 2..4] {
                let LineResult::Line(line) =
                    p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY)
                else {
                    panic!("planned line unavailable");
                };
                assert_eq!(line.text_range(), expected);
                token = line.break_token();
            }
            assert!(matches!(
                p.next_line(&mut cx, token, &options, &constraint, &AtomicSizes::EMPTY),
                LineResult::Done
            ));
        }
        for l in lines {
            let range = l.text_range();
            let clusters: Vec<_> = l
                .fragments()
                .filter_map(|r| match r {
                    Fragment::GlyphRun(r) => Some(r),
                    _ => None,
                })
                .flat_map(|r| r.clusters().map(|c| c.text_range).collect::<Vec<_>>())
                .collect();
            assert_eq!(clusters, vec![range]);
            assert_eq!(
                l.fragments()
                    .filter_map(|r| match r {
                        Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                        _ => None,
                    })
                    .sum::<usize>(),
                1
            );
        }
    }

    #[test]
    fn unshapeable_ligature_remainder_retains_whole_source() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::node::{NodeId, TextSource};
        use crate::style::{FontFamily, LineOptions, OverflowWrap, ParagraphStyle, TextWrapStyle};
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        use skrifa::MetadataProvider;
        let bytes = ligature_font(&[("ffi", 'f'), ("ff", 'W')]);
        let font = skrifa::FontRef::from_index(&bytes, 0).unwrap();
        let f = font.charmap().map('f').unwrap().to_u32() as u16;
        let i = font.charmap().map('i').unwrap().to_u32() as u16;
        let w = font.charmap().map('W').unwrap().to_u32() as u16;
        // Every initial prefix fits one glyph; the remainder fi needs two.
        let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
        let direct = harfrust::ShaperData::new(&font);
        let shaper = direct.shaper(&font).build();
        for (text, ids) in [
            ("ffi", vec![f as u32]),
            ("ff", vec![w as u32]),
            ("fi", vec![f as u32, i as u32]),
        ] {
            let mut buffer = harfrust::UnicodeBuffer::new();
            buffer.push_str(text);
            buffer.guess_segment_properties();
            assert_eq!(
                shaper
                    .shape(buffer, Default::default())
                    .glyph_infos()
                    .iter()
                    .map(|g| g.glyph_id)
                    .collect::<Vec<_>>(),
                ids
            );
        }
        let limits = Limits {
            max_shaped_glyphs: Some(1),
            ..Default::default()
        };
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                bytes.clone(),
                0,
                FontFaceDescriptor {
                    family: "Budget".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![FontFamily::Named("Budget".into())];
        style.root.overflow_wrap = OverflowWrap::Anywhere;
        let mut builder = ParagraphBuilder::new(&style, &limits);
        builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
        let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
        let wide = p.break_all(
            &mut LayoutContext::new(),
            &Default::default(),
            1000.0,
            &AtomicSizes::EMPTY,
        );
        let width = wide[0].inline_size() + 0.01;
        for wrap in [
            TextWrapStyle::Auto,
            TextWrapStyle::Balance,
            TextWrapStyle::Pretty,
        ] {
            let options = LineOptions {
                text_wrap_style: wrap,
                ..Default::default()
            };
            let mut cx = LayoutContext::new();
            let plan = p.plan_breaks(&mut cx, &options, width, &AtomicSizes::EMPTY);
            let mut constraint = LineConstraint::new(width);
            constraint.break_plan = Some(&plan);
            let LineResult::Line(line) = p.next_line(
                &mut cx,
                p.start_token(),
                &options,
                &constraint,
                &AtomicSizes::EMPTY,
            ) else {
                panic!("planned line unavailable");
            };
            assert_eq!(line.text_range(), 0..3);
            assert!(matches!(
                p.next_line(
                    &mut cx,
                    line.break_token(),
                    &options,
                    &constraint,
                    &AtomicSizes::EMPTY
                ),
                LineResult::Done
            ));
            let lines = p.break_all(&mut cx, &options, width, &AtomicSizes::EMPTY);
            assert_eq!(
                lines.iter().map(|l| l.text_range()).collect::<Vec<_>>(),
                vec![0..3]
            );
            assert_eq!(
                lines
                    .iter()
                    .flat_map(|l| l.fragments())
                    .filter_map(|r| match r {
                        Fragment::GlyphRun(r) => Some(r.glyphs().len()),
                        _ => None,
                    })
                    .sum::<usize>(),
                1
            );
        }
    }

    #[test]
    fn combined_square_survives_an_adjacent_owned_ligature_window() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::geometry::WritingMode;
        use crate::node::{InlineEdges, NodeId, TextSource};
        use crate::style::{
            FontFamily, ParagraphStyle, TextAutospace, TextCombineUpright, WordBreak,
        };
        use crate::{
            AtomicSizes, Fragment, GlyphOrientation, LayoutContext, LineConstraint, LineResult,
            ParagraphBuilder,
        };
        let limits = Default::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        for (family, bytes) in [
            (
                "Latin",
                include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").as_slice(),
            ),
            (
                "Arabic",
                include_bytes!("../../dev/fixtures/assets/fonts/arabic.ttf").as_slice(),
            ),
        ] {
            fonts
                .register_face(
                    bytes.to_vec(),
                    0,
                    FontFaceDescriptor {
                        family: family.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            for (family, combined_text) in [("Arabic", "بب"), ("Latin", "ab")] {
                let mut root = ParagraphStyle {
                    writing_mode: mode,
                    ..Default::default()
                };
                root.root.font_families = vec![FontFamily::Named("Latin".into())];
                root.root.font_size = 16.0;
                root.root.text_autospace = TextAutospace::NoAutospace;
                root.root.word_break = WordBreak::BreakAll;
                let mut combined = root.root.clone();
                combined.font_families = vec![FontFamily::Named(family.into())];
                combined.text_combine_upright = TextCombineUpright::All;
                let make = |suffix| {
                    let mut b = ParagraphBuilder::new(&root, &limits);
                    b.open_inline(NodeId(1), &combined, InlineEdges::default());
                    b.push_text(
                        TextSource::Dom {
                            node: NodeId(2),
                            offset: 0,
                        },
                        combined_text,
                    );
                    b.close_inline();
                    if suffix {
                        b.push_text(
                            TextSource::Dom {
                                node: NodeId(3),
                                offset: 0,
                            },
                            "ffi",
                        );
                    }
                    b.build(&mut LayoutContext::new(), &fonts).unwrap()
                };
                let shape = |line: &crate::Line| {
                    line.fragments()
                        .filter_map(|f| match f {
                            Fragment::GlyphRun(run)
                                if run.orientation() == GlyphOrientation::Combined =>
                            {
                                Some((
                                    run.glyph_transform(),
                                    run.glyphs()
                                        .map(|g| {
                                            (
                                                g.id,
                                                g.advance,
                                                g.inline_position - run.inline_start(),
                                                g.block_offset,
                                            )
                                        })
                                        .collect::<Vec<_>>(),
                                ))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                let alone = make(false).break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    100.0,
                    &AtomicSizes::EMPTY,
                );
                assert_eq!(
                    alone[0].inline_size(),
                    16.0,
                    "an internal shaping edge cannot replace the square cost"
                );
                let p = make(true);
                let LineResult::Line(line) = p.next_line(
                    &mut LayoutContext::new(),
                    p.start_token(),
                    &Default::default(),
                    &LineConstraint::new(24.0),
                    &AtomicSizes::EMPTY,
                ) else {
                    panic!("line")
                };
                assert!(
                    line.overlay.is_some(),
                    "real ffi source cut must exercise an owned window"
                );
                assert!(
                    line.text_range().end > combined_text.len()
                        && line.text_range().end < combined_text.len() + 3,
                    "must cut inside neighboring ffi: {:?}",
                    line.text_range()
                );
                assert_eq!(line.text_combinations().count(), 1);
                assert_eq!(shape(&line), shape(&alone[0]));
            }
        }
    }

    #[test]
    fn adjacent_unsafe_edges_with_different_fonts_keep_both_glyphs() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::node::{NodeId, TextSource};
        use crate::style::{FontFamily, ParagraphStyle};
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let latin = fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let cjk = fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/cjk.otf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Cjk".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut style = ParagraphStyle::default();
        style.root.font_families = vec![
            FontFamily::Named("Latin".into()),
            FontFamily::Named("Cjk".into()),
        ];
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "z a日");
        let mut p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        let LineResult::Line(first) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let token = first.break_token();
        drop(first);
        let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
        data.units
            .iter_mut()
            .find(|u| u.text == (2..3))
            .unwrap()
            .unsafe_to_concat = true;
        data.units
            .iter_mut()
            .find(|u| u.text == (3..6))
            .unwrap()
            .unsafe_to_break = true;
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            token,
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let runs: Vec<_> = line
            .fragments()
            .filter_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .collect();
        assert_eq!(runs.iter().map(|r| r.glyphs().len()).sum::<usize>(), 2);
        assert_eq!(
            runs.iter().map(|r| r.font()).collect::<Vec<_>>(),
            vec![latin, cjk]
        );
        assert_eq!(
            runs.iter().map(|r| r.text_range()).collect::<Vec<_>>(),
            vec![2..3, 3..6]
        );
    }
}
