# Ruby Candidate Ranges Implementation Plan

> For agentic workers: REQUIRED SUB-SKILL: superpowers:executing-plans. Execute natively inline in the existing isolated shodo-sbp5 worktree; user authorized implementation, PR, CI, merge and issue close.

**Goal:** Bound candidate measurement work by the selected columns and lanes.
**Architecture:** Binary queries over prepared order; stage lane filtering before compact column arrays. Original unit coordinates and lane IDs stay global.
**Tech Stack:** Rust, fixed-font shodo-bench probe, Python artifact packaging.
**Spec:** docs/superpowers/specs/2026-09-30-ruby-candidate-ranges.md

## Global Constraints

Preserve source/glyph/geometry, legal paired cuts, default limits, first-line and BreakToken, empty/spanning/multilevel/nested ruby, merge's original first-lane font cap, overhang and RTL inter-character placement. No retained PreparedRuby index or switch gate. Separate build/layout and normal timing/instrumented allocations; report gross/net/peak separately.

## Review Focus

- Empty or boundary-only selections: preserve fallback bounds and do not index empty arrays.
- Spans outside the selection: maintain original global lane IDs in paired cuts.
- Merge with a different font in the first lane outside the window: preserve its ic cap.
- RTL inter-character rightmost column and nested ruby: local indices must map to original metadata.
- First-line and BreakToken continuation: all offsets must survive resumed layout.

## Task 1: Bound lane selection

**Files:** ruby/measure.rs; ruby/tests/measure.rs; test-only context counters; external target/performance-artifacts/sbp5-probe.
**Interfaces — Consumes:** PreparedRuby.columns, lanes, cuts in original coordinates.
**Interfaces — Produces:** selected_columns(ruby, units) -> Range<usize>; selected_lanes(ruby, columns) -> iterator of global usize lane IDs. Full column arrays remain global in this stage.

- [ ] Create fixed-font build/fresh-layout probe at C=16/64/256/512; save immutable original binaries and raw allocation/timing output.
  Expected: all columns/content retained, complete output digests, default limits pass.
- [ ] Add test-only lane visit counter and a many-column short-window regression. Run cargo test -p shodo --lib ruby::.
  Expected: old loop fails the bounded-visit assertion; existing semantic tests pass.
- [ ] Implement ordered column and lane queries and use global lane IDs for measurement and inter-character range collection.
- [ ] Run cargo test -p shodo --lib ruby::.
  Expected: regression and ruby semantics pass.
- [ ] Save intermediate probe binaries/results and commit lane selection.

## Task 2: Compact selected column arrays

**Files:** ruby/measure.rs, place.rs, geometry.rs, overhang.rs; ruby/tests/measure.rs.
**Interfaces — Consumes:** Task 1 global selected-column range and global lane IDs.
**Interfaces — Produces:** RubyFragmentMeasure.column_start: usize; bases/base_widths/base_columns/cross_columns/columns and right_columns use local indices; original metadata uses column_start + local.

- [ ] Add bounded-column-work regression and empty/nonzero-offset tests. Run cargo test -p shodo --lib ruby::.
  Expected: old full-size column work fails; existing semantic cases pass.
- [ ] Compact selected columns and convert every consumer consistently. Keep source-unit and neighbor lookup coordinates global.
- [ ] Run cargo test -p shodo --lib ruby::.
  Expected: bounded-work and semantic regressions pass.
- [ ] Commit compact candidate arrays.

## Task 3: Validate, measure and integrate

**Files:** docs/records/ruby-candidate-ranges.md and data JSON/raw gzip; plan/spec.
**Interfaces — Consumes:** Completed lane/column optimization and original/intermediate probe artifacts.
**Interfaces — Produces:** Reproducible A/B evidence and merged PR linked from beads.

- [ ] Run workspace, no-default and complex-only tests, fmt, clippy, docs, snapshot verification.
  Expected: all required checks pass.
- [ ] Run final probe and 54-case standard runner against sbp4; compare all source/glyph/geometry digests for timing and allocation outputs.
  Expected: exact output equivalence; build/layout gross/net/peak and timing separately recorded, no unmeasured claim.
- [ ] Record evidence and limitations and commit the record.
  Expected: artifacts verify all collected digests, source fingerprint and successful validation logs.

The native execution final review follows Task 3: request one fresh whole-branch review with plan/spec/ledger, resolve Critical/Important findings, then create PR, wait for every CI check, merge matching reviewed head, close beads and remove only this worktree/branch. User already authorized these integration steps.
