//! Store a shaped window in logical cluster order. The index choice is
//! specialized once per window so glyph loops do not branch on a permutation.
use super::{GlyphStore, RUN_PEN_LIMIT, RunInstance, ShapedRun, cff_vertical_origin_delta};
use crate::analysis::itemize::{Scalar, ShapeItem};
use crate::font::FontId;
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::limits::WarningSink;
use std::{collections::HashMap, sync::Arc};

pub(super) struct GlyphWindow<'a, 'font> {
    pub(super) shaping_input: u32,
    pub(super) shaped: &'a harfrust::GlyphBuffer,
    pub(super) original: &'a ShapeItem,
    pub(super) scalars: &'a [Scalar],
    pub(super) font: FontId,
    pub(super) instance: &'a harfrust::ShaperInstance,
    pub(super) run_instance: &'a Arc<RunInstance>,
    pub(super) font_size: f32,
    pub(super) scale: f32,
    pub(super) mode: WritingMode,
    pub(super) upright: bool,
    pub(super) leading_end: u32,
    pub(super) window_end: u32,
    pub(super) window_run_start: usize,
    pub(super) cff_without_vorg: Option<&'a skrifa::FontRef<'font>>,
    pub(super) cff_origin_deltas: &'a mut HashMap<u32, Option<f32>>,
}

