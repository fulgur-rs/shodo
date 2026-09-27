//! Shaping into structure-of-arrays glyph storage.
//!
//! Harfrust shapes matched fonts; absent fonts use deterministic .notdef
//! advances. Cluster groups are kept in logical order, with intra-cluster
//! offsets normalized so public output applies bidi reversal once.

pub(crate) mod cache;
mod features;
mod instance;
pub(crate) mod orientation;
use instance::RunInstance;
pub(crate) use instance::resolve as resolve_instance;
use std::sync::Arc;

use std::ops::Range;

use crate::font::FontId;
use crate::geometry::{LayoutUnit, Saturation, WritingMode};
use crate::limits::{LimitExceeded, LimitKind, Limits};

/// A run is closed before its pen position would exceed this value, so
/// differences between pen positions within a run never saturate.
pub(crate) const RUN_PEN_LIMIT: i32 = 1 << 30;

#[derive(Clone, Debug, Default)]
pub(crate) struct GlyphStore {
    pub(crate) id: Vec<u32>,
    pub(crate) advance: Vec<LayoutUnit>,
    /// Pen position before the glyph, relative to the start of its run.
    pub(crate) pen: Vec<LayoutUnit>,
    pub(crate) offset_inline: Vec<LayoutUnit>,
    pub(crate) offset_block: Vec<LayoutUnit>,
    /// Byte offset of the glyph's character in the processed text.
    pub(crate) cluster: Vec<u32>,
    /// Bits0/1: unsafe to break/concat before this shaping cluster.
    pub(crate) flags: Vec<u8>,
    /// Per-glyph layout spacing for tracked/word-spaced/justified owned windows.
    /// Shaping advances remain unchanged for public cluster measurements.
    pub(crate) spacing: Option<Vec<LayoutUnit>>,
    /// Logical leading space, kept separate from the pen shared by marks.
    pub(crate) leading: Option<Vec<LayoutUnit>>,
}

