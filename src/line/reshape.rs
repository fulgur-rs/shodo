use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::line::fragments::{GlyphSource, RecordKind};
use crate::shape::GlyphStore;
use crate::{LayoutContext, Line};

pub(super) fn apply(line: &mut Line, cx: &mut LayoutContext, sat: &mut Saturation) {
    let units = &line.data.units[line.units.start as usize..line.units.end as usize];
    let clusters: Vec<_> = units
        .iter()
        .filter(|u| matches!(u.kind, UnitKind::Cluster { .. }))
        .collect();
    let mut windows = Vec::new();
    for u in clusters.first().into_iter().chain(clusters.last()).copied() {
        let UnitKind::Cluster { glyphs, .. } = &u.kind else {
            unreachable!()
        };
        let previous_unsafe = line
            .units
            .start
            .checked_sub(1)
            .is_some_and(|i| line.data.units[i as usize].unsafe_to_break);
        let first = clusters
            .first()
            .is_some_and(|first| std::ptr::eq(*first, u));
        let last = clusters.last().is_some_and(|last| std::ptr::eq(*last, u));
        if !(first && line.units.start > 0 && (u.unsafe_to_concat || previous_unsafe)
            || last && u.unsafe_to_break)
            || windows.iter().any(|(range, _)| range == glyphs)
        {
            continue;
        }
        if let Some(store) = crate::shape::shape_line_edge(&line.data, u, cx, sat) {
            windows.push((glyphs.clone(), store));
        }
    }
    if windows.is_empty() {
        return;
    }
    let mut overlay = GlyphStore::default();
    let mut sources = Vec::new();
    for (range, store) in windows {
        let start = overlay.len() as u32;
        overlay.flags.extend(store.flags);
        overlay.id.extend(store.id);
        overlay.advance.extend(store.advance);
        overlay.pen.extend(store.pen);
        overlay.offset_inline.extend(store.offset_inline);
        overlay.offset_block.extend(store.offset_block);
        overlay.cluster.extend(store.cluster);
        sources.push((range, start));
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
        for (range, _) in &sources {
            if range.start >= glyphs.start && range.end <= glyphs.end {
                cuts.push(range.start);
                cuts.push(range.end);
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let pen = |g: u32| -> LayoutUnit {
            if g == glyphs.end {
                record.inline_size
            } else if let Some((start, positions)) = &line.positions {
                positions[(g - start) as usize] - positions[(glyphs.start - start) as usize]
            } else {
                line.data.glyphs.pen[g as usize] - line.data.glyphs.pen[glyphs.start as usize]
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
                    .find(|(range, _)| range.start == begin && range.end == end)
                    .map_or(GlyphSource::Shared, |(_, start)| GlyphSource::Overlay {
                        start: *start,
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
    line.overlay = Some(Box::new(overlay));
}

#[cfg(test)]
mod tests {
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
