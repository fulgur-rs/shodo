//! Scratch state shared only while selecting faces for one actual grapheme.
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::{Candidate, FontPresentation, MAX_CACHE_KEY_BYTES, covers, covers_chars, ignored};
use skrifa::FontRef;

type CoverageKey = (u64, u32, Vec<(u32, u32)>);
const MAX_COVERAGE_ENTRIES: usize = 512;

pub(crate) struct FontCluster<'a> {
    text: &'a str,
    properties: OnceCell<Properties>,
    coverage: HashMap<CoverageKey, bool>,
}

struct Properties {
    chars: Vec<char>,
    auto_color: bool,
}

impl<'a> FontCluster<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            properties: OnceCell::new(),
            coverage: HashMap::new(),
        }
    }

    pub(crate) fn as_str(&self) -> &'a str {
        self.text
    }

    fn properties(&self) -> &Properties {
        self.properties.get_or_init(|| {
            let mut seen = HashSet::new();
            let mut chars = Vec::new();
            let mut selector = None;
            let mut emoji = false;
            let emoji_property =
                icu_properties::CodePointSetData::new::<icu_properties::props::EmojiPresentation>();
            for ch in super::cluster_chars(self.text) {
                if ch == '\u{fe0e}' || ch == '\u{fe0f}' {
                    selector = Some(ch);
                }
                emoji |= emoji_property.contains(ch);
                if !ignored(ch) && seen.insert(ch) {
                    chars.push(ch);
                }
            }
            Properties {
                chars,
                auto_color: selector.map_or(emoji, |ch| ch == '\u{fe0f}'),
            }
        })
    }

    pub(super) fn prefer_color(&self, presentation: FontPresentation) -> bool {
        if self.text.len() <= MAX_CACHE_KEY_BYTES {
            return super::prefer_color(presentation, self.text);
        }
        match presentation {
            FontPresentation::Emoji => true,
            FontPresentation::Text => false,
            FontPresentation::Auto => self.properties().auto_color,
        }
    }

    pub(super) fn covers(&mut self, candidate: &Candidate, font: &FontRef<'_>) -> bool {
        if self.text.len() <= MAX_CACHE_KEY_BYTES {
            return covers(candidate, font, self.text);
        }
        // Native candidates can all have the same temporary FontId. Identify
        // immutable bytes and the face instead; different CSS unicode-ranges
        // on the same bytes must still produce independent coverage results.
        let key = (
            candidate.data.data.id(),
            candidate.data.index,
            candidate.descriptor.unicode_ranges.clone(),
        );
        if let Some(&result) = self.coverage.get(&key) {
            return result;
        }
        // Nominal coverage is a conjunction over characters, so duplicate
        // scalars add no condition. Keep the original text for shaping/cache.
        let result = covers_chars(
            candidate,
            font,
            self.properties().chars.iter().copied(),
            self.text.len(),
        );
        // Unselected platform sources can reload with fresh Blob identities.
        // Bound scratch retention even when none of those keys can be reused.
        if self.coverage.len() < MAX_COVERAGE_ENTRIES {
            self.coverage.insert(key, result);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{Blob, FontData, FontFaceDescriptor, FontId};

    #[test]
    fn reloaded_unselected_native_faces_keep_cluster_scratch_bounded() {
        let bytes = crate::font::browser_tests::test_font("Partial", &['a'], 600);
        let text = format!("a{}", "\u{301}".repeat(4096));
        let mut cluster = FontCluster::new(&text);
        // SourceCache eviction reloads the same file as a fresh Blob. These
        // candidates never cover the full cluster and are never materialized.
        for _ in 0..2048 {
            let data = FontData::new(Blob::from(bytes.clone()), 0);
            let info = super::super::face_info(&data).unwrap();
            let candidate = Candidate {
                order_key: (info.source().id().to_u64(), info.index()),
                id: FontId {
                    layer: 0,
                    index: u32::MAX,
                },
                data,
                descriptor: FontFaceDescriptor::default(),
                info,
                color: false,
            };
            let font = FontRef::from_index(candidate.data.data.as_ref(), 0).unwrap();
            assert!(!cluster.covers(&candidate, &font));
        }
        assert!(
            cluster.coverage.len() <= 512,
            "{} stale coverage entries after native reloads",
            cluster.coverage.len()
        );
    }
}
