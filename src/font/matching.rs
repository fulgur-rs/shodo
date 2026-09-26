//! Cluster matching, CSS range ranking, and shared fallback configuration.

use super::{FontCollection, FontData, FontFaceDescriptor, FontId, LayerState, check};
use crate::limits::{LimitKind, Limits};
use crate::style::{FontFamily, FontStyle, FontSynthesis, FontVariation, GenericFamily};
use fontique::{FontInfo, SourceId, SourceInfo, SourceKind};
use skrifa::{FontRef, MetadataProvider};

/// Requested glyph presentation. Auto honors VS15/VS16 and UTS #51 defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontPresentation {
    #[default]
    Auto,
    Text,
    Emoji,
}

/// A resolved style query for one extended grapheme cluster. `script` is an
/// ISO 15924 tag; `language` is a BCP 47 language tag supplied by the caller.
#[derive(Clone, Debug, PartialEq)]
pub struct FontQuery {
    pub families: Vec<FontFamily>,
    pub weight: f32,
    pub width: f32,
    pub style: FontStyle,
    pub script: [u8; 4],
    pub language: Option<String>,
    pub presentation: FontPresentation,
    pub synthesis: FontSynthesis,
}
impl Default for FontQuery {
    fn default() -> Self {
        Self {
            families: vec![FontFamily::Generic(GenericFamily::SansSerif)],
            weight: 400.,
            width: 100.,
            style: FontStyle::Normal,
            script: *b"Latn",
            language: None,
            presentation: FontPresentation::Auto,
            synthesis: FontSynthesis::default(),
        }
    }
}

/// The selected stable face and the adjustments needed by a shaper/renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct FontMatch {
    pub id: FontId,
    pub variations: Vec<FontVariation>,
    pub embolden: bool,
    pub skew: Option<f32>,
}

pub(super) struct CacheEntry {
    query: FontQuery,
    cluster: String,
    generations: (u64, Option<u64>),
    result: Option<FontMatch>,
}
pub(super) struct FallbackEntry {
    script: [u8; 4],
    language: Option<String>,
    families: Vec<String>,
}
struct Candidate {
    id: FontId,
    data: FontData,
    descriptor: FontFaceDescriptor,
    info: FontInfo,
}

impl FontCollection {
    pub(super) fn root(&self) -> &Self {
        self.layer.parent.as_ref().unwrap_or(self)
    }

