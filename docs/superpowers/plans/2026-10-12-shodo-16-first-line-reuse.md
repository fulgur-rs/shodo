# SHODO-16 First-Line Reuse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan inline, task by task. Steps use checkbox (`- [ ]`) syntax for tracking. One independent whole-branch reviewer at the end; no implementer subagents.

**Goal:** Remove repeated analysis and shaping for paint-only first-line styles while retaining exact layout effects and limits.

**Architecture:** Keep both ordinary paragraph datasets and finalizers. Guard private analysis/shape reuse against style, warning, saturation, callback, Ruby and budget differences. Copy retained data rather than introduce public paint overlays.

**Tech Stack:** Rust workspace, existing fixed OFL fonts, Cargo tests/Clippy, existing allocation probe and recursive public-output snapshot helper.

**Spec:** `docs/superpowers/specs/2026-10-12-shodo-16-first-line-reuse-design.md`

## Global Constraints

- No public API, dependencies, MSRV or runtime switch is added; Rust floor remains 1.89.
- Every non-paint style field and inherited float/variation bit must match.
- Preserve style/text/item/glyph accounting, precise limit errors, source mappings, ordered warnings and saturation effects.
- Ruby aggregate/base scopes, callbacks, unsafe warnings/numeric values and insufficient bounds use the original path.
- Keep both finalizations and complete shape-changing alternate data; do not infer a maximum first-line width at build time.
- SHODO-13/PR #300 is held by the user. Do not merge it or base this branch on it.
- Scratch uses `~/tmp`; preserve reproducible records before removing this task's own artifacts.

## Review Focus

1. Style equality may hide signed-zero variation coordinates; exposed run values must remain exact. Task 2 pins the distinct-bit case.
2. Tiny remaining text/item/glyph bounds can produce prefix-based errors; do not replace them with a full-size synthetic error. Tasks 1/2 compare exact failures.
3. Warning suppression can make a dirty pass look clean; zero/one caps and numeric saturation must use fallback. Tasks 1/2 compare ordered effects.
4. Cross-node ligatures, transforms, RTL and vertical/composed data must preserve first-line cursor remapping and paint. Tasks 1/2 compare these classes.
5. Callback invocation and Ruby aggregate accounting are observable even when styles match. Tasks 1/2 retain and prove fallback.
6. Automatic combined-width trials can contain more glyphs than selected output. Task 3's real GSUB regression pins exact finite-cap errors; selected automatic width features use full shaping under a finite alternate glyph cap, with unlimited-cap reuse still proved.

## Task 1: Reuse paint-only context-free analysis

**Files:** create `crates/shodo/src/paragraph/build/paint_reuse.rs` and `paint_reuse_tests.rs`; modify `paragraph/build.rs`, `analysis/whitespace.rs`, `analysis/transform.rs`, `analysis/breaks.rs`, `analysis/bidi.rs`.

**Interfaces:**
- Consumes: existing `PreparedBuild`, sanitized paired `InlineStyle` arrays, `WarningSink::checkpoint`, alternate remaining `Limits`.
- Produces: private `paint_reuse::styles_match(normal: &[InlineStyle], alternate: &[InlineStyle]) -> bool`; `paint_reuse::enabled() -> bool` (constant true in production, thread-local reference switch only in tests); `BuildAnalysis` records whether alternate analysis reused normal data.
- Task 2 consumes that reuse decision and the identical non-paint style guard.

- [ ] Step 1: Add `paint_only_analysis_uses_one_pass`, with an actual test-only `analyze_data` call counter and real `ParagraphBuilder::analyze` input. Expected: baseline fails because it performs 2 passes instead of 1.
- [ ] Step 2: Run `cargo +stable test -p shodo paint_only_analysis_uses_one_pass`; read the 2 vs 1 failure. Expected: RED assertion, not a compile failure.
- [ ] Step 3: Implement narrowly guarded copying of `Processed`, combine ranges, breaks and bidi analysis; derive private Clone only where copying requires it. Keep original callbacks/scoped budgets and insufficient text/item processing on full fallback. Expected: GREEN work counter.
- [ ] Step 4: Add/run `reused_analysis_matches_reference_for_identical_transforms_and_style_maps` and `analysis_fallback_preserves_callbacks_warnings_and_limit_errors`. A test helper restores the reference-mode thread-local state even on panic. Compare mapping/break/bidi/style data, exact error fields and callback counts. Expected: all equal, pure-paint analysis 1 pass, fallback 2.
- [ ] Step 5: `cargo +stable fmt --all`; run `cargo +stable test -p shodo`. Expected: all tests pass; no unexplained failures.
- [ ] Step 6: Commit `perf: reuse paint-only first-line analysis`. Run task-done with the same crate suite. Expected: complete ledger line with successful suite.

