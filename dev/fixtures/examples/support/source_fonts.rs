//! Mirror the pinned engine's original bundled registry through public APIs.
use fontique::{GenericFamily as NativeGeneric, SourceKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{
    font::{FontCollection, FontOptions},
    limits::Limits,
    style::GenericFamily,
};
use std::{collections::BTreeSet, path::Path};

pub struct OriginalFonts {
    pub collection: FontCollection,
    pub hashes: Vec<String>,
    pub generics: Vec<Value>,
}

pub fn load(directory: &Path, limits: &Limits) -> Result<OriginalFonts, String> {
    let mut native = raikiri_dom::build_wpt_font_ctx(directory).map_err(|e| e.to_string())?;
    let collection = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..Default::default()
        },
    );
    let mut seen = BTreeSet::new();
    let mut hashes = Vec::new();
    let mut generics = Vec::new();
    for (source, target) in [
        (NativeGeneric::Serif, GenericFamily::Serif),
        (NativeGeneric::SansSerif, GenericFamily::SansSerif),
        (NativeGeneric::Monospace, GenericFamily::Monospace),
        (NativeGeneric::Cursive, GenericFamily::Cursive),
        (NativeGeneric::Fantasy, GenericFamily::Fantasy),
        (NativeGeneric::SystemUi, GenericFamily::SystemUi),
    ] {
        let ids: Vec<_> = native.collection.generic_families(source).collect();
        let mut names = Vec::new();
        for id in ids {
            let family = native
                .collection
                .family(id)
                .ok_or("missing original bundled family")?;
            names.push(family.name().to_owned());
            for font in family.fonts() {
                let SourceKind::Memory(blob) = font.source().kind() else {
                    return Err("original WPT registry contains a non-memory font".into());
                };
                let hash = format!("{:x}", Sha256::digest(blob.data()));
                if seen.insert(hash.clone()) {
                    collection
                        .register(blob.data().to_vec())
                        .map_err(|e| format!("{e:?}"))?;
                    hashes.push(hash);
                }
            }
        }
        generics.push(json!({"generic":format!("{target:?}"),"families":names}));
        collection.set_generic_families(target, names);
    }
    if hashes.is_empty() {
        return Err("empty original bundled font registry".into());
    }
    Ok(OriginalFonts {
        collection,
        hashes,
        generics,
    })
}
