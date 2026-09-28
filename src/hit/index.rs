use super::{Caret, TextPosition};
use crate::geometry::LogicalRect;
use crate::mapping::Affinity;
use crate::{Fragment, GlyphRunView, Line};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct Segment {
    pub(super) text: Range<u32>,
    pub(super) from: f32,
    pub(super) to: f32,
    pub(super) rect: LogicalRect,
}
struct RawCluster {
    text: Range<u32>,
    from: f32,
    to: f32,
    rect: LogicalRect,
    natural: f32,
    carets: Option<Vec<f32>>,
    block_axis: bool,
}
struct CombineHit {
    rect: LogicalRect,
    stops: Vec<usize>,
}
pub(super) struct LineIndex {
    pub(super) stops: Vec<Caret>,
    pub(super) visual: Vec<usize>,
    pub(super) segments: Vec<Segment>,
    range: Range<u32>,
    spatial: super::spatial::Tree,
    combined: Vec<CombineHit>,
    combined_spatial: super::spatial::Tree,
}

pub(super) fn visual_key(caret: &Caret) -> (f32, f32) {
    (
        caret.rect.inline_start,
        if caret.rect.block_size == 0.0 && caret.rect.inline_size > 0.0 {
            caret.rect.block_start
        } else {
            0.0
        },
    )
}
impl LineIndex {
    pub(super) fn new(number: usize, line: &Line) -> Self {
        let range = line.text_range();
        let mut result = Self {
            stops: Vec::new(),
            visual: Vec::new(),
            segments: Vec::new(),
            range: range.start as u32..range.end as u32,
            spatial: super::spatial::Tree::new(std::iter::empty(), false),
            combined: Vec::new(),
            combined_spatial: super::spatial::Tree::new(std::iter::empty(), false),
        };
        let mut raw = Vec::<RawCluster>::new();
        for (record_index, fragment) in line.fragments().enumerate() {
            match fragment {
                Fragment::GlyphRun(run) => {
                    if run.orientation() == crate::GlyphOrientation::Combined {
                        let glyphs: Vec<_> = run.glyphs().collect();
                        for cluster in run.clusters() {
                            let g = line
                                .data
                                .glyphs
                                .cluster
                                .partition_point(|c| *c < cluster.text_range.start as u32);
                            let Some(paint) =
                                line.data.combine_geometry.glyphs.get(g).copied().flatten()
                            else {
                                continue;
                            };
                            let em = line.data.combine_spans[paint.span].em;
                            let sign = if line.data.style.writing_mode
                                == crate::geometry::WritingMode::VerticalRl
                            {
                                -1.0
                            } else {
                                1.0
                            };
                            let origin = line.block_offset() + run.baseline() - sign * em / 2.0;
                            let text =
                                cluster.text_range.start as u32..cluster.text_range.end as u32;
                            let cuts = result.cuts(line, &text);
                            let gs = &glyphs[glyphs.partition_point(|g| g.cluster < text.start)
                                ..glyphs.partition_point(|g| g.cluster < text.end)];
                            let scale = line.data.combine_geometry.scales[paint.span];
                            let carets = if gs.len() == 1 && cuts.len() > 2 {
                                ligature_carets(
                                    run,
                                    gs[0].id,
                                    cuts.len() - 2,
                                    cluster.shaping_advance,
                                )
                                .map(|carets| carets.into_iter().map(|v| v * scale).collect())
                            } else {
                                None
                            };
                            raw.push(RawCluster {
                                text,
                                from: origin + sign * paint.from,
                                to: origin + sign * paint.to,
                                rect: LogicalRect {
                                    inline_start: run.inline_start(),
                                    inline_size: em,
                                    block_start: 0.0,
                                    block_size: 0.0,
                                },
                                natural: cluster.shaping_advance * scale,
                                carets,
                                block_axis: true,
                            });
                        }
                        continue;
                    }
                    let reversed = run.bidi_level() % 2 != line.data.base_level % 2;
                    let metrics = run.metrics();
                    let vertical = matches!(
                        line.data.style.writing_mode,
                        crate::geometry::WritingMode::VerticalRl
                            | crate::geometry::WritingMode::VerticalLr
                    );
                    let (over, under) =
                        if vertical && run.orientation() == crate::GlyphOrientation::Upright {
                            run.vertical_metrics()
                                .map_or((run.font_size() / 2.0, run.font_size() / 2.0), |v| {
                                    (v.ascent, v.descent)
                                })
                        } else {
                            (metrics.ascent, metrics.descent)
                        };
                    let line_over_at_block_end =
                        line.data.style.writing_mode == crate::geometry::WritingMode::VerticalLr;
                    let rect = LogicalRect {
                        inline_start: 0.0,
                        inline_size: 0.0,
                        block_start: line.block_offset() + run.baseline()
                            - if line_over_at_block_end { under } else { over },
                        block_size: (over + under).max(0.0),
                    };
                    let glyphs: Vec<_> = run.glyphs().collect();
                    let mut pen = 0.0;
                    for cluster in run.clusters() {
                        let from = run.inline_start()
                            + if reversed {
                                run.inline_size() - pen
                            } else {
                                pen
                            };
                        pen += cluster.advance;
                        let to = run.inline_start()
                            + if reversed {
                                run.inline_size() - pen
                            } else {
                                pen
                            };
                        let text = cluster.text_range.start as u32..cluster.text_range.end as u32;
                        let (before, after) = line.ruby_caret_padding(&text);
                        let sign = if reversed { -1.0 } else { 1.0 };
                        let from = from + sign * before;
                        let to = to - sign * after;
                        let cuts = result.cuts(line, &text);
                        let gs = &glyphs[glyphs.partition_point(|g| g.cluster < text.start)
                            ..glyphs.partition_point(|g| g.cluster < text.end)];
                        let carets = if gs.len() == 1 && cuts.len() > 2 {
                            ligature_carets(run, gs[0].id, cuts.len() - 2, cluster.shaping_advance)
                        } else {
                            None
                        };
                        raw.push(RawCluster {
                            text,
                            from,
                            to,
                            rect,
                            carets,
                            natural: cluster.shaping_advance,
                            block_axis: false,
                        });
                    }
                }
                Fragment::Atomic(atomic) => {
                    // Atomics consume one processed replacement scalar.
                    if let crate::line::fragments::RecordKind::Atomic { unit, .. } =
                        line.fragments[record_index].kind
                    {
                        let unit = &line.data.units[unit as usize];
                        let reversed = unit.level % 2 != line.data.base_level % 2;
                        let mut rect = atomic.margin_rect;
                        // Signed margins reserve inline space, but the painted
                        // border box supplies the caret's vertical extent.
                        rect.block_start = atomic.border_rect.block_start + line.block_offset();
                        rect.block_size = atomic.border_rect.block_size.max(0.0);
                        let (from, to) = (rect.inline_start, rect.inline_start + rect.inline_size);
                        result.add(
                            number,
                            unit.text.clone(),
                            vec![unit.text.start, unit.text.end],
                            if reversed { to } else { from },
                            if reversed { from } else { to },
                            rect,
                            None,
                            rect.inline_size,
                            false,
                        );
                    }
                }
                _ => {}
            }
        }
        for tab in &line.tabs {
            let unit = &line.data.units[tab.unit as usize];
            let tabs = &line.data.combine_geometry.tabs;
            let index = tabs.partition_point(|(text, _)| text.end <= unit.text.start);
            if let Some((text, paint)) = tabs
                .get(index)
                .filter(|(text, _)| text.start == unit.text.start)
            {
                let span = &line.data.combine_spans[paint.span];
                let sign =
                    if line.data.style.writing_mode == crate::geometry::WritingMode::VerticalRl {
                        -1.0
                    } else {
                        1.0
                    };
                let square = line
                    .combination_at(unit.text.start)
                    .expect("accepted tab composition")
                    .square;
                let baseline = square.block_start + square.block_size / 2.0;
                let origin = line.block_offset() + baseline - sign * span.em / 2.0;
                raw.push(RawCluster {
                    text: text.clone(),
                    from: origin + sign * paint.from,
                    to: origin + sign * paint.to,
                    rect: LogicalRect {
                        inline_start: tab.start.to_f32(),
                        inline_size: span.em,
                        block_start: 0.0,
                        block_size: 0.0,
                    },
                    natural: (paint.to - paint.from).abs(),
                    carets: None,
                    block_axis: true,
                });
                continue;
            }
            let rect = LogicalRect {
                inline_start: tab.start.to_f32(),
                inline_size: tab.width.to_f32(),
                block_start: line.block_offset(),
                block_size: line.block_size(),
            };
            result.add(
                number,
                unit.text.clone(),
                vec![unit.text.start, unit.text.end],
                rect.inline_start,
                rect.inline_start + rect.inline_size,
                rect,
                None,
                rect.inline_size,
                false,
            );
        }
        raw.sort_by_key(|r| r.text.start);
        let mut combined: Vec<RawCluster> = Vec::new();
        for cluster in raw {
            if let Some(previous) = combined.last_mut()
                && previous.text.end == cluster.text.start
                && previous.block_axis == cluster.block_axis
                && line
                    .data
                    .breaks
                    .caret_cuts
                    .binary_search(&previous.text.end)
                    .is_err()
            {
                previous.text.end = cluster.text.end;
                previous.to = cluster.to;
                previous.natural += cluster.natural;
                previous.carets = None;
                let end = (previous.rect.block_start + previous.rect.block_size)
                    .max(cluster.rect.block_start + cluster.rect.block_size);
                previous.rect.block_start = previous.rect.block_start.min(cluster.rect.block_start);
                previous.rect.block_size = end - previous.rect.block_start;
            } else {
                combined.push(cluster);
            }
        }
        for cluster in combined {
            let cuts = result.cuts(line, &cluster.text);
            result.add(
                number,
                cluster.text,
                cuts,
                cluster.from,
                cluster.to,
                cluster.rect,
                cluster.carets.as_deref(),
                cluster.natural,
                cluster.block_axis,
            );
        }
        // Empty lines and nonpainting source at their ends still have stops.
        for (offset, affinity) in [
            (result.range.start, Affinity::Downstream),
            (result.range.end, Affinity::Upstream),
        ] {
            if !result.stops.iter().any(|s| s.position.offset == offset) {
                let nearest = result
                    .stops
                    .iter()
                    .min_by_key(|s| s.position.offset.abs_diff(offset));
                let rect = nearest.map_or(
                    LogicalRect {
                        inline_start: line
                            .fragments
                            .first()
                            .map_or(0.0, |r| r.inline_start.to_f32()),
                        inline_size: 0.0,
                        block_start: line.block_offset(),
                        block_size: line.block_size(),
                    },
                    |s| s.rect,
                );
                result.stops.push(Caret {
                    position: TextPosition {
                        line: number,
                        offset,
                        affinity,
                    },
                    rect,
                });
            }
        }
        result.stops.sort_by(|a, b| {
            a.position
                .offset
                .cmp(&b.position.offset)
                .then(affinity_key(a.position.affinity).cmp(&affinity_key(b.position.affinity)))
                .then(a.rect.inline_start.total_cmp(&b.rect.inline_start))
        });
        result
            .stops
            .dedup_by(|a, b| a.position == b.position && a.rect == b.rect);
        result.visual = (0..result.stops.len()).collect();
        result.visual.sort_by(|a, b| {
            let a_key = visual_key(&result.stops[*a]);
            let b_key = visual_key(&result.stops[*b]);
            a_key
                .0
                .total_cmp(&b_key.0)
                .then(a_key.1.total_cmp(&b_key.1))
                .then(
                    affinity_key(result.stops[*a].position.affinity)
                        .cmp(&affinity_key(result.stops[*b].position.affinity)),
                )
                .then(
                    result.stops[*a]
                        .position
                        .offset
                        .cmp(&result.stops[*b].position.offset),
                )
        });
        result
            .visual
            .dedup_by(|a, b| visual_key(&result.stops[*a]) == visual_key(&result.stops[*b]));
        for span in &line.data.combine_spans {
            let begin = result
                .stops
                .partition_point(|stop| stop.position.offset < span.text.start);
            let end = result
                .stops
                .partition_point(|stop| stop.position.offset <= span.text.end);
            let mut stops: Vec<_> = result.stops[begin..end]
                .iter()
                .enumerate()
                .filter_map(|(index, stop)| {
                    (stop.rect.block_size == 0.0 && stop.rect.inline_size > 0.0)
                        .then_some(begin + index)
                })
                .collect();
            stops.sort_by(|a, b| {
                result.stops[*a]
                    .rect
                    .block_start
                    .total_cmp(&result.stops[*b].rect.block_start)
            });
            stops.dedup_by(|a, b| {
                result.stops[*a].rect.block_start == result.stops[*b].rect.block_start
            });
            if let (Some(&first), Some(&last)) = (stops.first(), stops.last()) {
                let mut rect = result.stops[first].rect;
                let width = result.stops[last].rect.block_start - rect.block_start;
                rect.block_start -= (span.em - width) / 2.0;
                rect.block_size = span.em;
                result.combined.push(CombineHit { rect, stops });
            }
        }
        result.combined_spatial = super::spatial::Tree::new(
            result
                .combined
                .iter()
                .enumerate()
                .map(|(i, group)| (i, group.rect)),
            false,
        );
        result.spatial = super::spatial::Tree::new(
            result
                .segments
                .iter()
                .enumerate()
                .filter(|(_, s)| !s.text.is_empty() && s.from != s.to && s.rect.block_size > 0.0)
                .map(|(i, s)| (i, s.rect)),
            false,
        );
        result
    }
    fn cuts(&self, line: &Line, text: &Range<u32>) -> Vec<u32> {
        let all = &line.data.breaks.caret_cuts;
        let start = all.partition_point(|c| *c < text.start);
        let end = all.partition_point(|c| *c <= text.end);
        all[start..end].to_vec()
    }
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        line: usize,
        text: Range<u32>,
        cuts: Vec<u32>,
        from: f32,
        to: f32,
        rect: LogicalRect,
        gdef: Option<&[f32]>,
        natural: f32,
        block_axis: bool,
    ) {
        if cuts.len() < 2 {
            return;
        }
        let n = cuts.len() - 1;
        let mut previous = None;
        for (i, offset) in cuts.into_iter().enumerate() {
            let ratio = if i == 0 {
                0.0
            } else if i == n {
                1.0
            } else {
                gdef.and_then(|g| g.get(i - 1))
                    .filter(|_| natural > 0.0)
                    .map_or(i as f32 / n as f32, |v| *v / natural)
            };
            // Distribute layout expansion over the grapheme intervals while
            // retaining the font's natural caret coordinate when provided.
            let distance = if i > 0 && i < n {
                gdef.and_then(|g| g.get(i - 1))
                    .map_or((to - from) * ratio, |v| {
                        (to - from).signum()
                            * (*v + ((to - from).abs() - natural) * i as f32 / n as f32)
                    })
            } else {
                (to - from) * ratio
            };
            let x = from + distance;
            let caret_rect = if block_axis {
                LogicalRect {
                    block_start: x,
                    block_size: 0.0,
                    ..rect
                }
            } else {
                LogicalRect {
                    inline_start: x,
                    inline_size: 0.0,
                    ..rect
                }
            };
            if i < n {
                self.stops.push(Caret {
                    position: TextPosition {
                        line,
                        offset,
                        affinity: Affinity::Downstream,
                    },
                    rect: caret_rect,
                });
            }
            if i > 0 {
                self.stops.push(Caret {
                    position: TextPosition {
                        line,
                        offset,
                        affinity: Affinity::Upstream,
                    },
                    rect: caret_rect,
                });
            }
            if let Some((before, bx)) = previous {
                self.segments.push(Segment {
                    text: before..offset,
                    from: bx,
                    to: x,
                    rect: if block_axis {
                        LogicalRect {
                            block_start: bx.min(x),
                            block_size: (x - bx).abs(),
                            ..rect
                        }
                    } else {
                        LogicalRect {
                            inline_start: bx.min(x),
                            inline_size: (x - bx).abs(),
                            ..rect
                        }
                    },
                });
            }
            previous = Some((offset, x));
        }
        let _ = text;
    }
    pub(super) fn caret(&self, position: TextPosition) -> Option<Caret> {
        if position.offset < self.range.start || position.offset > self.range.end {
            return None;
        }
        let begin = self
            .stops
            .partition_point(|s| s.position.offset < position.offset);
        let end = self
            .stops
            .partition_point(|s| s.position.offset <= position.offset);
        let stop = if begin < end {
            self.stops[begin..end]
                .iter()
                .find(|s| s.position.affinity == position.affinity)
                .unwrap_or(&self.stops[begin])
        } else {
            match position.affinity {
                Affinity::Upstream => self.stops.get(begin.checked_sub(1)?)?,
                Affinity::Downstream => self.stops.get(begin)?,
            }
        };
        Some(*stop)
    }
    pub(super) fn hit(&self, inline: f32, block: f32) -> Option<&Caret> {
        if let Some(group) = self
            .combined_spatial
            .containing(inline, block)
            .map(|i| &self.combined[i])
        {
            let at = group
                .stops
                .partition_point(|i| self.stops[*i].rect.block_start < block);
            let a = at
                .checked_sub(1)
                .and_then(|i| group.stops.get(i))
                .map(|i| &self.stops[*i]);
            let b = group.stops.get(at).map(|i| &self.stops[*i]);
            return match (a, b) {
                (Some(a), Some(b)) => Some(
                    if block - a.rect.block_start <= b.rect.block_start - block {
                        a
                    } else {
                        b
                    },
                ),
                (a, b) => a.or(b),
            };
        }
        let at = self
            .visual
            .partition_point(|i| self.stops[*i].rect.inline_start < inline);
        let a = at
            .checked_sub(1)
            .and_then(|i| self.visual.get(i))
            .map(|i| &self.stops[*i]);
        let b = self.visual.get(at).map(|i| &self.stops[*i]);
        match (a, b) {
            (Some(a), Some(b)) => Some(
                if inline - a.rect.inline_start <= b.rect.inline_start - inline {
                    a
                } else {
                    b
                },
            ),
            (a, b) => a.or(b),
        }
    }
    pub(super) fn inside(&self, inline: f32, block: f32) -> bool {
        self.spatial.contains(inline, block)
    }
}
fn affinity_key(a: Affinity) -> u8 {
    match a {
        Affinity::Downstream => 0,
        Affinity::Upstream => 1,
    }
}