## Task 2: Reuse selected shaping without changing finalization

**Files:** modify `paragraph/build.rs`, `build/paint_reuse.rs`, `build/paint_reuse_tests.rs`, `shape/features.rs`.

**Interfaces:**
- Consumes: Task 1's reusable-analysis decision and `styles_match`; normal `ParagraphData` before finalization; existing matching/Harfrust counters.
- Produces: private optional shaped-data input to `build_data`, cloning selected shape items, glyph store, runs and feature arrays; ordinary unit construction/finalization/cursors unchanged.
- Task 3 consumes the resulting public output and real work-counter proofs.

- [ ] Step 1: Write `paint_only_build_does_not_repeat_font_matching_or_shaping`, using an explicitly registered real Latin fixture. Compare counts with a no-first-line build. Expected: current first-line matching/shaping repeats work.
- [ ] Step 2: Run that test and inspect RED. Expected: observed work exceeds the normal build, not a synthetic estimate.
- [ ] Step 3: Reuse shape data only when normal warnings and saturation are clean, alternate glyph cap covers the copied store, and Task 1 qualified. Keep full alternate shaping otherwise. Preserve explicit float bits and share immutable instance/feature arrays through existing Arcs. Expected: work counts equal a normal build.
- [ ] Step 4: Add/run `reused_shaping_preserves_first_line_paint_and_full_line_geometry`, `shape_fallback_preserves_warnings_saturation_and_glyph_errors`, `inherited_float_bits_and_ruby_use_full_fallback`. Compare a forced full reference at widths 0, 32, 240 and f32::MAX; source maps, geometry, glyphs, intrinsic sizes, warnings, first/later paint, callbacks, Ruby and exact errors. Expected: equality; unsupported paths keep full work.
- [ ] Step 5: Run focused reuse tests, formatting and `cargo +stable test -p shodo`. Expected: green suite.
- [ ] Step 6: Commit `perf: reuse paint-only first-line shaping`. Run task-done with the crate suite. Expected: complete ledger line.

## Task 3: Measure adoption, document scope and verify the workspace

**Files:** create `dev/bench/examples/first_line_build_reuse.rs`, `docs/records/shodo-16-first-line-reuse-2026-10-12.{md,json}`; modify `docs/README.md`.

**Interfaces:**
- Consumes: unchanged public `ParagraphBuilder::{analyze,build}`, `ParagraphAnalysis::shape`, fixed fonts, public layout/intrinsic/plan output, existing allocation scopes and recursive snapshot support.
- Produces: standalone baseline/candidate measurement source, compact raw timing/allocation data and explicit runtime adoption or rejection decision.

- [ ] Step 1: Add the measurement example with short/4000-word controls, paint/color/decoration reuse and size/transform/Ruby fallback classes plus RTL/CJK/vertical/composed/structural inputs. Output hashes include full recursive geometry, mappings, build/layout warnings, intrinsic widths and height rejection. Expected: baseline smoke runs without oracle omissions; workload construction and hashing outside measured windows.
- [ ] Step 2: Build baseline and candidate release binaries before timing; run at least five alternating process pairs, >=3 samples per case. Allocation diagnostics are separate from counter-free timing. Expected: every baseline/candidate oracle agrees before evaluating speed.
- [ ] Step 3: Compute median/raw series and allocation/retention/peak comparisons; record any guard/copy/fallback regressions and decide adoption. Expected: scoped evidence supports keeping the runtime path, or the path is reverted with a ledgered ruling.
- [ ] Step 4: Save conditions, exact benchmark/driver sources, raw series, output digests, work proofs and limitations in the formal records and index. Expected: scratch can be removed without losing reproduction or decisions.
- [ ] Step 5: Run `cargo +stable fmt --all --check`, `cargo +stable clippy --workspace --all-targets -- -D warnings`, the accesskit variant, and `cargo +stable test --workspace`. Expected: all success; exact test totals recorded.
- [ ] Step 6: Commit `bench: record first-line reuse costs and coverage`; run task-done with the workspace suite. Expected: complete ledger line.

After all tasks, package the complete branch and dispatch one fresh-context review on the most capable available model, with this Review Focus and ledger rulings. Parent verifies final CI on the reviewed HEAD before the authorized merge. Record/resolve every declined judgment, preserve rulings in Plane/formal records, then remove only this task's temporary artifacts and worktrees.
