# raikiri integration measurements

These tools measure the saved S4v2 candidate at
`fe67a281210fbc52d22032911ac3584405aa8198` against native raikiri at
`ab7e619a8f321f03de8b8c8b9342954868e044c8`. They do not substitute current
shodo main or merge the integration spike. WPT inputs are pinned to
`97ea26e26a2aac3eec7e770650b25e7049ed4a4e`; the original ordered registry has
88 bundled fonts. The comparison uses original HTML, CSS and resources.

The [saved evidence](data/raikiri-measurements.json) includes both the whole-caller
and initial library collections. The library collection completed 1,446 of
1,452 process attempts; its 723 successful memory records balance all measured
owners across 71,052 allocation windows. Both modes have 97 eligible documents
after known native model diagnostics and the sequential observation bound are
excluded. Raw/source/binary/lock/reference hashes and independent full-output
matches are audited. Results retain the unsupported and incompatible operations
described below; they do not establish production-switch readiness.

## Reproduce

The input selection is the original S4 audit's 121 documents for which both
whole-page painting paths completed at 800×600, with 332 candidate leaf blocks.
This classification is an input restriction, not 121 passing WPT tests.
The selection records resource bytes, parser warnings, font hashes and the
original candidate geometry. All 121 selected records exactly match the saved
700-document audit (SHA-256
`67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d`).
The original `expectations/raikiri-baseline.txt` is read from the immutable
raikiri commit above, with SHA-256
`56154e4748a14a4762f1d25cf05f1c009e63151cba726fffd31dc2ebb6be0d65`;
a subsequently changed shared baseline is not substituted.
The native diagnostic inventory records actual
debug-build segmentation-model warnings; release disables that fallback log,
so release stderr alone cannot establish supported native shaping.

Supply the preserved S4 ignored `Cargo.lock` in the source checkout. Its SHA
must be `d41af8a11a1799cd990fd256b9f11e3592ecd07a787ed50e529d4f52374c1d71`.
Source is read from the exact immutable commit using git objects, even if a
shared checkout has since advanced. The recipe checks the original page SHA,
captures the current checkout's HEAD/status/diff, and preserves that checkout
and lock. It rejects an output directory inside the source checkout.

```bash
python dev/bench/tools/raikiri_measure.py \
  --spike /path/to/saved-s4-v2 \
  --wpt /path/to/pinned-wpt \
  --selection /path/to/selected-pages.json \
  --diagnostics /path/to/native-diagnostic-inventory.json \
  --output /path/to/new-measurement-directory \
  --target-dir /path/to/cargo-build-cache \
  --repetitions 3
```

The diagnostic inventory's `source` field must locate its original log;
`source_sha256`, message counts, phase and document set are checked. Cargo builds
offline with the installed `+stable` toolchain. The new output directory retains
archive/probe source, the original and resolved locks, compiler logs, actual
Cargo artifacts and feature graphs, executable hashes, host/configuration
metadata, exact argv, raw records and failure logs. Failed collection does not
delete its evidence; use a new output directory for another attempt.

The collector first validates all selected documents against the original
layout geometry. It then obtains fresh-output references for each engine and
operation before collecting independent processes. Every process records one
first call and nine warm calls. Engine order alternates between process pairs.
The first call follows font/input/capability setup; it is **not cold startup**.

Initial library construction uses a separate archive and collector. Supply the
pinned raikiri checkout and the independently verified `isolated` references
from the whole-caller collection above:

```bash
python dev/bench/tools/raikiri_library_measure.py \
  --spike /path/to/saved-s4-v2 \
  --raikiri /path/to/pinned-raikiri \
  --wpt /path/to/pinned-wpt \
  --selection /path/to/selected-pages.json \
  --diagnostics /path/to/native-diagnostic-inventory.json \
  --references /path/to/whole-caller-collection/references \
  --output /path/to/new-library-measurement-directory \
  --target-dir /path/to/cargo-build-cache \
  --repetitions 3
```

This collector shares the existing build, feature/profile validation and
original-input layout verification. It saves the original native diagnostic
log, source hashes, separate release binaries, reference records and every
process result. Missing references or failed processes remain exclusions.

## Boundaries

| Operation | Measured work | Retained output |
| --- | --- | --- |
| `layout` | Actual native `BaselineLayout` or the original candidate layout pass with raster/snapshot work removed; parse/cascade outside the window | Native laid-out DOM clone and atomic map; candidate block geometry |
| `pipeline` | Actual resource parse, screen cascade, supported Ahem declaration and complete layout; same-DOM reentry at widths 400, 1200, restored 800 | Parsed/cascaded DOM/resources and each engine's actual layout output |
| `isolated` | Fresh text preparation/shaping/initial lines; separately measured mutable reuse setup; retained shapes rebroken at original root widths ×1, ×0.5, ×1.5, ×1; actual height-zero rejection/checkpoint replay | Candidate prepared IFCs/context/lines; native DOM layouts plus mutable copies for reuse |
| `pagination` | Real native `layout_page_fragments` or a separately labelled provisional consumer using original candidate CSS inputs and preserved `FlowDriver::paginate`; heights 128, 64, 32, restored 128 | Native DOM and page-fragment snapshots; candidate prepared IFCs, accepted page lines, context, full flow checkpoints/traces |
| `initial-library-construction-phases` | Actual candidate `ParagraphBuilder::build` or native sequential Parley `ranged_builder`, default-style API calls and `build`; exclusive DOM/style preparation and initial line breaking stay separately measured | Real prepared IFCs/context/lines or native laid-out DOM; output, input geometry and shared font/cache release have named windows |

