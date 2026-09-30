//! Browser font collections, document-local CSS faces, cluster matching and metrics.
//!
//! System enumeration is lazy; a bundled-only application can disable it:
//! ```
//! use shodo::font::{FontCollection, FontOptions, FontQuery};
//! use shodo::limits::Limits;
//! let shared = FontCollection::with_options(&Limits::default(), FontOptions {
//!     system_fonts: false, ..Default::default()
//! });
//! let document = FontCollection::for_document(&shared, &Limits::default());
//! // Without a zero glyph, CSS defines ch as half an em.
//! assert_eq!(document.resolve_ch(&FontQuery::default(), 16.0).advance, 8.0);
//! ```
//!
//! Register CSS sources without changing the caller's family list:
//! ```no_run
//! use shodo::font::{FontCollection, FontFaceDescriptor, FontSource, FontQuery};
//! use shodo::limits::Limits;
//! use shodo::style::FontFamily;
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let shared = FontCollection::new(&Limits::default());
//! let document = FontCollection::for_document(&shared, &Limits::default());
//! let face = document.register_sources(FontFaceDescriptor {
//!     family: "Document Font".into(), weight: (300.0, 700.0), ..Default::default()
//! }, vec![FontSource::Local("Installed Font-Regular".into()),
//!         FontSource::Data(std::fs::read("web-font.woff2")?, 0)])?;
//! let query = FontQuery { families: vec![FontFamily::Named("Document Font".into())],
//!     weight: 600.0, ..Default::default() };
//! if let Some(selected) = document.match_cluster(&query, "水") {
//!     let data = document.font_data(selected.id).unwrap();
//!     let metrics = document.metrics(selected.id, 16.0);
//!     let shaper = document.shaper_data(selected.id).unwrap();
//! }
//! # let _ = face;
//! # Ok(()) }
//! ```

mod ch;
mod check;
mod descriptor;
mod matching;
mod metrics;
mod source;
mod web_font;
pub use web_font::decode_web_font;
mod structure;
pub use ch::ChLength;
pub use descriptor::FontFaceDescriptor;
pub use matching::{FontMatch, FontPresentation, FontQuery};
pub use metrics::{FontUnit, VerticalFontMetrics};
pub use skrifa::instance::NormalizedCoord;
pub use source::FontSource;
pub(crate) mod sfnt;

pub use check::FontError;

use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use peniko::{Blob, FontData};

use crate::limits::{LimitKind, Limits};

#[cfg(test)]
std::thread_local! {
    pub(crate) static FONT_DATA_ACQUISITIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static NEXT_LAYER_ID: AtomicU32 = AtomicU32::new(0);

fn allocate_layer_id(counter: &AtomicU32) -> Option<u32> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
}

/// An observer for invalidating renderer atlas entries after a layer dies.
/// It never keeps a document or its font blobs alive.
#[derive(Clone, Debug)]
pub struct WeakFontLayer {
    id: u32,
    layer: std::sync::Weak<Layer>,
}
impl WeakFontLayer {
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn is_alive(&self) -> bool {
        self.layer.strong_count() != 0
    }
}

/// Identifies a face: the layer it was registered in and its index there.
/// Stable for the lifetime of the layer; layer ids are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId {
    layer: u32,
    index: u32,
}

impl FontId {
    pub fn layer(self) -> u32 {
        self.layer
    }

    pub fn index(self) -> u32 {
        self.index
    }
}

/// Font metrics in px for a given size. `descent` and `underline_offset`
/// are measured downward from the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub x_height: f32,
    pub cap_height: f32,
    /// Positive displacement below the alphabetic baseline.
    pub subscript_offset: f32,
    /// Positive displacement above the alphabetic baseline.
    pub superscript_offset: f32,
    pub underline_offset: f32,
    pub underline_thickness: f32,
    pub strikeout_offset: f32,
    pub strikeout_thickness: f32,
}

/// Platform access and bounded cache policy. Tests and applications using
/// bundled fonts can disable all system enumeration.
#[derive(Clone, Debug)]
pub struct FontOptions {
    pub system_fonts: bool,
    /// Maximum cached cluster queries. Zero disables this cache.
    pub match_cache_entries: usize,
    /// Source cache age in calls to `prune`, including failed file loads.
    pub source_cache_max_age: u64,
}
impl Default for FontOptions {
    fn default() -> Self {
        Self {
            system_fonts: cfg!(feature = "system-fonts"),
            match_cache_entries: 256,
            source_cache_max_age: 8,
        }
    }
}

struct Layer {
    id: u32,
    limits: Limits,
    generation: AtomicU64,
    state: Mutex<LayerState>,
    parent: Option<FontCollection>,
}

