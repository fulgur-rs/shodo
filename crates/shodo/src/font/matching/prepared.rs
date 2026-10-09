//! Character-independent registered metadata and CSS property ranks.
//!
//! Blobs and source paths are already retained by the catalog and are shared.
//! Bound the additional owned storage, including descriptors, axes and keys.
use super::{
    Candidate, FontCluster, FontPresentation, FontQuery, is_color, range_rank, style_rank,
    weight_rank,
};
use crate::style::FontStyle;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering::Relaxed},
};

pub(super) const MAX_ENTRIES: usize = 16;
const MAX_CANDIDATES: usize = 256;
const MAX_BYTES: usize = 64 * 1024;
type PropertyRank = ((u8, f32), (u8, f32), (u8, f32));

pub(super) struct Candidates {
    faces: Box<[(Candidate, PropertyRank)]>,
    bytes: usize,
}
impl Candidates {
    pub(super) fn can_retain(candidates: &[Candidate], name: Option<&str>) -> bool {
        if candidates.len() > MAX_CANDIDATES
            || name.is_some_and(|n| n.len() > super::MAX_CACHE_KEY_BYTES)
        {
            return false;
        }
        // Reject large inputs before converting the candidate Vec. This is
        // conservative: named filtering might discard some of these faces.
        let mut bytes =
            MAX_ENTRIES * size_of::<Entry>() + size_of::<Self>() + 2 * size_of::<usize>();
        bytes = bytes
            .saturating_add(name.map_or(0, str::len))
            .saturating_add(
                candidates
                    .len()
                    .saturating_mul(size_of::<(Candidate, PropertyRank)>()),
            );
        for candidate in candidates {
            bytes = bytes.saturating_add(candidate_bytes(candidate));
            if bytes > MAX_BYTES {
                return false;
            }
        }
        bytes <= MAX_BYTES
    }
    pub(super) fn new(candidates: Vec<Candidate>, query: &FontQuery, named: bool) -> Self {
        let mut faces: Vec<_> = candidates
            .into_iter()
            .filter(|c| {
                c.info.axes().iter().all(|a| {
                    a.min.is_finite()
                        && a.default.is_finite()
                        && a.max.is_finite()
                        && a.min <= a.default
                        && a.default <= a.max
                })
            })
            .map(|c| {
                #[cfg(test)]
                super::matching_tests::record_prepared_rank();
                let rank = (
                    range_rank(query.width, c.descriptor.width, 100.),
                    style_rank(query.style, c.descriptor.style),
                    weight_rank(query.weight, c.descriptor.weight),
                );
                (c, rank)
            })
            .collect();
        if named
            && let Some(best) = faces
                .iter()
                .map(|(_, rank)| *rank)
                .min_by(|a, b| a.partial_cmp(b).unwrap())
        {
            // Preserve attribute selection before coverage, including composites.
            faces.retain(|(_, rank)| *rank == best);
        }
        let faces = faces.into_boxed_slice();
        let mut bytes = size_of::<Self>() + 2 * size_of::<usize>() + size_of_val(&*faces);
        for (candidate, _) in &faces {
            bytes = bytes.saturating_add(candidate_bytes(candidate));
        }
        Self { faces, bytes }
    }
    pub(super) fn best(
        &self,
        cluster: &mut FontCluster<'_>,
        presentation: FontPresentation,
    ) -> Option<&Candidate> {
        let color = cluster.prefer_color(presentation);
        let mut best: Option<(&Candidate, (u8, PropertyRank))> = None;
        for (candidate, property) in &self.faces {
            #[cfg(test)]
            crate::font::record_metric_font_ref_open();
            #[cfg(test)]
            super::matching_tests::record_font_read();
            let Ok(font) =
                skrifa::FontRef::from_index(candidate.data.data.as_ref(), candidate.data.index)
            else {
                continue;
            };
            if !cluster.covers(candidate, &font) {
                continue;
            }
            let rank = (u8::from(is_color(&font) != color), *property);
            if best.is_none_or(|(old, old_rank)| {
                rank.partial_cmp(&old_rank)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| candidate.order_cmp(old))
                    == std::cmp::Ordering::Less
            }) {
                best = Some((candidate, rank));
            }
        }
        best.map(|(candidate, _)| candidate)
    }
}

fn candidate_bytes(candidate: &Candidate) -> usize {
    // FontInfo clones its SmallVec axes. Charge a conservative rounded
    // capacity even when the one-axis inline buffer needs no allocation.
    let axes = candidate
        .info
        .axes()
        .len()
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX);
    candidate
        .descriptor
        .family
        .capacity()
        .saturating_add(
            candidate
                .descriptor
                .unicode_ranges
                .capacity()
                .saturating_mul(size_of::<(u32, u32)>()),
        )
        .saturating_add(axes.saturating_mul(size_of::<fontique::AxisInfo>()))
}

