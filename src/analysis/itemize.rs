//! Script, style, bidi and grapheme font selection across transparent nodes.
use super::bidi::BidiAnalysis;
use super::{ItemKind, whitespace::Processed};
use crate::font::{FontCollection, FontMatch, FontQuery};
use crate::style::InlineStyle;
use icu_properties::{CodePointMapData, props::Script};
use icu_segmenter::GraphemeClusterSegmenter;

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

pub(crate) fn itemize(
    input: &Processed,
    styles: &[InlineStyle],
    bidi: &BidiAnalysis,
    fonts: &FontCollection,
) -> Vec<ShapeItem> {
    let scripts = CodePointMapData::<Script>::new();
    let mut result = Vec::new();
    let mut text = String::new();
    let mut scalars = Vec::new();
    let mut style_indices = Vec::new();
    let flush = |text: &mut String,
                 scalars: &mut Vec<Scalar>,
                 style_indices: &mut Vec<u32>,
                 result: &mut Vec<ShapeItem>| {
        if scalars.is_empty() {
            return;
        }
        let segment_start = result.len();
        let mut scalar_scripts: Vec<_> = scalars.iter().map(|s| scripts.get(s.c)).collect();
        let neutral = |s: Script| matches!(s, Script::Common | Script::Inherited | Script::Unknown);
        let mut last = Script::Common;
        for script in &mut scalar_scripts {
            if neutral(*script) {
                *script = last;
            } else {
                last = *script;
            }
        }
        last = Script::Latin;
        for script in scalar_scripts.iter_mut().rev() {
            if neutral(*script) {
                *script = last;
            } else {
                last = *script;
            }
        }
        let offsets: Vec<_> = text.char_indices().map(|(i, _)| i).collect();
        let boundaries: Vec<_> = GraphemeClusterSegmenter::new().segment_str(text).collect();
        let mut scalar_start = 0;
        for window in boundaries.windows(2) {
            let scalar_end = offsets.partition_point(|i| *i < window[1]);
            scalars[scalar_start].grapheme_start = true;
            let source = &scalars[scalar_start];
            let style = style_indices[scalar_start];
            let s = &styles[style as usize];
            let locale_script: icu_locale_core::subtags::Script =
                scalar_scripts[scalar_start].into();
            let script: [u8; 4] = locale_script
                .as_str()
                .as_bytes()
                .try_into()
                .expect("script tag");
            let level = bidi.levels[source.offset as usize];
            let query = FontQuery {
                families: s.font_families.clone(),
                weight: s.font_weight,
                width: s.font_width,
                style: s.font_style,
                script,
                language: s.lang.clone(),
                synthesis: s.font_synthesis,
                ..Default::default()
            };
            let font = fonts.match_cluster(&query, &text[window[0]..window[1]]);
            let end = scalars[scalar_end - 1].offset + scalars[scalar_end - 1].c.len_utf8() as u32;
            if result.len() > segment_start
                && let Some(previous) = result.last_mut()
                && previous.style == style
                && previous.level == level
                && previous.script == script
                && previous.font == font
            {
                previous
                    .scalars
                    .extend_from_slice(&scalars[scalar_start..scalar_end]);
                previous.end = end;
                previous.after = scalars[scalar_end..(scalar_end + 5).min(scalars.len())]
                    .iter()
                    .map(|s| s.c)
                    .collect();
            } else {
                result.push(ShapeItem {
                    scalars: scalars[scalar_start..scalar_end].to_vec(),
                    end,
                    style,
                    level,
                    script,
                    font,
                    before: scalars[scalar_start.saturating_sub(5)..scalar_start]
                        .iter()
                        .map(|s| s.c)
                        .collect(),
                    after: scalars[scalar_end..(scalar_end + 5).min(scalars.len())]
                        .iter()
                        .map(|s| s.c)
                        .collect(),
                });
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
