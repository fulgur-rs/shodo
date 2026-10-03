# shodo-zb0.3 Borrowed Base Stops Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans natively in the issue worktree. The authorized issue workflow includes PR, green CI, merge, issue close, and owned worktree cleanup.

**Goal:** Remove per-annotation copies of parent caret stops from `AnnotationIndex` while preserving all hit results.
**Architecture:** Store `parent_line` and a `Range<usize>` into that line's `LineIndex::stops`. Borrow that slice from the owning `LineLayout` when selecting the nearest base caret; filter hidden annotations before computing or storing the range.
**Tech Stack:** Rust; fixed-font `shodo-bench` timing and allocation probes.
**Spec:** Beads issue `shodo-zb0.3` description and acceptance criteria (`bd show shodo-zb0.3`).

## Global Constraints

Preserve closed base-range endpoints, the exact nearest-caret distance calculation, tie-breaking by affinity, nested hit selection, and all source/geometry results. Do not store references into the owning `LineLayout` or change public APIs. Measure layout construction time and allocations with multiple levels, long bases, and hidden annotations.

## Review Focus

- Annotation ranges include stops at both endpoints; test offsets equal to range start and end.
- Equal-distance stops retain current affinity tie-breaking; compare borrowed selection with the previous owned-slice rule.
- Empty ranges still yield no base caret; include a range with no matching stop.
- Nested entries use the parent line's own stop range; test multiple levels and base-source mapping.
- Hidden annotations do not copy stops or recursively build their hidden reading; measure a long hidden base.

---

### Task 1: Pin range semantics and build measurements

**Files:** `crates/shodo/src/ruby/hit.rs`; `dev/bench/examples/ruby_base_caret_range.rs`; `dev/bench/tests/ruby_base_caret_range.rs`.

**Interfaces:** Use the existing finalized `LineIndex::stops`, `RubyAnnotationView::base_text_range()`, and `CountingAllocator`; the probe reports `LineLayout::new` timing and allocation results for visible depths and a hidden long-base case.

- [x] Add tests for inclusive stop selection, empty range, nearest distance, and equal-distance affinity.
- [x] Add a fixed-font workload with long bases at depths 1, 4, and 8, plus a hidden depth-4 case; measure `LineLayout::new` only.
- [x] Run the new tests and capture baseline timings/allocations before changing production code.

### Task 2: Borrow the parent stop range

**Files:** `crates/shodo/src/hit/mod.rs`; `crates/shodo/src/ruby/hit.rs`.

- [x] Change `AnnotationIndex` storage from `Vec<Caret>` to `Range<usize>` and pass the parent line's borrowed stop slice to nearest-caret selection.
- [x] Skip hidden annotations before range computation; retain the existing `< range.start` and `<= range.end` partition points.
- [x] Run Ruby hit tests and allocator probe; compare every pinned hit result to the previous rule.

### Task 3: Measure, record, and validate

**Files:** `docs/records/shodo-zb0-3-borrowed-base-stops.md`; CI allocator-test command if needed.

- [x] Run counter-free release timing and separate allocation counting on the same fixed-font workload, before and after.
- [x] Record construction time, allocation calls/bytes, hidden-case evidence, and the hit-equivalence checks. Keep the optimization only if the change is measurable.
- [ ] Run workspace checks, review the full branch, open PR, wait for green CI, merge, close the Beads issue, and remove only the owned worktree/branch.

## Execution ledger

- `Ruling:` use the same hidden Ruby fixture on baseline and candidate instead of a plain-line control. The plain line drops the bidi isolates and changes `LineIndex` allocations, so its delta cannot isolate the caret copy; if this ruling were wrong, the allocation threshold could be attributed to unrelated line-index work.
- `Ruling:` use 128 base characters for timing across depths 1/4/8 and retain a 256-character depth-4 hidden allocation test. A 256-character depth-8 fixture exceeds the default reshape-window budget and emits warnings, which would confound the requested layout comparison.
- `Deviation:` a candidate smoke timing ran before the paired baseline capture. The reported A/B uses a disposable archive of `origin/main` plus the same probe source and fixture, separate targets, and both fixture orders; the reported baseline code is unchanged.
- `Ruling:` adopt the range borrow because the two orderings show a small 1.0–1.7% construction-time reduction and it removes 1/4/8 allocation calls plus 8,224/32,896/65,792 bytes at visible depths 1/4/8. The 256-character hidden case removes one caret allocation per layout. The hit comparator and endpoint selection remain equivalent under tests.
