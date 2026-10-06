//! The text of a `::first-letter` pseudo-element.

use std::ops::Range;

use icu_properties::CodePointMapData;
use icu_properties::props::{BidiClass, GeneralCategory};
use icu_segmenter::GraphemeClusterSegmenter;

/// Byte range of the `::first-letter` text at the start of `text`
/// (CSS Pseudo-Elements 4 §3.2), or `None` when the text has none.
///
/// The range covers the first typographic letter unit (one extended grapheme
/// cluster) with its preceding and following punctuation, and the spaces
/// between them. White space before it is outside the range. This follows
/// Chromium's rules, which the spec leaves partly open:
///
/// - Preceding punctuation is Pc, Pd, Ps, Pe, Pi, Pf or Po. Zs spaces other
///   than U+3000 can follow it, before the letter.
/// - Following punctuation is Pc, Pe, Pi, Pf or Po. Zs spaces other than
///   U+0020, U+00A0, U+3000 and a few word separators can sit between the
///   letter and following punctuation; trailing spaces are never included.
/// - Other white space after the preceding punctuation, such as a line feed
///   or U+3000, ends the search with `None`. Text that is only punctuation is
///   returned whole.
///
/// The caller supplies the text that starts the block's first formatted line,
/// in content order and after white-space processing, and splits the returned
/// range into its own inline box with the pseudo-element's style. Floated
/// first letters (drop caps) and `initial-letter` are box-tree work for the
/// caller. Pass `preserve_breaks` when `white-space` preserves segment breaks:
/// then a leading line feed is not skipped, so the first line has no letter.
///
/// Chromium also styles the skipped leading white space with the
/// pseudo-element; use `0..range.end` to match that.
///
/// The scan stops at the end of the first letter, so its cost is bounded by
/// the prefix it examines, and it allocates nothing.
///
/// ```
/// let text = "  \u{201C}Call me Ishmael.\u{201D}";
/// let range = shodo::first_letter_range(text, false).unwrap();
/// assert_eq!(&text[range], "\u{201C}C");
/// assert_eq!(shodo::first_letter_range("\u{201C}\u{201D}", false), Some(0..6));
/// assert_eq!(shodo::first_letter_range(" ", false), None);
/// ```
pub fn first_letter_range(text: &str, preserve_breaks: bool) -> Option<Range<usize>> {
    let categories = CodePointMapData::<GeneralCategory>::new();
    let bidi = CodePointMapData::<BidiClass>::new();
    let space = |ch: char| {
        if preserve_breaks && is_new_line(ch) {
            return false;
        }
        let space_or_new_line = if ch.is_ascii() {
            ch == ' ' || ('\t'..='\r').contains(&ch)
        } else {
            bidi.get(ch) == BidiClass::WhiteSpace
        };
        space_or_new_line || ch == '\u{a0}'
    };
    let start = text
        .char_indices()
        .find(|&(_, ch)| !space(ch))
        .map(|(index, _)| index)?;

    use GeneralCategory as G;
    let preceding_punctuation = |ch| {
        matches!(
            categories.get(ch),
            G::ConnectorPunctuation
                | G::DashPunctuation
                | G::OpenPunctuation
                | G::ClosePunctuation
                | G::InitialPunctuation
                | G::FinalPunctuation
                | G::OtherPunctuation
        )
    };
    let following_punctuation = |ch| {
        matches!(
            categories.get(ch),
            G::ConnectorPunctuation
                | G::ClosePunctuation
                | G::InitialPunctuation
                | G::FinalPunctuation
                | G::OtherPunctuation
        )
    };
    let separator = |ch| categories.get(ch) == G::SpaceSeparator && ch != '\u{3000}';
    let following_separator = |ch| {
        separator(ch)
            && !matches!(
                ch,
                ' ' | '\u{a0}' | '\u{1361}' | '\u{10100}' | '\u{10101}' | '\u{1039f}' | '\u{1091f}'
            )
    };

    let rest = &text[start..];
    // Grapheme boundaries after the first, each paired with the scalar that
    // starts the cluster ending there.
    let mut boundaries = GraphemeClusterSegmenter::new().segment_str(rest).skip(1);
    let mut cluster_start = 0;
    let mut next = || {
        let end = boundaries.next()?;
        let first = rest[cluster_start..].chars().next()?;
        let cluster = (cluster_start, first, end);
        cluster_start = end;
        Some(cluster)
    };

    let mut cluster = next();
    let mut punctuated = false;
    while let Some((_, ch, end)) = cluster {
        if !(preceding_punctuation(ch) || (punctuated && separator(ch))) {
            break;
        }
        punctuated = true;
        cluster = next();
        if cluster.is_none() {
            return Some(start..start + end);
        }
    }
    let (_, ch, mut end) = cluster?;
    if space(ch) || is_new_line(ch) {
        return None;
    }
    // Spaces before following punctuation join only when that punctuation
    // does.
    while let Some((_, ch, cluster_end)) = next() {
        if following_punctuation(ch) {
            end = cluster_end;
        } else if !following_separator(ch) {
            break;
        }
    }
    Some(start..start + end)
}

