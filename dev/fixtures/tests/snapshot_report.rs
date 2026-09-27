#[path = "../examples/support/snapshot_cases.rs"]
mod snapshot_cases;
#[path = "../examples/support/snapshot_report.rs"]
mod snapshot_report;
use serde_json::Value;
use snapshot_report::{Options, compare_images, run};
use std::path::{Path, PathBuf};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "shodo-snapshot-report-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn options(&self, output: &str, update: bool) -> Options {
        Options {
            expected: self.0.join("expected"),
            output: self.0.join(output),
            case: None,
            update,
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pixel(color: [u8; 4]) -> tiny_skia::Pixmap {
    let mut image = tiny_skia::Pixmap::new(1, 1).unwrap();
    image.data_mut().copy_from_slice(&color);
    image
}
fn contents(directory: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().into(),
                std::fs::read(path).unwrap(),
            )
        })
        .collect()
}
fn update(temp: &Temp) {
    assert!(run(&temp.options("update", true)).unwrap().passed());
}

#[test]
fn accepted_geometry_survives_json_round_trip_exactly() {
    fn first_difference(a: &Value, b: &Value, path: String) -> Option<String> {
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => a
                .iter()
                .find_map(|(key, value)| first_difference(value, &b[key], format!("{path}/{key}"))),
            (Value::Array(a), Value::Array(b)) => a
                .iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (a, b))| first_difference(a, b, format!("{path}/{i}"))),
            _ if a != b => Some(format!("{path}: {a} != {b}")),
            _ => None,
        }
    }
    let geometry = snapshot_cases::render("japanese-short").unwrap().geometry;
    let decoded: Value = serde_json::from_slice(&serde_json::to_vec(&geometry).unwrap()).unwrap();
    assert_eq!(first_difference(&geometry, &decoded, String::new()), None);
}

#[test]
fn decoded_pixels_use_zero_tolerance_and_literal_magenta_difference() {
    let white = pixel([255, 255, 255, 255]);
    let same = compare_images(&white, &white).unwrap();
    assert_eq!(same.changed_pixels, 0);
    assert!(same.dimensions_equal);
    assert_eq!(same.image.data(), &[255, 255, 255, 255]);
    let changed = compare_images(&white, &pixel([0, 0, 0, 255])).unwrap();
    assert_eq!(changed.changed_pixels, 1);
    assert!(changed.dimensions_equal);
    assert_eq!(changed.image.data(), &[255, 0, 255, 255]);
    let mut wider = tiny_skia::Pixmap::new(2, 1).unwrap();
    wider.fill(tiny_skia::Color::WHITE);
    let size = compare_images(&white, &wider).unwrap();
    assert!(!size.dimensions_equal);
    assert_eq!(size.changed_pixels, 1);
    assert_eq!(size.image.width(), 2);
}

#[test]
fn missing_expectations_fail_with_report_and_never_create_a_baseline() {
    let temp = Temp::new();
    let mut options = temp.options("missing", false);
    options.case = Some("latin-short".into());
    assert!(!run(&options).unwrap().passed());
    assert!(!options.expected.exists());
    assert!(options.output.join("index.html").is_file());
    assert!(options.output.join("latin-short/actual.png").is_file());
    assert!(options.output.join("latin-short/diff.png").is_file());
}

#[test]
fn explicit_update_creates_the_full_matrix_and_check_preserves_all_bytes() {
    let temp = Temp::new();
    update(&temp);
    let expected = temp.0.join("expected");
    let before = contents(&expected);
    assert_eq!(before.len(), 53);
    assert!(before.contains_key(Path::new("manifest.json")));
    let report = run(&temp.options("check", false)).unwrap();
    assert!(
        report.passed(),
        "{}",
        std::fs::read_to_string(temp.0.join("check/report.json")).unwrap()
    );
    assert_eq!(contents(&expected), before);
}

#[test]
fn changed_pixel_produces_triples_and_retains_the_damaged_expectation() {
    let temp = Temp::new();
    update(&temp);
    let path = temp.0.join("expected/latin-short.png");
    let mut image = tiny_skia::Pixmap::load_png(&path).unwrap();
    image.data_mut()[0] = 0;
    image.save_png(&path).unwrap();
    let before = contents(&temp.0.join("expected"));
    let mut options = temp.options("changed", false);
    options.case = Some("latin-short".into());
    assert!(!run(&options).unwrap().passed());
    assert!(
        contents(&options.expected) == before,
        "original expectations changed after a failed update"
    );
    for name in ["expected.png", "actual.png", "diff.png"] {
        assert!(options.output.join("latin-short").join(name).is_file());
    }
    let difference =
        tiny_skia::Pixmap::load_png(options.output.join("latin-short/diff.png")).unwrap();
    assert_eq!(&difference.data()[..4], &[255, 0, 255, 255]);
}

#[test]
fn equal_pixels_cannot_hide_changed_geometry_or_font_conditions() {
    let temp = Temp::new();
    update(&temp);
    let path = temp.0.join("expected/latin-short.geometry.json");
    let mut geometry: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    geometry["lines"][0]["inline_size"] = serde_json::json!(999.0);
    std::fs::write(&path, serde_json::to_vec(&geometry).unwrap()).unwrap();
    let mut options = temp.options("geometry", false);
    options.case = Some("latin-short".into());
    assert!(!run(&options).unwrap().passed());
    let manifest = temp.0.join("expected/manifest.json");
    let mut data: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    data["conditions"]["fonts"][0]["sha256"] = serde_json::json!("0".repeat(64));
    std::fs::write(manifest, serde_json::to_vec(&data).unwrap()).unwrap();
    options.output = temp.0.join("font");
    options.case = Some("arabic-short".into());
    assert!(!run(&options).unwrap().passed());
}

