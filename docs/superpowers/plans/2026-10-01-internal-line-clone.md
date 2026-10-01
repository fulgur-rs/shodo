# Internal line clone implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove unused accepted-Line copies from break_all without changing public iterator results.
**Architecture:** One existing iterator with private const retention policy; public true, internal false. Test-only actual Clone marker; fixed-font standalone example and isolated source overlays for evidence.
**Tech Stack:** Rust, existing harfrust/fixture fonts, stdlib Python, shodo-bench allocator.
**Spec:** docs/superpowers/specs/2026-10-01-internal-line-clone.md

## Global Constraints

User-authorized inline six-issue worktree/PR/CI/merge/cleanup execution. No API/default-limit/dependency changes, switch blockers, saved-tree/protected-file changes. Whole-output oracle and exact warnings/resource behavior; counters excluded from final timing, gross/freed/net/peak/releases separate.

## Review Focus

- Public callback can inspect all previous Line data, not just its token; test that contract.
- Float and block transitions retain exact cursor and accumulated Q26 offset; compare manual driver.
- Saturated/negative/nonfinite widths and offsets retain warnings/order/caps; compare fresh controls.
- Ruby/first-line/edge overlays retain recursive owned geometry; snapshot every public cluster/run field.
- Grapheme0/1 plus resource-budget fallback retain source progress; compare both public paths.

### Task 1: Iterator ownership and resource regression

**Files:** Modify crates/shodo/src/line/iter.rs, crates/shodo/src/output.rs, crates/shodo/src/output/line.rs; create crates/shodo/src/line/iter_tests.rs.
**Interfaces:** Consumes existing Paragraph::lines/next_line and Line factory; produces private Lines const policy, unchanged public signatures and test-only clone marker.
- [ ] Run core baseline; expected all existing tests pass.
- [ ] Add actual derived-clone observation and tests internal_break_all_does_not_clone_lines, public_iterator_keeps_previous_line, grapheme_break_all_does_not_clone_lines; watch internal zero-clone tests FAIL before change.
- [ ] Route internal break_all to false retention and public lines to true on the same driver. Expected all focused/core tests PASS.
- [ ] Commit verified implementation. Task gate: cargo test -p shodo --lib line::iter_tests.

### Task 2: Fixed-font exact before/after evidence

**Files:** Create dev/bench/examples/internal_line_clone.rs; reuse support/ligature_snapshot.rs without weakening fields. Local capture/verify/observer scripts under root target/performance-artifacts/c91-1-line-clone.
**Interfaces:** Consumes unchanged public break_all/lines/next_line; produces full fixed-font output/warning/operation/release samples and immutable baseline/candidate time/memory/observer binaries.
- [ ] Compare original, candidate and fresh manual/public controls for 54 standard operations plus rich Ruby/first-line/shared-edge/forced/block/float/grapheme/resource/offset saturation inputs; exact full outputs/warnings, real-font literal guards.
- [ ] Observe actual clone counts and exclusive clone gross allocation without nested begin scopes; expected internal root clones removed, public previous unchanged. Allocator-only/observer samples must match.
- [ ] Run meaningful verifier, then final balanced counter-free processes only after builds/checks. Expected exact output equality and measured adoption decision.
- [ ] Commit example and self-contained compact manifest/raw archive. Task gate: python3 root-target/c91-1-line-clone/verify.py.

### Task 3: Validation and measured record

**Files:** Create docs/records/internal-line-clone.md, data/internal-line-clone.json and -raw.json.gz.
**Interfaces:** Consumes task2 pinned evidence; produces complete AC proof, fresh whole-branch review package and latest-head CI PR gate.
- [ ] Run core, workspace alltarget Clippy/fmt/docs, allocator/probe, fixed snapshots and required standard54 controls; expected PASS without snapshot updates.
- [ ] Independently verify archive source/fonts/binaries/raw bytes/all table values, including adverse timing/retention cases; commit final record.
- [ ] One fresh Astra whole-branch review; fix Important/Critical with failed-then-passing controls and final proof. PR/latest CI/merge/main/protected/saved/bd readback/owned cleanup follow user authorization.
- [ ] Task gate: python3 root-target/c91-1-line-clone/final_proof.py.
