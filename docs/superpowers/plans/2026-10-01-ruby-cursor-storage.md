# Ruby Cursor Storage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans natively inline in the owned shodo-sbp-sequential worktree. User authorization includes implementation, PR, all CI pass, merge, close and owned cleanup; continue every child.

**Goal:** Measure and remove material dense ruby cursor retention without changing correspondence or limits.
**Architecture:** Diagnose actual old prepared data first. If justified, retain paired cut metadata and share a row-indexed immutable cursor table; compress per-lane unchanged runs with full-value transitions and dense fallback. The existing legality walk supplies exact values, and the precharged logical budget remains unchanged.
**Tech Stack:** Rust, private diagnostic/regression tests, external fixed-font normal/count probes.
**Spec:** docs/superpowers/specs/2026-10-01-ruby-cursor-storage.md

## Global Constraints

Preserve every row unit/class/cursor, paired-cut ordinal (including duplicate parent-unit rows), source correspondence and normal/first-line spans/breaks. Preserve EXACT old count*(1+lane_count) global/per-base Items charges before storage allocation and default C1024 refusal. Preserve existing correspondence walk/counting algorithm and sbp5 selected-lane measurement. Source/glyph/geometry, warnings/limits/original inputs stable. No public API/dependency/font/fixture changes; no switch gate or S4 edits. Separate gross/net/peak/RSS, timing/instrumentation and history. Final frozen all54 matrix baseline sbp8.

## Review Focus

- First-line source matching may skip normal rows and remap child cursors; use exact matched row ordinal and preserve every cursor.
- Empty bases/zero spans can have duplicate parent-unit endpoints with distinct lane cursors; do not key compression solely by parent unit.
- Mandatory/emergency/prohibited cuts and indivisible shared clusters retain exact class precedence and legal break sets.
- Frequently changing/full-span lanes need dense fallback; sparse metadata and capacity overhead must not outweigh original retention.
- Logical Items product and ancestor/base budgets remain checked before allocation, even when physical storage becomes much smaller.

## Task 1: Measure the original table

**Files:** crates/shodo/src/ruby/tests/prepared_cuts.rs; external target/performance-artifacts/sbp6-probe.
**Interfaces — Consumes:** real prepared normal/first-line ruby.cuts:Vec<PairedCut>, each lanes:Vec<usize>, existing fixed fixture builders.
**Interfaces — Produces:** cut/lane/cell/change/capacity/metadata/requested/retained diagnostics, immutable before normal/count binaries, build/layout digests and default refusal evidence.

- [x] Add ignored cursor_storage_diagnostic with real-font column and full-span fixtures; report actual counts/capacities and logical row fingerprints, check cardinality/cursors and default Items refusal. No production edit.
- [x] Run cargo test -p shodo --lib cursor_storage_diagnostic -- --ignored --nocapture.
  Expected: old dense cells equal rows*lanes, real capacities/bytes recorded; C1024 default Items error unchanged.
- [x] Adapt sbp5 external fixed-font build/layout probe to current stable path and add full-span/first-line-large coverage; capture immutable before normal/count binaries and provenance.
  Expected: all output digests valid, allocation repeats deterministic; no existing probe overwritten.
- [x] Compare actual retained table bytes with whole-build net and density; ledger adoption decision or evidence-backed rejection. Material net savings, rather than unmeasured build speed, may justify prototype; do not label table the build-time dominant component without profile evidence.
  Expected: next step follows measured cost/density, not a guessed implementation.

## Task 2: Implement the justified representation

**Files:** crates/shodo/src/ruby/cuts.rs; index.rs; tests/cuts.rs; tests/prepared_cuts.rs.
**Interfaces — Consumes:** Task1 old row fingerprints/counts and stored binaries, existing emit(unit,&[usize],class) correspondence callbacks, exact precomputed count.
**Interfaces — Produces:** same Vec<PairedCut> metadata with lane row view indexing; immutable shared table, per-lane full-value transitions or dense fallback. Production uses exact known count, convenience test wrappers keep one legality walk. No row reconstruction in measurement.

- [x] If Task1 justifies it, add genuine retained-storage work regression on many short spanned lanes; separately pin dense-changing lanes and duplicate-unit endpoints, preserving every logical row via independent expected arrays/existing tests. Watch old storage fail the compression target before implementation.
- [x] Implement private row-indexed shared storage builder fed by existing emit; choose per-lane sparse/dense based on actual row count/change density. Pass precomputed count through index paths so preparation retains two walks. Keep both Items charges unchanged. Adjust tests to read logical cursors without assuming Vec ownership.
- [x] Re-run ruby tests and original diagnostics; compare exact row fingerprints/limits against old output. Capture candidate A/B; if no material gain or regression outweighs it, reject/revert production prototype with documented ruling.
  Expected: source/cursors/classes/budgets identical; measured memory benefit including metadata/capacity overhead, density-safe fallback.
- [x] Commit adopted implementation or verified diagnosis/rejection with spec/plan.

## Task 3: Verify, package and integrate

**Files:** required logs, external matrix/probes; docs/records/ruby-cursor-storage.md and data JSON/rawgzip.
**Interfaces — Consumes:** adopted Task2 state and Task1 original measurements.
**Interfaces — Produces:** required validation, final source-frozen all54 equivalence, standalone A/B with normal and first-line/RTL/nested/full-span/empty coverage, immutable evidence and reviewable branch.

- [x] Run workspace/no-default/complex-only, fmt/all-target Clippy/docs denied warnings/fixed snapshots; inspect existing base/ancestor Items tests.
- [x] Collect final frozen all54 against sbp8; compare warm/memory each7 digests and final source fingerprint.
- [x] After builds/collector, run balanced normal timings and verify dedicated build/layout output/warning/source signatures and original row fingerprints. Keep calls/gross/net/peak and table bytes separate, RSS unmeasured.
- [x] Record adoption/rejection, scopes and all samples/source/lock/binary hashes/RED-GREEN/limits/diagnostic logs; verify facts and commit.
  Expected: every claim auditable; no budget relaxation or universal speed claim.

One fresh read-only Astra whole-branch review after all tasks; fix Critical/Important, PR/all CI/merge matching reviewed head, close issue, copy native ledger/review externally and remove only this tree/branch. Continue next eligible child.

## One final-review fix pass

- [x] Reproduce dense8x64 metadata increase after exact cursor checks, then compare all column capacities/headers with flat arena and add global fallback/direct normal dense construction.
- [x] Verify raw-builder and real normal/first-line regression RED6176/6432→GREEN, Ruby91, required1071/618/621/fmt/clippy/docs/snapshots.
- [x] Confirm focused resolution with same reviewer; freeze source48fa29dc, new same-body64 A/B and54 matrix, all9 old logical fingerprints/default refusal and final ABBA. Preserve prototype data separately and disclose dense64 gross/peak and first-line CPU tradeoff.
