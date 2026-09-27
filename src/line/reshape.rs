use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::line::fragments::{GlyphSource, RecordKind};
use crate::shape::GlyphStore;
use crate::{LayoutContext, Line};

#[derive(Clone, Debug)]
pub(crate) struct EdgeOverlay {
    pub(crate) glyphs: std::ops::Range<u32>,
    pub(crate) text: std::ops::Range<u32>,
    pub(crate) store: GlyphStore,
    pub(crate) runs: Vec<crate::shape::ShapedRun>,
    pub(crate) hyphen: Option<std::ops::Range<u32>>,
}

/// Expose real grapheme opportunities inside a ligature without replacing
/// its shared glyphs. Prefix differences come from actual bounded shapes.
pub(crate) fn initialize_slices(
    data: &mut crate::paragraph::ParagraphData,
    cx: &mut LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) {
    use crate::analysis::units::{BreakClass, SharedCluster};
    let original = std::mem::take(&mut data.units);
    let mut units = Vec::with_capacity(original.len());
    let mut markers = Vec::new();
    for (i, unit) in original.iter().enumerate() {
        if !matches!(unit.kind, UnitKind::Cluster { .. }) {
            markers.push((i, unit.clone()));
        }
    }
    let mut offsets: Vec<_> = markers.iter().map(|(_, u)| u.text.start).collect();
    offsets.dedup();
    for (i, unit) in original.iter().enumerate() {
        let UnitKind::Cluster { glyphs, .. } = &unit.kind else {
            continue;
        };
        let begin = data
            .breaks
            .opportunities
            .partition_point(|o| o.offset <= unit.text.start);
        let end = data
            .breaks
            .opportunities
            .partition_point(|o| o.offset < unit.text.end);
        let mut boundaries: Vec<_> = data.breaks.opportunities[begin..end]
            .iter()
            .filter(|o| o.class != BreakClass::Prohibited)
            .copied()
            .collect();
        let marker_start = offsets.partition_point(|o| *o <= unit.text.start);
        let marker_end = offsets.partition_point(|o| *o < unit.text.end);
        for offset in &offsets[marker_start..marker_end] {
            boundaries.push(data.breaks.at(*offset));
        }
        boundaries.sort_unstable_by_key(|b| b.offset);
        boundaries.dedup_by_key(|b| b.offset);
        let storage_split = i > 0 && original[i - 1].text == unit.text
            || original
                .get(i + 1)
                .is_some_and(|next| next.text == unit.text);
        if boundaries.is_empty() || storage_split {
            units.push((i, unit.clone()));
            continue;
        }
        if data
            .limits
            .max_reshape_window_bytes
            .is_some_and(|max| u64::from(unit.text.end - unit.text.start) > max)
        {
            warnings.push(
                crate::limits::WarningKind::Unsupported,
                "intra-cluster reshape window exceeded; retaining whole cluster",
            );
            units.push((i, unit.clone()));
            continue;
        }
        let mut measured = Vec::with_capacity(boundaries.len());
        for boundary in &boundaries {
            let mut prefix = unit.clone();
            prefix.text.end = boundary.offset;
            let Some((store, _)) = crate::shape::shape_window(data, &prefix, cx, warnings, sat)
            else {
                break;
            };
            let width = store
                .advance
                .iter()
                .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat));
            // A break must leave a renderable remainder. Keeping only the
            // initial prefixes can accept a ligature slice whose suffix
            // expands past the glyph budget and then reuse the whole glyph.
            drop(store);
            let mut suffix = unit.clone();
            suffix.text.start = boundary.offset;
            if crate::shape::shape_window(data, &suffix, cx, warnings, sat).is_none() {
                break;
            }
            measured.push(width);
        }
        if measured.len() != boundaries.len() {
            units.push((i, unit.clone()));
            continue;
        }
        let shared = std::sync::Arc::new(SharedCluster {
            text: unit.text.clone(),
            glyphs: glyphs.clone(),
            units: 0..0,
            slices: Vec::new(),
        });
        let mut start = unit.text.start;
        let mut previous = LayoutUnit::ZERO;
        for (boundary, width) in boundaries.iter().zip(measured) {
            let mut slice = unit.clone();
            slice.shared_cluster = Some(std::sync::Arc::clone(&shared));
            slice.slice_advance = width.sub(previous, sat);
            slice.text = start..boundary.offset;
            slice.break_after = boundary.class;
            slice.emergency_min_content = boundary.min_content;
            slice.unsafe_to_break = true;
            slice.unsafe_to_concat |= start > unit.text.start;
            units.push((i, slice));
            previous = width;
            start = boundary.offset;
        }
        let total = glyphs.clone().fold(LayoutUnit::ZERO, |p, g| {
            p.add(data.glyphs.advance[g as usize], sat)
        });
        let mut last = unit.clone();
        last.shared_cluster = Some(shared);
        last.slice_advance = total.sub(previous, sat);
        last.text.start = start;
        last.unsafe_to_concat = true;
        units.push((i, last));
    }
    // Merge text slices and original markers in source order. A marker at
    // a continuation's start precedes that continuation, even though the
    // original ligature owner came from an earlier item.
    let mut merged = Vec::with_capacity(units.len() + markers.len());
    let mut markers = markers.into_iter().peekable();
    for (ordinal, unit) in units {
        while markers.peek().is_some_and(|(i, marker)| {
            marker.text.start < unit.text.start
                || marker.text.start == unit.text.start
                    && (*i < ordinal
                        || unit
                            .shared_cluster
                            .as_ref()
                            .is_some_and(|c| unit.text.start > c.text.start))
        }) {
            merged.push(markers.next().unwrap().1);
        }
        merged.push(unit);
    }
    merged.extend(markers.map(|(_, unit)| unit));
    // Rebuild group indices once. Window work visits only text slices,
    // so arbitrarily many zero-width markers do not multiply shaping work.
    let mut group: Vec<usize> = Vec::new();
    let finish = |group: &mut Vec<usize>, units: &mut Vec<crate::analysis::units::Unit>| {
        if group.is_empty() {
            return;
        }
        let previous = units[group[0]].shared_cluster.as_ref().unwrap();
        let shared = std::sync::Arc::new(SharedCluster {
            text: previous.text.clone(),
            glyphs: previous.glyphs.clone(),
            units: group[0]..group.last().unwrap() + 1,
            slices: group.clone(),
        });
        for i in group.drain(..) {
            units[i].shared_cluster = Some(std::sync::Arc::clone(&shared));
        }
    };
    let mut stack = Vec::new();
    for i in 0..merged.len() {
        match merged[i].kind {
            UnitKind::Open { box_index } => {
                merged[i].parent_box = stack.last().copied();
                stack.push(box_index);
            }
            UnitKind::Close { .. } => {
                stack.pop();
                merged[i].parent_box = stack.last().copied();
            }
            _ => merged[i].parent_box = stack.last().copied(),
        }
        if !matches!(merged[i].kind, UnitKind::Cluster { .. }) {
            continue;
        }
        if !group.is_empty() && !merged[group[0]].shares_cluster(&merged[i]) {
            finish(&mut group, &mut merged);
        }
        if merged[i].shared_cluster.is_some() {
            group.push(i);
        }
    }
    finish(&mut group, &mut merged);
    data.units = merged;
}

