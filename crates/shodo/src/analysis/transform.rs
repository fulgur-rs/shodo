//! Scalar-origin-preserving CSS transforms, after IFC whitespace processing.
use super::transform_context::*;
use super::whitespace::Processed;
use crate::analysis::ItemKind;
use crate::limits::{LimitExceeded, LimitKind, Limits, WarningKind, WarningSink};
use crate::mapping::{MappingKind, TransformSpan};
use crate::style::{CaseTransform, InlineStyle, TextTransform, WordSpaceTransform};
use icu_casemap::CaseMapper;
use icu_locale_core::{LanguageIdentifier, Locale};
use icu_normalizer::ComposingNormalizer;
use std::borrow::Cow;

pub(crate) struct WidthOrigin {
    pub(crate) text: std::ops::Range<u32>,
    pub(crate) before_width: String,
}

pub(crate) fn transform(
    input: Processed,
    styles: &[InlineStyle],
    limits: &Limits,
    warnings: &mut WarningSink,
    mode: crate::geometry::WritingMode,
) -> Result<Processed, LimitExceeded> {
    transform_inner(input, styles, limits, warnings, mode, None)
}

pub(crate) fn transform_with_base_scopes(
    input: Processed,
    styles: &[InlineStyle],
    limits: &Limits,
    warnings: &mut WarningSink,
    mode: crate::geometry::WritingMode,
    bases: Option<&mut crate::ruby::base_budget::BaseScopes>,
) -> Result<Processed, LimitExceeded> {
    if bases.is_some() {
        transform_inner(input, styles, limits, warnings, mode, bases)
    } else {
        transform(input, styles, limits, warnings, mode)
    }
}

