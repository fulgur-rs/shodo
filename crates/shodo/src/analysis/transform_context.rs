//! Linear context passes for Unicode SpecialCasing and CSS capitalization.
use icu_properties::{CodePointMapData, CodePointSetData, props};
use icu_segmenter::WordSegmenter;

pub(super) const BEFORE_CASED: u16 = 1;
pub(super) const AFTER_CASED: u16 = 2;
pub(super) const AFTER_I: u16 = 4;
pub(super) const AFTER_SOFT: u16 = 8;
pub(super) const MORE_ABOVE: u16 = 16;
pub(super) const BEFORE_DOT: u16 = 32;
pub(super) const HEAD: u16 = 64;
pub(super) const DUTCH_J: u16 = 128;
pub(super) const MULTI_LETTER: u16 = 256;
pub(super) const AFTER_TONOS: u16 = 512;

pub(super) fn context(text: &str) -> Vec<u16> {
    let mut flags = vec![0; text.len()];
    let cased = CodePointSetData::new::<props::Cased>();
    let case_ignore = CodePointSetData::new::<props::CaseIgnorable>();
    let default_ignore = CodePointSetData::new::<props::DefaultIgnorableCodePoint>();
    let soft = CodePointSetData::new::<props::SoftDotted>();
    let ccc = CodePointMapData::<props::CanonicalCombiningClass>::new();
    let mut before_cased = false;
    let mut after_i = false;
    let mut after_soft = false;
    let mut after_tonos = false;
    for (i, c) in text.char_indices() {
        if before_cased {
            flags[i] |= BEFORE_CASED;
        }
        if after_i {
            flags[i] |= AFTER_I;
        }
        if after_soft {
            flags[i] |= AFTER_SOFT;
        }
        if after_tonos {
            flags[i] |= AFTER_TONOS;
        }
        if !case_ignore.contains(c) {
            before_cased = cased.contains(c);
        }
        if default_ignore.contains(c) {
            continue;
        }
        match ccc.get(c).0 {
            0 => {
                after_i = c == 'I';
                after_soft = soft.contains(c);
            }
            230 => {
                after_i = false;
                after_soft = false;
            }
            _ => {}
        }
        after_tonos = matches!(
            c,
            'ά' | 'έ' | 'ή' | 'ί' | 'ό' | 'ύ' | 'ώ' | 'Ά' | 'Έ' | 'Ή' | 'Ί' | 'Ό' | 'Ύ' | 'Ώ'
        );
    }
    let mut after_cased = false;
    let mut above = false;
    let mut dot = false;
    for (i, c) in text.char_indices().rev() {
        if after_cased {
            flags[i] |= AFTER_CASED;
        }
        if above {
            flags[i] |= MORE_ABOVE;
        }
        if dot {
            flags[i] |= BEFORE_DOT;
        }
        if !case_ignore.contains(c) {
            after_cased = cased.contains(c);
        }
        if default_ignore.contains(c) {
            continue;
        }
        match ccc.get(c).0 {
            0 => {
                above = false;
                dot = false;
            }
            230 => {
                above = true;
                dot = c == '\u{0307}';
            }
            _ => {}
        }
    }
    let category = CodePointMapData::<props::GeneralCategory>::new();
    #[cfg(feature = "complex-scripts")]
    let segmenter = WordSegmenter::new_auto(Default::default());
    #[cfg(not(feature = "complex-scripts"))]
    let segmenter = WordSegmenter::new_for_non_complex_scripts(Default::default());
    let mut begin = 0;
    for end in segmenter.segment_str(text) {
        let word = &text[begin..end];
        let letters = word.chars().filter(|c| c.is_alphabetic()).take(2).count();
        let mut head = None;
        for (i, c) in word.char_indices() {
            let at = begin + i;
            if letters > 1 {
                flags[at] |= MULTI_LETTER;
            }
            let letter_number = props::GeneralCategoryGroup::Letter
                .union(props::GeneralCategoryGroup::Number)
                .contains(category.get(c));
            if head.is_none() && letter_number {
                flags[at] |= HEAD;
                head = Some((c, at + c.len_utf8()));
            } else if let Some(('i' | 'I', next)) = head
                && at == next
                && matches!(c, 'j' | 'J')
            {
                flags[at] |= DUTCH_J;
            }
        }
        begin = end;
    }
    flags
}

#[cfg(all(test, feature = "complex-scripts"))]
mod tests {
    use super::*;

    #[test]
    fn context_word_heads_use_complex_models() {
        // These flags are consumed by CSS capitalization. Checking literal
        // word heads also catches missing models when ICU logging is disabled.
        for (text, expected) in [
            ("こんにちは世界", &[0, 15][..]),
            ("ภาษาไทย", &[0, 12][..]),
            ("ភាសាខ្មែរជាភាសាជាតិ", &[0, 12, 27, 33][..]),
        ] {
            let flags = context(text);
            let heads: Vec<_> = text
                .char_indices()
                .filter_map(|(at, _)| (flags[at] & HEAD != 0).then_some(at))
                .collect();
            assert_eq!(heads, expected, "{text:?}");
        }
    }
}
