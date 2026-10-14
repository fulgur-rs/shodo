//! Scratch state shared only while selecting faces for one actual grapheme.
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use super::{Candidate, FontPresentation, MAX_CACHE_KEY_BYTES, covers, covers_chars, ignored};
use skrifa::FontRef;

type CoverageKey = (u64, u32, Option<Coverage>);
const MAX_COVERAGE_ENTRIES: usize = 512;

struct Coverage(crate::font::RegisteredDescriptor);

impl PartialEq for Coverage {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_coverage(&other.0)
    }
}
impl Eq for Coverage {}
impl Hash for Coverage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.coverage_hash().hash(state);
    }
}

pub(crate) struct FontCluster<'a> {
    text: &'a str,
    properties: OnceCell<Properties>,
    coverage: HashMap<CoverageKey, bool>,
}

struct Properties {
    chars: ClusterChars,
    auto_color: bool,
}

const INLINE_CHARS: usize = 16;

/// Most long graphemes repeat a few marks. Keep their distinct scalars inline;
/// spill in insertion order only when the number of distinct scalars grows.
struct ClusterChars {
    inline: [char; INLINE_CHARS],
    len: usize,
    overflow: Vec<char>,
}

impl ClusterChars {
    fn iter(&self) -> impl Iterator<Item = &char> {
        self.inline[..self.len].iter().chain(&self.overflow)
    }
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
            let mut seen: Option<HashSet<char>> = None;
            let mut chars = ClusterChars {
                inline: ['\0'; INLINE_CHARS],
                len: 0,
                overflow: Vec::new(),
            };
            let mut selector = None;
            let mut emoji = false;
            let emoji_property =
                icu_properties::CodePointSetData::new::<icu_properties::props::EmojiPresentation>();
            for ch in super::cluster_chars(self.text) {
                if ch == '\u{fe0e}' || ch == '\u{fe0f}' {
                    selector = Some(ch);
                }
                emoji |= emoji_property.contains(ch);
                if ignored(ch) {
                    continue;
                }
                if let Some(seen) = &mut seen {
                    if seen.insert(ch) {
                        chars.overflow.push(ch);
                    }
                } else if !chars.inline[..chars.len].contains(&ch) {
                    if chars.len < INLINE_CHARS {
                        chars.inline[chars.len] = ch;
                        chars.len += 1;
                    } else {
                        // Bound linear scans by INLINE_CHARS even for unusual
                        // clusters with many distinct scalars.
                        let mut spilled: HashSet<_> = chars.inline.into_iter().collect();
                        spilled.insert(ch);
                        seen = Some(spilled);
                        chars.overflow.push(ch);
                    }
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
            // Unrestricted native/CSS faces share the empty-range key across
            // temporary candidate reconstruction. Restricted CSS descriptors
            // use shared union values and a precomputed hash, preserving
            // coverage reuse across aliases without cloning/hashing ranges.
            (!candidate.descriptor.unicode_ranges.is_empty())
                .then(|| Coverage(candidate.descriptor.clone())),
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
    fn repeated_long_cluster_properties_need_no_heap_allocation() {
        let text = format!("a{}", "\u{301}\u{200d}\u{fe0f}".repeat(4096));
        let cluster = FontCluster::new(&text);
        let (properties, peak) = crate::test_support::alloc::measure_peak(|| cluster.properties());
        assert_eq!(
            properties.chars.iter().copied().collect::<Vec<_>>(),
            ['a', '\u{301}']
        );
        assert!(properties.auto_color);
        assert_eq!(peak, 0);
    }

    #[test]
    fn property_characters_keep_first_occurrence_order_after_many_distinct_marks() {
        let marks: String = (0x300..0x340).map(|n| char::from_u32(n).unwrap()).collect();
        let text = format!("a{marks}{marks}\u{fe0f}\u{fe0e}");
        let cluster = FontCluster::new(&text);
        let expected: Vec<_> = std::iter::once('a').chain(marks.chars()).collect();
        assert_eq!(
            cluster
                .properties()
                .chars
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            expected
        );
        assert!(!cluster.properties().auto_color);
    }

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
                descriptor: crate::font::RegisteredDescriptor::new(FontFaceDescriptor::default()),
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