impl GlyphStore {
    pub(crate) fn len(&self) -> usize {
        self.id.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedRun {
    pub(crate) glyphs: Range<u32>,
    pub(crate) text: Range<u32>,
    pub(crate) item: u32,
    pub(crate) orientation: orientation::RunOrientation,
    pub(crate) font: FontId,
    pub(crate) font_size: f32,
    pub(crate) instance: Arc<RunInstance>,
}

/// Shapes one compatible style/font/script segment, then assigns each cluster
/// to the item supplying its first scalar. Node boundaries do not lose GSUB.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_items(
    cx: &mut crate::LayoutContext,
    items: &[crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    mode: WritingMode,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    cx.bound_shaping_scratch(limits);
    let mut store = GlyphStore::default();
    let mut runs: Vec<ShapedRun> = Vec::new();
    for original in items {
        let style = &styles[original.style as usize];
        let font_data = original.font.as_ref().map(|found| {
            fonts
                .font_data(found.id)
                .expect("matched face remains retained")
        });
        let resolved = original
            .font
            .as_ref()
            .zip(font_data.as_ref())
            .map(|(found, data)| {
                let (shaper, mut instance, size) = instance::resolve(
                    data.data.as_ref(),
                    data.index,
                    found,
                    style,
                    original.script,
                    warnings,
                );
                let metrics = fonts.metrics_with_coords(found.id, size, &instance.coords);
                let vertical_metrics = fonts.vertical_metrics(found.id, size, &instance.coords);
                Arc::get_mut(&mut instance).expect("new instance").metrics = metrics;
                Arc::get_mut(&mut instance)
                    .expect("new instance")
                    .vertical_metrics = vertical_metrics;
                Arc::get_mut(&mut instance).expect("new instance").features =
                    features::for_orientation(style, original.orientation);
                (shaper, instance, size)
            });
        let missing_instance = if resolved.is_none() {
            Some(Arc::new(RunInstance {
                script: original.script,
                language: style.lang.clone(),
                features: features::for_orientation(style, original.orientation),
                metrics: Some(fonts.metrics(fonts.primary_font(), style.font_size)),
                ..Default::default()
            }))
        } else {
            None
        };
        let mut cursor = 0;
        while cursor < original.scalars.len() {
            let start = cursor;
            let budget = limits.max_shaping_run_bytes.unwrap_or(u64::MAX);
            let mut bytes = 0;
            let mut boundary = start;
            while cursor < original.scalars.len() {
                if original.scalars[cursor].grapheme_start && cursor > start {
                    boundary = cursor;
                }
                let next = original.scalars[cursor].c.len_utf8() as u64;
                if bytes + next > budget && cursor > start {
                    if boundary > start {
                        cursor = boundary;
                    } else {
                        warnings.push(crate::limits::WarningKind::Unsupported,"giant grapheme exceeds shaping run budget; splitting at scalar boundary");
                    }
                    break;
                }
                bytes += next;
                cursor += 1;
                if next > budget {
                    warnings.push(
                        crate::limits::WarningKind::Unsupported,
                        "scalar exceeds shaping run budget; forcing grapheme progress",
                    );
                    break;
                }
            }
            let last = &original.scalars[cursor - 1];
            let item = crate::analysis::itemize::ShapeItem {
                segment: original.segment,
                scalars: original.scalars[start..cursor].to_vec(),
                end: last.offset + last.c.len_utf8() as u32,
                style: original.style,
                level: original.level,
                script: original.script,
                font: original.font.clone(),
                orientation: original.orientation,
                before: original.before.clone(),
                after: original.after.clone(),
            };
            let window_run_start = runs.len();
            let Some(found) = &item.font else {
                warnings.push(
                    crate::limits::WarningKind::Unsupported,
                    "missing font; using .notdef glyphs",
                );
                let run_instance = missing_instance.as_ref().expect("missing font instance");
                for scalar in &item.scalars {
                    shape_item(
                        &mut store,
                        &mut runs,
                        &scalar.c.to_string(),
                        scalar.offset,
                        scalar.item,
                        fonts.primary_font(),
                        style.font_size,
                        limits,
                        sat,
                    )?;
                    *store.id.last_mut().unwrap() = 0;
                    let mut current = runs.pop().unwrap();
                    current.instance = Arc::clone(run_instance);
                    current.orientation = item.orientation;
                    if runs.len() > window_run_start
                        && let Some(previous) = runs.last_mut()
                        && previous.item == current.item
                        && previous.font == current.font
                        && previous.text.end == current.text.start
                        && i64::from(store.pen[previous.glyphs.end as usize - 1].raw())
                            + i64::from(store.advance[previous.glyphs.end as usize - 1].raw())
                            + i64::from(store.advance[current.glyphs.start as usize].raw())
                            <= i64::from(RUN_PEN_LIMIT)
                    {
                        let pen = store.pen[previous.glyphs.end as usize - 1]
                            + store.advance[previous.glyphs.end as usize - 1];
                        for pos in &mut store.pen[current.glyphs.start as usize..] {
                            *pos = pos.add(pen, sat);
                        }
                        previous.glyphs.end = current.glyphs.end;
                        previous.text.end = current.text.end;
                    } else {
                        runs.push(current);
                    }
                }
                continue;
            };
            let data = font_data.as_ref().expect("matched font data");
            let font = harfrust::FontRef::from_index(data.data.as_ref(), data.index)
                .expect("registered face");
            let shared = fonts
                .shaper_data(found.id)
                .expect("registered shaping data");
            let (instance, run_instance, font_size) = resolved.as_ref().expect("matched instance");
            let font_size = *font_size;
            let shaper = shared.shaper(&font).instance(Some(instance)).build();
            let mut buffer = cx.scratch.take().unwrap_or_default();
            buffer.clear();
            for scalar in &item.scalars {
                buffer.add(scalar.c, scalar.offset);
            }
            let pre: String = original.scalars[start.saturating_sub(5)..start]
                .iter()
                .map(|s| s.c)
                .collect();
            let post: String = original.scalars[cursor..(cursor + 5).min(original.scalars.len())]
                .iter()
                .map(|s| s.c)
                .collect();
            buffer.set_pre_context(if start == 0 { &original.before } else { &pre });
            buffer.set_post_context(if cursor == original.scalars.len() {
                &original.after
            } else {
                &post
            });
            let upright = item.orientation == orientation::RunOrientation::Upright;
            buffer.set_direction(if upright {
                harfrust::Direction::TopToBottom
            } else if item.level % 2 == 1 {
                harfrust::Direction::RightToLeft
            } else {
                harfrust::Direction::LeftToRight
            });
            buffer.set_script(
                harfrust::Script::from_iso15924_tag(harfrust::Tag::new(&item.script))
                    .unwrap_or(harfrust::script::UNKNOWN),
            );
            if let Some(language) = style.lang.as_ref().and_then(|l| l.parse().ok()) {
                buffer.set_language(language);
            }
            let mut flags = harfrust::BufferFlags::PRODUCE_UNSAFE_TO_CONCAT;
            if !item.scalars[0].grapheme_start {
                flags |= harfrust::BufferFlags::DO_NOT_INSERT_DOTTED_CIRCLE;
            }
            buffer.set_flags(flags);
            let features = &run_instance.features;
            let plan = cx
                .plans
                .get(found.id, &shaper, &buffer, Some(instance), features);
            let shaped = shaper.shape(
                buffer,
                harfrust::ShapeOptions::default()
                    .features(features)
                    .plan(Some(&plan)),
            );
            Limits::check(
                limits.max_shaped_glyphs,
                LimitKind::ShapedGlyphs,
                store.len() as u64 + shaped.len() as u64,
            )?;
            let scale = font_size / shaper.units_per_em() as f32;
            // Sort clusters into logical order while preserving the shaper's
            // intra-cluster order. Public output positions handle RTL groups.
            let mut order: Vec<_> = (0..shaped.len()).collect();
            order.sort_by_key(|i| shaped.glyph_infos()[*i].cluster);
            let mut begin = 0;
            while begin < order.len() {
                let cluster = shaped.glyph_infos()[order[begin]].cluster;
                let mut end = begin + 1;
                while end < order.len() && shaped.glyph_infos()[order[end]].cluster == cluster {
                    end += 1;
                }
                let scalar_index = item
                    .scalars
                    .partition_point(|s| s.offset <= cluster)
                    .saturating_sub(1);
                let owner = item.scalars[scalar_index].item;
                let next_cluster = if end < order.len() {
                    shaped.glyph_infos()[order[end]].cluster
                } else {
                    item.end
                };
                // Transparent anchors do not belong to the preceding cluster.
                // A cluster spanning an anchor still covers all its real scalars,
                // while a gap between clusters ends at the last actual scalar.
                let scalar_end = item.scalars.partition_point(|s| s.offset < next_cluster);
                let last_scalar = &item.scalars[scalar_end.saturating_sub(1)];
                let cluster_end = last_scalar.offset + last_scalar.c.len_utf8() as u32;
                let mut parts = Vec::new();
                let mut part_start = begin;
                let mut part_advance = 0i64;
                for (at, index) in order.iter().enumerate().take(end).skip(begin) {
                    let position = &shaped.glyph_positions()[*index];
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
                    if at > part_start && (part_advance + advance).abs() > i64::from(RUN_PEN_LIMIT)
                    {
                        parts.push((part_start..at, part_advance));
                        part_start = at;
                        part_advance = 0;
                    }
                    part_advance += advance;
                }
                parts.push((part_start..end, part_advance));
                if parts.len() > 1 || part_advance.abs() > i64::from(RUN_PEN_LIMIT) {
                    warnings.push(crate::limits::WarningKind::Unsupported,
                        "glyph cluster exceeds run pen budget; splitting storage without introducing a break");
                }
                // Splitting storage does not reverse a cluster's internal visual
                // order. Bidi reverses the chunk units, so store RTL chunks in
                // reverse order while keeping each chunk's glyph order intact.
                if item.level % 2 == 1 {
                    parts.reverse();
                }
                for (part, part_advance) in parts {
                    let run_start = store.len() as u32;
                    let mut pen = LayoutUnit::ZERO;
                    for index in &order[part.clone()] {
                        let info = &shaped.glyph_infos()[*index];
                        let pos = &shaped.glyph_positions()[*index];
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
                            u8::from(info.unsafe_to_break())
                                | (u8::from(info.unsafe_to_concat()) << 1),
                        );
                        store.id.push(info.glyph_id);
                        store.cluster.push(cluster);
                        store.advance.push(advance);
                        store.pen.push(pen);
                        let offset = LayoutUnit::from_f32_round(
                            if upright { -pos.y_offset } else { pos.x_offset } as f32 * scale,
                            sat,
                        );
                        let offset = if item.level % 2 == 1 {
                            LayoutUnit::from_raw(
                                part_advance.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
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
                    let previous_end_pen = runs.last().map_or(0, |r| {
                        i64::from(store.pen[r.glyphs.end as usize - 1].raw())
                            + i64::from(store.advance[r.glyphs.end as usize - 1].raw())
                    });
                    if runs.len() > window_run_start
                        && let Some(run) = runs.last_mut()
                        && run.item == owner
                        && run.font == found.id
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
                            glyphs: run_start..store.len() as u32,
                            text: cluster..cluster_end,
                            item: owner,
                            orientation: item.orientation,
                            font: found.id,
                            font_size,
                            instance: Arc::clone(run_instance),
                        });
                    }
                }
                begin = end;
            }
            cx.scratch_bytes = cx.scratch_bytes.max(
                item.scalars
                    .len()
                    .max(shaped.len())
                    .max(4)
                    .next_power_of_two()
                    .saturating_mul(128),
            );
            cx.scratch = Some(shaped.clear());
            cx.bound_shaping_scratch(limits);
        }
    }
    Ok((store, runs))
}

pub(crate) fn is_mark(c: char) -> bool {
    use icu_properties::{CodePointMapData, props::GeneralCategory};
    matches!(
        CodePointMapData::<GeneralCategory>::new().get(c),
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_item(
    store: &mut GlyphStore,
    runs: &mut Vec<ShapedRun>,
    text: &str,
    text_start: u32,
    item: u32,
    font: FontId,
    font_size: f32,
    limits: &Limits,
    sat: &mut Saturation,
) -> Result<(), LimitExceeded> {
    let em = LayoutUnit::from_f32_round(font_size, sat);
    let half_em = em.div_i32(2);
    let mut run_glyphs = store.len() as u32;
    let mut run_text = text_start;
    let mut pen = LayoutUnit::ZERO;
    for (i, c) in text.char_indices() {
        Limits::check(
            limits.max_shaped_glyphs,
            LimitKind::ShapedGlyphs,
            store.len() as u64 + 1,
        )?;
        let mark = is_mark(c);
        let advance = if mark
            || icu_properties::CodePointSetData::new::<
                icu_properties::props::DefaultIgnorableCodePoint,
            >()
            .contains(c)
        {
            LayoutUnit::ZERO
        } else {
            em
        };
        let cluster = text_start + i as u32;
        let overflows = i64::from(pen.raw()) + i64::from(advance.raw()) > i64::from(RUN_PEN_LIMIT);
        if overflows && store.len() as u32 > run_glyphs {
            runs.push(ShapedRun {
                glyphs: run_glyphs..store.len() as u32,
                text: run_text..cluster,
                item,
                orientation: orientation::RunOrientation::Horizontal,
                font,
                font_size,
                instance: Arc::new(RunInstance::default()),
            });
            run_glyphs = store.len() as u32;
            run_text = cluster;
            pen = LayoutUnit::ZERO;
        }
        store.flags.push(0);
        store.id.push(c as u32);
        store.advance.push(advance);
        store.pen.push(pen);
        store.offset_inline.push(if mark {
            LayoutUnit::ZERO - half_em
        } else {
            LayoutUnit::ZERO
        });
        store.offset_block.push(LayoutUnit::ZERO);
        store.cluster.push(cluster);
        pen = pen.add(advance, sat);
    }
    if store.len() as u32 > run_glyphs {
        let end = text_start + text.len() as u32;
        runs.push(ShapedRun {
            glyphs: run_glyphs..store.len() as u32,
            text: run_text..end,
            item,
            orientation: orientation::RunOrientation::Horizontal,
            font,
            font_size,
            instance: Arc::new(RunInstance::default()),
        });
    }
    Ok(())
}

pub(crate) fn shape_line_edge(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    cx: &mut crate::LayoutContext,
    sat: &mut Saturation,
) -> Option<GlyphStore> {
    let mut warnings = crate::limits::WarningSink::new(data.limits.max_warnings);
    let result = shape_window(data, unit, cx, &mut warnings, sat).map(|(store, _)| store);
    for warning in warnings.take() {
        cx.warnings.push(warning.kind, warning.message);
    }
    result
}

/// Shape only real scalars from the original itemization. Transparent controls
/// and anchors remain in source offsets, but are never submitted as glyphs.
pub(crate) fn shape_window(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    shape_window_budget(data, unit, data.limits.max_shaped_glyphs, cx, warnings, sat)
}

pub(crate) fn shape_window_budget(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    glyph_budget: Option<u64>,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    shape_window_edit(data, unit, glyph_budget, None, cx, warnings, sat)
}

pub(crate) struct Replacement {
    pub(crate) text: Range<u32>,
    pub(crate) c: char,
    pub(crate) font: Option<crate::font::FontMatch>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_window_edit(
    data: &crate::paragraph::ParagraphData,
    unit: &crate::analysis::units::Unit,
    glyph_budget: Option<u64>,
    replacement: Option<&Replacement>,
    cx: &mut crate::LayoutContext,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Option<(GlyphStore, Vec<ShapedRun>)> {
    let contains_replacement =
        replacement.is_some_and(|r| unit.text.start <= r.text.start && r.text.end <= unit.text.end);
    let synthetic_extra = replacement.filter(|_| contains_replacement).map_or(0, |r| {
        (r.c.len_utf8() as u64).saturating_sub(u64::from(r.text.end - r.text.start))
    });
    if data
        .limits
        .max_reshape_window_bytes
        .is_some_and(|max| u64::from(unit.text.end - unit.text.start) + synthetic_extra > max)
    {
        warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge reshape window exceeded; keeping shared glyphs",
        );
        return None;
    }
    let index = data
        .shape_items
        .partition_point(|item| item.end <= unit.text.start);
    let mut items: Vec<crate::analysis::itemize::ShapeItem> = Vec::new();
    for original in data.shape_items[index..]
        .iter()
        .take_while(|i| i.scalars.first().is_some_and(|s| s.offset < unit.text.end))
    {
        let begin = original
            .scalars
            .partition_point(|s| s.offset < unit.text.start);
        let end = original
            .scalars
            .partition_point(|s| s.offset < unit.text.end);
        if begin == end {
            continue;
        }
        // Font substitution can split an original item; newly identical
        // adjacent segments join before shaping so GPOS sees the hyphen.
        let mut at = begin;
        while at < end {
            let edited = replacement
                .filter(|r| contains_replacement && original.scalars[at].offset == r.text.start);
            let font = edited.map_or_else(|| original.font.clone(), |r| r.font.clone());
            let mut finish = at + 1;
            if edited.is_none() {
                while finish < end
                    && !replacement.is_some_and(|r| {
                        contains_replacement && original.scalars[finish].offset == r.text.start
                    })
                {
                    finish += 1;
                }
            }
            let mut before: Vec<_> = original
                .before
                .chars()
                .chain(original.scalars[..at].iter().map(|s| s.c))
                .rev()
                .take(5)
                .collect();
            before.reverse();
            let mut scalars = original.scalars[at..finish].to_vec();
            if let Some(r) = edited {
                scalars[0].c = r.c;
            }
            let part = crate::analysis::itemize::ShapeItem {
                segment: original.segment,
                end: edited.map_or_else(
                    || scalars.last().unwrap().offset + scalars.last().unwrap().c.len_utf8() as u32,
                    |r| r.text.end,
                ),
                scalars,
                style: original.style,
                level: original.level,
                script: original.script,
                font,
                orientation: original.orientation,
                before: before.into_iter().collect(),
                after: original.scalars[finish..]
                    .iter()
                    .map(|s| s.c)
                    .chain(original.after.chars())
                    .take(5)
                    .collect(),
            };
            if let Some(previous) = items.last_mut()
                && previous.segment == part.segment
                && previous.style == part.style
                && previous.level == part.level
                && previous.script == part.script
                && previous.font == part.font
                && previous.orientation == part.orientation
            {
                previous.scalars.extend(part.scalars);
                previous.end = part.end;
                previous.after = part.after;
            } else {
                items.push(part);
            }
            at = finish;
        }
    }
    let mut limits = data.limits.clone();
    limits.max_shaped_glyphs = glyph_budget;
    match shape_items(
        cx,
        &items,
        &data.styles,
        &data.fonts,
        data.style.writing_mode,
        &limits,
        warnings,
        sat,
    ) {
        Ok((store, mut runs)) => {
            if let Some(r) = replacement.filter(|_| contains_replacement) {
                for run in &mut runs {
                    if run.text.end == r.text.start + r.c.len_utf8() as u32 {
                        run.text.end = r.text.end;
                    }
                }
            }
            Some((store, runs))
        }
        Err(_) => {
            warnings.push(
                crate::limits::WarningKind::Unsupported,
                "line edge glyph budget exceeded; keeping shared glyphs",
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontCollection;
    use crate::geometry::LayoutUnit;
    use crate::limits::{LimitExceeded, LimitKind, Limits};

    fn shape(
        text: &str,
        size: f32,
        limits: &Limits,
    ) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
        let font = FontCollection::new(&Limits::default()).primary_font();
        let mut store = GlyphStore::default();
        let mut runs = Vec::new();
        let mut sat = Saturation::default();
        shape_item(
            &mut store, &mut runs, text, 0, 0, font, size, limits, &mut sat,
        )?;
        Ok((store, runs))
    }

    #[test]
    fn one_em_per_character() {
        let (g, runs) = shape("abc", 10.0, &Limits::default()).unwrap();
        assert_eq!(g.len(), 3);
        let px: Vec<f32> = g.pen.iter().map(|p| p.to_f32()).collect();
        assert_eq!(px, vec![0.0, 10.0, 20.0]);
        assert!(g.advance.iter().all(|a| a.to_f32() == 10.0));
        assert_eq!(g.cluster, vec![0, 1, 2]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, 0..3);
    }

    #[test]
    fn combining_marks_have_zero_advance_and_an_offset() {
        let (g, _) = shape("e\u{301}x", 10.0, &Limits::default()).unwrap();
        assert_eq!(g.advance[1], LayoutUnit::ZERO);
        assert_eq!(g.offset_inline[1].to_f32(), -5.0);
        assert_eq!(g.cluster[1], 1);
        assert_eq!(g.pen[2].to_f32(), 10.0);
    }

    #[test]
    fn pen_positions_restart_before_saturating() {
        // 1e6 px per glyph: 16 glyphs fit under 2^30 units (about 1.68e7 px).
        let text = "a".repeat(40);
        let (g, runs) = shape(&text, 1.0e6, &Limits::default()).unwrap();
        let sizes: Vec<u32> = runs.iter().map(|r| r.glyphs.end - r.glyphs.start).collect();
        assert_eq!(sizes, vec![16, 16, 8]);
        assert_eq!(g.pen[16], LayoutUnit::ZERO);
        // Differences inside a run stay exact.
        assert_eq!((g.pen[15] - g.pen[14]).to_f32(), 1.0e6);
        assert_eq!(runs[1].text, 16..32);
    }

    #[test]
    fn glyph_count_limit_is_checked_before_pushing() {
        let limits = Limits {
            max_shaped_glyphs: Some(2),
            ..Limits::default()
        };
        let err = shape("abc", 10.0, &limits).unwrap_err();
        assert_eq!(err.kind, LimitKind::ShapedGlyphs);
    }
    #[test]
    fn arabic_joining_retains_shaper_safety_flags() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                include_bytes!("../dev/fixtures/assets/fonts/arabic.ttf").to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Shodo Fixture Arabic".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named(
                    "Shodo Fixture Arabic".into(),
                )],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "السلام",
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert!(p.data.units.iter().any(|u| u.unsafe_to_break));
        assert!(p.data.units.iter().any(|u| u.unsafe_to_concat));
    }
    #[test]
    fn optical_sizing_and_explicit_variations_survive_public_views() {
        let bytes = include_bytes!("../dev/fixtures/assets/fonts/latin.ttf");
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            tables.push((
                bytes[at..at + 4].try_into().unwrap(),
                bytes[offset..offset + len].to_vec(),
            ));
        }
        let mut fvar = Vec::new();
        for field in [1u16, 0, 16, 2, 2, 20, 0, 8] {
            fvar.extend(field.to_be_bytes());
        }
        for (tag, values) in [(b"wght", [100i32, 400, 900]), (b"opsz", [8, 12, 72])] {
            fvar.extend(tag);
            for value in values {
                fvar.extend((value << 16).to_be_bytes());
            }
            fvar.extend([0, 0, 1, 0]);
        }
        tables.push((*b"fvar", fvar));
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::font::sfnt::build_sfnt(&tables),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Variable".into(),
                    weight: (100.0, 900.0),
                    ..Default::default()
                },
            )
            .unwrap();
        for (optical, explicit, want) in [
            (true, None, vec![1.0, 1.0]),
            (false, None, vec![1.0, 0.0]),
            (true, Some(8.0), vec![1.0, -1.0]),
        ] {
            let style = crate::style::ParagraphStyle {
                root: crate::style::InlineStyle {
                    font_size: 72.0,
                    font_families: vec![crate::style::FontFamily::Named("Variable".into())],
                    font_weight: 900.0,
                    font_optical_sizing: optical,
                    font_variations: explicit
                        .map(|value| crate::style::FontVariation {
                            tag: *b"opsz",
                            value,
                        })
                        .into_iter()
                        .collect(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut b = crate::ParagraphBuilder::new(&style, &limits);
            b.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                "a",
            );
            let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
            let crate::LineResult::Line(line) = p.next_line(
                &mut crate::LayoutContext::new(),
                p.start_token(),
                &Default::default(),
                &crate::LineConstraint::new(1000.0),
                &crate::AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            let run = line
                .fragments()
                .find_map(|f| {
                    if let crate::Fragment::GlyphRun(r) = f {
                        Some(r)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                run.normalized_coords()
                    .iter()
                    .map(|c| c.to_f32())
                    .collect::<Vec<_>>(),
                want
            );
            assert!(
                run.variations()
                    .iter()
                    .any(|v| v.tag == *b"wght" && v.value == 900.0)
            );
        }
    }
    #[test]
    fn real_font_pen_splits_before_prefix_overflow() {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                include_bytes!("../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 1e6,
                font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"W".repeat(32),
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert!(p.data.runs.len() > 1);
        assert!(
            p.data
                .glyphs
                .pen
                .iter()
                .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
        );
        assert_eq!(p.data.glyphs.len(), 32);
    }

    #[test]
    fn expanded_single_cluster_pen_splits_without_new_breaks() {
        use skrifa::MetadataProvider;
        let bytes = include_bytes!("../dev/fixtures/assets/fonts/latin.ttf");
        let font = skrifa::FontRef::from_index(bytes, 0).unwrap();
        let a = font.charmap().map('a').unwrap().to_u32() as u16;
        let w = font.charmap().map('W').unwrap().to_u32() as u16;
        // One ccmp MultipleSubst expands a single input cluster to32 glyphs.
        let mut gsub = Vec::new();
        for value in [1u16, 0, 10, 30, 44, 1] {
            gsub.extend(value.to_be_bytes());
        }
        gsub.extend(b"latn");
        for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
            gsub.extend(value.to_be_bytes());
        }
        gsub.extend(b"ccmp");
        for value in [8u16, 0, 1, 0, 1, 4, 2, 0, 1, 8, 1, 74, 1, 8, 32] {
            gsub.extend(value.to_be_bytes());
        }
        for _ in 0..32 {
            gsub.extend(w.to_be_bytes());
        }
        for value in [1u16, 1, a] {
            gsub.extend(value.to_be_bytes());
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
        let bytes = crate::font::sfnt::build_sfnt(&tables);
        let font = harfrust::FontRef::from_index(&bytes, 0).unwrap();
        let direct_data = harfrust::ShaperData::new(&font);
        let shaper = direct_data.shaper(&font).build();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str("a");
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, harfrust::ShapeOptions::default());
        assert_eq!(shaped.len(), 32, "direct test substitution must expand");
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::font::sfnt::build_sfnt(&tables),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Expansion".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 1e6,
                font_families: vec![crate::style::FontFamily::Named("Expansion".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert_eq!(p.data.glyphs.len(), 32, "test substitution must expand");
        assert!(
            p.data
                .glyphs
                .pen
                .iter()
                .all(|pen| pen.raw() <= RUN_PEN_LIMIT)
        );
        assert!(p.data.runs.len() > 1);
        assert!(
            p.data.units[..p.data.units.len() - 1]
                .iter()
                .all(|u| u.break_after == crate::analysis::units::BreakClass::Prohibited)
        );
    }

    #[test]
    fn negative_positioning_advances_obey_run_pen_budget() {
        use skrifa::MetadataProvider;
        let bytes = include_bytes!("../dev/fixtures/assets/fonts/latin.ttf");
        let glyph = skrifa::FontRef::from_index(bytes, 0)
            .unwrap()
            .charmap()
            .map('W')
            .unwrap()
            .to_u32() as u16;
        let mut gpos = Vec::new();
        for value in [1u16, 0, 10, 30, 44, 1] {
            gpos.extend(value.to_be_bytes());
        }
        gpos.extend(b"latn");
        for value in [8u16, 4, 0, 0, 0xffff, 1, 0, 1] {
            gpos.extend(value.to_be_bytes());
        }
        gpos.extend(b"kern");
        for value in [
            8u16,
            0,
            1,
            0,
            1,
            4,
            1,
            0,
            1,
            8,
            1,
            8,
            4,
            (-2000i16) as u16,
            1,
            1,
            glyph,
        ] {
            gpos.extend(value.to_be_bytes());
        }
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
            if tag != *b"GPOS" {
                tables.push((tag, bytes[offset..offset + len].to_vec()));
            }
        }
        tables.push((*b"GPOS", gpos));
        tables.sort_by_key(|(tag, _)| *tag);
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                crate::font::sfnt::build_sfnt(&tables),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Negative".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_size: 1e6,
                font_families: vec![crate::style::FontFamily::Named("Negative".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"W".repeat(64),
        );
        let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
        assert!(
            p.data.glyphs.advance.iter().all(|a| a.raw() < 0),
            "test positioning must make advances negative"
        );
        assert!(
            p.data
                .glyphs
                .pen
                .iter()
                .all(|p| i64::from(p.raw()).abs() <= i64::from(RUN_PEN_LIMIT))
        );
    }

    #[test]
    fn tiny_run_windows_share_one_resolved_instance() {
        for real in [false, true] {
            let limits = Limits {
                max_shaping_run_bytes: Some(1),
                ..Default::default()
            };
            let fonts = FontCollection::with_options(
                &limits,
                crate::font::FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            if real {
                fonts
                    .register_face(
                        include_bytes!("../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                        0,
                        crate::font::FontFaceDescriptor {
                            family: "Latin".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
            }
            let style = crate::style::ParagraphStyle {
                root: crate::style::InlineStyle {
                    font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                    font_features: vec![crate::style::FontFeature {
                        tag: *b"liga",
                        value: 0,
                    }],
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut b = crate::ParagraphBuilder::new(&style, &limits);
            b.push_text(
                crate::node::TextSource::Generated {
                    node: crate::node::NodeId(1),
                },
                &"a".repeat(32),
            );
            let p = b.build(&mut crate::LayoutContext::new(), &fonts).unwrap();
            assert_eq!(p.data.runs.len(), 32);
            assert!(
                p.data
                    .runs
                    .iter()
                    .all(|r| Arc::ptr_eq(&r.instance, &p.data.runs[0].instance)),
                "real={real}"
            );
        }
    }

    #[test]
    fn smaller_run_budget_automatically_releases_retained_scratch() {
        let limits = Limits::unlimited();
        let fonts = FontCollection::with_options(
            &limits,
            crate::font::FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                include_bytes!("../dev/fixtures/assets/fonts/latin.ttf").to_vec(),
                0,
                crate::font::FontFaceDescriptor {
                    family: "Latin".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let style = crate::style::ParagraphStyle {
            root: crate::style::InlineStyle {
                font_families: vec![crate::style::FontFamily::Named("Latin".into())],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cx = crate::LayoutContext::new();
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            &"a".repeat(4096),
        );
        b.build(&mut cx, &fonts).unwrap();
        assert!(cx.scratch_bytes > 1000);
        let limits = Limits {
            max_shaping_run_bytes: Some(8),
            ..Default::default()
        };
        let mut b = crate::ParagraphBuilder::new(&style, &limits);
        b.push_text(
            crate::node::TextSource::Generated {
                node: crate::node::NodeId(1),
            },
            "a",
        );
        b.build(&mut cx, &fonts).unwrap();
        assert!(cx.scratch_bytes <= 8 * 128);
    }
}
