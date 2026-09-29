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
    /// One external composition unit owns these selectable source parts.
    /// Assigned after shaping; line breaking cannot split this range.
    pub(crate) units: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Paint {
    pub(crate) span: usize,
    pub(crate) x: f32,
    pub(crate) from: f32,
    pub(crate) to: f32,
    pub(crate) extra: f32,
}

#[derive(Default)]
pub(crate) struct Geometry {
    pub(crate) scales: Vec<f32>,
    pub(crate) baselines: Vec<f32>,
    pub(crate) glyphs: Vec<Option<Paint>>,
    pub(crate) tabs: Vec<(Range<u32>, Paint)>,
}

struct HorizontalCluster {
    glyphs: Range<usize>,
    text_start: u32,
    font_width: f32,
    word: crate::geometry::LayoutUnit,
    tab_style: Option<usize>,
    width: f32,
    x: f32,
}

/// Keep the shaper's advances intact and build a separate horizontal paint
/// pen for the bidi-isolated composition. Layout consumes only the 1em square.
#[allow(clippy::too_many_arguments)]
pub(crate) fn geometry(
    text: &str,
    items: &[Item],
    styles: &[InlineStyle],
    spans: &[CombineSpan],
    glyphs: &crate::shape::GlyphStore,
    runs: &[crate::shape::ShapedRun],
    fonts: &crate::font::FontCollection,
    metrics: &[crate::line::font_metrics::StyleMetrics],
    sat: &mut crate::geometry::Saturation,
) -> Geometry {
    if spans.is_empty() {
        return Geometry::default();
    }
    let mut result = Geometry {
        scales: Vec::with_capacity(spans.len()),
        baselines: Vec::with_capacity(spans.len()),
        glyphs: vec![None; glyphs.len()],
        tabs: Vec::new(),
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
        let mut g = begin;
        while g < end {
            let start = g;
            let cluster = glyphs.cluster[g];
            while g < end && glyphs.cluster[g] == cluster {
                g += 1;
            }
            let width: f32 = glyphs.advance[start..g].iter().map(|v| v.to_f32()).sum();
            let text_end = if g < end {
                glyphs.cluster[g]
            } else {
                span.text.end
            };
            let mut extra = crate::geometry::LayoutUnit::ZERO;
            for (at, ch) in text[cluster as usize..text_end as usize].char_indices() {
                if crate::line::spacing::word_separator(ch) {
                    let item = items.partition_point(|item| item.text.end <= cluster + at as u32);
                    extra = extra.add(
                        crate::geometry::LayoutUnit::from_f32_round(
                            styles[items[item].style as usize].word_spacing,
                            sat,
                        ),
                        sat,
                    );
                }
            }
            clusters.push(HorizontalCluster {
                glyphs: start..g,
                text_start: cluster,
                font_width: width,
                word: extra,
                tab_style: None,
                width: width + extra.to_f32(),
                x: 0.0,
            });
        }
        let first_item = items.partition_point(|item| item.text.end <= span.text.start);
        for item in items[first_item..]
            .iter()
            .take_while(|item| item.text.start < span.text.end)
        {
            if matches!(item.kind, ItemKind::Tab) {
                clusters.push(HorizontalCluster {
                    glyphs: 0..0,
                    text_start: item.text.start,
                    font_width: 0.0,
                    word: crate::geometry::LayoutUnit::ZERO,
                    tab_style: Some(item.style as usize),
                    width: 0.0,
                    x: 0.0,
                });
            }
        }
        clusters.sort_by_key(|cluster| cluster.text_start);
        let levels: Vec<_> = clusters
            .iter()
            .map(|cluster| {
                if cluster.tab_style.is_some() {
                    level
                } else {
                    bidi.levels[(cluster.text_start - span.text.start) as usize]
                }
            })
            .collect();
        let rtl = direction == crate::geometry::Direction::Rtl;
        let mut order = unicode_bidi::BidiInfo::reorder_visual(&levels);
        if rtl {
            order.reverse();
        }
        let mut natural = 0.0;
        for &i in &order {
            let cluster = &mut clusters[i];
            cluster.x = natural;
            if let Some(style_index) = cluster.tab_style {
                let style = &styles[style_index];
                let font = metrics[style_index];
                let interval = match style.tab_size {
                    crate::style::TabSize::Spaces(n) => n * (font.space + style.word_spacing),
                    crate::style::TabSize::Px(value) => value,
                };
                cluster.width = crate::line::tab_advance(
                    crate::geometry::LayoutUnit::from_f32_round(natural, sat),
                    crate::geometry::LayoutUnit::from_f32_round(interval, sat),
                    crate::geometry::LayoutUnit::from_f32_round(font.ch * 0.5, sat),
                )
                .to_f32();
            }
            natural += cluster.width;
        }
        result.scales.push(if natural > 0.0 {
            (span.em / natural).min(1.0)
        } else {
            1.0
        });
        let tab_begin = result.tabs.len();
        for i in order {
            let cluster = &clusters[i];
            let range = &cluster.glyphs;
            let width = cluster.font_width;
            let layout_width = cluster.width;
            let x = if rtl {
                natural - cluster.x - layout_width
            } else {
                cluster.x
            };
            if cluster.tab_style.is_some() {
                result.tabs.push((
                    cluster.text_start..cluster.text_start + 1,
                    Paint {
                        span: span_index,
                        x,
                        from: x + if levels[i].is_rtl() {
                            layout_width
                        } else {
                            0.0
                        },
                        to: x + if levels[i].is_rtl() {
                            0.0
                        } else {
                            layout_width
                        },
                        extra: 0.0,
                    },
                ));
                continue;
            }
            let leading = cluster.word.div_i32(2).to_f32();
            let owner = range
                .clone()
                .rfind(|&g| glyphs.advance[g] != crate::geometry::LayoutUnit::ZERO)
                .unwrap_or(range.end - 1);
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
                    x: x + pen + leading,
                    from: x + if levels[i].is_rtl() {
                        layout_width
                    } else {
                        0.0
                    },
                    to: x + if levels[i].is_rtl() {
                        0.0
                    } else {
                        layout_width
                    },
                    extra: if g == owner {
                        cluster.word.to_f32()
                    } else {
                        0.0
                    },
                });
                relative += advance;
            }
        }
        // The centering offset is part of paint, never part of shaping.
        let center = (span.em - natural * result.scales[span_index]) / 2.0;
        for paint in result.glyphs[begin..end].iter_mut().flatten() {
            paint.x = center + paint.x * result.scales[span_index];
            paint.from = center + paint.from * result.scales[span_index];
            paint.to = center + paint.to * result.scales[span_index];
        }
        for (_, paint) in &mut result.tabs[tab_begin..] {
            paint.x = center + paint.x * result.scales[span_index];
            paint.from = center + paint.from * result.scales[span_index];
            paint.to = center + paint.to * result.scales[span_index];
        }
    }
    result.tabs.sort_by_key(|(text, _)| text.start);
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
    scope: Option<u32>,
}

