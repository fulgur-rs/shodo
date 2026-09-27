//! Shared trailing whitespace policy. Advances remain available to painting
//! and selection even when excluded from line fitting or alignment.
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::output::BreakReason;
use crate::paragraph::ParagraphData;
use crate::style::{TextWrapMode, WhiteSpaceCollapse};

pub(super) fn hangable(data: &ParagraphData, i: usize) -> bool {
    if !matches!(
        data.units[i].kind,
        UnitKind::Cluster { space: true, .. } | UnitKind::Tab
    ) {
        return false;
    }
    let s = &data.styles[data.items[data.units[i].item as usize].style as usize];
    match s.white_space_collapse {
        WhiteSpaceCollapse::Collapse
        | WhiteSpaceCollapse::PreserveBreaks
        | WhiteSpaceCollapse::PreserveSpaces => true,
        WhiteSpaceCollapse::Preserve => s.text_wrap_mode != TextWrapMode::NoWrap,
        WhiteSpaceCollapse::BreakSpaces => false,
    }
}

pub(super) fn preserved(data: &ParagraphData, i: usize) -> bool {
    let s = &data.styles[data.items[data.units[i].item as usize].style as usize];
    s.white_space_collapse == WhiteSpaceCollapse::Preserve
}

pub(super) fn transparent(data: &ParagraphData, i: usize) -> bool {
    matches!(
        data.units[i].kind,
        UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak
    )
}

/// End edges that occur between a preserved glyph and the line edge.
pub(super) fn obstructed(data: &ParagraphData, end: usize) -> bool {
    for u in &data.units[end..] {
        match u.kind {
            UnitKind::Close { box_index } => {
                let e = data.boxes[box_index as usize].edges;
                if e.padding.inline_end != 0.0 || e.border.inline_end != 0.0 {
                    return true;
                }
            }
            UnitKind::BidiControl => {}
            _ => break,
        }
    }
    super::decoration::chain(data, end).iter().any(|b| {
        let e = data.boxes[*b as usize].edges;
        super::decoration::cloned(data, *b)
            && (e.padding.inline_end != 0.0 || e.border.inline_end != 0.0)
    })
}

pub(super) fn fits_hanging(data: &ParagraphData, i: usize) -> bool {
    hangable(data, i) && (!preserved(data, i) || !obstructed(data, i + 1))
}

pub(super) fn trailing(
    data: &ParagraphData,
    start: usize,
    end: usize,
    widths: &[LayoutUnit],
    sat: &mut Saturation,
) -> (usize, LayoutUnit) {
    let mut begin = end;
    let mut sum = LayoutUnit::ZERO;
    let mut blocked = obstructed(data, end);
    for i in (start..end).rev() {
        if let UnitKind::Close { box_index } = data.units[i].kind {
            let e = data.boxes[box_index as usize].edges;
            blocked |= e.padding.inline_end != 0.0 || e.border.inline_end != 0.0;
        } else if hangable(data, i) && (!preserved(data, i) || !blocked) {
            begin = i;
            sum = sum.add(widths[i - start], sat);
        } else if !transparent(data, i) {
            break;
        }
    }
    (begin, sum)
}

/// Fit has already excluded eligible whitespace. Forced/end lines keep the
/// part of preserved whitespace that fits, before alignment/justification.
pub(super) fn finalize(
    data: &ParagraphData,
    start: usize,
    scan: &mut super::Scan,
    available: LayoutUnit,
    indent: LayoutUnit,
    sat: &mut Saturation,
) {
    let (_, total) = trailing(data, start, scan.end, &scan.widths, sat);
    scan.hanging_end = total;
    if matches!(scan.reason, BreakReason::Regular | BreakReason::Emergency) {
        return;
    }
    let conditional = (scan.hang_start..scan.end)
        .filter(|i| hangable(data, *i) && preserved(data, *i))
        .fold(LayoutUnit::ZERO, |sum, i| {
            sum.add(scan.widths[i - start], sat)
        });
    let retained = available
        .sub(indent, sat)
        .sub(scan.content, sat)
        .max(LayoutUnit::ZERO)
        .min(conditional);
    scan.content = scan.content.add(retained, sat);
    scan.hanging_end = total.sub(retained, sat);
    if scan.hanging_end == LayoutUnit::ZERO {
        scan.hang_start = scan.end;
    }
}
