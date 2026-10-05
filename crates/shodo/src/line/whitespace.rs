//! Shared trailing whitespace policy. Advances remain available to painting
//! and selection even when excluded from line fitting or alignment.
use crate::analysis::units::UnitKind;
use crate::geometry::{LayoutUnit, Saturation};
use crate::output::BreakReason;
use crate::paragraph::ParagraphData;
use crate::style::{TextWrapMode, WhiteSpaceCollapse};

pub(super) fn hangable(data: &ParagraphData, i: usize) -> bool {
    if data.combine_at_text(data.units[i].text.start).is_some() {
        return false;
    }
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

/// Bidi-class membership tests with a constant-time path for ASCII, where
/// the boundary-neutral controls are 00-08, 0E-1B and 7F. Other characters
/// take the Unicode table.
fn transparent_char(c: char) -> bool {
    use unicode_bidi::BidiClass::*;
    if c.is_ascii() {
        return matches!(c, '\0'..='\u{8}' | '\u{e}'..='\u{1b}' | '\u{7f}');
    }
    matches!(
        unicode_bidi::bidi_class(c),
        LRI | RLI | FSI | PDI | LRE | RLE | LRO | RLO | PDF | BN
    )
}

/// Characters UAX #9 L1 resets at the end of a line: whitespace, segment and
/// paragraph separators, isolate/embedding controls and boundary neutrals.
fn trailing_char(c: char) -> bool {
    use unicode_bidi::BidiClass::*;
    if c.is_ascii() {
        return matches!(c, '\t'..='\r' | ' ' | '\u{1c}'..='\u{1f}') || transparent_char(c);
    }
    matches!(
        unicode_bidi::bidi_class(c),
        WS | S | B | LRI | RLI | FSI | PDI | LRE | RLE | LRO | RLO | PDF | BN
    )
}

pub(super) fn transparent(data: &ParagraphData, i: usize) -> bool {
    match data.units[i].kind {
        UnitKind::Close { .. }
        | UnitKind::BidiControl
        | UnitKind::Float { .. }
        | UnitKind::Absolute { .. }
        | UnitKind::ForcedBreak => true,
        UnitKind::Cluster { .. } => data.text
            [data.units[i].text.start as usize..data.units[i].text.end as usize]
            .chars()
            .all(transparent_char),
        _ => false,
    }
}

/// End edges that occur between a preserved glyph and the line edge.
pub(super) fn obstructed(data: &ParagraphData, end: usize) -> bool {
    for (offset, u) in data.units[end..].iter().enumerate() {
        match u.kind {
            UnitKind::Close { box_index } => {
                let e = data.boxes[box_index as usize].edges;
                if e.padding.inline_end != 0.0 || e.border.inline_end != 0.0 {
                    return true;
                }
            }
            UnitKind::BidiControl | UnitKind::Float { .. } | UnitKind::Absolute { .. } => {}
            UnitKind::Cluster { .. } if transparent(data, end + offset) => {}
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

/// Start of the backward run `trailing` scans, without widths and without
/// `finalize` resetting it when the hanging advance is zero. Every unit in
/// `start..end` that `trailing` would hang lies at or after it; quirks-mode
/// text presence (`line::quirk`) uses it so zero-width spaces still trim.
#[allow(dead_code)]
pub(crate) fn trailing_start(data: &ParagraphData, start: usize, end: usize) -> usize {
    let mut begin = end;
    let mut blocked = obstructed(data, end);
    for i in (start..end).rev() {
        if let UnitKind::Close { box_index } = data.units[i].kind {
            let e = data.boxes[box_index as usize].edges;
            blocked |= e.padding.inline_end != 0.0 || e.border.inline_end != 0.0;
        } else if !(hangable(data, i) && (!preserved(data, i) || !blocked)) && !transparent(data, i)
        {
            break;
        }
        begin = i;
    }
    begin
}

/// Logical trailing spaces/tabs, including both retained and hanging advances.
/// Inline end edges and transparent markers do not contribute to this measure.
pub(crate) fn trailing_advance(
    data: &ParagraphData,
    start: usize,
    end: usize,
    widths: &[LayoutUnit],
    sat: &mut Saturation,
) -> LayoutUnit {
    let mut sum = LayoutUnit::ZERO;
    for i in (start..end).rev() {
        if data.units[i].combine.is_some() {
            break;
        }
        if matches!(
            data.units[i].kind,
            UnitKind::Cluster { space: true, .. } | UnitKind::Tab
        ) {
            sum = sum.add(widths[i - start], sat);
        } else if !transparent(data, i) {
            break;
        }
    }
    sum
}

/// UAX #9 L1 resets logical trailing whitespace regardless of CSS hanging.
/// Inline edges and zero-width anchors do not end that whitespace sequence.
pub(super) fn bidi_trailing(data: &ParagraphData, start: usize, end: usize) -> usize {
    let mut begin = end;
    for i in (start..end).rev() {
        match data.units[i].kind {
            UnitKind::Cluster { .. } => {
                if data.combine_at_text(data.units[i].text.start).is_some() {
                    break;
                }
                let text =
                    &data.text[data.units[i].text.start as usize..data.units[i].text.end as usize];
                if text.chars().all(trailing_char) {
                    begin = i;
                } else {
                    break;
                }
            }
            UnitKind::Tab => {
                if data.combine_at_text(data.units[i].text.start).is_some() {
                    break;
                }
                begin = i;
            }
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak => {}
            _ => break,
        }
    }
    begin
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

#[cfg(test)]
mod class_tests {
    use super::*;
    use unicode_bidi::{BidiClass::*, bidi_class};

    #[test]
    fn ascii_class_shortcuts_match_the_unicode_table() {
        for c in ('\0'..='\u{7f}').chain(['\u{85}', '\u{a0}', '\u{200e}', '\u{2066}', '\u{5d0}']) {
            let class = bidi_class(c);
            assert_eq!(
                transparent_char(c),
                matches!(
                    class,
                    LRI | RLI | FSI | PDI | LRE | RLE | LRO | RLO | PDF | BN
                ),
                "transparent {c:?}"
            );
            assert_eq!(
                trailing_char(c),
                matches!(
                    class,
                    WS | S | B | LRI | RLI | FSI | PDI | LRE | RLE | LRO | RLO | PDF | BN
                ),
                "trailing {c:?}"
            );
        }
    }
}
