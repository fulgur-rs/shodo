# Itemization Grapheme Reuse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan inline, task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement shodo-j2r.5 by reusing paragraph grapheme cuts for every flush while preserving the actual local shaping input contract.

**Architecture:** A private helper maps shared source cuts to scalar indices and repairs only truncated restart prefixes. Break projection retains authored bidi-control positions from its existing property pass. Normal runs do not require Unicode reclassification.

**Tech Stack:** Rust 1.89+, existing ICU properties/segmenter2.3 dependencies, existing ParagraphBuilder and FontCollection.

**Spec:** `docs/superpowers/specs/2026-09-29-itemization-grapheme-reuse-design.md`

## Global Constraints

* No public API or dependency changes.
* Keep Rust1.89 compatibility and all feature combinations.
* Do not alter existing font, browser, or WPT fixtures.
* Preserve font queries, orientation, source scalar ranges, and caret contracts.
* No per-flush ICU segmentation or normal-run Unicode property pass.
* Preserve the original raikiri spikes; exclude shodo-p2m.6.

## Review Focus

* Controls inside a projected grapheme must produce standalone local clusters and restart following context.
* RI context may stay different beyond the first shared cut and must be repaired through the initial sequence.
* Source gaps and duplicated width-origin ranges must map to ordered scalar cuts without cutting a generated Kana cluster.
* Style subdivisions within a cluster must query the same full local cluster and preserve the paragraph grapheme-start flag.
* Long context and repeated controls must not introduce quadratic Vec mutation or a hidden full-run fallback.

---

### Task 1: Replace duplicate segmentation and prove input equivalence

**Files:**
* Modify: `src/analysis/itemize.rs`, `src/analysis/breaks.rs`, `src/analysis/width.rs`.
* Create: `src/analysis/itemize/graphemes.rs`, `tests/data/GraphemeBreakTest-17.0.0.txt`.
* Tests: private helper module and existing analysis test modules.

**Interfaces:**
* Consumes: `Scalar { c:char, offset:u32, end:u32, .. }`, sorted `BreakAnalysis.graphemes` and `typographic_starts`.
* Produces: `graphemes::boundaries(scalars:&[Scalar], breaks:&BreakAnalysis) -> Vec<usize>` containing sorted distinct scalar-index cuts, including0 and `scalars.len()`.
* Produces: `BreakAnalysis.authored_bidi_controls: Vec<u32>` containing sorted offsets of bidi-control scalars in Text items omitted by projection.

- [ ] Keep and run the real builder characterization already written before functional edits. Assert the exact cluster strings and ranges in the spec; run `cargo +stable test --lib analysis:: --offline`. Expected: all analysis tests pass on the baseline algorithm.
- [ ] Wrap the old flush ICU iterator with a cfg(test) observed iterator. Add `shared_cuts_remove_local_segmentation` using real ParagraphBuilder for plain and decorated text. Assert local boundary visits0. Run the individual test before implementation. Expected: runtime assertion failure reporting positive actual old-iterator visits.
- [ ] Add the normative Unicode17 fixture with its original copyright/provenance. Read it in the helper tests. Test every scalar-aligned substring against independent ICU local cuts and whole normative cuts. Add generated short combinations and long restart chains. In the absence of the helper, keep an old-algorithm adapter long enough to verify these tests exercise valid inputs; do not count compile errors as RED.
- [ ] Add projection's authored control metadata and implement `boundaries` according to the spec. Preserve all existing local source assembly and paragraph caret data. Replace byte-boundary windows with scalar-boundary windows and use the existing char-offset table to form the same full-cluster query strings.
- [ ] Run `cargo +stable test --lib analysis:: --offline`. Expected: all characterizations, differential tests, width properties, and actual work-counter test pass. Inspect failures by input, not by weakening expectations.
- [ ] Run the standard final gates: fmt, warning-denied clippy/docs, workspace tests with default/AccessKit, core-only with and without complex-scripts, release library tests, and existing allocator/probe tests. Expected: every command exits0; inspect actual test totals and save a frozen source manifest.
- [ ] Commit the complete implementation and tests after verification.

### Task 2: Review, integrate, and clean the owned worktree

**Files:** branch diff and artifacts; no independent product edits unless review finds a defect.

**Interfaces:**
* Consumes: Task1's tested immutable commit, spec, this plan, test logs, and execution ledger.
* Produces: merged PR at the same tested tree, closed shodo-j2r.5, archived verification evidence, removed owned worktree/local branch.

- [ ] Dispatch one fresh read-only whole-branch reviewer while independent verification runs. Supply the full spec/plan/review focus, immutable patch, and actual dependency versions. Expected: explicit findings by effect, plus every declined judgment. Track all deferred findings as issues.
- [ ] Fix Important/Critical findings in one test-first pass and rerun the meaningful affected checks plus whole suite. Record every scope or correctness ruling. Expected: all required checks pass and no unresolved Important/Critical findings.
- [ ] Push and create PR using the repository's template. Poll that exact run/HEAD without restarting live processes. Expected: check/msrv/wasm succeed for the same feature commit.
- [ ] Verify each job's checked-out merge-preview SHA, parents and tree, and the current main parent; guarded merge only after that proof. Verify actual merge tree and issue readback before closing. Expected: tested behavior is exactly what was merged.
- [ ] Archive ignored Cargo.lock and the execution ledger; remove only this owned clean worktree and local branch. Verify automatic remote deletion. Continue with actual `bd ready` implementable issues.
