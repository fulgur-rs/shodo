# SHODO-6 Balance trial summary implementation

**Spec:** `docs/superpowers/specs/2026-10-12-shodo-6-balance-trial-summary-design.md`
**Execution:** inline with executing-plans; existing issue workflow authorizes PR, CI, merge and cleanup.
**Baseline:** cc3aa5b6b3844c995c92831fa4c114ca03873d61

## Global constraints

Preserve public APIs, Full/Pretty behavior, ordered warnings, saturation counts,
mapped ends, reshape/ruby budgets and complete recursive accepted geometry.
Only summarize bounded, effect-equivalent prepared scans; unsupported inputs
retain completed Lines. Do not stop infeasible trials early. Keep the public
height-to-LayoutUnit round trip when accumulating block offsets. Do not repeat
shaping to obtain metrics. Use task-owned builds under `~/tmp`; record results
before cleanup. No runtime adoption without a measured benefit and controls.

## Review focus

- Numeric proof: omitted fragment, glyph and annotation calculations must not
  hide saturation, including unusual font metrics and cumulative coordinates.
- Selected glyph membership: hidden soft hyphens, shared clusters, empty glyph
  ranges, bidi reordering and prepared overlays may invalidate scalar extents.
- Fallback progress: first-line cursor remapping, floats, block boundaries and
  previous block offsets must match the full driver.
- Cache and budgets: summary success must not retain geometry or reset budgets;
  rejection must not change warnings or state before full finalization.
- Public path cost and performance claims: validate counter-free controls,
  complete snapshots and observed coverage, including fallback-heavy inputs.

### Task 1: typed trial driver with complete fallback

**Files:** `crates/shodo/src/line/{mod.rs,iter.rs,trial.rs,trial/tests.rs}`.
**Interfaces:** Produces private `TrialSummary { token: BreakToken, block_size:
f32 }`, a typed trial result, static Full/Trial finalizers and
`Paragraph::fixed_width_trials` yielding summaries. Existing full methods keep
their signatures; a shared generic core performs every existing effect.
Consumes the current complete formatting state machine and Retention::LastLine.

1. Add `trial_driver_matches_completed_text_lines` using a registered fixture
   font and raw ParagraphBuilder text at several widths. Independently drive
   public completed lines, compare copied tokens, exact public heights and
   ordered warnings with the private trial driver.
2. Run `cargo +stable test -p shodo trial_driver_matches_completed_text_lines`.
   Expected: RED because the private trial driver does not yet exist.
3. Add typed finalizers; initially Trial always reduces completed LineResult.
   Factor shared validation/core, preserving full first-line remapping before
   trial reduction. Add fixed-width trial iteration with float cursor and
   block progress. Share the exact existing offset arithmetic with Lines.
4. Run the new test. Expected: GREEN with identical values and warning order.
5. Add mixed float/block/forced and first-line comparison coverage, using
   existing fixture helpers where practical. Verify those tests and existing
   Balance summary tests. Expected: all pass; atomics still construct 64 Lines.
6. Commit the typed driver. Complete via task-done with
   `cargo +stable test -p shodo line::`. Expected: all line tests pass.

### Task 2: guarded prepared-scan summaries and Balance integration

**Files:** Task 1 files plus existing metrics helpers if needed.
**Interfaces:** Consumes Task 1 finalizers/driver and existing prepared Scan,
ProfileResolver and font metrics. Produces a pure optional TrialSummary before
Line::new; Balance break_ends consumes trial tokens. Full finalizer always
materializes. Failed probes have no observable effects.

1. Add `balance_eligible_text_does_not_construct_trial_lines` with real
   synthetic-TTF glyphs, raw root text, max_balance_iterations=1 and registered
   fixed metrics. Compare public reference breaks/heights and assert zero
   completed-Line constructors during planning.
2. Run that test. Expected: RED from nonzero completed-Line constructors.
3. Implement the spec's conservative guards. Explicitly reject hidden soft
   hyphens, empty/shared glyph ranges and ambiguous membership. Reuse existing
   record extents without collecting records, shifts or groups. Validate all
   omitted numeric work within the stated bounds. Route only Balance ends
   through the trial driver; leave Full/Pretty on their full methods.
4. Run the constructor test and comparison tests. Expected: GREEN; eligible
   trials construct zero Lines and copy exact heights/tokens.
5. Add guard boundaries and fresh/warm budget/warning tests; compare fallback
   first-line, ruby, combined/vertical, styled, float and numeric saturation
   behavior against public complete formatting. Run relevant existing budget
   and geometry suites. Expected: no changed effects or final geometry.
6. Commit guarded summaries. Complete via task-done with
   `cargo +stable test -p shodo`. Expected: full crate suite passes.

### Task 3: measure, decide, record and verify

**Files:** `dev/bench/examples/balance_summary.rs`, benchmark support as needed,
`docs/records/shodo-6-balance-trial-summary-2026-10-12.{md,json}`,
`docs/records/README.md`.
**Interfaces:** Consumes the optimized branch and baseline plus fixed-font
benchmark snapshots. Produces reproducible raw samples, construction/fallback
counts, allocation diagnostics, counter-free times and adoption decision.

1. Extend existing fixed-font benchmark coverage with raw root text, combined,
   vertical and complex styles if absent. Assert complete snapshots and ordered
   warnings match baseline for every measured case. Expected: identical output.
2. Build separate counter-free timing and allocation/constructor diagnostics.
   Use balanced baseline/candidate order and controlled CPU affinity, record
   all build flags, fonts and samples. Measure narrow/wide/short plain text,
   ruby, combined/vertical, style, floats and public Start/Pretty controls;
   separate plan and accepted formatting. Expected: measured coverage and
   honest ratios, including full-fallback costs and retained-byte effects.
3. Decide from results. If not worthwhile, remove the production candidate
   while preserving tested diagnostic code and complete reproduction evidence.
   Record any such change as a ruling. Expected: final runtime matches decision.
4. Run workspace tests, formatting, required Clippy and allocation-feature
   checks for final changed targets. Expected: all pass. Commit records and
   final implementation; task-done runs the final workspace test command.
5. Generate whole-branch review package; request fresh independent review on
   the most capable available model. Resolve Important/Critical once with
   reproducing RED/GREEN tests, ledger every declined judgment or deferred
   minor. Expected: reviewed final branch satisfies effect and adoption gates.
6. Create PR, verify required CI on reviewed HEAD and merge under existing
   authorization. Record results/link in Plane and mark Done, then remove
   task worktrees/build scratch after preserving ledger rulings in records.
   Expected: merged PR, fresh Done readback and task-only cleanup.
