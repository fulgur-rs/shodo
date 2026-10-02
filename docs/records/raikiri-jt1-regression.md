# raikiri S4 candidate increase: reproduction and stage attribution (shodo-jt1)

This record reproduces the 34-36% pipeline increase that
[raikiri measurements](raikiri-measurements.md) observed for the saved S4
candidate, and attributes it by measurement window. It is an investigation
record, not a switching decision.

## Question and scope

Question: does the saved candidate `fe67a281` still cost more than native
raikiri `ab7e619a` on the two original hyphens reference documents
(`hyphens-out-of-flow-001-ref.html`, `hyphens-auto-001-ref.html`), and in which
stage does the extra time sit?

The measured profile is one warm-caller profile: parse, screen cascade, Ahem
preflight and layout at 800x600, with caches preconditioned by earlier calls in
the same process. The first call of each process is not cold startup and is not
part of the reported medians.

This record does not cover current main, WPT PASS/FAIL, page paint, cold
startup, or whether switching is necessary. Those stay undetermined (see
"What stays undetermined").

## Reproduction

Data: [`raikiri-jt1-reproduction.json`](../../dev/raikiri/data/raikiri-jt1-reproduction.json),
produced by `tools/raikiri/jt1_measure.py reproduce`.

- Inputs: the saved, hash-pinned release time probe (SHA256
  `7caf472b3d40d670c270e4a97291a05c8ecc76056fefa222e26b7be9d66d7d30`) and the
  saved page selection. All inputs are read-only.
- Method: 16 alternating native/candidate pairs, each call in an independent
  process, pinned to one CPU (CPU 0). The reported value is the median of the
  per-pair candidate/native ratio of the warm-process median time.
- Environment label `quiet`: 1-minute load average 2.58 before and 2.97 after
  the run (load during the run is not observed); the pinned CPU was 2.0% busy
  before and 3.0% after.

Paired candidate/native ratios (16 pairs each):

| Operation | Document | Median | Min | Max | Pairs candidate slower | Saved ratio |
| --- | --- | --- | --- | --- | --- | --- |
| `pipeline` | out-of-flow | 1.370 | 0.953 | 1.660 | 15/16 | 1.361 |
| `pipeline` | auto | 1.363 | 1.311 | 1.536 | 16/16 | 1.342 |
| `layout` | out-of-flow | 3.002 | 1.787 | 3.121 | 16/16 | - |
| `layout` | auto | 3.411 | 3.271 | 5.174 | 16/16 | - |
| `isolated` | both | not measured | | | | - |

Warm-process median times (native / candidate): `pipeline` 925 us / 1263 us
(out-of-flow) and 813 us / 1104 us (auto); `layout` 162 us / 483 us and
118 us / 397 us.

The saved pipeline increase reproduced: 1.370 and 1.363 against the saved 1.361
and 1.342. The saved values come from the saved manifest
`target/8ei-artifacts/pipeline-regression-repro/manifest.json` (local, not in
CI), whose saved spreads were 1.329..1.382 (out-of-flow) and 1.333..1.372
(auto). The spread of this reproduction is wider despite the `quiet` label
(out-of-flow `pipeline` 0.953..1.660, auto 1.311..1.536, `layout` auto up to
5.174), while the medians reproduce. The one low pair (0.953) for out-of-flow
`pipeline` does not move the median.

`isolated` was not measured. The pinned time binary reports `unknown operation`
for `isolated`; the first (native) attempt for each document failed and the
runner stopped there, and those two failure records are kept in the JSON as
evidence. `isolated` was not attempted with the memory binary. A separate
isolated-release binary exists but is out of scope, so no isolated ratio is
presented here, fast or slow.

## Where the time goes

Pipeline versus layout. The `pipeline` window contains parse, cascade and
preflight, which run the same raikiri crates on both engines, plus layout.
Using the warm medians above, the candidate minus native difference is about
338 us (out-of-flow) and 291 us (auto) for `pipeline`, and about 321 us and
279 us for the `layout` window. About 95% and 96% of the pipeline increase
therefore lies inside the layout window, and the remainder (parse, cascade,
preflight and anything else outside layout) differs by only about 17 us and
11 us. This is a derived observation from separately measured operations in
separate processes, so it is approximate. It is consistent with a layout-window
effect, not a parse or cascade effect. The large `layout` ratios (about 3.0 and
3.4) reflect that layout is a small share of the total `pipeline` time.

Allocations. Data:
[`raikiri-jt1-attribution.json`](../../dev/raikiri/data/raikiri-jt1-attribution.json),
`memory` section, produced by `jt1_measure.py memory` (requested-heap allocator
counters per window, not RSS; median of the warm samples per run, then the
median of 3 runs). Candidate over native:

| Window | Document | Allocated bytes | Allocation calls | Operation-relative peak | Retained net bytes |
| --- | --- | --- | --- | --- | --- |
| `pipeline` | out-of-flow | 1.19 | 1.94 | 1.00 | 0.88 |
| `pipeline` | auto | 1.22 | 1.88 | 1.00 | 0.84 |
| `layout` | out-of-flow | 1.53 | 3.99 | 0.36 | 0.008 |
| `layout` | auto | 1.70 | 4.81 | 0.89 | 0.007 |

In the layout window the candidate allocates more bytes and makes four to five
times as many allocation calls, while retaining almost nothing (net 704 and 352
bytes, against 85648 and 49632 for native). Peak extra bytes are identical for
both engines in the `pipeline` window; this probably reflects a shared setup
allocation and is an observation, not a conclusion. The allocation counts fit a
layout-window increase but do not by themselves say what the extra time is.

