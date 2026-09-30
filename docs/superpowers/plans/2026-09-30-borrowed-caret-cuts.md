# Borrowed Caret Cuts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans natively inline in the isolated shodo-sbp-sequential worktree. User authorization includes implementation, PR, all CI pass, merge, issue close and owned worktree cleanup.

**Goal:** Eliminate per-cluster caret cut Vec copies during finalized line index construction.
**Architecture:** Return a slice borrowing the finalized Line dataset and pass it through add; copy each u32 into existing retained Caret values. Atomic/tab two-endpoint slices borrow stack arrays.
**Tech Stack:** Rust, fixed-font shodo-bench normal/allocation probes.
**Spec:** docs/superpowers/specs/2026-09-30-borrowed-caret-cuts.md

## Global Constraints

Preserve inclusive endpoints, indivisible transforms/shared graphemes, GDEF/variable ligature caret geometry, ruby alignment/annotations, RTL and both vertical combined writing modes. No public API or retention/lifetime changes. Preserve source/glyph/geometry and warnings/limits, original inputs, separate normal time from allocation counting, gross from net/peak/RSS, and sequence/allocator history. No raikiri switch gate or saved S4 edits. Freeze Rust source before final all54 collector against sbp7.

## Review Focus

- An indivisible transformed/shared grapheme cluster merges before final cuts; internal offsets must not become stops.
- GDEF and variation carets use unchanged internal-cut counts and expansion/compression coordinates.
- Ruby padding and annotation retained caret indexes stay aligned with source cuts and hit geometry.
- RTL and both vertical combined writing modes retain closed endpoints across adjacent lines.
- Empty/single-cut slices and atomic/tab stack endpoints remain valid while add creates owned stop values.

## Task 1: Borrow finalized caret cuts

**Files:** crates/shodo/src/hit/index.rs; external target/performance-artifacts/sbp8-probe.
**Interfaces — Consumes:** finalized Line.data.breaks.caret_cuts and selected inclusive Range<u32>.
**Interfaces — Produces:** cuts<'a>(&self, line: &'a Line, text: &Range<u32>) -> &'a [u32]; add(cuts: &[u32]); existing owned index values unchanged.

- [ ] Add cuts_borrow_finalized_line_data regression on real TCY finalized lines in both writing modes/directions; assert cut values and original slice pointer for full/partial/empty selections, owning caret data remains stable.
- [ ] Run cargo test -p shodo --lib cuts_borrow_finalized_line_data.
  Expected: old to_vec fails retained-data identity, not glyph/geometry setup.
- [ ] Capture immutable original normal/memory index-construction binaries and source/config provenance before production edit; actual caret/selection/hit and source/glyph/geometry outputs recorded outside scope.
  Expected: fixed-font finalized output valid; previous shodo-sbp.7 normal/memory are not overwritten.
- [ ] Change cuts return slice with explicit Line lifetime, add takes &[u32] and iter().copied(), atomic/tab &[start,end]. Preserve selection partition points and all index algorithms.
- [ ] Run cargo test -p shodo --lib hit::, then related ruby/transformed/shared-grapheme regressions in full required suites. GDEF tests and TCY closed endpoints pin review focus; empty selection identity regression pins fallback.
  Expected: zero cut copying, geometry unchanged and existing tests pass.
- [ ] Commit implementation and plan/spec.

## Task 2: Verify and measure

**Files:** external sbp8 probes/matrix/validation logs.
**Interfaces — Consumes:** Task1 final implementation, immutable original probe binaries, sbp7 matrix baseline.
**Interfaces — Produces:** required green validation and final-source-verified all54 equivalence, dedicated time/calls/gross/net/peak evidence.

- [ ] Run workspace/no-default/complex-only, fmt, all-target Clippy and docs with denied warnings, unchanged fixed glyph snapshots. Check transform/shared grapheme, GDEF/variation, ruby/RTL/TCY and atomic/tab behavior.
  Expected: all pass; no expected geometry update.
- [ ] Collect final source-frozen all54 matrix against sbp7; compare warm/memory each7 digests.
  Expected: all outputs and final source fingerprint match.
- [ ] Run separate normal/count A/B for LineLayout::new with original lines prepared outside scope; compare exact caret/selection/hit outputs, source/glyph/geometry; balanced timings after all validation, record sequence/history.
  Expected: calls/gross reduced; measured net/peak/time saved without unmeasured claims.

## Task 3: Record evidence

**Files:** docs/records/borrowed-caret-cuts.md and data/borrowed-caret-cuts{.json,-raw.json.gz}.
**Interfaces — Consumes:** Task2 verified reports/logs, probe source/manifest/lock/binary provenance.
**Interfaces — Produces:** reproducible evidence and reviewable committed branch.

- [ ] Package exact samples/digests/geometry, RED/GREEN and required logs, source and binary hashes; write scope/results/limitations.
- [ ] Verify evidence and final source fingerprint; commit record.
  Expected: all claims match immutable measurements.

After Task3, one fresh read-only Astra whole-branch review; resolve Critical/Important, create PR, wait all CI checks, merge matching reviewed head, close beads, preserve native ledger/review externally and remove only owned tree/branch. Continue next eligible child.
