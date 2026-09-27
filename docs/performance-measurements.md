# Standalone performance measurements

`dev/bench` measures shodo with the checked-in fixture fonts and text. It is a
development workspace package; the library's normal dependencies are unchanged.
Run it from the repository with Rust stable (at least 1.89) and Python 3:

```sh
python3 dev/bench/tools/run.py --output /tmp/shodo-full --quick
python3 dev/bench/tools/run.py --output /tmp/shodo-latin-before --quick --case latin-short
python3 dev/bench/tools/run.py --output /tmp/shodo-latin-after --quick --case latin-short --baseline /tmp/shodo-latin-before
```

Each output directory must be new. Failed commands, invalid output or incompatible
baselines fail the run before publishing results. Remove `--quick` for Criterion's
default 100 samples, 3-second warmup and 5-second measurement per operation.
Quick runs use 10 samples, 100-millisecond warmup and 200-millisecond measurement;
actual sampling may take longer for expensive workloads. `--cold-samples N`
selects at least two independent processes per workload (default three).
The runner preserves an existing `Cargo.lock`, or resolves one when absent in a
fresh checkout, then uses locked builds and saves the resolved lock with results.

The full matrix has 54 workloads: the 12 fixture corpus cases and six structural
variants, each at scales 1, 8 and 64. It includes Latin, Japanese, Arabic, combining
characters and mixed scripts; many separate short paragraphs; four nested inline
boxes and a 20×20 atomic with baseline 16; preserved spaces and tabs; two float
retry anchors; justification; and fallback from a Latin-first family to the fixed
CJK/Arabic faces. System font discovery is disabled. Input text, style, widths,
paragraph counts and structural settings are stored in each report.
Measurements use the real font shaper with `complex-scripts` enabled. Synthetic
font output or missing fixed-face glyphs aborts the run; stub shaping timings are
not used as performance evidence. The fixtures do not establish full Unicode
or arbitrary web-font coverage.

Seven warm operations are timed separately: paragraph build, first `next_line`,
all lines, intrinsic sizes, three widths with paragraph reuse, the same widths
with paragraph rebuild, and height rejection followed by same-token retry.
The widths are the case width, half that width and 1.5 times that width. The
height trial uses zero available height. Float and page cases measure the public
shodo protocol; they do not represent a complete browser BFC or pagination engine.

Build includes input insertion, whitespace processing, bidi and shaping. Source
strings, styles and caller atomic maps are prepared outside measurement. Layout
operations reuse a prebuilt paragraph and warmed context; rebuild includes build
and layout. The small loop that collects lines and follows retries is included.
Criterion uses bounded `LargeInput` batches to exclude setup, output destruction
and digest validation from its timed interval. Validation runs on every retained
output after the interval, increasing total wall time and potentially affecting
adaptive sampling and cache state. No rasterization is timed. Source ranges,
geometry, glyph IDs/clusters/positions, fixed font identities and output counts
are hashed, and incomplete or different output fails instead of appearing faster.

Cold timing uses a separate executable without allocator instrumentation. Each
new process parses its workload before timing context initialization, fixture font
initialization/registration, first build and first complete layout. Parent process
wall time includes startup, JSON output and logging, and is recorded separately.
It must not be interpreted as an engine-only duration. Raw phases and all cold
samples are retained; one invocation is insufficient to establish a speedup.

Memory runs use a separate executable with an optional global `System` allocator
wrapper. Successful allocations and reallocations count calls and requested bytes;
successful realloc counts the new size as allocated and the old size as freed.
Failed realloc leaves the old allocation and counters intact. Each nonnested
scope reports gross allocated/freed bytes, starting/current live bytes, additional
peak and signed retained net. Negative net is valid when releasing prior owners.
The scopes retain and release actual fonts, context, paragraphs and lines, compare
independently warmed Start and Justify output, measure width reuse/rebuild and
page retry, then shrink/drop the context and fonts. Normalization reports named
paragraph/context bytes per painted glyph and line/context bytes per line, with
no division by zero. These are requested allocator blocks, excluding stack,
RSS, mmap, native malloc overhead and temporary native storage during realloc.
Allocator callbacks never format, allocate, lock or unwind.

The runner saves both probe executables before rebuilding with different features.
`results.json` contains raw Criterion samples and literal per-iteration medians,
cold phases/process wall samples, memory counters and output digests. The output
also contains `metadata.json`, the resolved `Cargo.lock`, raw Criterion files,
individual probe JSON, command logs and saved binaries. Metadata records Git
revision/dirty state, source and harness hashes, fixed font/input/lock hashes,
verbose compiler version, Cargo version, CPU/OS, build flags/features/profile and
measurement settings. Builds select only this package, complex-scripts enabled,
release optimization 3, debug information 0 and incremental compilation disabled.

Baseline comparison requires identical machine/toolchain/build conditions,
configuration, selected inputs, harness/lock/font hashes and operation output.
Engine revision may differ. `comparison.json` reports warm median time ratios,
cold-phase/process-wall median ratios and scope net/peak byte deltas. A zero cold
baseline median produces a null ratio. It imposes no percentage threshold. Scheduling,
frequency scaling, thermal state and shared caches still affect timings even on
one machine; repeat isolated runs before attributing changes to code.

Normal CI exercises workload/protocol tests, allocator accounting, process probes,
one Criterion functional smoke case and Python validators. The manual
`Performance artifacts` workflow runs release quick measurements and uploads
the complete output directory; it does not gate on wall-time regressions.
GitHub runners are unsuitable as interchangeable performance baselines.

This harness measures standalone shodo. Raikiri integration and whole-page WPT
coverage remain separate. Historical debug measurements in
`shaping-measurements.md` retain their original scope.

The initial full run is summarized in `dev/bench/results/initial.json`: 54
workloads, 378 warm operation samples and 162 cold/memory process outputs.
It used Rust 1.97.1, an AMD Ryzen 5 5600G, release optimization 3 and quick mode
with two cold processes per workload. The engine revision was `beef4f8`; the
runner was uncommitted, with its exact harness and source hashes recorded in the
metadata. The summary records the SHA256 of the complete raw report and its
local archive path, outside the disposable worktree. It is diagnostic evidence,
not a performance golden or a cross-machine baseline.

Selected warm medians in microseconds, with scales 1 / 8 / 64:

| Case | Build | All lines |
| --- | --- | --- |
| latin-short | 82.4 / 592.0 / 4552.7 | 31.1 / 219.1 / 1729.2 |
| japanese-short | 87.7 / 566.3 / 4308.9 | 42.5 / 269.1 / 2254.7 |
| arabic-short | 133.0 / 781.0 / 6200.2 | 224.6 / 6019.3 / 52302.1 |

A separate repeated `latin-short` run at all three scales compared successfully
with the first representative run using identical recorded conditions. Warm
ratios ranged from approximately 0.96 to 1.88 and all scope net byte deltas were
zero. Both runs use the same engine revision; this checks baseline comparison
and illustrates measurement variation, without claiming a code improvement.
Full raw outputs are retained under the root checkout's
`target/performance-artifacts/{full-final,latin-final-before,latin-final-after}`.
