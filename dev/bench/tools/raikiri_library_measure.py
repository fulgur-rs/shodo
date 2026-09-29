#!/usr/bin/env python3
"""Fresh original library construction, separate from whole-caller measurements."""
import argparse
import json
import os
import platform
import shutil
import subprocess
from pathlib import Path

import importlib.util

HERE = Path(__file__).resolve().parent


def sibling(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


measurement = sibling("raikiri_measure")
recipe = sibling("raikiri_library_overlay")
require = measurement.require
file_hash = measurement.file_hash
write_json = measurement.write_json


def windows(value):
    result = {name: value[name] for name in ["font_setup", "input_geometry", "release_input_geometry", "release_font_registry"]}
    for index, sample in enumerate(value["samples"]):
        phases = sample["exclusive_phases"]
        require(len(phases) >= 3 and len(phases) % 2 == 1, "missing exclusive preparation/library phases")
        for n, phase in enumerate(phases):
            require(phase["kind"] == ("preparation" if n % 2 == 0 else "library"), "overlapping/missing library boundary")
            result[f"{index}/phase/{n}"] = phase["measurement"]
        result[f"{index}/release_prepared_owner"] = sample["release_prepared_owner"]
    return result


def validate(value, case, mode, engine, reference):
    try:
        require(value["schema"] == 1 and value["operation"] == "initial-library-construction-phases", "wrong library operation/schema")
        require(value["id"] == case["id"] and value["mode"] == mode and value["engine"] == engine, "wrong case/mode/engine")
        require(type(value["instrumented"]) is bool and value["instrumented"] == (mode == "memory"), "instrumented time or wrong memory build")
        require(value["viewport_css_px"] == [800, 600] and value["font_sha256"] == case["font_sha256"]
                and value["resources"] == case["resources"] and value["parse_warnings"] == case["parse_warnings"], "original input drift")
        for field in ["scope", "library_input_semantics", "allocation_semantics", "observer_overhead", "preconditioning"]:
            require(isinstance(value[field], str) and value[field], "missing original library boundary/ownership/limitations")
        require(value["pure_glyph_shaping_ratio"] is None, "different library APIs/inputs cannot establish a pure shaping ratio")
        measurement.validate_record(reference, case, "isolated", "time", engine)
        require(reference.get("fresh_shape_outputs_verified") is True, "missing independent fresh-shape proof")
        require(value["initial_output"] == reference["initial_output"], "library output differs from independent original source/glyph proof")
        roots = {b["node"] for b in case["candidate_page"]["blocks"]}
        measurement.source_rows(value["initial_output"], engine, roots)
        require(len(value["samples"]) == 10, "missing actual cold/warm repetitions")
        expected = None
        for index, sample in enumerate(value["samples"]):
            require(sample["state"] == ("first-call-in-process" if index == 0 else "warm-process"), "wrong cold/warm state")
            require(sample["output"] == value["initial_output"], "fresh construction source/glyph differs")
            tags = []
            for phase in sample["exclusive_phases"]:
                if phase["kind"] != "library":
                    continue
                measurement.numeric(phase["node"], integer=True)
                measurement.numeric(phase["input_bytes"], integer=True)
                digest = phase["input_sha256"]
                require(isinstance(digest, str) and len(digest) == 64 and all(c in "0123456789abcdef" for c in digest), "invalid actual library input digest")
                require(phase["paired_root"] is None or phase["paired_root"] in roots, "wrong original paired root")
                tags.append((phase["node"], phase["paired_root"], phase["input_bytes"], digest))
            require(len(tags) == len({r[0] for r in tags}), "duplicated actual library job")
            pairs = {(r[0], r[1]) for r in tags if r[1] is not None}
            required = {(r["root"] if engine == "candidate" else r["text_node"], r["root"]) for r in value["initial_output"]}
            require(required <= pairs and {r[1] for r in pairs} == roots, "real original paired library construction missing")
            require(expected is None or tags == expected, "cold/warm library source/order differs")
            expected = tags
        net = 0
        for window in windows(value).values():
            if mode == "time":
                require(set(window) == {"duration_ns"}, "allocator instrumentation in a timing window")
                measurement.numeric(window["duration_ns"], integer=True)
            else:
                require(set(window) == {"counts"}, "missing real allocation window")
                counts = window["counts"]
                require(set(counts) == {"calls", "allocated_bytes", "deallocated_bytes", "start_live_bytes", "live_bytes", "peak_extra_bytes", "net_bytes"}, "missing real counts")
                for name, count in counts.items():
                    measurement.numeric(count, integer=True, signed=name == "net_bytes")
                require(counts["net_bytes"] == counts["allocated_bytes"] - counts["deallocated_bytes"]
                        == counts["live_bytes"] - counts["start_live_bytes"], "wrong allocator owner arithmetic")
                require(counts["peak_extra_bytes"] >= max(0, counts["net_bytes"]), "peak misses retained bytes")
                net += counts["net_bytes"]
        require(mode != "memory" or net == 0, "unbalanced actual library/preparation/cache owners")
    except (KeyError, IndexError, TypeError) as error:
        raise ValueError(f"missing or invalid library measurement: {error}") from error


def summarize(records, mode):
    groups = {}
    for record in records:
        for index, sample in enumerate(record["samples"]):
            state = "first-call" if index == 0 else "warm"
            for n, row in enumerate(sample["exclusive_phases"]):
                # Per-call peaks are retained individually, never summed into a
                # fabricated peak for disjoint windows or different owners.
                label = f'{state}/{row["kind"]}/{row["node"] if row["kind"] == "library" else n}'
                fields = row["measurement"] if mode == "time" else row["measurement"]["counts"]
                for metric, number in fields.items():
                    groups.setdefault(label, {}).setdefault(metric, []).append(number)
    return {name: {metric: measurement.distribution(values) for metric, values in fields.items()}
            for name, fields in groups.items()}


def invoke(binary, mode, engine, args, selector, case, output):
    argv = [str(binary), mode, engine, str(args.wpt), str(selector), case["id"], str(output)]
    run = subprocess.run(argv, capture_output=True, text=True)
    log = output.with_suffix(".log")
    log.write_text(run.stdout + run.stderr)
    row = {"id": case["id"], "mode": mode, "engine": engine, "argv": argv, "returncode": run.returncode,
           "log": str(log), "log_sha256": file_hash(log), "output": str(output), "diagnostic": run.stderr[-1200:]}
    if run.returncode == 0:
        require(output.is_file(), "successful library process produced no output")
        row["output_sha256"] = file_hash(output)
    else:
        require(not output.exists(), "failed library process left successful output")
    return row


def collect(args):
    selected = measurement.selection(args.selection)
    fallback = measurement.diagnostic_inventory(args.diagnostics)
    stage = args.output.resolve()
    for source in [args.spike, args.raikiri, args.wpt, args.references]:
        source = source.resolve(strict=True)
        require(stage != source and source not in stage.parents, "output cannot be inside a protected source/input directory")
    stage.mkdir(parents=True, exist_ok=False)
    status = {"collection_complete": False, "runs": [], "original_case_count": len(selected["cases"])}
    write_json(stage / "progress.json", status)
    shutil.copy2(args.selection, stage / "selection.json")
    selector = stage / "selection.json"
    inventory = json.loads(args.diagnostics.read_text())
    shutil.copy2(inventory["source"], stage / "original-native-diagnostics.log")
    inventory["original_source"] = inventory["source"]
    inventory["source"] = str(stage / "original-native-diagnostics.log")
    write_json(stage / "native-diagnostic-inventory.json", inventory)
    sources = measurement.source_hashes()
    for name in ["raikiri_library_measure.py", "raikiri_library_overlay.py"]:
        p = HERE / name
        sources[str(p.relative_to(measurement.ROOT))] = file_hash(p)
    for name in sources:
        dest = stage / "harness-sources" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(measurement.ROOT / name, dest)
    probe = stage / "probe"
    archive = recipe.prepare_library(args.spike, args.raikiri, probe)
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(args.target_dir.resolve()), CARGO_BUILD_JOBS="1", CARGO_INCREMENTAL="0",
               CARGO_PROFILE_RELEASE_OPT_LEVEL="3", CARGO_PROFILE_RELEASE_DEBUG="0", RUSTFLAGS="-D warnings")
    configuration = measurement.foundation.build_configuration(probe, env)
    binaries, builds = measurement.build(stage, probe, args, env, sources, binary_name="library-probe")
    metadata = {"schema": 1, "archive": archive, "builds": builds, "source_sha256": sources,
                "build_configuration": configuration, "selection_sha256": file_hash(selector),
                "diagnostic_inventory_sha256": file_hash(stage / "native-diagnostic-inventory.json"),
                "rustc": measurement.foundation.execute(["rustc", "+stable", "-Vv"], stage / "rustc.log").strip(),
                "cargo": measurement.foundation.execute(["cargo", "+stable", "-V"], stage / "cargo.log").strip(),
                "host": {"os": platform.platform(), "machine": platform.machine(), "logical_cpus": os.cpu_count(),
                         "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None},
                "repetitions": args.repetitions, "samples_per_process": 10,
                "boundary": "actual library calls; fresh original preparation outside each library window; no common glyph-only ratio",
                "reference_directory": str(args.references.resolve()), "references": []}
    write_json(stage / "metadata.json", metadata)
    (stage / "references").mkdir()
    (stage / "raw").mkdir()
    summaries = []
    for i, case in enumerate(selected["cases"]):
        references = {}
        for engine in ["native", "candidate"]:
            source = args.references / f"isolated-{engine}-{i:03d}.json"
            if not source.exists():
                continue
            reference = json.loads(source.read_text())
            measurement.validate_record(reference, case, "isolated", "time", engine)
            require(reference.get("fresh_shape_outputs_verified") is True, "missing real fresh-shape proof")
            dest = stage / "references" / source.name
            shutil.copy2(source, dest)
            metadata["references"].append({"id": case["id"], "engine": engine, "original": str(source), "saved": str(dest), "sha256": file_hash(dest)})
            references[engine] = reference
        for mode in ["time", "memory"]:
            records = {"native": [], "candidate": []}
            for repeat in range(args.repetitions):
                for engine in (["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]):
                    if engine not in references:
                        continue
                    output = stage / "raw" / f"library-{mode}-{engine}-{i:03d}-{repeat}.json"
                    row = invoke(binaries[mode], mode, engine, args, selector, case, output)
                    row["repetition"] = repeat
                    status["runs"].append(row)
                    if row["returncode"] == 0:
                        value = json.loads(output.read_text())
                        validate(value, case, mode, engine, references[engine])
                        records[engine].append(value)
            success = {e: len(v) for e, v in records.items()}
            eligible = case["id"] not in fallback and all(v == args.repetitions for v in success.values())
            summaries.append({"id": case["id"], "mode": mode, "eligible_per_engine_construction_costs": eligible,
                              "successful_processes": success, "exclusion": None if eligible else "native model diagnostic, sequential bound, failed process or missing proof",
                              "engines": {e: summarize(v, mode) for e, v in records.items() if v},
                              "pure_glyph_shaping_ratio": None, "cross_engine_retained_ratio": None})
        write_json(stage / "progress.json", status)
        write_json(stage / "metadata.json", metadata)
        if (i + 1) % 20 == 0:
            print(f'library construction: {i + 1}/{len(selected["cases"])} original cases', flush=True)
    require(all(file_hash(measurement.ROOT / name) == digest for name, digest in sources.items()), "harness changed during library collection")
    require(measurement.foundation.build_configuration(probe, env) == configuration, "build configuration changed")
    require(file_hash(probe / "Cargo.lock") == builds[0]["lock_sha256"], "resolved library lock changed")
    status["collection_complete"] = True
    write_json(stage / "progress.json", status)
    result = {"schema": 1, "collection_complete": True, "metadata": metadata, "rows": summaries,
              "coverage": {mode: sum(r["eligible_per_engine_construction_costs"] for r in summaries if r["mode"] == mode) for mode in ["time", "memory"]},
              "pure_glyph_shaping_ratio": None, "raw_manifest_sha256": file_hash(stage / "progress.json"),
              "wpt_image_verdicts": 0, "baseline_pass_delta": None}
    write_json(stage / "results.json", result)
    print(json.dumps({"collection_complete": True, "coverage": result["coverage"], "output": str(stage)}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["spike", "raikiri", "wpt", "selection", "diagnostics", "references", "output", "target-dir"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    args = parser.parse_args()
    require(args.repetitions >= 2, "at least two independent process repetitions required")
    collect(args)


if __name__ == "__main__":
    main()