fn transform_inner(
    mut input: Processed,
    styles: &[InlineStyle],
    limits: &Limits,
    warnings: &mut WarningSink,
    mode: crate::geometry::WritingMode,
    mut bases: Option<&mut crate::ruby::base_budget::BaseScopes>,
) -> Result<Processed, LimitExceeded> {
    let omissions = super::combine::omissions(&input, styles, mode);
    if styles
        .iter()
        .all(|s| s.text_transform == TextTransform::None)
        && styles
            .iter()
            .all(|s| s.word_space_transform == WordSpaceTransform::None)
        && omissions.is_empty()
    {
        Limits::check(
            limits.max_text_bytes,
            LimitKind::TextBytes,
            input.text.len() as u64,
        )?;
        if let Some(bases) = &mut bases {
            for (i, item) in input.items.iter().enumerate() {
                bases.item(
                    i,
                    LimitKind::TextBytes,
                    u64::from(item.text.end - item.text.start),
                )?;
            }
        }
        return Ok(input);
    }
    let locales: Vec<LanguageIdentifier> = styles
        .iter()
        .map(
            |s| match s.lang.as_deref().map(str::parse::<Locale>).transpose() {
                Ok(Some(lang)) => lang.id,
                Ok(None) => LanguageIdentifier::UNKNOWN,
                Err(_) => {
                    warnings.push(
                        WarningKind::Unsupported,
                        "invalid transform language; using root locale",
                    );
                    LanguageIdentifier::UNKNOWN
                }
            },
        )
        .collect();
    for style in styles {
        if matches!(
            style.word_space_transform,
            WordSpaceTransform::SpaceAutoPhrase | WordSpaceTransform::IdeographicSpaceAutoPhrase
        ) {
            warnings.push(
                WarningKind::Unsupported,
                "automatic phrase segmentation unavailable; transforming explicit zero-width spaces only",
            );
        }
    }
    let needs_case = styles
        .iter()
        .any(|s| !matches!(s.text_transform.components().0, CaseTransform::None));
    let flags = needs_case.then(|| {
        // OOF and generated bidi controls must not split words/casing context.
        // WJ is a same-byte-length Format character ignored by UAX29.
        let mut logical = input.text.clone();
        for item in &input.items {
            if matches!(
                item.kind,
                ItemKind::OutOfFlow { .. } | ItemKind::BidiControl
            ) {
                let start = item.text.start as usize;
                let end = item.text.end as usize;
                if end - start == 3 {
                    logical.replace_range(start..end, "\u{2060}");
                }
            }
        }
        context(&logical)
    });
    let cm = CaseMapper::new();
    let root_locale = LanguageIdentifier::UNKNOWN;
    let mut output = String::new();
    let mut spans: Vec<TransformSpan> = Vec::new();
    let mut consumed_mark = None;
    let mut dutch_title_head = false;
    let mut width_origins = Vec::new();
    for (index, item) in input.items.iter().enumerate() {
        for (offset, c) in
            input.text[item.text.start as usize..item.text.end as usize].char_indices()
        {
            let at = item.text.start as usize + offset;
            let end = at + c.len_utf8();
            let flags_at = flags.as_ref().map_or(0, |flags| flags[at]);
            let style = &styles[item.style as usize];
            let (case, width, kana) = style.text_transform.components();
            let locale = &locales[item.style as usize];
            let omit = omissions
                .get(omissions.partition_point(|range| range.end <= at as u32))
                .is_some_and(|range| range.start <= at as u32);
            if flags_at & HEAD != 0 {
                dutch_title_head = matches!(item.kind, ItemKind::Text)
                    && matches!(case, CaseTransform::Capitalize)
                    && locale.language.as_str() == "nl"
                    && matches!(c, 'i' | 'I');
            }
            let mut scalar_buf = [0; 4];
            let scalar = c.encode_utf8(&mut scalar_buf);
            let mut mapped: Cow<'_, str> = if omit || consumed_mark == Some(at) {
                Cow::Borrowed("")
            } else if matches!(item.kind, ItemKind::Text) {
                match case {
                    CaseTransform::None => Cow::Borrowed(scalar),
                    CaseTransform::Lowercase => {
                        if c == 'Σ' && flags_at & BEFORE_CASED != 0 && flags_at & AFTER_CASED == 0
                        {
                            Cow::Borrowed("ς")
                        } else if matches!(locale.language.as_str(), "tr" | "az")
                            && c == 'I'
                            && flags_at & BEFORE_DOT != 0
                        {
                            Cow::Borrowed("i")
                        } else if matches!(locale.language.as_str(), "tr" | "az")
                            && c == '\u{0307}'
                            && flags_at & AFTER_I != 0
                        {
                            Cow::Borrowed("")
                        } else if locale.language.as_str() == "lt"
                            && matches!(c, 'I' | 'J' | '\u{012E}')
                            && flags_at & MORE_ABOVE != 0
                        {
                            format!("{}\u{0307}", cm.lowercase_to_string(scalar, locale)).into()
                        } else {
                            cm.lowercase_to_string(scalar, locale)
                        }
                    }
                    CaseTransform::Uppercase => {
                        if locale.language.as_str() == "lt"
                            && c == '\u{0307}'
                            && flags_at & AFTER_SOFT != 0
                        {
                            Cow::Borrowed("")
                        } else {
                            cm.uppercase_to_string(
                                scalar,
                                if locale.language.as_str() == "el" && flags_at & MULTI_LETTER == 0
                                {
                                    &root_locale
                                } else {
                                    locale
                                },
                            )
                        }
                    }
                    CaseTransform::Capitalize => {
                        if flags_at & HEAD != 0
                            || locale.language.as_str() == "nl"
                                && dutch_title_head
                                && flags_at & DUTCH_J != 0
                        {
                            cm.titlecase_segment_with_only_case_data_to_string(
                                scalar,
                                locale,
                                Default::default(),
                            )
                        } else {
                            Cow::Borrowed(scalar)
                        }
                    }
                }
            } else {
                Cow::Borrowed(scalar)
            };
            if matches!(item.kind, ItemKind::Text)
                && matches!(case, CaseTransform::Uppercase)
                && locale.language.as_str() == "el"
                && flags_at & MULTI_LETTER != 0
            {
                mapped = mapped
                    .chars()
                    .filter(|c| *c != '\u{0301}')
                    .map(remove_tonos)
                    .collect::<String>()
                    .into();
                if flags_at & AFTER_TONOS != 0 {
                    mapped = mapped
                        .chars()
                        .map(|c| match c {
                            'Ι' => 'Ϊ',
                            'Υ' => 'Ϋ',
                            _ => c,
                        })
                        .collect::<String>()
                        .into();
                }
            }
            let mut before_width = None;
            let mut kana_buf = [0; 4];
            let mut width_buf = [0; 4];
            if matches!(item.kind, ItemKind::Text) {
                if kana {
                    mapped = map_chars(&mapped, &mut kana_buf, full_size_kana);
                }
                if width {
                    if style.text_combine_upright == crate::style::TextCombineUpright::All {
                        before_width = Some(mapped.to_string());
                    }
                    mapped = map_chars(&mapped, &mut width_buf, full_width);
                }
            }
            if matches!(item.kind, ItemKind::Text)
                && width
                && !mapped.is_empty()
                && let Some((mark_at, mark, mark_style)) = next_scalar(&input, index, end)
                && matches!(mark, '\u{FF9E}' | '\u{FF9F}')
                && styles[mark_style as usize].text_transform.components().1
            {
                let sequence = format!("{mapped}{}", full_width(mark));
                let composed = ComposingNormalizer::new_nfc().normalize(&sequence);
                if composed.chars().count() == 1 {
                    if let Some(original) = &mut before_width {
                        original.push(mark);
                    }
                    mapped = composed.into_owned().into();
                    consumed_mark = Some(mark_at);
                }
            }
            let width_changed = before_width
                .as_deref()
                .is_some_and(|before| before != mapped.as_ref());
            if matches!(item.kind, ItemKind::Text) && !omit && c == '\u{200b}' {
                mapped = match style.word_space_transform {
                    WordSpaceTransform::None => mapped,
                    WordSpaceTransform::Space | WordSpaceTransform::SpaceAutoPhrase => " ".into(),
                    WordSpaceTransform::IdeographicSpace
                    | WordSpaceTransform::IdeographicSpaceAutoPhrase => "\u{3000}".into(),
                };
            }
            let next_len = output.len() as u64 + mapped.len() as u64;
            Limits::check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, next_len)?;
            Limits::check(limits.max_text_bytes, LimitKind::TextBytes, next_len)?;
            if let Some(bases) = &mut bases {
                bases.item(index, LimitKind::TextBytes, mapped.len() as u64)?;
            }
            let start_new = output.len() as u32;
            output.push_str(&mapped);
            // Width reversion applies only to text-transform's width change;
            // a later word-space substitution must not be reverted to ZWSP.
            if let Some(before_width) = before_width
                && width_changed
                && !mapped.is_empty()
            {
                width_origins.push(WidthOrigin {
                    text: start_new..output.len() as u32,
                    before_width,
                });
            }
            let kind = if mapped.is_empty() {
                MappingKind::Collapsed
            } else if mapped.len() == c.len_utf8() && mapped.chars().count() == 1 {
                MappingKind::Identity
            } else {
                MappingKind::Expanded
            };
            let span = TransformSpan {
                old: at as u32..end as u32,
                new: start_new..output.len() as u32,
                kind,
            };
            if let Some(last) = spans.last_mut()
                && kind == MappingKind::Identity
                && last.kind == kind
                && last.old.end == span.old.start
                && last.new.end == span.new.start
            {
                last.old.end = span.old.end;
                last.new.end = span.new.end;
            } else {
                spans.push(span);
            }
        }
    }
    for item in &mut input.items {
        item.text = TransformSpan::map_position(&spans, item.text.start)
            ..TransformSpan::map_position(&spans, item.text.end);
        if matches!(item.kind, ItemKind::ForcedBreak) && item.text.is_empty() {
            item.kind = ItemKind::BidiControl;
        }
    }
    if let Some(mapping) = &mut input.mapping {
        mapping.remap_text(&spans);
    }
    input.indivisible = spans
        .iter()
        .filter(|span| span.kind == MappingKind::Expanded)
        .map(|span| span.new.clone())
        .collect();
    input.text = output;
    input.source_spans = spans;
    input.width_origins = width_origins;
    Ok(input)
}

