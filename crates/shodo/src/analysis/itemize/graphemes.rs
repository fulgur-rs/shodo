//! Scalar-index cuts for the actual local shaping input.

use super::Scalar;
use crate::analysis::breaks::BreakAnalysis;
use icu_properties::{CodePointMapData, CodePointSetData, props};

pub(super) fn boundaries(scalars: &[Scalar], breaks: &BreakAnalysis) -> Vec<usize> {
    let Some(first) = scalars.first() else {
        return vec![0];
    };
    let last_end = scalars.last().unwrap().end;
    let lo = breaks.graphemes.partition_point(|cut| *cut <= first.offset);
    let hi = breaks.graphemes.partition_point(|cut| *cut < last_end);
    let shared = breaks.graphemes[lo..hi].iter().map(|cut| {
        #[cfg(test)]
        super::tests::record_shared_cut();
        scalars.partition_point(|s| s.end <= *cut)
    });
    let lo = breaks
        .authored_bidi_controls
        .partition_point(|at| *at < first.offset);
    let hi = breaks
        .authored_bidi_controls
        .partition_point(|at| *at < last_end);
    let controls: Vec<_> = breaks.authored_bidi_controls[lo..hi]
        .iter()
        .filter_map(|at| {
            let i = scalars.partition_point(|s| s.offset < *at);
            scalars.get(i).filter(|s| s.offset == *at).map(|_| i)
        })
        .collect();
    let mut shared = std::iter::once(0)
        .chain(shared)
        .chain(std::iter::once(scalars.len()))
        .peekable();
    let mut control_cuts = controls.iter().flat_map(|i| [*i, *i + 1]).peekable();
    let mut cuts = Vec::new();
    while shared.peek().is_some() || control_cuts.peek().is_some() {
        let next = if control_cuts.peek().is_none()
            || shared
                .peek()
                .is_some_and(|s| Some(s) <= control_cuts.peek())
        {
            shared.next().unwrap()
        } else {
            control_cuts.next().unwrap()
        };
        if cuts.last() != Some(&next) {
            cuts.push(next);
        }
    }

    // Restarting at a complete paragraph grapheme preserves subsequent cuts.
    // Only truncated prefixes need local context; RI parity can extend that
    // disagreement across several paragraph graphemes.
    let gcb = CodePointMapData::<props::GraphemeClusterBreak>::new();
    let mut repaired: Option<Vec<usize>> = None;
    let mut consumed = 0;
    for start in std::iter::once(0).chain(controls.iter().map(|i| i + 1)) {
        if start == scalars.len()
            || breaks
                .typographic_starts
                .binary_search(&scalars[start].offset)
                .is_ok()
        {
            continue;
        }
        let mut end_index = cuts.partition_point(|cut| *cut <= start);
        if gcb.get(scalars[start].c) == props::GraphemeClusterBreak::RegionalIndicator {
            let ri_end = start
                + scalars[start..]
                    .iter()
                    .take_while(|s| gcb.get(s.c) == props::GraphemeClusterBreak::RegionalIndicator)
                    .count();
            end_index = end_index.max(cuts.partition_point(|cut| *cut < ri_end));
        }
        let end = cuts[end_index];
        let output = repaired.get_or_insert_with(|| Vec::with_capacity(cuts.len()));
        let before = cuts.partition_point(|cut| *cut < start);
        output.extend_from_slice(&cuts[consumed..before]);
        repair_prefix(&scalars[start..end], start, output);
        consumed = end_index;
    }
    if let Some(mut output) = repaired {
        output.extend_from_slice(&cuts[consumed..]);
        output
    } else {
        cuts
    }
}

/// UAX29 GB3–GB13 for a truncated prefix only. Paragraph ICU cuts govern
/// every complete grapheme; this state never replaces their segmentation.
fn repair_prefix(scalars: &[Scalar], start: usize, cuts: &mut Vec<usize>) {
    use props::{GraphemeClusterBreak as G, IndicConjunctBreak as I};
    let gcb = CodePointMapData::<G>::new();
    let incb = CodePointMapData::<I>::new();
    let ep = CodePointSetData::new::<props::ExtendedPictographic>();
    let mut previous = None;
    let mut ri_odd = false;
    let mut ep_extend = false;
    let mut zwj_after_ep = false;
    let mut consonant = false;
    let mut linker = false;
    for (i, scalar) in scalars.iter().enumerate() {
        #[cfg(test)]
        super::tests::record_repaired_scalar();
        let current = gcb.get(scalar.c);
        let indic = incb.get(scalar.c);
        let pictographic = ep.contains(scalar.c);
        let control = |c| matches!(c, G::Control | G::CR | G::LF);
        let boundary = previous.is_none_or(|previous| {
            if previous == G::CR && current == G::LF {
                // GB3
                false
            } else if control(previous) || control(current) {
                // GB4/5
                true
            } else if previous == G::L && matches!(current, G::L | G::V | G::LV | G::LVT)
                || matches!(previous, G::LV | G::V) && matches!(current, G::V | G::T)
                || matches!(previous, G::LVT | G::T) && current == G::T
                || matches!(current, G::Extend | G::ZWJ | G::SpacingMark)
                || previous == G::Prepend // GB6–GB9b
                || indic == I::Consonant && consonant && linker // GB9c
                || pictographic && previous == G::ZWJ && zwj_after_ep // GB11
                || previous == G::RegionalIndicator && current == G::RegionalIndicator && ri_odd
            // GB12/13
            {
                false
            } else {
                true // GB999
            }
        });
        if boundary {
            cuts.push(start + i);
        }
        ri_odd =
            current == G::RegionalIndicator && (previous != Some(G::RegionalIndicator) || !ri_odd);
        zwj_after_ep = current == G::ZWJ && ep_extend;
        ep_extend = pictographic || current == G::Extend && ep_extend;
        match indic {
            I::Consonant => {
                consonant = true;
                linker = false;
            }
            I::Linker => linker |= consonant,
            I::Extend => {}
            _ => {
                consonant = false;
                linker = false;
            }
        }
        previous = Some(current);
    }
}

