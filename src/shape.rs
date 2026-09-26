//! Shaping into structure-of-arrays glyph storage.
//!
//! Harfrust shapes matched fonts; absent fonts use deterministic .notdef
//! advances. Cluster groups are kept in logical order, with intra-cluster
//! offsets normalized so public output applies bidi reversal once.

pub(crate) mod cache;

use std::ops::Range;

use crate::font::FontId;
use crate::geometry::{LayoutUnit, Saturation};
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
    pub(crate) font: FontId,
    pub(crate) font_size: f32,
}

/// Shapes one compatible style/font/script segment, then assigns each cluster
/// to the item supplying its first scalar. Node boundaries do not lose GSUB.
#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_items(
    cx: &mut crate::LayoutContext,
    items: &[crate::analysis::itemize::ShapeItem],
    styles: &[crate::style::InlineStyle],
    fonts: &crate::font::FontCollection,
    limits: &Limits,
    warnings: &mut crate::limits::WarningSink,
    sat: &mut Saturation,
) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
    let mut store = GlyphStore::default();
    let mut runs: Vec<ShapedRun> = Vec::new();
    for original in items {
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
                scalars: original.scalars[start..cursor].to_vec(),
                end: last.offset + last.c.len_utf8() as u32,
                style: original.style,
                level: original.level,
                script: original.script,
                font: original.font.clone(),
                before: original.before.clone(),
                after: original.after.clone(),
            };
            let window_run_start = runs.len();
            let style = &styles[item.style as usize];
            let Some(found) = &item.font else {
                warnings.push(
                    crate::limits::WarningKind::Unsupported,
                    "missing font; using .notdef glyphs",
                );
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
                    let current = runs.pop().unwrap();
                    if let Some(previous) = runs.last_mut()
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
            let data = fonts
                .font_data(found.id)
                .expect("matched face remains retained");
            let font = harfrust::FontRef::from_index(data.data.as_ref(), data.index)
                .expect("registered face");
            let shared = fonts
                .shaper_data(found.id)
                .expect("registered shaping data");
            let variations = found
                .variations
                .iter()
                .chain(&style.font_variations)
                .map(|v| harfrust::Variation {
                    tag: harfrust::Tag::new(&v.tag),
                    value: v.value,
                });
            let instance = harfrust::ShaperInstance::from_variations(&font, variations);
            let shaper = shared.shaper(&font).instance(Some(&instance)).build();
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
            buffer.set_direction(if item.level % 2 == 1 {
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
            buffer.set_flags(harfrust::BufferFlags::PRODUCE_UNSAFE_TO_CONCAT);
            let features: Vec<_> = style
                .font_features
                .iter()
                .map(|f| harfrust::Feature::new(harfrust::Tag::new(&f.tag), f.value, ..))
                .collect();
            let plan = cx
                .plans
                .get(found.id, &shaper, &buffer, Some(&instance), &features);
            let shaped = shaper.shape(
                buffer,
                harfrust::ShapeOptions::default()
                    .features(&features)
                    .plan(Some(&plan)),
            );
            Limits::check(
                limits.max_shaped_glyphs,
                LimitKind::ShapedGlyphs,
                store.len() as u64 + shaped.len() as u64,
            )?;
            let scale = style.font_size / shaper.units_per_em() as f32;
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
                let cluster_end = if end < order.len() {
                    shaped.glyph_infos()[order[end]].cluster
                } else {
                    item.end
                };
                let run_start = store.len() as u32;
                let mut pen = LayoutUnit::ZERO;
                let cluster_advance = order[begin..end]
                    .iter()
                    .map(|i| {
                        LayoutUnit::from_f32_round(
                            shaped.glyph_positions()[*i].x_advance as f32 * scale,
                            sat,
                        )
                        .raw() as i64
                    })
                    .sum::<i64>();
                for index in &order[begin..end] {
                    let info = &shaped.glyph_infos()[*index];
                    let pos = &shaped.glyph_positions()[*index];
                    let advance = LayoutUnit::from_f32_round(pos.x_advance as f32 * scale, sat);
                    store.flags.push(
                        u8::from(info.unsafe_to_break()) | (u8::from(info.unsafe_to_concat()) << 1),
                    );
                    store.id.push(info.glyph_id);
                    store.cluster.push(cluster);
                    store.advance.push(advance);
                    store.pen.push(pen);
                    let offset = LayoutUnit::from_f32_round(pos.x_offset as f32 * scale, sat);
                    let offset = if item.level % 2 == 1 {
                        LayoutUnit::from_raw(
                            cluster_advance.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
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
                        -pos.y_offset as f32 * scale,
                        sat,
                    ));
                    pen = pen.add(advance, sat);
                }
                if runs.len() > window_run_start
                    && let Some(run) = runs.last_mut()
                    && run.item == owner
                    && run.font == found.id
                    && run.text.end == cluster
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
                        font: found.id,
                        font_size: style.font_size,
                    });
                }
                begin = end;
            }
            cx.scratch_bytes = cx
                .scratch_bytes
                .max(item.scalars.len().max(shaped.len()).saturating_mul(64));
            cx.scratch = Some(shaped.clear());
        }
    }
    Ok((store, runs))
}