// Width/kana maps preserve scalar count. A single scalar fits in the caller's
// UTF-8 buffer; casing expansions still use an owned string when needed.
fn map_chars<'a>(text: &str, buf: &'a mut [u8; 4], map: fn(char) -> char) -> Cow<'a, str> {
    let mut chars = text.chars();
    match chars.next() {
        None => Cow::Borrowed(""),
        Some(c) if chars.next().is_none() => Cow::Borrowed(map(c).encode_utf8(buf)),
        Some(_) => Cow::Owned(text.chars().map(map).collect()),
    }
}

fn next_scalar(input: &Processed, item_index: usize, end: usize) -> Option<(usize, char, u32)> {
    let item = &input.items[item_index];
    if end < item.text.end as usize {
        return input.text[end..]
            .chars()
            .next()
            .map(|c| (end, c, item.style));
    }
    // Called only at the end of a text item. Each intervening marker group
    // is visited once, so many empty inline elements do not cause repeated
    // scanning of a growing prefix.
    for next in &input.items[item_index + 1..] {
        match next.kind {
            ItemKind::Text if !next.text.is_empty() => {
                let at = next.text.start as usize;
                return input.text[at..].chars().next().map(|c| (at, c, next.style));
            }
            ItemKind::Text
            | ItemKind::OpenInline { .. }
            | ItemKind::CloseInline
            | ItemKind::BidiControl
            | ItemKind::OutOfFlow { .. } => {}
            _ => return None,
        }
    }
    None
}

