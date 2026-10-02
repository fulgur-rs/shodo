#!/usr/bin/env python3
"""Compare the saved S4 and current-main Shodo candidates on the JT1 inputs.

Each run is a fresh process. Results keep per-process warm samples and per-run
allocation counters so the medians can be recomputed without the local probes.
"""
import argparse
import hashlib
import json
import os
import platform
import statistics
import subprocess
import tomllib
import time
from pathlib import Path

DOCUMENTS = [
    "css/css-text/hyphens/reference/hyphens-out-of-flow-001-ref.html",
    "css/css-text/hyphens/reference/hyphens-auto-001-ref.html",
]
OPERATIONS = ("pipeline", "layout")
TIME_PAIRS = 16
MEMORY_RUNS = 3
WPT_REVISION = "97ea26e26a2aac3eec7e770650b25e7049ed4a4e"
WINDOWS = {"pipeline": "parse_cascade_layout", "layout": "layout"}
MEMORY_FIELDS = ("allocated_bytes", "calls", "peak_extra_bytes", "net_bytes")
LOADED_ABOVE = 6.0


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tree_sha256(root):
    root = Path(root)
    digest = hashlib.sha256()
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(b"\0")
        digest.update(bytes.fromhex(sha256(path)))
    return digest.hexdigest()


def git_revision(path):
    result = subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def lock_raikiri_revision(path):
    lock = tomllib.loads(Path(path).read_text())
    revisions = set()
    for package in lock.get("package", []):
        if package["name"].startswith("raikiri-"):
            source = package.get("source", "")
            if "github.com/fulgur-rs/raikiri.git" not in source:
                raise ValueError(f"unexpected source for {package['name']} in {path}")
            revisions.add(source.rsplit("#", 1)[-1])
    if len(revisions) != 1:
        raise ValueError(f"expected one pinned raikiri revision in {path}; got {sorted(revisions)}")
    return revisions.pop()