pub(crate) fn is_mark(c: char) -> bool {
    ('\u{300}'..='\u{36F}').contains(&c)
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
        let advance = if mark { LayoutUnit::ZERO } else { em };
        let cluster = text_start + i as u32;
        let overflows = i64::from(pen.raw()) + i64::from(advance.raw()) > i64::from(RUN_PEN_LIMIT);
        if overflows && store.len() as u32 > run_glyphs {
            runs.push(ShapedRun {
                glyphs: run_glyphs..store.len() as u32,
                text: run_text..cluster,
                item,
                font,
                font_size,
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
            font,
            font_size,
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
    let crate::analysis::units::UnitKind::Cluster { run, glyphs, .. } = &unit.kind else {
        return None;
    };
    let text = &data.text[unit.text.start as usize..unit.text.end as usize];
    if data
        .limits
        .max_reshape_window_bytes
        .is_some_and(|max| text.len() as u64 > max)
    {
        cx.warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge reshape window exceeded; keeping shared glyphs",
        );
        return None;
    }
    let shaped = &data.runs[*run as usize];
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    let script_map = icu_properties::CodePointMapData::<icu_properties::props::Script>::new();
    let script = text
        .chars()
        .map(|c| script_map.get(c))
        .find(|s| {
            !matches!(
                *s,
                icu_properties::props::Script::Common
                    | icu_properties::props::Script::Inherited
                    | icu_properties::props::Script::Unknown
            )
        })
        .unwrap_or(icu_properties::props::Script::Latin);
    let script: icu_locale_core::subtags::Script = script.into();
    let paragraph = data.bidi_paragraph_at_text(unit.text.start);
    let para_start = paragraph.map_or(0, |p| p.text.start) as usize;
    let para_end = paragraph.map_or(data.text.len(), |p| p.text.end as usize);
    let before: String = data.text[para_start..unit.text.start as usize]
        .chars()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = data.text[unit.text.end as usize..para_end]
        .chars()
        .take(5)
        .collect();
    let item = crate::analysis::itemize::ShapeItem {
        scalars: text
            .char_indices()
            .map(|(i, c)| crate::analysis::itemize::Scalar {
                c,
                offset: unit.text.start + i as u32,
                item: unit.item,
                grapheme_start: i == 0,
            })
            .collect(),
        end: unit.text.end,
        style: data.items[unit.item as usize].style,
        level: unit.level,
        script: script.as_str().as_bytes().try_into().unwrap(),
        font: data
            .fonts
            .shaper_data(shaped.font)
            .map(|_| crate::font::FontMatch {
                id: shaped.font,
                variations: style.font_variations.clone(),
                embolden: false,
                skew: None,
            }),
        before,
        after,
    };
    let mut warnings = crate::limits::WarningSink::default();
    warnings.set_max(data.limits.max_warnings);
    let result = shape_items(
        cx,
        &[item],
        &data.styles,
        &data.fonts,
        &data.limits,
        &mut warnings,
        sat,
    );
    for w in warnings.take() {
        cx.warnings.push(w.kind, w.message);
    }
    let Ok((store, _)) = result else {
        cx.warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge glyph budget exceeded; keeping shared glyphs",
        );
        return None;
    };
    if store.len() != (glyphs.end - glyphs.start) as usize {
        cx.warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge glyph count changed; keeping shared glyphs until variable overlays",
        );
        return None;
    }

    Some(store)
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
}
