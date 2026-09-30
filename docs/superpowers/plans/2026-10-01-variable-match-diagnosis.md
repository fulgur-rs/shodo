# Variable Match Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-sbp.10 with reproducible variable-font evidence and only evidence-backed private optimization.

**Architecture:** A development-only external probe snapshots a fixed font and immutable baseline/candidate executables. A documented local overlay exposes normalized matching and counters without changing the shipping public API. If measured costs warrant it, production cache results and retained shape items share immutable matches while public callers still receive owned FontMatch.

**Tech Stack:** Rust1.97.1 release, existing shodo-bench CountingAllocator, Python3 stdlib, fixed CPU10.

**Spec:** docs/superpowers/specs/2026-10-01-variable-match-diagnosis.md

## Global Constraints

Public FontMatch remains Vec<FontVariation>. System discovery disabled; default limits unchanged. Existing fixed fixtures and saved spikes unchanged. Performance never blocks raikiri switching/S4. Preserve source/glyph/geometry, warnings, coords/variations/size-adjust, identity, generation and bounded cache retention. Separate time/gross/net/peak; RSS unmeasured. Native execution and existing user PR/CI/merge authorization apply.

## Review Focus

- Negative cached matches and oversized/disabled cache keys still yield the same fallback.
- Public owned variation mutation cannot corrupt cached/private results.
- Parent/document generation changes invalidate cache while previously retained matches remain valid.
- Explicit opsz, italic/oblique and size-adjust preserve actual shaping instance and glyph geometry.
- Cold, many-style and static-font overhead must accompany warm-hit improvement.

### Task 1: Fixed baseline and probe

**Files:** External target/performance-artifacts/sbp10-variable-probe/{src/main.rs,Cargo.toml,capture.py,verify.py,fonts/}; no shipping Rust change.
**Interfaces:** Produces immutable before timing/count executables, source/overlay/font manifests, raw operation outputs and actual hit/miss statistics. Task2 consumes their signatures/costs.

- [ ] Download fixed Roboto Flex and OFL, assert sfnt fvar ranges and archive SHA256 and original Git blob identity. Keep existing fixture assets intact.
- [ ] Capture current main and prove normalized warm-variable hits independently of public query normalization; include static and negative controls, misses, cache cap0/8/default, retained results and generation changes. Measure long text1024/4096/16384, styles8/64, explicit opsz/size-adjust/italic/oblique outputs.
- [ ] Run `python3 target/performance-artifacts/sbp10-variable-probe/verify.py before` from main. Expected: all scope/result invariants, finite geometry, actual hit/miss accounting and fixed source/font hashes verified.
- [ ] Record allocation share and decide whether a private shared result deserves an A/B. Commit spec/plan with baseline facts.

### Task 2: Conditional shared result

**Files:** Conditional crates/shodo/src/font/matching.rs, matching/matching_tests.rs, analysis/itemize.rs, shape.rs, line/hyphen.rs and dedicated tests only as required.
**Interfaces:** Consumes Task1's immutable baseline; produces a source-frozen candidate and public-API-compatible results, or a documented no-change conclusion.

- [ ] If measured cloning is material, write a real warm-hit resource regression first, with literal variation values and public ownership isolation assertions. Observe allocation or backing-storage duplication RED under existing code before implementation.
- [ ] Implement private shared immutable matches; keep static/missing paths efficient, original value equality and public Vec ownership. Test disabled/capped cache, synthetic ital/wght/slnt clamps, generation invalidation and retained old values.
- [ ] Run focused matching/itemize/shape/hyphen tests and workspace suite. Expected: resource GREEN plus all correctness tests green. A no-change diagnosis runs baseline verification instead; ledger the measured reason.
- [ ] Capture candidate with unchanged probe/font/scopes and compare every output signature; commit code/tests and ledger measured tradeoffs.

### Task 3: Final evidence and integration

**Files:** docs/records/variable-match-diagnosis.md, data/variable-match-diagnosis{.json,-raw.json.gz}; native ledger and exact CI proofs.
**Interfaces:** Consumes immutable before/candidate outputs, source hashes, test results; produces an auditable adoption decision.

- [ ] Run counter-free timing warmup then ABBA after local builds finish; deterministic memory repeats and actual hit/miss counter runs separate. Expected: all signatures stable, exact source/config/font/binary hashes, costs split by scope.
- [ ] If shipping Rust changed run workspace/no-default/complex-only/fmt/all-target clippy/docs/fixed snapshots and standard54-case matrix against sbp9-selection-source-final. Expected: all tests/checks pass, all accepted outputs exact, default refusals unchanged.
- [ ] Archive raw data, source overlay patches, font/license and reproducible commands; manifest verifies checksum. State warm/miss/static/retained costs and limitations. Commit final evidence.
- [ ] One fresh whole-branch Astra review; resolve Important/Critical in one RED/GREEN pass, then create PR, all latest-head CI, merge, close issue and remove owned worktree.
