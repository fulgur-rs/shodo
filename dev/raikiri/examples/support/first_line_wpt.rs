//! Fixed original inputs, normal native rendering and real first-line candidate.
use crate::{caller, offline_wpt, source_fonts};
use raikiri_style::{StyleDom, StyleNodeId};
use raikiri_traits::PageBox;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const ENGINE: &str = "a62ea75b65a8547bcd0e7874addeeebee5b0beee";
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key].as_str().ok_or_else(|| format!("missing {key}"))
}
pub fn verify_assets(root: &Path, assets: &[Value]) -> Result<(), String> {
    for asset in assets {
        let path = string(asset, "path")?;
        let bytes = std::fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
        if hash(&bytes) != string(asset, "sha256")? {
            return Err(format!("original bytes changed: {path}"));
        }
    }
    Ok(())
}
fn font_paths(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut stack = vec![root.join("fonts")];
    let mut files = BTreeSet::new();
    while let Some(dir) = stack.pop() {
        for item in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("ttf" | "otf")
            ) {
                files.insert(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    Ok(files)
}
#[derive(Debug)]
pub enum CandidateError {
    Unsupported(String),
    Failed(String),
}
pub fn classify(native: Result<bool, String>, candidate: Result<bool, CandidateError>) -> Value {
    let native_match = native.as_ref().ok().copied();
    let candidate_match = candidate.as_ref().ok().copied();
    let (status, error) = match (&native, &candidate) {
        (Err(e), _) => ("failed", Some(e.clone())),
        (_, Err(CandidateError::Failed(e))) => ("failed", Some(e.clone())),
        (_, Err(CandidateError::Unsupported(e))) => ("unsupported", Some(e.clone())),
        (_, Ok(true)) => ("inline-match", None),
        (_, Ok(false)) => ("mismatch", None),
    };
    let classification = if status == "inline-match" {
        if native_match == Some(true) {
            "required"
        } else {
            "additional"
        }
    } else {
        "unclassified"
    };
    // This probe measures a common ordinary block boundary, not full-page WPT.
    json!({"status":status,"error":error,"native_reference_exact":native_match,
        "candidate_reference_exact":candidate_match,"capability":classification,
        "counted_as_pass":false})
}
fn diff(a: &[u8], b: &[u8]) -> Value {
    let mismatched = a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0.iter())
        .filter(|(x, y)| x != y)
        .count();
    let max = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    json!({"mismatched_pixels":mismatched,"max_channel_delta":max,"pixels":WIDTH*HEIGHT})
}
fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let pix = tiny_skia::Pixmap::from_vec(
        bytes.to_vec(),
        tiny_skia::IntSize::from_wh(WIDTH, HEIGHT).unwrap(),
    )
    .ok_or("invalid RGBA")?;
    pix.save_png(path).map_err(|e| e.to_string())
}
fn root_id(parsed: &raikiri_html::UncascadedDocument, tag: &str) -> Result<usize, String> {
    (0..parsed.dom.node_count())
        .find(|&id| parsed.dom.get_node(id).unwrap().tag_name() == Some(tag))
        .ok_or_else(|| format!("missing {tag}"))
}
fn boundary(doc: &raikiri_dom::Document, root: usize) -> Result<[f32; 3], String> {
    let node = doc.get_node(root).ok_or("missing native root")?;
    let layout = &node.unrounded_layout;
    let mut x = layout.padding.left + layout.border.left;
    let mut y = layout.padding.top + layout.border.top;
    let width = layout.size.width
        - layout.padding.left
        - layout.padding.right
        - layout.border.left
        - layout.border.right;
    let mut id = Some(StyleNodeId(root as u64));
    while let Some(current) = id {
        let node = doc
            .get_node(current.0 as usize)
            .ok_or("missing native ancestor")?;
        x += node.unrounded_layout.location.x;
        y += node.unrounded_layout.location.y;
        id = doc.parent_id(current);
    }
    if !x.is_finite() || !y.is_finite() || !width.is_finite() || width <= 0.0 {
        return Err("invalid native content boundary".into());
    }
    Ok([x, y, width])
}
fn native(root: &Path, id: &str, tag: &str) -> Result<(Vec<u8>, [f32; 3], Value), String> {
    let input = offline_wpt::parse_screen(root, id)?;
    let root_id = root_id(&input.parsed, tag)?;
    let mut dom = input.parsed.dom;
    let mut page = PageBox::new();
    page.width = WIDTH as f32;
    page.height = HEIGHT as f32;
    let fonts = raikiri_dom::build_wpt_font_ctx(&root.join("fonts")).map_err(|e| e.to_string())?;
    raikiri_dom::layout_single_page(&mut dom, &input.cascade, page, fonts)
        .map_err(|e| format!("native layout: {e:?}"))?;
    let common = boundary(&dom, root_id)?;
    let mut lines = Vec::new();
    for id in 0..dom.node_count() {
        let node = dom.get_node(id).unwrap();
        if let Some(layout) = node.text_layout() {
            for line in layout.lines() {
                let runs:Vec<_>=line.items().filter_map(|item|match item {
                    parley::layout::PositionedLayoutItem::GlyphRun(g)=>{
                        let run=g.run(); let font=run.font();
                        Some(json!({"font_sha256":hash(font.data.data()),"font_index":font.index,"font_size":run.font_size(),
                            "baseline":g.baseline(),"inline_offset":g.offset(),"advance":g.advance(),
                            "glyphs":g.positioned_glyphs().map(|p|json!({"id":p.id,"x":p.x,"y":p.y,"advance":p.advance})).collect::<Vec<_>>()}))
                    },_=>None}).collect();
                let cv = &input.cascade.computed[id];
                lines.push(json!({"text_node":id,"source":node.text_content(),"color":[cv.color.r,cv.color.g,cv.color.b,cv.color.a],
                    "line_metrics":format!("{:?}",line.metrics()),"runs":runs}));
            }
        }
    }
    let rgba = anyrender::render_to_buffer::<anyrender_vello_cpu::VelloCpuImageRenderer, _>(
        |painter| raikiri_paint::paint_single_page(painter, &dom, &input.cascade, page),
        WIDTH,
        HEIGHT,
    );
    Ok((
        rgba,
        common,
        json!({"resources":input.resources,"lines":lines}),
    ))
}
fn candidate(
    root: &Path,
    id: &str,
    tag: &str,
    common: [f32; 3],
    fonts: &source_fonts::OriginalFonts,
) -> Result<(Vec<u8>, Value), CandidateError> {
    let input = offline_wpt::parse_screen(root, id).map_err(CandidateError::Failed)?;
    let root_id = root_id(&input.parsed, tag).map_err(CandidateError::Failed)?;
    offline_wpt::verify_original_resources(root, &input.resources)
        .map_err(CandidateError::Failed)?;
    let resolved = caller::resolve_document(
        input.parsed,
        StyleNodeId(root_id as u64),
        &raikiri_style::MediaContext::screen(),
    )
    .map_err(CandidateError::Unsupported)?;
    let has_first_line = resolved.has_first_line();
    let output = caller::layout_with_font_policy(
        &resolved,
        &fonts.collection,
        common[2],
        caller::FontPolicy::BundledWpt,
    )
    .map_err(CandidateError::Unsupported)?;
    let (image, count) =
        shodo_harness::glyph_paint::try_paint_styled_on_canvas(&output.lines, WIDTH, HEIGHT)
            .map_err(|e| CandidateError::Failed(e.to_string()))?;
    let mut placed = tiny_skia::Pixmap::new(WIDTH, HEIGHT)
        .ok_or_else(|| CandidateError::Failed("canvas".into()))?;
    placed.fill(tiny_skia::Color::WHITE);
    // The shared painter's documented 10px margin is removed at composition.
    // Native ordinary block location is shared; glyph geometry stays shodo's.
    placed.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &Default::default(),
        tiny_skia::Transform::from_translate(common[0] - 10.0, common[1] - 10.0),
        None,
    );
    let lines:Vec<_>=output.lines.iter().map(|line|{
        let runs:Vec<_>=line.fragments().filter_map(|f|match f {shodo::Fragment::GlyphRun(r)=>{
            let data=r.font_data().unwrap();
            Some(json!({"node":r.node().map(|n|n.0),"range":r.text_range(),"font_sha256":hash(data.data.data()),"font_index":data.index,
                "font_size":r.font_size(),"color":r.paint_style().color,"baseline":r.baseline(),"inline_start":r.inline_start(),"inline_size":r.inline_size(),
                "glyphs":r.glyphs().map(|g|json!({"id":g.id,"cluster":g.cluster,"inline":g.inline_position,"block_offset":g.block_offset})).collect::<Vec<_>>()}))
        },_=>None}).collect();
        let mapping:Vec<_>=line.offset_mapping().unwrap().units().iter().map(|u|json!({"node":u.node.0,"dom":u.dom,"text":u.text,"kind":format!("{:?}",u.kind)})).collect();
        json!({"text":&line.text()[line.text_range()],"range":line.text_range(),"inline_size":line.inline_size(),"block_offset":line.block_offset(),"block_size":line.block_size(),"runs":runs,"mapping":mapping})
    }).collect();
    Ok((
        placed.data().to_vec(),
        json!({"has_first_line":has_first_line,"drawn_glyphs":count,"resources":input.resources,"lines":lines}),
    ))
}
pub fn run(root: &Path, out: &Path) -> Result<(), String> {
    let pins: Value = serde_json::from_str(include_str!("../../inputs/first-line-wpt-pins.json"))
        .map_err(|e| e.to_string())?;
    verify_assets(root, pins["assets"].as_array().ok_or("missing assets")?)?;
    let expected: BTreeSet<_> = pins["font_paths"]
        .as_array()
        .ok_or("missing fonts")?
        .iter()
        .map(|p| p.as_str().unwrap().to_owned())
        .collect();
    if font_paths(root)? != expected {
        return Err("original font inventory changed".into());
    }
    let actual = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| e.to_string())?;
    if String::from_utf8_lossy(&actual.stdout).trim() != string(&pins, "wpt_commit")? {
        return Err("WPT checkout commit changed".into());
    }
    let fonts = source_fonts::load(&root.join("fonts"), &Default::default())?;
    let allowed: BTreeSet<_> = pins["assets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| string(v, "path").unwrap().starts_with("fonts/"))
        .map(|v| string(v, "sha256").unwrap())
        .collect();
    if fonts.hashes.iter().any(|s| !allowed.contains(s.as_str())) {
        return Err("registered font bytes outside original pin".into());
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for (i, row) in pins["cases"]
        .as_array()
        .ok_or("missing cases")?
        .iter()
        .enumerate()
    {
        let id = string(row, "test")?;
        let reference = string(row, "reference")?;
        let tag = string(row, "root_tag")?;
        let mut evidence = json!({"test":id,"reference":reference,"root_tag":tag});
        let test = native(root, id, tag);
        let control = native(root, reference, tag);
        let status = match (test, control) {
            (
                Ok((native_bytes, common, native_record)),
                Ok((ref_bytes, ref_common, ref_record)),
            ) => {
                save(&out.join(format!("{i}-native.png")), &native_bytes)?;
                save(&out.join(format!("{i}-reference.png")), &ref_bytes)?;
                evidence["common_native_boundary"] = json!(common);
                evidence["reference_native_boundary"] = json!(ref_common);
                evidence["native"] = native_record;
                evidence["reference_native"] = ref_record;
                evidence["native_reference_diff"] = diff(&native_bytes, &ref_bytes);
                let native_equal = native_bytes == ref_bytes;
                match candidate(root, id, tag, common, &fonts) {
                    Ok((bytes, record)) => {
                        save(&out.join(format!("{i}-candidate.png")), &bytes)?;
                        // The reference also uses the same caller to isolate raster
                        // and line-metric differences from pseudo inheritance.
                        match candidate(root, reference, tag, common, &fonts) {
                            Ok((reference_bytes, reference_record)) => {
                                save(
                                    &out.join(format!("{i}-candidate-reference.png")),
                                    &reference_bytes,
                                )?;
                                evidence["candidate"] = record;
                                evidence["candidate_reference"] = reference_record;
                                evidence["candidate_reference_diff"] =
                                    diff(&bytes, &reference_bytes);
                                evidence["candidate_native_reference_diff"] =
                                    diff(&bytes, &ref_bytes);
                                classify(Ok(native_equal), Ok(bytes == reference_bytes))
                            }
                            Err(e) => classify(Ok(native_equal), Err(e)),
                        }
                    }
                    Err(e) => classify(Ok(native_equal), Err(e)),
                }
            }
            (Err(e), _) | (_, Err(e)) => classify(
                Err(e),
                Err(CandidateError::Failed("native boundary unavailable".into())),
            ),
        };
        evidence["result"] = status;
        rows.push(evidence);
    }
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let lock = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"))
        .map_err(|e| e.to_string())?;
    let checkout = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| e.to_string())?;
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        "examples/first_line_wpt.rs",
        "examples/support/first_line_wpt.rs",
        "examples/support/raikiri_contracts.rs",
        "examples/support/source_fonts.rs",
        "examples/support/offline_wpt.rs",
    ]
    .iter()
    .map(|path| {
        let bytes = std::fs::read(source_root.join(path)).map_err(|e| e.to_string())?;
        Ok(json!({"path":path,"sha256":hash(&bytes)}))
    })
    .collect::<Result<Vec<_>, String>>()?;
    let result = json!({"measurement":"common ordinary block boundary; inline glyph comparison, not full-page WPT conformance",
        "raikiri_commit":ENGINE,"shodo_checkout_head":String::from_utf8_lossy(&checkout.stdout).trim(),"measured_source_files":sources,"wpt_commit":pins["wpt_commit"],"viewport":[WIDTH,HEIGHT],"media":"screen","system_fonts":false,
        "exact_process_argv":std::env::args().collect::<Vec<_>>(),"rustc":String::from_utf8_lossy(&rustc.stdout).trim(),"cargo_lock_sha256":hash(&lock),
        "font_hashes":fonts.hashes,"font_generics":fonts.generics,"assets":pins["assets"],"rows":rows,"full_page_wpt_passes":0});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(&result).unwrap() + "\n",
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{}: {} measured rows; no full-page WPT passes claimed",
        out.display(),
        rows.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn changed_original_resource_is_rejected() {
        let root =
            std::env::temp_dir().join(format!("shodo-first-line-pins-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let names = ["test.html", "ref.html", "font.ttf"];
        for name in names {
            std::fs::write(root.join(name), b"original").unwrap();
        }
        let assets = names
            .iter()
            .map(|name| json!({"path":name,"sha256":hash(b"original")}))
            .collect::<Vec<_>>();
        assert!(verify_assets(&root, &assets).is_ok());
        for name in names {
            std::fs::write(root.join(name), b"changed").unwrap();
            assert!(verify_assets(&root, &assets).is_err(), "{name}");
            std::fs::write(root.join(name), b"original").unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unsupported_and_failed_rows_do_not_count_as_pass() {
        let unsupported = classify(
            Ok(false),
            Err(CandidateError::Unsupported("opacity".into())),
        );
        let failed = classify(Err("native failed".into()), Ok(true));
        assert_eq!(unsupported["status"], "unsupported");
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["capability"], "unclassified");
        assert_eq!(unsupported["counted_as_pass"], false);
        assert_eq!(failed["counted_as_pass"], false);
        assert_eq!(classify(Ok(false), Ok(false))["status"], "mismatch");
    }
    #[test]
    fn no_first_line_rule_keeps_control_output() {
        let html = "<style>#root{font-family:'Shodo Fixture Latin';font-size:16px;color:green}</style><div id=root><span>a</span></div>";
        let input = crate::caller::resolve_html(html, "root").unwrap();
        assert!(!input.has_first_line());
        let fonts = shodo_fixtures::load_fonts(&Default::default()).unwrap();
        let output = crate::caller::layout(&input, &fonts.collection, 400.0).unwrap();
        let run = output.lines[0]
            .fragments()
            .find_map(|f| match f {
                shodo::Fragment::GlyphRun(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(run.font_size(), 16.0);
        assert_eq!(run.paint_style().color, [0, 128, 0, 255]);
        assert_eq!(output.lines[0].text(), "a");
    }
}
