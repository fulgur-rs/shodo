//! A discretionary hyphen is measured and shaped only as a break candidate.
use crate::analysis::units::{Unit, UnitKind};
use crate::font::FontQuery;
use crate::geometry::Saturation;
use crate::paragraph::ParagraphData;
use crate::{LayoutContext, limits::WarningKind};

fn replacement(
    data: &ParagraphData,
    unit: &Unit,
    cx: &mut LayoutContext,
) -> Option<crate::shape::Replacement> {
    let UnitKind::Cluster { run, .. } = &unit.kind else {
        return None;
    };
    let (relative, original_char) = data
        .text
        .get(unit.text.start as usize..unit.text.end as usize)?
        .char_indices()
        .next_back()?;
    if original_char != '\u{ad}' {
        return None;
    }
    let offset = unit.text.start + relative as u32;
    let original = &data.runs[*run as usize];
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    let query = FontQuery {
        families: style.font_families.clone(),
        weight: style.font_weight,
        width: style.font_width,
        style: style.font_style,
        script: original.instance.script,
        language: style.lang.clone(),
        synthesis: style.font_synthesis,
        ..Default::default()
    }
    .normalized();
    let (c, found) = ['\u{2010}', '-']
        .into_iter()
        .find_map(|c| {
            data.fonts
                .match_scripted(&query, query.script, &c.to_string())
                .map(|f| (c, Some(f)))
        })
        .unwrap_or(('-', None));
    if data
        .limits
        .max_reshape_window_bytes
        .is_some_and(|max| c.len_utf8() as u64 > max)
    {
        cx.warnings.push(
            WarningKind::Unsupported,
            "hyphen reshape window exceeded; retaining unbroken word",
        );
        return None;
    }
    Some(crate::shape::Replacement {
        text: offset..unit.text.end,
        c,
        font: found,
    })
}

pub(super) fn line(
    data: &ParagraphData,
    start: usize,
    end: usize,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<Vec<super::windows::Window>> {
    let index = source_unit(data, start, end)?;
    let replacement = replacement(data, &data.units[index], cx)?;
    super::windows::hyphen(data, start, end, &replacement, cx, sat)
}

/// A coordinated ruby cut can follow the source hyphen's box/isolate closers.
pub(super) fn source_unit(data: &ParagraphData, start: usize, end: usize) -> Option<usize> {
    (start..end).rev().find(|i| {
        !matches!(
            data.units[*i].kind,
            UnitKind::Close { .. } | UnitKind::BidiControl
        )
    })
}
