# Single-pass Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-sbp.11 with one feature construction per shaped item and reproducible A/B evidence.
**Architecture:** Coordinate resolution leaves features empty; shaping owns item-specific features. An append helper writes CSS and author features into the existing orientation Vec. A development-only external probe snapshots baseline/candidate sources and immutable executables.
**Tech Stack:** Rust stable1.97.1 release, existing CountingAllocator, Python3, CPU10.
**Spec:** docs/superpowers/specs/2026-10-01-single-pass-features.md

## Global Constraints

Private code only; public API/dependencies/default limits/fixtures/saved spikes unchanged. Keep source/glyph/geometry/warnings, variations/size-adjust and retained inputs exact. Performance never blocks raikiri/S4. Separate time/gross/net/peak; RSS unmeasured. Native sequential execution and existing user PR/CI/merge authorization apply.

## Review Focus

- Width defaults precede explicit author width overrides, duplicates remain ordered.
- Upright vert/vrt2 and vkrn defaults preserve existing author override behavior.
- Combined/sideways run features omit automatic vertical defaults.
- Metric and missing-font consumers retain their actual contracts; tiny windows share final features.
- Default empty features and many short feature-rich runs have separately disclosed costs.

### Task 1: Fixed baseline

**Files:** target/performance-artifacts/sbp11-feature-probe/{src/main.rs,src/allocator.rs,capture.py,verify.py,Cargo.toml}; spec and plan.
**Interfaces:** Produces immutable before timing/memory/trace executables, source/fixture hashes and exact signatures consumed by Task2.

- [ ] Confirm all resolve/feature consumers and record the three production coordinate-resolver calls plus its direct test.
- [ ] Build the public paragraph probe with fixed Latin/CJK, default/rich, orientation/combine/width and one/many styles; measure builder+build with paragraph retained through scope end, preparation/registration/prewarm/validation excluded or explicitly documented.
- [ ] Capture baseline from the merged .10 source; verify all output hashes, actual feature construction counters and deterministic allocation repeats with `python3 target/performance-artifacts/sbp11-feature-probe/verify.py before`. Expected: fixed sources/fonts and all conditions valid.
- [ ] Commit spec/plan and baseline facts.

### Task 2: Single-pass feature construction

**Files:** crates/shodo/src/shape/instance.rs and features.rs, focused tests in those files.
**Interfaces:** Consumes baseline signatures; produces source-frozen candidate with same public outputs and fewer actual feature constructions.

- [ ] Add test-only thread-local actual feature-construction observation and resource regression for a feature-rich resolved metric instance; run its exact selector. Expected: assertion actual1 versus expected0 fails under old resolver, not compilation failure.
- [ ] Remove feature construction from coordinate resolve; keep shape_items assignment/missing fallback as final owner. Introduce append helper to populate upright Vec directly while preserving literal ordered feature values. Add ordered horizontal/upright/combined/sideways/width/author-override tests.
- [ ] Run focused shape/metric/tiny-window tests and workspace/no-default/complex/fmt/clippy/docs/fixed snapshots. Expected: resource GREEN and all positive-count checks pass.
- [ ] Capture candidate under identical scopes; `verify.py after` must verify exact signatures and counters. Commit code/tests and costs.

### Task 3: Evidence and integration

**Files:** docs/records/single-pass-features.md and data/single-pass-features{.json,-raw.json.gz}; native ledger.
**Interfaces:** Consumes fixed baseline/candidate raw and verification; produces checked archive and scoped adoption decision.

- [ ] Warmup and counter-free ABBA, then standard54 matrix versus sbp10-variable-matches. Expected: dedicated and standard outputs/refusals unchanged, stable counts, exact hashes, current local checks pass.
- [ ] Archive sources, harness, commands, fixed fonts/licenses, raw samples and logs; verify archive checksum and fingerprints. Record default/rich time/gross/net/peak and unmeasured domains. Commit evidence.
- [ ] One final fresh Astra whole-branch review; then user-authorized PR, latest-head CI, merge, issue close and clean owned worktree removal.
