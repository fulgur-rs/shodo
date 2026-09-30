# MRU Cache Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-sbp.13 using actual cache-search workload evidence and independent changes.
**Architecture:** Keep keys and VecDeque LRU mutation/caps unchanged; only lookup direction is a candidate. Local overlays expose actual event/retention diagnostics for immutable state-specific binaries.
**Tech Stack:** Rust stable, existing CountingAllocator, Python3, CPU10.
**Spec:** docs/superpowers/specs/2026-10-01-mru-cache-search.md

## Global Constraints

No capacity/word cache/publicAPI/dependency/defaultlimit/fixture expansion. Preserve source/glyph/geometry/warnings/FontId qualification and full key semantics, cache order, limits, shrink and externally owned handles. Separate time/gross/net/peak; RSS unmeasured. Saved spikes unchanged and no raikiri/S4 blockade. User already authorizes sequential PR/latestheadCI/merge/close/cleanup. Repository TMPDIR for checks.

## Review Focus

- rposition index remains forward-indexed, with unchanged remove/push_back/pop_front and bounds.
- Match expression retains exact FontId/script/direction/language/feature-selector/features semantics.
- Actual small/many font/script/feature and MRU/LRU/cycling/churn workloads, all misses and slower adverse controls disclosed.
- Clear/shrink/cap0/cap1 and evicted externally owned Arc lifetime stay intact.
- Trace logical comparisons do not prove release CPU contribution or parallel contention improvement.

### Task 1: Baseline and workload attribution

**Files:** target/performance-artifacts/sbp13-cache-probe/, spec/plan.
**Interfaces:** Produces immutable baseline source/raw/binaries, actual cache operations/retention and exact output/control signatures used by both isolated candidates.

- [ ] Inspect actual plan/shaper keys and primary harfrust dependency, caps and owner lifecycle. Record exact trace boundaries and any supported-axis/feature limitations.
- [ ] Capture separate counter-free time/allocator-only/trace-only binaries with small/many font/script/feature, MRU/oldest/round-robin/skew/churn cases. Probe cache operations and public-build output controls; font registration/validation outside measured scope. Save fonts/licenses/hashes/source/overlays.
- [ ] Verify literal key/glyph/Arc/LRU controls and positive measured comparison/hit/miss/eviction/retention counts; source/harness identity. Commit spec/plan with baseline facts.

### Task 2: Plan lookup candidate

**Files:** crates/shodo/src/shape/cache.rs and tests.
**Interfaces:** Consumes baseline evidence; produces plan-only candidate, measured comparison tradeoff and adoption decision.

- [ ] Actual search-count regression RED MRU distanceN versus1 with independent identity/literal output guards. Pin full font-qualified key distinctions and LRU eviction/cap/shrink/external Arc lifetime.
- [ ] Reverse only the position search, preserving key and mutation. Run focused tests and capture immutable plan-only state; compare baseline counters/time/alloc/retention/full signatures. Disclose oldest/cycling/miss tradeoffs. Commit scoped adoption or rejection.

### Task 3: Shaper lookup candidate

**Files:** crates/shodo/src/font/mod.rs and focused cache tests.
**Interfaces:** Consumes baseline; produces shaper-only isolated comparison and final combined shipping state if both adopted.

- [ ] Actual shaper-search MRU REDN versus1 with independent Arc identity/face controls. Preserve cap0/1/default, shared/document delegation, LRU promotion/eviction and external ownership.
- [ ] Reverse only search. Capture shaper-only state from baseline plus final combined state independently; exact outputs/order/retention and component attribution. Commit candidate and scoped decision.
- [ ] Run required final workspace/no-default/complex/fmt/all-targetclippy/docs/snapshots; preserve all failure evidence and fix actual regressions before adoption.

### Task 4: Evidence and integration

**Files:** docs/records/mru-cache-search.md, data/mru-cache-search{.json,-raw.json.gz}, native ledger.
**Interfaces:** Produces verified scoped outcomes and branch-finishing gates.

- [ ] Standard54 versus sbp12-font-data-reuse and balanced counter-free processes after builds. Preserve all samples/output/refusal contracts; report direct-cache and public-build scopes separately with unchanged/adverse cases.
- [ ] Package exact engine/harness/lock/overlay/font/binarySHA/raw/checks; reconstruct all source fingerprints, key/lifecycle controls, actual counters/allocations/medians and archive checksum. Commit records and run final proof.
- [ ] One final fresh whole-branch Astra review; one Important/Critical fixpass with genuine RED/GREEN and required full validation/recapture for sourcechanges, Minor deferred. Authorized PR/latestheadCI/merge/close/ownedcleanup after evidence gates.
