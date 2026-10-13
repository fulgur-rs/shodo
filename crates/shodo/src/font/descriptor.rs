//! CSS descriptors kept separately from a font's intrinsic metadata.

use super::FontError;
use crate::style::FontStyle;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[cfg(test)]
std::thread_local! {
    pub(super) static RANGE_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INDEX_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Resolved `@font-face` descriptors. Ranges are inclusive. An empty
/// `unicode_ranges` list covers all Unicode scalar values.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFaceDescriptor {
    pub family: String,
    /// CSS font-weight range, from 1 through 1000.
    pub weight: (f32, f32),
    /// CSS font-width percentages; positive and finite.
    pub width: (f32, f32),
    pub style: FontStyle,
    pub unicode_ranges: Vec<(u32, u32)>,
}

impl Default for FontFaceDescriptor {
    fn default() -> Self {
        Self {
            family: String::new(),
            weight: (400., 400.),
            width: (100., 100.),
            style: FontStyle::Normal,
            unicode_ranges: Vec::new(),
        }
    }
}

impl FontFaceDescriptor {
    pub(super) fn admitted_bytes(&self) -> u64 {
        // Original family + its lookup key; original ranges + the maximum
        // coverage-index/sorting payload. Fixed headers/allocator overhead are
        // bounded separately by face/cache counts and are not logical payload.
        (self.family.len() as u64)
            .saturating_mul(2)
            .saturating_add((self.unicode_ranges.len() as u64).saturating_mul(16))
    }

    pub(super) fn validate(&self) -> Result<(), FontError> {
        let ordered = |(min, max): (f32, f32)| min.is_finite() && max.is_finite() && min <= max;
        if self.family.trim().is_empty()
            || !ordered(self.weight)
            || self.weight.0 < 1.
            || self.weight.1 > 1000.
            || !ordered(self.width)
            || self.width.0 <= 0.
            || matches!(self.style, FontStyle::Oblique(a) if !a.is_finite() || !(-90. ..=90.).contains(&a))
            || self
                .unicode_ranges
                .iter()
                .any(|&(min, max)| min > max || max > 0x10ffff)
        {
            return Err(FontError::Malformed("invalid CSS font descriptor"));
        }
        Ok(())
    }
}

/// Immutable registered CSS metadata. Matching shares ownership instead of
/// cloning caller-sized strings/ranges. Keep the public descriptor unchanged
/// and use a separate sorted union only for internal nominal coverage.
#[derive(Clone, Debug)]
pub(super) struct RegisteredDescriptor(Arc<Payload>);

#[derive(Debug)]
struct Payload {
    css: FontFaceDescriptor,
    coverage: Box<[(u32, u32)]>,
    coverage_hash: u64,
    bytes: u64,
}

impl RegisteredDescriptor {
    /// The caller has admitted and validated CSS input, or constructed an
    /// intrinsic descriptor with no caller-controlled ranges.
    pub(super) fn new(mut css: FontFaceDescriptor) -> Self {
        #[cfg(test)]
        INDEX_BUILDS.with(|count| count.set(count.get() + 1));
        let bytes = css.admitted_bytes();
        // Do not retain caller-owned oversized spare capacity. Conversion may
        // shrink allocations, but leaves range ordering and duplicates intact.
        css.family = css.family.into_boxed_str().into_string();
        css.unicode_ranges = css.unicode_ranges.into_boxed_slice().into_vec();
        let mut coverage = css.unicode_ranges.clone();
        coverage.sort_unstable();
        let mut retained = 0;
        for i in 0..coverage.len() {
            let (min, max) = coverage[i];
            if retained != 0 && min <= coverage[retained - 1].1.saturating_add(1) {
                coverage[retained - 1].1 = coverage[retained - 1].1.max(max);
            } else {
                coverage[retained] = (min, max);
                retained += 1;
            }
        }
        coverage.truncate(retained);
        // Hash once during bounded registration, never per matching query.
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        coverage.hash(&mut hash);
        let coverage_hash = hash.finish();
        Self(Arc::new(Payload {
            css,
            coverage: coverage.into_boxed_slice(),
            coverage_hash,
            bytes,
        }))
    }

    pub(super) fn bytes(&self) -> u64 {
        self.0.bytes
    }

    pub(super) fn cache_bytes(&self) -> usize {
        // Cache accounting includes the moved descriptor/union headers and
        // Arc counters as well. Registered faces already share this allocation,
        // but intrinsic candidates can own it solely through the cache.
        (size_of::<Payload>() + 2 * size_of::<usize>())
            .saturating_add(usize::try_from(self.bytes()).unwrap_or(usize::MAX))
    }

    pub(super) fn covers(&self, ch: char) -> bool {
        if self.0.coverage.is_empty() {
            return true;
        }
        let scalar = ch as u32;
        let at = self.0.coverage.partition_point(|&(min, _)| {
            #[cfg(test)]
            RANGE_PROBES.with(|count| count.set(count.get() + 1));
            min <= scalar
        });
        at != 0 && scalar <= self.0.coverage[at - 1].1
    }

    pub(super) fn coverage_hash(&self) -> u64 {
        self.0.coverage_hash
    }

    pub(super) fn same_coverage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || self.0.coverage_hash == other.0.coverage_hash && self.0.coverage == other.0.coverage
    }
}

impl Deref for RegisteredDescriptor {
    type Target = FontFaceDescriptor;

    fn deref(&self) -> &Self::Target {
        &self.0.css
    }
}
