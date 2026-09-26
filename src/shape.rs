//! Shaping into structure-of-arrays glyph storage.
//!
//! This is a placeholder shaper: one glyph per character with a 1em advance
//! (like the Ahem test font). Combining marks U+0300–U+036F get a zero
//! advance and a −0.5em inline offset, so that glyph offsets are exercised
//! separately from pen positions.
//!
//! Runs are stored in logical order and reversed for display by
//! `GlyphRunView`; a real shaper that emits right-to-left runs in visual
//! order must not be reversed twice.

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
    let mut store = GlyphStore::default();
    let mut runs = Vec::new();
    let shaped = &data.runs[*run as usize];
    if shape_item(
        &mut store,
        &mut runs,
        text,
        unit.text.start,
        unit.item,
        shaped.font,
        shaped.font_size,
        &data.limits,
        sat,
    )
    .is_err()
        || store.len() != (glyphs.end - glyphs.start) as usize
    {
        cx.warnings.push(
            crate::limits::WarningKind::Unsupported,
            "line edge glyph budget exceeded; keeping shared glyphs",
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
}
