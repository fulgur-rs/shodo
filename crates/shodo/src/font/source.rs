//! Ordered CSS font sources and exact local name resolution.

use super::{FontCollection, FontData, FontError, FontFaceDescriptor, FontId};
use skrifa::{FontRef, MetadataProvider, string::StringId};

/// Sources in CSS `src` order. URL fetching belongs to the caller; Data
/// supplies downloaded sfnt/TTC or WOFF/WOFF2 bytes and a selected face.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// Installed/bundled face's full or PostScript name, not a family name.
    Local(String),
    Data(Vec<u8>, u32),
}

impl FontCollection {
    /// Tries sources in order. Unavailable local names and malformed fonts
    /// fall through; a resource limit immediately fails closed. Only the
    /// first successful source changes the layer's state or generation.
    pub fn register_sources(
        &self,
        descriptor: FontFaceDescriptor,
        sources: Vec<FontSource>,
    ) -> Result<FontId, FontError> {
        descriptor.validate()?;
        let mut last_error = FontError::Malformed("no usable font source");
        for source in sources {
            let result = match source {
                FontSource::Local(name) => {
                    let Some((data, source)) = self.local_font(&name) else {
                        continue;
                    };
                    self.register_source_blob(data.data, data.index, descriptor.clone(), source)
                }
                FontSource::Data(bytes, index) => {
                    super::decode_web_font(&bytes, &self.layer.limits)
                        .and_then(|bytes| self.register_face(bytes, index, descriptor.clone()))
                }
            };
            match result {
                Ok(id) => return Ok(id),
                Err(err @ FontError::Limit(_)) => return Err(err),
                Err(err) => last_error = err,
            }
        }
        Err(last_error)
    }

    fn local_font(&self, name: &str) -> Option<(FontData, Option<fontique::SourceId>)> {
        // Document CSS faces aren't installed faces; local() resolves only
        // against the shared application/platform layer.
        let root = self.root();
        let mut state = root.state();
        if let Some(data) = state
            .faces
            .iter()
            .zip(&state.descriptors)
            .find_map(|(data, desc)| {
                (desc.is_none() && has_local_name(data, name)).then(|| data.clone())
            })
        {
            return Some((data, None));
        }
        root.ensure_system(&mut state);
        let names: Vec<_> = state.native.family_names().map(str::to_owned).collect();
        let mut result = None;
        'families: for family_name in names {
            let Some(family) = state.native.family_by_name(&family_name) else {
                continue;
            };
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
                if has_local_name(&data, name) {
                    result = Some((data, Some(info.source().id())));
                    break 'families;
                }
            }
        }
        let age = state.options.source_cache_max_age;
        state.source_cache.prune(age, true);
        result
    }
}

fn has_local_name(data: &FontData, requested: &str) -> bool {
    let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
        return false;
    };
    let requested = requested.to_lowercase();
    [StringId::FULL_NAME, StringId::POSTSCRIPT_NAME]
        .into_iter()
        .any(|id| {
            font.localized_strings(id)
                .any(|name| name.to_string().to_lowercase() == requested)
        })
}
