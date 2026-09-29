//! Pixel comparison and explicit expectation publication for fixed snapshots.
use super::snapshot_cases::{self, Rendered};
use serde::Serialize;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use tiny_skia::Pixmap;

pub struct Difference {
    pub changed_pixels: u64,
    pub dimensions_equal: bool,
    pub image: Pixmap,
}
pub fn compare_images(expected: &Pixmap, actual: &Pixmap) -> Result<Difference, String> {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let mut image = Pixmap::new(width, height).ok_or("cannot allocate difference image")?;
    image.fill(tiny_skia::Color::WHITE);
    fn pixel(p: &Pixmap, x: u32, y: u32) -> Option<&[u8]> {
        if x >= p.width() || y >= p.height() {
            None
        } else {
            let n = ((y * p.width() + x) * 4) as usize;
            Some(&p.data()[n..n + 4])
        }
    }
    let mut changed_pixels = 0;
    for y in 0..height {
        for x in 0..width {
            if pixel(expected, x, y) != pixel(actual, x, y) {
                let n = ((y * width + x) * 4) as usize;
                image.data_mut()[n..n + 4].copy_from_slice(&[255, 0, 255, 255]);
                changed_pixels += 1;
            }
        }
    }
    Ok(Difference {
        changed_pixels,
        dimensions_equal: expected.width() == actual.width()
            && expected.height() == actual.height(),
        image,
    })
}
pub struct Options {
    pub expected: PathBuf,
    pub output: PathBuf,
    pub case: Option<String>,
    pub update: bool,
}
pub fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut output = None;
    let mut expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("snapshots");
    let mut case = None;
    let mut update = false;
    while let Some(arg) = args.next() {
        match arg.to_str().ok_or("option name is not UTF-8")? {
            "--output" => output = Some(PathBuf::from(args.next().ok_or("--output needs a path")?)),
            "--expected" => expected = PathBuf::from(args.next().ok_or("--expected needs a path")?),
            "--case" => {
                case = Some(
                    args.next()
                        .ok_or("--case needs an ID")?
                        .into_string()
                        .map_err(|_| "case ID is not UTF-8")?,
                )
            }
            "--update" => update = true,
            flag => return Err(format!("unknown option: {flag}")),
        }
    }
    let options = Options {
        expected,
        output: output.ok_or("--output <new-directory> is required")?,
        case,
        update,
    };
    validate_selection(&options)?;
    Ok(options)
}
fn validate_selection(options: &Options) -> Result<Vec<String>, String> {
    let ids = snapshot_cases::case_ids();
    if options.update && options.case.is_some() {
        return Err("--update requires the complete matrix; omit --case".into());
    }
    match &options.case {
        Some(id) if ids.contains(id) => Ok(vec![id.clone()]),
        Some(id) => Err(format!("unknown snapshot case: {id}")),
        None => Ok(ids),
    }
}
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    let mut result = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => {
                result.push(other.as_os_str());
                if fs::symlink_metadata(&result).is_ok() {
                    result = fs::canonicalize(&result)
                        .map_err(|e| format!("resolve {}: {e}", result.display()))?;
                }
            }
        }
    }
    Ok(result)
}
fn allowed_expected_files(expected: &Path) -> Result<(), String> {
    if !expected.exists() {
        return Ok(());
    }
    if !expected.is_dir() {
        return Err("expected path is not a directory".into());
    }
    let mut allowed = std::collections::BTreeSet::from([OsString::from("manifest.json")]);
    for id in snapshot_cases::case_ids() {
        allowed.insert(format!("{id}.png").into());
        allowed.insert(format!("{id}.geometry.json").into());
    }
    for entry in fs::read_dir(expected).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !allowed.contains(&entry.file_name())
            || !entry.file_type().map_err(|e| e.to_string())?.is_file()
        {
            return Err(format!(
                "refusing to replace unknown expected file: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}
fn fresh_path(parent: &Path, label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    parent.join(format!(".{label}-{}-{stamp}", std::process::id()))
}
struct Stage {
    path: PathBuf,
}
impl Stage {
    fn new(parent: &Path, label: &str) -> Result<Self, String> {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let path = fresh_path(parent, label);
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self { path })
    }
    fn publish(&mut self, target: &Path) -> Result<(), String> {
        if fs::symlink_metadata(target).is_ok() {
            return Err(format!("output already exists: {}", target.display()));
        }
        fs::rename(&self.path, target).map_err(|e| format!("publish {}: {e}", target.display()))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}
fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Entry {
    id: String,
    status: String,
    changed_pixels: Option<u64>,
    reason: Option<String>,
}
#[derive(Serialize)]
pub struct Report {
    schema: u32,
    updated: bool,
    partial: bool,
    passed: bool,
    conditions: Value,
    cases: Vec<Entry>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.passed
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn html(report: &Report) -> String {
    let mut text = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>Shodo snapshots</title><style>body{font:16px sans-serif;margin:24px}table{border-collapse:collapse}th,td{padding:8px;border:1px solid #ddd;vertical-align:top}img{width:512px;max-width:100%;height:auto}section{margin-bottom:32px}.fail{color:#a00}.pass{color:#060}pre{white-space:pre-wrap}</style><h1>Shodo snapshots</h1>",
    );
    text.push_str(&format!(
        "<p>{}; {}; {} cases.</p>",
        if report.passed { "Passed" } else { "Failed" },
        if report.updated {
            "explicit full update"
        } else if report.partial {
            "partial check"
        } else {
            "full check"
        },
        report.cases.len()
    ));
    for row in &report.cases {
        let id = escape(&row.id);
        text.push_str(&format!(
            "<section><h2>{id}</h2><p>{}: {} changed pixels</p>",
            escape(&row.status),
            row.changed_pixels
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".into())
        ));
        if let Some(reason) = &row.reason {
            text.push_str(&format!("<pre>{}</pre>", escape(reason)));
        }
        text.push_str("<table><tr><th>Expected</th><th>Actual</th><th>Difference</th></tr><tr>");
        for asset in ["expected", "actual", "diff"] {
            text.push_str(&format!("<td><a href=\"{id}/{asset}.png\"><img loading=\"lazy\" src=\"{id}/{asset}.png\" alt=\"{asset} {id}\"></a></td>"));
        }
        text.push_str(&format!("</tr></table><p><a href=\"{id}/expected.geometry.json\">Expected geometry</a> · <a href=\"{id}/actual.geometry.json\">Actual geometry</a></p></section>"));
    }
    text
}
fn compare_case(
    rendered: &Rendered,
    expected: &Path,
    manifest: &Value,
    directory: &Path,
    update: bool,
) -> Result<Entry, String> {
    fs::create_dir(directory).map_err(|e| e.to_string())?;
    rendered
        .image
        .save_png(directory.join("actual.png"))
        .map_err(|e| e.to_string())?;
    write_json(&directory.join("actual.geometry.json"), &rendered.geometry)?;
    let mut reasons = Vec::new();
    if manifest["schema"] != 1 || manifest["conditions"] != snapshot_cases::conditions() {
        reasons
            .push("expected font/canvas/renderer conditions differ or manifest is missing".into());
    }
    if manifest["cases"][&rendered.id] != rendered.settings {
        reasons.push("expected case settings differ or are missing".into());
    }
    let geometry_path = expected.join(format!("{}.geometry.json", rendered.id));
    match read_json(&geometry_path) {
        Ok(geometry) => {
            write_json(&directory.join("expected.geometry.json"), &geometry)?;
            if geometry != rendered.geometry {
                reasons.push("expected geometry differs".into());
            }
        }
        Err(error) => reasons.push(format!("missing/unreadable expected geometry: {error}")),
    }
    let expected_image = Pixmap::load_png(expected.join(format!("{}.png", rendered.id)));
    let difference = match expected_image {
        Ok(image) => {
            image
                .save_png(directory.join("expected.png"))
                .map_err(|e| e.to_string())?;
            let difference = compare_images(&image, &rendered.image)?;
            if !difference.dimensions_equal {
                reasons.push("expected dimensions differ".into());
            }
            if difference.changed_pixels > 0 {
                reasons.push("expected pixels differ".into());
            }
            difference
        }
        Err(error) => {
            reasons.push(format!("missing/unreadable expected image: {error}"));
            if update {
                rendered
                    .image
                    .save_png(directory.join("expected.png"))
                    .map_err(|e| e.to_string())?;
                write_json(
                    &directory.join("expected.geometry.json"),
                    &rendered.geometry,
                )?;
                compare_images(&rendered.image, &rendered.image)?
            } else {
                let mut image = rendered.image.clone();
                image.fill(tiny_skia::Color::from_rgba8(255, 0, 255, 255));
                Difference {
                    changed_pixels: u64::from(image.width()) * u64::from(image.height()),
                    dimensions_equal: false,
                    image,
                }
            }
        }
    };
    difference
        .image
        .save_png(directory.join("diff.png"))
        .map_err(|e| e.to_string())?;
    Ok(Entry {
        id: rendered.id.clone(),
        status: if reasons.is_empty() {
            "match"
        } else {
            "failed"
        }
        .into(),
        changed_pixels: Some(difference.changed_pixels),
        reason: if reasons.is_empty() {
            None
        } else {
            Some(reasons.join("; "))
        },
    })
}
fn replace_expected(stage: &mut Stage, expected: &Path) -> Result<Option<PathBuf>, String> {
    let backup = if expected.exists() {
        let path = fresh_path(
            expected.parent().ok_or("expected path has no parent")?,
            "snapshot-backup",
        );
        if path.exists() {
            return Err("backup path collision".into());
        }
        fs::rename(expected, &path).map_err(|e| e.to_string())?;
        Some(path)
    } else {
        None
    };
    if let Err(error) = fs::rename(&stage.path, expected) {
        if let Some(path) = &backup
            && let Err(restore) = fs::rename(path, expected)
        {
            return Err(format!(
                "update failed: {error}; restore failed: {restore}; recovery backup: {}",
                path.display()
            ));
        }
        return Err(format!("update failed: {error}"));
    }
    Ok(backup)
}
pub fn run(options: &Options) -> Result<Report, String> {
    run_with_renderer(options, snapshot_cases::render)
}
pub fn run_with_renderer(
    options: &Options,
    mut render: impl FnMut(&str) -> Result<Rendered, String>,
) -> Result<Report, String> {
    let ids = validate_selection(options)?;
    let expected = resolve(&options.expected)?;
    let output = resolve(&options.output)?;
    if expected.starts_with(&output) || output.starts_with(&expected) {
        return Err("expected and output paths overlap".into());
    }
    if fs::symlink_metadata(&output).is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    if options.update {
        allowed_expected_files(&expected)?;
    }
    let mut stage = Stage::new(
        output.parent().ok_or("output has no parent")?,
        "snapshot-report",
    )?;
    let manifest = read_json(&expected.join("manifest.json")).unwrap_or(Value::Null);
    let mut rows = Vec::new();
    let mut images = Vec::new();
    let mut render_failed = false;
    for id in &ids {
        match render(id) {
            Ok(rendered)
                if rendered.id == *id
                    && rendered.image.width() == 512
                    && rendered.image.height() == 1024
                    && rendered.glyph_count > 0 =>
            {
                rows.push(compare_case(
                    &rendered,
                    &expected,
                    &manifest,
                    &stage.path.join(id),
                    options.update,
                )?);
                images.push(rendered);
            }
            Ok(_) => {
                render_failed = true;
                rows.push(Entry {
                    id: id.clone(),
                    status: "render_error".into(),
                    changed_pixels: None,
                    reason: Some(
                        "renderer returned invalid case identity/canvas/glyph count".into(),
                    ),
                });
            }
            Err(error) => {
                render_failed = true;
                rows.push(Entry {
                    id: id.clone(),
                    status: "render_error".into(),
                    changed_pixels: None,
                    reason: Some(error),
                });
            }
        }
    }
    let updated = options.update && !render_failed;
    let mut report = Report {
        schema: 1,
        updated,
        partial: options.case.is_some(),
        passed: if options.update {
            updated
        } else {
            !render_failed && rows.iter().all(|row| row.status == "match")
        },
        conditions: snapshot_cases::conditions(),
        cases: rows,
    };
    if updated {
        for row in &mut report.cases {
            row.status = "updated".into();
        }
    }
    write_json(&stage.path.join("report.json"), &report)?;
    fs::write(stage.path.join("index.html"), html(&report)).map_err(|e| e.to_string())?;
    let mut expectations = None;
    let mut backup = None;
    if updated {
        let mut fresh = Stage::new(
            expected.parent().ok_or("expected has no parent")?,
            "snapshot-expectations",
        )?;
        let mut cases = serde_json::Map::new();
        for rendered in &images {
            rendered
                .image
                .save_png(fresh.path.join(format!("{}.png", rendered.id)))
                .map_err(|e| e.to_string())?;
            write_json(
                &fresh.path.join(format!("{}.geometry.json", rendered.id)),
                &rendered.geometry,
            )?;
            cases.insert(rendered.id.clone(), rendered.settings.clone());
        }
        write_json(
            &fresh.path.join("manifest.json"),
            &json!({"schema":1,"conditions":snapshot_cases::conditions(),"cases":cases,"provenance":"Explicit agent-generated fixed-output expectations; review/commit through PR, no claim of separate human approval or browser conformance."}),
        )?;
        backup = replace_expected(&mut fresh, &expected)?;
        expectations = Some(fresh);
    }
    if let Err(error) = stage.publish(&output) {
        if let Some(fresh) = &expectations {
            fs::rename(&expected,&fresh.path).map_err(|restore|format!("{error}; cannot withdraw unpublished update: {restore}; recovery backup: {backup:?}"))?;
            if let Some(original) = &backup {
                fs::rename(original,&expected).map_err(|restore|format!("{error}; cannot restore original expectations: {restore}; recovery backup: {}",original.display()))?;
            }
        }
        return Err(error);
    }
    drop(expectations);
    if let Some(path) = backup {
        fs::remove_dir_all(&path).map_err(|e| {
            format!(
                "update/report published; old backup retained at {}: {e}",
                path.display()
            )
        })?;
    }
    Ok(report)
}
