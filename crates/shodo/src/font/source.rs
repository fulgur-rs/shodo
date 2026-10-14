//! Ordered CSS font sources and exact local name resolution.

use super::{FontCollection, FontData, FontError, FontFaceDescriptor, FontId};
use crate::limits::{LimitKind, Limits};
use skrifa::{
    FontRef,
    raw::tables::name::{Name, NameRecord},
    string::{LocalizedString, StringId},
};

mod index;
pub(super) use index::LocalNames;

#[cfg(test)]
std::thread_local! {
    static LOCAL_NAME_FACE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LOCAL_CATALOG_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LOCAL_INDEX_BLOBS: std::cell::RefCell<Vec<peniko::WeakBlob<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests;

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
    /// first successful source changes registered faces or generation.
    /// Source count and total local-name bytes are checked before attempts.
    /// CSS descriptor payload is admitted and prepared once before attempts.
    /// Local names share a bounded root-owned index of the installed catalog.
    /// Recreate the shared collection to discover changed or newly available
    /// installed files. After lookup starts, root additions must fit the index
    /// budgets before publication, preserving previously indexed names.
    pub fn register_sources(
        &self,
        descriptor: FontFaceDescriptor,
        sources: Vec<FontSource>,
    ) -> Result<FontId, FontError> {
        Limits::check(
            self.layer.limits.max_font_sources,
            LimitKind::FontSources,
            sources.len() as u64,
        )?;
        let name_bytes = sources.iter().fold(0u64, |bytes, source| {
            bytes.saturating_add(match source {
                FontSource::Local(name) => name.len() as u64,
                FontSource::Data(..) => 0,
            })
        });
        Limits::check(
            self.layer.limits.max_font_source_name_bytes,
            LimitKind::FontSourceNameBytes,
            name_bytes,
        )?;
        let descriptor = self.prepare_descriptor(descriptor)?;
        let mut verification = LookupWork::default();
        let mut last_error = FontError::Malformed("no usable font source");
        for source in sources {
            let result = match source {
                FontSource::Local(name) => {
                    let normalized = name.to_lowercase();
                    let Some((data, source)) = self.local_font(&normalized, &mut verification)?
                    else {
                        continue;
                    };
                    self.register_source_blob(data.data, data.index, descriptor.clone(), source)
                }
                FontSource::Data(bytes, index) => {
                    super::decode_web_font(&bytes, &self.layer.limits).and_then(|bytes| {
                        self.register_source_blob(
                            super::Blob::from(bytes),
                            index,
                            descriptor.clone(),
                            None,
                        )
                    })
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

    fn local_font(
        &self,
        name: &str,
        verification: &mut LookupWork,
    ) -> Result<Option<(FontData, Option<fontique::SourceId>)>, FontError> {
        // Document CSS faces aren't installed faces; local() resolves only
        // against the shared application/platform layer.
        let root = self.root();
        let mut state = root.state();
        // The catalog lock serializes initial construction and incremental
        // registration. The state is restored on every Result outcome.
        let mut names = std::mem::take(&mut state.local_names);
        let result = names.lookup(root, &mut state, name, verification, &self.layer.limits);
        state.local_names = names;
        result
    }
}

#[derive(Clone, Default)]
struct LookupWork(u64);

impl LookupWork {
    fn charge(&mut self, amount: u64, limits: &Limits) -> Result<(), FontError> {
        let actual = self.0.saturating_add(amount);
        Limits::check(
            limits.max_local_font_lookup_work,
            LimitKind::LocalFontLookupWork,
            actual,
        )?;
        self.0 = actual;
        Ok(())
    }
}

fn has_local_name(
    data: &FontData,
    requested: &str,
    work: &mut LookupWork,
    limits: &Limits,
) -> Result<bool, FontError> {
    use skrifa::raw::TableProvider;
    work.charge(1, limits)?;
    #[cfg(test)]
    LOCAL_NAME_FACE_READS.with(|calls| calls.set(calls.get() + 1));
    let Ok(font) = FontRef::from_index(data.data.as_ref(), data.index) else {
        return Ok(false);
    };
    let Ok(table) = font.name() else {
        return Ok(false);
    };
    for record in local_name_records(&table, work, limits)? {
        // Bound the decode scratch as the index does; this re-reads a face
        // the root index already decoded under its own limit.
        let scratch = admit_name_record(record, work, limits)?;
        Limits::check(
            limits.max_local_font_index_bytes,
            LimitKind::LocalFontIndexBytes,
            scratch,
        )?;
        if decode_name(&table, record) == requested {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The full and PostScript name records of a face, in lookup order, after
/// charging `work` for scanning every record once per identifier.
fn local_name_records<'a>(
    table: &Name<'a>,
    work: &mut LookupWork,
    limits: &Limits,
) -> Result<impl Iterator<Item = &'a NameRecord> + use<'a>, FontError> {
    let records = table.name_record();
    work.charge(records.len() as u64 * 2, limits)?;
    Ok([StringId::FULL_NAME, StringId::POSTSCRIPT_NAME]
        .into_iter()
        .flat_map(move |id| records.iter().filter(move |record| record.name_id() == id)))
}

/// Charge a name record's encoded bytes to `work`, and return the scratch
/// bytes that decoding and lowercasing it may take, including UTF-8
/// expansion and allocation growth. Callers check that bound before
/// `decode_name` constructs either String.
fn admit_name_record(
    record: &NameRecord,
    work: &mut LookupWork,
    limits: &Limits,
) -> Result<u64, FontError> {
    let encoded = u64::from(record.length());
    work.charge(encoded, limits)?;
    Ok(encoded.saturating_mul(12))
}

/// A name record's text, lowercased for case-insensitive `local()` lookup.
fn decode_name(table: &Name<'_>, record: &NameRecord) -> String {
    LocalizedString::new(table, record)
        .to_string()
        .to_lowercase()
}
