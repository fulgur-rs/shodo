# Borrow edge input implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Remove unedited edge scalar/context/metadata allocations while preserving exact shaping and owned edits.
**Architecture:** Private lazy ShapeInput views feed the existing shaping core. Stack UTF8 contexts and conservative compatible-item merge detection preserve original metadata and boundaries.
**Tech Stack:** Rust, existing fixed fonts/full-output bench and stdlib Python evidence tools.
**Spec:** docs/superpowers/specs/2026-10-01-borrow-edge-input.md

## Global Constraints
Public APIs/default limits/warnings/source/glyph/geometry unchanged. Edge context exactly5 scalars and at most20 UTF8 bytes; no retained borrowing. Existing owned replacement/font/merge behavior, budgets/base scopes/scratch/pen/cursor unchanged. User-authorized native sequential worktree/PR/CI/merge/cleanup; no switch blockers; protect originals and saved trees.

## Review Focus
- Mid-item UTF8/combining cuts keep scalar offsets/end/item/grapheme flags and5-scalar pre/post context.
- Adjacent compatible original items still join; replacement/font substitution retains old ownership/GPOS/text-end behavior.
- Missing fonts/giant grapheme/run-byte/glyph/window budgets keep exact progress and warning order/cap/suppression.
- Script/bidi/lang/features/variation/orientation/first-line/Ruby metadata stays identical through the shared core.
- Removing temporary allocations can alter allocator history; fresh and sequential adverse cases must remain visible.

### Task 1: Borrowed shaping input and regression contracts
**Files:** Modify crates/shodo/src/shape.rs; add focused shape input helper module if clarity requires; extend shape/tests.rs.
**Interfaces:** Existing shape_items/shape_items_with_base_scopes/shape_window_edit signatures stay unchanged; private ShapeInput borrows metadata/scalars and carries borrowed/stack contexts.
- [ ] Add unedited_edge_windows_borrow_source_scalars: real and missing Latin, run budgets1/1024, clipped text5..31, actual Scalar clones0, exact clusters5..31 and original pointer/length/end preserved. Watch original fail with26 real scalar clones.
- [ ] Implement private view iteration,20-byte context and can-borrow predicate. Only unedited/no-merge input uses views; keep old owned path.
- [ ] Check literal multibyte context boundaries, direct owned edited/merged oracle, glyph/run budgets and saturation/warnings. Existing scalar-budget, complex-script/variation and first-line/Ruby controls remain.
- [ ] Run core/fmt/alltargetClippy and commit verified change. Gate: cargo test -p shodo --lib.

### Task 2: Independent fixed-font work/allocation/time
**Files:** dev/bench/examples/edge_input_borrow.rs; target/performance-artifacts/c91-3-borrow-edge-input capture/verify/codec recipes; docs/records/data evidence.
**Interfaces:** Public/manual/internal layout oracle, disposable actual Scalar Clone observer, fixed source/profile/font/binary pins and counter-free timing. New example consumes Task1 public wrappers without special-case output normalization.
- [ ] Preserve current192 cold/warm controls and true glyph build refusals; extend edited/missing/giant/variable input as required. Add positive exact input selection for fresh-process controls.
- [ ] Verify full outputs/ordered warnings/default limits plus actual scalar copies and cache/shaper events, allocation-neutral observer, calls/gross/freed/net/peak and separate releases.
- [ ] Run required checks/strict54 first; then balanced counter-free sequential and selected fresh A/B. Keep adverse ratios and allocator-history interpretation without untraced causal/RSS claims.
- [ ] Archive exact raw/source/font/recipes/binary/check/time identities and adoption decision.

### Task 3: Record and integrate
**Files:** docs/records/borrowed-edge-input.md, data manifest and lossless raw; native execution ledger.
**Interfaces:** Independently verify Task2 archive/full-output/table proof; one fresh Astra whole-branch review at exact final HEAD.
- [ ] Restore archived byte streams; verify all source/fonts/producer/binary/log/default-control/output/time/table identities; record necessary edit copies and adverse cases.
- [ ] Complete required checks and fresh review. Fix any Critical/Important via actual RED→GREEN; rule on every declined behavior.
- [ ] PR/exact latest-head all CI SUCCESS/merge/main core+hash/protected+saved/beads close readback/archive/owned cleanup under existing authorization; then .4.