fn remove_tonos(c: char) -> char {
    match c {
        'Ά' => 'Α',
        'Έ' => 'Ε',
        'Ή' => 'Η',
        'Ί' => 'Ι',
        'Ό' => 'Ο',
        'Ύ' => 'Υ',
        'Ώ' => 'Ω',
        _ => c,
    }
}

// CSS Text 3 reverses Unicode <wide> decompositions and follows <narrow>
// decompositions. Compatibility normalization would change unrelated symbols
// and map halfwidth Hangul past its required compatibility-jamo target.
fn full_width(c: char) -> char {
    match c {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
        '\u{2985}' => '\u{FF5F}',
        '\u{2986}' => '\u{FF60}',
        '¢' => '\u{FFE0}',
        '£' => '\u{FFE1}',
        '¬' => '\u{FFE2}',
        '¯' => '\u{FFE3}',
        '¦' => '\u{FFE4}',
        '¥' => '\u{FFE5}',
        '₩' => '\u{FFE6}',
        '\u{FF61}'..='\u{FF9F}' => "。「」、・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン\u{3099}\u{309A}".chars().nth(c as usize - 0xFF61).unwrap_or(c),
        '\u{FFA0}' => '\u{3164}',
        '\u{FFA1}'..='\u{FFBE}' => char::from_u32(c as u32 - 0xFFA1 + 0x3131).unwrap_or(c),
        '\u{FFC2}'..='\u{FFC7}' => char::from_u32(c as u32 - 0xFFC2 + 0x314F).unwrap_or(c),
        '\u{FFCA}'..='\u{FFCF}' => char::from_u32(c as u32 - 0xFFCA + 0x3155).unwrap_or(c),
        '\u{FFD2}'..='\u{FFD7}' => char::from_u32(c as u32 - 0xFFD2 + 0x315B).unwrap_or(c),
        '\u{FFDA}'..='\u{FFDC}' => char::from_u32(c as u32 - 0xFFDA + 0x3161).unwrap_or(c),
        '\u{FFE8}' => '│',
        '\u{FFE9}' => '←',
        '\u{FFEA}' => '↑',
        '\u{FFEB}' => '→',
        '\u{FFEC}' => '↓',
        '\u{FFED}' => '■',
        '\u{FFEE}' => '○',
        _ => c,
    }
}

fn full_size_kana(c: char) -> char {
    match c {
        'ぁ' => 'あ',
        'ぃ' => 'い',
        'ぅ' => 'う',
        'ぇ' => 'え',
        'ぉ' => 'お',
        'ゕ' => 'か',
        'ゖ' => 'け',
        'っ' => 'つ',
        'ゃ' => 'や',
        'ゅ' => 'ゆ',
        'ょ' => 'よ',
        'ゎ' => 'わ',
        'ァ' => 'ア',
        'ィ' => 'イ',
        'ゥ' => 'ウ',
        'ェ' => 'エ',
        'ォ' => 'オ',
        'ヵ' => 'カ',
        'ㇰ' => 'ク',
        'ヶ' => 'ケ',
        'ㇱ' => 'シ',
        'ㇲ' => 'ス',
        'ッ' => 'ツ',
        'ㇳ' => 'ト',
        'ㇴ' => 'ヌ',
        'ㇵ' => 'ハ',
        'ㇶ' => 'ヒ',
        'ㇷ' => 'フ',
        'ㇸ' => 'ヘ',
        'ㇹ' => 'ホ',
        'ㇺ' => 'ム',
        'ャ' => 'ヤ',
        'ュ' => 'ユ',
        'ョ' => 'ヨ',
        'ㇻ' => 'ラ',
        'ㇼ' => 'リ',
        'ㇽ' => 'ル',
        'ㇾ' => 'レ',
        'ㇿ' => 'ロ',
        'ヮ' => 'ワ',
        'ｧ' => 'ｱ',
        'ｨ' => 'ｲ',
        'ｩ' => 'ｳ',
        'ｪ' => 'ｴ',
        'ｫ' => 'ｵ',
        'ｯ' => 'ﾂ',
        'ｬ' => 'ﾔ',
        'ｭ' => 'ﾕ',
        'ｮ' => 'ﾖ',
        '\u{1B132}' => 'こ',
        '\u{1B150}' => 'ゐ',
        '\u{1B151}' => 'ゑ',
        '\u{1B152}' => 'を',
        '\u{1B155}' => 'コ',
        '\u{1B164}' => 'ヰ',
        '\u{1B165}' => 'ヱ',
        '\u{1B166}' => 'ヲ',
        '\u{1B167}' => 'ン',
        _ => c,
    }
}

