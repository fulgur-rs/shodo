# Borrowed Shaping Windows Implementation Plan

> For agentic workers: REQUIRED SUB-SKILL: superpowers:executing-plans. Execute natively inline in the isolated shodo-sbp-sequential worktree; implementation, PR, passing CI, merge, issue close and cleanup are authorized.

**Goal:** Remove temporary scalar and metadata copies from shaping windows.
**Architecture:** Borrow the original scalar sub-slice and read original immutable metadata. Keep window_end separately; retain original ShapeItems and edge-edit owned input.
**Tech Stack:** Rust, fixed-font shodo-bench allocation and timing probes.
**Spec:** docs/superpowers/specs/2026-09-30-borrowed-shaping-windows.md

## Global Constraints

Preserve source/glyph/geometry and warning/limit behavior, original data retention, scalar/grapheme/pen progress, ruby base scopes, RTL, variable fonts, first-line and edge reshape. No raikiri switch gate or S4 spike edits. Separate gross from net/peak/RSS and instrumentation from time. Freeze Rust source before the final standard collector.

## Review Focus

- Last cluster uses the selected window end, including scalar budget/grapheme splits.
- Missing-font fallback iterates only selected scalars and keeps warning/base glyph budgets.
- RTL pen storage and glyph ownership remain stable across window boundaries.
- FontMatch variations and metadata are borrowed for every window without changing instance resolution.
- First-line/edge reshaping retains original ShapeItems and original/neighbor context rules.

## Task 1: Borrow per-window scalars and metadata

**Files:** crates/shodo/src/analysis/itemize.rs (test-only clone instrumentation); crates/shodo/src/shape.rs; crates/shodo/src/shape/tests.rs.
**Interfaces — Consumes:** original: &ShapeItem, start..cursor, last.end, resolved font instance, optional BaseScopes.
**Interfaces — Produces:** scalars: &[Scalar], window_end: u32; font/orientation/level/script read from original. No public API change.

- [ ] Add actual Scalar Clone instrumentation in cfg(test), keeping production derive Clone unchanged. Test real and missing fonts with one and split budget windows; reset clone counter after input preparation and call real shape_items.
- [ ] Run cargo test -p shodo --lib shaping_windows_borrow_source_scalars.
  Expected: old to_vec fails zero-clone-work assertion; outputs and limits still exercise actual shaping.
- [ ] Preserve immutable old normal/instrumented fixed-font build probe binaries and configuration/source provenance before production edits. The sbp5 standard report is the full-matrix baseline.
  Expected: all source/glyph/geometry outputs valid; timings and allocations use separate binaries/scopes.
- [ ] Replace temporary owned ShapeItem with borrowed scalar slice, selected window_end, and original metadata; leave context/cursor/budget/pen/edge-edit logic unchanged.
- [ ] Run cargo test -p shodo --lib shape::.
  Expected: work regression and actual shaping/limits/variation cases pass.
- [ ] Commit the borrowing change.

## Task 2: Verify and measure

**Files:** external target/performance-artifacts/sbp7-* probes and results.
**Interfaces — Consumes:** Task 1 implementation, old immutable binaries and sbp5 baseline.
**Interfaces — Produces:** final source-frozen A/B, full-matrix digest equivalence and successful required validation logs.

- [ ] Run workspace, no-default and complex-only tests, fmt, Clippy with denied warnings, docs and fixed glyph snapshots.
  Expected: missing-font, giant grapheme/pen, ruby budgets, RTL, variation, first-line and edge reshape regressions pass; expectations unchanged.
- [ ] Collect final all54 standard matrix against sbp5 and compare warm and memory digests; verify final source fingerprint.
  Expected: all54 cases and each7 output digests agree.
- [ ] Run representative build A/B with saved old/final allocation binaries; after validation builds, measure separate normal binaries in balanced order with fixed CPU affinity.
  Expected: source/glyph/geometry equivalence; observed time/calls/gross/net/peak saved with explicit scopes/remaining retained data.

## Task 3: Record evidence

**Files:** docs/records/borrowed-shaping-windows.md and JSON/raw gzip evidence.
**Interfaces — Consumes:** verified outputs and measurements from Task 2.
**Interfaces — Produces:** reproducible record and reviewable committed branch.

- [ ] Write evidence and limitations; include probe source/manifest/lock, immutable binary hashes, RED/GREEN and validation logs.
- [ ] Verify evidence against stored digests and final source fingerprint; commit record.
  Expected: all recorded facts match collected output.

After Task 3, request one fresh whole-branch review per native execution, resolve Critical/Important findings, create PR, wait for every CI check, merge matching reviewed head, close beads and remove only this worktree/branch. Continue with the next eligible issue.