    /// Sets a deterministic generic family list in the shared layer. Applies
    /// to existing document collections and invalidates their cached matches.
    pub fn set_generic_families(&self, generic: GenericFamily, families: Vec<String>) {
        let root = self.root();
        root.state().generics.insert(generic, families);
        root.layer
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Sets fallback families for a script and optional language. The longest
    /// matching language prefix wins (e.g. ja matches ja-JP). None is default.
    pub fn set_fallback_families(
        &self,
        script: [u8; 4],
        language: Option<String>,
        families: Vec<String>,
    ) {
        let root = self.root();
        let language = language.map(|s| s.to_ascii_lowercase());
        let mut state = root.state();
        state
            .fallbacks
            .retain(|entry| entry.script != script || entry.language != language);
        state.fallbacks.push(FallbackEntry {
            script,
            language,
            families,
        });
        root.layer
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// Matches the entire caller-supplied grapheme, never a partial face.
    /// Nominal cmap checks ignore joiners and variation selectors; S2 shapes
    /// the chosen face to resolve sequence substitutions. None means missing.
    pub fn match_cluster(&self, query: &FontQuery, cluster: &str) -> Option<FontMatch> {
        if cluster.is_empty() {
            return None;
        }
        let mut query = query.clone();
        query.weight = finite(query.weight, 400.).clamp(1., 1000.);
        query.width = finite(query.width, 100.).max(0.01);
        if let FontStyle::Oblique(angle) = &mut query.style {
            *angle = finite(*angle, 14.).clamp(-90., 90.);
        }
        query.language = query.language.map(|s| s.to_ascii_lowercase());
        let generations = self.generations();
        {
            let mut state = self.state();
            if let Some(index) = state.matches.iter().position(|entry| {
                entry.generations == generations && entry.query == query && entry.cluster == cluster
            }) {
                let entry = state.matches.remove(index)?;
                let result = entry.result.clone();
                state.matches.push_back(entry);
                return result;
            }
        }
        let result = self.find_cluster(&query, cluster);
        let mut state = self.state();
        let cap = state.options.match_cache_entries;
        // Keep each retained key bounded even for direct, untrusted API calls.
        let key_bytes = cluster.len()
            + query
                .families
                .iter()
                .map(|f| match f {
                    FontFamily::Named(n) => n.len(),
                    _ => 0,
                })
                .sum::<usize>()
            + query.language.as_ref().map_or(0, String::len);
        if cap > 0 && key_bytes <= 4096 && query.families.len() <= 128 {
            while state.matches.len() >= cap {
                state.matches.pop_front();
            }
            state.matches.push_back(CacheEntry {
                query,
                cluster: cluster.into(),
                generations,
                result: result.clone(),
            });
        }
        result
    }

    fn find_cluster(&self, query: &FontQuery, cluster: &str) -> Option<FontMatch> {
        for family in &query.families {
            let names = match family {
                FontFamily::Named(name) => vec![name.clone()],
                FontFamily::Generic(generic) => self.generic_names(*generic),
            };
            for name in names {
                if let Some(found) = self.named_match(&name, query, cluster) {
                    return Some(found);
                }
            }
        }
        let fallback_names = {
            let mut state = self.root().state();
            self.root().ensure_system(&mut state);
            let configured = state
                .fallbacks
                .iter()
                .filter(|entry| {
                    entry.script == query.script
                        && match &entry.language {
                            None => true,
                            Some(lang) => query.language.as_ref().is_some_and(|requested| {
                                requested == lang
                                    || requested
                                        .strip_prefix(lang)
                                        .is_some_and(|tail| tail.starts_with('-'))
                            }),
                        }
                })
                .max_by_key(|entry| entry.language.as_ref().map_or(0, String::len));
            if let Some(entry) = configured {
                entry.families.clone()
            } else {
                let lang = query
                    .language
                    .as_ref()
                    .and_then(|s| fontique::Language::parse(s).ok());
                let key = fontique::FallbackKey::new(
                    fontique::Script::from_bytes(query.script),
                    lang.as_ref(),
                );
                let ids: Vec<_> = state.native.fallback_families(key).collect();
                ids.into_iter()
                    .filter_map(|id| state.native.family_name(id).map(str::to_owned))
                    .collect()
            }
        };
        for name in fallback_names {
            if let Some(found) = self.named_match(&name, query, cluster) {
                return Some(found);
            }
        }
        // Last resort is deterministic registration order, still whole-cluster.
        self.best_match(self.registered_candidates(None), query, cluster, false)
            .or_else(|| {
                self.layer.parent.as_ref().and_then(|parent| {
                    parent.best_match(parent.registered_candidates(None), query, cluster, false)
                })
            })
    }

    fn named_match(&self, name: &str, query: &FontQuery, cluster: &str) -> Option<FontMatch> {
        self.best_match(self.registered_candidates(Some(name)), query, cluster, true)
            .or_else(|| self.best_match(self.native_candidates(name), query, cluster, true))
            .or_else(|| {
                self.layer
                    .parent
                    .as_ref()
                    .and_then(|p| p.named_match(name, query, cluster))
            })
    }

    fn generic_names(&self, generic: GenericFamily) -> Vec<String> {
        let root = self.root();
        let mut state = root.state();
        if let Some(names) = state.generics.get(&generic) {
            return names.clone();
        }
        root.ensure_system(&mut state);
        let native = match generic {
            GenericFamily::Serif => fontique::GenericFamily::Serif,
            GenericFamily::SansSerif => fontique::GenericFamily::SansSerif,
            GenericFamily::Monospace => fontique::GenericFamily::Monospace,
            GenericFamily::Cursive => fontique::GenericFamily::Cursive,
            GenericFamily::Fantasy => fontique::GenericFamily::Fantasy,
            GenericFamily::SystemUi => fontique::GenericFamily::SystemUi,
        };
        let ids: Vec<_> = state.native.generic_families(native).collect();
        ids.into_iter()
            .filter_map(|id| state.native.family_name(id).map(str::to_owned))
            .collect()
    }

    pub(super) fn ensure_system(&self, state: &mut LayerState) {
        if state.options.system_fonts && self.layer.parent.is_none() && !state.system_loaded {
            state.native.load_system_fonts();
            state.system_loaded = true;
        }
    }

    fn registered_candidates(&self, name: Option<&str>) -> Vec<Candidate> {
        let state = self.state();
        state
            .faces
            .iter()
            .enumerate()
            .filter_map(|(index, data)| {
                let info = FontInfo::from_source(
                    SourceInfo::new(SourceId::new(), SourceKind::Memory(data.data.clone())),
                    data.index,
                )?;
                let descriptor = match &state.descriptors[index] {
                    Some(desc)
                        if name.is_none_or(|name| desc.family.eq_ignore_ascii_case(name)) =>
                    {
                        desc.clone()
                    }
                    Some(_) => return None,
                    None if name.is_none() => intrinsic_descriptor(&info, String::new()),
                    None => return None, // Named intrinsic families come from fontique.
                };
                Some(Candidate {
                    id: FontId {
                        layer: self.layer.id,
                        index: index as u32,
                    },
                    data: data.clone(),
                    descriptor,
                    info,
                })
            })
            .collect()
    }

    fn native_candidates(&self, name: &str) -> Vec<Candidate> {
        let mut state = self.state();
        self.ensure_system(&mut state);
        let Some(family) = state.native.family_by_name(name) else {
            return vec![];
        };
        let mut result = Vec::new();
        for info in family.fonts() {
            let Some(blob) = state
                .retained_sources
                .get(&info.source().id())
                .cloned()
                .or_else(|| info.load(Some(&mut state.source_cache)))
            else {
                continue;
            };
            let data = FontData::new(blob, info.index());
            let index = match state
                .faces
                .iter()
                .position(|face| face.data.id() == data.data.id() && face.index == data.index)
            {
                Some(index) => index,
                None => {
                    // Trusted platform faces are loaded only when selected below.
                    // Use a temporary id whose slot is materialized by best_match.
                    usize::MAX
                }
            };
            result.push(Candidate {
                id: FontId {
                    layer: self.layer.id,
                    index: if index == usize::MAX {
                        u32::MAX
                    } else {
                        index as u32
                    },
                },
                descriptor: intrinsic_descriptor(info, name.into()),
                data,
                info: info.clone(),
            });
        }
        let age = state.options.source_cache_max_age;
        state.source_cache.prune(age, true);
        result
    }

    fn best_match(
        &self,
        mut candidates: Vec<Candidate>,
        query: &FontQuery,
        cluster: &str,
        select_style: bool,
    ) -> Option<FontMatch> {
        let property_rank = |c: &Candidate| {
            (
                range_rank(query.width, c.descriptor.width, 100.),
                style_rank(query.style, c.descriptor.style),
                weight_rank(query.weight, c.descriptor.weight),
            )
        };
        if select_style {
            let best = candidates
                .iter()
                .map(&property_rank)
                .min_by(|a, b| a.partial_cmp(b).unwrap())?;
            // A family selects its style before testing character coverage.
            // Equal-ranked faces can still form a unicode-range composite.
            candidates.retain(|c| property_rank(c) == best);
        }
        let color = prefer_color(query.presentation, cluster);
        candidates.retain(|candidate| covers(candidate, cluster));
        candidates.sort_by(|a, b| {
            let rank = |c: &Candidate| {
                (
                    u8::from(is_color(&c.data) != color),
                    range_rank(query.width, c.descriptor.width, 100.),
                    style_rank(query.style, c.descriptor.style),
                    weight_rank(query.weight, c.descriptor.weight),
                )
            };
            rank(a)
                .partial_cmp(&rank(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.id.index.cmp(&a.id.index))
        });
        let mut selected = candidates.into_iter().next()?;
        if selected.id.index == u32::MAX {
            let mut state = self.state();
            let source = selected.info.source().id();
            if let Some(blob) = state.retained_sources.get(&source) {
                selected.data.data = blob.clone();
            }
            let index = if let Some(index) = state.faces.iter().position(|face| {
                face.data.id() == selected.data.data.id() && face.index == selected.data.index
            }) {
                index
            } else {
                let limits = &self.layer.limits;
                check::check_font(selected.data.data.as_ref(), limits).ok()?;
                Limits::check(
                    limits.max_faces_per_layer,
                    LimitKind::FacesPerLayer,
                    state.faces.len() as u64 + 1,
                )
                .ok()?;
                let additional = if state
                    .faces
                    .iter()
                    .any(|face| face.data.id() == selected.data.data.id())
                {
                    0
                } else {
                    selected.data.data.len() as u64
                };
                let bytes = state.blob_bytes.checked_add(additional)?;
                Limits::check(
                    limits.max_layer_blob_bytes,
                    LimitKind::LayerBlobBytes,
                    bytes,
                )
                .ok()?;
                let index = state.faces.len();
                state.faces.push(selected.data.clone());
                state.descriptors.push(None);
                state.blob_bytes = bytes;
                index
            };
            state
                .retained_sources
                .insert(source, selected.data.data.clone());
            selected.id.index = index as u32;
            selected.id.layer = self.layer.id;
        }
        let info = &selected.info;
        let synthesis = info.synthesis(
            fontique::FontWidth::from_percentage(query.width),
            native_style(query.style),
            fontique::FontWeight::new(query.weight),
        );
        let mut variations: Vec<_> = synthesis
            .variation_settings()
            .iter()
            .map(|(tag, value)| FontVariation {
                tag: tag.to_be_bytes(),
                value: *value,
            })
            .collect();
        // Emit property axes even when the intrinsic default equals the
        // request: a CSS descriptor may clamp that request to another value.
        let axis_style = if self.face_descriptor(selected.id).is_some() {
            selected.descriptor.style
        } else {
            query.style
        };
        for axis in info.axes() {
            let tag = axis.tag.to_be_bytes();
            let property = match &tag {
                b"wght" => Some(
                    query
                        .weight
                        .clamp(selected.descriptor.weight.0, selected.descriptor.weight.1),
                ),
                b"wdth" => Some(
                    query
                        .width
                        .clamp(selected.descriptor.width.0, selected.descriptor.width.1),
                ),
                b"slnt" => Some(match axis_style {
                    FontStyle::Normal => 0.,
                    FontStyle::Italic if info.has_italic_axis() => 0.,
                    FontStyle::Italic => -14.,
                    FontStyle::Oblique(angle) => -angle,
                }),
                b"ital" => Some(if axis_style == FontStyle::Italic {
                    1.
                } else {
                    0.
                }),
                _ => None,
            };
            if let Some(value) = property {
                variations.retain(|v| v.tag != tag);
                variations.push(FontVariation { tag, value });
            }
        }
        for var in &mut variations {
            if let Some(axis) = info
                .axes()
                .iter()
                .find(|axis| axis.tag.to_be_bytes() == var.tag)
            {
                var.value = var.value.clamp(axis.min, axis.max);
            }
        }
        let embolden = query.synthesis.weight
            && !info.has_weight_axis()
            && query.weight > selected.descriptor.weight.1;
        let skew = if query.synthesis.style
            && selected.descriptor.style == FontStyle::Normal
            && !info.has_slant_axis()
            && !info.has_italic_axis()
        {
            match query.style {
                FontStyle::Normal => None,
                FontStyle::Italic => Some(14.),
                FontStyle::Oblique(a) => Some(a),
            }
        } else {
            None
        };
        Some(FontMatch {
            id: selected.id,
            variations,
            embolden,
            skew,
        })
    }
}

fn finite(value: f32, default: f32) -> f32 {
    if value.is_finite() { value } else { default }
}
fn native_style(style: FontStyle) -> fontique::FontStyle {
    match style {
        FontStyle::Normal => fontique::FontStyle::Normal,
        FontStyle::Italic => fontique::FontStyle::Italic,
        FontStyle::Oblique(a) => fontique::FontStyle::Oblique(Some(a)),
    }
}
fn intrinsic_descriptor(info: &FontInfo, family: String) -> FontFaceDescriptor {
    let weight = info.weight().value();
    let width = info.width().percentage();
    let mut descriptor = FontFaceDescriptor {
        family,
        weight: (weight, weight),
        width: (width, width),
        style: match info.style() {
            fontique::FontStyle::Normal => FontStyle::Normal,
            fontique::FontStyle::Italic => FontStyle::Italic,
            fontique::FontStyle::Oblique(a) => FontStyle::Oblique(a.unwrap_or(14.)),
        },
        unicode_ranges: vec![],
    };
    for axis in info.axes() {
        match &axis.tag.to_be_bytes() {
            b"wght" => descriptor.weight = (axis.min, axis.max),
            b"wdth" => descriptor.width = (axis.min, axis.max),
            _ => {}
        }
    }
    descriptor
}
fn ignored(ch: char) -> bool {
    matches!(ch,'\u{200c}'|'\u{200d}'|'\u{fe00}'..='\u{fe0f}'|'\u{e0100}'..='\u{e01ef}')
}
fn covers(candidate: &Candidate, cluster: &str) -> bool {
    let Ok(font) = FontRef::from_index(candidate.data.data.as_ref(), candidate.data.index) else {
        return false;
    };
    let map = font.charmap();
    cluster.chars().filter(|&ch| !ignored(ch)).all(|ch| {
        let ranges = &candidate.descriptor.unicode_ranges;
        (ranges.is_empty()
            || ranges
                .iter()
                .any(|&(min, max)| (min..=max).contains(&(ch as u32))))
            && map.map(ch).is_some_and(|id| id.to_u32() != 0)
    })
}
fn is_color(data: &FontData) -> bool {
    let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
        return false;
    };
    [*b"COLR", *b"CBDT", *b"sbix", *b"SVG "]
        .iter()
        .any(|tag| font.table_data(skrifa::raw::types::Tag::new(tag)).is_some())
}
fn prefer_color(presentation: FontPresentation, cluster: &str) -> bool {
    match presentation {
        FontPresentation::Emoji => true,
        FontPresentation::Text => false,
        FontPresentation::Auto => {
            if let Some(selector) = cluster
                .chars()
                .rev()
                .find(|&c| c == '\u{fe0e}' || c == '\u{fe0f}')
            {
                return selector == '\u{fe0f}';
            }
            cluster.chars().any(|ch|icu_properties::CodePointSetData::new::<icu_properties::props::EmojiPresentation>().contains(ch))
        }
    }
}
// CSS Fonts search direction around a pivot (width=100%; weight special below).
fn range_rank(requested: f32, (min, max): (f32, f32), pivot: f32) -> (u8, f32) {
    if (min..=max).contains(&requested) {
        return (0, 0.);
    }
    if requested <= pivot {
        if max < requested {
            (1, requested - max)
        } else {
            (2, min - requested)
        }
    } else if min > requested {
        (1, min - requested)
    } else {
        (2, requested - max)
    }
}
fn weight_rank(requested: f32, range: (f32, f32)) -> (u8, f32) {
    let (min, max) = range;
    if (min..=max).contains(&requested) {
        return (0, 0.);
    }
    if (400. ..=500.).contains(&requested) {
        if min > requested && min <= 500. {
            (1, min - requested)
        } else if max < requested {
            (2, requested - max)
        } else {
            (3, min - requested)
        }
    } else {
        range_rank(requested, range, 400.)
    }
}
// CSS Fonts 4 §5.2: directional search, with the 11-degree threshold.
fn style_rank(requested: FontStyle, available: FontStyle) -> (u8, f32) {
    match requested {
        FontStyle::Normal => match available {
            FontStyle::Normal => (0, 0.),
            FontStyle::Oblique(a) if a >= 0. => (1, a),
            FontStyle::Italic => (2, 0.),
            FontStyle::Oblique(a) => (3, -a),
        },
        FontStyle::Italic => match available {
            FontStyle::Italic => (0, 0.),
            FontStyle::Oblique(a) if a >= 11. => (1, a - 11.),
            FontStyle::Oblique(a) if a > 0. => (2, 11. - a),
            FontStyle::Normal => (3, 0.),
            FontStyle::Oblique(a) => (4, -a),
        },
        FontStyle::Oblique(angle) => match available {
            FontStyle::Oblique(candidate) if candidate == angle => (0, 0.),
            FontStyle::Oblique(candidate) => {
                let sign = if angle < 0. { -1. } else { 1. };
                let a = angle * sign;
                let b = candidate * sign;
                if b > 0. {
                    let preferred = if a >= 11. { b >= a } else { b <= a };
                    (if preferred { 1 } else { 2 }, (a - b).abs())
                } else {
                    (4, -b)
                }
            }
            FontStyle::Italic => (3, 0.),
            FontStyle::Normal => (5, 0.),
        },
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    fn reloaded(source: SourceId) -> Candidate {
        let blob = super::super::Blob::from(super::super::browser_tests::test_font(
            "Native",
            &['a'],
            600,
        ));
        let info =
            FontInfo::from_source(SourceInfo::new(source, SourceKind::Memory(blob.clone())), 0)
                .unwrap();
        Candidate {
            id: FontId {
                layer: 0,
                index: u32::MAX,
            },
            data: FontData::new(blob, 0),
            descriptor: intrinsic_descriptor(&info, "Native".into()),
            info,
        }
    }
    #[test]
    fn reloaded_platform_face_keeps_identity_and_blob() {
        let fonts = FontCollection::new(&crate::limits::Limits::default());
        let source = SourceId::new();
        let first = fonts
            .best_match(vec![reloaded(source)], &FontQuery::default(), "a", true)
            .unwrap();
        let blob = fonts.state().faces[1].data.id();
        for _ in 0..3 {
            let next = fonts
                .best_match(vec![reloaded(source)], &FontQuery::default(), "a", true)
                .unwrap();
            assert_eq!(next.id, first.id);
            assert_eq!(fonts.state().faces.len(), 2);
            assert_eq!(fonts.state().faces[1].data.id(), blob);
        }
        assert!(fonts.state().blob_bytes > 0);
    }
    #[test]
    fn platform_retention_obeys_face_and_blob_limits() {
        for limits in [
            crate::limits::Limits {
                max_faces_per_layer: Some(1),
                ..Default::default()
            },
            crate::limits::Limits {
                max_layer_blob_bytes: Some(0),
                ..Default::default()
            },
        ] {
            let fonts = FontCollection::new(&limits);
            assert!(
                fonts
                    .best_match(
                        vec![reloaded(SourceId::new())],
                        &FontQuery::default(),
                        "a",
                        true
                    )
                    .is_none()
            );
            assert_eq!(fonts.state().faces.len(), 1);
            assert_eq!(fonts.state().blob_bytes, 0);
        }
    }
}