def validate_saved_builds(builds, binaries, source_archive, build_manifest):
    if not isinstance(builds, list):
        raise ValueError("saved S4 build manifest must be a list")
    by_mode = {build.get("mode"): build for build in builds}
    if set(by_mode) != {"time", "memory"}:
        raise ValueError("saved S4 build manifest must contain exactly time and memory builds")
    root_manifest = tomllib.loads((source_archive / "Cargo.toml").read_text())
    shodo_dep = root_manifest.get("dependencies", {}).get("shodo", {})
    if shodo_dep.get("default-features") is not False or shodo_dep.get("features") != ["complex-scripts"]:
        raise ValueError("saved S4 probe manifest does not pin default-features=false, complex-scripts")
    integration_manifest = tomllib.loads((source_archive / "s4/dev/raikiri/Cargo.toml").read_text())
    integration_shodo = integration_manifest.get("dependencies", {}).get("shodo", {})
    if integration_shodo.get("default-features") is not False or integration_shodo.get("features") != ["complex-scripts"]:
        raise ValueError("saved S4 integration manifest has a different Shodo feature set")
    for mode, binary in binaries.items():
        build = by_mode[mode]
        expected_features = [] if mode == "time" else ["allocation-counting"]
        if build.get("features") != expected_features:
            raise ValueError(f"unexpected saved S4 {mode} features: {build.get('features')}")
        profile = build.get("profile", {})
        if profile != {
            "opt_level": "3",
            "debuginfo": 0,
            "debug_assertions": False,
            "overflow_checks": False,
            "test": False,
        }:
            raise ValueError(f"unexpected saved S4 {mode} release profile: {profile}")
        overrides = build.get("environment_overrides", {})
        if (
            overrides.get("RUSTFLAGS") != "-D warnings"
            or overrides.get("CARGO_PROFILE_RELEASE_OPT_LEVEL") != "3"
            or overrides.get("CARGO_PROFILE_RELEASE_DEBUG") != "0"
        ):
            raise ValueError(f"saved S4 {mode} build did not use -D warnings")
        argv = build.get("argv", [])
        if "--release" not in argv or (mode == "memory" and "allocation-counting" not in argv):
            raise ValueError(f"saved S4 {mode} build command differs from the recorded mode")
        if build.get("binary_sha256") != sha256(binary):
            raise ValueError(f"saved S4 {mode} binary differs from its build manifest")
        source_snapshot = build_manifest.parent / "sources"
        for relative, digest in build.get("source_hashes", {}).items():
            source_path = source_snapshot / relative
            if not source_path.is_file() or sha256(source_path) != digest:
                raise ValueError(f"saved S4 {mode} source hash mismatch: {relative}")
    return by_mode


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.lower().startswith(("model name", "hardware")):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def validate_provenance(args, saved_builds, current_provenance):
    saved_archive = json.loads(args.saved_s4_archive_provenance.read_text())
    if saved_archive.get("revision") != args.s4_shodo_commit:
        raise ValueError("saved S4 archive revision differs from --s4-shodo-commit")
    if sha256(args.saved_s4_source_archive / "s4" / "Cargo.lock") != saved_archive.get("original_lock_sha256"):
        raise ValueError("saved S4 archive Cargo.lock differs from archive provenance")
    validate_saved_builds(
        saved_builds,
        {"time": args.saved_s4_time, "memory": args.saved_s4_memory},
        args.saved_s4_source_archive,
        args.saved_s4_builds_manifest,
    )
    for label, path in (
        ("saved S4", args.saved_s4_cargo_lock),
        ("current candidate", args.current_candidate_cargo_lock),
    ):
        revision = lock_raikiri_revision(path)
        if revision != args.raikiri_commit:
            raise ValueError(f"{label} lock pins raikiri {revision}, expected {args.raikiri_commit}")
    if current_provenance.get("source_archive_tree_sha256") != tree_sha256(args.saved_s4_source_archive):
        raise ValueError("saved S4 source archive tree differs from current build provenance")
    checks = {
        "saved_s4_archive_sha256": saved_archive.get("archive_sha256"),
        "saved_s4_source_lock_sha256": saved_archive.get("original_lock_sha256"),
        "saved_s4_shodo_revision": args.s4_shodo_commit,
        "current_shodo_revision": args.current_shodo_commit,
        "raikiri_revision": args.raikiri_commit,
        "current_candidate_lock_sha256": sha256(args.current_candidate_cargo_lock),
        "current_selection_sha256": sha256(args.current_selection),
        "current_time_binary_sha256": sha256(args.current_time),
        "current_memory_binary_sha256": sha256(args.current_memory),
    }
    for key, expected in checks.items():
        if current_provenance.get(key) != expected:
            raise ValueError(f"current build provenance mismatch for {key}")
    if current_provenance.get("current_shodo_feature") != "default-features=false,complex-scripts":
        raise ValueError("current build does not pin the expected Shodo feature set")
    if current_provenance.get("release_profile") != {
        "opt_level": "3",
        "debug": "0",
        "rustflags": "-D warnings",
    }:
        raise ValueError("current build release profile differs from the measurement profile")
    expected_measurement_features = {
        "time": ["complex-scripts"],
        "memory": ["complex-scripts", "allocation-counting"],
    }
    if current_provenance.get("measurement_features") != expected_measurement_features:
        raise ValueError("current build feature sets differ from the measurement profile")
    if current_provenance.get("probe_source_hashes") is None:
        raise ValueError("current build provenance is missing probe source hashes")
    current_root = args.current_source_root.resolve()
    current_build_root = args.current_provenance.resolve().parent
    for relative, digest in current_provenance.get("current_candidate_manifest_hashes", {}).items():
        path = current_build_root / relative
        if not path.is_file() or sha256(path) != digest:
            raise ValueError(f"current candidate manifest hash mismatch: {relative}")
    if set(current_provenance.get("current_candidate_manifest_hashes", {})) != {
        "Cargo.toml",
        "s4/dev/bench/Cargo.toml",
        "s4/dev/fixtures/Cargo.toml",
        "s4/dev/raikiri/Cargo.toml",
    }:
        raise ValueError("current build provenance is missing a patched manifest hash")
    current_manifest = tomllib.loads((current_build_root / "Cargo.toml").read_text())
    current_shodo_dep = current_manifest.get("dependencies", {}).get("shodo", {})
    if current_shodo_dep.get("default-features") is not False or current_shodo_dep.get("features") != ["complex-scripts"]:
        raise ValueError("current candidate manifest does not pin default-features=false, complex-scripts")
    expected_shodo_path = (current_root / "crates" / "shodo").as_posix()
    if current_shodo_dep.get("path") != expected_shodo_path:
        raise ValueError("current probe is not linked to the pinned Shodo crate path")
    for relative in ("s4/dev/bench/Cargo.toml", "s4/dev/fixtures/Cargo.toml", "s4/dev/raikiri/Cargo.toml"):
        manifest = tomllib.loads((current_build_root / relative).read_text())
        dependency = manifest.get("dependencies", {}).get("shodo", {})
        if dependency.get("path") != expected_shodo_path or dependency.get("default-features") is not False:
            raise ValueError(f"current candidate dependency differs in {relative}")
    if tomllib.loads((current_build_root / "s4/dev/raikiri/Cargo.toml").read_text())[
        "dependencies"
    ]["shodo"].get("features") != ["complex-scripts"]:
        raise ValueError("current integration probe does not enable complex-scripts")
    if not current_provenance.get("toolchain", {}).get("rustc", "").startswith("rustc 1.96.0 "):
        raise ValueError("current build rustc version differs from the recorded measurement")

    built_head = current_provenance.get("current_shodo_head_at_build")
    subprocess.run(["git", "-C", str(current_root), "cat-file", "-e", f"{built_head}^{{commit}}"], check=True)
    subprocess.run(["git", "-C", str(current_root), "cat-file", "-e", f"{args.current_shodo_commit}^{{commit}}"], check=True)
    subprocess.run(
        ["git", "-C", str(current_root), "merge-base", "--is-ancestor", args.current_shodo_commit, built_head],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(current_root), "merge-base", "--is-ancestor", built_head, "HEAD"],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(current_root), "diff", "--quiet", args.current_shodo_commit, "HEAD", "--", "Cargo.toml", "Cargo.lock", "crates/shodo"],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(current_root), "diff", "--quiet", built_head, "HEAD", "--", "Cargo.toml", "Cargo.lock", "crates/shodo", "dev/raikiri/probe"],
        check=True,
    )
    dirty = subprocess.run(
        ["git", "-C", str(current_root), "status", "--short", "--", "Cargo.toml", "Cargo.lock", "crates/shodo", "dev/raikiri/probe"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if dirty:
        raise ValueError("current Shodo/probe sources have uncommitted changes")
    if sha256(current_root / "Cargo.lock") != current_provenance.get("current_shodo_workspace_lock_sha256"):
        raise ValueError("current Shodo workspace lock differs from build provenance")
    for relative, digest in current_provenance["probe_source_hashes"].items():
        source = current_root / relative
        if not source.is_file() or sha256(source) != digest:
            raise ValueError(f"current probe source hash mismatch: {relative}")
    return saved_archive


def cpu_busy(seconds=1.0):
    def read():
        rows = {}
        for line in Path("/proc/stat").read_text().splitlines():
            if line.startswith("cpu") and line[3].isdigit():
                fields = [int(value) for value in line.split()[1:]]
                rows[int(line.split()[0][3:])] = (sum(fields), fields[3] + fields[4])
        return rows

    before = read()
    time.sleep(seconds)
    after = read()
    return {
        cpu: 1 - (after[cpu][1] - before[cpu][1]) / max(1, after[cpu][0] - before[cpu][0])
        for cpu in after
    }


def slug(document):
    return document.rsplit("/", 1)[1].removesuffix(".html")


def load_selection(path):
    data = json.loads(Path(path).read_text())
    if data.get("wpt_revision") != WPT_REVISION:
        raise ValueError(f"selection WPT revision is not pinned: {path}")
    cases = {case["id"]: case for case in data.get("cases", [])}
    if set(cases) != set(DOCUMENTS):
        raise ValueError(f"selection must contain exactly the two JT1 references: {path}")
    for document, case in cases.items():
        if len(case.get("font_sha256", [])) != 88:
            raise ValueError(f"expected 88 pinned fonts for {document}: {path}")
        if case["candidate_page"].get("width") != 800 or case["candidate_page"].get("height") != 600:
            raise ValueError(f"viewport differs from 800x600 for {document}: {path}")
    return data, cases


def ratio_summary(values):
    return {
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "values": values,
    }


def run_probe(binary, mode, engine, wpt, selection, document, operation, output):
    argv = [
        str(binary),
        mode,
        engine,
        str(wpt),
        str(selection),
        document,
        str(output),
        operation,
    ]
    result = subprocess.run(argv, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(
            f"probe failed ({result.returncode}): {' '.join(argv)}\n{result.stderr[-1200:]}"
        )
    recorded_argv = [
        f"<{mode}-binary>",
        mode,
        engine,
        "<wpt-checkout>",
        "<selection-json>",
        document,
        "<scratch-report>",
        operation,
    ]
    return json.loads(output.read_text()), recorded_argv


def warm_time(report, operation):
    key = WINDOWS[operation]
    samples = report["samples"]
    if len(samples) != 10 or samples[0].get("state") != "first-call-in-process":
        raise ValueError("expected one first call and nine warm calls")
    warm = samples[1:]
    if any(sample.get("state") != "warm-process" for sample in warm):
        raise ValueError("all samples after first call must be warm")
    return [sample[key]["duration_ns"] for sample in warm]


def time_pair(scratch, arm_name, config, operation, document, wpt, repeat):
    order = ["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]
    runs = {}
    for engine in order:
        output = scratch / f"time-{operation}-{slug(document)}-r{repeat:02d}-{arm_name}-{engine}.json"
        report, argv = run_probe(
            config["time_binary"],
            "time",
            engine,
            wpt,
            config["selection"],
            document,
            operation,
            output,
        )
        samples = warm_time(report, operation)
        runs[engine] = {"argv": argv, "warm_samples_ns": samples, "median_ns": statistics.median(samples)}
    return {
        "repeat": repeat,
        "order": order,
        "native": runs["native"],
        "candidate": runs["candidate"],
        "candidate_over_native": runs["candidate"]["median_ns"] / runs["native"]["median_ns"],
    }


def summarize_time(pairs):
    ratios = [pair["candidate_over_native"] for pair in pairs]
    return {
        "pairs": pairs,
        "candidate_over_native": ratio_summary(ratios),
        "process_median_ns": {
            engine: statistics.median(pair[engine]["median_ns"] for pair in pairs)
            for engine in ("native", "candidate")
        },
        "pairs_candidate_slower": sum(ratio > 1 for ratio in ratios),
    }


def warm_memory(report, operation):
    key = WINDOWS[operation]
    samples = report["samples"]
    if len(samples) != 10 or samples[0].get("state") != "first-call-in-process":
        raise ValueError("expected one first call and nine warm calls")
    warm = samples[1:]
    if any(sample.get("state") != "warm-process" for sample in warm):
        raise ValueError("all samples after first call must be warm")
    raw = [{field: sample[key]["counts"][field] for field in MEMORY_FIELDS} for sample in warm]
    medians = {field: statistics.median(row[field] for row in raw) for field in MEMORY_FIELDS}
    return raw, medians


def memory_run(scratch, arm_name, config, operation, document, wpt, repeat):
    order = ["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]
    results = {}
    for engine in order:
        output = scratch / f"memory-{operation}-{slug(document)}-r{repeat:02d}-{arm_name}-{engine}.json"
        report, argv = run_probe(
            config["memory_binary"],
            "memory",
            engine,
            wpt,
            config["selection"],
            document,
            operation,
            output,
        )
        samples, medians = warm_memory(report, operation)
        results[engine] = {"argv": argv, "warm_samples": samples, "median": medians}
    return {"repeat": repeat, "order": order, **results}


def summarize_memory(runs):
    medians = {
        engine: {
            field: statistics.median(run[engine]["median"][field] for run in runs)
            for field in MEMORY_FIELDS
        }
        for engine in ("native", "candidate")
    }
    ratios = {
        field: medians["candidate"][field] / medians["native"][field]
        if medians["native"][field]
        else None
        for field in MEMORY_FIELDS
    }
    return {"runs": runs, "run_median": medians, "candidate_over_native": ratios}


def candidate_cross_arm(arms, operation, document):
    saved_time = arms["saved_s4"]["time"][operation][document]
    current_time = arms["current_main"]["time"][operation][document]
    time_ratios = [
        current["candidate"]["median_ns"] / saved["candidate"]["median_ns"]
        for saved, current in zip(saved_time["pairs"], current_time["pairs"])
    ]
    saved_memory = arms["saved_s4"]["memory"][operation][document]
    current_memory = arms["current_main"]["memory"][operation][document]
    memory_ratios = {
        field: [
            current_run["candidate"]["median"][field] / saved_run["candidate"]["median"][field]
            if saved_run["candidate"]["median"][field]
            else None
            for saved_run, current_run in zip(saved_memory["runs"], current_memory["runs"])
        ]
        for field in MEMORY_FIELDS
    }
    return {
        "current_candidate_over_saved_s4_candidate_time": ratio_summary(time_ratios),
        "current_candidate_over_saved_s4_candidate_memory": {
            field: ratio_summary([value for value in values if value is not None])
            if all(value is not None for value in values)
            else None
            for field, values in memory_ratios.items()
        },
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wpt", type=Path, required=True)
    parser.add_argument("--saved-s4-time", type=Path, required=True)
    parser.add_argument("--saved-s4-memory", type=Path, required=True)
    parser.add_argument("--saved-s4-builds-manifest", type=Path, required=True)
    parser.add_argument("--saved-s4-archive-provenance", type=Path, required=True)
    parser.add_argument("--saved-s4-source-archive", type=Path, required=True)
    parser.add_argument("--saved-s4-cargo-lock", type=Path, required=True)
    parser.add_argument("--saved-s4-selection", type=Path, required=True)
    parser.add_argument("--original-selection", type=Path, required=True)
    parser.add_argument("--current-time", type=Path, required=True)
    parser.add_argument("--current-memory", type=Path, required=True)
    parser.add_argument("--current-selection", type=Path, required=True)
    parser.add_argument("--current-provenance", type=Path, required=True)
    parser.add_argument("--current-candidate-cargo-lock", type=Path, required=True)
    parser.add_argument("--current-source-root", type=Path, required=True)
    parser.add_argument("--s4-shodo-commit", required=True)
    parser.add_argument("--current-shodo-commit", required=True)
    parser.add_argument("--raikiri-commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--scratch", type=Path, required=True)
    parser.add_argument("--validate-only", action="store_true", help="verify pinned inputs without collecting measurements")
    args = parser.parse_args()

    wpt_revision = git_revision(args.wpt)
    if wpt_revision != WPT_REVISION:
        raise SystemExit(f"WPT checkout is {wpt_revision}, expected {WPT_REVISION}")
    _, saved_cases = load_selection(args.saved_s4_selection)
    _, current_cases = load_selection(args.current_selection)
    original_data = json.loads(args.original_selection.read_text())
    if original_data.get("wpt_revision") != WPT_REVISION:
        raise SystemExit("original selection WPT revision is not pinned")
    original_cases = {case["id"]: case for case in original_data.get("cases", [])}
    if any(document not in original_cases for document in DOCUMENTS):
        raise SystemExit("original selection is missing a JT1 reference")
    if any(
        saved_cases[document]["font_sha256"] != current_cases[document]["font_sha256"]
        or saved_cases[document]["font_sha256"] != original_cases[document]["font_sha256"]
        for document in DOCUMENTS
    ):
        raise SystemExit("saved S4 and current selection font hashes/order differ")
    current_provenance = json.loads(args.current_provenance.read_text())
    saved_builds = json.loads(args.saved_s4_builds_manifest.read_text())
    saved_archive = validate_provenance(args, saved_builds, current_provenance)
    if args.validate_only:
        print("saved S4 and current-main measurement inputs validated")
        return
    args.scratch.mkdir(parents=True, exist_ok=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)

    configurations = {
        "saved_s4": {
            "time_binary": args.saved_s4_time,
            "memory_binary": args.saved_s4_memory,
            "selection": args.saved_s4_selection,
        },
        "current_main": {
            "time_binary": args.current_time,
            "memory_binary": args.current_memory,
            "selection": args.current_selection,
        },
    }
    busy_before = cpu_busy()
    allowed_cpus = sorted(os.sched_getaffinity(0))
    cpu = min(allowed_cpus, key=lambda item: busy_before[item])
    loadavg_before = os.getloadavg()
    os.sched_setaffinity(0, {cpu})
    arms = {arm: {"time": {}, "memory": {}} for arm in configurations}
    for operation in OPERATIONS:
        for document in DOCUMENTS:
            pairs_by_arm = {arm: [] for arm in configurations}
            for repeat in range(TIME_PAIRS):
                arm_order = ["saved_s4", "current_main"] if repeat % 2 == 0 else ["current_main", "saved_s4"]
                for arm in arm_order:
                    pairs_by_arm[arm].append(time_pair(
                        args.scratch,
                        arm,
                        configurations[arm],
                        operation,
                        document,
                        args.wpt,
                        repeat,
                    ))
            for arm in configurations:
                arms[arm]["time"].setdefault(operation, {})[document] = summarize_time(pairs_by_arm[arm])
    for operation in OPERATIONS:
        for document in DOCUMENTS:
            runs_by_arm = {arm: [] for arm in configurations}
            for repeat in range(MEMORY_RUNS):
                arm_order = ["saved_s4", "current_main"] if repeat % 2 == 0 else ["current_main", "saved_s4"]
                for arm in arm_order:
                    runs_by_arm[arm].append(memory_run(
                        args.scratch,
                        arm,
                        configurations[arm],
                        operation,
                        document,
                        args.wpt,
                        repeat,
                    ))
            for arm in ("saved_s4", "current_main"):
                arms[arm]["memory"].setdefault(operation, {})[document] = summarize_memory(runs_by_arm[arm])
    busy_after = cpu_busy()
    loadavg_after = os.getloadavg()

    time_comparisons = {
        operation: {
            document: candidate_cross_arm(arms, operation, document)
            for document in DOCUMENTS
        }
        for operation in OPERATIONS
    }
    selection_geometry_differences = {}
    for document in DOCUMENTS:
        saved_blocks = {block["node"]: block for block in saved_cases[document]["candidate_page"]["blocks"]}
        current_blocks = {block["node"]: block for block in current_cases[document]["candidate_page"]["blocks"]}
        differences = []
        for node in sorted(saved_blocks.keys() | current_blocks.keys()):
            for field in sorted(saved_blocks.get(node, {}).keys() | current_blocks.get(node, {}).keys()):
                old_value = saved_blocks.get(node, {}).get(field)
                new_value = current_blocks.get(node, {}).get(field)
                if old_value != new_value:
                    differences.append({"node": node, "field": field, "saved_s4": old_value, "current_main": new_value})
        selection_geometry_differences[document] = differences

    summary = {
        "schema": 1,
        "scope": "Saved S4 candidate and current-main Shodo candidate against native raikiri on the original two references; warm timing and allocator windows only; not a WPT verdict or switching decision.",
        "pins": {
            "wpt_revision": wpt_revision,
            "raikiri_revision": args.raikiri_commit,
            "saved_s4_shodo_revision": args.s4_shodo_commit,
            "current_shodo_revision": args.current_shodo_commit,
            "saved_s4_build_manifest_sha256": sha256(args.saved_s4_builds_manifest),
            "saved_s4_archive_sha256": saved_archive["archive_sha256"],
            "saved_s4_archive_provenance_sha256": sha256(args.saved_s4_archive_provenance),
            "saved_s4_source_lock_sha256": saved_archive["original_lock_sha256"],
            "saved_s4_probe_lock_sha256": sha256(args.saved_s4_cargo_lock),
            "current_candidate_lock_sha256": sha256(args.current_candidate_cargo_lock),
            "original_selection_sha256": sha256(args.original_selection),
            "saved_selection_sha256": sha256(args.saved_s4_selection),
            "current_selection_sha256": sha256(args.current_selection),
            "current_provenance_sha256": sha256(args.current_provenance),
            "current_source_archive_tree_sha256": current_provenance["source_archive_tree_sha256"],
            "current_shodo_workspace_lock_sha256": current_provenance["current_shodo_workspace_lock_sha256"],
            "current_lock_sha256": current_provenance["current_candidate_lock_sha256"],
            "current_shodo_feature": current_provenance["current_shodo_feature"],
            "current_probe_source_hashes": current_provenance["probe_source_hashes"],
            "saved_s4_time_binary_sha256": sha256(args.saved_s4_time),
            "saved_s4_memory_binary_sha256": sha256(args.saved_s4_memory),
            "current_time_binary_sha256": sha256(args.current_time),
            "current_memory_binary_sha256": sha256(args.current_memory),
            "font_hashes_by_document": {document: current_cases[document]["font_sha256"] for document in DOCUMENTS},
            "font_count_per_document": 88,
            "effective_features": "complex-scripts for both candidates; allocation-counting off for time binaries, on for memory binaries",
            "saved_s4_release_profile": "opt-level=3, debuginfo=0, default Cargo features, RUSTFLAGS=-D warnings; original build manifest invokes cargo +stable but does not record the exact rustc version",
            "current_main_release_profile": "opt-level=3, debuginfo=0, RUSTFLAGS=-D warnings, rustc 1.96.0",
            "saved_s4_build_modes": [build["mode"] for build in saved_builds],
        },
        "method": {
            "viewport_css_px": [800, 600],
            "documents": DOCUMENTS,
            "operations": list(OPERATIONS),
            "time_pairs_per_arm_operation_document": TIME_PAIRS,
            "memory_runs_per_arm_operation_document": MEMORY_RUNS,
            "time_order": "native/candidate alternates inside each 16-pair arm; saved-S4/current-main arms were scheduled in alternating order",
            "memory_order": "native/candidate alternates by run; saved-S4/current-main arms were scheduled in alternating order; counters are median across nine warm samples per process",
            "cpu_affinity": [cpu],
            "time_warm_samples_per_process": 9,
            "allocation_metrics": "requested heap counters (allocated_bytes, calls, peak_extra_bytes, net_bytes), not RSS",
            "load_label_threshold": "loaded if max(1-minute load average before, after) > 6.0; load during run is not sampled",
        },
        "environment": {
            "label": "loaded" if max(loadavg_before[0], loadavg_after[0]) > LOADED_ABOVE else "quiet",
            "loadavg_before": loadavg_before,
            "loadavg_after": loadavg_after,
            "affinity_cpu_busy_before": busy_before[cpu],
            "affinity_cpu_busy_after": busy_after[cpu],
            "allowed_cpus_before_pinning": allowed_cpus,
            "host": {
                "os": platform.system(),
                "os_release": platform.release(),
                "os_version": platform.version(),
                "architecture": platform.machine(),
                "cpu_model": cpu_model(),
            },
        },
        "candidate_selection_geometry_differences": selection_geometry_differences,
        "arms": arms,
        "cross_arm_candidate_comparison": time_comparisons,
        "scope_limits": [
            "The native caller prepares all DOM text; the candidate projects only the paired leaf IFCs.",
            "Native retains its laid-out DOM clone and atomic map. Candidate returns geometry blocks and releases paragraph/line/context within the caller; configured font caches remain shared.",
            "The current-main selection records current Shodo geometry, so the candidate inputs match their respective candidate outputs; out-of-flow has seven nodes with sub-micro-pixel width differences from saved S4.",
            "These are caller-input and retained-owner differences, not a WPT PASS claim. Cold startup, paint, and switching necessity are outside scope.",
        ],
    }
    args.output.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"wrote {args.output}; environment={summary['environment']['label']}; CPU={cpu}")


if __name__ == "__main__":
    main()
