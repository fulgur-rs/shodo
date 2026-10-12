# SHODO-6: bounded Balance trial summaries

## Purpose and acceptance

Balance currently streams completed Lines to retain only their mapped ends.
Avoid constructing completed Lines for trial inputs whose post-scan effects
can be proved equivalent without geometry. Keep public line formatting and
Pretty on their existing complete path. This is a guarded optimization, with
explicit full formatting for every unsupported case.

The issue requires identical mapped breaks, final recursive geometry, ordered
warnings and saturation counts, first-line remapping, ruby extents, float
progress/block offsets, and reshape/ruby budgets. Measure actual construction
counts, requested allocation traffic/retention/peaks and counter-free time for
narrow/wide ordinary, small, ruby, combined/vertical, styled and float cases.
Adopt only measured beneficial work; do not promise a general speedup.

Baseline is `cc3aa5b6b3844c995c92831fa4c114ca03873d61`. Public APIs, dependencies,
MSRV 1.89.0 and default resource limits stay unchanged. Existing summaries in
`docs/records/balance-summary.md` remove simultaneous retention, not construction.
The conditional proposal in `balance-trial-geometry.md` remains the contract.

## Chosen approach

Use a private typed finalizer for the existing line-formatting state machine.
Full formatting returns the existing `LineResult`. Balance trial formatting
returns a private `TrialResult`, whose accepted result owns only a copied
`BreakToken` and the exact public `f32` block size. All non-line results retain
the same information and progress rules. Public callers continue through the
full finalizer; no public enum or callback changes.

The generic core preserves validation, completed/partial-cache handling,
scan selection, reshape preparation, ruby measuring, spacing, punctuation,
float discovery/withdrawal, whitespace and alignment. Immediately before
`Line::new`, the trial finalizer may summarize an eligible prepared scan.
Otherwise it performs the unchanged complete post-scan work and reduces its
result to a trial value after observing all effects.

First-line formatting remains the existing complete alternate-data path,
including normal-cursor remapping. Its result is reduced only after remapping.
Initial implementation treats a paragraph with first-line data as fallback
throughout. Ruby, floats and combined text likewise retain full formatting.

Alternatives considered: extracting all geometry into a new Frame would move
construction without necessarily reducing work; replacing the trial with the
existing scalar metric query would repeat edge shaping and change budgets and
effects. Both are broader and less defensible than a guarded finalizer.

## Private boundaries

- `line/trial.rs`: `TrialSummary { token: BreakToken, block_size: f32 }`,
  `TrialResult` and the full/trial finalizers. `from_full(LineResult)` preserves
  non-line values and drops fallback Lines after extracting their two values.
  Eligibility and summarization have no shaper calls or warning writes.
- `line/mod.rs`: shared formatting core with a private finalizer type;
  existing full entry points keep their signatures. Trial entry validates the
  same token and uses `Retention::LastLine`.
- `line/iter.rs`: Balance-only fixed-width trial driver. Reuse one helper for
  accumulated block offset arithmetic in both drivers.
- `line/trial/tests.rs`: reference comparisons and constructor/budget probes.
- `dev/bench/examples/balance_summary.rs`: reuse its fixed-font snapshots,
  independent plan/accepted windows and separate counting/time builds.

The shared offset helper must preserve the original round trip exactly:
convert previous offset and the public `f32` height to LayoutUnit, add with
Saturation, convert back, then record saturation. Directly adding stored raw
metric units changes rounding/overflow behavior near the fixed-point boundary.

## Initial summary eligibility

The trial finalizer is entered after all shared prefix effects. Summarize only
when all of these are true; otherwise fall back:

1. Ordinary horizontal writing; no first-line data, line-height quirk, ruby,
   floats, inline boxes, composition spans, or text emphasis.
2. Selected units are nonempty ordinary glyph-bearing clusters, without
   parent boxes, combination or shared-cluster slicing. They use the root
   style, baseline vertical alignment and horizontal shaped-run orientation.
   Controls, tabs, forced breaks, atomics and out-of-flow units fall back.
3. No prepared overlays, leading adjustments, justification positions,
   glyph-spacing output or ruby payload require materialization. A rejected
   eligibility check must not restart shaping or reset its budgets.
4. Saturation is clean, warning checkpoint is available and unchanged, and
   all omitted geometry arithmetic is provably within safe numeric bounds.
   Require finite root size/metrics and selected record profiles of magnitude
   at most 4096 px. Require nonnegative advances with total at most 2^20 px and
   origin magnitude at most 2^20 px. Reject any profile group or own group.
   These bounds keep discarded coordinate sums and annotation arithmetic far
   below Q26.6 saturation; they are conservative eligibility limits, not new
   user-facing limits. Outliers use the complete path and preserve warnings.

Compute root and selected glyph extents using the existing ProfileResolver
and font metrics, without retaining FragmentRecords. Adjacent equal run/item
profiles may reuse the same profile only where the eligibility conditions
prove profile independence from glyph/text ranges. Apply the existing extent
union, nonnegative height, ceil block size and rounded baseline semantics;
empty or ambiguous cases fall back. Validate summary numeric conversions in a
local Saturation and fall back if they are not clean. A failed probe must not
change the caller's saturation or ordered warnings.

This guard intentionally excludes many ordinary lines with prepared edge
overlays. Coverage and actual fallback counts must be measured; broaden it
only with a demonstrated effect-equivalence contract, not to improve a table.

## Comparison and failure gates

Use an independent reference that drives existing public completed lines at
fixed widths. Compare mapped endpoints, accepted block offsets, complete
final geometry and ordered warning values, not only line counts. Preserve
complete scans of infeasible trials; no early stop after exceeding the greedy
count. Preserve binary-search widths/iterations, default 16/None fallback 32,
zero/one iteration behavior and all tail warning effects.

Prove eligible real-font/synthetic-TTF text trials have zero completed-Line
constructions while fallback classes retain constructors. Compare fresh and
warm contexts, disabled/tiny reshape budgets, suppressed warnings, first-line
transform/cursor maps, ruby/combined/vertical/styled/float fixtures, forced and
block boundaries, atomic generation changes and saturated numeric inputs.
Reuse existing budget/geometry regression suites and add tests where this
specific trial branch creates a new risk. Public Full and Pretty controls
must retain their behavior and measured cost bounds.

## Performance and publication

Use fixed fixture fonts, exact input/selection digests and task-owned builds
under `~/tmp`. Separate no-counter timing from allocation/construction
diagnostics. Preserve all samples and conditions before cleaning task files.
Measure plan and accepted formatting separately, since trial changes do not
promise cheaper accepted geometry. Include normal short/wide controls as well
as large/narrow Balance, and report fallback-heavy workloads explicitly.

If the guard saves construction but does not provide a worthwhile measured
benefit, publish the diagnosis and rejected candidate rather than enabling
it. Final records must state the supported classes, retained full geometry,
numeric proof assumptions, tests, tradeoffs and reproduction commands. Finish
with independent review, all required CI, merge, Plane completion and cleanup
under the existing authorized issue workflow.
