#!/usr/bin/env python3
"""Compare the nonlinear_algorithms probe's wall time, Callgrind Ir and RSS.

The two executables must use the same harness, release profile and dependencies.
Build/fixture setup is outside the reported operation wall time and Callgrind
region. Native RSS includes setup, warmup, measured work and final validation.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time

CASES = [
    "ws-0-512", "ws-32-32", "ws-128-128", "ws-512-512",
    "mixed-128-128", "ruby-1", "ruby-8", "ruby-16", "ruby-32",
]


def run_child(command: list[str], prefix: Path) -> dict:
    """wait4 observes this child only; cumulative RUSAGE_CHILDREN is unsuitable."""
    stdout = prefix.with_suffix(".stdout")
    stderr = prefix.with_suffix(".stderr")
    begin = time.perf_counter_ns()
    with stdout.open("w") as out, stderr.open("w") as err:
        child = subprocess.Popen(command, stdout=out, stderr=err)
        _, status, usage = os.wait4(child.pid, 0)
        child.returncode = os.waitstatus_to_exitcode(status)
    process_ns = time.perf_counter_ns() - begin
    if child.returncode:
        raise RuntimeError(f"{command}: exit {child.returncode}\n{stderr.read_text()[-8000:]}")
    result = json.loads(stdout.read_text())
    if result["iterations"] <= 0 or result["measured_ns"] <= 0:
        raise RuntimeError(f"invalid probe result: {result}")
    result["process_wall_ns"] = process_ns
    result["wait4_peak_rss_kib"] = usage.ru_maxrss
    return result


def summary(values: list[float]) -> dict:
    ordered = sorted(values)
    return {
        "median": statistics.median(ordered),
        "min": ordered[0], "max": ordered[-1],
        "p25": ordered[(len(ordered) - 1) // 4],
        "p75": ordered[3 * (len(ordered) - 1) // 4],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--cpu", type=int, default=2)
    parser.add_argument("--samples", type=int, default=11)
    parser.add_argument("--callgrind-samples", type=int, default=2)
    parser.add_argument("--target-ms", type=float, default=150.0)
    parser.add_argument("--case", action="append", dest="cases")
    args = parser.parse_args()
    if args.samples < 3 or args.callgrind_samples < 1 or args.target_ms <= 0:
        parser.error("need >=3 native samples, >=1 Callgrind sample and positive target-ms")
    if args.cpu not in os.sched_getaffinity(0):
        parser.error("selected CPU is unavailable")
    if any(not re.fullmatch(r"(?:ws|mixed)-\d+-\d+|ruby-\d+", case) for case in args.cases or CASES):
        parser.error("invalid case name")
    binaries = {"before": args.before.resolve(strict=True), "after": args.after.resolve(strict=True)}
    args.output.mkdir(parents=True, exist_ok=False)
    metadata = json.loads(args.metadata.read_text())
    for version, binary in binaries.items():
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        if digest != metadata["binary_sha256"][version]:
            raise RuntimeError(f"{version} executable differs from build metadata")
    metadata.update(
        cpu=args.cpu, native_samples=args.samples, callgrind_samples=args.callgrind_samples,
        target_ms=args.target_ms, platform=platform.platform(),
        started_at=datetime.now(timezone.utc).isoformat(),
        cpuinfo=Path("/proc/cpuinfo").read_text().split("\n\n", 1)[0],
        valgrind=subprocess.check_output(["valgrind", "--version"], text=True).strip(),
        runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        rss_note="Linux /proc/self/status VmHWM, native process peak including fixture/setup/validation; KiB",
        callgrind_note="Ir only; collect-atstart=no, toggle only measurement function; not wall time or RSS under Valgrind",
    )
    report = {"metadata": metadata, "cases": []}
    for case in args.cases or CASES:
        expected = None

        def invoke(version: str, count: int, label: str, callgrind: bool = False) -> dict:
            nonlocal expected
            prefix = args.output / f"{case}-{version}-{label}"
            command = ["taskset", "-c", str(args.cpu)]
            if callgrind:
                command += [
                    "valgrind", "--tool=callgrind", "--collect-atstart=no",
                    "--toggle-collect=shodo_whitespace_measure",
                    "--toggle-collect=shodo_ruby_measure", "--cache-sim=no", "--branch-sim=no",
                    f"--callgrind-out-file={prefix}.callgrind", f"--log-file={prefix}.valgrind",
                ]
            command += [str(binaries[version]), case, str(count)]
            result = run_child(command, prefix)
            if result["case"] != case or result["iterations"] != count:
                raise RuntimeError("wrong workload/iteration count")
            if expected is None:
                expected = result["output"]
            if result["output"] != expected:
                raise RuntimeError(f"output mismatch: {case}/{version}/{label}")
            if callgrind:
                profile = Path(f"{prefix}.callgrind")
                text = profile.read_text()
                match = re.search(r"^summary: (\d+)$", text, re.MULTILINE)
                symbol = "shodo_ruby_measure" if case.startswith("ruby-") else "shodo_whitespace_measure"
                if not match or int(match[1]) <= 0 or symbol not in text:
                    raise RuntimeError("Callgrind did not collect the measurement region")
                result["instructions"] = int(match[1])
                result["instructions_per_op"] = int(match[1]) / count
                result["annotation"] = subprocess.check_output(
                    ["callgrind_annotate", "--auto=no", "--threshold=90", str(profile)], text=True
                )
            return result

        pilots = {version: invoke(version, 4, "pilot") for version in binaries}
        slowest = max(p["measured_ns"] / p["iterations"] for p in pilots.values())
        calibration_count = max(4, min(1_000_000, int(20_000_000 / slowest)))
        pilots = {version: invoke(version, calibration_count, "calibration") for version in binaries}
        slowest = max(p["measured_ns"] / p["iterations"] for p in pilots.values())
        count = max(1, min(5_000_000, int(args.target_ms * 1_000_000 / slowest)))
        cg_count = max(1, min(2000, int(5_000_000 / slowest)))
        row = {
            "case": case, "iterations": count, "callgrind_iterations": cg_count,
            "native": {v: [] for v in binaries}, "callgrind": {v: [] for v in binaries},
        }
        print(f"{case}: {count} native iterations; {cg_count} Callgrind iterations", flush=True)
        for sample in range(args.samples):
            for version in (list(binaries) if sample % 2 == 0 else list(reversed(binaries))):
                row["native"][version].append(invoke(version, count, f"native-{sample}"))
        for sample in range(args.callgrind_samples):
            for version in (list(binaries) if sample % 2 == 0 else list(reversed(binaries))):
                row["callgrind"][version].append(invoke(version, cg_count, f"cg-{sample}", True))
        row["output"] = expected
        row["summary"] = {}
        for version in binaries:
            row["summary"][version] = {
                "wall_ns_per_op": summary([r["measured_ns"] / count for r in row["native"][version]]),
                "process_wall_ns": summary([r["process_wall_ns"] for r in row["native"][version]]),
                "peak_rss_kib": summary([r["peak_rss_kib"] for r in row["native"][version]]),
                "rss_kib": summary([r["rss_kib"] for r in row["native"][version]]),
                "instructions_per_op": summary([r["instructions_per_op"] for r in row["callgrind"][version]]),
            }
        pairs = [
            row["native"]["after"][i]["measured_ns"] / row["native"]["before"][i]["measured_ns"]
            for i in range(args.samples)
        ]
        row["paired_wall_ratio"] = summary(pairs)
        before, after = row["summary"]["before"], row["summary"]["after"]
        print(
            f"  wall after/before={row['paired_wall_ratio']['median']:.4f}; "
            f"Ir={after['instructions_per_op']['median'] / before['instructions_per_op']['median']:.4f}; "
            f"RSS KiB={before['peak_rss_kib']['median']} -> {after['peak_rss_kib']['median']}", flush=True,
        )
        report["cases"].append(row)
        (args.output / "results.json").write_text(json.dumps(report, indent=2))
    report["metadata"]["finished_at"] = datetime.now(timezone.utc).isoformat()
    (args.output / "results.json").write_text(json.dumps(report, indent=2))
    print(f"Saved {args.output / 'results.json'}", flush=True)


if __name__ == "__main__":
    main()
