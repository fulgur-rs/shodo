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
    pub(crate) metadata: Option<crate::shape::ShapedRun>,
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
    for (i, unit) in original.iter().enumerate() {
        let UnitKind::Cluster { glyphs, .. } = &unit.kind else {
            units.push(unit.clone());
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
        let boundaries: Vec<_> = data.breaks.opportunities[begin..end]
            .iter()
            .filter(|o| o.class != BreakClass::Prohibited)
            .copied()
            .collect();
        let storage_split = i > 0 && original[i - 1].text == unit.text
            || original
                .get(i + 1)
                .is_some_and(|next| next.text == unit.text);
        if boundaries.is_empty() || storage_split {
            units.push(unit.clone());
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
            units.push(unit.clone());
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
            measured.push(width);
        }
        if measured.len() != boundaries.len() {
            units.push(unit.clone());
            continue;
        }
        let shared = std::sync::Arc::new(SharedCluster {
            text: unit.text.clone(),
            glyphs: glyphs.clone(),
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
            units.push(slice);
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
        units.push(last);
    }
    data.units = units;
}

/// Materialize selected edge shapes before alignment reads their widths.
pub(super) fn prepare(
    data: &crate::paragraph::ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) {
    let clusters: Vec<_> = (start..scan.end)
        .filter(|i| matches!(data.units[*i].kind, UnitKind::Cluster { .. }))
        .collect();
    for at in clusters.first().into_iter().chain(clusters.last()).copied() {
        let u = &data.units[at];
        let UnitKind::Cluster { glyphs, .. } = &u.kind else {
            unreachable!()
        };
        if scan.overlays.iter().any(|w| &w.glyphs == glyphs) {
            continue;
        }
        let mut selected = vec![at];
        let mut window = u.clone();
        let partial = if let Some(shared) = &u.shared_cluster {
            selected = clusters
                .iter()
                .copied()
                .filter(|i| {
                    data.units[*i]
                        .shared_cluster
                        .as_ref()
                        .is_some_and(|other| std::sync::Arc::ptr_eq(other, shared))
                })
                .collect();
            window.text = data.units[*selected.first().unwrap()].text.start
                ..data.units[*selected.last().unwrap()].text.end;
            window.text != shared.text
        } else {
            false
        };
        let previous_unsafe = start
            .checked_sub(1)
            .is_some_and(|i| data.units[i].unsafe_to_break);
        let first = clusters.first() == Some(&at);
        let last = clusters.last() == Some(&at);
        if !(partial
            || first && start > 0 && (u.unsafe_to_concat || previous_unsafe)
            || last && u.unsafe_to_break)
        {
            continue;
        }
        if u.shared_cluster.is_none()
            && clusters
                .iter()
                .any(|other| *other != at && data.units[*other].text == u.text)
        {
            cx.warnings.push(
                crate::limits::WarningKind::Unsupported,
                "resource-split shaping cluster retained whole at line edge",
            );
            continue;
        }
        let mut warnings = crate::limits::WarningSink::new(data.limits.max_warnings);
        let shaped = crate::shape::shape_window(data, &window, cx, &mut warnings, sat);
        for warning in warnings.take() {
            cx.warnings.push(warning.kind, warning.message);
        }
        let Some((store, runs)) = shaped else {
            continue;
        };
        let count = scan
            .overlays
            .iter()
            .map(|w| w.store.len() as u64)
            .sum::<u64>()
            + store.len() as u64;
        if data.limits.max_shaped_glyphs.is_some_and(|max| count > max) {
            cx.warnings.push(
                crate::limits::WarningKind::Unsupported,
                "aggregate line edge glyph budget exceeded; keeping shared glyphs",
            );
            continue;
        }
        let width = store
            .advance
            .iter()
            .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat));
        let old = selected
            .iter()
            .fold(LayoutUnit::ZERO, |p, i| p.add(scan.widths[*i - start], sat));
        for i in &selected {
            scan.widths[*i - start] = LayoutUnit::ZERO;
        }
        scan.widths[selected[0] - start] = width;
        if selected[0] < scan.hang_start {
            scan.content = scan.content.sub(old, sat).add(width, sat);
        }
        scan.overlays.push(EdgeOverlay {
            glyphs: glyphs.clone(),
            text: window.text,
            store,
            metadata: if partial {
                runs.into_iter().next()
            } else {
                None
            },
        });
    }
}

