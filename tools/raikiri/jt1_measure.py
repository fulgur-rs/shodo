#!/usr/bin/env python3
"""Re-run the saved, hash-pinned release probes for the shodo-jt1 investigation.
Read-only on all saved inputs; raw outputs go to a scratch directory."""
import argparse
import hashlib
import json
import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import jt1_attribution as attribution  # noqa: E402
import report_paths  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = Path(os.environ.get("JT1_ARTIFACTS", ROOT / "target" / "8ei-artifacts"))
TIME_BINARY = ARTIFACTS / "pipeline-release" / "measurement-probe-time"
MEMORY_BINARY = ARTIFACTS / "pipeline-release" / "measurement-probe-memory"
TIME_BINARY_SHA256 = "7caf472b3d40d670c270e4a97291a05c8ecc76056fefa222e26b7be9d66d7d30"
WPT = Path(os.environ.get("RAIKIRI_WPT", Path.home() / ".cache" / "raikiri" / "wpt"))
SELECTION = ARTIFACTS / "selected-pages.json"
DOCUMENTS = [
    "css/css-text/hyphens/reference/hyphens-out-of-flow-001-ref.html",
    "css/css-text/hyphens/reference/hyphens-auto-001-ref.html",
]
OPERATIONS = ["pipeline", "layout", "isolated"]
PAIRS = 16
MAX_PINNED_CPU_BUSY = 0.25
LOADED_ABOVE = 6.0
NOTES = [
    "isolated not measured: the pinned pipeline-release time binary (SHA256 7caf472b...) reports 'unknown operation' for the isolated operation; the first (native) attempt for each document failed and the runner stopped there, so only those two failure records exist and they are kept as evidence, not skipped. A separate isolated-release binary exists but is out of scope.",
    "Load label uses the 1-minute load average sampled before and after the run only; load during the run is not observed.",
]


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def cpu_busy(seconds=1.0):
    """Busy fraction per CPU over a short window, from /proc/stat."""
    def read():
        rows = {}
        for line in Path("/proc/stat").read_text().splitlines():
            if line.startswith("cpu") and line[3].isdigit():
                fields = [int(x) for x in line.split()[1:]]
                idle = fields[3] + fields[4]
                rows[int(line.split()[0][3:])] = (sum(fields), idle)
        return rows
    before = read()
    time.sleep(seconds)
    after = read()
    return {cpu: 1 - (after[cpu][1] - before[cpu][1]) / max(1, after[cpu][0] - before[cpu][0]) for cpu in after}


def environment(cpu, busy_before):
    return {
        "affinity_cpu": cpu,
        "chosen_cpu_busy_before": busy_before[cpu],
        "loadavg_before": os.getloadavg(),
        "allowed_cpus_before_pinning": sorted(os.sched_getaffinity(0)),
    }


def run_probe(binary, mode, engine, doc, output, operation):
    argv = [str(binary), mode, engine, str(WPT), str(SELECTION), doc, str(output), operation]
    result = subprocess.run(argv, capture_output=True, text=True)
    if result.returncode != 0:
        return report_paths.portable({"argv": argv, "ok": False, "returncode": result.returncode,
                                      "stderr_tail": result.stderr[-400:]}, ROOT)
    return report_paths.portable({"argv": argv, "ok": True, "report": json.loads(Path(output).read_text())}, ROOT)


