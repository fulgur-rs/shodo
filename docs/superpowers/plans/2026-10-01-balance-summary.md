# Balance summary implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stop retaining complete greedy/trial Lines during Balance width search.
**Architecture:** Share the existing fixed-width, no-previous-Line iterator. Balance collects mapped u32 ends and drops each owned Line; vector length supplies count. Keep all necessary post-scan work and Start/Pretty behavior.
**Tech Stack:** Rust, fixed-font benchmark and stdlib Python evidence tools.
**Spec:** docs/superpowers/specs/2026-10-01-balance-summary.md

## Global Constraints
All public contracts/default limits and bounded Q26.6 search/warnings/fallbacks unchanged. Protect original diagnostics and saved spikes. Native worktree/PR/CI-success merge/cleanup authorized; no switch blockers. Test-only owner observation is real Arc strong count at Line construction, not a geometry-byte or total live-Line counter.

## Review Focus
- Full scan after an infeasible count, warning cap/suppression and saturation.
- First-line mapped ends and float/forced/block offsets retain the original driver.
- Ruby/atomic metrics and plan/font/atomic keys remain identical.
- Ordinary break_all/Start/Pretty preserve ownership and no Line clone.
- All actual work, requested allocation/retention and adverse timings distinguished.

### Task 1: Streamed private ends and regression contracts
**Files:** line/iter.rs, line/iter/summary_tests.rs, line/plan.rs, test-only output/owner_probe.rs and constructor hook.
**Interfaces:** fixed_width_lines(...)->Iterator<Item=Line>, private break_ends(...)->Vec<u32>; public signatures unchanged.
- [ ] Add actual-root-owner probe and 32 forced atomic lines with one Balance iteration; assert final32 ends/ordered atomics and peak owner refs<=4, watch existing full retention fail.
- [ ] Extract identical fixed-width iterator and use break_ends for initial/trial Balance count/end; preserve Start/Pretty and all scanning.
- [ ] Verify literal endpoints/counts, full oracle warnings/geometry, forced/block/float/first-line/Ruby/atomic/limits and no clones; run core/fmt/Clippy/docs/allocator checks and commit.

### Task 2: Allocation/work/time evidence
**Files:** focused balance_summary example/helper; target/performance-artifacts/c91-5-balance-summary recipes; docs/records/data.
**Interfaces:** original planning controls and full output/warnings; actual bounded width sequence/constructors/metrics/Ruby/shapers/clone/owner observations.
- [ ] Freeze six source/font/binary-pinned producers, validate exact outputs and allocator-neutral work observations.
- [ ] Complete checks/default54; run balanced counter-free A/B and selected fresh controls; separate remaining materialization from removed retention.
- [ ] Archive exact raw streams with all licenses/source/producers and independent proof; decide measured adoption with adverse cases retained.

### Task 3: Record, review and integrate
**Files:** docs/records/balance-summary.md/data and native ledger.
**Interfaces:** One fresh Astra whole-branch review of final HEAD, one narrow correction confirmation if needed.
- [ ] Independently verify source/archive/medians/retention/contracts; fix Critical/Important and record Minor rulings.
- [ ] Create PR with concrete public-input provenance, require exact latest-head CI SUCCESS then merge.
- [ ] Verify actual main/classify concurrent changes, protect saved work/originals, close .5 readback and remove owned worktree/branches; continue .6.

Publication constraint: user declined raw evidence and host metadata publication. Keep full evidence locally; publish implementation, tests and aggregate results only. Re-root the unpublished branch so private blobs are not reachable through history.
