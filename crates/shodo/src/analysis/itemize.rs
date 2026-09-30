//! Script, style, bidi and grapheme font selection across transparent nodes.
use super::bidi::BidiAnalysis;
use super::{ItemKind, whitespace::Processed};
use crate::font::{FontCollection, FontMatch, FontQuery};
use crate::style::{FontFamily, FontStyle, InlineStyle};
use std::collections::HashMap;

mod graphemes;

#[cfg(test)]
std::thread_local! {
    pub(crate) static MATCH_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug)]
#[cfg_attr(not(test), derive(Clone))]
pub(crate) struct Scalar {
    pub(crate) c: char,
    pub(crate) offset: u32,
    pub(crate) end: u32,
    pub(crate) item: u32,
    pub(crate) grapheme_start: bool,
}
#[cfg(test)]
std::thread_local! {
    pub(crate) static SCALAR_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// Observe actual cloning (including slice::to_vec), not a reported estimate.
#[cfg(test)]
impl Clone for Scalar {
    fn clone(&self) -> Self {
        SCALAR_CLONES.with(|count| count.set(count.get() + 1));
        Self {
            c: self.c,
            offset: self.offset,
            end: self.end,
            item: self.item,
            grapheme_start: self.grapheme_start,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeItem {
    /// Hard-boundary context; substitutions may join only within this segment.
    pub(crate) segment: u32,
    pub(crate) scalars: Vec<Scalar>,
    pub(crate) end: u32,
    pub(crate) style: u32,
    pub(crate) level: u8,
    pub(crate) script: [u8; 4],
    pub(crate) font: Option<FontMatch>,
    pub(crate) orientation: crate::shape::orientation::RunOrientation,
    pub(crate) combine: Option<u32>,
    pub(crate) width_feature: Option<[u8; 4]>,
    pub(crate) before: String,
    pub(crate) after: String,
}

pub(crate) fn inline_boundary_breaks_shaping(
    style: &InlineStyle,
    edges: &crate::node::InlineEdges,
    start: bool,
) -> bool {
    let values = if start {
        [
            edges.margin.inline_start,
            edges.border.inline_start,
            edges.padding.inline_start,
        ]
    } else {
        [
            edges.margin.inline_end,
            edges.border.inline_end,
            edges.padding.inline_end,
        ]
    };
    style.vertical_align != crate::style::VerticalAlign::Baseline
        || values.into_iter().any(|v| v != 0.0)
}

#[derive(Hash, PartialEq, Eq)]
struct QueryKey {
    families: Vec<(u8, String)>,
    weight: u32,
    width: u32,
    style: (u8, u32),
    language: Option<String>,
    synthesis: u8,
}
impl QueryKey {
    fn new(s: &FontQuery) -> Self {
        Self {
            families: s
                .families
                .iter()
                .map(|f| match f {
                    FontFamily::Named(name) => (0, name.clone()),
                    FontFamily::Generic(generic) => (*generic as u8 + 1, String::new()),
                })
                .collect(),
            weight: s.weight.to_bits(),
            width: s.width.to_bits(),
            style: match s.style {
                FontStyle::Normal => (0, 0),
                FontStyle::Italic => (1, 0),
                FontStyle::Oblique(angle) => (2, if angle == 0.0 { 0 } else { angle.to_bits() }),
            },
            language: s.language.clone(),
            synthesis: u8::from(s.synthesis.weight)
                | u8::from(s.synthesis.style) << 1
                | u8::from(s.synthesis.small_caps) << 2,
        }
    }
}

fn shaping_compatible(a: &InlineStyle, b: &InlineStyle) -> bool {
    // Face/variation selection is compared separately through FontMatch. Keep
    // layout-only properties on their source items instead of cutting GSUB/GPOS.
    macro_rules! same {
        ($($field:ident),* $(,)?) => { true $(&& a.$field == b.$field)* };
    }
    same!(
        font_size,
        font_variations,
        font_features,
        font_kerning,
        font_variant_ligatures,
        font_variant_caps,
        font_variant_numeric,
        font_variant_east_asian,
        font_variant_position,
        font_variant_alternates,
        font_optical_sizing,
        font_synthesis,
        font_size_adjust,
        lang,
        letter_spacing,
        word_spacing
    )
}

fn after_context(scalars: &[Scalar]) -> String {
    #[cfg(test)]
    tests::AFTER_CONTEXT_CALLS.with(|calls| calls.set(calls.get() + 1));
    scalars.iter().take(5).map(|s| s.c).collect()
}

pub(crate) fn itemize(
    input: &Processed,
    styles: &[InlineStyle],
    bidi: &BidiAnalysis,
    breaks: &super::breaks::BreakAnalysis,
    fonts: &FontCollection,
    mode: crate::geometry::WritingMode,
    combined: &[super::combine::CombineSpan],
) -> Vec<ShapeItem> {
    let revert_width: Vec<_> = combined
        .iter()
        .map(|span| {
            let start = breaks
                .typographic_starts
                .partition_point(|offset| *offset < span.text.start);
            let end = breaks
                .typographic_starts
                .partition_point(|offset| *offset < span.text.end);
            end - start > 1
        })
        .collect();
    let combined_levels: Vec<_> = combined
        .iter()
        .map(|span| {
            let style = &styles[input.items[span.item as usize].style as usize];
            let level = unicode_bidi::Level::new(u8::from(
                style.direction == crate::geometry::Direction::Rtl,
            ))
            .unwrap();
            unicode_bidi::BidiInfo::new(
                &input.text[span.text.start as usize..span.text.end as usize],
                Some(level),
            )
            .levels
        })
        .collect();
    // Canonical query identities are built once per style, without searching a
    // growing list of styles or cloning family/language strings per scalar.
    let mut query_ids = HashMap::new();
    let mut queries = Vec::new();
    let style_queries: Vec<_> = styles
        .iter()
        .map(|s| {
            let query = FontQuery {
                families: s.font_families.clone(),
                weight: s.font_weight,
                width: s.font_width,
                style: s.font_style,
                language: s.lang.clone(),
                synthesis: s.font_synthesis,
                ..Default::default()
            }
            .normalized();
            let next = queries.len();
            *query_ids.entry(QueryKey::new(&query)).or_insert_with(|| {
                queries.push(query);
                next
            })
        })
        .collect();
    let mut result = Vec::new();
    let mut text = String::new();
    let mut scalars = Vec::new();
    let mut style_indices = Vec::new();
    let mut compatible_styles = HashMap::new();
    let mut flush = |text: &mut String,
                     scalars: &mut Vec<Scalar>,
                     style_indices: &mut Vec<u32>,
                     result: &mut Vec<ShapeItem>| {
        if scalars.is_empty() {
            return;
        }
        let segment_start = result.len();
        let scalar_scripts = super::scripts::resolve(scalars.iter().map(|s| s.c));
        let offsets: Vec<_> = text.char_indices().map(|(i, _)| i).collect();
        let boundaries = graphemes::boundaries(scalars, breaks);
        for window in boundaries.windows(2) {
            let scalar_start = window[0];
            let scalar_end = window[1];
            let cluster = &text
                [offsets[scalar_start]..offsets.get(scalar_end).copied().unwrap_or(text.len())];
            let mut matched = HashMap::new();
            scalars[scalar_start].grapheme_start = breaks
                .graphemes
                .binary_search(&scalars[scalar_start].offset)
                .is_ok();
            let mut part_start = scalar_start;
            while part_start < scalar_end {
                let style = style_indices[part_start];
                let mut part_end = part_start + 1;
                while part_end < scalar_end && style_indices[part_end] == style {
                    part_end += 1;
                }
                let source = &scalars[part_start];
                let combine_index = combined.partition_point(|span| span.text.end <= source.offset);
                let combine = combined.get(combine_index).and_then(|span| {
                    (span.text.start <= source.offset && source.offset < span.text.end)
                        .then_some(combine_index as u32)
                });
                let orientation = combine.map_or_else(
                    || {
                        crate::shape::orientation::resolve(
                            mode,
                            styles[style as usize].text_orientation,
                            scalars[scalar_start].c,
                        )
                    },
                    |_| crate::shape::orientation::RunOrientation::Combined,
                );
                let locale_script: icu_locale_core::subtags::Script =
                    scalar_scripts[part_start].into();
                let script: [u8; 4] = locale_script
                    .as_str()
                    .as_bytes()
                    .try_into()
                    .expect("script tag");
                let level = combine.map_or_else(
                    || bidi.levels[source.offset as usize],
                    |index| {
                        combined_levels[index as usize]
                            [(source.offset - combined[index as usize].text.start) as usize]
                            .number()
                    },
                );
                let query_id = style_queries[style as usize];
                let select = || {
                    #[cfg(test)]
                    MATCH_CALLS.with(|calls| calls.set(calls.get() + 1));
                    #[cfg(test)]
                    tests::record_cluster(cluster);
                    fonts.match_scripted(&queries[query_id], script, cluster)
                };
                let font = if part_start == scalar_start && part_end == scalar_end {
                    // Ordinary one-style graphemes need no local cache allocation.
                    select()
                } else {
                    matched
                        .entry((query_id, script))
                        .or_insert_with(select)
                        .clone()
                };
                let end = scalars[part_end - 1].end;
                if result.len() > segment_start
                    && let Some(previous) = result.last_mut()
                    && (previous.style == style
                        || *compatible_styles
                            .entry((previous.style, style))
                            .or_insert_with(|| {
                                shaping_compatible(
                                    &styles[previous.style as usize],
                                    &styles[style as usize],
                                )
                            }))
                    && previous.level == level
                    && previous.script == script
                    && previous.font == font
                    && previous.orientation == orientation
                    && previous.combine == combine
                {
                    previous
                        .scalars
                        .extend_from_slice(&scalars[part_start..part_end]);
                    previous.end = end;
                } else {
                    if result.len() > segment_start {
                        // The preceding run is now complete. Intermediate
                        // grapheme suffixes are never used by the shaper.
                        result.last_mut().unwrap().after = after_context(&scalars[part_start..]);
                    }
                    result.push(ShapeItem {
                        segment: segment_start as u32,
                        scalars: scalars[part_start..part_end].to_vec(),
                        end,
                        style,
                        level,
                        script,
                        font,
                        orientation,
                        combine,
                        width_feature: None,
                        before: scalars[part_start.saturating_sub(5)..part_start]
                            .iter()
                            .map(|s| s.c)
                            .collect(),
                        after: String::new(),
                    });
                }
                part_start = part_end;
            }
        }
        // The final run has no following context inside this hard segment.
        result.last_mut().unwrap().after = after_context(&[]);
        text.clear();
        scalars.clear();
        style_indices.clear();
    };
    let mut close_boundaries = Vec::new();
    let mut active_combine = None;
    for (index, item) in input.items.iter().enumerate() {
        match item.kind {
            ItemKind::Text => {
                let mut chars = input.text[item.text.start as usize..item.text.end as usize]
                    .char_indices()
                    .peekable();
                while let Some((at, c)) = chars.next() {
                    let offset = item.text.start + at as u32;
                    let ci = combined.partition_point(|span| span.text.end <= offset);
                    let combine = combined
                        .get(ci)
                        .and_then(|span| (span.text.start <= offset).then_some(ci));
                    if combine != active_combine {
                        // The horizontal composition is a shaping isolate on
                        // both edges. Context must not join neighboring Arabic.
                        flush(&mut text, &mut scalars, &mut style_indices, &mut result);
                        active_combine = combine;
                    }
                    let origin_index = input
                        .width_origins
                        .partition_point(|origin| origin.text.end <= offset);
                    let origin = input.width_origins.get(origin_index).filter(|origin| {
                        origin.text.start == offset
                            && combined
                                .get(ci)
                                .is_some_and(|span| span.text.start <= offset && revert_width[ci])
                    });
                    if let Some(origin) = origin {
                        let mapped: Vec<_> = input.text
                            [origin.text.start as usize..origin.text.end as usize]
                            .char_indices()
                            .collect();
                        let source: Vec<_> = origin.before_width.chars().collect();
                        for (part, c) in source.iter().copied().enumerate() {
                            let (offset, end) = if mapped.len() == source.len() {
                                let (at, mapped_c) = mapped[part];
                                let offset = origin.text.start + at as u32;
                                (offset, offset + mapped_c.len_utf8() as u32)
                            } else {
                                (origin.text.start, origin.text.end)
                            };
                            let (forms, count) = super::width::narrow(c);
                            for c in forms.into_iter().take(count) {
                                text.push(c);
                                scalars.push(Scalar {
                                    c,
                                    offset,
                                    end,
                                    item: index as u32,
                                    grapheme_start: false,
                                });
                                style_indices.push(item.style);
                            }
                        }
                        while chars
                            .peek()
                            .is_some_and(|(at, _)| item.text.start + (*at as u32) < origin.text.end)
                        {
                            chars.next();
                        }
                    } else {
                        let end = offset + c.len_utf8() as u32;
                        let revert = combine.is_some_and(|i| revert_width[i]);
                        // A decomposed voiced mark follows a narrowed Katakana
                        // base. Leave marks on unrelated scripts unchanged.
                        let narrow_mark = !matches!(c, '\u{3099}' | '\u{309a}')
                            || scalars
                                .last()
                                .is_some_and(|s| matches!(s.c, '\u{ff66}'..='\u{ff9d}'));
                        let (forms, count) = if revert && narrow_mark {
                            super::width::narrow(c)
                        } else {
                            ([c, '\0'], 1)
                        };
                        for c in forms.into_iter().take(count) {
                            text.push(c);
                            scalars.push(Scalar {
                                c,
                                offset,
                                end,
                                item: index as u32,
                                grapheme_start: false,
                            });
                            style_indices.push(item.style);
                        }
                    }
                }
            }
            ItemKind::OpenInline { edges } => {
                close_boundaries.push(inline_boundary_breaks_shaping(
                    &styles[item.style as usize],
                    &edges,
                    false,
                ));
                if inline_boundary_breaks_shaping(&styles[item.style as usize], &edges, true) {
                    flush(&mut text, &mut scalars, &mut style_indices, &mut result);
                }
            }
            ItemKind::CloseInline => {
                if close_boundaries.pop().unwrap_or(false) {
                    flush(&mut text, &mut scalars, &mut style_indices, &mut result);
                }
            }
            ItemKind::OutOfFlow { .. } => {}
            _ => flush(&mut text, &mut scalars, &mut style_indices, &mut result),
        }
    }
    flush(&mut text, &mut scalars, &mut style_indices, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use crate::font::{FontCollection, FontOptions};
    use crate::geometry::WritingMode;
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::shape::orientation::RunOrientation;
    use crate::style::{ParagraphStyle, TextOrientation};
    use crate::{LayoutContext, Paragraph, ParagraphBuilder};

    std::thread_local! {
        pub(super) static AFTER_CONTEXT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static CLUSTERS: std::cell::RefCell<Option<Vec<String>>> = const {
            std::cell::RefCell::new(None)
        };
        static LOCAL_BOUNDARY_VISITS: std::cell::Cell<usize> = const {
            std::cell::Cell::new(0)
        };
        static SHARED_CUT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static REPAIRED_SCALARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn record_shared_cut() {
        SHARED_CUT_VISITS.with(|visits| visits.set(visits.get() + 1));
    }

    pub(super) fn record_repaired_scalar() {
        REPAIRED_SCALARS.with(|visits| visits.set(visits.get() + 1));
    }

    pub(super) fn observe_local_boundaries(
        boundaries: impl Iterator<Item = usize>,
    ) -> impl Iterator<Item = usize> {
        boundaries.inspect(|_| {
            LOCAL_BOUNDARY_VISITS.with(|visits| visits.set(visits.get() + 1));
        })
    }

    pub(super) fn record_cluster(cluster: &str) {
        CLUSTERS.with_borrow_mut(|clusters| {
            if let Some(clusters) = clusters {
                clusters.push(cluster.to_owned());
            }
        });
    }

    fn observe_clusters(build: impl FnOnce() -> Paragraph) -> (Paragraph, Vec<String>) {
        CLUSTERS.with_borrow_mut(|clusters| *clusters = Some(Vec::new()));
        let paragraph = build();
        let clusters = CLUSTERS.with_borrow_mut(|clusters| clusters.take().unwrap());
        (paragraph, clusters)
    }

    fn paragraph(mode: WritingMode, text: &str) -> Paragraph {
        let style = ParagraphStyle {
            writing_mode: mode,
            ..Default::default()
        };
        build(&style, |builder| {
            builder.push_text(TextSource::Generated { node: NodeId(1) }, text);
        })
    }

    fn build(style: &ParagraphStyle, add: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
        let limits = Limits::default();
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut builder = ParagraphBuilder::new(style, &limits);
        add(&mut builder);
        builder.build(&mut LayoutContext::new(), &fonts).unwrap()
    }

    #[test]
    fn after_context_work_is_bounded_by_runs_for_long_text_and_first_line() {
        for text in ["abcdefgh".repeat(256), "مرحبا".repeat(256)] {
            AFTER_CONTEXT_CALLS.with(|calls| calls.set(0));
            let p = paragraph(WritingMode::HorizontalTb, &text);
            assert_eq!(p.data.shape_items.len(), 1);
            assert_eq!(p.data.shape_items[0].before, "");
            assert_eq!(p.data.shape_items[0].after, "");
            AFTER_CONTEXT_CALLS.with(|calls| assert_eq!(calls.get(), 1));
        }
        let mut style = ParagraphStyle::default();
        let mut first = style.root.clone();
        first.text_transform = crate::style::TextTransform::Uppercase;
        style.first_line = Some(first);
        AFTER_CONTEXT_CALLS.with(|calls| calls.set(0));
        let p = build(&style, |builder| {
            builder.push_text(
                TextSource::Generated { node: NodeId(1) },
                &"Straße".repeat(256),
            );
        });
        assert_eq!(p.data.shape_items.len(), 1);
        let alternate = &p.data.first_line.as_ref().unwrap().data;
        assert_eq!(alternate.shape_items.len(), 1);
        assert_eq!(alternate.text, "STRASSE".repeat(256));
        assert_eq!(alternate.shape_items[0].after, "");
        AFTER_CONTEXT_CALLS.with(|calls| assert_eq!(calls.get(), 2));
    }

    #[test]
    fn after_context_preserves_split_grapheme_style_and_hard_boundaries() {
        let style = ParagraphStyle::default();
        let mut mark_style = style.root.clone();
        mark_style.font_size += 2.0;
        let mut edges = InlineEdges::default();
        edges.padding.inline_start = 1.0;
        let p = build(&style, |builder| {
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "abcd");
            builder.open_inline(NodeId(2), &mark_style, InlineEdges::default());
            builder.push_text(TextSource::Generated { node: NodeId(3) }, "\u{301}efghijk");
            builder.close_inline();
            builder.push_text(TextSource::Generated { node: NodeId(4) }, "lmnopqr");
            builder.open_inline(NodeId(5), &style.root, edges);
            builder.push_text(TextSource::Generated { node: NodeId(6) }, "stuvwxy");
            builder.close_inline();
        });
        let contexts: Vec<_> = p
            .data
            .shape_items
            .iter()
            .map(|item| {
                (
                    item.scalars.iter().map(|s| s.c).collect::<String>(),
                    item.before.as_str(),
                    item.after.as_str(),
                )
            })
            .collect();
        assert_eq!(
            contexts,
            [
                ("abcd".into(), "", "\u{301}efgh"),
                ("\u{301}efghijk".into(), "abcd", "lmnop"),
                ("lmnopqr".into(), "ghijk", ""),
                ("stuvwxy".into(), "", ""),
            ]
        );
        let ranges: Vec<_> = p
            .data
            .shape_items
            .iter()
            .map(|item| item.scalars[0].offset..item.end)
            .collect();
        assert_eq!(ranges, [0..4, 4..13, 13..20, 20..27]);
        assert!(!p.data.shape_items[1].scalars[0].grapheme_start);
        assert_eq!(p.data.shape_items[0].segment, p.data.shape_items[2].segment);
        assert_ne!(p.data.shape_items[2].segment, p.data.shape_items[3].segment);
        assert!(p.data.shape_items.iter().all(|item| item.combine.is_none()));
    }

    #[test]
    fn padding_boundary_restarts_regional_indicator_pairing() {
        let style = ParagraphStyle::default();
        let mut edges = InlineEdges::default();
        edges.padding.inline_start = 1.0;
        let (paragraph, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "🇦");
                builder.open_inline(NodeId(2), &style.root, edges);
                builder.push_text(TextSource::Generated { node: NodeId(3) }, "🇧🇨🇩");
                builder.close_inline();
            })
        });
        assert_eq!(paragraph.text(), "🇦🇧🇨🇩");
        assert_eq!(clusters, ["🇦", "🇧🇨", "🇩"]);
    }

    #[test]
    fn padding_boundary_restarts_emoji_zwj_context() {
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            ..Default::default()
        };
        let mut edges = InlineEdges::default();
        edges.padding.inline_start = 1.0;
        let (paragraph, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "👩");
                builder.open_inline(NodeId(2), &style.root, edges);
                builder.push_text(TextSource::Generated { node: NodeId(3) }, "\u{200d}💻");
                builder.close_inline();
            })
        });
        assert_eq!(clusters, ["👩", "\u{200d}", "💻"]);
        let orientations: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .map(|item| (item.scalars[0].offset..item.end, item.orientation))
            .collect();
        assert_eq!(
            orientations,
            [
                (0..4, RunOrientation::Upright),
                (4..7, RunOrientation::SidewaysClockwise),
                (7..11, RunOrientation::Upright),
            ]
        );
    }

    #[test]
    fn combined_width_reversion_preserves_font_clusters_and_source_ranges() {
        let mut style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            ..Default::default()
        };
        style.root.text_combine_upright = crate::style::TextCombineUpright::All;
        let (paragraph, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "ガ12");
            })
        });
        assert_eq!(paragraph.text(), "ガ12");
        assert_eq!(clusters, ["ｶﾞ", "1", "2"]);
        let ranges: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .flat_map(|item| item.scalars.iter().map(|s| (s.offset, s.end)))
            .collect();
        assert_eq!(ranges, [(0, 3), (0, 3), (3, 4), (4, 5)]);
    }

    #[test]
    fn padding_boundary_restarts_indic_linker_context() {
        let style = ParagraphStyle::default();
        let mut edges = InlineEdges::default();
        edges.padding.inline_start = 1.0;
        let (_, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "क");
                builder.open_inline(NodeId(2), &style.root, edges);
                builder.push_text(TextSource::Generated { node: NodeId(3) }, "्क");
                builder.close_inline();
            })
        });
        assert_eq!(clusters, ["क", "्", "क"]);
    }

    #[test]
    fn authored_bidi_control_restarts_regional_indicator_pairing() {
        let (_, clusters) =
            observe_clusters(|| paragraph(WritingMode::HorizontalTb, "🇦\u{200e}🇧🇨🇩"));
        assert_eq!(clusters, ["🇦", "\u{200e}", "🇧🇨", "🇩"]);
    }

    #[test]
    fn source_gap_preserves_the_full_cluster_query() {
        let style = ParagraphStyle::default();
        let (paragraph, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "a");
                builder.push_out_of_flow(NodeId(2), crate::node::OutOfFlowKind::Absolute);
                builder.push_text(TextSource::Generated { node: NodeId(3) }, "\u{301}b");
            })
        });
        assert_eq!(clusters, ["a\u{301}", "b"]);
        let scalars: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .flat_map(|item| {
                item.scalars
                    .iter()
                    .map(|s| (s.offset, s.end, s.grapheme_start))
            })
            .collect();
        assert_eq!(scalars, [(0, 1, true), (4, 6, false), (6, 7, true)]);
    }

    #[test]
    fn cluster_style_parts_use_the_same_full_font_query() {
        let style = ParagraphStyle::default();
        let mut mark_style = style.root.clone();
        mark_style.font_weight = 700.0;
        let (paragraph, clusters) = observe_clusters(|| {
            build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "a");
                builder.open_inline(NodeId(2), &mark_style, InlineEdges::default());
                builder.push_text(TextSource::Generated { node: NodeId(3) }, "\u{301}");
                builder.close_inline();
            })
        });
        assert_eq!(clusters, ["a\u{301}", "a\u{301}"]);
        let scalars: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .flat_map(|item| {
                item.scalars
                    .iter()
                    .map(|s| (s.offset, s.end, s.grapheme_start))
            })
            .collect();
        assert_eq!(scalars, [(0, 1, true), (1, 3, false)]);
    }

    #[test]
    fn shared_cuts_remove_local_segmentation() {
        let mut visits = Vec::new();
        for decorated in [false, true] {
            LOCAL_BOUNDARY_VISITS.with(|visits| visits.set(0));
            SHARED_CUT_VISITS.with(|visits| visits.set(0));
            REPAIRED_SCALARS.with(|visits| visits.set(0));
            let style = ParagraphStyle::default();
            let mut edges = InlineEdges::default();
            edges.padding.inline_start = 1.0;
            edges.padding.inline_end = 1.0;
            let p = build(&style, |builder| {
                for i in 0..32 {
                    if decorated {
                        builder.open_inline(NodeId(i * 2 + 1), &style.root, edges);
                    }
                    builder.push_text(
                        TextSource::Generated {
                            node: NodeId(i * 2 + 2),
                        },
                        "a§",
                    );
                    if decorated {
                        builder.close_inline();
                    }
                }
            });
            assert_eq!(p.text(), "a§".repeat(32));
            assert!(!p.data.shape_items.is_empty());
            visits.push(LOCAL_BOUNDARY_VISITS.with(|visits| visits.get()));
            assert!(SHARED_CUT_VISITS.with(|visits| visits.get()) > 0);
            assert_eq!(REPAIRED_SCALARS.with(|visits| visits.get()), 0);
        }
        assert_eq!(
            visits,
            [0, 0],
            "plain and decorated runs must reuse paragraph cuts"
        );
    }

    #[test]
    fn authored_combined_width_forms_keep_original_scalar_ranges() {
        use crate::style::TextCombineUpright;
        for (text, expected) in [
            ("ＡＢ", "AB"),
            ("Ａ\u{3000}Ｂ", "A B"),
            ("ガ12", "ｶﾞ12"),
            ("カ\u{3099}12", "ｶﾞ12"),
            ("パ12", "ﾊﾟ12"),
            ("￦￡", "₩£"),
            ("ㄱㄴ", "ﾡﾤ"),
        ] {
            let mut style = ParagraphStyle {
                writing_mode: WritingMode::VerticalRl,
                ..Default::default()
            };
            style.root.text_combine_upright = TextCombineUpright::All;
            let p = build(&style, |b| {
                b.push_text(TextSource::Generated { node: NodeId(1) }, text);
            });
            let actual: String = p
                .data
                .shape_items
                .iter()
                .flat_map(|item| item.scalars.iter().map(|s| s.c))
                .collect();
            assert_eq!(actual, expected, "{text}");
            assert_eq!(p.text(), text);
            let scalars: Vec<_> = p
                .data
                .shape_items
                .iter()
                .flat_map(|item| &item.scalars)
                .collect();
            assert!(
                scalars
                    .iter()
                    .all(|s| text.is_char_boundary(s.offset as usize)
                        && text.is_char_boundary(s.end as usize))
            );
            assert_eq!(scalars.first().unwrap().offset, 0);
            assert_eq!(scalars.last().unwrap().end as usize, text.len());
            if text == "ガ12" {
                assert_eq!(
                    scalars
                        .iter()
                        .map(|s| (s.offset, s.end))
                        .collect::<Vec<_>>(),
                    vec![(0, 3), (0, 3), (3, 4), (4, 5)]
                );
            }
        }
    }

    #[test]
    fn mixed_vertical_orientation_cuts_shaping_items() {
        // UAX50: section sign is upright, both Latin letters are rotated.
        // Their script and missing-font identity are equal, so only the
        // orientation boundary should prevent joining their shaping items.
        let horizontal = paragraph(WritingMode::HorizontalTb, "a§b");
        assert_eq!(horizontal.data.shape_items.len(), 1);
        let vertical = paragraph(WritingMode::VerticalRl, "a§b");
        assert_eq!(vertical.data.shape_items.len(), 3);
        let ranges: Vec<_> = vertical
            .data
            .shape_items
            .iter()
            .map(|item| item.scalars[0].offset..item.end)
            .collect();
        assert_eq!(ranges, vec![0..1, 1..3, 3..4]);
    }

    #[test]
    fn writing_mode_and_text_orientation_item_matrix() {
        use RunOrientation::{
            Horizontal as H, SidewaysClockwise as C, SidewaysCounterClockwise as A, Upright as U,
        };
        use TextOrientation::{Mixed, Sideways, Upright};
        use WritingMode::{HorizontalTb, SidewaysLr, SidewaysRl, VerticalLr, VerticalRl};
        for (mode, orientation, expected) in [
            (HorizontalTb, Mixed, vec![H]),
            (HorizontalTb, Upright, vec![H]),
            (HorizontalTb, Sideways, vec![H]),
            (VerticalRl, Mixed, vec![C, U, C]),
            (VerticalRl, Upright, vec![U]),
            (VerticalRl, Sideways, vec![C]),
            (VerticalLr, Mixed, vec![C, U, C]),
            (VerticalLr, Upright, vec![U]),
            (VerticalLr, Sideways, vec![C]),
            (SidewaysRl, Mixed, vec![C]),
            (SidewaysRl, Upright, vec![C]),
            (SidewaysRl, Sideways, vec![C]),
            (SidewaysLr, Mixed, vec![A]),
            (SidewaysLr, Upright, vec![A]),
            (SidewaysLr, Sideways, vec![A]),
        ] {
            let mut style = ParagraphStyle {
                writing_mode: mode,
                ..Default::default()
            };
            style.root.text_orientation = orientation;
            let paragraph = build(&style, |builder| {
                builder.push_text(TextSource::Generated { node: NodeId(1) }, "a§b");
            });
            let actual: Vec<_> = paragraph
                .data
                .shape_items
                .iter()
                .map(|item| item.orientation)
                .collect();
            assert_eq!(actual, expected, "{mode:?}/{orientation:?}");
        }
    }

    #[test]
    fn mixed_orientation_keeps_marks_and_selectors_with_their_grapheme() {
        use RunOrientation::{SidewaysClockwise as C, Upright as U};
        // U+2329 is Tr and U+3001 is Tu; both use upright vertical shaping.
        let paragraph = paragraph(WritingMode::VerticalRl, "a\u{301}§\u{fe0f}〈、");
        let actual: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .flat_map(|item| {
                item.scalars
                    .iter()
                    .map(move |scalar| (scalar.c, item.orientation))
            })
            .collect();
        assert_eq!(
            actual,
            vec![
                ('a', C),
                ('\u{301}', C),
                ('§', U),
                ('\u{fe0f}', U),
                ('〈', U),
                ('、', U)
            ]
        );
    }

    #[test]
    fn mixed_grapheme_orientation_survives_source_and_style_boundaries() {
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            ..Default::default()
        };
        let mut mark_style = style.root.clone();
        mark_style.font_size += 1.0;
        let paragraph = build(&style, |builder| {
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(1),
                    offset: 0,
                },
                "§",
            );
            builder.open_inline(NodeId(2), &mark_style, InlineEdges::default());
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(3),
                    offset: 0,
                },
                "\u{301}",
            );
            builder.close_inline();
            builder.push_text(
                TextSource::Dom {
                    node: NodeId(4),
                    offset: 0,
                },
                "ab",
            );
        });
        let actual: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .map(|item| (item.scalars[0].offset..item.end, item.orientation))
            .collect();
        assert_eq!(
            actual,
            vec![
                (0..2, RunOrientation::Upright),
                (2..4, RunOrientation::Upright),
                (4..6, RunOrientation::SidewaysClockwise),
            ]
        );
    }

    #[test]
    fn different_text_orientation_styles_prevent_horizontal_script_joining() {
        let style = ParagraphStyle {
            writing_mode: WritingMode::VerticalRl,
            ..Default::default()
        };
        let mut upright = style.root.clone();
        upright.text_orientation = TextOrientation::Upright;
        let paragraph = build(&style, |builder| {
            builder.push_text(TextSource::Generated { node: NodeId(1) }, "a");
            builder.open_inline(NodeId(2), &upright, InlineEdges::default());
            builder.push_text(TextSource::Generated { node: NodeId(3) }, "b");
            builder.close_inline();
        });
        let actual: Vec<_> = paragraph
            .data
            .shape_items
            .iter()
            .map(|item| item.orientation)
            .collect();
        assert_eq!(
            actual,
            vec![RunOrientation::SidewaysClockwise, RunOrientation::Upright]
        );
    }
}
