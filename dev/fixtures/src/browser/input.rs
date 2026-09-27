//! Shared fixed inputs for numeric browser comparison and future snapshots.
use std::sync::OnceLock;

use serde::Deserialize;
use shodo::geometry::Direction;
use shodo::mapping::{Affinity, TextOrigin};
use shodo::node::{NodeId, TextSource};
use shodo::style::{FontFamily, InlineStyle, LineHeight, ParagraphStyle, WhiteSpaceCollapse};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, Line, Paragraph, ParagraphBuilder};

use crate::{FixtureFonts, font};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserPart {
    pub text: String,
    pub depth: usize,
    pub color: Option<String>,
    pub atomic_width: Option<f32>,
    pub atomic_height: Option<f32>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserCase {
    pub id: String,
    pub seed: u32,
    pub font_ids: Vec<String>,
    pub font_size: f32,
    pub width_subpixels: u32,
    pub direction: String,
    pub lang: String,
    pub white_space: String,
    pub parts: Vec<BrowserPart>,
}

impl BrowserCase {
    pub fn text(&self) -> String {
        self.parts.iter().map(|p| p.text.as_str()).collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.font_ids.is_empty()
            || self.font_ids.iter().any(|id| font(id).is_none())
            || !self.font_size.is_finite()
            || !(1.0..=128.0).contains(&self.font_size)
            || self.width_subpixels == 0
            || self.width_subpixels > 1_048_576
            || !matches!(self.direction.as_str(), "ltr" | "rtl")
            || !matches!(self.white_space.as_str(), "normal" | "pre-wrap")
            || self.parts.is_empty()
            || self.text().is_empty()
        {
            return Err(format!("{}: invalid fixed browser input", self.id));
        }
        if self
            .parts
            .iter()
            .filter(|p| p.atomic_width.is_some())
            .count()
            > 1
        {
            return Err(format!(
                "{}: fixed recorder supports only one atomic",
                self.id
            ));
        }
        for part in &self.parts {
            if part.depth > 8 || part.text.is_empty() {
                return Err(format!("{}: invalid part", self.id));
            }
            match (part.atomic_width, part.atomic_height) {
                (None, None) if !part.text.contains('\u{fffc}') => {}
                (Some(w), Some(h))
                    if part.text == "\u{fffc}"
                        && w.is_finite()
                        && h.is_finite()
                        && w > 0.0
                        && h > 0.0 => {}
                _ => return Err(format!("{}: invalid atomic input", self.id)),
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFile {
    format_version: u32,
    generator: String,
    cases: Vec<BrowserCase>,
}

pub fn cases() -> &'static [BrowserCase] {
    static CASES: OnceLock<Vec<BrowserCase>> = OnceLock::new();
    CASES.get_or_init(|| {
        let input: InputFile =
            serde_json::from_str(include_str!("../../assets/browser-inputs.json"))
                .expect("checked-in browser inputs parse");
        assert_eq!(input.format_version, 1);
        assert!(!input.generator.is_empty());
        for case in &input.cases {
            case.validate().expect("valid fixed browser input");
        }
        input.cases
    })
}

pub struct BuiltCase {
    pub paragraph: Paragraph,
    pub atomics: AtomicSizes,
}

pub fn build(
    case: &BrowserCase,
    cx: &mut LayoutContext,
    fonts: &FixtureFonts,
) -> Result<BuiltCase, String> {
    case.validate()?;
    let direction = if case.direction == "rtl" {
        Direction::Rtl
    } else {
        Direction::Ltr
    };
    let inline = InlineStyle {
        font_families: case
            .font_ids
            .iter()
            .map(|id| FontFamily::Named(font(id).unwrap().family.into()))
            .collect(),
        font_size: case.font_size,
        lang: Some(case.lang.clone()),
        direction,
        line_height: LineHeight::Px(40.0),
        white_space_collapse: if case.white_space == "pre-wrap" {
            WhiteSpaceCollapse::Preserve
        } else {
            WhiteSpaceCollapse::Collapse
        },
        line_break: shodo::style::LineBreak::Normal,
        ..Default::default()
    };
    let style = ParagraphStyle {
        root: inline.clone(),
        direction,
        ..Default::default()
    };
    let limits = Default::default();
    let mut b = ParagraphBuilder::new(&style, &limits);
    b.with_offset_mapping(true);
    let mut depth = 0;
    let mut atomics = AtomicSizes::new();
    for (index, part) in case.parts.iter().enumerate() {
        while depth > part.depth {
            b.close_inline();
            depth -= 1;
        }
        while depth < part.depth {
            b.open_inline(
                NodeId(10000 + index as u64 * 10 + depth as u64),
                &inline,
                Default::default(),
            );
            depth += 1;
        }
        let node = NodeId(100 + index as u64);
        if let (Some(w), Some(h)) = (part.atomic_width, part.atomic_height) {
            b.push_atomic(node, &inline, Default::default());
            atomics.insert(
                node,
                AtomicSize {
                    inline_size: w,
                    block_size: h,
                    baseline: None,
                    ..Default::default()
                },
            );
        } else {
            b.push_text(TextSource::Dom { node, offset: 0 }, &part.text);
        }
    }
    while depth > 0 {
        b.close_inline();
        depth -= 1;
    }
    let paragraph = b
        .build(cx, &fonts.collection)
        .map_err(|e| format!("{}: {e:?}", case.id))?;
    Ok(BuiltCase { paragraph, atomics })
}

/// Original concatenated-source endpoint, not a processed UTF-8 offset.
pub fn source_end(case: &BrowserCase, line: &Line) -> Result<usize, String> {
    let range = line.text_range();
    if range.end == 0 {
        return Ok(0);
    }
    let origin = line
        .offset_mapping()
        .and_then(|m| m.text_to_dom(range.end as u32, Affinity::Upstream))
        .ok_or_else(|| format!("{}: missing endpoint mapping at {}", case.id, range.end))?;
    let (node, offset) = match origin {
        TextOrigin::Dom { node, offset } => (node, Some(offset as usize)),
        TextOrigin::Generated { node } => (node, None),
    };
    let index = node
        .0
        .checked_sub(100)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|&i| i < case.parts.len())
        .ok_or_else(|| format!("{}: unexpected source node {node:?}", case.id))?;
    let offset = offset.unwrap_or(case.parts[index].text.len());
    if !case.parts[index].text.is_char_boundary(offset) {
        return Err(format!("{}: invalid source endpoint", case.id));
    }
    Ok(case.parts[..index]
        .iter()
        .map(|p| p.text.len())
        .sum::<usize>()
        + offset)
}

/// Convert UTF-16 code units only at scalar boundaries; surrogate interiors fail.
pub fn utf16_to_utf8(text: &str, units: usize) -> Option<usize> {
    let mut used = 0;
    for (byte, ch) in text.char_indices() {
        if used == units {
            return Some(byte);
        }
        used += ch.len_utf16();
        if used > units {
            return None;
        }
    }
    (used == units).then_some(text.len())
}

pub fn utf8_to_utf16(text: &str, byte: usize) -> Option<usize> {
    text.get(..byte).map(|prefix| prefix.encode_utf16().count())
}