struct LayerState {
    faces: Vec<FontData>,
    face_infos: Vec<Option<fontique::FontInfo>>,
    blob_bytes: u64,
    descriptors: Vec<Option<FontFaceDescriptor>>,
    native: fontique::Collection,
    source_cache: fontique::SourceCache,
    retained_sources: std::collections::HashMap<fontique::SourceId, Blob<u8>>,
    system_loaded: bool,
    options: FontOptions,
    generics: std::collections::HashMap<crate::style::GenericFamily, Vec<String>>,
    fallbacks: Vec<matching::FallbackEntry>,
    matches: matching::MatchCache,
    units: metrics::UnitCache,
    shapers: std::collections::VecDeque<(u32, Arc<harfrust::ShaperData>)>,
}

/// A layer of fonts. The shared layer (created with [`FontCollection::new`])
/// holds application fonts; document layers (created with
/// [`FontCollection::for_document`]) hold `@font-face` fonts and see the
/// shared layer, but not each other. Cheap to clone.
#[derive(Clone)]
pub struct FontCollection {
    layer: Arc<Layer>,
}

impl fmt::Debug for FontCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontCollection")
            .field("layer", &self.layer.id)
            .finish()
    }
}

impl FontCollection {
    /// Creates the shared layer. It contains a built-in stub face at index 0,
    /// so layout works without any system or bundled fonts.
    pub fn new(limits: &Limits) -> Self {
        Self::with_options(limits, FontOptions::default())
    }

    /// Creates a shared collection with explicit platform/cache policy.
    pub fn with_options(limits: &Limits, options: FontOptions) -> Self {
        let stub = FontData::new(Blob::from(sfnt::build_sfnt(&[])), 0);
        Self::with_faces(limits, vec![stub], None, options)
    }

    /// Creates an empty document layer on top of the root shared layer.
    /// Passing a document collection does not expose its private faces.
    pub fn for_document(shared: &FontCollection, limits: &Limits) -> Self {
        let mut root = shared;
        while let Some(parent) = &root.layer.parent {
            root = parent;
        }
        Self::with_faces(
            limits,
            Vec::new(),
            Some(root.clone()),
            root.state().options.clone(),
        )
    }

