# Font Data Reuse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-sbp.12 with separately attributable data/metric reuse and conditional preliminary instance construction.
**Architecture:** Borrow existing FontData for run metrics through private shared metric helpers; avoid repeated data acquisition and parse. Independently make the preliminary coordinate instance conditional on size-adjust. An external probe preserves baseline/A/AB sources and executables, with separate timing/allocation/trace modes.
**Tech Stack:** Rust stable1.97.1 release, existing CountingAllocator, Python3, CPU10.
**Spec:** docs/superpowers/specs/2026-10-01-font-data-reuse.md

## Global Constraints

No public API/dependencies/default limits/fixture/cache expansion; preserve source/glyph/geometry/warnings, metric values, variation/opsz/MVAR/size-adjust/generation/retained old data. Saved spikes unchanged; performance never blocks raikiri/S4. Separate time/gross/net/peak; RSS unmeasured. Existing sequential PR/latest-head CI/merge/close/cleanup authorization applies. Use a repository TMPDIR for local validation after the prior /tmp linker failure.

## Review Focus

- Shared root stub uses its documented metrics; a real document face at index0 must not be mistaken for it.
- Invalid sizes/unknown faces and public default metrics retain existing behavior.
- MVAR and vertical metric signs/units preserve literal values at nondefault coordinates.
- Automatic opsz uses adjusted size; explicit opsz and VVAR/ic-height adjustment preserve pre-adjust locations.
- Old FontData/matches survive root/document generation changes; no cache retention growth.

### Task 1: Baseline attribution

**Files:** target/performance-artifacts/sbp12-font-probe/{src/main.rs,src/allocator.rs,capture.py,verify.py,Cargo.toml,fonts/}; spec/plan.
**Interfaces:** Produces immutable before source/binaries/raw and actual lock/Blob-clone/parse/instance events consumed by A and B.

- [ ] Confirm font_data/state/shaper_data and metric callsites; identify exactly which locks/clones/parses the counter observes. Preserve trace-only source overlays and do not infer release work solely from trace counts.
- [ ] Reuse pinned Latin/CJK/Roboto Flex and licenses from prior checked artifacts. Probe warm public builds with one/many short styles, horizontal/upright, explicit axes/opsz, size-adjust and generation controls; full run metrics join source/glyph/geometry/coordinate/warning signatures.
- [ ] Capture immutable counter-free time, allocator-only and trace-only before executables. Run verify.py before. Expected: fixed sources/font/registry/harness, positive actual counters, deterministic allocations and literal metric/input guards pass.
- [ ] Commit spec/plan with attribution and candidate scope.

### Task 2: Candidate A — acquired data for metrics

**Files:** crates/shodo/src/font/metrics.rs, font/mod.rs test instrumentation, shape.rs and focused metric/shape tests.
**Interfaces:** Consumes before signatures; produces candidateA frozen source/raw/binaries with unchanged metrics and fewer data acquisitions.

- [ ] Write actual warm shape_items data-acquisition regression with itemization/matching outside scope and independent literal glyph/metric guards. Expected RED: duplicate font_data acquisitions versus one intended acquisition (settle exact observed count from the real boundary and ledger it).
- [ ] Extract pure horizontal/vertical calculations, preserving all existing formulas. Add private combined metrics method using already acquired FontData and one parse, with valid_size/shared-root-stub behavior intact; shape_items consumes it. Public methods use the same pure calculations without public signature changes.
- [ ] Test root stub, real document face index0, invalid-size/unknown face, literal MVAR/vertical corrections and retained-generation data. Run focused shape/browser/metric tests. Expected: resource GREEN and existing literal metric behavior unchanged.
- [ ] Capture A with identical probe; verify before→A exact outputs and actual avoided locks/clones/parses, disclose all scoped costs. Commit A and tests. If contribution is immaterial or contracts cannot be preserved, ledger no-adoption instead of pretending a speed improvement.

### Task 3: Candidate B — lazy preliminary instance

**Files:** crates/shodo/src/shape/instance.rs and focused tests.
**Interfaces:** Consumes frozen A (or baseline if A rejected); produces candidateAB final source/raw, constructor count comparison and required validation logs.

- [ ] Observe actual preliminary/final constructor calls in a resource regression. Expected RED no-adjust2 versus1; adjusted case keeps2 and literal varied coords/size.
- [ ] Move preliminary construction into font_size_adjust branch; final variation/opsz/clamp order unchanged. Record limitations if trace observability differs from optimizer behavior, using counter-free allocations/time as release evidence.
- [ ] Capture AB and verify A→AB independently, including literal VVAR/ic-height and explicit/automatic opsz. Run focused tests plus workspace/no-default/complex/fmt/all-target clippy/docs/fixed snapshots on final frozen source. Expected: all positive-count checks pass and every signature/metric contract matches.
- [ ] Commit B/tests and scoped adoption decision; no source changes after final captures unless Important/Critical fix requires recapture.

### Task 4: Evidence and integration

**Files:** docs/records/font-data-reuse.md, data/font-data-reuse{.json,-raw.json.gz}; native ledger.
**Interfaces:** Consumes all immutable states and verified checks; produces audited per-component results and integration evidence.

- [ ] Standard54 control versus sbp11-single-pass-features; after all compiler work, counter-free warmups and balanced before/A/AB processes. Expected: all dedicated and standard output/refusal signatures exact, metrics/limits/generation unchanged; actual counters and allocations separate from time. Preserve noisy/outlying runs and limit CPU conclusions.
- [ ] Archive every state/source/overlay/command/font/license/raw/check; reconstruct source fingerprints and verify checksum. Report before→A and A→AB contribution and time/gross/net/peak separately. Commit evidence and run final proof.
- [ ] One final fresh whole-branch Astra review, one Important/Critical RED/GREEN fix pass, Minor deferred. User-authorized PR/latest-head CI/merge/close and clean owned worktree removal are branch finishing gates after evidence verification.
