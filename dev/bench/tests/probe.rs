use std::process::Command;

#[test]
fn describe_matches_the_complete_timing_matrix() {
    let output = Command::new(env!("CARGO_BIN_EXE_shodo-probe"))
        .arg("--describe")
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Vec<_> = shodo_bench::workloads()
        .iter()
        .map(shodo_bench::Workload::settings)
        .collect();
    assert_eq!(actual.as_array().unwrap().len(), 54);
    assert!(
        actual == serde_json::json!(expected),
        "probe settings differ from timing matrix"
    );
}

fn probe(mode: &str, id: &str, scale: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_shodo-probe"))
        .args([mode, id, scale])
        .output()
        .unwrap()
}

fn font_match_probe(mode: &str, workload: &str, face_count: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_shodo-font-match-probe"))
        .args([mode, workload, face_count])
        .output()
        .unwrap()
}
#[test]
fn wrong_build_mode_and_invalid_inputs_fail_without_a_report() {
    let wrong = if cfg!(feature = "allocation-counting") {
        "--cold"
    } else {
        "--memory"
    };
    for (mode, id, scale) in [
        (wrong, "japanese-short", "1"),
        ("--unknown", "latin-short", "1"),
        ("--cold", "missing", "1"),
        ("--cold", "latin-short", "0"),
    ] {
        let r = probe(mode, id, scale);
        assert!(!r.status.success());
        assert!(r.stdout.is_empty());
    }
}
#[cfg(not(feature = "allocation-counting"))]
#[test]
fn independent_process_cold_runs_have_complete_font_backed_output() {
    let mut digests = Vec::new();
    for _ in 0..2 {
        let r = probe("--cold", "japanese-short", "1");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["mode"], "cold");
        assert_eq!(v["instrumented"], false);
        assert!(v["digest"]["glyphs"].as_u64().unwrap() > 0);
        assert_eq!(v["digest"]["synthetic_glyphs"], 0);
        for phase in [
            "context_init",
            "font_initialization_registration",
            "build",
            "all_lines",
        ] {
            assert!(v["durations_ns"][phase].as_u64().is_some());
        }
        digests.push(v["digest"].clone());
    }
    assert_eq!(digests[0], digests[1]);
}

#[cfg(not(feature = "allocation-counting"))]
#[test]
fn itemize_grapheme_sizes_use_the_fixed_latin_font_workload() {
    let r = probe("--cold", "itemize-graphemes", "1024");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(v["settings"]["id"], "itemize-graphemes");
    assert_eq!(v["settings"]["scale"], 1024);
    assert_eq!(v["settings"]["text"].as_str().unwrap().len(), 1024);
    assert!(v["durations_ns"]["build"].as_u64().unwrap() > 0);
    assert!(v["digest"]["glyphs"].as_u64().unwrap() > 0);
    assert_eq!(v["digest"]["synthetic_glyphs"], 0);
}

#[cfg(not(feature = "allocation-counting"))]
#[test]
fn font_match_probe_covers_registered_misses_and_native_matches() {
    let registered = font_match_probe("--cold", "registered", "4");
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let registered: serde_json::Value = serde_json::from_slice(&registered.stdout).unwrap();
    assert_eq!(registered["settings"]["face_count"], 4);
    assert_eq!(registered["matched_face_slot"], serde_json::Value::Null);
    assert!(
        registered["durations_ns"]["match_cache_miss"]
            .as_u64()
            .unwrap()
            > 0
    );

    let native = font_match_probe("--cold", "native", "4");
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let native: serde_json::Value = serde_json::from_slice(&native.stdout).unwrap();
    assert_eq!(native["settings"]["workload"], "native");
    assert!(native["matched_face_slot"].as_u64().is_some());
}

#[cfg(feature = "allocation-counting")]
#[test]
fn font_match_probe_memory_mode_reports_registration_and_warm_hit_scopes() {
    let output = font_match_probe("--memory", "registered", "4");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["settings"]["face_count"], 4);
    for scope in ["registration", "match_cache_miss", "warm_cache_hit"] {
        assert!(report["scopes"][scope]["calls"].as_u64().is_some());
    }
    assert!(
        report["scopes"]["warm_cache_hit"]["calls"]
            .as_u64()
            .unwrap()
            <= 3
    );
    assert!(
        report["scopes"]["warm_cache_hit"]["allocated_bytes"]
            .as_u64()
            .unwrap()
            <= 128
    );
}