impl GlyphWindow<'_, '_> {
    pub(super) fn append(
        self,
        glyph_index: impl Fn(usize) -> usize,
        store: &mut GlyphStore,
        runs: &mut Vec<ShapedRun>,
        warnings: &mut WarningSink,
        sat: &mut Saturation,
    ) {
        let Self {
            shaping_input,
            shaped,
            original,
            scalars,
            font,
            instance,
            run_instance,
            font_size,
            scale,
            mode,
            upright,
            leading_end,
            window_end,
            window_run_start,
            cff_without_vorg,
            cff_origin_deltas,
        } = self;
        let output_cluster = |index: usize| shaped.glyph_infos()[index].cluster.max(leading_end);
        // A glyph-free prefix still owns text, grapheme limits and break
        // opportunities (including a standalone discretionary hyphen).
        let first_cluster = shaped
            .glyph_infos()
            .first()
            .map_or(window_end, |_| output_cluster(glyph_index(0)));
        let mut absent = 0;
        while absent < scalars.len() && scalars[absent].offset < first_cluster {
            let first = &scalars[absent];
            let mut finish = absent + 1;
            while finish < scalars.len()
                && scalars[finish].offset < first_cluster
                && !scalars[finish].grapheme_start
                && scalars[finish].item == first.item
            {
                finish += 1;
            }
            runs.push(ShapedRun {
                shaping_input,
                glyphs: store.len() as u32..store.len() as u32,
                text: first.offset..scalars[finish - 1].end,
                item: first.item,
                orientation: original.orientation,
                font,
                font_size,
                instance: Arc::clone(run_instance),
            });
            absent = finish;
        }
        let mut begin = 0;
        // The traversal is cluster-ascending, so the cluster value examined each
        // iteration only grows; `scalar_cursor` tracks the matching position
        // in `scalars` (offset-ascending) instead of re-searching the
        // whole slice with `partition_point` on every cluster.
        let mut scalar_cursor = 0usize;
        while begin < shaped.len() {
            let cluster = output_cluster(glyph_index(begin));
            let mut end = begin + 1;
            while end < shaped.len() && output_cluster(glyph_index(end)) == cluster {
                end += 1;
            }
            while scalar_cursor < scalars.len() && scalars[scalar_cursor].offset <= cluster {
                scalar_cursor += 1;
            }
            let scalar_index = scalar_cursor.saturating_sub(1);
            debug_assert_eq!(
                scalar_cursor,
                scalars.partition_point(|s| s.offset <= cluster),
                "scalar_cursor must track partition_point(|s| s.offset <= cluster); \
             scalars is not offset-ascending"
            );
            let owner = scalars[scalar_index].item;
            let next_cluster = if end < shaped.len() {
                output_cluster(glyph_index(end))
            } else {
                window_end
            };
            // Transparent anchors do not belong to the preceding cluster.
            // A cluster spanning an anchor still covers all its real scalars,
            // while a gap between clusters ends at the last actual scalar.
            while scalar_cursor < scalars.len() && scalars[scalar_cursor].offset < next_cluster {
                scalar_cursor += 1;
            }
            let scalar_end = scalar_cursor;
            debug_assert_eq!(
                scalar_end,
                scalars.partition_point(|s| s.offset < next_cluster),
                "scalar_cursor must track partition_point(|s| s.offset < next_cluster); \
             scalars is not offset-ascending"
            );
            let last_scalar = &scalars[scalar_end.saturating_sub(1)];
            let cluster_end = last_scalar.end;
            let mut parts = Vec::new();
            let mut part_start = begin;
            let mut part_advance = 0i64;
            for at in begin..end {
                let position = &shaped.glyph_positions()[glyph_index(at)];
                let advance = LayoutUnit::from_f32_round(
                    if upright {
                        -position.y_advance
                    } else {
                        position.x_advance
                    } as f32
                        * scale,
                    sat,
                )
                .raw() as i64;
                if at > part_start && (part_advance + advance).abs() > i64::from(RUN_PEN_LIMIT) {
                    parts.push((part_start..at, part_advance));
                    part_start = at;
                    part_advance = 0;
                }
                part_advance += advance;
            }
            // The usual unsplit cluster keeps its sole part on the stack.
            // Only a pen-budget split above allocates the fallback Vec.
            let single = if parts.is_empty() {
                Some((part_start..end, part_advance))
            } else {
                parts.push((part_start..end, part_advance));
                None
            };
            if parts.len() > 1 || part_advance.abs() > i64::from(RUN_PEN_LIMIT) {
                warnings.push(crate::limits::WarningKind::Unsupported,
                "glyph cluster exceeds run pen budget; splitting storage without introducing a break");
            }
            // Splitting storage does not reverse a cluster's internal visual
            // order. Bidi reverses the chunk units, so store RTL chunks in
            // reverse order while keeping each chunk's glyph order intact.
            if original.level % 2 == 1 {
                parts.reverse();
            }
            for (part, part_advance) in single.into_iter().chain(parts) {
                let run_start = store.len() as u32;
                let mut pen = LayoutUnit::ZERO;
                for at in part.clone() {
                    let index = glyph_index(at);
                    let info = &shaped.glyph_infos()[index];
                    let pos = &shaped.glyph_positions()[index];
                    let advance = LayoutUnit::from_f32_round(
                        if upright {
                            -pos.y_advance
                        } else {
                            pos.x_advance
                        } as f32
                            * scale,
                        sat,
                    );
                    store.flags.push(
                        u8::from(info.unsafe_to_break()) | (u8::from(info.unsafe_to_concat()) << 1),
                    );
                    store.id.push(info.glyph_id);
                    store.cluster.push(cluster);
                    store.advance.push(advance);
                    store.pen.push(pen);
                    let offset = LayoutUnit::from_f32_round(
                        (if upright { -pos.y_offset } else { pos.x_offset } as f32
                            + if upright {
                                cff_without_vorg
                                    .as_ref()
                                    .and_then(|font| {
                                        *cff_origin_deltas.entry(info.glyph_id).or_insert_with(
                                            || {
                                                cff_vertical_origin_delta(
                                                    font,
                                                    info.glyph_id,
                                                    instance.coords(),
                                                )
                                            },
                                        )
                                    })
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            })
                            * scale,
                        sat,
                    );
                    let offset = if original.level % 2 == 1 {
                        LayoutUnit::from_raw(
                            part_advance.clamp(i32::MIN as i64, i32::MAX as i64) as i32
                        )
                        .sub(pen, sat)
                        .sub(pen, sat)
                        .sub(advance, sat)
                        .sub(offset, sat)
                    } else {
                        offset
                    };
                    store.offset_inline.push(offset);
                    store.offset_block.push(LayoutUnit::from_f32_round(
                        if upright {
                            match mode {
                                WritingMode::VerticalLr => pos.x_offset,
                                _ => -pos.x_offset,
                            }
                        } else {
                            -pos.y_offset
                        } as f32
                            * scale,
                        sat,
                    ));
                    pen = pen.add(advance, sat);
                }
                let (part_min, part_max) = store.pen[run_start as usize..].iter().fold(
                    (part_advance.min(0), part_advance.max(0)),
                    |(min, max), p| (min.min(i64::from(p.raw())), max.max(i64::from(p.raw()))),
                );
                let previous_end_pen =
                    runs.last().filter(|r| !r.glyphs.is_empty()).map_or(0, |r| {
                        i64::from(store.pen[r.glyphs.end as usize - 1].raw())
                            + i64::from(store.advance[r.glyphs.end as usize - 1].raw())
                    });
                if runs.len() > window_run_start
                    && let Some(run) = runs.last_mut()
                    && !run.glyphs.is_empty()
                    && run.item == owner
                    && run.font == font
                    && run.text.end == cluster
                    && previous_end_pen + part_min >= -i64::from(RUN_PEN_LIMIT)
                    && previous_end_pen + part_max <= i64::from(RUN_PEN_LIMIT)
                {
                    let old_end = run.glyphs.end as usize;
                    let old_pen = store.pen[old_end - 1] + store.advance[old_end - 1];
                    for p in &mut store.pen[run_start as usize..] {
                        *p = p.add(old_pen, sat);
                    }
                    run.glyphs.end = store.len() as u32;
                    run.text.end = cluster_end;
                } else {
                    runs.push(ShapedRun {
                        shaping_input,
                        glyphs: run_start..store.len() as u32,
                        text: cluster..cluster_end,
                        item: owner,
                        orientation: original.orientation,
                        font,
                        font_size,
                        instance: Arc::clone(run_instance),
                    });
                }
            }
            begin = end;
        }
    }
}
