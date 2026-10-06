# Annotation space implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose per-line annotation overflow and unused leading for caller-owned spacing.

**Architecture:** Capture an emphasis-free selected profile on accepted lines. Cache side metrics from accepted font-content and shared retained ruby edges; expose six fixed-size values without changing standalone layout or candidate probes.

**Tech Stack:** Rust workspace and fixed-font harness.

**Spec:** `docs/records/shodo-q57-annotation-space.md`

## Global Constraints

- Japanese responses; English code comments and record.
- Dedicated worktree based on jw6; isolated Cargo target, jobs 2, `TMPDIR=~/tmp`.
- No automatic spacing collapse, candidate rescans, new dependencies, or new browser results.
- Preserve first-line, retained cache capacity, fixed-point and writing-mode contracts.

## Review Focus

- Negative leading must not become reusable space or ordinary font overflow become annotation overflow.
- Mixed fonts and top/bottom alignment must consider accepted content, not only root edges.
- Hidden ruby still reserves geometry; collapsed ruby and opposite-side marks remain separate.
- Vertical-lr swaps line-over/under relative to logical block-start/end.
- Rejected-line reuse and repeated metrics access must preserve results with bounded work/storage.

---

### Task 1: Capture and publish annotation geometry

**Files:** `crates/shodo/src/{lib.rs,output.rs,output/annotations.rs,output/line.rs,line/metrics.rs,line/mod.rs,ruby/geometry.rs,ruby/place.rs}`;
`crates/shodo/tests/annotation_metrics.rs`; `dev/harness/tests/annotation_metrics.rs`.

**Interfaces:** `Line::annotation_metrics() -> AnnotationMetrics`; fields
`unannotated_block_start`, `unannotated_block_end`, `overflow_over`,
`overflow_under`, `space_over`, `space_under`, all `f32`.

- [x] Add contract tests: synthetic 40px/10px plain space 15/15, over marks 10/15;
  4px emphasized line bare box 8..12 and overflow 8/0; adjacent 14px marked
  lines expose 3px overflow and 2px opposite space so caller can borrow 2px.
- [x] Run focused tests and confirm the missing public API fails compilation.
- [x] Implement emphasis-free accepted profile, fixed geometry capture and public API;
  include atomic/combined text, hidden/nested ruby, accepted displacements and em trimming.
- [x] Add/run real-font tests for scratch dimensions, vertical reflection, large
  leading, first-line retries, mixed/top/bottom content, and hidden/collapsed ruby.
- [x] Add/run retained budget and repeated-query/scaling module tests.
- [x] Run default workspace tests, fmt, default/accesskit Clippy with warnings denied,
  and warnings-denied workspace docs; record evidence and remove owned raw logs.
- [x] Commit the verified branch and hand it to the parent for review/PR/CI/merge.