def measure_document(scratch, operation, doc, probe=None):
    """Paired native/candidate measurement for one operation/document, or an explicit failure record."""
    probe = probe or run_probe
    slug = doc.rsplit("/", 1)[1].removesuffix(".html")
    medians = {"native": [], "candidate": []}
    for repeat in range(PAIRS):
        order = ["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]
        for engine in order:
            out = scratch / f"{operation}-{slug}-r{repeat:02d}-{engine}.json"
            try:
                result = probe(TIME_BINARY, "time", engine, doc, out, operation)
                if not result["ok"]:
                    return {"failed": {"repeat": repeat, "engine": engine, "returncode": result["returncode"], "stderr_tail": result["stderr_tail"]}}
                medians[engine].append(attribution.process_median_ns(result["report"], operation))
            except (ValueError, KeyError, TypeError, IndexError) as error:
                return {"failed": {"repeat": repeat, "engine": engine, "error": f"{type(error).__name__}: {error}"}}
    return attribution.paired_summary(medians["native"], medians["candidate"])


def reproduce(scratch, output):
    if sha256(TIME_BINARY) != TIME_BINARY_SHA256:
        raise SystemExit("time binary SHA256 does not match the saved pin")
    scratch.mkdir(parents=True, exist_ok=True)
    busy = cpu_busy()
    cpu = min(busy, key=busy.get)
    if busy[cpu] > MAX_PINNED_CPU_BUSY:
        raise SystemExit(f"least busy CPU {cpu} is {busy[cpu]:.0%} busy; retry when the machine is quiet")
    env = environment(cpu, busy)
    os.sched_setaffinity(0, {cpu})
    summary = {"operations": {}}
    for operation in OPERATIONS:
        summary["operations"][operation] = {}
        for doc in DOCUMENTS:
            summary["operations"][operation][doc] = measure_document(scratch, operation, doc)
    after = cpu_busy()
    env.update({"chosen_cpu_busy_after": after[cpu], "loadavg_after": os.getloadavg()})
    env["label"] = "loaded" if max(env["loadavg_before"][0], env["loadavg_after"][0]) > LOADED_ABOVE else "quiet"
    summary["environment"] = env
    summary["binary_sha256"] = TIME_BINARY_SHA256
    summary["pairs"] = PAIRS
    summary["notes"] = NOTES
    summary["scope"] = (
        "saved candidate fe67a281 vs native raikiri ab7e619a, original parse/screen cascade/Ahem preflight/layout at 800x600, "
        "warm calls per independent process (first call is not cold startup); not current main, not WPT PASS, not a switching-necessity decision"
    )
    report_paths.write_json(output, summary, ROOT)
    return summary


MEMORY_OPERATIONS = ["pipeline", "layout"]
MEMORY_RUNS = 3
PERF_RUNS = 200
PERF_TOP = 25
PERF_COMMAND = ["perf", "record", "-F", "20000"]
PERF_REPORT = ["--stdio", "--no-children", "-g", "none", "--comm", "measurement-pro", "--fields", "sample,sym", "--sort", "sym"]
PERF_LOOP = 'for i in $(seq %d); do "$0" time "$1" "$2" "$3" "$4" /dev/null pipeline >/dev/null 2>&1 || exit 1; done' % PERF_RUNS


PERF_SCOPE = (
    "Flat perf (no call graph) over 200 whole probe processes of the pipeline operation at 20 kHz; user-space [.] symbols only, kernel [k] samples are dropped. "
    "Whole-process samples include setup outside the measured windows, so interpret only engine-to-engine differences per bucket; "
    "buckets are an approximation by the first shodo:: path or known crate in a symbol, not call-graph attribution. Counts are divided by the 200 runs."
)
# The observation notes below (PERF_NOTES, MEMORY_NOTE_PEAK) describe the committed 2026-09-30 recording: load 8.44/4.59,
# the listed noise deltas, and identical peak_extra_bytes. They must be edited or dropped when the runner is re-run, because
# they would contradict a fresh recording's own environment and deltas.
PERF_NOTES = [
    "Each engine/document comes from a SINGLE recording (200 process runs, one perf record): no repeat and no variance estimate.",
    "The recording was labelled loaded (1-minute load average 8.44 before, 4.59 after; the label in environment repeats this).",
    "Noise is large: the other bucket differs in sign between the two documents (about -95.7 per run vs +49.6) and runtime is about +1 vs +25 per run, so small deltas are not distinguishable from noise; only positives that appear on BOTH documents with large size (shodo::line, shodo::analysis, shodo::font) are candidates for interpretation.",
    "other is setup-dominated (sha2 digest, serde_json in the probe's unmeasured setup).",
    "The bucket rule assigns a symbol to the FIRST shodo:: path in it, so generic runtime/core code instantiated with shodo types (for example an Iterator::position closure over shodo::font::matching::CacheEntry) lands in a shodo bucket: buckets are symbol-name attribution, not call-graph or inclusive time.",
    "Kernel [k] rows are dropped (user space only).",
    "The perf loop exits non-zero on the first failing probe and perf() then returns an explicit failed record.",
]
MEMORY_NOTE_PEAK = "Observation, not a conclusion: peak_extra_bytes is identical for both engines in the pipeline windows and probably reflects a shared setup allocation."


def slug_of(doc):
    return doc.rsplit("/", 1)[1].removesuffix(".html")


def memory_document(scratch, operation, doc, probe=None):
    """Three independent memory-binary runs per engine (alternating order), or an explicit failure record."""
    probe = probe or run_probe
    runs = {"native": [], "candidate": []}
    for repeat in range(MEMORY_RUNS):
        order = ["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]
        for engine in order:
            out = scratch / f"memory-{operation}-{slug_of(doc)}-r{repeat}-{engine}.json"
            try:
                result = probe(MEMORY_BINARY, "memory", engine, doc, out, operation)
                if not result["ok"]:
                    return {"failed": {"repeat": repeat, "engine": engine, "returncode": result["returncode"], "stderr_tail": result["stderr_tail"]}}
                runs[engine].append(attribution.window_memory(result["report"], operation))
            except (ValueError, KeyError, TypeError, IndexError) as error:
                return {"failed": {"repeat": repeat, "engine": engine, "error": f"{type(error).__name__}: {error}"}}
    row = {"runs": runs, "median": {}, "candidate_over_native": {}}
    for engine, rows in runs.items():
        row["median"][engine] = {f: statistics.median(r[f] for r in rows) for f in attribution.MEMORY_FIELDS}
    for f in attribution.MEMORY_FIELDS:
        n, c = row["median"]["native"][f], row["median"]["candidate"][f]
        row["candidate_over_native"][f] = c / n if n else None
    return row


def memory(scratch, output):
    scratch.mkdir(parents=True, exist_ok=True)
    summary = {"operations": {}}
    for operation in MEMORY_OPERATIONS:
        summary["operations"][operation] = {doc: memory_document(scratch, operation, doc) for doc in DOCUMENTS}
    summary["binary"] = MEMORY_BINARY.name
    summary["binary_sha256"] = sha256(MEMORY_BINARY)
    summary["runs"] = MEMORY_RUNS
    summary["notes"] = [
        "Requested-heap accounting per window (allocator counters), not RSS. Values are the median over warm samples per run, then the median over runs.",
        "isolated was not attempted with the memory binary (its SHA256 is recorded, not enforced); a separate isolated-release binary exists but is out of scope.",
        MEMORY_NOTE_PEAK,
    ]
    report_paths.write_json(output, summary, ROOT)
    return summary


def perf_engine_document(scratch, engine, doc):
    """One flat perf recording of PERF_RUNS pipeline probe processes; returns normalized buckets and top symbols."""
    data = scratch / f"perf-{engine}-{slug_of(doc)}.data"
    record = PERF_COMMAND + ["-o", str(data), "--", "bash", "-c", PERF_LOOP, str(TIME_BINARY), engine, str(WPT), str(SELECTION), doc]
    try:
        done = subprocess.run(record, capture_output=True, text=True)
    except FileNotFoundError as error:
        return {"failed": {"stage": "record", "error": f"perf tool not found: {error}"}}
    if done.returncode != 0:
        # The loop exits non-zero on the first failing probe, so this also covers probe failures.
        return {"failed": {"stage": "record", "returncode": done.returncode, "stderr_tail": done.stderr[-400:]}}
    try:
        report = subprocess.run(["perf", "report", "-i", str(data)] + PERF_REPORT, capture_output=True, text=True)
    except FileNotFoundError as error:
        return {"failed": {"stage": "report", "error": f"perf tool not found: {error}"}}
    if report.returncode != 0:
        return {"failed": {"stage": "report", "returncode": report.returncode, "stderr_tail": report.stderr[-400:]}}
    (scratch / f"perf-{engine}-{slug_of(doc)}.txt").write_text(report.stdout)
    rows = attribution.perf_samples(report.stdout)
    if not rows:
        return {"failed": {"stage": "parse", "error": "no user-space sample rows in perf report"}}
    totals = attribution.aggregate(rows)
    symbols = {}
    for count, name in rows:
        symbols[name] = symbols.get(name, 0) + count
    top = sorted(symbols.items(), key=lambda kv: -kv[1])[:PERF_TOP]
    return {
        "total_samples": sum(totals.values()),
        "buckets_per_run": {k: v / PERF_RUNS for k, v in sorted(totals.items(), key=lambda kv: -kv[1])},
        "bucket_samples": totals,
        "top_symbols": [{"symbol": n, "samples": c} for n, c in top],
    }


def perf(scratch, output):
    scratch.mkdir(parents=True, exist_ok=True)
    if sha256(TIME_BINARY) != TIME_BINARY_SHA256:
        raise SystemExit("time binary SHA256 does not match the saved pin")
    busy = cpu_busy()
    cpu = min(busy, key=busy.get)
    if busy[cpu] > MAX_PINNED_CPU_BUSY:
        raise SystemExit(f"least busy CPU {cpu} is {busy[cpu]:.0%} busy; retry when the machine is quiet")
    env = environment(cpu, busy)
    os.sched_setaffinity(0, {cpu})
    summary = {"documents": {}}
    for doc in DOCUMENTS:
        engines = {}
        for engine in ("native", "candidate"):
            engines[engine] = perf_engine_document(scratch, engine, doc)
        entry = {"engines": engines}
        if all("failed" not in e for e in engines.values()):
            names = set(engines["native"]["buckets_per_run"]) | set(engines["candidate"]["buckets_per_run"])
            delta = {n: engines["candidate"]["buckets_per_run"].get(n, 0) - engines["native"]["buckets_per_run"].get(n, 0) for n in names}
            entry["candidate_minus_native_per_run"] = dict(sorted(delta.items(), key=lambda kv: -kv[1]))
        summary["documents"][doc] = entry
    after = cpu_busy()
    env.update({"chosen_cpu_busy_after": after[cpu], "loadavg_after": os.getloadavg()})
    env["label"] = "loaded" if max(env["loadavg_before"][0], env["loadavg_after"][0]) > LOADED_ABOVE else "quiet"
    summary["environment"] = env
    summary["binary_sha256"] = TIME_BINARY_SHA256
    summary["runs_per_recording"] = PERF_RUNS
    summary["sampling_hz"] = 20000
    summary["scope"] = PERF_SCOPE
    summary["notes"] = PERF_NOTES
    report_paths.write_json(output, summary, ROOT)
    return summary


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ("reproduce", "memory", "perf"):
        p = sub.add_parser(name)
        p.add_argument("--scratch", type=Path, required=True)
        p.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "reproduce":
        summary = reproduce(args.scratch, args.output)
        for operation, docs in summary["operations"].items():
            for doc, row in docs.items():
                print(operation, doc.rsplit("/", 1)[1], "FAILED" if "failed" in row else f"ratio median {row['paired_ratio_median']:.3f}")
        print("environment:", summary["environment"]["label"])
        for operation, docs in summary["operations"].items():
            for doc, row in docs.items():
                if "failed" in row:
                    print(f"not measured: {operation}/{doc.rsplit('/', 1)[1]} {json.dumps(row['failed'])}")
    elif args.command == "memory":
        summary = memory(args.scratch, args.output)
        for operation, docs in summary["operations"].items():
            for doc, row in docs.items():
                print(operation, slug_of(doc), "FAILED " + json.dumps(row["failed"]) if "failed" in row else json.dumps(row["candidate_over_native"]))
    else:
        summary = perf(args.scratch, args.output)
        for doc, entry in summary["documents"].items():
            print(slug_of(doc), {k: (e["total_samples"] if "failed" not in e else e["failed"]) for k, e in entry["engines"].items()})
            print("  delta", list(entry.get("candidate_minus_native_per_run", {}).items())[:5])
        print("environment:", summary["environment"]["label"])


if __name__ == "__main__":
    main()
