//! Fonts: registration, identity and metrics.

mod check;
pub(crate) mod sfnt;

pub use check::FontError;

use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use peniko::{Blob, FontData};

use crate::limits::{LimitKind, Limits};

static NEXT_LAYER_ID: AtomicU32 = AtomicU32::new(0);

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
    pub underline_offset: f32,
    pub underline_thickness: f32,
    pub strikeout_offset: f32,
    pub strikeout_thickness: f32,
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
    blob_bytes: u64,
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
        let stub = FontData::new(Blob::from(sfnt::build_sfnt(&[])), 0);
        Self::with_faces(limits, vec![stub], None)
    }

    /// Creates an empty document layer on top of the root shared layer.
    /// Passing a document collection does not expose its private faces.
    pub fn for_document(shared: &FontCollection, limits: &Limits) -> Self {
        let mut root = shared;
        while let Some(parent) = &root.layer.parent {
            root = parent;
        }
        Self::with_faces(limits, Vec::new(), Some(root.clone()))
    }

    fn with_faces(limits: &Limits, faces: Vec<FontData>, parent: Option<FontCollection>) -> Self {
        Self {
            layer: Arc::new(Layer {
                id: NEXT_LAYER_ID.fetch_add(1, Ordering::Relaxed),
                limits: limits.clone(),
                generation: AtomicU64::new(0),
                state: Mutex::new(LayerState {
                    faces,
                    blob_bytes: 0,
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
        for index in 0..faces {
            state.faces.push(FontData::new(blob.clone(), index));
        }
        self.layer.generation.fetch_add(1, Ordering::SeqCst);
        Ok(FontId {
            layer: self.layer.id,
            index: first,
        })
    }

    /// Font data of a face in this layer or its shared layer.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
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

    /// Metrics of a face at `size` px. Placeholder values per em: ascent 0.8,
    /// descent 0.2, no line gap.
    pub fn metrics(&self, _id: FontId, size: f32) -> FontMetrics {
        FontMetrics {
            ascent: 0.8 * size,
            descent: 0.2 * size,
            line_gap: 0.0,
            underline_offset: 0.1 * size,
            underline_thickness: 0.05 * size,
            strikeout_offset: -0.3 * size,
            strikeout_thickness: 0.05 * size,
        }
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
