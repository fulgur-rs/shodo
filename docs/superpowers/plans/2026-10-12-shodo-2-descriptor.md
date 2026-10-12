# SHODO-2 descriptor presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove owned descriptor clones used only for presence checks, preserving font selection.

**Architecture:** Add one borrowed bool lookup alongside the unchanged public owned lookup. Keep the existing layered traversal and mutex lifetime. Measure real matching paths with fixed fonts.

**Tech Stack:** Rust 2024, existing shodo fixtures and CountingAllocator.

**Spec:** `docs/superpowers/specs/2026-10-12-shodo-2-descriptor-design.md`

## Global Constraints

- Rust MSRV 1.89; no public API/dependency changes.
- Use `.worktrees/shodo-2`, `~/tmp`, dedicated build output; retain unrelated root files.
- The user's active goal authorizes implementation, PR, CI, merge and closure without a further handoff approval.

## Review Focus

- Invisible sibling/foreign IDs must not alias a local descriptor slot.
- Shared registrations made after document creation must remain visible.
- Descriptor-less slant axes must keep the query style; registered axes must use descriptor style.
- Cache generation changes must select the newly registered face.
- The match-cache hit control bypasses selection and should show no systematic allocation saving.

### Task 1: Borrow descriptor presence, verify selection and measure

**Files:**
- Modify: `crates/shodo/src/font/mod.rs`, `crates/shodo/src/font/matching.rs`
- Test: `crates/shodo/src/font/browser_tests.rs`, `dev/bench/tests/font_descriptor_allocations.rs`
- Create: `dev/bench/examples/font_descriptor_lookup.rs`, `docs/records/shodo-2-descriptor-2026-10-12.md`, `.json`
- Modify: `.github/workflows/ci.yml`, `docs/README.md`

**Interfaces:**
- Consumes: existing `FontId`, `LayerState.descriptors`, parent collection, fixed fixture fonts and CountingAllocator.
- Produces: private `has_face_descriptor(&self, id: FontId) -> bool`; public `face_descriptor` stays unchanged.

- [ ] **Step 1: Write the allocation regression and selection tests.** Eight alternating `a`/`b` matches with match cache capacity 1, a warmed static candidate and 1,024 descriptor ranges must allocate less than 8,192 bytes in total and return the registered FontId with no variations. Add layered visibility/update cases and slant cases at cache capacities 0/1.
- [ ] **Step 2: Run the regression before implementation.** Run `cargo +stable test -p shodo-bench --features allocation-counting --test font_descriptor_allocations -- --test-threads=1`. Expected: allocation-budget assertion FAIL because each selected face clones the 8,192-byte ranges.
- [ ] **Step 3: Implement the borrowed bool lookup and replace the presence-only call.** Same layer reads `Option::is_some` through a borrowed slot; otherwise recurse into the parent or return false. Do not change selection, owned API or catalog update behavior.
- [ ] **Step 4: Verify GREEN and compatibility.** Run the allocation regression, `cargo +stable test -p shodo --lib font::`, then `cargo +stable test --workspace`. Expected: all pass. Add the new allocation test to the existing CI allocation command.
- [ ] **Step 5: Measure and record.** Build identical probe source on baseline and fix in separate release target directories. Alternate at least 7 runs per revision for small prepared, heavy prepared, raw static and repeat-cache-hit cases. Registration/warmup stay outside timed/counted work. Report full samples, medians, allocation counts and identical selection digests; distinguish observed noise from speed claims. Record why candidate flags are or are not warranted.
- [ ] **Step 6: Verify and commit.** Run fmt/diff checks, workspace Clippy with warnings denied, feature tests and snapshots or their CI equivalents. Expected: all pass. Commit only task files and review baseline..head, including the Review Focus above.
- [ ] **Step 7: Integrate.** Create PR, wait for all CI on the reviewed head, merge with expected-head check, update main, close SHODO-2 with results/PR link, and remove this task's scratch/worktrees.
