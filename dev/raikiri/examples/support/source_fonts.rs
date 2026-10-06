//! Reproduce the pinned native WPT font registration policy for this caller.
//! This is a policy mirror, not enumeration of the opaque native collection.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shodo::{
    font::{FontCollection, FontOptions},
    limits::Limits,
    style::GenericFamily,
};
use skrifa::{FontRef, MetadataProvider, string::StringId};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct OriginalFonts {
    pub collection: FontCollection,
    pub hashes: Vec<String>,
    pub generics: Vec<Value>,
}

// raikiri d7dabe7: fonts::walk_fonts / layout::ifc::font::wpt_collection.
const FONT_SIZE_CAP: u64 = 100 * 1024 * 1024;

fn walk(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|e| e.to_string())?
        .map(|entry| {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            Ok((entry.path(), kind))
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, kind) in entries {
        if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
            eprintln!(
                "source font policy: skipping non-regular entry {}",
                path.display()
            );
        } else if kind.is_dir() {
            walk(&path, paths)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("ttf") || s.eq_ignore_ascii_case("otf"))
        {
            if std::fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .len()
                <= FONT_SIZE_CAP
            {
                paths.push(path);
            } else {
                eprintln!(
                    "source font policy: skipping oversized font {}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

pub fn load(directory: &Path, limits: &Limits) -> Result<OriginalFonts, String> {
    let root = directory.canonicalize().map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    walk(directory, &mut paths)?;
    paths.sort();
    // Stable partition preserves all preferred duplicates in sorted path order.
    let (mut preferred, other): (Vec<_>, Vec<_>) = paths
        .into_iter()
        .partition(|path| path.file_name().and_then(|s| s.to_str()) == Some("Ahem.ttf"));
    preferred.extend(other);
    if preferred.is_empty() {
        return Err("empty original WPT font directory".into());
    }
    let collection = FontCollection::with_options(
        limits,
        FontOptions {
            system_fonts: false,
            ..FontOptions::default()
        },
    );
    let mut seen = BTreeSet::new();
    let mut hashes = Vec::new();
    let mut families = Vec::new();
    let mut has_ahem = false;
    for path in preferred {
        let bytes =
            match raikiri_traits::io::read_bounded_contained_file(&path, &root, FONT_SIZE_CAP) {
                Ok(bytes) => bytes,
                Err(raikiri_traits::io::RejectReason::Io(error)) => return Err(error.to_string()),
                Err(reason) => {
                    eprintln!(
                        "source font policy: skipping {}: {reason:?}",
                        path.display()
                    );
                    continue;
                }
            };
        let family = FontRef::from_index(&bytes, 0).ok().and_then(|font| {
            [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
                .into_iter()
                .find_map(|id| {
                    font.localized_strings(id)
                        .english_or_first()
                        .map(|name| name.to_string())
                })
        });
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if collection.register(bytes).is_err() {
            eprintln!(
                "source font policy: skipping unregistrable font {}",
                path.display()
            );
            continue;
        }
        has_ahem |= path.file_name().and_then(|s| s.to_str()) == Some("Ahem.ttf");
        if seen.insert(hash.clone()) {
            hashes.push(hash);
        }
        if let Some(family) = family
            && !families.contains(&family)
        {
            families.push(family);
        }
    }
    if !has_ahem {
        return Err("required Ahem.ttf was not registered".into());
    }
    if families.is_empty() {
        return Err("no original WPT font families registered".into());
    }
    let mut generics = Vec::new();
    for generic in [
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
    ] {
        collection.set_generic_families(generic, families.clone());
        generics.push(json!({"generic":format!("{generic:?}"),"families":families}));
    }
    Ok(OriginalFonts {
        collection,
        hashes,
        generics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("shodo-font-policy-{}-{index}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pinned_font_policy_preserves_preferred_bytes_and_all_generic_orders() {
        let dir = Dir::new();
        let latin = &shodo_fixtures::FONTS[0];
        let cjk = &shodo_fixtures::FONTS[1];
        std::fs::write(dir.0.join("AAA.otf"), cjk.bytes).unwrap();
        std::fs::write(dir.0.join("Ahem.ttf"), latin.bytes).unwrap();
        std::fs::create_dir(dir.0.join("nested")).unwrap();
        std::fs::write(dir.0.join("nested/Ahem.ttf"), latin.bytes).unwrap();
        std::fs::write(dir.0.join("broken.ttf"), b"invalid").unwrap();
        let fonts = load(&dir.0, &Limits::default()).unwrap();
        assert!(fonts.collection.is_bundled_only());
        assert_eq!(
            fonts.hashes,
            vec![
                format!("{:x}", Sha256::digest(latin.bytes)),
                format!("{:x}", Sha256::digest(cjk.bytes))
            ]
        );
        assert_eq!(fonts.generics.len(), 6);
        for generic in fonts.generics {
            assert_eq!(generic["families"], json!([latin.family, cjk.family]));
        }
    }

    #[test]
    fn pinned_font_policy_rejects_missing_or_corrupt_preferred_font() {
        let dir = Dir::new();
        std::fs::write(dir.0.join("regular.ttf"), shodo_fixtures::FONTS[0].bytes).unwrap();
        assert!(
            load(&dir.0, &Limits::default())
                .err()
                .unwrap()
                .contains("Ahem.ttf")
        );
        std::fs::write(dir.0.join("Ahem.ttf"), b"invalid").unwrap();
        assert!(
            load(&dir.0, &Limits::default())
                .err()
                .unwrap()
                .contains("Ahem.ttf")
        );
    }

    #[test]
    fn mirrored_generic_selection_uses_the_same_bytes_as_native_ifc() {
        let dir = Dir::new();
        std::fs::write(dir.0.join("AAA.otf"), shodo_fixtures::FONTS[1].bytes).unwrap();
        std::fs::write(dir.0.join("Ahem.ttf"), shodo_fixtures::FONTS[0].bytes).unwrap();
        let mirrored = load(&dir.0, &Limits::default()).unwrap();
        let native = raikiri_dom::build_wpt_font_collection(&dir.0).unwrap();
        let parsed = raikiri_html::parse(
            "<!doctype html><style>div{font-family:serif;font-size:16px}</style><div>x日</div>"
                .as_bytes(),
            &raikiri_html::ParseOptions {
                extra_stylesheets: &[],
                network: None,
                base_url: None,
            },
        )
        .unwrap();
        let cascade = raikiri_html::build_cascaded_with_media_context(
            &parsed,
            &raikiri_style::MediaContext::screen(),
        );
        let mut dom = parsed.dom;
        dom.set_font_collection(native);
        raikiri_dom::layout_single_page(&mut dom, &cascade, raikiri_traits::PageBox::new())
            .unwrap();
        let mut native_hashes = Vec::new();
        for node in 0..dom.node_count() {
            if let Some(lines) = raikiri_dom::PositionedLines::new(&dom, &cascade, node, None) {
                for line in lines.lines() {
                    for run in line.runs {
                        native_hashes.push(format!(
                            "{:x}",
                            Sha256::digest(run.run.font_data().unwrap().data.data())
                        ));
                    }
                }
            }
        }
        let paragraph_style = shodo::style::ParagraphStyle {
            root: shodo::style::InlineStyle {
                font_families: vec![shodo::style::FontFamily::Generic(GenericFamily::Serif)],
                ..Default::default()
            },
            ..Default::default()
        };
        let mut builder = shodo::ParagraphBuilder::new(&paragraph_style, &Limits::default());
        builder.push_text(
            shodo::node::TextSource::Generated {
                node: shodo::node::NodeId(1),
            },
            "x日",
        );
        let paragraph = builder
            .build(&mut shodo::LayoutContext::new(), &mirrored.collection)
            .unwrap();
        let lines = paragraph.break_all(
            &mut shodo::LayoutContext::new(),
            &Default::default(),
            400.0,
            &shodo::AtomicSizes::EMPTY,
        );
        let mirrored_hashes: Vec<_> = lines
            .iter()
            .flat_map(|line| line.fragments())
            .filter_map(|fragment| {
                if let shodo::Fragment::GlyphRun(run) = fragment {
                    Some(format!(
                        "{:x}",
                        Sha256::digest(run.font_data().unwrap().data.data())
                    ))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            native_hashes.len(),
            2,
            "native text must use Latin and CJK faces"
        );
        assert_eq!(mirrored_hashes, native_hashes);
    }

    #[test]
    fn pinned_font_policy_skips_oversized_files_and_unrelated_assets() {
        let dir = Dir::new();
        let latin = &shodo_fixtures::FONTS[0];
        std::fs::write(dir.0.join("Ahem.ttf"), latin.bytes).unwrap();
        std::fs::write(dir.0.join("ignored.txt"), b"not a font").unwrap();
        std::fs::File::create(dir.0.join("oversized.ttf"))
            .unwrap()
            .set_len(FONT_SIZE_CAP + 1)
            .unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&dir.0, dir.0.join("cycle")).unwrap();
            std::os::unix::fs::symlink(dir.0.join("Ahem.ttf"), dir.0.join("linked.ttf")).unwrap();
        }
        let fonts = load(&dir.0, &Limits::default()).unwrap();
        assert_eq!(
            fonts.hashes,
            vec![format!("{:x}", Sha256::digest(latin.bytes))]
        );
    }
}
