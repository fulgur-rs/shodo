# Float Integration Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-p2m.12 with a real caller/Taffy driver, complete checkpoint examples and automated numeric/paint evidence.
**Architecture:** Dev-only shared example support; immutable input checkpoint, provisional output, real Taffy placement and replay on withdrawal. No core or raikiri spike changes.
**Tech Stack:** Rust 1.89+, shodo-fixtures, Taffy 0.14 float_layout, existing TinySkia/Skrifa painter.
**Spec:** docs/superpowers/specs/2026-09-27-float-harness-design.md

## Global Constraints

Keep root dependencies and library API unchanged. Taffy dev-only. No copying Parley or unmerged S4 source. Build outputs use the workspace ignored target, not /tmp. Keep S4 unmerged; normal PR/CI/merge is authorized. One final whole-branch review.

## Review Focus

1. Reverse withdrawal preserves earlier floats: numeric F1/F2 regression.
2. Height/lookahead rollback leaks cursor or pending state: complete checkpoint and replay comparisons.
3. Slot geometry ignores full line height: tall atomic/short float band regression.
4. Paragraph cursor reset accidentally drops the shared BFC: two-paragraph numeric test.
5. Page width changes leave right float at old x or cause infinite oversized retries: continuation and oversized regression.

## Tasks

- [x] Add `tests/float_flow.rs` with basic left/right/clear and line-retry tests, observe missing-driver failure. Add Taffy dev dependency and implement support checkpoint, validation, placement, slot and trial. Run focused tests.
- [x] Add tab/unbreakable/one-by-one withdrawal, pending and oversized cases with independent numeric expectations. Implement retry/withdrawal/replay and geometry candidate policy; record observed results and bounds.
- [x] Add height rejection, nonempty checkpoint, preview prefix commit/discard, widow/orphan caller policy, page continuation, paragraph BFC handoff and block-boundary tests. Implement corresponding methods and document full saved state.
- [x] Add `examples/float_png.rs`, a real-font glyph/float rendering check and documentation/README commands. View generated PNG and preserve its checksum.
- [ ] Run stable/MSRV workspace, all-target Clippy, docs, fmt and diff checks. Review full branch once. Push, PR, verify exact HEAD CI check/msrv/wasm success, merge, read back, close issue and clean owned worktree/branch.

## Verification commands

Use `cargo +stable test --offline -p shodo-fixtures --test float_flow` for focused cycles. Final `cargo +stable test --offline --workspace`, `cargo +1.89.0 test --offline --workspace`, `cargo +stable clippy --offline --workspace --all-targets -- -D warnings`, `cargo +stable doc --offline --workspace --no-deps`, `cargo +stable fmt --all -- --check`, `git diff --check`. All builds use one job and the existing root target caches. Save command logs under /tmp/shodo-float-harness-*.

## Local execution record

2026-09-27: 29 focused tests, including the144-case float matrix; stable/MSRV1.89 workspace477 each; Clippy/docs/fmt/diff pass. Fixed-font PNG generated and viewed, SHA256 e8aa5e77eb198f55a8c3b3b4b117d0bed81b24e5bd88ed0ff3655f87f2b9a9dc. Final review/PR/exact-HEAD CI/merge/cleanup remain the integration gate. External command and execution logs: /tmp/shodo-float-harness-*.

Review correction: source-aware geometry deferral prevents a middle float from staying above earlier inline when moving the same line. The corrected midword oracle, legitimate head/indent placement, zero-width/atomic prefixes, alternate first-line offsets, mapping-off explicit metadata, reverse geometry deferral/source flush order and repeated candidate movement are covered. Focused27 tests pass; final full gates are rerun after the correction.

Empty painted inline edges are also covered through explicit SourceEdge metadata and an error boundary when unavailable; focused29 pass before the final complete verification.

Final corrected verification: stable/MSRV workspace477 each, all-target Clippy/docs with warnings denied/fmt/diff/PNG all exit0; unchanged PNG SHA e8aa5e77eb198f55a8c3b3b4b117d0bed81b24e5bd88ed0ff3655f87f2b9a9dc. Same final reviewer rechecks the fix commit before integration.
