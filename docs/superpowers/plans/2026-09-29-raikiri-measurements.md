# raikiri measurements implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for native inline execution under the existing autonomous goal. No implementation delegation.

**Goal:** Complete the original `shodo-8ei` time and memory comparison without merging or altering preserved integration spikes.

**Architecture:** A SHA-guarded disposable S4 archive supplies actual candidate/native dependencies. Existing measurement primitives separate timing and allocation builds. Probe validation precedes sampling and preserves original inputs and explicit gaps.

**Tech Stack:** Rust, the pinned raikiri and S4 dependencies, existing allocation counter, Python orchestration.

**Spec:** `docs/superpowers/specs/2026-09-29-raikiri-measurements-design.md`.

## Global Constraints

- Preserve S4v2 `fe67a281210fbc52d22032911ac3584405aa8198` and original S4 `3ef5acf879004f9cd76745666187d420ce5aa285`; no push or merge.
- WPT `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8`, original 88 bundled fonts and six generic-family orders, no system fonts.
- Rust floor 1.89; explicit local `cargo +stable`; protected Cargo.lock remains unchanged.
- No raster or serialization in layout scopes; no counting allocator in timing build; unsupported/failed operations do not count as completed comparisons.
- Preserve every original acceptance clause; stage-one validation alone does not finish the issue.

## Review Focus

- Source or input drift must fail before accepting benchmark results.
- Layout-only extraction must preserve actual block geometry for all 121 original supported pages.
- Height rejection must restore source and layout state, including a final accepted line and Done.
- Retained ownership differences must be explicit, including context and output release scopes.
- Incompatible operations, instrumented timings, absent outputs and unsupported pagination must never produce comparison ratios.

## Task 1: Validated layout-only boundary

Files: `dev/bench/tools/raikiri_overlay.py`, `dev/bench/raikiri_probe/layout_check.rs`.

Interface: `prepare(spike: Path, destination: Path, *, expose_layout_boundary: bool) -> dict` creates a new disposable archive and external Cargo probe; `expose_layout_boundary=False` is a baseline TDD check, never measurement. Generated `layout_candidate_screen_page(document, fonts, viewport, limits) -> Result<Vec<CandidateBlock>, IntegrationError>` borrows the original private layout code; it is absent in the unmodified baseline.

- [x] Write the real WPT block/resource/font validation CLI against the new entry point.
- [x] Build it against an unchanged archived S4: observed E0432 missing-entry-point compilation failure.
- [x] Implement guarded extraction and build the same CLI successfully.
- [x] Replay all 121 supported pages / 332 blocks: exact original node/origin/size/width/height/line counts and matching original resources, warnings and 88 fonts. Five actual CLI negatives reject changed pin/font/resource/geometry and empty selection. Evidence is in `target/8ei-artifacts`; this is correctness validation, not a timing result.

## Task 2: Real operations and exclusive measurement scopes

Files: `dev/bench/raikiri_probe/{main,core,caller,scope,isolated,paged,library,observer}.rs`, guarded library overlay, existing allocator imported by path in the generated probe, no duplicated allocator.

Interfaces: explicit operation records for initial preparation/layout, retained-shape width reuse, complete-caller changed-width re-entrance and real rejected/retried flow; scope records carry either uninstrumented nanoseconds or allocation counts. Digest output is computed outside scopes. Prepared owners remain alive until named release scopes.

- [x] Add actual native/candidate source-completeness and retry restoration tests; run RED before implementing operations. Real retained-shape contracts cover all 121 original documents / 2070 width rows. Actual initial preparation plus reuse/retry CLI and its missing-operation RED are retained in `target/8ei-artifacts`.
- [x] Implement each declared boundary using real preserved public APIs; retain unrepresented input/fragmentation as explicit gaps. Actual provisional FlowDriver/native pagination produces 19 paired successes but zero matching width/partition pairs; unsupported and incompatible rows are excluded and tracked in shodo-tmr. Separate SHA-guarded initial library observers preserve original algorithms and fresh source/glyph output; preparation, actual library construction and release are disjoint, with different API/input semantics explicit.
- [x] Verify fresh preparation equals the corresponding reused output for each engine; measure initial shaping rather than cloned shapes. Both engines pass 121-document fresh-preparation versus retained-shape source/glyph snapshots, and complete-caller widths 400/1200/restored800 match fresh-width block outputs. Native mutable-layout copies have their own setup scope, never initial shaping. The native initial text boundary includes real DOM/text preparation beyond the paired leaf roots; this difference remains explicit.
- [ ] Build separate time/memory binaries and record real cold/warm repetitions, allocation/peak/retained/release counts for each successful operation and both isolated/complete-caller boundaries.

## Task 3: Validated comparison and evidence

Files: `dev/bench/tools/{raikiri_measure,test_raikiri_measure,raikiri_library_measure,test_raikiri_library_measure}.py`, `docs/raikiri-measurements.md`, compact qualified report under `docs/data`.

Interfaces: strict report validation and operation-paired comparisons; raw evidence stays in a uniquely published output directory.

- [x] Test rejection of incompatible input, instrumented timings, missing scopes, shorter accepted output and unsupported pagination using real collected result records. Fifteen whole-caller and six library validator tests pass; complete existing bench Python suite is 40 tests. Actual compressed fixtures preserve collected JSON unchanged.
- [x] Implement orchestration, provenance, validation and summaries with raw samples and uncertainty; retain failure logs. Whole-caller collection completed 4710 successful processes and an independent hash/owner audit. Initial library collection runs separately, shares existing build/profile/layout validation, saves its independent output references and diagnostic log, and retains individual peaks without summing incompatible windows. Final library sampling/audit and combined qualified report remain required.
- [ ] Inspect actual regressions, reproduce where needed, and register reasoned followups and switching necessity.
- [ ] Audit every original acceptance clause against actual artifacts. Keep the issue open for any missing measurement; do not substitute a stage report.
- [ ] Run appropriate repo checks, single fresh final review, exact-head CI; merge only the normal measurement PR, close the issue, and remove only its owned worktree after remote automatic deletion.
