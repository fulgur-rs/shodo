# S4 candidate 34-36% regression: reproduction and stage attribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reproduce the warm-median increase the saved S4v2 candidate showed over native raikiri on two original hyphens reference documents, and attribute it to a stage (parse/cascade, layout, initial text pipeline, and a flat per-module profile), recording time and allocations, with the limits stated.

**Architecture:** Two small Python tools under `tools/raikiri/`: `jt1_attribution.py` (pure, unit-tested helpers over the probe's existing JSON records and `perf report` text) and `jt1_measure.py` (a runner that re-executes the already-built, hash-pinned release probe binaries under a pinned, load-checked CPU). Results are committed as a summary JSON plus a record under `docs/records/`. No product code changes and no rebuild: the saved binaries have symbols, so attribution uses a flat (no call graph) `perf record`.

**Tech Stack:** Python 3 (stdlib only, `unittest`), `perf` (flat profile), the saved probe binaries under `target/8ei-artifacts/`.

**Spec:** Design field of beads issue `shodo-jt1` (`bd show shodo-jt1`). Details fixed while planning: the saved binaries take `MODE ENGINE WPT SELECTION ID OUTPUT OPERATION` (`MODE` is `time` or `memory`, `OPERATION` is `layout`, `pipeline` or `isolated`); a time report has `samples[0]` with `state: first-call-in-process` and `samples[1..]` with `state: warm-process`; the pipeline window is `sample["parse_cascade_layout"]["duration_ns"]`, the layout window is `sample["layout"]["duration_ns"]`, the isolated window is `sample["initial_text_pipeline"]` (inspect its shape before using it). Real fixture records for tests exist in `tools/raikiri/fixtures/raikiri/` (`pipeline-{native,candidate}-time.json.gz`, memory-mode `pipeline-*.json.gz` and `layout-*.json.gz`).

## Global Constraints

- Investigation only: no changes under `crates/`, `dev/`, or to either saved S4 spike, the pinned WPT checkout, the saved `target/8ei-artifacts/` inputs, or the WPT baseline. Outputs of runs go to a scratch directory outside the repo (`~/tmp/jt1-artifacts/`, disk-backed; never `/tmp`, which is tmpfs).
- Source comments and docs are written in English.
- Pins: candidate `fe67a281210fbc52d22032911ac3584405aa8198`, raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8`, WPT `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`; time binary SHA256 `7caf472b3d40d670c270e4a97291a05c8ecc76056fefa222e26b7be9d66d7d30` (`target/8ei-artifacts/pipeline-release/measurement-probe-time`). The runner must verify the binary hash before running and refuse a mismatch.
- Documents (WPT-relative ids): `css/css-text/hyphens/reference/hyphens-out-of-flow-001-ref.html` and `css/css-text/hyphens/reference/hyphens-auto-001-ref.html`. Do not substitute simplified fixtures.
- Timing rules: 16 independent process pairs per document/operation, engine order alternating by pair, CPU affinity fixed to one CPU chosen as the least busy at start, nine warm calls per process (as the probe records). Record `/proc/loadavg` and the chosen CPU's busy fraction before and after; refuse to start if the chosen CPU is more than 25% busy; label results taken while the 1-minute load average exceeds 6 as `loaded`.
- The first call in each process is not cold startup; keep that wording.
- Claims: this measures the saved candidate against native under one preconditioned warm-caller profile. It does not attribute anything to current main, does not measure WPT PASS deltas, and does not decide switching necessity (stays undetermined; no `shodo-p2m.6` dependency).
- Perf attribution is flat (`perf record` without a call graph; the default DWARF call graph is unreliable in this environment) over whole processes, so setup outside the measured windows is included; only engine-to-engine differences per bucket are meaningful, and they are relative sample counts, not calibrated time.

## Review Focus

- A paired ratio must pair the same repeat of the same document/operation, not sorted or shuffled values: `test_pairs_are_matched_by_repeat`.
- The first call must never enter the warm median: `test_first_call_is_excluded`.
- A run with a mismatching binary hash, an unfinished/failed process, or a busy pinned CPU must fail loudly, not be dropped from the summary: `test_failed_run_is_an_error_not_a_skip`, plus the runner's refusal checks.
- The perf parser must ignore lines that are not sample rows, and buckets must be a partition (every sample counted exactly once): `test_every_sample_lands_in_exactly_one_bucket`.
- Results taken under load must be labelled `loaded` in the JSON and the record, not presented as clean.
- If the increase does not reproduce, the record says so plainly rather than explaining it away.

---

### Task 1: Analysis helpers with unit tests

**Files:**
- Create: `tools/raikiri/jt1_attribution.py`
- Create: `tools/raikiri/test_jt1_attribution.py`

**Interfaces:**
- Consumes: real records from `tools/raikiri/fixtures/raikiri/*.json.gz` (gzip JSON) in tests.
- Produces (used by Task 2): `warm_samples_ns(report, operation) -> list[int]`, `process_median_ns(report, operation) -> float`, `paired_summary(native: list[float], candidate: list[float]) -> dict`, `perf_samples(text: str) -> list[tuple[int, str]]`, `bucket(symbol: str) -> str`, `aggregate(rows) -> dict[str, int]`. `operation` is `"pipeline"`, `"layout"` or `"isolated"`.

- [ ] **Step 1: Write the failing tests**

Create `tools/raikiri/test_jt1_attribution.py`:

```python
"""Unit tests for the shodo-jt1 attribution helpers, on real probe records."""
import gzip
import importlib.util
import json
import unittest
from pathlib import Path

FIXTURES = Path(__file__).with_name("fixtures") / "raikiri"


def load_module():
    path = Path(__file__).with_name("jt1_attribution.py")
    spec = importlib.util.spec_from_file_location("jt1_attribution", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def record(name):
    return json.loads(gzip.decompress((FIXTURES / f"{name}.json.gz").read_bytes()))


class Attribution(unittest.TestCase):
    def setUp(self):
        self.m = load_module()

    def test_first_call_is_excluded(self):
        report = record("pipeline-native-time")
        warm = self.m.warm_samples_ns(report, "pipeline")
        self.assertEqual(len(warm), len(report["samples"]) - 1)
        first = report["samples"][0]["parse_cascade_layout"]["duration_ns"]
        self.assertEqual(warm, [s["parse_cascade_layout"]["duration_ns"] for s in report["samples"][1:]])
        self.assertNotIn("first-call-in-process", [s["state"] for s in report["samples"][1:]])
        self.assertIsInstance(first, int)

    def test_process_median_is_the_median_of_warm_calls(self):
        report = record("pipeline-candidate-time")
        warm = sorted(self.m.warm_samples_ns(report, "pipeline"))
        self.assertEqual(self.m.process_median_ns(report, "pipeline"), warm[len(warm) // 2] if len(warm) % 2 else (warm[len(warm) // 2 - 1] + warm[len(warm) // 2]) / 2)

    def test_layout_operation_uses_the_layout_window(self):
        report = record("layout-native")
        # memory-mode fixture: the window object exists for every sample
        self.assertTrue(all("layout" in s for s in report["samples"]))
        with self.assertRaises(ValueError):
            self.m.warm_samples_ns({"samples": [{"state": "warm-process"}]}, "layout")

    def test_wrong_state_layout_is_rejected(self):
        with self.assertRaises(ValueError):
            self.m.warm_samples_ns({"samples": [{"state": "warm-process", "layout": {"duration_ns": 1}}, {"state": "warm-process", "layout": {"duration_ns": 2}}]}, "layout")

    def test_pairs_are_matched_by_repeat(self):
        summary = self.m.paired_summary([100.0, 200.0, 300.0], [150.0, 200.0, 240.0])
        self.assertEqual(summary["paired_ratios"], [1.5, 1.0, 0.8])
        self.assertEqual(summary["pairs"], 3)
        self.assertEqual(summary["paired_ratio_median"], 1.0)
        self.assertEqual(summary["pairs_candidate_slower"], 1)
        self.assertEqual(summary["warm_process_median_ns"], {"native": 200.0, "candidate": 200.0})

    def test_unequal_or_empty_pairs_are_an_error(self):
        with self.assertRaises(ValueError):
            self.m.paired_summary([1.0], [1.0, 2.0])
        with self.assertRaises(ValueError):
            self.m.paired_summary([], [])

    def test_perf_rows_ignore_non_sample_lines(self):
        text = "\n".join([
            "# Samples: 4K of event 'cycles'",
            "#",
            "     120  [.] <parley::bidi::BidiResolver>::resolve::<X>",
            "      30  [.] shodo::paragraph::build::run",
            "       5  [k] some_kernel_symbol",
            "",
            "garbage line",
        ])
        rows = self.m.perf_samples(text)
        self.assertEqual(rows, [(120, "<parley::bidi::BidiResolver>::resolve::<X>"), (30, "shodo::paragraph::build::run")])

    def test_buckets(self):
        b = self.m.bucket
        self.assertEqual(b("shodo::line::scan::next"), "shodo::line")
        self.assertEqual(b("<parley::bidi::BidiResolver>::resolve::<X>"), "parley")
        self.assertEqual(b("raikiri_style::cascade::apply"), "raikiri_style")
        self.assertEqual(b("core::slice::sort::merge"), "runtime")
        self.assertEqual(b("malloc"), "runtime")
        self.assertEqual(b("totally_unknown_symbol"), "other")

    def test_every_sample_lands_in_exactly_one_bucket(self):
        rows = [(10, "shodo::line::a"), (7, "parley::x"), (3, "malloc"), (2, "mystery")]
        totals = self.m.aggregate(rows)
        self.assertEqual(sum(totals.values()), 22)
        self.assertEqual(totals, {"shodo::line": 10, "parley": 7, "runtime": 3, "other": 2})


if __name__ == "__main__":
    unittest.main()
```

Run: `python3 -m unittest tools.raikiri.test_jt1_attribution 2>&1 | tail -15`
Expected: FAIL/ERROR because `jt1_attribution.py` does not exist (the `assertTrue(path.is_file())` style is not used here; the `spec_from_file_location` load raises `FileNotFoundError`). That is the RED state. (If `tools` is not importable as a package, run `python3 tools/raikiri/test_jt1_attribution.py` instead.)

- [ ] **Step 2: Implement the helpers**

Create `tools/raikiri/jt1_attribution.py`:

```python
"""Pure helpers for the shodo-jt1 investigation: paired warm-median ratios over
probe time reports, and flat perf attribution by module. No I/O beyond text."""
import re
import statistics

WINDOWS = {
    "pipeline": "parse_cascade_layout",
    "layout": "layout",
    "isolated": "initial_text_pipeline",
}


def warm_samples_ns(report, operation):
    """Warm-call durations of one time-mode report. Sample 0 is the first call
    in the process (not cold startup) and is never included."""
    key = WINDOWS[operation]
    samples = report["samples"]
    if len(samples) < 2 or samples[0].get("state") != "first-call-in-process":
        raise ValueError("expected a first-call-in-process sample followed by warm samples")
    if any(s.get("state") != "warm-process" for s in samples[1:]):
        raise ValueError("samples after the first call must be warm-process")
    return [s[key]["duration_ns"] for s in samples[1:]]


def process_median_ns(report, operation):
    return statistics.median(warm_samples_ns(report, operation))


def paired_summary(native, candidate):
    """Pair values by position (the same repeat), never by sorted order."""
    if not native or len(native) != len(candidate):
        raise ValueError("native and candidate need the same non-zero number of pairs")
    ratios = [c / n for n, c in zip(native, candidate)]
    return {
        "pairs": len(ratios),
        "warm_process_median_ns": {
            "native": statistics.median(native),
            "candidate": statistics.median(candidate),
        },
        "paired_ratio_median": statistics.median(ratios),
        "paired_ratio_min": min(ratios),
        "paired_ratio_max": max(ratios),
        "pairs_candidate_slower": sum(1 for r in ratios if r > 1),
        "paired_ratios": ratios,
    }


_ROW = re.compile(r"^\s*(\d+)\s+\[\.\]\s+(.+?)\s*$")


def perf_samples(text):
    """(sample count, symbol) rows of `perf report --stdio --no-children`
    user-space entries; every other line is ignored."""
    rows = []
    for line in text.splitlines():
        match = _ROW.match(line)
        if match:
            rows.append((int(match.group(1)), match.group(2)))
    return rows


KNOWN_CRATES = [
    "parley", "harfrust", "fontique", "skrifa", "read_fonts", "raikiri_html",
    "raikiri_style", "raikiri_dom", "raikiri_traits", "cssparser", "html5ever",
    "selectors", "icu_segmenter", "icu_properties", "icu_normalizer", "icu_casemap",
    "icu_collections", "icu_provider", "smol_str", "taffy",
]
_RUNTIME = re.compile(r"\b(core|alloc|std)::|^(memcpy|memmove|memset|malloc|free|realloc|calloc|cfree|_int_\w+|__\w+)")


def bucket(symbol):
    """Coarse owner of a symbol: shodo::<module>, a known crate, runtime, or other.
    Generic instantiations attribute to the first shodo path in the symbol, so
    this is an approximation, not a call-graph attribution."""
    match = re.search(r"\bshodo::(\w+)", symbol)
    if match:
        return f"shodo::{match.group(1)}"
    for crate in KNOWN_CRATES:
        if re.search(rf"\b{crate}::", symbol):
            return crate
    if _RUNTIME.search(symbol):
        return "runtime"
    return "other"


def aggregate(rows):
    totals = {}
    for count, symbol in rows:
        name = bucket(symbol)
        totals[name] = totals.get(name, 0) + count
    return totals
```

- [ ] **Step 3: Run the tests and confirm they pass**

Run: `python3 -m unittest tools.raikiri.test_jt1_attribution 2>&1 | tail -15` (or `python3 tools/raikiri/test_jt1_attribution.py`)
Expected: `Ran 10 tests ... OK`. If `test_layout_operation_uses_the_layout_window`'s `assertTrue(all("layout" in s ...))` fails because the memory-mode window key differs, inspect `record("layout-native")["samples"][1].keys()` and use the real key in the test and in `WINDOWS`; report the deviation.

- [ ] **Step 4: Run the whole tools suite as CI does**

Run: `python3 -m unittest discover -s tools 2>&1 | tail -6`
Expected: `OK` (69 existing tests + the new ones; the existing run takes about a minute).

- [ ] **Step 5: Commit**

```bash
git add tools/raikiri/jt1_attribution.py tools/raikiri/test_jt1_attribution.py
git commit -m "test(tools): add shodo-jt1 paired-ratio and perf attribution helpers"
```

---

### Task 2: Reproduce the increase and collect stage windows

**Files:**
- Create: `tools/raikiri/jt1_measure.py`
- Create: `dev/raikiri/data/raikiri-jt1-reproduction.json` (generated summary, committed)
- Raw outputs: `~/tmp/jt1-artifacts/reproduction/` (not committed)

**Interfaces:**
- Consumes (Task 1): `jt1_attribution.process_median_ns`, `jt1_attribution.paired_summary` (import by inserting the tools/raikiri directory on `sys.path`).
- Produces: the CLI `python3 tools/raikiri/jt1_measure.py reproduce --scratch DIR --output SUMMARY.json` and, in Task 3, `... perf --scratch DIR --output SUMMARY.json`; a summary JSON with keys `environment`, `operations.<op>.<doc>` (paired summary or an explicit failure record), and `memory`.

Fixed inputs: time binary `target/8ei-artifacts/pipeline-release/measurement-probe-time`; memory binary `.../pipeline-release/measurement-probe-memory`; WPT `~/.cache/raikiri/wpt`; selection `target/8ei-artifacts/selected-pages.json`. These are read-only inputs outside the worktree.

- [ ] **Step 1: Implement the runner (reproduce subcommand)**

Create `tools/raikiri/jt1_measure.py`:

```python
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
            slug = doc.rsplit("/", 1)[1].removesuffix(".html")
            medians = {"native": [], "candidate": []}
            failure = None
            for repeat in range(PAIRS):
                order = ["native", "candidate"] if repeat % 2 == 0 else ["candidate", "native"]
                for engine in order:
                    out = scratch / f"{operation}-{slug}-r{repeat:02d}-{engine}.json"
                    result = run_probe(TIME_BINARY, "time", engine, doc, out, operation)
                    if not result["ok"]:
                        failure = {"repeat": repeat, "engine": engine, "returncode": result["returncode"], "stderr_tail": result["stderr_tail"]}
                        break
                    medians[engine].append(attribution.process_median_ns(result["report"], operation))
                if failure:
                    break
            summary["operations"][operation][doc] = (
                {"failed": failure} if failure else attribution.paired_summary(medians["native"], medians["candidate"])
            )
    after = cpu_busy()
    env.update({"chosen_cpu_busy_after": after[cpu], "loadavg_after": os.getloadavg()})
    env["label"] = "loaded" if max(env["loadavg_before"][0], env["loadavg_after"][0]) > LOADED_ABOVE else "quiet"
    summary["environment"] = env
    summary["binary_sha256"] = TIME_BINARY_SHA256
    summary["pairs"] = PAIRS
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
    else:
        raise SystemExit(f"{args.command} is added by a later task")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Smoke-test the runner against the real binary on one small run**

Run (a plain, separate command; scratch is disk-backed):

`python3 -c "import sys; sys.path.insert(0,'tools/raikiri'); import jt1_measure as m, pathlib; out=pathlib.Path('~/tmp/jt1-artifacts/smoke'); out.mkdir(parents=True,exist_ok=True); r=m.run_probe(m.TIME_BINARY,'time','native',m.DOCUMENTS[0],out/'one.json','pipeline'); print(r['ok'], m.attribution.process_median_ns(r['report'],'pipeline'))"`

Expected: `True <a number of nanoseconds near 900000>`. Then do the same for `layout` and `isolated` on both engines and print, for each, whether it succeeded and, for `isolated`, `list(report['samples'][1]['initial_text_pipeline'].keys())`. If `isolated`'s window is not `initial_text_pipeline.duration_ns` (the object may nest a `measurement` key), fix `WINDOWS["isolated"]` handling in `jt1_attribution.warm_samples_ns` (extend it to a small path tuple) with a unit test using the real fixture `isolated-native-time`; if `isolated` is unsupported for these documents (non-zero exit), keep the recorded failure as evidence. Record everything you observe in the report.

- [ ] **Step 3: Check the machine is quiet enough, then run the reproduction**

Run: `cat /proc/loadavg` and `python3 -c "import sys; sys.path.insert(0,'tools/raikiri'); import jt1_measure as m; b=m.cpu_busy(); print(sorted(b.items(), key=lambda x: x[1])[:3])"`. If the least busy CPU is over 25% busy the runner refuses on its own; in that case wait (use `sleep` in a separate command, at most a few minutes at a time) and retry up to 5 times, then report BLOCKED with the observed loads instead of lowering the threshold.

Run: `python3 tools/raikiri/jt1_measure.py reproduce --scratch ~/tmp/jt1-artifacts/reproduction --output ~/tmp/jt1-artifacts/reproduction/summary.json`
Expected: prints one `ratio median X.XXX` line per operation/document (or `FAILED`) and `environment: quiet|loaded`. This is 192 short process runs.

- [ ] **Step 4: Examine and copy the summary**

Read the summary (`python3 -c "import json; d=json.load(open('~/tmp/jt1-artifacts/reproduction/summary.json')); print(json.dumps({op:{doc.rsplit('/',1)[1]:(v.get('paired_ratio_median'), v.get('paired_ratio_min'), v.get('paired_ratio_max'), v.get('pairs_candidate_slower'), v.get('failed')) for doc,v in docs.items()} for op,docs in d['operations'].items()}, indent=1)); print(d['environment'])"`). Compare the `pipeline` medians with the saved 1.361 (out-of-flow) and 1.342 (auto). Do not adjust anything to make them agree. If the run is labelled `loaded`, note it; you may re-run once when quieter and keep both, labelled.

Copy it: `cp ~/tmp/jt1-artifacts/reproduction/summary.json dev/raikiri/data/raikiri-jt1-reproduction.json` and check it has no absolute scratch paths (`grep -c "jt1-artifacts" dev/raikiri/data/raikiri-jt1-reproduction.json` prints 0; if `argv` paths leaked in, they come only from the raw files, not the summary).

- [ ] **Step 5: Lint and commit**

Run: `python3 -m py_compile tools/raikiri/jt1_measure.py` and `python3 -m unittest discover -s tools 2>&1 | tail -4` (must stay OK).

```bash
git add tools/raikiri/jt1_measure.py dev/raikiri/data/raikiri-jt1-reproduction.json
git commit -m "feat(tools): reproduce the saved S4 candidate pipeline/layout increase on two documents"
```

---

### Task 3: Allocation windows and flat perf attribution

**Files:**
- Modify: `tools/raikiri/jt1_measure.py` (add the `memory` and `perf` subcommands)
- Create: `dev/raikiri/data/raikiri-jt1-attribution.json` (generated, committed)
- Raw outputs: `~/tmp/jt1-artifacts/attribution/` (not committed)

**Interfaces:**
- Consumes (Task 1/2): `attribution.aggregate`, `attribution.perf_samples`, `run_probe`, `MEMORY_BINARY`, `TIME_BINARY`, `DOCUMENTS`.
- Produces: `python3 tools/raikiri/jt1_measure.py memory ...` writes allocation windows per engine/document/operation; `... perf ...` writes per-bucket sample counts per engine/document and the candidate-minus-native delta.

- [ ] **Step 1: Discover the memory record shape, then write its unit test**

Run the memory binary once: `python3 -c "import sys; sys.path.insert(0,'tools/raikiri'); import jt1_measure as m, pathlib, json; out=pathlib.Path('~/tmp/jt1-artifacts/attribution'); out.mkdir(parents=True,exist_ok=True); r=m.run_probe(m.MEMORY_BINARY,'memory','native',m.DOCUMENTS[0],out/'probe.json','pipeline'); print(r['ok']); s=r['report']['samples'][1]; print(json.dumps(s['parse_cascade_layout'])[:600])"`.
Identify the per-window allocation fields (requested bytes, allocation count, operation-relative peak, retained net bytes). Add `window_memory(report, operation)` to `tools/raikiri/jt1_attribution.py` returning those numbers for the warm samples (median across warm samples per field), and a unit test in `test_jt1_attribution.py` against the real memory fixtures `pipeline-native` and `pipeline-candidate` (check exact numbers against a hand computation of one field from the fixture, not just types). Write the test first, see it fail, then implement; keep the existing tests green. Requested-heap accounting is not RSS; say so in the docstring.

- [ ] **Step 2: Add the `memory` subcommand**

For each operation in `pipeline`, `layout`, `isolated`, each document, each engine: run the memory binary 3 times (independent processes, engine order alternating), save raw outputs under the scratch directory, and write `window_memory` results (per run and their median) into the output JSON with the binary path and SHA256 (compute and record; the memory binary has no saved pin in this plan, so record its hash rather than refusing). A failed or unsupported combination is recorded as `{"failed": ...}` exactly as in Task 2. Run it: `python3 tools/raikiri/jt1_measure.py memory --scratch ~/tmp/jt1-artifacts/attribution --output ~/tmp/jt1-artifacts/attribution/memory.json` (no timing is taken, so machine load does not matter here).

- [ ] **Step 3: Add the `perf` subcommand and run it**

For each engine and document: run the time binary's `pipeline` operation 200 times under one `perf record`, flat (no `-g`, no `--call-graph`):
`perf record -F 20000 -o <scratch>/perf-<engine>-<slug>.data -- bash -c 'for i in $(seq 200); do "$0" time "$1" "$2" "$3" "$4" /dev/null pipeline >/dev/null 2>&1; done' <binary> <engine> <wpt> <selection> <doc>` — verify that the probe accepts `/dev/null` as OUTPUT; if it does not, write to a scratch file that is overwritten each iteration. Then `perf report -i <data> --stdio --no-children --comm measurement-probe-time --sort sym` and parse with `attribution.perf_samples` / `attribution.aggregate`. Check `perf_event_paranoid` allows it (it is 2: user-space profiling of your own processes works). Pin to the least-busy CPU as in Task 2 and record the environment label. Normalize by dividing bucket counts by the 200 runs, give per-engine bucket tables and `candidate_minus_native` per bucket, plus each engine's total sample count. Because whole processes are sampled, the setup outside the measured windows is included: say so in the JSON `scope` field and only interpret differences. Also save the top 25 symbols per engine/document (name and samples) so a reader can see what a bucket contains. Run it: `python3 tools/raikiri/jt1_measure.py perf --scratch ~/tmp/jt1-artifacts/attribution --output ~/tmp/jt1-artifacts/attribution/perf.json`.

- [ ] **Step 4: Combine into one committed summary**

Write `dev/raikiri/data/raikiri-jt1-attribution.json` as `{"memory": <memory.json content>, "perf": <perf.json content>, "scope": "..."}` (a small Python one-liner is fine; keep field names). Check `grep -c "/home/" dev/raikiri/data/raikiri-jt1-attribution.json` and remove any absolute scratch path from the JSON (keep binary basenames and hashes). Report the headline: per operation the candidate/native allocation ratios, and the top buckets of `candidate_minus_native` for each document.

- [ ] **Step 5: Verify and commit**

Run `python3 -m unittest discover -s tools 2>&1 | tail -4` (OK) and `python3 -m py_compile tools/raikiri/jt1_measure.py`.

```bash
git add tools/raikiri/jt1_attribution.py tools/raikiri/test_jt1_attribution.py tools/raikiri/jt1_measure.py dev/raikiri/data/raikiri-jt1-attribution.json
git commit -m "feat(tools): attribute the S4 candidate increase by allocation window and flat perf module"
```

---

### Task 4: Record the findings and limits

**Files:**
- Create: `docs/records/raikiri-jt1-regression.md`
- Modify: `docs/records/raikiri-measurements.md` (the last paragraph that says the 34-36% observation "is tracked in `shodo-jt1`")
- Modify: `docs/README.md` (add a Records row next to the raikiri-measurements entry)

**Interfaces:**
- Consumes: `dev/raikiri/data/raikiri-jt1-reproduction.json`, `dev/raikiri/data/raikiri-jt1-attribution.json`, the saved manifest ratios (1.361 / 1.342). Quote real numbers from the JSON files only.
- Produces: documentation only.

- [ ] **Step 1: Write `docs/records/raikiri-jt1-regression.md`**

English. Read the two data files first; every number must come from them. Sections:

1. **Question and scope**: the saved candidate `fe67a281` vs native `ab7e619a` on two original documents; warm-caller profile (parse, screen cascade, Ahem preflight, layout at 800x600, caches preconditioned, first call is not cold startup); what this is not (current main, WPT PASS, switching necessity, cold startup).
2. **Reproduction**: the exact command, pins, binary SHA256, CPU pinning, environment label and load before/after; the table of paired ratios (median, min, max, pairs slower) for `pipeline`, `layout`, `isolated` per document, against the saved 1.361 / 1.342; state plainly whether the increase reproduced, and any `loaded` label or failed operation.
3. **Where the time goes**: what the `pipeline` vs `layout` vs `isolated` ratios say about parse/cascade (same raikiri crates on both engines) vs layout vs the initial text pipeline; the allocation ratios per operation; the flat perf bucket table and `candidate_minus_native` top buckets with the caveats (whole-process samples include setup, buckets are an approximation by symbol name, generic instantiations attribute to the first shodo path). State the input-scope difference (native prepares all DOM text, candidate projects the paired leaf IFCs) and the retained-output owner difference as facts that limit comparability.
4. **What stays undetermined**: switching necessity (no acceptable budget defined; latest candidate on current main not evaluated), attribution to current main, the split between old-S4-only and current differences. Follow-ups: a candidate caller on current main (acceptance 3) and a budget-based necessity decision (acceptance 4); no `shodo-p2m.6` dependency is added.
5. **Reproduce**: the three commands (`reproduce`, `memory`, `perf`), where raw outputs go, and that the saved inputs live under `target/8ei-artifacts/` and are local, not in CI (CI runs only the unit tests of the helpers).

- [ ] **Step 2: Update `raikiri-measurements.md` and `docs/README.md`**

Replace the sentence that ends "...tracked in `shodo-jt1`; it is not attributed to current main." so it also links to the new record (keep the original meaning and the `shodo-bqz` sentence intact). Add one Records row to `docs/README.md` in the neighbouring style: `| Reproduction and stage attribution of the saved S4 candidate increase | [raikiri jt1 regression](records/raikiri-jt1-regression.md) |`.

- [ ] **Step 3: Verify and commit**

Run `python3 -m unittest discover -s tools 2>&1 | tail -4` (OK) and `git status --short` (only the three doc files changed).

```bash
git add docs/records/raikiri-jt1-regression.md docs/records/raikiri-measurements.md docs/README.md
git commit -m "docs: record the reproduction and stage attribution of the S4 candidate increase"
```