/// Locate whole-box text that may be composed. `TextSource` boundaries are
/// storage only, while even an empty inline box is a CSS composition boundary.
pub(crate) fn prepare(
    text: &str,
    items: &[Item],
    styles: &[InlineStyle],
    mode: WritingMode,
) -> Vec<CombineSpan> {
    prepare_with_rejections(text, items, styles, mode).0
}

/// [`prepare`] plus the source ranges of `text-combine-upright: all` boxes
/// that were not composed because a box boundary separated them from an
/// adjacent candidate in the same combine scope. Callers report these as
/// diagnostics instead of letting them fall back to normal text silently.
pub(crate) fn prepare_with_rejections(
    text: &str,
    items: &[Item],
    styles: &[InlineStyle],
    mode: WritingMode,
) -> (Vec<CombineSpan>, Vec<Range<u32>>) {
    if !matches!(mode, WritingMode::VerticalRl | WritingMode::VerticalLr) {
        return (Vec::new(), Vec::new());
    }
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut before = Separation::None;
    let mut scope = styles
        .first()
        .and_then(|style| (style.text_combine_upright == TextCombineUpright::All).then_some(0));
    let mut scopes = Vec::new();
    let mut next_scope = 1u32;
    for (index, item) in items.iter().enumerate() {
        match item.kind {
            ItemKind::Text | ItemKind::ForcedBreak | ItemKind::Tab
                if !item.text.is_empty()
                    && (matches!(item.kind, ItemKind::Text)
                        || styles[item.style as usize].text_combine_upright
                            == TextCombineUpright::All) =>
            {
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
                        units: 0..0,
                    },
                    style: item.style,
                    all: style.text_combine_upright == TextCombineUpright::All,
                    before,
                    scope,
                });
                before = Separation::None;
            }
            ItemKind::OpenInline { .. } => {
                before = before.max(Separation::Box);
                scopes.push(scope);
                scope = if styles[item.style as usize].text_combine_upright
                    == TextCombineUpright::All
                {
                    scope.or_else(|| {
                        let id = next_scope;
                        next_scope += 1;
                        Some(id)
                    })
                } else {
                    None
                };
            }
            ItemKind::CloseInline => {
                before = before.max(Separation::Box);
                scope = scopes.pop().flatten();
            }
            ItemKind::OutOfFlow { .. } | ItemKind::BidiControl => {}
            ItemKind::Text => {}
            _ => before = Separation::Barrier,
        }
    }
    let mut rejected = vec![false; candidates.len()];
    for i in 1..candidates.len() {
        if candidates[i].before == Separation::Box
            && candidates[i - 1].all
            && candidates[i].all
            && candidates[i].scope.is_some()
            && candidates[i - 1].scope == candidates[i].scope
        {
            rejected[i - 1] = true;
            rejected[i] = true;
        }
    }
    let mut spans = Vec::new();
    let mut skipped = Vec::new();
    for (candidate, rejected) in candidates.into_iter().zip(rejected) {
        if !candidate.all {
            continue;
        }
        if rejected {
            skipped.push(candidate.span.text);
        } else {
            spans.push(candidate.span);
        }
    }
    (spans, skipped)
}