/// Materialize selected edge shapes before alignment reads their widths.
pub(super) fn prepare(
    data: &crate::paragraph::ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    if scan.prepared {
        return;
    }
    let windows = super::windows::measure(data, start, scan.end, cx, sat);
    apply_windows(data, start, scan, windows, cx, sat);
}

pub(super) fn apply_windows(
    data: &crate::paragraph::ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    windows: Vec<super::windows::Window>,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    scan.prepared = true;
    for window in windows {
        if scan.overlays.iter().any(|existing| {
            existing.glyphs.start < window.overlay.glyphs.end
                && window.overlay.glyphs.start < existing.glyphs.end
        }) {
            continue;
        }
        let count = scan
            .overlays
            .iter()
            .map(|w| w.store.len() as u64)
            .sum::<u64>()
            + window.overlay.store.len() as u64;
        if data.limits.max_shaped_glyphs.is_some_and(|max| count > max) {
            cx.warnings.push(
                crate::limits::WarningKind::Unsupported,
                "aggregate line edge glyph budget exceeded; keeping shared glyphs",
            );
            continue;
        }
        for (index, _, new) in window.changes {
            let old = scan.widths[index - start];
            scan.widths[index - start] = new;
            if index < scan.hang_start {
                scan.content = scan.content.sub(old, sat).add(new, sat);
            }
        }
        scan.overlays.push(window.overlay);
    }
}

