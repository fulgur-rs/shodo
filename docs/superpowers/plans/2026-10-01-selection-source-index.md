# Selection Source Index Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans natively inline, not per-task delegation. User authorized every child implementation/PR/all CI/merge/close/owned cleanup; no repeated design or integration approval.

**Goal:** Bound source candidate search for short selections while preserving exact rectangles and measure construction/retention tradeoffs.
**Architecture:** Private immutable source index references original segments. Monotonic start/end vectors borrow binary-searched ranges; general order uses sorted original ordinals and subtree-max-end interval pruning. Original caret normalization and visual sorting/merging remain.
**Tech Stack:** Rust, real-font/private regression tests, external separate normal/count probes.
**Spec:** docs/superpowers/specs/2026-10-01-selection-source-index.md

## Global Constraints

Base ab33dfa758becbdecd61e74796f3bb9252fbefb5, baseline sbp6-ruby-cursors-final source48fa29dc. Preserve original Segment order and accepted_segments/paint_segments consumers, source/glyph/geometry/warnings/limits and public API. Do not discard overlapping/duplicate ranges or use spatial order as source order. No dependencies/fonts/corpus/S4/switch changes. Separate query and LineLayout construction scopes; held output counts net/peak, gross differs, RSS unmeasured. Fixed1000 queries alone do not prove empirical O(N²). Frozen final54 matrix.

## Review Focus

- Equal/overlapping source ranges may have distinct geometry: keep every matching original segment and prune with subtree max end, not single endpoint.
- Atomic/tab segments precede sorted glyph clusters; unordered vectors must select generic index, with exact geometry preserved.
- Reversed endpoints/affinities normalize through existing caret semantics, including empty/multiline/first-line datasets.
- Bidi gaps and vertical combined text must retain both old visual sort/merge phases and block-axis rectangles.
- Index metadata/build overhead also affects paint/accessibility callers constructing LineIndex; measure and disclose, no universal retention/speed claim.

## Task 1: Freeze original selection and index costs

**Files:** external target/performance-artifacts/sbp9-selection-probe; existing hit index/selection and real-font fixture references.
**Interfaces — Consumes:** LineLayout::new(&[Line]), selection_rects(TextPosition,TextPosition), fixed fixture fonts, bench CountingAllocator/geometry digests.
**Interfaces — Produces:** immutable before normal/count binaries, source/profile/dependency provenance, independent index-build vs fixed-query calls/gross/net/peak/time and full output signatures.

- [x] Build fixed-font probe with N1024/4096/16384 plain single-character/full-line queries, bidi/mixed atomic-tab/duplicate-grapheme/vertical-combined fixtures and endpoint/affinity variants. Hold index or last query output through scope end; exclude font/paragraph preparation and validation from each scope. Separate normal/count binaries, CPU10.
- [x] Capture immutable original production before implementation, validate all glyph/source/rectangle signatures and warnings, deterministic scoped allocation repeats. Do not use counting timings as causal normal speed evidence.
- [x] Inspect real segment order/overlaps and record representation ruling in ledger. Freeze measured baseline before production changes; no guessed uniform benefit.

## Task 2: Index candidates with preserved geometry

**Files:** crates/shodo/src/hit/source.rs (new private module), index.rs, selection.rs, private regression tests.
**Interfaces — Consumes:** original immutable &[Segment] text intervals and retained rectangles, Task1 frozen output/cost baseline.
**Interfaces — Produces:** private SourceIndex::new(&[Segment]) and for_each(from:u32,to:u32,segments:&[Segment],callback:impl FnMut(&Segment)); immutable source references, no query heap allocation before existing rect collection.

- [x] Add test-only actual visit counter around old source predicate. Large real-font short queries must match independent expected rectangle before bounded-work assertion (e.g. <=64 visits for N16384), so old full scan produces genuine RED. Add exact linear-oracle and independent geometry regressions for every Review Focus case, generic overlapping/duplicate ranges and monotonic path.
- [x] Run focused tests and preserve genuine resource/work RED after expected geometry passes.
- [x] Implement private source index: detect monotonic starts/ends, borrow binary-searched slice; otherwise retain sorted ordinals and balanced implicit interval tree max-end summaries. Traverse without query Vec, preserve original segment identity and all source intersections. Build once after all segments are emitted; replace only selection candidate iteration, keep old geometry filters and visual sort/merge code.
- [x] Run hit/source/selection, vertical and source regressions; capture same-body candidate A/B. Confirm all exact signatures and build/query scoped counters, record metadata/time tradeoff and reject any unjustified complexity or semantics regression.
- [x] Commit adopted implementation with spec/plan. No production freeze until all required meaningful regression additions are included.

## Task 3: Final evidence and integration

**Files:** docs/records/selection-source-index.md; data manifest/raw; external final54/probe/logs/native ledger.
**Interfaces — Consumes:** adopted Task2 frozen source and original immutable Task1 baseline.
**Interfaces — Produces:** source-frozen equivalent matrix and auditable memory/time evidence, one final whole-branch review and latest-head CI integration.

- [ ] Run workspace/no-default/complex-only/fmt/all-target Clippy/docs denied warnings/new-directory fixed snapshots, inspect source/affinity/atomic/tab/TCY regressions.
- [ ] Collect frozen all54 against sbp6-ruby-cursors-final; verify source fingerprint and warm/memory each7 digests.
- [ ] After all local builds/collector stop, run normal balanced A/B for index construction and short/full queries; retain every sample and investigate material anomalies. Verify all probe signatures/warnings and scoped repeats.
- [ ] Package scopes, retained metadata, fixed1000 query scale, adoption/tradeoffs/limitations, all raw samples/config/lock/source/binary hashes/RED-GREEN/logs/ledger; verify facts and commit.

One fresh read-only Astra whole-branch review after all tasks, one fix pass for Critical/Important then exact frozen verification. PR/all latest-head CI/merge matching verified head, close issue, copy native artifacts and remove only owned tree/branch. Continue next child.