fn is_new_line(ch: char) -> bool {
    ch == '\n' || ch == '\r'
}

#[cfg(test)]
mod tests {
    use super::first_letter_range;

    fn letter(text: &str) -> Option<&str> {
        first_letter_range(text, false).map(|range| &text[range])
    }

    #[test]
    fn takes_one_grapheme() {
        assert_eq!(letter("Hello"), Some("H"));
        assert_eq!(letter("é tait"), Some("é"));
        assert_eq!(letter("e\u{301}tait"), Some("e\u{301}"));
        assert_eq!(
            letter("\u{1F469}\u{200D}\u{1F4BB} code"),
            Some("\u{1F469}\u{200D}\u{1F4BB}")
        );
        assert_eq!(letter("123"), Some("1"));
        assert_eq!(letter("日本語"), Some("日"));
        assert_eq!(letter(""), None);
    }

    #[test]
    fn skips_leading_white_space() {
        let text = " \t\n\u{a0}\u{2003}Word";
        assert_eq!(
            first_letter_range(text, false),
            Some(text.len() - 4..text.len() - 3)
        );
        assert_eq!(letter("   "), None);
        // Preserved segment breaks leave the letter on a later line.
        assert_eq!(first_letter_range("\nWord", true), None);
        assert_eq!(first_letter_range("  Word", true), Some(2..3));
    }

    #[test]
    fn includes_surrounding_punctuation() {
        assert_eq!(letter("\u{201C}Quoted\u{201D}"), Some("\u{201C}Q"));
        assert_eq!(letter("(a) item"), Some("(a)"));
        assert_eq!(letter("\u{BF}Qu\u{E9}?"), Some("\u{BF}Q"));
        assert_eq!(letter("-- dash"), Some("-- d"));
        assert_eq!(letter("--x"), Some("--x"));
        assert_eq!(letter("「日本」"), Some("「日"));
        assert_eq!(letter("A.B"), Some("A."));
        // Opening and dash punctuation do not follow the letter.
        assert_eq!(letter("A(b"), Some("A"));
        assert_eq!(letter("A-b"), Some("A"));
    }

    #[test]
    fn spaces_inside_punctuation() {
        // Spaces may separate preceding punctuation marks.
        assert_eq!(letter("\u{201C} \u{2018}A"), Some("\u{201C} \u{2018}A"));
        assert_eq!(letter("\u{201C} A"), Some("\u{201C} A"));
        // A line feed or ideographic space after preceding punctuation means
        // there is no letter.
        assert_eq!(letter("\u{201C}\nA"), None);
        assert_eq!(letter("\u{300C}\u{3000}A"), None);
        // Narrow spaces before following punctuation join it, but trailing
        // spaces and ordinary spaces do not.
        assert_eq!(letter("A\u{2009}!"), Some("A\u{2009}!"));
        assert_eq!(letter("A\u{2009}b"), Some("A"));
        assert_eq!(letter("A !"), Some("A"));
        assert_eq!(letter("A\u{3000}」"), Some("A"));
    }

    #[test]
    fn punctuation_only_text_is_whole() {
        assert_eq!(letter("..."), Some("..."));
        assert_eq!(letter(" \u{201C}\u{201D}"), Some("\u{201C}\u{201D}"));
    }

    #[test]
    fn stops_at_the_first_letter() {
        let long = format!("A{}", "b".repeat(1 << 20));
        assert_eq!(first_letter_range(&long, false), Some(0..1));
    }
}
