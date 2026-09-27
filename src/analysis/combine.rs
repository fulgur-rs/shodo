//! Text-combine-upright candidate ranges before horizontal shaping.

use std::ops::Range;

use super::{Item, ItemKind};
use crate::geometry::WritingMode;
use crate::style::{InlineStyle, TextCombineUpright};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CombineSpan {
    pub(crate) text: Range<u32>,
    /// The first source item; later items in the span retain their ownership.
    pub(crate) item: u32,
    pub(crate) em: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Paint {
    pub(crate) span: usize,
    pub(crate) x: f32,
    pub(crate) from: f32,
    pub(crate) to: f32,
}

#[derive(Default)]
pub(crate) struct Geometry {
    pub(crate) scales: Vec<f32>,
    pub(crate) baselines: Vec<f32>,
    pub(crate) glyphs: Vec<Option<Paint>>,
}

/// Keep the shaper's advances intact and build a separate horizontal paint
/// pen for the bidi-isolated composition. Layout consumes only the 1em square.
pub(crate) fn geometry(
    text: &str,
    items: &[Item],
    styles: &[InlineStyle],
    spans: &[CombineSpan],
    glyphs: &crate::shape::GlyphStore,
    runs: &[crate::shape::ShapedRun],
    fonts: &crate::font::FontCollection,
) -> Geometry {
    if spans.is_empty() {
        return Geometry::default();
    }
    let mut result = Geometry {
        scales: Vec::with_capacity(spans.len()),
        baselines: Vec::with_capacity(spans.len()),
        glyphs: vec![None; glyphs.len()],
    };
    for (span_index, span) in spans.iter().enumerate() {
        let (mut ascent, mut descent) = (0.0f32, 0.0f32);
        let first_run = runs.partition_point(|run| run.text.end <= span.text.start);
        for run in runs[first_run..]
            .iter()
            .take_while(|run| run.text.start < span.text.end)
        {
            let metrics = run
                .instance
                .metrics
                .unwrap_or_else(|| fonts.metrics(run.font, run.font_size));
            ascent = ascent.max(metrics.ascent);
            descent = descent.max(metrics.descent);
        }
        result
            .baselines
            .push(span.em / 2.0 + (ascent - descent) / 2.0);
        let direction = styles[items[span.item as usize].style as usize].direction;
        let level =
            unicode_bidi::Level::new(u8::from(direction == crate::geometry::Direction::Rtl))
                .unwrap();
        let bidi = unicode_bidi::BidiInfo::new(
            &text[span.text.start as usize..span.text.end as usize],
            Some(level),
        );
        let begin = glyphs.cluster.partition_point(|c| *c < span.text.start);
        let end = glyphs.cluster.partition_point(|c| *c < span.text.end);
        let mut clusters = Vec::new();
        let mut levels = Vec::new();
        let mut g = begin;
        while g < end {
            let start = g;
            let cluster = glyphs.cluster[g];
            while g < end && glyphs.cluster[g] == cluster {
                g += 1;
            }
            let width: f32 = glyphs.advance[start..g].iter().map(|v| v.to_f32()).sum();
            clusters.push((start..g, width));
            levels.push(bidi.levels[(cluster - span.text.start) as usize]);
        }
        let natural: f32 = clusters.iter().map(|(_, width)| width).sum();
        result.scales.push(if natural > 0.0 {
            (span.em / natural).min(1.0)
        } else {
            1.0
        });
        let mut x = 0.0;
        for i in unicode_bidi::BidiInfo::reorder_visual(&levels) {
            let (range, width) = &clusters[i];
            let mut relative = 0.0;
            for g in range.clone() {
                let advance = glyphs.advance[g].to_f32();
                let offset = glyphs.offset_inline[g].to_f32();
                let pen = if levels[i].is_rtl() {
                    width - relative - advance - offset
                } else {
                    relative + offset
                };
                result.glyphs[g] = Some(Paint {
                    span: span_index,
                    x: x + pen,
                    from: x + if levels[i].is_rtl() { *width } else { 0.0 },
                    to: x + if levels[i].is_rtl() { 0.0 } else { *width },
                });
                relative += advance;
            }
            x += width;
        }
        // The centering offset is part of paint, never part of shaping.
        let center = (span.em - natural * result.scales[span_index]) / 2.0;
        for paint in result.glyphs[begin..end].iter_mut().flatten() {
            paint.x = center + paint.x * result.scales[span_index];
            paint.from = center + paint.from * result.scales[span_index];
            paint.to = center + paint.to * result.scales[span_index];
        }
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Separation {
    None,
    Box,
    Barrier,
}

struct Candidate {
    span: CombineSpan,
    style: u32,
    all: bool,
    before: Separation,
}

/// Locate whole-box text that may be composed. `TextSource` boundaries are
/// storage only, while even an empty inline box is a CSS composition boundary.
pub(crate) fn prepare(
    text: &str,
    items: &[Item],
    styles: &[InlineStyle],
    mode: WritingMode,
) -> Vec<CombineSpan> {
    if !matches!(mode, WritingMode::VerticalRl | WritingMode::VerticalLr) {
        return Vec::new();
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut before = Separation::None;
    for (index, item) in items.iter().enumerate() {
        match item.kind {
            ItemKind::Text if !item.text.is_empty() => {
                debug_assert!(item.text.end as usize <= text.len());
                if before == Separation::None
                    && let Some(last) = candidates.last_mut()
                    && last.style == item.style
                    && last.span.text.end == item.text.start
                {
                    last.span.text.end = item.text.end;
                    continue;
                }
                let style = &styles[item.style as usize];
                candidates.push(Candidate {
                    span: CombineSpan {
                        text: item.text.clone(),
                        item: index as u32,
                        em: style.font_size,
                    },
                    style: item.style,
                    all: style.text_combine_upright == TextCombineUpright::All,
                    before,
                });
                before = Separation::None;
            }
            ItemKind::OpenInline { .. } | ItemKind::CloseInline => {
                before = before.max(Separation::Box);
            }
            ItemKind::OutOfFlow { .. } | ItemKind::BidiControl => {}
            _ => before = Separation::Barrier,
        }
    }
    let mut rejected = vec![false; candidates.len()];
    for i in 1..candidates.len() {
        if candidates[i].before == Separation::Box && candidates[i - 1].all && candidates[i].all {
            rejected[i - 1] = true;
            rejected[i] = true;
        }
    }
    candidates
        .into_iter()
        .zip(rejected)
        .filter_map(|(candidate, rejected)| (candidate.all && !rejected).then_some(candidate.span))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Item, ItemKind};
    use crate::geometry::WritingMode;
    use crate::node::InlineEdges;
    use crate::style::{InlineStyle, TextCombineUpright};

    fn text(start: u32, end: u32, style: u32) -> Item {
        Item {
            kind: ItemKind::Text,
            text: start..end,
            style,
            node: None,
        }
    }

    fn marker(kind: ItemKind, at: u32, style: u32) -> Item {
        Item {
            kind,
            text: at..at,
            style,
            node: None,
        }
    }

    fn all() -> InlineStyle {
        InlineStyle {
            text_combine_upright: TextCombineUpright::All,
            font_size: 16.0,
            ..Default::default()
        }
    }

    #[test]
    fn joins_text_sources_but_only_in_vertical_writing_modes() {
        let items = [text(0, 2, 0), text(2, 4, 0)];
        for mode in [WritingMode::VerticalRl, WritingMode::VerticalLr] {
            assert_eq!(
                prepare("1234", &items, &[all()], mode),
                vec![CombineSpan {
                    text: 0..4,
                    item: 0,
                    em: 16.0
                }]
            );
        }
        for mode in [
            WritingMode::HorizontalTb,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            assert!(prepare("1234", &items, &[all()], mode).is_empty());
        }
    }

    #[test]
    fn box_boundaries_reject_adjacent_all_candidates_even_when_empty() {
        let open = ItemKind::OpenInline {
            edges: InlineEdges::default(),
        };
        let items = [
            text(0, 2, 0),
            marker(open.clone(), 2, 0),
            text(2, 4, 0),
            marker(ItemKind::CloseInline, 4, 0),
        ];
        assert!(prepare("1234", &items, &[all()], WritingMode::VerticalRl).is_empty());
        let items = [
            text(0, 2, 0),
            marker(open, 2, 0),
            marker(ItemKind::CloseInline, 2, 0),
            text(2, 4, 0),
        ];
        assert!(prepare("1234", &items, &[all()], WritingMode::VerticalRl).is_empty());
    }

    #[test]
    fn atomic_and_non_all_content_do_not_suppress_neighbors() {
        let items = [
            text(0, 2, 0),
            marker(
                ItemKind::Atomic {
                    edges: InlineEdges::default(),
                    parent_style: 0,
                },
                2,
                0,
            ),
            text(2, 4, 0),
        ];
        assert_eq!(
            prepare("1234", &items, &[all()], WritingMode::VerticalRl)
                .iter()
                .map(|s| s.text.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 2..4]
        );
        let items = [
            text(0, 2, 0),
            marker(
                ItemKind::OpenInline {
                    edges: InlineEdges::default(),
                },
                2,
                1,
            ),
            text(2, 4, 1),
            marker(ItemKind::CloseInline, 4, 1),
        ];
        assert_eq!(
            prepare(
                "1234",
                &items,
                &[all(), InlineStyle::default()],
                WritingMode::VerticalRl
            )
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>(),
            vec![0..2]
        );
    }
}
