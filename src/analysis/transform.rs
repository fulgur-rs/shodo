//! Scalar-origin-preserving CSS transforms, after IFC whitespace processing.
use super::transform_context::*;
use super::whitespace::Processed;
use crate::analysis::ItemKind;
use crate::limits::{LimitExceeded, LimitKind, Limits, WarningKind, WarningSink};
use crate::mapping::{MappingKind, TransformSpan};
use crate::style::{CaseTransform, InlineStyle, TextTransform};
use icu_casemap::CaseMapper;
use icu_locale_core::{LanguageIdentifier, Locale};
use icu_normalizer::ComposingNormalizer;

pub(crate) struct WidthOrigin {
    pub(crate) text: std::ops::Range<u32>,
    pub(crate) before_width: String,
}

pub(crate) fn transform(
    mut input: Processed,
    styles: &[InlineStyle],
    limits: &Limits,
    warnings: &mut WarningSink,
    mode: crate::geometry::WritingMode,
) -> Result<Processed, LimitExceeded> {
    let omissions = super::combine::omissions(&input, styles, mode);
    if styles
        .iter()
        .all(|s| s.text_transform == TextTransform::None)
        && omissions.is_empty()
    {
        Limits::check(
            limits.max_text_bytes,
            LimitKind::TextBytes,
            input.text.len() as u64,
        )?;
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
    let flags = context(&logical);
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
            let style = &styles[item.style as usize];
            let (case, width, kana) = style.text_transform.components();
            let locale = &locales[item.style as usize];
            let omit = omissions
                .get(omissions.partition_point(|range| range.end <= at as u32))
                .is_some_and(|range| range.start <= at as u32);
            if flags[at] & HEAD != 0 {
                dutch_title_head = matches!(item.kind, ItemKind::Text)
                    && matches!(case, CaseTransform::Capitalize)
                    && locale.language.as_str() == "nl"
                    && matches!(c, 'i' | 'I');
            }
            let mut mapped = if omit || consumed_mark == Some(at) {
                String::new()
            } else if matches!(item.kind, ItemKind::Text) {
                let mut buf = [0; 4];
                let scalar = c.encode_utf8(&mut buf);
                match case {
                    CaseTransform::None => scalar.to_owned(),
                    CaseTransform::Lowercase => {
                        if c == 'Σ' && flags[at] & BEFORE_CASED != 0 && flags[at] & AFTER_CASED == 0
                        {
                            "ς".to_owned()
                        } else if matches!(locale.language.as_str(), "tr" | "az")
                            && c == 'I'
                            && flags[at] & BEFORE_DOT != 0
                        {
                            "i".to_owned()
                        } else if matches!(locale.language.as_str(), "tr" | "az")
                            && c == '\u{0307}'
                            && flags[at] & AFTER_I != 0
                        {
                            String::new()
                        } else if locale.language.as_str() == "lt"
                            && matches!(c, 'I' | 'J' | '\u{012E}')
                            && flags[at] & MORE_ABOVE != 0
                        {
                            format!("{}\u{0307}", cm.lowercase_to_string(scalar, locale))
                        } else {
                            cm.lowercase_to_string(scalar, locale).into_owned()
                        }
                    }
                    CaseTransform::Uppercase => {
                        if locale.language.as_str() == "lt"
                            && c == '\u{0307}'
                            && flags[at] & AFTER_SOFT != 0
                        {
                            String::new()
                        } else {
                            cm.uppercase_to_string(
                                scalar,
                                if locale.language.as_str() == "el" && flags[at] & MULTI_LETTER == 0
                                {
                                    &root_locale
                                } else {
                                    locale
                                },
                            )
                            .into_owned()
                        }
                    }
                    CaseTransform::Capitalize => {
                        if flags[at] & HEAD != 0
                            || locale.language.as_str() == "nl"
                                && dutch_title_head
                                && flags[at] & DUTCH_J != 0
                        {
                            cm.titlecase_segment_with_only_case_data_to_string(
                                scalar,
                                locale,
                                Default::default(),
                            )
                            .into_owned()
                        } else {
                            scalar.to_owned()
                        }
                    }
                }
            } else {
                c.to_string()
            };
            if matches!(item.kind, ItemKind::Text)
                && matches!(case, CaseTransform::Uppercase)
                && locale.language.as_str() == "el"
                && flags[at] & MULTI_LETTER != 0
            {
                mapped = mapped
                    .chars()
                    .filter(|c| *c != '\u{0301}')
                    .map(remove_tonos)
                    .collect();
                if flags[at] & AFTER_TONOS != 0 {
                    mapped = mapped
                        .chars()
                        .map(|c| match c {
                            'Ι' => 'Ϊ',
                            'Υ' => 'Ϋ',
                            _ => c,
                        })
                        .collect();
                }
            }
            let mut before_width = None;
            if matches!(item.kind, ItemKind::Text) {
                if kana {
                    mapped = mapped.chars().map(full_size_kana).collect();
                }
                if width {
                    if style.text_combine_upright == crate::style::TextCombineUpright::All {
                        before_width = Some(mapped.clone());
                    }
                    mapped = mapped.chars().map(full_width).collect();
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
                    mapped = composed.into_owned();
                    consumed_mark = Some(mark_at);
                }
            }
            let next_len = output.len() as u64 + mapped.len() as u64;
            Limits::check(Some(u64::from(u32::MAX)), LimitKind::TextBytes, next_len)?;
            Limits::check(limits.max_text_bytes, LimitKind::TextBytes, next_len)?;
            let start_new = output.len() as u32;
            output.push_str(&mapped);
            if let Some(before_width) = before_width
                && before_width != mapped
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

fn full_width(c: char) -> char {
    match c {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
        '\u{FF61}'..='\u{FF9F}' => "。「」、・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン\u{3099}\u{309A}".chars().nth(c as usize - 0xFF61).unwrap_or(c),
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