struct SourceWindow {
    shared: std::ops::Range<u32>,
    text: std::ops::Range<u32>,
    glyphs: (u32, u32),
    clusters: (u32, u32),
    delta: LayoutUnit,
    run: Option<u32>,
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
    for mut window in windows {
        let original_width = window.glyphs.clone().fold(LayoutUnit::ZERO, |p, g| {
            p.add(line.data.glyphs.advance[g as usize], sat)
        });
        let shaped_width = window
            .store
            .advance
            .iter()
            .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat));
        let delta = shaped_width.sub(original_width, sat);
        let start = overlay.len() as u32;
        let cluster_start = overlay_clusters.len() as u32;
        let mut g = 0;
        while g < window.store.len() {
            let mut end = g + 1;
            while end < window.store.len() && window.store.cluster[end] == window.store.cluster[g] {
                end += 1;
            }
            overlay_clusters.push(crate::output::OverlayCluster {
                glyphs: start + g as u32..start + end as u32,
                text: window.store.cluster[g]..if end < window.store.len() {
                    window.store.cluster[end]
                } else {
                    window.text.end
                },
            });
            g = end;
        }
        overlay.flags.extend(window.store.flags);
        overlay.id.extend(window.store.id);
        overlay.advance.extend(window.store.advance);
        overlay.pen.extend(window.store.pen);
        overlay.offset_inline.extend(window.store.offset_inline);
        overlay.offset_block.extend(window.store.offset_block);
        overlay.cluster.extend(window.store.cluster);
        let run = window.metadata.take().map(|mut run| {
            run.glyphs = start..overlay.len() as u32;
            run.text = window.text.clone();
            let index = overlay_runs.len() as u32;
            overlay_runs.push(run);
            index
        });
        sources.push(SourceWindow {
            shared: window.glyphs,
            text: window.text,
            glyphs: (start, overlay.len() as u32),
            clusters: (cluster_start, overlay_clusters.len() as u32),
            delta,
            run,
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
        for w in &sources {
            if w.shared.start >= glyphs.start && w.shared.end <= glyphs.end {
                cuts.push(w.shared.start);
                cuts.push(w.shared.end);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let mut pen = |g: u32| -> LayoutUnit {
            if g == glyphs.end {
                record.inline_size
            } else if let Some((start, positions)) = &line.positions {
                positions[(g - start) as usize] - positions[(glyphs.start - start) as usize]
            } else {
                let mut p =
                    line.data.glyphs.pen[g as usize] - line.data.glyphs.pen[glyphs.start as usize];
                for w in &sources {
                    if w.shared.start >= glyphs.start && w.shared.end <= g {
                        p = p.add(w.delta, sat);
                    }
                }
                p
            }
        };
        let reversed = record.level % 2 != line.data.base_level % 2;
        let mut parts = Vec::new();
        for pair in cuts.windows(2) {
            let (begin, end) = (pair[0], pair[1]);
            let mut part = record.clone();
            let left = pen(begin);
            let right = pen(end);
            part.inline_start = record.inline_start
                + if reversed {
                    record.inline_size - right
                } else {
                    left
                };
            part.inline_size = right - left;
            if let RecordKind::Glyphs {
                glyphs: part_glyphs,
                text: part_text,
                source,
                item: part_item,
                ..
            } = &mut part.kind
            {
                *part_glyphs = begin..end;
                *part_text = line.data.glyphs.cluster[begin as usize]..if end == glyphs.end {
                    text.end
                } else {
                    line.data.glyphs.cluster[end as usize]
                };
                *source = sources
                    .iter()
                    .find(|w| w.shared.start == begin && w.shared.end == end)
                    .map_or(GlyphSource::Shared, |w| {
                        *part_text = w.text.clone();
                        if let Some(run) = w.run {
                            *part_item = overlay_runs[run as usize].item;
                        }
                        GlyphSource::Overlay {
                            glyphs: w.glyphs,
                            clusters: w.clusters,
                            run: w.run,
                        }
                    });
            }
            parts.push(part);
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