Flat perf by symbol bucket. Data: `perf` section of the attribution JSON,
produced by `jt1_measure.py perf`. Limits that apply to every perf number here:

- Each engine/document is a single recording of 200 process runs at 20 kHz, with
  no repeat and no variance estimate.
- The recording was taken while the machine was labelled `loaded` (load average
  8.44 before, 4.59 after; the pinned CPU was 4.9% busy before and 1.0% after).
- It is flat (no call graph), whole-process and user space only: setup outside
  the measured windows is included (the `other` bucket is dominated by setup
  such as SHA-256 hashing and `serde_json`), and kernel samples are dropped.
- Buckets are symbol-name attribution: a symbol goes to the first `shodo::` path
  in its name, so generic runtime code instantiated with shodo types lands in a
  shodo bucket. This is not inclusive time.
- That all 200 probe processes of each recording ran was checked after the fact
  from per-process sample counts (200 probe processes per recording, none with a
  tiny sample count). It is inferred from sample counts, not from probe exit
  codes: the committed recordings predate the exit-status check that was added
  afterwards.

Candidate minus native, samples per run, largest shodo positives (all
positive on both documents):

| Bucket | out-of-flow | auto |
| --- | --- | --- |
| `shodo::line` | +98.1 | +79.7 |
| `shodo::analysis` | +55.8 | +50.8 |
| `shodo::font` | +48.2 | +42.7 |
| `shodo::shape` (next) | +26.0 | +13.6 |

Native has about zero samples in every `shodo::*` bucket (all `shodo::*`
buckets together: 1.68 samples per run on out-of-flow and 0.88 on auto, against
262.1 and 200.2 for the candidate). So these positives are the candidate's whole
cost in modules that native never runs, not extra time inside shared code.
Within `shodo::`, line, analysis and font are the largest buckets on both
documents, with shape next and also positive on both.

The other side of the comparison is the native text stack. The sum of the
native layout-side crates (`parley`, `fontique`, `raikiri_dom`, `taffy`,
`icu_segmenter`, `harfrust`, `skrifa`, `read_fonts`) drops from 158.1 to 71.8
samples per run on out-of-flow and from 81.3 to 35.7 on auto. For the three
text-crate buckets alone, the candidate-minus-native change is -105.0
(`parley` -51.8, `raikiri_dom` -38.2, `fontique` -15.0) on out-of-flow and -52.3
(`parley` -30.5, `raikiri_dom` -14.7, `fontique` -7.2) on auto. That is the work
the candidate replaced. The perf data therefore compares two different
implementations of the same layout work; it does not locate a hotspot inside
shared code.

The remaining buckets are treated as noise. In particular `other` (-95.7 versus
+49.6 per run) and `runtime` (+1.0 versus +25.1) change sign or size between
the documents. None of this locates the cost more finely than a module.

Comparability limits, from the Boundaries section of
[raikiri measurements](raikiri-measurements.md):

- Input scope differs. Native prepares all original DOM text, while the candidate
  projects the paired leaf IFCs, so the two engines do not process an identical
  set of text nodes.
- The retained-output owner differs. Native retains its laid-out DOM and atomic
  map, and the candidate retains block geometry, so retention and release costs
  are not like for like.

Ratios and deltas here are therefore comparisons of two callers' actual
workloads, not of a pure layout algorithm.

## What stays undetermined

- Whether switching is necessary. No acceptable budget has been defined, so the
  size of the increase cannot yet be judged against one.
- Attribution to current main. Follow-up issue `shodo-7dt` measured the saved
  S4 candidate and a caller built from current main; see
  [the current-main comparison](raikiri-jt1-current-main.md). The remaining
  current-main gap is not a switching decision or a function-level attribution.
- Behavior outside this profile: cold startup, page paint and WPT results.
- Isolated text pipeline cost (not measured, see above).
- Finer attribution inside the layout window than the module level.

Follow-ups:

- The current-main caller comparison (acceptance 3 of `shodo-jt1`) is recorded
  in [the follow-up report](raikiri-jt1-current-main.md).
- A budget-based switching-necessity decision remains open (acceptance 4).
- Optional: a quiet re-recording of the perf attribution; and, if the layout
  window needs finer attribution, a frame-pointer rebuild in a disposable
  archive, or the `isolated` operation with the separate isolated-release binary.

## Reproduce

The saved inputs live under `target/8ei-artifacts/` (binaries and the page
selection) and the WPT checkout in the raikiri cache; they are local and are not
in CI. CI runs only the unit tests of the helpers
(`python3 -m unittest discover -s tools`). Raw per-run outputs go to the
`--scratch` directory and are not committed; the summaries below are.

```sh
python3 tools/raikiri/jt1_measure.py reproduce --scratch <scratch-dir> \
  --output dev/raikiri/data/raikiri-jt1-reproduction.json
python3 tools/raikiri/jt1_measure.py memory --scratch <scratch-dir> \
  --output <memory-summary.json>
python3 tools/raikiri/jt1_measure.py perf --scratch <scratch-dir> \
  --output <perf-summary.json>
```

`reproduce` prints an explicit `not measured` line for a failed operation.
`memory` and `perf` write separate summaries; the committed
`raikiri-jt1-attribution.json` was assembled from the two as `memory`, `perf`
and `scope`. That combining step, and the perf "verified after the fact"
per-process note in the JSON, were added outside the runner. `perf` needs the
`perf` tool and records at 20 kHz. A re-run replaces the environment labels, so
check that `reproduce` and `perf` report `quiet`. A quiet re-recording is a
fresh recording, not a confirmation of the numbers above, and the observation
notes in the runner describe the committed recording (see the comment above
`PERF_NOTES`).
