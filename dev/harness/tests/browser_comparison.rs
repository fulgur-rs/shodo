use sha2::{Digest, Sha256};
use shodo::LayoutContext;
use shodo_fixtures::{browser, load_fonts};

#[test]
fn saved_browser_metadata_and_source_endpoints_are_valid() {
    let data = browser::capture().unwrap();
    data.validate(browser::cases()).unwrap();
    let hashes = [
        (
            "inputs_sha256",
            include_bytes!("../../fixtures/assets/browser-inputs.json").as_slice(),
        ),
        (
            "corpus_sha256",
            include_bytes!("../../fixtures/assets/cases.json").as_slice(),
        ),
        (
            "recorder_sha256",
            include_bytes!("../../fixtures/tools/browser_recorder.js").as_slice(),
        ),
    ];
    for (key, bytes) in hashes {
        assert_eq!(data.metadata[key], format!("{:x}", Sha256::digest(bytes)));
    }
    assert!(
        !data.metadata["browser_version"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn raw_width_comparison_detects_a_wrong_browser_endpoint_with_a_reproducer() {
    let mut data = browser::capture().unwrap();
    let c = browser::cases()
        .iter()
        .find(|c| c.id == "color-ffi")
        .unwrap();
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let r = data.records.iter_mut().find(|r| r.id == c.id).unwrap();
    r.samples
        .retain(|s| s.width_subpixels == r.initial.width_subpixels);
    r.samples[0].end_utf8 = 0;
    let result = browser::compare(&data, std::slice::from_ref(c), &mut cx, &fonts).unwrap();
    assert_eq!(result.mismatches.len(), 1);
    let m = &result.mismatches[0];
    assert_eq!(m.id, "color-ffi");
    assert_eq!(m.seed, 1002);
    assert_eq!(m.expected, 0);
    assert!(m.actual > 0);
    let report = m.to_string();
    for fragment in [
        "color-ffi",
        "1002",
        "140",
        "ffi office",
        "expected",
        "actual",
        "font=",
        "size=16",
    ] {
        assert!(report.contains(fragment), "{fragment}: {report}");
    }
}

#[test]
fn transition_analysis_measures_raw_thresholds_without_adjusting_widths() {
    let data = browser::capture().unwrap();
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let transitions = browser::transitions(&data, browser::cases(), &mut cx, &fonts).unwrap();
    let t = transitions
        .iter()
        .find(|t| t.id == "japanese-punctuation")
        .unwrap();
    assert_eq!(t.browser_subpixels, 7168);
    assert_eq!(t.target, 21);
    assert!(t.shodo_subpixels > 1);
    let c = browser::cases().iter().find(|c| c.id == t.id).unwrap();
    let built = browser::build(c, &mut cx, &fonts).unwrap();
    assert!(browser::first_end(c, &built, &mut cx, t.shodo_subpixels - 1).unwrap() < 21);
    assert!(browser::first_end(c, &built, &mut cx, t.shodo_subpixels).unwrap() >= 21);
}

#[test]
fn exact_difference_ledger_rejects_changes_improvements_duplicates_and_missing_entries() {
    let result = browser::Comparison {
        samples: 1,
        mismatches: vec![browser::Mismatch {
            font_ids: vec!["latin".into()],
            font_size: 16.0,
            id: "case".into(),
            seed: 1,
            width_subpixels: 64,
            expected: 3,
            actual: 2,
            text: "abc".into(),
        }],
    };
    let mut entries: Vec<browser::KnownDifference> = serde_json::from_str(
        r#"[
        {"id":"case","width_subpixels":64,"expected":3,"actual":2,"category":"boundary-drift",
        "evidence":"adjacent raw transition differs by one unit","issue":null}]
    "#,
    )
    .unwrap();
    browser::check_differences(&result, &entries).unwrap();
    assert!(browser::check_differences(&result, &[]).is_err());
    entries[0].actual = 1;
    assert!(browser::check_differences(&result, &entries).is_err());
    entries[0].actual = 2;
    let improved = browser::Comparison {
        samples: 1,
        mismatches: vec![],
    };
    assert!(browser::check_differences(&improved, &entries).is_err());
    entries.push(entries[0].clone());
    assert!(browser::check_differences(&result, &entries).is_err());
}

#[test]
fn empty_inline_block_geometry_uses_its_bottom_baseline_in_actual_lines() {
    let case = browser::cases()
        .iter()
        .find(|c| c.id == "atomic-baseline")
        .unwrap();
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let built = browser::build(case, &mut cx, &fonts).unwrap();
    let observation = browser::atomic_observation(&built, &mut cx, 5760)
        .unwrap()
        .unwrap();
    assert_eq!(observation.width, 24.0);
    assert_eq!(observation.height, 18.0);
    assert_eq!(observation.bottom - observation.top, 18.0);
    let shodo::LineResult::Line(line) = built.paragraph.next_line(
        &mut cx,
        built.paragraph.start_token(),
        &Default::default(),
        &shodo::LineConstraint::new(90.0),
        &built.atomics,
    ) else {
        panic!("no line")
    };
    assert_eq!(
        observation.bottom,
        f64::from(line.baseline(shodo::geometry::BaselineKind::Alphabetic))
    );
}

#[test]
fn capture_validation_rejects_missing_atomic_and_invalid_hash_metadata() {
    let mut data = browser::capture().unwrap();
    let r = data
        .records
        .iter_mut()
        .find(|r| r.id == "atomic-baseline")
        .unwrap();
    r.samples[0].atomic = None;
    assert!(data.validate(browser::cases()).is_err());
    let mut data = browser::capture().unwrap();
    data.metadata["inputs_sha256"] = serde_json::json!("bad");
    assert!(data.validate(browser::cases()).is_err());
}

#[test]
fn full_offline_comparison_checks_exact_boundaries_geometry_and_known_differences() {
    let data = browser::capture().unwrap();
    let ledger = browser::difference_ledger().unwrap();
    let fonts = load_fonts(&Default::default()).unwrap();
    let result = browser::check_all(
        &data,
        browser::cases(),
        &mut LayoutContext::new(),
        &fonts,
        &ledger,
    )
    .unwrap();
    assert!(
        result.samples >= 400,
        "all saved initial/neighbor probes must be compared"
    );
    let mut changed = browser::difference_ledger().unwrap();
    changed.transitions[0].shodo_subpixels += 1;
    assert!(
        browser::check_all(
            &data,
            browser::cases(),
            &mut LayoutContext::new(),
            &fonts,
            &changed
        )
        .is_err()
    );
    let mut changed = browser::difference_ledger().unwrap();
    changed.atomic_geometry[0].shodo.as_mut().unwrap().top += 1.0;
    assert!(
        browser::check_all(
            &data,
            browser::cases(),
            &mut LayoutContext::new(),
            &fonts,
            &changed
        )
        .is_err()
    );
}

#[test]
fn capture_validation_rejects_inconsistent_transitions_and_missing_probes() {
    let original = browser::capture().unwrap();
    original.validate(browser::cases()).unwrap();
    for defect in [
        0,
        u32::MAX,
        1,
        7769,
        7831,
        7832,
        7833,
        7834,
        7835,
        7897,
        8960,
    ] {
        let mut data = browser::capture().unwrap();
        let r = data
            .records
            .iter_mut()
            .find(|r| r.id == "color-ffi")
            .unwrap();
        match defect {
            0 => {
                r.boundary_subpixels = None;
                r.samples.retain(|s| matches!(s.width_subpixels, 1 | 8960));
            }
            u32::MAX => {
                r.samples[0].end_utf16 = 19;
                r.samples[0].end_utf8 = 19;
            }
            width => r.samples.retain(|s| s.width_subpixels != width),
        }
        assert!(
            data.validate(browser::cases()).is_err(),
            "accepted defect {defect}"
        );
    }
}

#[test]
fn real_pre_wrap_tab_consumes_browser_source_endpoint_at_ninety_pixels() {
    let case = browser::cases()
        .iter()
        .find(|c| c.id == "pre-wrap-tab")
        .unwrap();
    let fonts = load_fonts(&Default::default()).unwrap();
    let mut cx = LayoutContext::new();
    let built = browser::build(case, &mut cx, &fonts).unwrap();
    assert_eq!(browser::first_end(case, &built, &mut cx, 5760).unwrap(), 9);
    assert_eq!(browser::first_end(case, &built, &mut cx, 4337).unwrap(), 5);
    assert_eq!(browser::first_end(case, &built, &mut cx, 4338).unwrap(), 9);
}
