# Retained raikiri caller reuse implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement inline task-by-task. No implementation agents; one final Astra whole-branch review.

**Goal:** Verify the four caller reuse paths with exact fresh output and explicit invalidation.
**Architecture:** Keep the real CSS/DOM walker and link projection. Split preparation and output assembly, then add an owned immutable-input development session with context and prepared paragraph retention. Core/public APIs and saved S4 spikes are untouched.
**Tech Stack:** Rust2024/MSRV1.89, existing shodo/raikiri fixtures, stdlib Python archive.
**Spec:** docs/superpowers/specs/2026-10-01-retained-caller-reuse.md

## Global Constraints

Use original limits/font pins and source/glyph/geometry; no native geometry double shape. No production/S4 switch blocker. Counter-free time separate from gross/net/peak allocation and actual shaper calls; no RSS/frame claim. Shared/document font generation and input/content/style/first-line changes invalidate. Preserve original fresh wrapper as archived independent oracle.

## Review Focus

- Font identity and both generations: rebuild after registration in either layer, including equal numeric generations from a different collection.
- First-line boundary at new width: preserve transformed mapping and accepted-line source links.
- Height rejection: retry the same token; oversized first-page line must progress.
- Token lifecycle: old token rejected after input/font rebuild; fresh paragraph ID differs legitimately.
- Scratch and prepared ownership: separate setup/operation/release scopes and report retained costs.

### Task 1: Extract prepare/output without changing the fresh caller

**Files:** dev/raikiri/examples/support/raikiri_contracts.rs; frozen original in target/performance-artifacts/sbp14-caller-reuse/original-caller.rs.
**Interfaces:** prepare(input,context,fonts,policy)->PreparedParagraph; output(lines,sources)->Output. Existing layout/layout_with_font_policy signatures and behavior remain.
- [ ] Preserve original source and run existing raikiri_contracts tests against merged baseline.
- [ ] Extract owned paragraph/source map preparation and accepted lines output assembly.
- [ ] Run existing contract tests and fmt; no mirrored new extraction tests.
- [ ] Commit and record native task evidence.

### Task 2: Owned retained session and invalidation

**Files:** dev/raikiri/examples/support/retained_caller.rs; dev/raikiri/examples/retained_caller.rs; dev/raikiri/Cargo.toml.
**Interfaces:** session owns immutable ResolvedInput, explicit cloned shared/document collections and LayoutContext; replace input clears prepared state; width/height layout uses existing preparation/output and next_line.
- [ ] Add genuine behavioral RED tests for identity reuse, width/fresh source+glyph+geometry+links, height reject/retry/progress and token invalidation.
- [ ] Implement session with actual shared/document generation stamps and immutable input replacement.
- [ ] GREEN tests for content/style/first-line, shared/document registration and collection identity changes; old token rejects and unchanged prepared ID remains.
- [ ] Commit and record native gates.

### Task 3: Four-path measurements and evidence

**Files:** diagnostic example/support measurement harness; docs/records/retained-caller-reuse.md; docs/records/data/retained-caller-reuse{.json,-raw.json.gz}.
**Interfaces:** fresh context+paragraph, reused context+fresh paragraph, retained width and retained height conditions; original fresh module independent oracle; shape hooks only in disposable overlay.
- [ ] Fixed actual fonts small/long style/link/first-line conditions, full accepted output and token continuation oracle.
- [ ] Separate immutable timing/memory/shape-trace binaries; prepare/operation/release scopes and balanced raw samples.
- [ ] Verify source/font/harness fingerprints, full output equality, actual shaper calls and all measured medians/allocations; record scoped adoption and retention costs.
- [ ] Relevant caller tests, workspace fmt/clippy/docs and required checks; core standard digest only if source changed. Commit evidence.
- [ ] One fresh Astra review, exact latest-head CI, merge, close and owned cleanup before next issue.