#[cfg(test)]
mod work_tests {
    use super::*;
    use crate::analysis::Item;
    use crate::geometry::WritingMode;

    fn input(text: &str) -> Processed {
        Processed {
            text: text.into(),
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
        }
    }

    #[test]
    fn non_case_transforms_skip_context_without_skipping_locale_warnings_or_limits() {
        for (text, transform, word, expected) in [
            (
                "a ｶﾞ",
                TextTransform::FullWidth,
                WordSpaceTransform::None,
                "ａ　ガ",
            ),
            (
                "ぁｧ",
                TextTransform::FullSizeKana,
                WordSpaceTransform::None,
                "あｱ",
            ),
            (
                "ぁｧ",
                TextTransform::FullWidthFullSizeKana,
                WordSpaceTransform::None,
                "あア",
            ),
            (
                "a\u{200b}b",
                TextTransform::None,
                WordSpaceTransform::Space,
                "a b",
            ),
        ] {
            let style = InlineStyle {
                text_transform: transform,
                word_space_transform: word,
                ..Default::default()
            };
            CONTEXT_CALLS.with(|count| count.set(0));
            let p = transform_inner(
                input(text),
                &[style],
                &Limits::default(),
                &mut WarningSink::default(),
                WritingMode::HorizontalTb,
                None,
            )
            .unwrap();
            assert_eq!(p.text, expected);
            assert_eq!(
                CONTEXT_CALLS.with(|count| count.get()),
                0,
                "non-case transform performed casing analysis"
            );
        }
        let style = InlineStyle {
            lang: Some("!invalid".into()),
            word_space_transform: WordSpaceTransform::SpaceAutoPhrase,
            ..Default::default()
        };
        let mut warnings = WarningSink::new(Some(1));
        let limits = Limits {
            max_text_bytes: Some(2),
            ..Limits::default()
        };
        let error = transform_inner(
            input("a\u{200b}b"),
            &[style],
            &limits,
            &mut warnings,
            WritingMode::HorizontalTb,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(error.kind, LimitKind::TextBytes);
        assert_eq!(error.actual, 3);
        let warnings = warnings.take();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].kind, WarningKind::Unsupported);
        assert!(
            warnings[0]
                .message
                .starts_with("invalid transform language")
        );
        assert_eq!(warnings[1].kind, WarningKind::Suppressed);
    }

    #[test]
    fn case_context_still_crosses_non_case_items() {
        let mut p = input("ΟΣ");
        p.items[0].text.end = 2;
        p.items.push(Item {
            kind: ItemKind::Text,
            text: 2..4,
            style: 1,
            node: None,
        });
        let styles = [
            InlineStyle::default(),
            InlineStyle {
                text_transform: TextTransform::Lowercase,
                ..Default::default()
            },
        ];
        let p = transform_inner(
            p,
            &styles,
            &Limits::default(),
            &mut WarningSink::default(),
            WritingMode::HorizontalTb,
            None,
        )
        .unwrap();
        assert_eq!(p.text, "Ος");
    }
}

#[cfg(test)]
mod width_tests {
    use super::full_width;

    #[test]
    fn full_width_matches_all_unicode_width_decompositions() {
        let mut count = 0;
        for line in include_str!("../../tests/data/FullWidth-17.0.0.txt").lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let (source, target) = line.split_once(';').unwrap();
            let scalar =
                |s: &str| char::from_u32(u32::from_str_radix(s.trim(), 16).unwrap()).unwrap();
            let source = scalar(source);
            assert_eq!(
                full_width(source),
                scalar(target),
                "U+{:04X}",
                source as u32
            );
            count += 1;
        }
        assert_eq!(count, 226);
    }

    #[test]
    fn full_width_preserves_unmapped_characters_and_reserved_hangul_holes() {
        for c in [
            '\u{ffbf}', '\u{ffc0}', '\u{ffc1}', '\u{ffc8}', '\u{ffc9}', '\u{ffd0}', '\u{ffd1}',
            '\u{ffd8}', '\u{ffd9}', '\u{ffdd}', 'ﬀ', '①', '㎏', 'é', '😀', '\u{3000}', '\u{3131}',
            '\u{ff5f}',
        ] {
            assert_eq!(full_width(c), c, "U+{:04X}", c as u32);
        }
    }
}