#[cfg(test)]
mod tests {
    use super::{Scalar, boundaries};
    use crate::analysis::breaks::{BreakAnalysis, analyze_breaks};
    use crate::analysis::whitespace::Processed;
    use crate::analysis::{Item, ItemKind};
    use crate::limits::WarningSink;
    use crate::style::InlineStyle;
    use icu_segmenter::GraphemeClusterSegmenter;

    fn prepared(text: &str) -> (Vec<Scalar>, BreakAnalysis) {
        let input = Processed {
            text: text.to_owned(),
            items: vec![Item {
                kind: ItemKind::Text,
                text: 0..text.len() as u32,
                style: 0,
                node: None,
            }],
            mapping: None,
            indivisible: Vec::new(),
            source_spans: Vec::new(),
            width_origins: Vec::new(),
        };
        let breaks = analyze_breaks(
            &input,
            &[InlineStyle::default()],
            &mut WarningSink::default(),
        );
        let scalars = text
            .char_indices()
            .map(|(offset, c)| Scalar {
                c,
                offset: offset as u32,
                end: (offset + c.len_utf8()) as u32,
                item: 0,
                grapheme_start: false,
            })
            .collect();
        (scalars, breaks)
    }

    fn check_substrings(text: &str) {
        let (scalars, breaks) = prepared(text);
        let offsets: Vec<_> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(text.len()))
            .collect();
        for start in 0..offsets.len() {
            for end in start..offsets.len() {
                let local = &text[offsets[start]..offsets[end]];
                let local_offsets: Vec<_> = local.char_indices().map(|(at, _)| at).collect();
                let expected: Vec<_> = super::super::tests::observe_local_boundaries(
                    GraphemeClusterSegmenter::new().segment_str(local),
                )
                .map(|at| local_offsets.partition_point(|offset| *offset < at))
                .collect();
                assert_eq!(
                    boundaries(&scalars[start..end], &breaks),
                    expected,
                    "{text:?} scalar range {start}..{end} ({local:?})"
                );
            }
        }
    }

    #[test]
    fn normative_grapheme_cases_and_every_substring_match_icu() {
        // Original Unicode17 test data, including copyright and license URL.
        // https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt
        let corpus = include_str!("../../../tests/data/GraphemeBreakTest-17.0.0.txt");
        let mut cases = 0;
        for line in corpus.lines() {
            let data = line.split('#').next().unwrap().trim();
            if data.is_empty() {
                continue;
            }
            let mut text = String::new();
            let mut expected = Vec::new();
            for token in data.split_whitespace() {
                match token {
                    "÷" => expected.push(text.len()),
                    "×" => {}
                    scalar => {
                        text.push(char::from_u32(u32::from_str_radix(scalar, 16).unwrap()).unwrap())
                    }
                }
            }
            assert_eq!(
                GraphemeClusterSegmenter::new()
                    .segment_str(&text)
                    .collect::<Vec<_>>(),
                expected,
                "normative input {text:?}"
            );
            check_substrings(&text);
            cases += 1;
        }
        assert_eq!(cases, 766);
    }

    #[test]
    fn generated_context_combinations_match_local_icu() {
        // GB3–GB13 representatives, overlapping Indic/emoji properties,
        // and a control removed by paragraph projection but kept locally.
        let alphabet = [
            'a', '\r', '\n', '\u{301}', '\u{600}', '\u{903}', 'ᄀ', 'ᅡ', 'ᆨ', '가', '각', '🇦', '👩',
            '\u{200d}', 'क', '्', '\u{200e}',
        ];
        for &a in &alphabet {
            for &b in &alphabet {
                for &c in &alphabet {
                    check_substrings(&[a, b, c].into_iter().collect::<String>());
                }
            }
        }
    }

    #[test]
    fn long_restart_chains_match_local_icu() {
        for text in [
            "🇦".repeat(31),
            format!("👩{}", "\u{301}\u{200d}👩".repeat(8)),
            format!("क{}", "\u{301}्\u{200d}क".repeat(8)),
            "🇦\u{200e}🇧🇨🇩\u{200f}🇦🇧🇨".repeat(4),
        ] {
            check_substrings(&text);
        }
    }
}
