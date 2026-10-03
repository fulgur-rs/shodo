//! Temporary shaping input; original metadata and scalars stay paragraph-owned.
use crate::analysis::itemize::{Scalar, ShapeItem};
use std::ops::Range;

pub(super) struct ShapeInput<'a> {
    pub(super) original: &'a ShapeItem,
    pub(super) scalars: &'a [Scalar],
    pub(super) before: Context<'a>,
    pub(super) after: Context<'a>,
    pub(super) width_feature: Option<[u8; 4]>,
}

pub(super) enum Context<'a> {
    Borrowed(&'a str),
    Inline { bytes: [u8; 20], len: usize },
}
impl Context<'_> {
    fn from_chars(chars: impl Iterator<Item = char>) -> Self {
        let mut bytes = [0; 20];
        let mut len = 0;
        for c in chars.take(5) {
            len += c.encode_utf8(&mut bytes[len..]).len();
        }
        Self::Inline { bytes, len }
    }
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Inline { bytes, len } => {
                std::str::from_utf8(&bytes[..*len]).expect("context encoded from chars")
            }
        }
    }
}
impl<'a> ShapeInput<'a> {
    pub(super) fn whole(original: &'a ShapeItem) -> Self {
        Self {
            original,
            scalars: &original.scalars,
            before: Context::Borrowed(&original.before),
            after: Context::Borrowed(&original.after),
            width_feature: original.width_feature,
        }
    }
    pub(super) fn with_width_feature(mut self, width_feature: Option<[u8; 4]>) -> Self {
        self.width_feature = width_feature;
        self
    }
    fn clipped(original: &'a ShapeItem, text: &Range<u32>) -> Option<Self> {
        let begin = original.scalars.partition_point(|s| s.offset < text.start);
        let end = original.scalars.partition_point(|s| s.offset < text.end);
        if begin == end {
            return None;
        }
        let mut reversed = ['\0'; 5];
        let mut count = 0;
        for c in original
            .before
            .chars()
            .chain(original.scalars[..begin].iter().map(|s| s.c))
            .rev()
            .take(5)
        {
            reversed[count] = c;
            count += 1;
        }
        Some(Self {
            original,
            scalars: &original.scalars[begin..end],
            before: Context::from_chars(reversed[..count].iter().rev().copied()),
            after: Context::from_chars(
                original.scalars[end..]
                    .iter()
                    .map(|s| s.c)
                    .chain(original.after.chars()),
            ),
            width_feature: original.width_feature,
        })
    }
}

pub(super) fn compatible(a: &ShapeItem, b: &ShapeItem) -> bool {
    a.segment == b.segment
        && a.style == b.style
        && a.level == b.level
        && a.script == b.script
        && a.font == b.font
        && a.orientation == b.orientation
        && a.combine == b.combine
        && a.width_feature == b.width_feature
}

pub(super) fn can_borrow(items: &[ShapeItem], text: &Range<u32>) -> bool {
    // No second item before the same stop boundary means no possible merge.
    // Avoid repeating the scalar binary searches for the ordinary one-item cut.
    if items
        .get(1)
        .is_none_or(|item| item.scalars.first().is_none_or(|s| s.offset >= text.end))
    {
        return true;
    }
    let mut previous = None;
    for item in items
        .iter()
        .take_while(|i| i.scalars.first().is_some_and(|s| s.offset < text.end))
    {
        let begin = item.scalars.partition_point(|s| s.offset < text.start);
        let end = item.scalars.partition_point(|s| s.offset < text.end);
        if begin == end {
            continue;
        }
        if previous.is_some_and(|p| compatible(p, item)) {
            return false;
        }
        previous = Some(item);
    }
    true
}

pub(super) fn clipped_items(
    items: &[ShapeItem],
    text: Range<u32>,
) -> impl Iterator<Item = ShapeInput<'_>> {
    let end = text.end;
    items
        .iter()
        .take_while(move |i| i.scalars.first().is_some_and(|s| s.offset < end))
        .filter_map(move |original| ShapeInput::clipped(original, &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> ShapeItem {
        ShapeItem {
            segment: 3,
            scalars: [
                ('中', 0, 3, true),
                ('😀', 3, 7, true),
                ('a', 7, 8, true),
                ('\u{301}', 8, 10, false),
                ('ب', 10, 12, true),
                ('é', 12, 14, true),
                ('z', 14, 15, true),
            ]
            .into_iter()
            .map(|(c, offset, end, grapheme_start)| Scalar {
                c,
                offset,
                end,
                item: 42,
                grapheme_start,
            })
            .collect(),
            end: 15,
            style: 0,
            level: 1,
            script: *b"Arab",
            font: None,
            orientation: crate::shape::orientation::RunOrientation::Horizontal,
            combine: None,
            width_feature: None,
            before: "前😀é甲乙丙".into(),
            after: "後😀é甲乙丙".into(),
        }
    }
    #[test]
    fn utf8_clip_keeps_five_scalar_contexts_and_original_flags() {
        let original = item();
        let view = ShapeInput::clipped(&original, &(7..12)).unwrap();
        assert_eq!(view.before.as_str(), "甲乙丙中😀");
        assert_eq!(view.after.as_str(), "éz後😀é");
        assert_eq!(
            view.scalars
                .iter()
                .map(|s| (s.c, s.offset, s.end, s.item, s.grapheme_start))
                .collect::<Vec<_>>(),
            [
                ('a', 7, 8, 42, true),
                ('\u{301}', 8, 10, 42, false),
                ('ب', 10, 12, 42, true)
            ]
        );
        assert!(std::ptr::eq(view.original, &original));
        assert_eq!(view.scalars.as_ptr(), original.scalars[2..].as_ptr());
        assert!(ShapeInput::clipped(&original, &(3..3)).is_none());
        let tail = ShapeInput::clipped(&original, &(14..15)).unwrap();
        assert_eq!(tail.before.as_str(), "😀a\u{301}بé");
        assert_eq!(tail.after.as_str(), "後😀é甲乙");
    }
    #[test]
    fn maximum_utf8_context_has_twenty_bytes_without_truncating_a_scalar() {
        let context = Context::from_chars("😀😁😂😃😄😅".chars());
        assert_eq!(context.as_str(), "😀😁😂😃😄");
        assert_eq!(context.as_str().len(), 20);
    }
    #[test]
    fn compatible_adjacent_clips_require_the_owned_merge_path() {
        let first = item();
        let mut next = item();
        for scalar in &mut next.scalars {
            scalar.offset += 15;
            scalar.end += 15;
        }
        next.end += 15;
        assert!(!can_borrow(&[first.clone(), next.clone()], &(0..30)));
        next.segment = 4;
        assert!(can_borrow(&[first.clone(), next.clone()], &(0..30)));
        next.segment = 3;
        next.width_feature = Some(*b"hwid");
        assert!(can_borrow(&[first.clone(), next.clone()], &(0..30)));
        // An empty clipped predecessor cannot create a false merge dependency.
        next.width_feature = None;
        assert!(can_borrow(&[first, next], &(15..30)));
    }
}
