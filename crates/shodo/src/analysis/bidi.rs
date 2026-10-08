//! UAX #9 paragraphs and CSS plaintext line direction. Byte offsets stay unchanged.
use super::{ItemKind, whitespace::Processed};
use crate::geometry::{Direction, WritingMode};
use crate::style::{InlineStyle, ParagraphStyle, TextOrientation, UnicodeBidi};
use std::{borrow::Cow, ops::Range};
use unicode_bidi::{BidiClass, BidiInfo, Level, bidi_class};

pub(crate) struct BidiParagraph {
    pub(crate) text: Range<u32>,
    pub(crate) base_level: u8,
    // Neutral plaintext lines inherit the preceding line's direction; this
    // differs from UAX #9's default LTR paragraph level and block coordinates.
    pub(crate) inline_level: u8,
}

pub(crate) struct BidiAnalysis {
    pub(crate) levels: Vec<u8>,
    pub(crate) paragraphs: Vec<BidiParagraph>,
}

/// CSS upright changes the used inline direction, without changing the
/// computed `direction` that children may inherit into horizontal flows.
pub(crate) fn used_root_direction(style: &ParagraphStyle, root: &InlineStyle) -> Direction {
    if matches!(
        style.writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) && root.text_orientation == TextOrientation::Upright
    {
        Direction::Ltr
    } else {
        style.direction
    }
}

/// A same-byte-length view for UAX #9. The original Processed text and all
/// source mappings remain untouched; text in upright style is strong LTR.
pub(crate) fn upright_analysis_text(
    input: &Processed,
    styles: &[InlineStyle],
    mode: WritingMode,
    combined: &[super::combine::CombineSpan],
) -> Option<String> {
    if !matches!(mode, WritingMode::VerticalRl | WritingMode::VerticalLr)
        || (combined.is_empty()
            && !input.items.iter().any(|item| {
                matches!(item.kind, ItemKind::Text)
                    && styles[item.style as usize].text_orientation == TextOrientation::Upright
            }))
    {
        return None;
    }
    let mut bytes = input.text.as_bytes().to_vec();
    for item in &input.items {
        if !matches!(item.kind, ItemKind::Text)
            || styles[item.style as usize].text_orientation != TextOrientation::Upright
        {
            continue;
        }
        replace_with_ltr(&input.text, &mut bytes, &item.text);
    }
    // The composition is upright (strong LTR) in its containing paragraph.
    // Internal direction still belongs to the independent horizontal isolate.
    // Same-width scalars preserve external byte levels and source offsets.
    for span in combined {
        replace_with_ltr(&input.text, &mut bytes, &span.text);
    }
    Some(String::from_utf8(bytes).expect("same-width Unicode scalar replacement"))
}

fn replace_with_ltr(text: &str, bytes: &mut [u8], range: &Range<u32>) {
    let start = range.start as usize;
    let end = range.end as usize;
    for (offset, c) in text[start..end].char_indices() {
        let ltr: &[u8] = match c.len_utf8() {
            1 => b"A",
            2 => "À".as_bytes(),
            3 => "अ".as_bytes(),
            4 => "𐐀".as_bytes(),
            _ => unreachable!("UTF-8 scalar length"),
        };
        bytes[start + offset..start + offset + ltr.len()].copy_from_slice(ltr);
    }
}

/// Bidi-class shortcuts for ASCII. ASCII has no R/AL/explicit-format
/// characters, its only letters are L, and its only paragraph separators are
/// the B class controls (LF, CR, FS, GS, RS). Anything else takes the table.
fn paragraph_separator(c: char) -> bool {
    if c.is_ascii() {
        matches!(c, '\n' | '\r' | '\u{1c}'..='\u{1e}')
    } else {
        bidi_class(c) == BidiClass::B || c == '\u{2028}'
    }
}

fn forces_bidi(c: char) -> bool {
    use BidiClass::*;
    !c.is_ascii()
        && matches!(
            bidi_class(c),
            R | AL | RLE | RLO | RLI | LRE | LRO | LRI | FSI | PDF | PDI
        )
}

/// `Some(is_L)` for ASCII, `None` when the table must decide.
fn strong_ltr_ascii(c: char) -> Option<bool> {
    c.is_ascii().then(|| c.is_ascii_alphabetic())
}

pub(crate) fn needs_bidi(
    text: &str,
    style: &ParagraphStyle,
    styles: &[InlineStyle],
    direction: Direction,
) -> bool {
    style.unicode_bidi_plaintext
        || direction == Direction::Rtl
        || styles
            .iter()
            .any(|s| s.direction != Direction::Ltr || s.unicode_bidi != UnicodeBidi::Normal)
        || text.chars().any(forces_bidi)
}