fn ligature_carets(
    run: GlyphRunView<'_>,
    glyph: u32,
    count: usize,
    natural: f32,
) -> Option<Vec<f32>> {
    use skrifa::raw::{
        TableProvider,
        tables::{gdef::CaretValue, layout::DeviceOrVariationIndex, variations::DeltaSetIndex},
        types::GlyphId,
    };
    let data = run.font_data()?;
    let font = skrifa::FontRef::from_index(data.data.as_ref(), data.index).ok()?;
    let gdef = font.gdef().ok()?;
    let list = gdef.lig_caret_list()?.ok()?;
    let index = list.coverage().ok()?.get(GlyphId::new(glyph))? as usize;
    let lig = list.lig_glyphs().get(index).ok()?;
    if lig.caret_count() as usize != count {
        return None;
    }
    let scale = run.font_size() / font.head().ok()?.units_per_em() as f32;
    let mut result = Vec::with_capacity(count);
    for value in lig.caret_values().iter() {
        let coordinate = match value.ok()? {
            CaretValue::Format1(v) => v.coordinate() as f32 * scale,
            CaretValue::Format2(_) => return None,
            CaretValue::Format3(v) => {
                let base = v.coordinate() as f32 * scale;
                if v.device_offset().is_null() {
                    base
                } else {
                    match v.device().ok()? {
                        DeviceOrVariationIndex::VariationIndex(v) => {
                            let delta = gdef
                                .item_var_store()?
                                .ok()?
                                .compute_delta(
                                    DeltaSetIndex {
                                        outer: v.delta_set_outer_index(),
                                        inner: v.delta_set_inner_index(),
                                    },
                                    run.normalized_coords(),
                                )
                                .ok()?;
                            base + delta as f32 * scale
                        }
                        DeviceOrVariationIndex::Device(v) => {
                            let ppem = run.font_size().round() as u16;
                            base + if ppem >= v.start_size() && ppem <= v.end_size() {
                                v.iter().nth((ppem - v.start_size()) as usize)? as f32
                            } else {
                                0.0
                            }
                        }
                    }
                }
            }
        };
        if !coordinate.is_finite()
            || coordinate <= 0.0
            || coordinate >= natural
            || result.last().is_some_and(|last| *last >= coordinate)
        {
            return None;
        }
        result.push(coordinate);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::node::{NodeId, TextSource};
    use crate::style::{FontFamily, FontVariation, InlineStyle, ParagraphStyle};
    use crate::{AtomicSizes, LayoutContext, ParagraphBuilder};
    fn synthetic_font(format: u16, bad: bool) -> Vec<u8> {
        let bytes = include_bytes!("../../dev/fixtures/assets/fonts/latin.ttf");
        let glyph = 367u16;
        let mut gdef = Vec::new();
        for word in [1u16, 3, 0, 0, 18, 0, 0] {
            gdef.extend(word.to_be_bytes());
        }
        let coverage = if format == 3 { 28u16 } else { 20 };
        gdef.extend((18u32 + coverage as u32 + 6).to_be_bytes());
        for word in [
            coverage,
            1,
            6,
            2,
            6,
            10,
            1,
            200,
            format,
            if bad { 30000 } else { 400 },
        ] {
            gdef.extend(word.to_be_bytes());
        }
        if format == 3 {
            for word in [6u16, 0, 0, 0x8000] {
                gdef.extend(word.to_be_bytes());
            }
        }
        for word in [1u16, 1, glyph] {
            gdef.extend(word.to_be_bytes());
        }
        gdef.extend(1u16.to_be_bytes());
        gdef.extend(12u32.to_be_bytes());
        gdef.extend(1u16.to_be_bytes());
        gdef.extend(22u32.to_be_bytes());
        for word in [1u16, 1, 0, 16384, 16384, 1, 1, 1, 0, 50] {
            gdef.extend(word.to_be_bytes());
        }
        let mut fvar = Vec::new();
        for word in [1u16, 0, 16, 2, 1, 20, 0, 8] {
            fvar.extend(word.to_be_bytes());
        }
        fvar.extend(b"wght");
        for v in [100i32, 400, 900] {
            fvar.extend((v << 16).to_be_bytes());
        }
        fvar.extend([0, 0, 1, 0]);
        let mut tables = Vec::new();
        for n in 0..u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize {
            let at = 12 + n * 16;
            let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
            if matches!(&tag, b"GDEF" | b"fvar") {
                continue;
            }
            let offset = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()) as usize;
            tables.push((tag, bytes[offset..offset + len].to_vec()));
        }
        tables.push((*b"GDEF", gdef));
        tables.push((*b"fvar", fvar));
        tables.sort_by_key(|v| v.0);
        crate::font::sfnt::build_sfnt(&tables)
    }
    #[test]
    fn combined_ligature_carets_follow_gdef_under_horizontal_compression() {
        use crate::geometry::{Direction, WritingMode};
        use crate::style::TextCombineUpright;
        for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            for direction in [Direction::Ltr, Direction::Rtl] {
                let limits = Default::default();
                let fonts = FontCollection::with_options(
                    &limits,
                    FontOptions {
                        system_fonts: false,
                        ..Default::default()
                    },
                );
                fonts
                    .register_face(
                        synthetic_font(1, false),
                        0,
                        FontFaceDescriptor {
                            family: "Caret".into(),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let style = ParagraphStyle {
                    writing_mode: mode,
                    direction,
                    root: InlineStyle {
                        direction,
                        font_families: vec![FontFamily::Named("Caret".into())],
                        font_size: 20.0,
                        text_combine_upright: TextCombineUpright::All,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut builder = ParagraphBuilder::new(&style, &limits);
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "ffiMMMM");
                let p = builder.build(&mut LayoutContext::new(), &fonts).unwrap();
                let lines = p.break_all(
                    &mut LayoutContext::new(),
                    &Default::default(),
                    100.0,
                    &AtomicSizes::EMPTY,
                );
                let run = lines[0]
                    .fragments()
                    .find_map(|f| match f {
                        Fragment::GlyphRun(run) => Some(run),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    run.glyphs().next().unwrap().id,
                    367,
                    "fixture must form the actual ffi ligature"
                );
                let scale = run.glyph_transform().block_x.abs();
                assert!(scale < 1.0, "remaining M characters force real compression");
                let layout = super::super::LineLayout::new(&lines);
                let at = |offset| {
                    layout
                        .caret(TextPosition {
                            line: 0,
                            offset,
                            affinity: Affinity::Downstream,
                        })
                        .unwrap()
                        .rect
                        .block_start
                };
                // The synthetic font declares 200/400-unit caret positions,
                // unlike a proportional third of the ffi advance.
                assert!(((at(1) - at(0)).abs() - 4.0 * scale).abs() < 0.032);
                assert!(((at(2) - at(0)).abs() - 8.0 * scale).abs() < 0.032);
            }
        }
    }

    #[test]
    fn gdef_carets_scale_and_apply_actual_variation_with_safe_fallbacks() {
        for format in [1u16, 2, 3] {
            for bad in [false, true] {
                for size in [20.0, 40.0] {
                    let limits = Default::default();
                    let fonts = FontCollection::with_options(
                        &limits,
                        FontOptions {
                            system_fonts: false,
                            ..Default::default()
                        },
                    );
                    fonts
                        .register_face(
                            synthetic_font(format, bad),
                            0,
                            FontFaceDescriptor {
                                family: "Caret".into(),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    let root = InlineStyle {
                        font_families: vec![FontFamily::Named("Caret".into())],
                        font_size: size,
                        font_variations: vec![FontVariation {
                            tag: *b"wght",
                            value: 900.0,
                        }],
                        ..Default::default()
                    };
                    let mut b = ParagraphBuilder::new(
                        &ParagraphStyle {
                            root,
                            ..Default::default()
                        },
                        &limits,
                    );
                    b.push_text(TextSource::Generated { node: NodeId(1) }, "ffi");
                    let lines = b
                        .build(&mut LayoutContext::new(), &fonts)
                        .unwrap()
                        .break_all(
                            &mut LayoutContext::new(),
                            &Default::default(),
                            1000.0,
                            &AtomicSizes::EMPTY,
                        );
                    let layout = super::super::LineLayout::new(&lines);
                    let at = |offset| {
                        layout
                            .caret(TextPosition {
                                line: 0,
                                offset,
                                affinity: Affinity::Downstream,
                            })
                            .unwrap()
                            .rect
                            .inline_start
                    };
                    let (first, second) = if format == 2 || bad {
                        (
                            lines[0].inline_size() / 3.0,
                            lines[0].inline_size() * 2.0 / 3.0,
                        )
                    } else {
                        (size * 0.2, size * if format == 3 { 0.45 } else { 0.4 })
                    };
                    assert!(
                        (at(1) - first).abs() < 0.032,
                        "format {format},bad {bad},size {size}:{} vs {first}",
                        at(1)
                    );
                    assert!(
                        (at(2) - second).abs() < 0.032,
                        "format {format},bad {bad},size {size}:{} vs {second}",
                        at(2)
                    );
                }
            }
        }
    }
}