struct OwnedPart {
    glyphs: std::ops::Range<u32>,
    clusters: std::ops::Range<u32>,
    run: u32,
}
struct SourceWindow {
    shared: std::ops::Range<u32>,
    text: std::ops::Range<u32>,
    glyphs: std::ops::Range<u32>,
    parts: Vec<OwnedPart>,
}

fn shared_pen(
    line: &Line,
    record: &crate::line::fragments::FragmentRecord,
    sources: &[SourceWindow],
    overlay: &GlyphStore,
    g: u32,
    sat: &mut Saturation,
) -> LayoutUnit {
    let RecordKind::Glyphs { glyphs, text, .. } = &record.kind else {
        unreachable!()
    };
    if g == glyphs.end {
        return record.inline_size;
    }
    if let Some((start, positions)) = &line.positions {
        return positions[(g - start) as usize] - positions[(glyphs.start - start) as usize];
    }
    let mut p = line.data.glyphs.pen[g as usize] - line.data.glyphs.pen[glyphs.start as usize];
    for window in sources {
        let begin = window.shared.start.max(glyphs.start);
        let end = window.shared.end.min(g);
        if begin >= end {
            continue;
        }
        let from = line.data.glyphs.cluster[begin as usize]
            .max(text.start)
            .max(window.text.start);
        let to = line.data.glyphs.cluster[end as usize].min(text.end);
        let actual = &overlay.cluster[window.glyphs.start as usize..window.glyphs.end as usize];
        let a = actual.partition_point(|c| *c < from) + window.glyphs.start as usize;
        let b = actual.partition_point(|c| *c < to) + window.glyphs.start as usize;
        let new = (a..b).fold(LayoutUnit::ZERO, |p, g| {
            p.add(overlay.advance[g], sat).add(
                overlay.spacing.as_ref().map_or(LayoutUnit::ZERO, |s| s[g]),
                sat,
            )
        });
        let old = line.data.glyphs.advance[begin as usize..end as usize]
            .iter()
            .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat));
        p = p.add(new.sub(old, sat), sat);
    }
    p
}