fn first_strong(text: &str) -> Option<u8> {
    use BidiClass::*;
    let mut depth = 0usize;
    for c in text.chars() {
        if let Some(is_l) = strong_ltr_ascii(c) {
            if is_l && depth == 0 {
                return Some(0);
            }
            continue;
        }
        match bidi_class(c) {
            LRI | RLI | FSI => depth += 1,
            PDI => depth = depth.saturating_sub(1),
            L if depth == 0 => return Some(0),
            R | AL if depth == 0 => return Some(1),
            _ => {}
        }
    }
    None
}

pub(crate) fn analyze_bidi(
    text: &str,
    style: &ParagraphStyle,
    styles: &[InlineStyle],
    direction: Direction,
) -> BidiAnalysis {
    let coordinate_level = u8::from(direction == Direction::Rtl);
    if !needs_bidi(text, style, styles, direction) {
        let mut paragraphs = Vec::new();
        let mut start = 0;
        for (pos, c) in text.char_indices() {
            if paragraph_separator(c) {
                let end = (pos + c.len_utf8()) as u32;
                paragraphs.push(BidiParagraph {
                    text: start..end,
                    base_level: 0,
                    inline_level: 0,
                });
                start = end;
            }
        }
        if start < text.len() as u32 {
            paragraphs.push(BidiParagraph {
                text: start..text.len() as u32,
                base_level: 0,
                inline_level: 0,
            });
        }
        return BidiAnalysis {
            levels: vec![0; text.len()],
            paragraphs,
        };
    }
    // CSS preserved line separators start a new plaintext directional scope.
    // When replacement is needed, both scalars are three bytes, so byte offsets stay stable.
    let normalized = normalized_bidi_text(text);
    let normalized_text = normalized.as_ref();
    let default_level = if style.unicode_bidi_plaintext {
        None
    } else {
        Some(Level::new(coordinate_level).unwrap())
    };
    let info = BidiInfo::new(normalized_text, default_level);
    let mut inline_level = coordinate_level;
    let paragraphs = info
        .paragraphs
        .iter()
        .map(|p| {
            if style.unicode_bidi_plaintext
                && let Some(strong) = first_strong(&normalized_text[p.range.clone()])
            {
                inline_level = strong;
            }
            BidiParagraph {
                text: p.range.start as u32..p.range.end as u32,
                base_level: p.level.number(),
                inline_level,
            }
        })
        .collect();
    BidiAnalysis {
        levels: info.levels.iter().map(|l| l.number()).collect(),
        paragraphs,
    }
}

