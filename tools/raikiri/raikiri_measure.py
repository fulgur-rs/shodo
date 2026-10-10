#!/usr/bin/env python3
"""Validated original-input raikiri measurements on disposable frozen S4.

Time and requested-heap accounting use distinct binaries. Unsupported input or
different paged semantics are evidence, never a successful performance pair.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
import platform
import shutil
import statistics
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OPERATIONS = {
    "layout": "whole-layout-initial",
    "pipeline": "parse-cascade-layout-and-width-reentry",
    "isolated": "isolated-text-pipeline-width-reuse-and-height-retry",
    "pagination": "provisional-complete-caller-pagination",
}


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def sibling(name):
    return load(name, HERE / f"{name}.py")


foundation = load("run", ROOT / "tools" / "bench" / "run.py")
overlay = sibling("raikiri_overlay")
report_paths = overlay.report_paths
require = foundation.require
file_hash = foundation.file_hash


def numeric(value, *, signed=False, integer=False):
    require(type(value) in ((int,) if integer else (int, float)), "invalid numeric measurement")
    require(math.isfinite(value) and (signed or value >= 0), "negative/nonfinite measurement")


def windows(report, operation):
    """Enumerate required, disjoint windows; missing paths cannot disappear."""
    result = {name: report[name] for name in ["font_setup", "release_font_registry"]}
    if operation == "isolated":
        result.update({name: report[name] for name in ["geometry_preconditioning", "release_geometry"]})
    if operation == "pagination":
        result.update({name: report[name] for name in ["capability_preconditioning", "release_capability_inputs"]})
    for i, sample in enumerate(report["samples"]):
        if operation == "layout":
            for name in ["layout", "release_output"]:
                result[f"{i}/{name}"] = sample[name]
        elif operation in ["pipeline", "pagination"]:
            name = "parse_cascade_layout" if operation == "pipeline" else "parse_cascade_paginate"
            result[f"{i}/{name}"] = sample[name]
            result[f"{i}/release_pipeline_owner"] = sample["release_pipeline_owner"]
            key = "complete_caller_width_reentry" if operation == "pipeline" else "complete_caller_height_reentry"
            expected = [400, 1200, 800] if operation == "pipeline" else [64, 32, 128]
            rows = sample[key]
            require(len(rows) == 3, "missing complete caller reentry")
            for step, row in zip(expected, rows):
                require(row["viewport_css_px"] == ([step, 600] if operation == "pipeline" else [800, step]), "reentry viewport differs")
                result[f"{i}/reentry/{step}"] = row["measurement"]
        else:
            for name in ["initial_text_pipeline", "mutable_reuse_owner_setup", "release_prepared_owner"]:
                result[f"{i}/{name}"] = sample[name]
            rows = sample["reused_widths_and_retries"]
            require(len(rows) == 4, "missing isolated width or height retry")
            for step, row in enumerate(rows):
                require(row["root_width_multiplier"] == [1.0, 0.5, 1.5, 1.0][step], "isolated root width differs")
                for name in ["reused_shape_width", "height_rejected_retry"]:
                    result[f"{i}/{name}/{step}"] = row[name]
                numeric(row["engine_height_rejections"], integer=True)
                numeric(row["caller_height_rejections"], integer=True)
    return result


def outputs(report, operation):
    if operation == "layout":
        return [report["output"]]
    key = "initial_output" if operation == "isolated" else "output"
    result = [report[key]]
    field = {"pipeline": "complete_caller_width_reentry", "isolated": "reused_widths_and_retries",
             "pagination": "complete_caller_height_reentry"}[operation]
    for sample in report["samples"]:
        result.extend(row["output"] for row in sample[field])
    return result


def source_rows(rows, engine, expected_roots):
    require(isinstance(rows, list) and rows, "missing accepted source output")
    require({r["root"] for r in rows} == expected_roots, "accepted source roots differ")
    for row in rows:
        snapshot = row["snapshot"]
        lines = snapshot["lines"] if engine == "candidate" else snapshot
        require(isinstance(lines, list), "missing source lines")
        end = 0
        for line in lines:
            span = line["source_range"] if engine == "candidate" else line["range"]
            numeric(span["start"], integer=True)
            numeric(span["end"], integer=True)
            require(span["start"] == end and span["end"] >= end, "accepted source gap/overlap")
            end = span["end"]
        if engine == "candidate":
            require(end == row["processed_source_bytes"], "shortened/unconsumed accepted source")


def paged_output(value, engine, roots):
    require(value["source_partition_complete"] is True, "unsupported/incomplete pagination")
    numeric(value["page_count"], integer=True)
    require(value["page_count"] > 0, "missing committed pages")
    require({r["root"] for r in value["root_widths"]} == roots, "paged roots differ")
    require(isinstance(value["snapshots"], list) and value["snapshots"], "missing paged source/glyph snapshot")
    counts = {}
    for row in value["snapshots"]:
        key = (row["root"], row.get("text_node"))
        require(key not in counts, "duplicate paged source owner")
        counts[key] = [len(row["lines"]), 0]
        if engine == "candidate":
            require(len(row["processed_text"].encode()) == row["processed_source_bytes"], "shortened paged source")
            source_rows([{"root": row["root"], "processed_source_bytes": row["processed_source_bytes"],
                          "snapshot": {"lines": row["lines"]}}], engine, {row["root"]})
    require({key[0] for key in counts} == roots, "missing paged source root")
    for row in value["partition"]:
        key = (row["root"], row.get("text_node"))
        require(key in counts, "unknown paged source owner")
        total, end = counts[key]
        require(type(row["page"]) is int and 0 <= row["page"] < value["page_count"], "invalid source page")
        require(row["line_start"] == end and end < row["line_end"] <= total, "paged line gap/overlap")
        counts[key][1] = row["line_end"]
    require(all(total == end for total, end in counts.values()), "missing source fragment")
    if engine == "candidate":
        require(value["retry_trace_visible"] is True, "missing actual candidate retry trace")
        numeric(value["actual_height_rejections"], integer=True)
        numeric(value["actual_lookahead_rejections"], integer=True)
    else:
        require(value["retry_trace_visible"] is False and value["actual_height_rejections"] is None
                and value["actual_lookahead_rejections"] is None, "fabricated native retry events")


def validate_record(record, case, operation, mode, engine, *, reference=None, for_comparison=False):
    """Reject drift against original inputs and fresh same-engine output proof."""
    try:
        require(operation in OPERATIONS and mode in ["time", "memory"] and engine in ["native", "candidate"], "unknown measurement configuration")
        require(record["schema"] == 1 and record["operation"] == OPERATIONS[operation], "wrong operation/schema")
        require(record["id"] == case["id"] and record["mode"] == mode and record["engine"] == engine, "wrong original case/mode/engine")
        require(type(record["instrumented"]) is bool and record["instrumented"] == (mode == "memory"), "instrumented timing or wrong memory build")
        require(record["font_sha256"] == case["font_sha256"] and record["resources"] == case["resources"]
                and record["parse_warnings"] == case["parse_warnings"], "original font/resource/warning drift")
        require(record["viewport_css_px"] == ([800, 128] if operation == "pagination" else [800, 600]), "initial viewport differs")
        require(record["scope"] and record["retained_owner"] and record["preconditioning"], "missing measurement boundary/owner")
        require(not for_comparison or not record.get("correctness_only", False), "correctness-only work cannot be timed as benchmark work")
        require(len(record["samples"]) == 10, "missing cold/warm repetitions")
        for i, sample in enumerate(record["samples"]):
            require(sample["state"] == ("first-call-in-process" if i == 0 else "warm-process"), "cold/warm state differs")
        net = 0
        for window in windows(record, operation).values():
            if mode == "time":
                require(set(window) == {"duration_ns"}, "allocation instrumentation in a timing window")
                numeric(window["duration_ns"], integer=True)
            else:
                require(set(window) == {"counts"}, "missing allocator window or instrumented timing")
                counts = window["counts"]
                require(set(counts) == {"calls", "allocated_bytes", "deallocated_bytes", "start_live_bytes", "live_bytes", "peak_extra_bytes", "net_bytes"}, "missing allocator counts")
                for key, value in counts.items():
                    numeric(value, integer=True, signed=key == "net_bytes")
                require(counts["net_bytes"] == counts["allocated_bytes"] - counts["deallocated_bytes"]
                        == counts["live_bytes"] - counts["start_live_bytes"], "allocator ownership arithmetic differs")
                require(counts["peak_extra_bytes"] >= max(0, counts["net_bytes"]), "allocator peak misses retained bytes")
                net += counts["net_bytes"]
        require(mode != "memory" or net == 0, "measured page/cache owners are unbalanced")
        roots = {b["node"] for b in case["candidate_page"]["blocks"]}
        values = outputs(record, operation)
        if operation in ["layout", "pipeline"]:
            for value in values:
                require({r["node"] for r in value} == roots, "layout roots missing/changed")
            if engine == "candidate":
                require(values[0] == case["candidate_page"]["blocks"], "original candidate geometry changed")
            if operation == "pipeline":
                require(record["width_sequence_css_px"] == [800, 400, 1200, 800], "wrong width sequence")
                for sample in record["samples"]:
                    require(sample["complete_caller_width_reentry"][-1]["output"] == values[0], "restored width output differs")
        elif operation == "isolated":
            widths = [{"root": b["node"], "width": b["content_width"]} for b in case["candidate_page"]["blocks"]]
            require(record["root_content_width_css_px"] == widths and record["root_width_multipliers"] == [1.0, 0.5, 1.5, 1.0], "root content-width drift")
            require(record["initial_boundary_difference"] and record["mutable_reuse_owner_setup"] and record["retry_boundary"], "missing isolated boundary differences")
            for value in values:
                source_rows(value, engine, roots)
            for sample in record["samples"]:
                require(sample["reused_widths_and_retries"][-1]["output"] == sample["reused_widths_and_retries"][0]["output"], "restored isolated source/glyph differs")
        else:
            require(record["height_sequence_css_px"] == [128, 64, 32, 128], "wrong page height sequence")
            require(record["candidate_boundary"] and record["native_boundary"], "missing paged API differences")
            for value in values:
                paged_output(value, engine, roots)
                require(value["snapshots"] == values[0]["snapshots"], "paged source/glyph changes with height")
            for sample in record["samples"]:
                require(sample["complete_caller_height_reentry"][-1]["output"] == values[0], "restored height differs")
        if reference is not None:
            require(reference["id"] == record["id"] and reference["engine"] == engine
                    and reference["operation"] == record["operation"], "wrong reference operation/input")
            require(outputs(reference, operation) == values, "accepted output differs from fresh source/glyph reference")
        if for_comparison and operation in ["pipeline", "isolated", "pagination"]:
            require(reference is not None, "comparison needs actual fresh-output verification")
            flag = {"pipeline": "fresh_width_outputs_verified", "isolated": "fresh_shape_outputs_verified",
                    "pagination": "fresh_fragment_outputs_verified"}[operation]
            require(reference.get(flag) is True, "missing fresh-output verification")
    except (KeyError, TypeError, IndexError) as error:
        raise ValueError(f"missing or invalid measurement/output: {error}") from error


def validate_pair(native, candidate, case, operation, mode):
    validate_record(native, case, operation, mode, "native")
    validate_record(candidate, case, operation, mode, "candidate")
    if operation == "pagination":
        for left, right in zip(outputs(native, operation), outputs(candidate, operation)):
            require(left["root_widths"] == right["root_widths"], "paged root content widths differ")
            node_owners = {}
            for row in left["snapshots"]:
                require(row["root"] not in node_owners, "multiple native text-node layouts are not one candidate IFC")
                node_owners[row["root"]] = row["text_node"]
            native_partition = [{k: v for k, v in row.items() if k != "text_node"} for row in left["partition"]]
            require(native_partition == right["partition"], "paged source/line/page partitions differ")
    return True


def validate_build(artifact, mode, *, binary_name="measurement-probe"):
    try:
        require(artifact["reason"] == "compiler-artifact" and artifact["target"]["name"] == binary_name
                and "bin" in artifact["target"]["kind"] and artifact["executable"], "missing actual probe build")
        require(artifact["features"] == ([] if mode == "time" else ["allocation-counting"]), "actual instrumentation features differ")
        profile = artifact["profile"]
        require(profile["opt_level"] == "3" and profile["debuginfo"] == 0
                and profile["debug_assertions"] is False and profile["test"] is False,
                "actual build profile differs from optimized release")
    except (KeyError, TypeError) as error:
        raise ValueError(f"missing actual build provenance: {error}") from error


def write_json(path, value):
    report_paths.write_json(path, value, ROOT)


def source_hashes():
    paths = list((ROOT / "dev/raikiri/probe").glob("*.rs"))
    paths += [HERE / "raikiri_measure.py", HERE / "raikiri_overlay.py", HERE / "report_paths.py",
              ROOT / "tools" / "bench" / "run.py", ROOT / "dev/bench/src/allocator.rs"]
    return {str(p.relative_to(ROOT)): file_hash(p) for p in sorted(paths)}


def selection(path):
    value = json.loads(Path(path).read_text())
    require(value["wpt_revision"] == "97ea26e26a2aac3eec7e770650b25e7049ed4a4e", "wrong original WPT pin")
    cases = value["cases"]
    require(isinstance(cases, list) and cases and len({c["id"] for c in cases}) == len(cases), "empty or duplicate selected cases")
    for case in cases:
        require(case["candidate_page"]["classification"] == "candidate-static-leaf-block-page"
                and case["native_page"]["classification"] == "native-static-screen-page", "unsupported original whole-page case")
        require(len(case["font_sha256"]) == 88 and case["candidate_page"]["blocks"], "missing original fonts/roots")
    return value


def diagnostic_source(path, value):
    source = Path(value["source"])
    return source if source.is_absolute() else Path(path).resolve().parent / source


def diagnostic_inventory(path):
    value = json.loads(Path(path).read_text())
    require(file_hash(diagnostic_source(path, value)) == value["source_sha256"], "native diagnostic source log changed")
    require(value["diagnostic_count"] == len(value["messages"]), "native diagnostic count differs")
    documents = {m["id"] for m in value["messages"]}
    require(set(value["documents"]) == documents and value["document_count"] == len(documents), "native diagnostic case set differs")
    require(all(m["phase"] == "native-initial" for m in value["messages"]), "wrong native diagnostic phase")
    return documents


def archive_diagnostics(path, stage):
    value = json.loads(Path(path).read_text())
    source = diagnostic_source(path, value)
    saved = stage / "original-native-diagnostics.log"
    shutil.copy2(source, saved)
    require(file_hash(saved) == value["source_sha256"], "archived native diagnostic source log changed")
    value.setdefault("original_source", value["source"])
    value["source"] = saved.name
    write_json(stage / "native-diagnostic-inventory.json", value)


def invoke(binary, mode, engine, wpt, selector, case, output, operation):
    argv = [str(binary), mode, engine, str(wpt), str(selector), case["id"], str(output), operation]
    run = subprocess.run(argv, capture_output=True, text=True)
    log = Path(output).with_suffix(".log")
    log.write_text(run.stdout + run.stderr)
    row = {"argv": argv, "returncode": run.returncode, "output": str(output), "log": str(log),
           "log_sha256": file_hash(log), "diagnostic": run.stderr[-1200:]}
    if run.returncode == 0:
        require(Path(output).is_file(), "successful child produced no measurement output")
        row["output_sha256"] = file_hash(output)
    else:
        require(not Path(output).exists(), "failed child left a successful measurement output")
    return row


def build(stage, probe, args, env, sources, *, binary_name="measurement-probe"):
    binaries = {}
    builds = []
    manifest = probe / "Cargo.toml"
    for mode in ["time", "memory"]:
        argv = ["cargo", "+stable", "build", "--offline", "--manifest-path", str(manifest),
                "--release", "--bin", binary_name, "--message-format=json"]
        if mode == "memory":
            argv += ["--features", "allocation-counting"]
        output = foundation.execute(argv, stage / f"{mode}-build.log", cwd=probe, env=env)
        artifacts = [json.loads(line) for line in output.splitlines()]
        artifacts = [r for r in artifacts if r.get("reason") == "compiler-artifact"
                     and r["target"]["name"] == binary_name and r.get("executable")]
        require(len(artifacts) == 1, "missing/ambiguous actual probe artifact")
        validate_build(artifacts[0], mode, binary_name=binary_name)
        destination = stage / f"{binary_name}-{mode}"
        shutil.copy2(artifacts[0]["executable"], destination)
        binaries[mode] = destination
        tree_argv = ["cargo", "+stable", "tree", "--offline", "--locked", "--manifest-path", str(manifest), "-e", "features"]
        if mode == "memory":
            tree_argv += ["--features", "allocation-counting"]
        foundation.execute(tree_argv, stage / f"{mode}-feature-graph.log", cwd=probe, env=env)
        builds.append({"mode": mode, "argv": argv, "artifact": artifacts[0], "binary": str(destination),
                       "binary_sha256": file_hash(destination), "lock_sha256": file_hash(probe / "Cargo.lock"),
                       "feature_graph_sha256": file_hash(stage / f"{mode}-feature-graph.log"), "source_sha256": sources})
        write_json(stage / "builds.json", builds)
    require(builds[0]["lock_sha256"] == builds[1]["lock_sha256"], "time/memory dependency lock differs")
    # This original-input verifier is compiled against the same archive/lock.
    foundation.execute(["cargo", "+stable", "build", "--offline", "--locked", "--manifest-path", str(manifest),
                        "--release", "--bin", "layout-check"], stage / "layout-check-build.log", cwd=probe, env=env)
    checker = Path(env["CARGO_TARGET_DIR"]) / "release/layout-check"
    foundation.execute([checker, args.wpt, args.selection, stage / "original-layout-check.json"],
                       stage / "original-layout-check.log", cwd=probe, env=env)
    return binaries, builds


def distribution(values):
    require(values, "empty statistical sample")
    return {"n": len(values), "median": statistics.median(values), "min": min(values), "max": max(values)}


def summarize_records(records, operation, mode):
    groups = {}
    owners = set()
    for record in records:
        owners.add(record["retained_owner"])
        for name, value in windows(record, operation).items():
            if "/" in name and name.split("/", 1)[0].isdigit():
                index, name = name.split("/", 1)
                name = ("first-call/" if index == "0" else "warm/") + name
            fields = {"duration_ns": value["duration_ns"]} if mode == "time" else value["counts"]
            for metric, number in fields.items():
                groups.setdefault(name, {}).setdefault(metric, []).append(number)
    return {"windows": {name: {metric: distribution(values) for metric, values in fields.items()}
                        for name, fields in groups.items()}, "retained_owners": sorted(owners)}


def collect(args):
    selected = selection(args.selection)
    native_fallback = diagnostic_inventory(args.diagnostics)
    spike = Path(args.spike).resolve(strict=True)
    stage = Path(args.output).resolve()
    require(stage != spike and spike not in stage.parents, "measurement output cannot be inside protected spike")
    if stage.exists():
        raise FileExistsError(stage)
    stage.mkdir(parents=True)
    status = {"collection_complete": False, "original_case_count": len(selected["cases"]), "runs": [], "references": []}
    write_json(stage / "progress.json", status)
    shutil.copy2(args.selection, stage / "selection.json")
    archive_diagnostics(args.diagnostics, stage)
    selector = stage / "selection.json"
    sources = source_hashes()
    snapshots = stage / "harness-sources"
    for name in sources:
        target = snapshots / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, target)
    probe = stage / "probe"
    archive = overlay.prepare(spike, probe)
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(Path(args.target_dir).resolve()), CARGO_BUILD_JOBS="1",
               CARGO_PROFILE_RELEASE_OPT_LEVEL="3", CARGO_PROFILE_RELEASE_DEBUG="0", CARGO_INCREMENTAL="0", RUSTFLAGS="-D warnings")
    # Record the effective config before building; hidden profile/config changes
    # are neither normalized away nor silently described as the defaults.
    configuration = foundation.build_configuration(probe, env)
    binaries, builds = build(stage, probe, args, env, sources)
    metadata = {"schema": 1, "archive": archive, "source_sha256": sources,
                "harness_base_revision": foundation.execute(["git", "rev-parse", "HEAD"], stage / "harness-revision.log", cwd=ROOT).strip(),
                "input_selection_sha256": file_hash(selector), "native_diagnostic_inventory_sha256": file_hash(args.diagnostics),
                "archived_native_diagnostic_inventory_sha256": file_hash(stage / "native-diagnostic-inventory.json"),
                "wpt_revision": selected["wpt_revision"], "raikiri_revision": overlay.RAIKIRI_REVISION,
                "candidate_revision": overlay.S4_REVISION, "builds": builds, "build_configuration": configuration,
                "rustc": foundation.execute(["rustc", "+stable", "-Vv"], stage / "rustc.log").strip(),
                "cargo": foundation.execute(["cargo", "+stable", "-V"], stage / "cargo.log").strip(),
                "host": {"os": platform.platform(), "machine": platform.machine(), "logical_cpus": os.cpu_count(),
                         "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
                         "cpu_models": sorted({line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                                               if line.startswith("model name")}) if Path("/proc/cpuinfo").exists() else []},
                "repetitions": args.repetitions, "samples_per_process": 10,
                "cold_semantics": "first-call-in-process after original input/font/capability preconditioning; nine warm calls; not cold startup",
                "allocator_semantics": "requested heap bytes with operation-relative peaks; distinct memory binary; not RSS; common font/cache scopes separate",
                "operations": args.operations, "native_segmentation_exclusions": sorted(native_fallback)}
    write_json(stage / "metadata.json", metadata)
    reference_dir = stage / "references"
    reference_dir.mkdir()
    raw_dir = stage / "raw"
    raw_dir.mkdir()
    summaries = []
    for i, case in enumerate(selected["cases"]):
        for operation in args.operations:
            references = {}
            for engine in ["native", "candidate"]:
                output = reference_dir / f"{operation}-{engine}-{i:03d}.json"
                proof_operation = operation if operation == "layout" else operation + "-check"
                row = invoke(binaries["time"], "time", engine, args.wpt, selector, case, output, proof_operation)
                row.update(id=case["id"], engine=engine, operation=operation)
                status["references"].append(row)
                if row["returncode"] == 0:
                    reference = json.loads(output.read_text())
                    validate_record(reference, case, operation, "time", engine)
                    references[engine] = reference
            for mode in ["time", "memory"]:
                records = {"native": [], "candidate": []}
                for repeat in range(args.repetitions):
                    # Alternate engine order between independent process pairs.
                    for engine in (["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]):
                        if engine not in references:
                            continue
                        output = raw_dir / f"{operation}-{mode}-{engine}-{i:03d}-{repeat}.json"
                        row = invoke(binaries[mode], mode, engine, args.wpt, selector, case, output, operation)
                        row.update(id=case["id"], engine=engine, mode=mode, operation=operation, repetition=repeat)
                        status["runs"].append(row)
                        if row["returncode"] == 0:
                            value = json.loads(output.read_text())
                            validate_record(value, case, operation, mode, engine,
                                            reference=references[engine], for_comparison=True)
                            records[engine].append(value)
                row = {"id": case["id"], "operation": operation, "mode": mode, "eligible": False,
                       "successful_processes": {engine: len(values) for engine, values in records.items()}}
                if case["id"] in native_fallback:
                    row["exclusion"] = "known-native-segmentation-model-fallback; release stderr absence is not support"
                elif any(len(records[engine]) != args.repetitions for engine in records):
                    row["exclusion"] = "unsupported/failed operation, failed fresh-output proof, or native memory thread bound"
                else:
                    try:
                        validate_pair(records["native"][0], records["candidate"][0], case, operation, mode)
                        row["eligible"] = True
                    except ValueError as error:
                        row["exclusion"] = str(error)
                row["engines"] = {engine: summarize_records(values, operation, mode)
                                  for engine, values in records.items() if values}
                if row["eligible"]:
                    left = row["engines"]["native"]["windows"]
                    right = row["engines"]["candidate"]["windows"]
                    ratios = {}
                    for name in left.keys() & right.keys():
                        # Broader native initial text preparation is not pure
                        # per-run shaping; retain costs but omit that ratio.
                        if operation == "isolated" and name.endswith("initial_text_pipeline"):
                            continue
                        if "/" not in name or "release" in name or "mutable_reuse_owner_setup" in name:
                            continue
                        metrics = ["duration_ns"] if mode == "time" else ["allocated_bytes", "peak_extra_bytes"]
                        ratios[name] = {metric: right[name][metric]["median"] / left[name][metric]["median"]
                                        if left[name][metric]["median"] else None for metric in metrics}
                    row["candidate_over_native_ratios"] = ratios
                summaries.append(row)
        write_json(stage / "progress.json", status)
        if (i + 1) % 20 == 0:
            print(f"collected {i + 1}/{len(selected['cases'])} original cases", flush=True)
    require(source_hashes() == sources, "harness changed during collection")
    require(foundation.build_configuration(probe, env) == configuration, "build configuration changed during collection")
    require(file_hash(probe / "Cargo.lock") == builds[0]["lock_sha256"], "resolved dependency lock changed")
    status["collection_complete"] = True
    write_json(stage / "progress.json", status)
    report = {"schema": 1, "collection_complete": True, "metadata": metadata, "rows": summaries,
              "coverage": {operation: {mode: sum(r["eligible"] for r in summaries if r["operation"] == operation and r["mode"] == mode)
                                       for mode in ["time", "memory"]} for operation in args.operations},
              "wpt_image_verdicts": 0, "baseline_pass_delta": None,
              "original_static_candidate_pagination": "unsupported: original wrapper ignores height; provisional flow consumer is separate",
              "pure_initial_shaping_ratio": None,
              "initial_text_boundary_difference": "native all-DOM preparation/clone versus candidate leaf IFC projection; costs retained separately, no pure shaping ratio",
              "retained_memory_comparison": "engine-specific named owners; net retained bytes are not equivalent outputs and have no cross-engine ratio",
              "raw_manifest": "progress.json", "raw_manifest_sha256": file_hash(stage / "progress.json")}
    write_json(stage / "results.json", report)
    print(json.dumps({"collection_complete": True, "coverage": report["coverage"],
                      "output": report_paths.portable(str(stage), ROOT)}, ensure_ascii=False))
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--validate-record", type=Path)
    parser.add_argument("--selection", type=Path, required=True)
    parser.add_argument("--operation", choices=list(OPERATIONS), default="layout")
    parser.add_argument("--mode", choices=["time", "memory"], default="time")
    parser.add_argument("--engine", choices=["native", "candidate"], default="candidate")
    parser.add_argument("--spike", type=Path)
    parser.add_argument("--wpt", type=Path)
    parser.add_argument("--diagnostics", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target/raikiri-measurements-build")
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--operations", nargs="+", choices=list(OPERATIONS), default=list(OPERATIONS))
    args = parser.parse_args()
    if args.validate_record:
        value = json.loads(args.validate_record.read_text())
        cases = {case["id"]: case for case in selection(args.selection)["cases"]}
        validate_record(value, cases[value["id"]], args.operation, args.mode, args.engine)
        print(json.dumps({"valid": True, "id": value["id"], "operation": args.operation}))
    else:
        require(all([args.spike, args.wpt, args.diagnostics, args.output]), "collection requires spike/wpt/diagnostics/output")
        require(args.repetitions >= 2 and len(set(args.operations)) == len(args.operations), "at least two independent processes and unique operations required")
        collect(args)


if __name__ == "__main__":
    main()
