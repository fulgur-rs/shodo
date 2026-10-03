# shodo-zb0.4 Ruby Base Decoration Ancestors Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans natively in the issue worktree. The authorized issue workflow includes PR, green CI, merge, issue close, and owned worktree cleanup.

**Goal:** Remove the temporary ancestor `Vec`s and quadratic `contains` scans from Ruby base edge accounting without changing measured widths.
**Architecture:** Keep a scalar ancestor-path handle (first box plus lazily cached depth), align two parent chains by depth to find their lowest common ancestor, then sum only the shared Clone chain starting from `InlineBoxInfo::nearest_clone` at that ancestor. Preserve the normal `decoration::width` path unchanged.
**Tech Stack:** Rust; fixed-font `shodo-bench` timing/allocation probes and test-only ancestor-visit counters.
**Spec:** Beads issue `shodo-zb0.4` description and acceptance criteria (`bd show shodo-zb0.4`).

## Constraints

- Preserve the legacy membership rule: an edge contributes only when the boundary ancestor is also in the `scope.start` ancestor path and has `box-decoration-break: clone`.
- Preserve close-unit ownership, Clone/Slice mixtures, first-line resolved styles, signed edges, per-edge rounding, inner-to-outer saturating addition order, source/glyph data, and final geometry.
- Keep the existing nearest-clone iterator used by ordinary line decoration width unchanged.
- Report ancestor visits separately from allocations and timing.

## Review focus

- A close boundary starts at the closing box itself; it must participate in LCA selection.
- Boundary chains are inner-to-outer. Shared nodes form one suffix; only the suffix beginning at the lowest common ancestor is eligible.
- Start/end edge values must be rounded and added independently in their original order. Compare full `Saturation` values as well as raw width.
- First-line `ParagraphData` has separately resolved styles; do not read normal-line style tables.
- Deep mixed Clone/Slice with multiple Ruby bases must avoid ancestor-vector allocations and reduce membership work without changing snapshots.

### Task 1: Oracle, boundaries, and visit counters

**Files:** `crates/shodo/src/line/decoration.rs`; tests near Ruby base measurement.

- [x] Add a legacy vector-plus-`contains` oracle for common Clone ancestors.
- [x] Compare a constant-space helper against it for nested, mixed Clone/Slice, text/close, sibling, and first-line boundaries.
- [x] Pin signed-edge saturation across both selected edges and verify parent-link reads against legacy chain and membership work.

### Task 2: Fixed-font workload and output comparison

**Files:** `dev/bench/examples/ruby_base_decoration.rs`; support fixture and allocation integration test.

- [x] Construct deep mixed inline boxes around multi-column Ruby with fixed fonts.
- [x] Measure layout time and allocations separately from fixture/font setup; snapshot warnings, sources, glyphs, and geometry.
- [x] Capture baseline and candidate results from identical probe sources and separate targets.

### Task 3: Replace Ruby base chain materialization

**Files:** `crates/shodo/src/line/mod.rs`; `crates/shodo/src/line/decoration.rs`.

- [x] Use reusable scalar ancestor paths and lowest-common-ancestor alignment in `ruby_base_width`.
- [x] Leave `decoration::width` intact and preserve per-edge conversion/saturation order across both boundaries.
- [x] Keep the implementation after parent-link work, allocation counts, timing medians, and output snapshots were compared.

### Task 4: Validate and complete the issue

- [x] Run focused edge/oracle tests, allocator regression, workspace tests, clippy, and formatting.
- [ ] Review the branch, open PR, wait for all required CI checks, merge, close the Beads issue, and delete only its owned worktree/branch.

## Execution ledger

- `Hypothesis:` deepest common ancestry can be found in O(depth) time and O(1) additional space by aligning parent chains by length; `nearest_clone` can then enumerate the same shared Clone boxes inner-to-outer.
- `Risk:` advancing the wrong chain or starting at the LCA's parent drops the close/current box; mitigate with close-boundary differential tests and signed-saturation equality.
- `Saturation correction:` update the running excluded width edge-by-edge across start then end boundaries; separately saturating each boundary subtotal changes signed cancellation results.
