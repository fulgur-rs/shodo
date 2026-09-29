#!/usr/bin/env python3
"""Re-run the saved, hash-pinned release probes for the shodo-jt1 investigation.
Read-only on all saved inputs; raw outputs go to a scratch directory."""
import argparse
import hashlib
import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import jt1_attribution as attribution  # noqa: E402

ARTIFACTS = Path("/home/mitz/Work/oss/shodo/target/8ei-artifacts")
TIME_BINARY = ARTIFACTS / "pipeline-release" / "measurement-probe-time"
MEMORY_BINARY = ARTIFACTS / "pipeline-release" / "measurement-probe-memory"
TIME_BINARY_SHA256 = "7caf472b3d40d670c270e4a97291a05c8ecc76056fefa222e26b7be9d66d7d30"
WPT = Path("/home/mitz/.cache/raikiri/wpt")
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
    "isolated not measured: the pinned pipeline-release time binary (SHA256 7caf472b...) does not implement the isolated operation ('unknown operation'); the failure records are kept as evidence, not skipped. A separate isolated-release binary exists but is not part of this summary.",
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
        return {"argv": argv, "ok": False, "returncode": result.returncode, "stderr_tail": result.stderr[-400:]}
    return {"argv": argv, "ok": True, "report": json.loads(Path(output).read_text())}


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
    output.write_text(json.dumps(summary, indent=2) + "\n")
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
    else:
        raise SystemExit(f"{args.command} is added by a later task")


if __name__ == "__main__":
    main()
