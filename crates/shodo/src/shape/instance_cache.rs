//! Bounded preparation reuse within one shape_inputs call. Keys borrow the
//! paragraph's immutable input; only fully initialized instances are shared.
//! `WindowInstances` extends the same reuse across the line-edge window
//! shapes of one paragraph.
use super::instance::{ResolutionWarning, RunInstance};
use crate::{font::FontMatch, limits::WarningSink, style::InlineStyle};
use std::{ops::Deref, sync::Arc};

#[cfg(test)]
std::thread_local! {
    pub(super) static BYPASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const ENTRIES: usize = 8;
const BYTES: usize = 16 * 1024;
type Resolved = (harfrust::ShaperInstance, Arc<RunInstance>, f32);

#[derive(Clone, Copy)]
pub(super) struct Key<'items, 'styles> {
    pub(super) found: &'items FontMatch,
    pub(super) style: &'styles InlineStyle,
    pub(super) script: [u8; 4],
}
impl Key<'_, '_> {
    fn matches(self, other: Self) -> bool {
        self.script == other.script
            && (std::ptr::eq(self.found, other.found)
                || (self.found.id == other.found.id
                    && self.found.embolden == other.found.embolden
                    && self.found.skew.map(f32::to_bits) == other.found.skew.map(f32::to_bits)
                    && same_variations(&self.found.variations, &other.found.variations)))
            && (std::ptr::eq(self.style, other.style)
                || (self.style.font_size.to_bits() == other.style.font_size.to_bits()
                    && self
                        .style
                        .font_size_adjust
                        .map(|v| (v.metric, v.value.to_bits()))
                        == other
                            .style
                            .font_size_adjust
                            .map(|v| (v.metric, v.value.to_bits()))
                    && self.style.font_optical_sizing == other.style.font_optical_sizing
                    && same_variations(&self.style.font_variations, &other.style.font_variations)
                    && self.style.lang == other.style.lang))
    }
}

fn same_variations(a: &[crate::style::FontVariation], b: &[crate::style::FontVariation]) -> bool {
    // Public design coordinates retain the authored bits, including signed zero.
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(a, b)| a.tag == b.tag && a.value.to_bits() == b.value.to_bits())
}

struct Entry<'items, 'styles> {
    key: Key<'items, 'styles>,
    resolved: Resolved,
    warning: Option<ResolutionWarning>,
    bytes: usize,
}

pub(super) enum Cached<'cache> {
    Borrowed(&'cache Resolved),
    Owned(Resolved),
}
impl Deref for Cached<'_> {
    type Target = Resolved;
    fn deref(&self) -> &Resolved {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned(value) => value,
        }
    }
}

pub(super) struct InstanceCache<'items, 'styles> {
    entries: [Option<Entry<'items, 'styles>>; ENTRIES],
    next: usize,
    bytes: usize,
}
impl Default for InstanceCache<'_, '_> {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|_| None),
            next: 0,
            bytes: 0,
        }
    }
}
impl<'items, 'styles> InstanceCache<'items, 'styles> {
    pub(super) fn resolve(
        &mut self,
        key: Key<'items, 'styles>,
        features: &Arc<[harfrust::Feature]>,
        warnings: &mut WarningSink,
        prepare: impl FnOnce() -> (Resolved, Option<ResolutionWarning>, bool),
    ) -> Cached<'_> {
        let eligible = eligible(key, features);
        if eligible
            && let Some(index) = self.entries.iter().position(|entry| {
                entry.as_ref().is_some_and(|entry| {
                    key.matches(entry.key)
                        && (Arc::ptr_eq(features, &entry.resolved.1.features)
                            || **features == *entry.resolved.1.features)
                })
            })
        {
            let entry = self.entries[index].as_ref().expect("located instance");
            if let Some(warning) = entry.warning {
                warning.emit(warnings);
            }
            return Cached::Borrowed(&entry.resolved);
        }
        let (resolved, warning, inline_coords) = prepare();
        if let Some(warning) = warning {
            warning.emit(warnings);
        }
        let bytes = retained_bytes(&resolved);
        if !eligible || !inline_coords || bytes > BYTES {
            return Cached::Owned(resolved);
        }
        // FIFO eviction needs no per-hit mutation or clock. All heap payloads
        // retained by the cache, including shared features, are charged below.
        while self.entries[self.next].is_some() || self.bytes + bytes > BYTES {
            if let Some(entry) = self.entries[self.next].take() {
                self.bytes -= entry.bytes;
            }
            if self.bytes + bytes <= BYTES {
                break;
            }
            self.next = (self.next + 1) % ENTRIES;
        }
        let index = self.next;
        self.entries[index] = Some(Entry {
            key,
            resolved,
            warning,
            bytes,
        });
        self.bytes += bytes;
        self.next = (index + 1) % ENTRIES;
        Cached::Borrowed(
            &self.entries[index]
                .as_ref()
                .expect("inserted instance")
                .resolved,
        )
    }
}