struct Entry {
    name: Option<Box<str>>,
    weight: f32,
    width: f32,
    style: FontStyle,
    candidates: Arc<Candidates>,
    used: AtomicU64,
}
impl Entry {
    fn matches(&self, name: Option<&str>, query: &FontQuery) -> bool {
        match (self.name.as_deref(), name) {
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => {}
            (None, None) => {}
            _ => return false,
        }
        self.weight == query.weight && self.width == query.width && self.style == query.style
    }
    fn bytes(&self) -> usize {
        self.candidates
            .bytes
            .saturating_add(self.name.as_ref().map_or(0, |n| n.len()))
    }
}

#[derive(Default)]
pub(in crate::font) struct Cache {
    slots: Vec<Entry>,
    generations: Option<(u64, Option<u64>)>,
    clock: AtomicU64,
}
impl Cache {
    pub(super) fn sync_generations(&mut self, generations: (u64, Option<u64>)) {
        if self.generations != Some(generations) {
            self.slots.clear();
            self.generations = Some(generations);
        }
    }
    pub(super) fn get(
        &self,
        generations: (u64, Option<u64>),
        name: Option<&str>,
        query: &FontQuery,
    ) -> Option<Arc<Candidates>> {
        if self.generations != Some(generations)
            || name.is_some_and(|n| n.len() > super::MAX_CACHE_KEY_BYTES)
        {
            return None;
        }
        let entry = self.slots.iter().find(|entry| entry.matches(name, query))?;
        entry
            .used
            .fetch_max(self.clock.fetch_add(1, Relaxed), Relaxed);
        Some(entry.candidates.clone())
    }
    pub(super) fn insert(
        &mut self,
        cap: usize,
        generations: (u64, Option<u64>),
        name: Option<&str>,
        query: &FontQuery,
        candidates: Arc<Candidates>,
    ) {
        self.sync_generations(generations);
        let cap = cap.min(MAX_ENTRIES);
        // Account for the maximum slots allocation as well as all payloads.
        let overhead = MAX_ENTRIES * size_of::<Entry>();
        let bytes = candidates.bytes.saturating_add(name.map_or(0, str::len));
        if cap == 0
            || candidates.faces.len() > MAX_CANDIDATES
            || bytes > MAX_BYTES - overhead
            || name.is_some_and(|n| n.len() > super::MAX_CACHE_KEY_BYTES)
        {
            return;
        }
        if self.slots.iter().any(|entry| entry.matches(name, query)) {
            return;
        }
        while !self.slots.is_empty()
            && (self.slots.len() >= cap
                || self
                    .slots
                    .iter()
                    .map(|e| e.candidates.faces.len())
                    .sum::<usize>()
                    + candidates.faces.len()
                    > MAX_CANDIDATES
                || self.slots.iter().map(Entry::bytes).sum::<usize>() + bytes
                    > MAX_BYTES - overhead)
        {
            let victim = self
                .slots
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.used.load(Relaxed))
                .unwrap()
                .0;
            self.slots.swap_remove(victim);
        }
        self.slots.push(Entry {
            name: name.map(Into::into),
            weight: query.weight,
            width: query.width,
            style: query.style,
            candidates,
            used: AtomicU64::new(self.clock.fetch_add(1, Relaxed)),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions};
    use crate::limits::Limits;
    use crate::style::FontFamily;
    fn collection(cap: usize) -> FontCollection {
        let fonts = FontCollection::with_options(
            &Limits::unlimited(),
            FontOptions {
                system_fonts: false,
                match_cache_entries: cap,
                ..Default::default()
            },
        );
        fonts
            .register_face(
                super::super::super::browser_tests::test_font("Internal", &['a', 'b'], 600),
                0,
                FontFaceDescriptor {
                    family: "Web".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        fonts
    }
    fn query(weight: f32) -> FontQuery {
        FontQuery {
            families: vec![FontFamily::Named("Web".into())],
            weight,
            ..Default::default()
        }
    }
    #[test]
    fn cache_is_optional_lru_and_shares_ascii_family_names() {
        let disabled = collection(0);
        for text in ["a", "b"] {
            assert!(disabled.match_cluster(&query(400.), text).is_some());
        }
        assert!(disabled.caches().prepared.slots.is_empty());
        let fonts = collection(2);
        for weight in [400., 500.] {
            fonts.match_cluster(&query(weight), "a");
        }
        let mut same = query(400.);
        same.families = vec![FontFamily::Named("wEB".into())];
        fonts.match_cluster(&same, "b");
        fonts.match_cluster(&query(600.), "a");
        let cache = fonts.caches();
        assert_eq!(cache.prepared.slots.len(), 2);
        assert!(
            cache
                .prepared
                .get(fonts.generations(), Some("WEB"), &query(400.))
                .is_some()
        );
        assert!(
            cache
                .prepared
                .get(fonts.generations(), Some("Web"), &query(500.))
                .is_none()
        );
    }
    #[test]
    fn generations_release_old_payloads_in_both_layers() {
        let root = collection(8);
        let doc = FontCollection::for_document(&root, &Limits::unlimited());
        doc.match_cluster(&query(400.), "a");
        let old_root = Arc::downgrade(&root.caches().prepared.slots[0].candidates);
        let old_doc = Arc::downgrade(&doc.caches().prepared.slots[0].candidates);
        let new_id = root
            .register_face(
                super::super::super::browser_tests::test_font("New", &['a', 'b'], 800),
                0,
                FontFaceDescriptor {
                    family: "Web".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(doc.match_cluster(&query(400.), "b").unwrap().id, new_id);
        assert!(old_root.upgrade().is_none());
        assert!(old_doc.upgrade().is_none());
        let old = Arc::downgrade(&doc.caches().prepared.slots[0].candidates);
        let local_id = doc
            .register_face(
                super::super::super::browser_tests::test_font("Local", &['a'], 900),
                0,
                FontFaceDescriptor {
                    family: "Web".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(doc.match_cluster(&query(400.), "a").unwrap().id, local_id);
        assert!(old.upgrade().is_none());
        let last = Arc::downgrade(&doc.caches().prepared.slots[0].candidates);
        drop(doc);
        assert!(last.upgrade().is_none());
    }
    #[test]
    fn entries_candidates_and_owned_bytes_are_bounded() {
        let fonts = collection(256);
        // Enough retained candidates to exercise the aggregate candidate/byte
        // limits across different requests, rather than only the entry limit.
        for _ in 0..24 {
            fonts
                .register_face(
                    super::super::super::browser_tests::test_font("Internal", &['a', 'b'], 600),
                    0,
                    FontFaceDescriptor {
                        family: "Web".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        for weight in 1..=1000 {
            fonts.match_cluster(&query(weight as f32), "a");
            let cache = fonts.caches();
            let cache = &cache.prepared;
            assert!(cache.slots.len() <= MAX_ENTRIES);
            assert!(
                cache
                    .slots
                    .iter()
                    .map(|e| e.candidates.faces.len())
                    .sum::<usize>()
                    <= MAX_CANDIDATES
            );
            assert!(
                cache.slots.capacity() * size_of::<Entry>()
                    + cache.slots.iter().map(Entry::bytes).sum::<usize>()
                    <= MAX_BYTES
            );
        }
    }
    #[test]
    fn oversized_keys_candidates_and_descriptors_are_not_retained() {
        let fonts = collection(256);
        let generation = fonts.generations();
        let candidates = Arc::new(Candidates::new(
            fonts.registered_candidates(Some("Web")),
            &query(400.),
            true,
        ));
        fonts.write_caches().prepared.insert(
            256,
            generation,
            Some(&"x".repeat(super::super::MAX_CACHE_KEY_BYTES + 1)),
            &query(400.),
            candidates,
        );
        assert!(fonts.caches().prepared.slots.is_empty());
        for _ in 0..MAX_CANDIDATES {
            fonts
                .register_face(
                    super::super::super::browser_tests::test_font("Internal", &['a', 'b'], 600),
                    0,
                    FontFaceDescriptor {
                        family: "Web".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let expected = fonts.best_match(
            fonts.registered_candidates(Some("Web")),
            &query(400.),
            &mut FontCluster::new("a"),
            true,
        );
        assert_eq!(
            fonts.registered_match(Some("Web"), &query(400.), &mut FontCluster::new("a")),
            expected
        );
        assert!(fonts.caches().prepared.slots.is_empty());
        let huge = collection(8);
        huge.register_face(
            super::super::super::browser_tests::test_font("Huge", &['a', 'b'], 600),
            0,
            FontFaceDescriptor {
                family: "Huge".into(),
                unicode_ranges: vec![(97, 98); MAX_BYTES / 8 + 1],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            huge.registered_match(Some("Huge"), &query(400.), &mut FontCluster::new("a"))
                .is_some()
        );
        assert!(huge.caches().prepared.slots.is_empty());
    }
}