fn normalized_bidi_text(text: &str) -> Cow<'_, str> {
    if text.contains('\u{2028}') {
        Cow::Owned(text.replace('\u{2028}', "\u{2029}"))
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ascii_fast_paths_match_the_unicode_tables() {
        use super::*;
        for c in ('\0'..='\u{7f}').chain(['\u{85}', '\u{2028}', '\u{2029}', '\u{5d0}']) {
            let class = bidi_class(c);
            assert_eq!(
                paragraph_separator(c),
                class == BidiClass::B || c == '\u{2028}',
                "separator {c:?}"
            );
            assert_eq!(
                forces_bidi(c),
                matches!(
                    class,
                    BidiClass::R
                        | BidiClass::AL
                        | BidiClass::RLE
                        | BidiClass::RLO
                        | BidiClass::RLI
                        | BidiClass::LRE
                        | BidiClass::LRO
                        | BidiClass::LRI
                        | BidiClass::FSI
                        | BidiClass::PDF
                        | BidiClass::PDI
                ),
                "forces_bidi {c:?}"
            );
        }
        assert_eq!(strong_ltr_ascii('a'), Some(true));
        assert_eq!(strong_ltr_ascii('1'), Some(false));
        assert_eq!(strong_ltr_ascii('\u{5d0}'), None);
        for c in '\0'..='\u{7f}' {
            assert_eq!(
                strong_ltr_ascii(c),
                Some(bidi_class(c) == BidiClass::L),
                "strong ltr {c:?}"
            );
        }
    }

    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::geometry::Direction;
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::output::Fragment;
    use crate::style::{InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};
    use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};

    #[test]
    fn upright_and_combined_text_preserve_utf8_offsets_and_surrounding_text() {
        let input = Processed {
            text: "אבaé水😀/bö火😁גד".into(),
            items: [(0..4, 0), (4..14, 1), (14..29, 0)]
                .into_iter()
                .map(|(text, style)| super::super::Item {
                    kind: ItemKind::Text,
                    text,
                    style,
                    node: None,
                    own_break_style: false,
                })
                .collect(),
            mapping: None,
            indivisible: Vec::new(),
            source_spans: Vec::new(),
            width_origins: Vec::new(),
        };
        let styles = [
            InlineStyle::default(),
            InlineStyle {
                text_orientation: TextOrientation::Upright,
                ..Default::default()
            },
        ];
        let combined = [super::super::combine::CombineSpan {
            text: 15..25,
            item: 2,
            em: 16.0,
            units: 0..0,
        }];

        let text = upright_analysis_text(&input, &styles, WritingMode::VerticalRl, &combined)
            .expect("upright and combined text need an LTR analysis view");

        assert_eq!(text, "אבAÀअ𐐀/AÀअ𐐀גד");
        assert_eq!(text.len(), input.text.len());
        assert_eq!(
            text.char_indices().map(|(i, _)| i).collect::<Vec<_>>(),
            input
                .text
                .char_indices()
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        );
        assert_eq!(input.text, "אבaé水😀/bö火😁גד");
    }

    #[test]
    fn bidi_analysis_text_borrows_without_preserved_line_separators() {
        let text = "אב 123 ".repeat(32);
        let normalized = normalized_bidi_text(&text);

        let std::borrow::Cow::Borrowed(normalized) = normalized else {
            panic!("text without U+2028 should be borrowed");
        };
        assert!(std::ptr::eq(normalized.as_ptr(), text.as_ptr()));
        assert_eq!(normalized, text);
    }

    #[test]
    fn bidi_analysis_text_replaces_only_preserved_line_separators() {
        let text = "אב\u{2028}\u{2029}\r\nabc";
        let normalized = normalized_bidi_text(text);

        let std::borrow::Cow::Owned(normalized) = normalized else {
            panic!("text with U+2028 should own its replacement");
        };
        assert_eq!(normalized, "אב\u{2029}\u{2029}\r\nabc");
        assert_eq!(normalized.len(), text.len());
    }

    #[test]
    fn plaintext_multiple_paragraph_directions() {
        let style = ParagraphStyle {
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let a = analyze_bidi(
            "abc\nאב\n123\nxyz",
            &style,
            std::slice::from_ref(&style.root),
            style.direction,
        );
        assert_eq!(
            a.paragraphs
                .iter()
                .map(|p| (p.text.clone(), p.base_level, p.inline_level))
                .collect::<Vec<_>>(),
            vec![(0..4, 0, 0), (4..9, 1, 1), (9..13, 0, 1), (13..16, 0, 0)]
        );
        assert_eq!(&a.levels[4..8], &[1, 1, 1, 1]);
    }

    #[test]
    fn line_separator_starts_new_plaintext_paragraph() {
        let style = ParagraphStyle {
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let a = analyze_bidi(
            "אב\u{2028}abc",
            &style,
            std::slice::from_ref(&style.root),
            style.direction,
        );
        assert_eq!(
            a.paragraphs
                .iter()
                .map(|p| (p.text.clone(), p.base_level, p.inline_level))
                .collect::<Vec<_>>(),
            vec![(0..7, 1, 1), (7..10, 0, 0)]
        );
        assert_eq!(a.levels.len(), 10);
        assert_eq!(a.levels, vec![1, 1, 1, 1, 1, 1, 1, 0, 0, 0]);
    }

    #[test]
    fn unicode_paragraph_separator_and_crlf_keep_plaintext_byte_ranges() {
        let style = ParagraphStyle {
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let cases = [
            (
                "אב\u{2029}abc",
                vec![(0..7, 1, 1), (7..10, 0, 0)],
                vec![1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
            ),
            (
                "אב\r\nabc",
                vec![(0..5, 1, 1), (5..6, 0, 1), (6..9, 0, 0)],
                vec![1, 1, 1, 1, 1, 0, 0, 0, 0],
            ),
        ];

        for (text, expected_paragraphs, expected_levels) in cases {
            let analysis = analyze_bidi(
                text,
                &style,
                std::slice::from_ref(&style.root),
                style.direction,
            );
            let paragraphs = analysis
                .paragraphs
                .iter()
                .map(|p| (p.text.clone(), p.base_level, p.inline_level))
                .collect::<Vec<_>>();

            assert_eq!(paragraphs, expected_paragraphs, "ranges for {text:?}");
            assert_eq!(analysis.levels, expected_levels, "levels for {text:?}");
        }
    }

    #[test]
    fn isolated_strong_character_does_not_set_plaintext_line_direction() {
        let style = ParagraphStyle {
            direction: Direction::Rtl,
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let a = analyze_bidi(
            "\u{2066}abc\u{2069} 123\nabc",
            &style,
            std::slice::from_ref(&style.root),
            style.direction,
        );
        assert_eq!(
            a.paragraphs
                .iter()
                .map(|p| (p.base_level, p.inline_level))
                .collect::<Vec<_>>(),
            vec![(0, 1), (0, 0)]
        );
    }

    #[test]
    fn bidi_fast_path_conditions() {
        let style = ParagraphStyle::default();
        let mut inline = style.root.clone();
        assert!(!needs_bidi(
            "abc",
            &style,
            &[inline.clone()],
            style.direction
        ));
        for text in ["אב", "ع", "\u{2066}abc\u{2069}"] {
            assert!(needs_bidi(text, &style, &[inline.clone()], style.direction));
        }
        inline.direction = Direction::Rtl;
        assert!(needs_bidi(
            "abc",
            &style,
            &[inline.clone()],
            style.direction
        ));
        inline.direction = Direction::Ltr;
        inline.unicode_bidi = UnicodeBidi::Embed;
        assert!(needs_bidi("abc", &style, &[inline], style.direction));
        let plaintext = ParagraphStyle {
            unicode_bidi_plaintext: true,
            ..style.clone()
        };
        assert!(needs_bidi(
            "abc",
            &plaintext,
            std::slice::from_ref(&style.root),
            plaintext.direction,
        ));
        let rtl = ParagraphStyle {
            direction: Direction::Rtl,
            ..style.clone()
        };
        assert!(needs_bidi("abc", &rtl, &[style.root], rtl.direction));
    }

    #[test]
    fn plaintext_start_alignment_preserves_block_coordinates_and_visual_order() {
        let limits = Limits::default();
        let style = ParagraphStyle {
            unicode_bidi_plaintext: true,
            root: InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "אב\n123\nabc");
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts).unwrap();
        let mut token = p.start_token();
        for (right, positions) in [
            (true, vec![2, 0]),
            (true, vec![5, 6, 7]),
            (false, vec![9, 10, 11]),
        ] {
            let LineResult::Line(line) = p.next_line(
                &mut cx,
                token,
                &Default::default(),
                &LineConstraint::new(100.0),
                &AtomicSizes::EMPTY,
            ) else {
                panic!()
            };
            token = line.break_token();
            let runs = line
                .fragments()
                .filter_map(|f| {
                    if let Fragment::GlyphRun(r) = f {
                        Some(r)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let start = runs
                .iter()
                .map(|r| r.inline_start())
                .fold(f32::INFINITY, f32::min);
            assert_eq!(start > 0.0, right, "plaintext start alignment");
            let mut glyphs = runs.iter().flat_map(|r| r.glyphs()).collect::<Vec<_>>();
            glyphs.sort_by(|a, b| a.inline_position.total_cmp(&b.inline_position));
            assert_eq!(
                glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(),
                positions
            );
        }
    }
    #[test]
    fn plaintext_indent_is_on_resolved_inline_start() {
        let limits = Limits::default();
        let style = ParagraphStyle {
            unicode_bidi_plaintext: true,
            ..Default::default()
        };
        let mut b = ParagraphBuilder::new(&style, &limits);
        b.push_text(TextSource::Generated { node: NodeId(1) }, "אב");
        let fonts = FontCollection::with_options(
            &limits,
            FontOptions {
                system_fonts: false,
                ..Default::default()
            },
        );
        let mut cx = LayoutContext::new();
        let p = b.build(&mut cx, &fonts).unwrap();
        let options = crate::style::LineOptions {
            text_indent: crate::style::TextIndent {
                length: 12.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let LineResult::Line(line) = p.next_line(
            &mut cx,
            p.start_token(),
            &options,
            &LineConstraint::new(100.0),
            &AtomicSizes::EMPTY,
        ) else {
            panic!()
        };
        let end = line
            .fragments()
            .filter_map(|f| {
                if let Fragment::GlyphRun(r) = f {
                    Some(r.inline_start() + r.inline_size())
                } else {
                    None
                }
            })
            .fold(0.0, f32::max);
        assert_eq!(end, 88.0);
    }
}