/// Large authored vectors are excluded before equality checks.
fn eligible(key: Key<'_, '_>, features: &Arc<[harfrust::Feature]>) -> bool {
    let eligible = key.style.font_variations.len()
        <= BYTES / size_of::<crate::style::FontVariation>()
        && key.found.variations.len() <= BYTES / size_of::<crate::style::FontVariation>()
        && features.len() <= BYTES / size_of::<harfrust::Feature>()
        && key
            .style
            .lang
            .as_ref()
            .is_none_or(|language| language.len() <= BYTES);
    #[cfg(test)]
    let eligible = eligible && !BYPASS.with(std::cell::Cell::get);
    eligible
}

struct WindowEntry {
    found: Arc<FontMatch>,
    style: u32,
    script: [u8; 4],
    resolved: Resolved,
    warning: Option<ResolutionWarning>,
    bytes: usize,
}

/// Prepared instances of the most recent paragraph's line-edge windows.
///
/// A line scan shapes a window at many break candidates, and each call used
/// to resolve the instance and read the font metrics again although the
/// window repeats an input the paragraph already prepared. Entries are keyed
/// by the paragraph's own style index and font match, so they are dropped
/// whenever a window of another paragraph is shaped. Bounds and eligibility
/// are those of `InstanceCache`.
#[derive(Default)]
pub(crate) struct WindowInstances {
    owner: Option<(u64, usize)>,
    entries: Vec<WindowEntry>,
    bytes: usize,
}

