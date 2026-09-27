//! UAX #9 paragraphs and CSS plaintext line direction. Byte offsets stay unchanged.
use super::{ItemKind, whitespace::Processed};
use crate::geometry::{Direction, WritingMode};
use crate::style::{InlineStyle, ParagraphStyle, TextOrientation, UnicodeBidi};
use std::ops::Range;
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
) -> Option<String> {
    if !matches!(mode, WritingMode::VerticalRl | WritingMode::VerticalLr)
        || !input.items.iter().any(|item| {
            matches!(item.kind, ItemKind::Text)
                && styles[item.style as usize].text_orientation == TextOrientation::Upright
        })
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
        let start = item.text.start as usize;
        let end = item.text.end as usize;
        for (offset, c) in input.text[start..end].char_indices() {
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
    Some(String::from_utf8(bytes).expect("same-width Unicode scalar replacement"))
}

pub(crate) fn needs_bidi(
    text: &str,
    style: &ParagraphStyle,
    styles: &[InlineStyle],
    direction: Direction,
) -> bool {
    use BidiClass::*;
    style.unicode_bidi_plaintext
        || direction == Direction::Rtl
        || styles
            .iter()
            .any(|s| s.direction != Direction::Ltr || s.unicode_bidi != UnicodeBidi::Normal)
        || text.chars().any(|c| {
            matches!(
                bidi_class(c),
                R | AL | RLE | RLO | RLI | LRE | LRO | LRI | FSI | PDF | PDI
            )
        })
}

fn first_strong(text: &str) -> Option<u8> {
    use BidiClass::*;
    let mut depth = 0usize;
    for c in text.chars() {
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
            if bidi_class(c) == BidiClass::B || c == '\u{2028}' {
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
    // Both scalars are three bytes, so the analyzed copy preserves every offset.
    let normalized = text.replace('\u{2028}', "\u{2029}");
    let default_level = if style.unicode_bidi_plaintext {
        None
    } else {
        Some(Level::new(coordinate_level).unwrap())
    };
    let info = BidiInfo::new(&normalized, default_level);
    let mut inline_level = coordinate_level;
    let paragraphs = info
        .paragraphs
        .iter()
        .map(|p| {
            if style.unicode_bidi_plaintext
                && let Some(strong) = first_strong(&normalized[p.range.clone()])
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::geometry::Direction;
    use crate::limits::Limits;
    use crate::node::{NodeId, TextSource};
    use crate::output::Fragment;
    use crate::style::{InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};
    use crate::{AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};

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
                .map(|p| p.base_level)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert_eq!(&a.levels[7..], &[0, 0, 0]);
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