    fn with_faces(
        limits: &Limits,
        faces: Vec<FontData>,
        parent: Option<FontCollection>,
        options: FontOptions,
    ) -> Self {
        Self {
            layer: Arc::new(Layer {
                id: allocate_layer_id(&NEXT_LAYER_ID).expect("font layer identity space exhausted"),
                limits: limits.clone(),
                generation: AtomicU64::new(0),
                state: Mutex::new(LayerState {
                    descriptors: vec![None; faces.len()],
                    face_infos: faces.iter().map(matching::face_info).collect(),
                    native: fontique::Collection::new(fontique::CollectionOptions {
                        shared: false,
                        system_fonts: false,
                    }),
                    faces,
                    blob_bytes: 0,
                    source_cache: fontique::SourceCache::default(),
                    retained_sources: Default::default(),
                    system_loaded: false,
                    options,
                    generics: Default::default(),
                    fallbacks: Vec::new(),
                    matches: Default::default(),
                    units: Default::default(),
                    shapers: Default::default(),
                }),
                parent,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, LayerState> {
        // A panic while holding the lock cannot leave the state inconsistent
        // (every update is a single push), so a poisoned lock is recovered.
        self.layer.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Checks and registers a font file or collection. Returns the id of its
    /// first face. On error nothing is stored.
    pub fn register(&self, data: Vec<u8>) -> Result<FontId, FontError> {
        let limits = &self.layer.limits;
        let faces = check::check_font(&data, limits)?;
        let mut state = self.state();
        Limits::check(
            limits.max_faces_per_layer,
            LimitKind::FacesPerLayer,
            state.faces.len() as u64 + u64::from(faces),
        )?;
        let bytes = state.blob_bytes + data.len() as u64;
        Limits::check(
            limits.max_layer_blob_bytes,
            LimitKind::LayerBlobBytes,
            bytes,
        )?;
        state.blob_bytes = bytes;
        let first = state.faces.len() as u32;
        let blob = Blob::from(data);
        let registered = state.native.register_fonts(blob.clone(), None);
        let mut infos = vec![None; faces as usize];
        for (_, family) in registered {
            for info in family {
                if let Some(slot) = infos.get_mut(info.index() as usize) {
                    *slot = Some(info);
                }
            }
        }
        for (index, info) in infos.into_iter().enumerate() {
            let data = FontData::new(blob.clone(), index as u32);
            // Fonts without intrinsic family names can still be last resorts.
            let info = info.or_else(|| matching::face_info(&data));
            state.face_infos.push(info);
            state.descriptors.push(None);
            state.faces.push(data);
        }
        self.layer.generation.fetch_add(1, Ordering::SeqCst);
        Ok(FontId {
            layer: self.layer.id,
            index: first,
        })
    }

    /// Registers one selected sfnt/TTC face with CSS descriptors. The whole
    /// blob is checked and budgeted, but only the selected face is retained.
    /// An invalid descriptor, index or font leaves the layer unchanged.
    pub fn register_face(
        &self,
        data: Vec<u8>,
        index: u32,
        descriptor: FontFaceDescriptor,
    ) -> Result<FontId, FontError> {
        self.register_blob(Blob::from(data), index, descriptor)
    }

    pub(super) fn register_blob(
        &self,
        blob: Blob<u8>,
        index: u32,
        descriptor: FontFaceDescriptor,
    ) -> Result<FontId, FontError> {
        self.register_source_blob(blob, index, descriptor, None)
    }

    pub(super) fn register_source_blob(
        &self,
        blob: Blob<u8>,
        index: u32,
        descriptor: FontFaceDescriptor,
        source: Option<fontique::SourceId>,
    ) -> Result<FontId, FontError> {
        descriptor.validate()?;
        let mut state = self.state();
        let blob = source
            .and_then(|id| state.retained_sources.get(&id).cloned())
            .unwrap_or(blob);
        let limits = &self.layer.limits;
        let count = check::check_font(blob.as_ref(), limits)?;
        if index >= count {
            return Err(FontError::Malformed("face index out of bounds"));
        }
        // Unlike the S0 stub, a CSS face must have a usable cmap.
        let data = FontData::new(blob.clone(), index);
        let info = matching::face_info(&data)
            .ok_or(FontError::Malformed("face has no usable character map"))?;
        Limits::check(
            limits.max_faces_per_layer,
            LimitKind::FacesPerLayer,
            state.faces.len() as u64 + 1,
        )?;
        let additional = if state.faces.iter().any(|face| face.data.id() == blob.id()) {
            0
        } else {
            blob.len() as u64
        };
        let bytes = state.blob_bytes + additional;
        Limits::check(
            limits.max_layer_blob_bytes,
            LimitKind::LayerBlobBytes,
            bytes,
        )?;
        let id = FontId {
            layer: self.layer.id,
            index: state.faces.len() as u32,
        };
        if let Some(source) = source {
            state.retained_sources.insert(source, blob.clone());
        }
        state.face_infos.push(Some(info));
        state.faces.push(data);
        state.descriptors.push(Some(descriptor));
        state.blob_bytes = bytes;
        self.layer.generation.fetch_add(1, Ordering::SeqCst);
        Ok(id)
    }

    /// CSS descriptors of an explicitly registered face, including shared
    /// faces visible through this document layer.
    pub fn face_descriptor(&self, id: FontId) -> Option<FontFaceDescriptor> {
        if id.layer == self.layer.id {
            self.state().descriptors.get(id.index as usize)?.clone()
        } else {
            self.layer.parent.as_ref()?.face_descriptor(id)
        }
    }

    /// Font data of a face in this layer or its shared layer.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        #[cfg(test)]
        FONT_DATA_ACQUISITIONS.with(|count| count.set(count.get() + 1));
        if id.layer == self.layer.id {
            self.state().faces.get(id.index as usize).cloned()
        } else {
            self.layer.parent.as_ref().and_then(|p| p.font_data(id))
        }
    }

    /// Incremented by every successful registration in this layer.
    pub fn generation(&self) -> u64 {
        self.layer.generation.load(Ordering::SeqCst)
    }

    /// (shared layer generation, document layer generation).
    pub(crate) fn generations(&self) -> (u64, Option<u64>) {
        match &self.layer.parent {
            Some(shared) => (shared.generation(), Some(self.generation())),
            None => (self.generation(), None),
        }
    }

    /// Placeholder for font matching: the first face of a document layer,
    /// otherwise the shared stub face.
    pub(crate) fn primary_font(&self) -> FontId {
        match &self.layer.parent {
            Some(shared) if self.state().faces.is_empty() => shared.primary_font(),
            _ => FontId {
                layer: self.layer.id,
                index: 0,
            },
        }
    }

    /// Weak lifecycle observer for this layer.
    pub fn layer_handle(&self) -> WeakFontLayer {
        WeakFontLayer {
            id: self.layer.id,
            layer: Arc::downgrade(&self.layer),
        }
    }

    /// Shared, entry-count-bounded shaping data for a registered face.
    /// Returned handles remain usable after LRU eviction. Zero disables
    /// retention without disabling shaping.
    pub fn shaper_data(&self, id: FontId) -> Option<Arc<harfrust::ShaperData>> {
        if id.layer != self.layer.id {
            return self.layer.parent.as_ref()?.shaper_data(id);
        }
        let mut state = self.state();
        if let Some(index) = state
            .shapers
            .iter()
            .position(|(index, _)| *index == id.index)
        {
            let entry = state.shapers.remove(index)?;
            let result = entry.1.clone();
            state.shapers.push_back(entry);
            return Some(result);
        }
        let data = state.faces.get(id.index as usize)?;
        let font = harfrust::FontRef::from_index(data.data.as_ref(), data.index).ok()?;
        let shaper = Arc::new(harfrust::ShaperData::new(&font));
        let cap = self
            .layer
            .limits
            .max_shaper_cache_entries
            .unwrap_or(u64::MAX);
        if cap > 0 {
            while state.shapers.len() as u64 >= cap {
                state.shapers.pop_front();
            }
            state.shapers.push_back((id.index, shaper.clone()));
        }
        Some(shaper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{LimitKind, Limits};

    fn font_bytes() -> Vec<u8> {
        sfnt::build_sfnt(&[])
    }

    #[test]
    fn shared_layer_has_a_stub_face() {
        let fonts = FontCollection::new(&Limits::default());
        let id = fonts.primary_font();
        assert_eq!(id.index(), 0);
        assert!(fonts.font_data(id).is_some());
        assert_eq!(fonts.generations(), (0, None));
    }

    #[test]
    fn document_of_document_normalizes_to_root_shared_layer() {
        let limits = Limits::default();
        let shared = FontCollection::new(&limits);
        let first = FontCollection::for_document(&shared, &limits);
        let private = first.register(font_bytes()).unwrap();
        let mut doc = first;
        for _ in 0..100 {
            doc = FontCollection::for_document(&doc, &limits);
        }
        assert_eq!(doc.layer.parent.as_ref().unwrap().layer.id, shared.layer.id);
        assert!(doc.font_data(private).is_none());
        assert_eq!(doc.generations(), (0, Some(0)));
    }

    #[test]
    fn register_bumps_generation_and_returns_first_face() {
        let fonts = FontCollection::new(&Limits::default());
        let id = fonts.register(font_bytes()).unwrap();
        assert_eq!(id.index(), 1);
        assert_eq!(fonts.generation(), 1);
        assert_eq!(fonts.font_data(id).unwrap().index, 0);
    }

    #[test]
    fn rejected_fonts_leave_the_layer_unchanged() {
        let fonts = FontCollection::new(&Limits::default());
        assert!(fonts.register(vec![1, 2, 3]).is_err());
        assert_eq!(fonts.generation(), 0);
    }

    #[test]
    fn layer_budgets_are_checked_before_storing() {
        let limits = Limits {
            max_faces_per_layer: Some(2), // the stub face counts
            ..Default::default()
        };
        let fonts = FontCollection::new(&limits);
        fonts.register(font_bytes()).unwrap();
        match fonts.register(font_bytes()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::FacesPerLayer),
            other => panic!("expected FacesPerLayer, got {other:?}"),
        }

        let limits = Limits {
            max_layer_blob_bytes: Some(15),
            ..Default::default()
        };
        let doc = FontCollection::for_document(&FontCollection::new(&Limits::default()), &limits);
        doc.register(font_bytes()).unwrap(); // 12 bytes
        match doc.register(font_bytes()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayerBlobBytes),
            other => panic!("expected LayerBlobBytes, got {other:?}"),
        }
    }

    #[test]
    fn document_layers_are_isolated() {
        let shared = FontCollection::new(&Limits::default());
        let a = FontCollection::for_document(&shared, &Limits::default());
        let b = FontCollection::for_document(&shared, &Limits::default());
        let id = a.register(font_bytes()).unwrap();
        assert!(a.font_data(id).is_some());
        assert!(b.font_data(id).is_none());
        assert!(shared.font_data(id).is_none());
        // Shared faces are visible through a document layer.
        assert!(a.font_data(shared.primary_font()).is_some());
        assert_eq!(a.primary_font(), id);
        assert_eq!(b.primary_font(), shared.primary_font());
        assert_eq!(a.generations(), (0, Some(1)));
    }

    #[test]
    fn stub_metrics_scale_with_size() {
        let fonts = FontCollection::new(&Limits::default());
        let m = fonts.metrics(fonts.primary_font(), 10.0);
        assert_eq!((m.ascent, m.descent, m.line_gap), (8.0, 2.0, 0.0));
    }

    #[test]
    fn collection_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FontCollection>();
    }
}

#[cfg(test)]
mod browser_tests;
