//! A discretionary hyphen is measured and shaped only as a break candidate.
use super::reshape::EdgeOverlay;
use crate::analysis::units::{Unit, UnitKind};
use crate::font::FontQuery;
use crate::geometry::{LayoutUnit, Saturation};
use crate::paragraph::ParagraphData;
use crate::{LayoutContext, limits::WarningKind};

pub(super) fn shape(
    data: &ParagraphData,
    unit: &Unit,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Option<EdgeOverlay> {
    let UnitKind::Cluster { run, glyphs, .. } = &unit.kind else {
        return None;
    };
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
    };
    let (c, found) = ['\u{2010}', '-']
        .into_iter()
        .find_map(|c| {
            data.fonts
                .match_cluster(&query, &c.to_string())
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
    let item = crate::analysis::itemize::ShapeItem {
        scalars: vec![crate::analysis::itemize::Scalar {
            c,
            offset: unit.text.start,
            item: unit.item,
            grapheme_start: true,
        }],
        end: unit.text.end,
        style: data.items[unit.item as usize].style,
        level: unit.level,
        script: original.instance.script,
        font: found,
        before: String::new(),
        after: String::new(),
    };
    let mut warnings = crate::limits::WarningSink::new(data.limits.max_warnings);
    let shaped = crate::shape::shape_items(
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
    let Ok((store, mut runs)) = shaped else {
        cx.warnings.push(
            WarningKind::Unsupported,
            "hyphen glyph budget exceeded; retaining unbroken word",
        );
        return None;
    };
    for run in &mut runs {
        run.text = unit.text.clone();
    }
    Some(EdgeOverlay {
        glyphs: glyphs.clone(),
        text: unit.text.clone(),
        store,
        runs,
    })
}

pub(super) fn width(edge: &EdgeOverlay, sat: &mut Saturation) -> LayoutUnit {
    edge.store
        .advance
        .iter()
        .fold(LayoutUnit::ZERO, |p, w| p.add(*w, sat))
}
