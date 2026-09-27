//! Script, style, bidi and grapheme font selection across transparent nodes.
use super::bidi::BidiAnalysis;
use super::{ItemKind, whitespace::Processed};
use crate::font::{FontCollection, FontMatch, FontQuery};
use crate::style::{FontFamily, FontStyle, InlineStyle};
use icu_segmenter::GraphemeClusterSegmenter;
use std::collections::HashMap;

#[cfg(test)]
std::thread_local! {
    pub(crate) static MATCH_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct Scalar {
    pub(crate) c: char,
    pub(crate) offset: u32,
    pub(crate) item: u32,
    pub(crate) grapheme_start: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct ShapeItem {
    pub(crate) scalars: Vec<Scalar>,
    pub(crate) end: u32,
    pub(crate) style: u32,
    pub(crate) level: u8,
    pub(crate) script: [u8; 4],
    pub(crate) font: Option<FontMatch>,
    pub(crate) before: String,
    pub(crate) after: String,
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
    fn new(s: &InlineStyle) -> Self {
        Self {
            families: s
                .font_families
                .iter()
                .map(|f| match f {
                    FontFamily::Named(name) => (0, name.clone()),
                    FontFamily::Generic(generic) => (*generic as u8 + 1, String::new()),
                })
                .collect(),
            weight: s.font_weight.to_bits(),
            width: s.font_width.to_bits(),
            style: match s.font_style {
                FontStyle::Normal => (0, 0),
                FontStyle::Italic => (1, 0),
                FontStyle::Oblique(angle) => (2, angle.to_bits()),
            },
            language: s.lang.as_ref().map(|l| l.to_ascii_lowercase()),
            synthesis: u8::from(s.font_synthesis.weight)
                | u8::from(s.font_synthesis.style) << 1
                | u8::from(s.font_synthesis.small_caps) << 2,
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

pub(crate) fn itemize(
    input: &Processed,
    styles: &[InlineStyle],
    bidi: &BidiAnalysis,
    breaks: &super::breaks::BreakAnalysis,
    fonts: &FontCollection,
) -> Vec<ShapeItem> {
    // Canonical query identities are built once per style, without searching a
    // growing list of styles or cloning family/language strings per scalar.
    let mut query_ids = HashMap::new();
    let mut queries = Vec::new();
    let style_queries: Vec<_> = styles
        .iter()
        .map(|s| {
            let next = queries.len();
            *query_ids.entry(QueryKey::new(s)).or_insert_with(|| {
                queries.push(FontQuery {
                    families: s.font_families.clone(),
                    weight: s.font_weight,
                    width: s.font_width,
                    style: s.font_style,
                    language: s.lang.clone(),
                    synthesis: s.font_synthesis,
                    ..Default::default()
                });
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
        let boundaries: Vec<_> = GraphemeClusterSegmenter::new().segment_str(text).collect();
        let mut scalar_start = 0;
        for window in boundaries.windows(2) {
            let mut matched = HashMap::new();
            let scalar_end = offsets.partition_point(|i| *i < window[1]);
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
                let locale_script: icu_locale_core::subtags::Script =
                    scalar_scripts[part_start].into();
                let script: [u8; 4] = locale_script
                    .as_str()
                    .as_bytes()
                    .try_into()
                    .expect("script tag");
                let level = bidi.levels[source.offset as usize];
                let query_id = style_queries[style as usize];
                let select = || {
                    let mut query = queries[query_id].clone();
                    query.script = script;
                    #[cfg(test)]
                    MATCH_CALLS.with(|calls| calls.set(calls.get() + 1));
                    fonts.match_cluster(&query, &text[window[0]..window[1]])
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
                let end = scalars[part_end - 1].offset + scalars[part_end - 1].c.len_utf8() as u32;
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
                {
                    previous
                        .scalars
                        .extend_from_slice(&scalars[part_start..part_end]);
                    previous.end = end;
                    previous.after = scalars[part_end..(part_end + 5).min(scalars.len())]
                        .iter()
                        .map(|s| s.c)
                        .collect();
                } else {
                    result.push(ShapeItem {
                        scalars: scalars[part_start..part_end].to_vec(),
                        end,
                        style,
                        level,
                        script,
                        font,
                        before: scalars[part_start.saturating_sub(5)..part_start]
                            .iter()
                            .map(|s| s.c)
                            .collect(),
                        after: scalars[part_end..(part_end + 5).min(scalars.len())]
                            .iter()
                            .map(|s| s.c)
                            .collect(),
                    });
                }
                part_start = part_end;
            }
            scalar_start = scalar_end;
        }
        text.clear();
        scalars.clear();
        style_indices.clear();
    };
    for (index, item) in input.items.iter().enumerate() {
        match item.kind {
            ItemKind::Text => {
                for (at, c) in
                    input.text[item.text.start as usize..item.text.end as usize].char_indices()
                {
                    text.push(c);
                    scalars.push(Scalar {
                        c,
                        offset: item.text.start + at as u32,
                        item: index as u32,
                        grapheme_start: false,
                    });
                    style_indices.push(item.style);
                }
            }
            ItemKind::OpenInline { .. } | ItemKind::CloseInline | ItemKind::OutOfFlow { .. } => {}
            _ => flush(&mut text, &mut scalars, &mut style_indices, &mut result),
        }
    }
    flush(&mut text, &mut scalars, &mut style_indices, &mut result);
    result
}