Both native and candidate initial preparation actually shape text. Native's
isolated public boundary includes all original DOM text and an unshaped DOM
clone; candidate projects the paired leaf IFCs. Native DOM layouts have no
public mutable accessor, so copying already shaped layouts for reuse is a
distinct setup window. Those copies are never reported as initial shaping.
The report retains initial costs separately and omits a pure per-run shaping
ratio. Retained-width costs are also labelled as original paired-root workloads,
not a guarantee of identical text-node/IFC granularity or WPT output.

Library observations are inserted only into guarded disposable source archives;
upstream algorithms and the saved spikes are unchanged. Every sample builds
fresh shapes. Full source/glyph output must equal the independent same-engine
isolated reference. Raw candidate builder text and native caller-processed job
strings retain their own byte lengths and digests. Native jobs outside the
paired leaf roots remain visible and measured. These API boundaries and input
sets differ, so the report publishes per-engine construction costs and no pure
glyph-shaping or retained-output ratio.

Observer hashing, JSON and result storage run between counted windows. Thread
local dispatch at their boundaries remains included; these are observed API
costs rather than instruction-level shaping costs. Per-call allocation peaks
are kept individually, never summed into a claimed combined peak. The native
observer covers the actual sequential job branch, so both time and memory modes
reject inputs with 32 or more original text nodes.

Native consumes its `FontContext`; its clone is allocated inside each consuming
measurement window. Candidate configured font caches survive page-output
release. Common `WptFonts` setup, geometry/capability preconditioning, transient
output release and final font-registry release have separate windows. The
memory validator requires all measured owner deltas to balance after release.
Net retained bytes name different output owners and have no cross-engine ratio.

Time binaries have no counting allocator. Memory binaries count requested heap
allocation, deallocation, operation-relative peak and retained net bytes;
these are not RSS. The actual Cargo artifact must report release optimization
level 3, debug assertions off and the expected instrumentation feature. Native
memory runs conservatively reject original documents with 32 or more text nodes
because the public pre-shape job scheduler's single-thread bound is not proved
for them.

## Pagination limits and exclusions

The saved static candidate wrapper ignores viewport height and cannot paginate.
The provisional flow consumer is a new caller around preserved public APIs;
it is recorded separately. It borrows the original CSS sizing/preflight prefix,
which matches all 121 original documents' root IDs, widths and edges. It keeps
positive margins and rejects leaf padding/borders, negative margins, nonpositive
content widths, page styles/insets and forced/avoided box breaks that it cannot
represent. Unsatisfiable widow/orphan limits remain failures.

The first full memory correctness inventory attempted both engines on all 121
documents. Candidate completed 19, native 41; all 60 successful owner lifecycles
balanced. Both engines completed 19 inputs, but none had both exact content
widths and page/line partitions in agreement across the height schedule.
For an auto-width root, candidate used 784px while native paged layout used
800px. For the original four-line `hyphens-none-shy-on-2nd-line-001-ref.html`,
candidate used approximately 160.000305px and retained the UA body's top margin;
at 32px height its two-line fragments occupied pages 1 and 2. Native used
160px and pages 0 and 1. Native's public pagination contract also leaves
oversized IFC widow/orphan handling outside its block-flow pass.

These outputs cannot establish a page-processing speedup. The report keeps
actual per-engine costs and explicit failures, and omits incompatible ratios.
Candidate rejection counts come from actual flow traces; native's public API
does not expose checkpoint retries, so those counts remain unavailable.
The caller contract difference is tracked in `shodo-tmr`, dependent on the S4
validation issue. Switching necessity is still undetermined.

## Validation and interpretation

```bash
python -m unittest discover -s dev/bench/tools -v

# Optional integration contract on the original four-line WPT reference:
python dev/bench/tools/check_raikiri_pagination.py \
  /path/to/measurement-probe-memory memory /path/to/wpt \
  /path/to/selected-pages.json /path/to/new-contract-output
```

The validator rejects changed input/font/resource/warning/viewport data,
instrumented timing, missing measurement or release windows, unbalanced owners,
shortened accepted source, lost fragments, failed restoration, changed fresh
source/glyph output and incompatible paged partitions. Tests mutate unchanged
real collected records; compressed fixture JSON is correctness evidence, not
additional benchmark samples. Rust probe templates are external optional tools,
so normal workspace Cargo checks do not compile their archived dependencies.

`results.json` contains distributions and qualified pair counts. Its
`collection_complete` means the declared collection finished; unsupported and
failed operations remain excluded. It does not mean complete CSS support,
equivalent native/candidate output, a WPT PASS or a production-switch decision.
No WPT image reftest is performed here; pinned baseline PASS delta remains
unknown. The broader initial native/candidate text preparation boundaries are
documented rather than converted into a pure shaping ratio.

Exploratory earlier samples reproduced a 34–36% increase for the saved candidate
in two original hyphens reference documents, with 16 independent alternating
process pairs on one CPU. That observation is tracked in `shodo-jt1`; it is not
attributed to current main. Native segmentation-model configuration is tracked
in `shodo-bqz`. Both followups require S4 validation before a switching-necessity
decision, and do not currently block the excluded production-switch issue.
