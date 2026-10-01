# Edge-window sharing implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove redundant cache insertion/hit deep copies while preserving independent final output.
**Architecture:** Immutable private Arc tuple for clean cached windows; private owned/shared read handle for probes and final owned conversion. Existing cache state machine/budgets unchanged.
**Tech Stack:** Rust, existing fixture fonts/allocator, stdlib Python.
**Spec:** docs/superpowers/specs/2026-10-01-edge-window-sharing.md

## Global Constraints
User-authorized inline six-issue worktree/PR/latestCI/merge/cleanup. No public API/default-limit/dependency changes, switch blockers, saved/protected mutations. Full exact public outputs/ordered resource warnings. Source identities pinned; any parallel release delta must be independently classified and rechecked, never hidden.

## Review Focus
- Cache data immutable; final owned overlay spacing/position changes cannot mutate another result.
- Retained original Vec capacities, Arc headers and owner lifetime versus deep-cloned narrow capacities; cold/oversized paths must be measured.
- Exact paragraph identity/range/glyph-budget keys, clean/warning/saturation eligibility and charge-before-hit.
-256entry/32768cost/1024oversize, owner switch/clear/shrink and output still live after cache release.
- Full first-line/Ruby/edge/float/forced/source progress and budget warning order/caps unchanged.

### Task 1: Ownership and resource regression
**Files:** Modify crates/shodo/src/shape.rs, line/windows.rs and line/windows/edge_shape_cache_tests.rs.
**Interfaces:** Unchanged public layout APIs; new private immutable cache handle and test-only actual clone marker.
- [ ] Confirm main178dd07 core baseline383passed2ignored (fresh merged check archived).
- [ ] Add pointer identity and actual derived GlyphStore clone insert/hit tests; watch real original fail.
- [ ] Implement owned/shared handle, preserve dirty/edited/oversized no-extra-allocation ownership, trim retained capacities and final owned independence; watch tests pass.
- [ ] Test caps/replace/owner/release, selected output mutation and budget controls. Commit verified change.
- [ ] Gate: cargo test -p shodo --lib.

### Task 2: Full fixed-font A/B and retention evidence
**Files:** Create/extend dev/bench/examples edge-sharing evidence; local target/performance-artifacts/c91-2-edge-sharing recipes; docs/records/data evidence.
**Interfaces:** Existing public break_all/next_line plus disposable observers; no shipped counters.
- [ ] Pin original/candidate source/profile/font/harness and actual copy scopes (insert/hit/final-owned), shape work and capacity/owner/release controls.
- [ ] Full fixed-font output/warning equality against fresh controls, including warm/cold/owner/disabled/cache/resource/Ruby/first-line/atomic invalidation.
- [ ] Requested calls/gross/freed/net/whole-operation peak/releases, observer neutrality, adverse cases and capacities; meaningful independent verifier.
- [ ] After all builds/checks, balanced fresh counter-free A/B; archive exact evidence and adoption decision.

### Task 3: Record, verification and integration
**Files:** docs/records/edge-window-sharing.md, data manifest and raw evidence.
**Interfaces:** Task2 pinned evidence consumed by independently verified AC and fresh whole-branch review.
- [ ] Core/fmt/alltarget Clippy/docs/allocator/probe/strictstandard54 and fixed snapshots pass.
- [ ] Independently reconstruct archive/source/font/binaries/output/table data; final record and fullproof pass.
- [ ] One fresh Astra whole-branch review; Critical/Important fixes with failed→passing controls. PR/exactlatestCI/merge/main/protected/saved/bdclosed readback/ownedcleanup under existing authorization.