/// Independent horizontal inline-block whitespace processing for each
/// candidate. The transform pass applies these omissions to the source map.
pub(crate) fn omissions(
    input: &super::whitespace::Processed,
    styles: &[InlineStyle],
    mode: WritingMode,
) -> Vec<Range<u32>> {
    let spans = prepare(&input.text, &input.items, styles, mode);
    let mut result: Vec<Range<u32>> = Vec::new();
    for span in spans {
        let start = input
            .items
            .partition_point(|item| item.text.end <= span.text.start);
        let mut chars = Vec::new();
        for item in input.items[start..]
            .iter()
            .take_while(|item| item.text.start < span.text.end)
        {
            let collapse = matches!(
                styles[item.style as usize].white_space_collapse,
                crate::style::WhiteSpaceCollapse::Collapse
                    | crate::style::WhiteSpaceCollapse::PreserveBreaks
            );
            let ignored = matches!(item.kind, ItemKind::ForcedBreak | ItemKind::BidiControl);
            for (at, c) in
                input.text[item.text.start as usize..item.text.end as usize].char_indices()
            {
                let at = item.text.start + at as u32;
                chars.push((at..at + c.len_utf8() as u32, ignored, collapse && c == ' '));
            }
        }
        let first = chars
            .iter()
            .position(|(_, ignored, space)| !ignored && !space);
        let last = chars
            .iter()
            .rposition(|(_, ignored, space)| !ignored && !space);
        for (index, (range, ignored, space)) in chars.into_iter().enumerate() {
            if ignored
                || space
                    && (first.is_none_or(|first| index < first)
                        || last.is_none_or(|last| last < index))
            {
                if let Some(previous) = result.last_mut()
                    && previous.end == range.start
                {
                    previous.end = range.end;
                } else {
                    result.push(range);
                }
            }
        }
    }
    result
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
                    em: 16.0,
                    units: 0..0,
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
