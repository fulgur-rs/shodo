# Edge cache eviction implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to execute inline task-by-task.

**Goal:** Diagnose full-clear cost and evaluate bounded partial eviction.
**Architecture:** Use the existing immutable window cache and fixed-font public probes. Keep owner invalidation and all shape eligibility/charge rules. Candidate minimum sufficient hash-order eviction adds no recency fields or per-hit mutation; adopt only after evidence.
**Tech Stack:** Rust and local Python evidence tools.
**Spec:** docs/superpowers/specs/2026-10-01-edge-cache-eviction.md

## Global Constraints
256 entries/32768 total glyph-equivalents/1024 single-window cost unchanged. All public contracts/defaults/warning order and resource fallback unchanged. Raw/host data private, aggregate-only publication; original diagnosis/saved work unchanged. Sequential worktree/PR/CI-success merge/cleanup authorized.

## Review Focus
Owner resets vs actual capacity clears; budget charged on hits; warning/saturation/replacement/oversize exclusion; entry/cost/zero-cost bounds and exact accounting; actual retention/font/root lifetime; adverse cold/warm/width/plan behavior.

### Task 1: Observe the actual cause
**Files:** edge_cache_eviction example, local observer/capture/verify recipes.
**Interfaces:** fixed-font cold/warm/full-output controls and allocation-free cache event counters; actual collection-layer disablement outside scopes.
- [ ] Freeze baseline sources/fonts/binary/profile; observe hits/misses/entry-cost capacity clears/owner-reset reasons and actual shaping.
- [ ] Compare independent complete output and distinguish empty initialization/retained eviction; pin causal baseline before policy mutation.
- [ ] Commit focused public harness; record all detailed evidence locally.

### Task 2: Test and independently evaluate the candidate
**Files:** line/windows.rs and focused cache regression tests; local fixed producer/evidence recipes.
**Interfaces:** Same existing private insert/get/clear signatures and budgets; minimum sufficient partial eviction, no per-hit update.
- [ ] Watch partial-retention regression fail on full clear; implement minimal eviction and test exact cost/entry/oversize/owner/replacement/budget/warning/lifetime contracts.
- [ ] Freeze before/after normal/memory/observer producers and prove full output, observer allocation neutrality and actual cause/reshape sequence.
- [ ] Run checks/default54, independent window/map/owner release controls, balanced counter-free A/B with cold/warm/width/plan/locality/churn controls. Keep adverse/net/peak costs.
- [ ] Adopt when worthwhile; otherwise restore original runtime and preserve rejected-candidate evidence locally. Commit aggregate record and selected code only.

### Task 3: Review, integrate and finish the epic
**Files:** aggregate record/manifest and native ledger.
**Interfaces:** One independent Astra whole-branch review; exact latest-head CI then merge; private raw/history exclusion proof.
- [ ] Review final immutable implementation/aggregate scope and fix Important/Critical; record restrictions.
- [ ] Create aggregate-only PR; merge after all latest-head checks succeed; classify concurrent source and verify actual main.
- [ ] Close .6 readback, remove only owned worktree/refs, preserve originals/saved work/private evidence; audit six children, close epic and complete explicit goal.