pub(super) fn apply(line: &mut Line, _cx: &mut LayoutContext, sat: &mut Saturation) {
    let windows = std::mem::take(&mut line.pending_overlays);
    if windows.is_empty() {
        return;
    }
    let mut overlay = GlyphStore::default();
    let mut sources = Vec::new();
    let mut overlay_clusters = Vec::new();
    let mut overlay_runs = Vec::new();
    for window in windows {
        let start = overlay.len() as u32;
        let mut parts = Vec::new();
        for mut run in window.runs {
            let cluster_start = overlay_clusters.len() as u32;
            let mut g = run.glyphs.start as usize;
            while g < run.glyphs.end as usize {
                let mut end = g + 1;
                while end < run.glyphs.end as usize
                    && window.store.cluster[end] == window.store.cluster[g]
                {
                    end += 1;
                }
                overlay_clusters.push(crate::output::OverlayCluster {
                    glyphs: start + g as u32..start + end as u32,
                    text: window.store.cluster[g]..if end < run.glyphs.end as usize {
                        window.store.cluster[end]
                    } else {
                        run.text.end
                    },
                });
                g = end;
            }
            run.glyphs = start + run.glyphs.start..start + run.glyphs.end;
            let index = overlay_runs.len() as u32;
            parts.push(OwnedPart {
                glyphs: run.glyphs.clone(),
                clusters: cluster_start..overlay_clusters.len() as u32,
                run: index,
            });
            overlay_runs.push(run);
        }
        if window.store.spacing.is_some() || overlay.spacing.is_some() {
            let spacing = overlay
                .spacing
                .get_or_insert_with(|| vec![LayoutUnit::ZERO; start as usize]);
            spacing.extend(
                window
                    .store
                    .spacing
                    .unwrap_or_else(|| vec![LayoutUnit::ZERO; window.store.id.len()]),
            );
        }
        overlay.flags.extend(window.store.flags);
        overlay.id.extend(window.store.id);
        overlay.advance.extend(window.store.advance);
        overlay.pen.extend(window.store.pen);
        overlay.offset_inline.extend(window.store.offset_inline);
        overlay.offset_block.extend(window.store.offset_block);
        overlay.cluster.extend(window.store.cluster);
        sources.push(SourceWindow {
            shared: window.glyphs,
            text: window.text,
            glyphs: start..overlay.len() as u32,
            parts,
        });
    }
    let mut records = Vec::new();
    let mut shifts = Vec::new();
    let mut index_map = Vec::new();
    for (index, record) in line.fragments.iter().enumerate() {
        index_map.push(records.len() as u32);
        let RecordKind::Glyphs { glyphs, text, .. } = &record.kind else {
            records.push(record.clone());
            shifts.push(line.block_shifts[index]);
            continue;
        };
        let mut cuts = vec![glyphs.start, glyphs.end];
        for window in &sources {
            if window.shared.start < glyphs.end && glyphs.start < window.shared.end {
                cuts.push(window.shared.start.max(glyphs.start));
                cuts.push(window.shared.end.min(glyphs.end));
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let reversed = record.level % 2 != line.data.base_level % 2;
        let mut parts = Vec::new();
        for pair in cuts.windows(2) {
            let (begin, end) = (pair[0], pair[1]);
            let left = shared_pen(line, record, &sources, &overlay, begin, sat);
            let right = shared_pen(line, record, &sources, &overlay, end, sat);
            let width = right.sub(left, sat);
            let from = line.data.glyphs.cluster[begin as usize].max(text.start);
            let to = if end == glyphs.end {
                text.end
            } else {
                line.data.glyphs.cluster[end as usize]
            };
            if let Some(window) = sources
                .iter()
                .find(|w| w.shared.start <= begin && end <= w.shared.end)
            {
                let mut selected = Vec::new();
                for owner in &window.parts {
                    let clusters =
                        &overlay.cluster[owner.glyphs.start as usize..owner.glyphs.end as usize];
                    let a = owner.glyphs.start + clusters.partition_point(|c| *c < from) as u32;
                    let b = owner.glyphs.start + clusters.partition_point(|c| *c < to) as u32;
                    if a < b {
                        selected.push((owner, a..b));
                    }
                }
                let mut consumed = LayoutUnit::ZERO;
                for (position, (owner, actual)) in selected.iter().enumerate() {
                    let natural = actual.clone().fold(LayoutUnit::ZERO, |p, g| {
                        let g = g as usize;
                        p.add(overlay.advance[g], sat).add(
                            overlay.spacing.as_ref().map_or(LayoutUnit::ZERO, |s| s[g]),
                            sat,
                        )
                    });
                    let advance = if position + 1 == selected.len() {
                        width.sub(consumed, sat)
                    } else {
                        natural
                    };
                    let mut part = record.clone();
                    part.inline_start = record.inline_start
                        + if reversed {
                            record.inline_size - left - consumed - advance
                        } else {
                            left + consumed
                        };
                    part.inline_size = advance;
                    let c = &overlay_clusters
                        [owner.clusters.start as usize..owner.clusters.end as usize];
                    let ca = owner.clusters.start
                        + c.partition_point(|c| c.glyphs.end <= actual.start) as u32;
                    let cb = owner.clusters.start
                        + c.partition_point(|c| c.glyphs.start < actual.end) as u32;
                    if let RecordKind::Glyphs {
                        glyphs,
                        text,
                        source,
                        item,
                        ..
                    } = &mut part.kind
                    {
                        *glyphs = begin..end;
                        *text = overlay.cluster[actual.start as usize]
                            ..if actual.end == owner.glyphs.end {
                                overlay_runs[owner.run as usize].text.end
                            } else {
                                overlay.cluster[actual.end as usize]
                            };
                        *item = overlay_runs[owner.run as usize].item;
                        *source = GlyphSource::Overlay {
                            glyphs: (actual.start, actual.end),
                            clusters: (ca, cb),
                            run: Some(owner.run),
                        };
                    }
                    parts.push(part);
                    consumed = consumed.add(advance, sat);
                }
            } else {
                let mut part = record.clone();
                part.inline_start = record.inline_start
                    + if reversed {
                        record.inline_size - right
                    } else {
                        left
                    };
                part.inline_size = width;
                if let RecordKind::Glyphs {
                    glyphs,
                    text,
                    source,
                    ..
                } = &mut part.kind
                {
                    *glyphs = begin..end;
                    *text = from..to;
                    *source = GlyphSource::Shared;
                }
                parts.push(part);
            }
        }
        if reversed {
            parts.reverse();
        }
        shifts.extend(std::iter::repeat_n(line.block_shifts[index], parts.len()));
        records.extend(parts);
    }
    for record in &mut records {
        if let RecordKind::InlineBox {
            parent: Some(parent),
            ..
        } = &mut record.kind
        {
            *parent = index_map[*parent as usize];
        }
    }
    line.fragments = records;
    line.block_shifts = shifts;
    line.overlay_clusters = overlay_clusters.into_boxed_slice();
    line.overlay_runs = overlay_runs.into_boxed_slice();
    line.overlay = Some(Box::new(overlay));
}

#[cfg(test)]
mod tests {
    #[test]
    fn edge_overlay_exposes_changed_count_clusters_and_positions() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::style::{FontFamily, FontFeature, InlineStyle, ParagraphStyle};
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
        fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Latin".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "ffi",
        );
        let mut p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
        assert_eq!(p.data.glyphs.len(), 1);
        // Force a real edge shape with another GSUB result, isolating the
        // owned-source contract from the separate intra-cluster break work.
        let data = std::sync::Arc::get_mut(&mut p.data).unwrap();
        data.styles[0].font_features.push(FontFeature {
            tag: *b"liga",
            value: 0,
        });
        data.units[0].unsafe_to_break = true;
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let run = line
            .fragments()
            .find_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(run.glyphs().len(), 3);
        assert_eq!(run.clusters().len(), 3);
        assert_eq!(
            run.clusters().map(|c| c.text_range).collect::<Vec<_>>(),
            vec![0..1, 1..2, 2..3]
        );
        for (i, g) in run.glyphs().enumerate() {
            assert_eq!(run.glyphs().get(i), Some(g));
            assert_eq!(g.cluster, i as u32);
        }
        assert!(run.glyphs().get(3).is_none());
        let natural = run.glyphs().map(|g| g.advance).sum::<f32>();
        assert!((run.inline_size() - natural).abs() < 0.02);
        assert!((line.inline_size() - natural).abs() < 0.02);
        assert_eq!(p.data.glyphs.len(), 1);
        let original: Vec<_> = run.glyphs().collect();
        let options = crate::style::LineOptions {
            text_align: crate::style::TextAlign::JustifyAll,
            text_align_last: crate::style::TextAlignLast::Justify,
            text_justify: crate::style::TextJustify::InterCharacter,
            ..Default::default()
        };
        let LineResult::Line(justified) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &LineConstraint::new(100.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let run = justified
            .fragments()
            .find_map(|f| match f {
                Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        let extra = (100.0 - natural) / 2.0;
        assert!((justified.inline_size() - 100.0).abs() < 0.02);
        for (i, g) in run.glyphs().enumerate() {
            assert!(
                (g.inline_position - original[i].inline_position - extra * i as f32).abs() < 0.03
            );
            assert!(
                (g.advance - original[i].advance - if i < 2 { extra } else { 0.0 }).abs() < 0.03
            );
        }
        assert!((run.clusters().map(|c| c.shaping_advance).sum::<f32>() - natural).abs() < 0.02);
        let LineResult::Line(extreme) = p.next_line(
            &mut LayoutContext::new(),
            p.start_token(),
            &options,
            &LineConstraint::new(30_000_000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        assert!(
            extreme
                .overlay
                .as_ref()
                .unwrap()
                .pen
                .iter()
                .all(|p| p.raw().abs() <= crate::shape::RUN_PEN_LIMIT)
        );
    }

    #[test]
    fn changed_edge_width_moves_shared_neighbors() {
        use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
        use crate::limits::Limits;
        use crate::style::{FontFamily, InlineStyle, ParagraphStyle};
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
        fonts
            .register_face(
                include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = ParagraphStyle {
            root: InlineStyle {
                font_families: vec![FontFamily::Named("Latin".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "z AV x",
        );
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
        data.styles[0].font_kerning = crate::style::FontKerning::None;
        data.units
            .iter_mut()
            .find(|u| u.text.start == 2)
            .unwrap()
            .unsafe_to_concat = true;
        let LineResult::Line(line) = p.next_line(
            &mut LayoutContext::new(),
            token,
            &Default::default(),
            &LineConstraint::new(1000.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let actual = line
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r)
                } else {
                    None
                }
            })
            .flat_map(|r| r.glyphs())
            .collect::<Vec<_>>();
        let font = harfrust::FontRef::from_index(
            include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf"),
            0,
        )
        .unwrap();
        let d = harfrust::ShaperData::new(&font);
        let shaper = d.shaper(&font).build();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("AV x");
        buffer.guess_segment_properties();
        let features = [harfrust::Feature::new(harfrust::Tag::new(b"kern"), 0, ..)];
        let shaped = shaper.shape(
            buffer,
            harfrust::ShapeOptions::default().features(&features),
        );
        assert_eq!(actual.len(), shaped.len());
        let mut pen = 0.0;
        for ((g, info), pos) in actual
            .iter()
            .zip(shaped.glyph_infos())
            .zip(shaped.glyph_positions())
        {
            assert_eq!(g.id, info.glyph_id);
            assert_eq!(g.cluster, info.cluster + 2);
            assert!(
                (g.inline_position - pen).abs() < 0.04,
                "cluster {}: {} vs {pen}",
                g.cluster,
                g.inline_position
            );
            pen +=
                (pos.x_advance as f32 * 16.0 / shaper.units_per_em() as f32 * 64.0).round() / 64.0;
        }
    }

    #[test]
    fn flagged_edges_use_owned_overlay_and_window_limit_falls_back() {
        use crate::limits::{Limits, WarningKind};
        use crate::style::{LineOptions, ParagraphStyle};
        use crate::{
            AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, ParagraphBuilder,
        };
        for budget in [0, 4096] {
            let limits = Limits {
                max_reshape_window_bytes: Some(budget),
                ..Limits::default()
            };
            let fonts = crate::font::FontCollection::new(&limits);
            let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
            b.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                "a b",
            );
            let mut p = b.build(&mut LayoutContext::new(), &fonts).unwrap();
            for u in &mut std::sync::Arc::get_mut(&mut p.data).unwrap().units {
                u.unsafe_to_break = true;
                u.unsafe_to_concat = true;
            }
            let mut cx = LayoutContext::new();
            let LineResult::Line(l) = p.next_line(
                &mut cx,
                p.start_token(),
                &LineOptions::default(),
                &LineConstraint::new(20.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            assert_eq!(l.overlay.is_some(), budget != 0);
            if budget == 0 {
                assert!(
                    cx.take_warnings()
                        .iter()
                        .any(|w| w.kind == WarningKind::Unsupported)
                );
            }
            let LineResult::Line(next) = p.next_line(
                &mut cx,
                l.break_token(),
                &LineOptions::default(),
                &LineConstraint::new(20.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            assert_eq!(next.overlay.is_some(), budget != 0);
            drop(p);
            drop(fonts);
            for line in [l, next] {
                let ids: Vec<_> = line
                    .fragments()
                    .filter_map(|f| match f {
                        Fragment::GlyphRun(r) => {
                            assert!(r.font_data().is_some());
                            Some(r.glyphs().map(|g| g.id).collect::<Vec<_>>())
                        }
                        _ => None,
                    })
                    .flatten()
                    .collect();
                assert_eq!(ids.len(), line.text_range().len());
            }
        }
    }
}