#[cfg(feature = "allocation-counting")]
#[test]
fn memory_scopes_record_ownership_retries_and_releases() {
    for id in [
        "japanese-short",
        "mixed-scripts",
        "nested-atomic",
        "float-retry",
    ] {
        let r = probe("--memory", id, "1");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
        assert_eq!(v["mode"], "memory");
        assert_eq!(v["instrumented"], true);
        assert!(v.get("durations_ns").is_none());
        let scopes = v["scopes"].as_array().unwrap();
        for name in [
            "font_context_init",
            "build",
            "plain_lines",
            "release_plain_lines",
            "justify_lines",
            "release_justify_lines",
            "reuse_widths",
            "rebuild_widths",
            "page_retry",
            "drop_paragraphs",
            "context_shrink_zero",
            "drop_context",
            "drop_fonts",
        ] {
            assert!(scopes.iter().any(|s| s["name"] == name), "{name}");
        }
        for s in scopes {
            let c = &s["counts"];
            let alloc = c["allocated_bytes"].as_i64().unwrap();
            let free = c["deallocated_bytes"].as_i64().unwrap();
            let net = c["net_bytes"].as_i64().unwrap();
            assert_eq!(alloc - free, net);
            assert_eq!(
                c["live_bytes"].as_i64().unwrap() - c["start_live_bytes"].as_i64().unwrap(),
                net
            );
        }
        assert!(
            scopes
                .iter()
                .any(|s| s["counts"]["net_bytes"].as_i64().unwrap() < 0)
        );
        assert!(v["digests"]["plain"]["glyphs"].as_u64().unwrap() > 0);
        assert!(v["digests"]["default"]["glyphs"].as_u64().unwrap_or(0) > 0);
        assert_eq!(v["digests"]["intrinsic"]["intrinsic_measurements"], 1);
        assert_eq!(v["digests"]["default"]["intrinsic_measurements"], 0);
        assert_eq!(v["digests"]["reuse"], v["digests"]["rebuild"]);
        assert!(v["digests"]["pages"]["height_retries"].as_u64().unwrap() > 0);
        if id == "float-retry" {
            assert_eq!(v["digests"]["reuse"]["float_reports"], 6);
        }
    }
}

#[cfg(feature = "allocation-counting")]
#[test]
fn justify_workload_measures_plain_and_justified_ownership_separately() {
    let r = probe("--memory", "justify", "1");
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_ne!(v["digests"]["plain"], v["digests"]["justify"]);
    assert_eq!(v["digests"]["default"], v["digests"]["justify"]);
    assert!(
        v["normalization"]["justify_incremental_net_bytes"]
            .as_i64()
            .unwrap()
            > 0
    );
}

#[cfg(feature = "allocation-counting")]
#[test]
fn ordinary_clusters_do_not_allocate_one_temporary_block_per_cluster() {
    // A per-cluster parts Vec makes build allocations scale with glyph count.
    // These pinned, predominantly single-glyph clusters leave a generous budget
    // for retained stores, shaping buffers, itemization and run windows.
    // The pinned CJK workload also has unrelated per-glyph allocations, so its budget is
    // separate: restoring parts Vec pushes it above five calls per glyph.
    for (id, numerator, denominator) in [
        ("latin-long", 1, 2),
        ("japanese-long", 5, 1),
        ("arabic-long", 1, 2),
    ] {
        let r = probe("--memory", id, "8");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
        let build = v["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "build")
            .unwrap();
        let calls = build["counts"]["calls"].as_u64().unwrap();
        let glyphs = v["digests"]["plain"]["glyphs"].as_u64().unwrap();
        assert!(glyphs > 1000, "{id} must exercise many clusters");
        assert!(
            calls < glyphs * numerator / denominator,
            "{id}: {calls} build allocations for {glyphs} glyphs; temporary cluster blocks must not dominate"
        );
    }
}