impl std::fmt::Debug for WindowInstances {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowInstances")
            .field("entries", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl WindowInstances {
    pub(crate) fn begin(&mut self, data: &crate::paragraph::ParagraphData) {
        let owner = (
            data.id,
            data as *const crate::paragraph::ParagraphData as usize,
        );
        if self.owner != Some(owner) {
            self.clear();
            self.owner = Some(owner);
        }
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    /// `styles` must be the styles of the paragraph passed to `begin`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve<'s>(
        &'s mut self,
        found: &Arc<FontMatch>,
        style: u32,
        styles: &[InlineStyle],
        script: [u8; 4],
        features: &Arc<[harfrust::Feature]>,
        warnings: &mut WarningSink,
        prepare: impl FnOnce() -> (Resolved, Option<ResolutionWarning>, bool),
    ) -> Cached<'s> {
        let key = Key {
            found,
            style: &styles[style as usize],
            script,
        };
        let eligible = eligible(key, features);
        if eligible
            && let Some(index) = self.entries.iter().position(|entry| {
                key.matches(Key {
                    found: &entry.found,
                    style: &styles[entry.style as usize],
                    script: entry.script,
                }) && (Arc::ptr_eq(features, &entry.resolved.1.features)
                    || **features == *entry.resolved.1.features)
            })
        {
            let entry = &self.entries[index];
            if let Some(warning) = entry.warning {
                warning.emit(warnings);
            }
            return Cached::Borrowed(&entry.resolved);
        }
        let (resolved, warning, inline_coords) = prepare();
        if let Some(warning) = warning {
            warning.emit(warnings);
        }
        let bytes = retained_bytes(&resolved);
        if !eligible || !inline_coords || bytes > BYTES {
            return Cached::Owned(resolved);
        }
        // FIFO eviction, as in `InstanceCache`; entries stay in insertion order.
        while self.bytes + bytes > BYTES || self.entries.len() == ENTRIES {
            let evicted = self.entries.remove(0);
            self.bytes -= evicted.bytes;
        }
        self.entries.push(WindowEntry {
            found: Arc::clone(found),
            style,
            script,
            resolved,
            warning,
            bytes,
        });
        self.bytes += bytes;
        Cached::Borrowed(&self.entries.last().expect("inserted instance").resolved)
    }
}

fn retained_bytes(resolved: &Resolved) -> usize {
    let instance = &resolved.1;
    let parts = [
        size_of::<RunInstance>() + 2 * size_of::<usize>(),
        instance
            .coords
            .capacity()
            .saturating_mul(size_of::<crate::font::NormalizedCoord>()),
        instance
            .variations
            .capacity()
            .saturating_mul(size_of::<crate::style::FontVariation>()),
        instance.language.as_ref().map_or(0, String::capacity),
        instance
            .features
            .len()
            .saturating_mul(size_of::<harfrust::Feature>())
            .saturating_add(2 * size_of::<usize>()),
    ];
    parts.into_iter().fold(0usize, usize::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontCollection;
    use crate::limits::Limits;
    use std::cell::Cell;

    fn found() -> FontMatch {
        FontMatch {
            id: FontCollection::new(&Limits::default()).primary_font(),
            variations: vec![],
            embolden: false,
            skew: None,
        }
    }

    fn prepared(style: &InlineStyle) -> Resolved {
        (
            harfrust::ShaperInstance::default(),
            Arc::new(RunInstance {
                language: style.lang.clone(),
                ..Default::default()
            }),
            style.font_size,
        )
    }

    #[test]
    fn entry_limit_evicts_and_cache_drop_releases_retained_instances() {
        let found = found();
        let styles: Vec<_> = (0..9)
            .map(|i| InlineStyle {
                font_size: 16.0 + i as f32,
                ..Default::default()
            })
            .collect();
        let features = Arc::default();
        let mut warnings = WarningSink::default();
        let mut cache = InstanceCache::default();
        let mut weak = Vec::new();
        for style in &styles {
            let value = cache.resolve(
                Key {
                    found: &found,
                    style,
                    script: *b"Latn",
                },
                &features,
                &mut warnings,
                || (prepared(style), None, true),
            );
            weak.push(Arc::downgrade(&value.1));
        }
        assert!(weak[0].upgrade().is_none());
        assert!(weak[1..].iter().all(|value| value.upgrade().is_some()));
        assert_eq!(cache.entries.iter().flatten().count(), 8);
        drop(cache);
        assert!(weak.iter().all(|value| value.upgrade().is_none()));
    }

    #[test]
    fn byte_limit_evicts_before_entry_limit_and_oversized_results_are_not_retained() {
        let found = found();
        let styles: Vec<_> = (0..3)
            .map(|i| InlineStyle {
                lang: Some(format!("{i}{}", "a".repeat(7000))),
                ..Default::default()
            })
            .collect();
        let features = Arc::default();
        let mut warnings = WarningSink::default();
        let mut cache = InstanceCache::default();
        let mut weak = Vec::new();
        for style in &styles {
            let value = cache.resolve(
                Key {
                    found: &found,
                    style,
                    script: *b"Latn",
                },
                &features,
                &mut warnings,
                || (prepared(style), None, true),
            );
            weak.push(Arc::downgrade(&value.1));
            assert!(cache.bytes <= BYTES);
        }
        assert!(weak[0].upgrade().is_none());
        assert_eq!(cache.entries.iter().flatten().count(), 2);
        let large = InlineStyle {
            lang: Some("a".repeat(BYTES + 1)),
            ..Default::default()
        };
        let weak = {
            let value = cache.resolve(
                Key {
                    found: &found,
                    style: &large,
                    script: *b"Latn",
                },
                &features,
                &mut warnings,
                || (prepared(&large), None, true),
            );
            assert!(matches!(value, Cached::Owned(_)));
            Arc::downgrade(&value.1)
        };
        assert!(weak.upgrade().is_none());
        let style = InlineStyle::default();
        let features: Arc<[harfrust::Feature]> =
            vec![
                harfrust::Feature::new(harfrust::Tag::new(b"liga"), 0, ..);
                BYTES / size_of::<harfrust::Feature>() + 1
            ]
            .into();
        assert!(matches!(
            cache.resolve(
                Key {
                    found: &found,
                    style: &style,
                    script: *b"Latn"
                },
                &features,
                &mut warnings,
                || (prepared(&style), None, true)
            ),
            Cached::Owned(_)
        ));
    }

    #[test]
    fn face_with_heap_coordinates_is_not_cached_even_when_default_slice_is_empty() {
        let found = found();
        let style = InlineStyle::default();
        let features = Arc::default();
        let calls = Cell::new(0);
        let mut cache = InstanceCache::default();
        for _ in 0..2 {
            let value = cache.resolve(
                Key {
                    found: &found,
                    style: &style,
                    script: *b"Latn",
                },
                &features,
                &mut WarningSink::default(),
                || {
                    calls.set(calls.get() + 1);
                    (prepared(&style), None, false)
                },
            );
            assert!(matches!(value, Cached::Owned(_)));
        }
        assert_eq!(calls.get(), 2);
        assert_eq!(cache.bytes, 0);
    }
}
