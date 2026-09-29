//! Raw-byte flags for IFC-wide phase-I whitespace processing. Every raw
//! scalar is visited a bounded number of times, including across many empty
//! elements and out-of-flow markers.

use crate::builder::RawItem;
use crate::style::{InlineStyle, WhiteSpaceCollapse};
use icu_properties::{CodePointMapData, CodePointSetData, props};

pub(super) const REMOVE: u8 = 1;
const COLLAPSE_SPACE: u8 = 2;
const BREAK: u8 = 4;
const COLLAPSE_BREAK: u8 = 8;
const LEFT_WIDE: u8 = 16;
const LEFT_ZWSP: u8 = 32;

pub(super) fn ignorable(c: char) -> bool {
    // The first Default_Ignorable_Code_Point is U+00AD; ASCII text, which is
    // most text, skips the table lookup.
    c >= '\u{AD}'
        && c != '\u{200B}'
        && CodePointSetData::new::<props::DefaultIgnorableCodePoint>().contains(c)
}

fn wide(c: char) -> bool {
    // Korean joins need spaces even when Hangul has East Asian Width W.
    if matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}'
        | '\u{A960}'..='\u{A97F}' | '\u{AC00}'..='\u{D7AF}' | '\u{D7B0}'..='\u{D7FF}')
    {
        return false;
    }
    matches!(
        CodePointMapData::<props::EastAsianWidth>::new().get(c),
        props::EastAsianWidth::Wide
            | props::EastAsianWidth::Fullwidth
            | props::EastAsianWidth::Halfwidth
    )
}

pub(super) fn whitespace_flags(text: &str, raw: &[RawItem], styles: &[InlineStyle]) -> Vec<u8> {
    flags_in_context(text, raw, styles, false)
}

pub(super) fn flags_in_context(
    text: &str,
    raw: &[RawItem],
    styles: &[InlineStyle],
    annotation: bool,
) -> Vec<u8> {
    use WhiteSpaceCollapse::*;
    let mut flags = vec![0; text.len()];
    let mut boundaries = vec![0usize];
    let mut cursor = 0;
    for item in raw {
        match item {
            RawItem::Text { range, style, .. } => {
                let mode = styles[*style as usize].white_space_collapse;
                for (i, c) in text[range.start as usize..range.end as usize].char_indices() {
                    let f = &mut flags[range.start as usize + i];
                    if matches!(c, ' ' | '\t' | '\r') && matches!(mode, Collapse | PreserveBreaks) {
                        *f |= COLLAPSE_SPACE;
                    }
                    if annotation && matches!(c, '\n' | '\u{2028}' | '\u{2029}' | '\u{0085}') {
                        *f |= BREAK | COLLAPSE_BREAK;
                    } else if c == '\n' && mode != PreserveSpaces {
                        *f |= BREAK;
                        if mode == Collapse {
                            *f |= COLLAPSE_BREAK;
                        }
                    }
                }
                cursor = range.end as usize;
            }
            RawItem::Atomic { .. }
            | RawItem::BlockInInline { .. }
            | RawItem::ForcedBreak { .. }
                if boundaries.last() != Some(&cursor) =>
            {
                boundaries.push(cursor);
            }
            // Out-of-flow items, inline markers and their generated bidi
            // controls do not participate in whitespace neighbor selection.
            _ => {}
        }
    }
    if boundaries.last() != Some(&text.len()) {
        boundaries.push(text.len());
    }
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let segment = &text[start..end];
        let mut pending_spaces = None;
        let mut after_break = false;
        let mut previous = None;
        let mut previous_collapsed_break = false;
        for (i, c) in segment.char_indices() {
            let at = start + i;
            if flags[at] & BREAK != 0 {
                if let Some(space_start) = pending_spaces.take() {
                    // A pending run is drained once; no reverse rescanning
                    // of a growing prefix at each subsequent LF.
                    for (j, _) in text[space_start..at].char_indices() {
                        let f = &mut flags[space_start + j];
                        if *f & COLLAPSE_SPACE != 0 {
                            *f |= REMOVE;
                        }
                    }
                }
                after_break = true;
                if flags[at] & COLLAPSE_BREAK != 0 {
                    if previous_collapsed_break {
                        flags[at] |= REMOVE;
                    }
                    if previous.is_some_and(wide) {
                        flags[at] |= LEFT_WIDE;
                    }
                    if previous == Some('\u{200B}') {
                        flags[at] |= LEFT_ZWSP;
                    }
                    previous_collapsed_break = true;
                } else {
                    previous_collapsed_break = false;
                    previous = None;
                }
            } else if flags[at] & COLLAPSE_SPACE != 0 {
                if after_break {
                    flags[at] |= REMOVE;
                } else {
                    pending_spaces.get_or_insert(at);
                }
            } else if !ignorable(c) {
                pending_spaces = None;
                after_break = false;
                previous_collapsed_break = false;
                previous = Some(if c == '\r' { ' ' } else { c });
            }
        }
        let mut next = None;
        for (i, c) in segment.char_indices().rev() {
            let at = start + i;
            if flags[at] & COLLAPSE_BREAK != 0 {
                if flags[at] & LEFT_ZWSP != 0
                    || next == Some('\u{200B}')
                    || flags[at] & LEFT_WIDE != 0 && next.is_some_and(wide)
                {
                    flags[at] |= REMOVE;
                }
            } else if flags[at] & REMOVE == 0 && !ignorable(c) {
                next = Some(if c == '\r' { ' ' } else { c });
            }
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    #[test]
    fn ignorable_fast_path_matches_the_table_for_every_scalar() {
        use icu_properties::{CodePointSetData, props};
        let table = CodePointSetData::new::<props::DefaultIgnorableCodePoint>();
        for c in ('\0'..=char::MAX).filter(|c| *c != '\u{200B}') {
            assert_eq!(super::ignorable(c), table.contains(c), "{c:?}");
        }
        assert!(!super::ignorable('\u{200B}'));
    }
}