#[test]
fn missing_and_corrupt_png_fail_without_writing_expectations() {
    let temp = Temp::new();
    update(&temp);
    let path = temp.0.join("expected/latin-short.png");
    std::fs::remove_file(&path).unwrap();
    let mut options = temp.options("deleted", false);
    options.case = Some("latin-short".into());
    assert!(!run(&options).unwrap().passed());
    assert!(!path.exists());
    std::fs::write(&path, b"not a PNG").unwrap();
    let before = contents(&options.expected);
    options.output = temp.0.join("corrupt");
    assert!(!run(&options).unwrap().passed());
    assert_eq!(contents(&options.expected), before);
    assert!(options.output.join("index.html").is_file());
}

#[test]
fn late_render_error_prevents_partial_update_and_retains_diagnostics() {
    let temp = Temp::new();
    update(&temp);
    let options = temp.options("late-error", true);
    let before = contents(&options.expected);
    let result = snapshot_report::run_with_renderer(&options, |id| {
        if id == "float-pages" {
            Err("late actual renderer error".into())
        } else {
            snapshot_cases::render(id)
        }
    });
    assert!(!result.unwrap().passed());
    assert_eq!(contents(&options.expected), before);
    assert!(
        std::fs::read_to_string(options.output.join("index.html"))
            .unwrap()
            .contains("late actual renderer error")
    );
}

#[test]
fn failed_report_publication_restores_original_expectations_after_update() {
    let temp = Temp::new();
    update(&temp);
    let path = temp.0.join("expected/latin-short.png");
    let mut image = tiny_skia::Pixmap::load_png(&path).unwrap();
    image.data_mut()[0] = 0;
    image.save_png(&path).unwrap();
    let options = temp.options("occupied-during-render", true);
    let before = contents(&options.expected);
    let output = options.output.clone();
    let result = snapshot_report::run_with_renderer(&options, |id| {
        let rendered = snapshot_cases::render(id)?;
        if id == "float-pages" {
            std::fs::create_dir(&output).unwrap();
            std::fs::write(output.join("keep.txt"), b"concurrent caller output").unwrap();
        }
        Ok(rendered)
    });
    assert!(result.is_err());
    assert_eq!(contents(&options.expected), before);
    assert_eq!(
        std::fs::read(options.output.join("keep.txt")).unwrap(),
        b"concurrent caller output"
    );
}

#[test]
fn unknown_expected_files_and_existing_outputs_are_preserved() {
    let temp = Temp::new();
    update(&temp);
    std::fs::write(temp.0.join("expected/user-note.txt"), b"keep this").unwrap();
    let before = contents(&temp.0.join("expected"));
    assert!(run(&temp.options("unknown-file", true)).is_err());
    assert_eq!(contents(&temp.0.join("expected")), before);
    let output = temp.0.join("occupied");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("keep.txt"), b"keep this").unwrap();
    assert!(run(&temp.options("occupied", false)).is_err());
    assert_eq!(
        std::fs::read(output.join("keep.txt")).unwrap(),
        b"keep this"
    );
}

#[test]
fn unknown_case_partial_update_and_canonical_overlap_fail_before_mutation() {
    let temp = Temp::new();
    let mut options = temp.options("invalid", true);
    options.case = Some("latin-short".into());
    assert!(run(&options).is_err());
    options.update = false;
    options.case = Some("unknown".into());
    assert!(run(&options).is_err());
    assert!(!options.expected.exists());
    assert!(!options.output.exists());
    options.case = None;
    options.output = options.expected.join("report");
    assert!(run(&options).is_err());
    assert!(!options.expected.exists());
    #[cfg(unix)]
    {
        std::fs::create_dir(&options.expected).unwrap();
        std::fs::write(options.expected.join("keep.txt"), b"keep").unwrap();
        std::os::unix::fs::symlink(&options.expected, temp.0.join("alias")).unwrap();
        options.output = temp.0.join("alias/report");
        assert!(run(&options).is_err());
        assert_eq!(
            std::fs::read(options.expected.join("keep.txt")).unwrap(),
            b"keep"
        );
        assert!(!options.output.exists());
    }
}

#[test]
fn parser_requires_explicit_output_and_rejects_unknown_flags() {
    let args = |values: &[&str]| {
        values
            .iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>()
    };
    assert!(snapshot_report::parse_args(args(&[])).is_err());
    assert!(snapshot_report::parse_args(args(&["--output", "new", "--surprise"])).is_err());
    let parsed = snapshot_report::parse_args(args(&[
        "--output",
        "new",
        "--expected",
        "goldens",
        "--case",
        "latin-short",
    ]))
    .unwrap();
    assert_eq!(parsed.output, PathBuf::from("new"));
    assert_eq!(parsed.expected, PathBuf::from("goldens"));
    assert_eq!(parsed.case.as_deref(), Some("latin-short"));
    assert!(!parsed.update);
}
