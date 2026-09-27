# Trailing Tab Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for inline execution, then one fresh whole-branch review.

**Goal:** Correct preserved trailing space/tab fitting, metrics and source endpoints consistently.
**Architecture:** Existing style-driven internal hanging policy shared by scan/cache/plan/intrinsics; ICU opportunities tailored for preserved whitespace sequences.
**Tech Stack:** Rust1.89+, existing fixed fonts and browser diagnostic fixtures.
**Spec:** docs/superpowers/specs/2026-09-27-tab-hanging-design.md

## Global Constraints

- Unchanged public API/dependency graph, font bytes, recorded browser capture and inputs.
- No Chrome slack/font-size override or blanket tolerance.
- S4 remains unmerged; .6 excluded. Preserve full original source and tab-stop advance.
- PreserveSpaces hanging is an explicit compatibility decision because CSS4 leaves it open.

## Review Focus

- Conditional trailing whitespace at forced/end must retain fitting advances for center/end alignment.
- Cached shrinking scans and prescribed plans must match direct range and geometry.
- Whitespace-unit style, transparent nested ends, first-line transitions and nonzero padding must obey the same edge policy.
- NoWrap must prohibit soft breaks even with BreakSpaces; mandatory breaks must survive tailoring.
- Intrinsic min/max and float retry must not use a different whitespace model.

### Task1: Behavioral regressions and shared policy

**Files:** tests/lines.rs, tests/intrinsic.rs, tests/break_plan.rs; create src/line/whitespace.rs; modify src/line/mod.rs, scan.rs, cache.rs, plan.rs, intrinsic.rs, src/analysis/breaks.rs.
**Interfaces:** Internal policy consumes ParagraphData+unit index/style; fitting and trailing metrics helpers preserve actual widths, explicit available width and BreakReason. No public interface additions.
- [x] Write literal synthetic tests:10px glyphs,40px tab stops, pre-wrap soft trailing tab hangs; Preserve+NoWrap retains width; BreakSpaces retains trailing spaces and breaks each character; forced/end conditional fit/overflow has correct line size and alignment. Observe functional RED.
- [x] Add mixed-style/transparent/nested padding, cached repeat/shrink, balanced/pretty plan range+size, intrinsic min/max and float retry/source mapping checks in owning test suites. Observe functional RED for missing behavior.
- [x] Implement shared whitespace policy and apply every trailing/fitting path, with mandatory/NoWrap break precedence and preserved sequence tailoring. Run focused tests GREEN without blessing regressions.
- [x] Run full workspace tests; resolve any existing failure according to actual CSS/source contract. Commit implementation and tests.

### Task2: Real-font comparison and exact ledger

**Files:** dev/fixtures/tests/browser_comparison.rs, assets/browser/differences.json, docs/browser-comparison.md.
**Interfaces:** Existing browser::build/first_end/transitions/atomic_geometry/check_all and existing capture, used unchanged.
- [x] Test real pre-wrap-tab90px original end9 and actual adjacent raw threshold with no font/width adjustments; observe RED before Task1 where feasible.
- [x] Produce raw comparison report; inspect every changed endpoint/transition/geometry, remove stale tab exceptions only after measured improvement and classify any residual numerical drift.
- [x] Update shodo-side strict ledger and documented counts with observed results; genuine capture unchanged. Strict check_all GREEN; commit.

### Task3: Integration

- [ ] Stable/MSRV full workspace, all-target Clippy, warning-denied docs, fmt/diff/Python/generator/strict report pass.
- [ ] One fresh whole-branch review; fix material findings with meaningful RED/GREEN and full verification.
- [ ] Push/PR; exact HEAD allCI SUCCESS, merge/readback/main containment, bd close, owned workspace/local branch cleanup; resume bd ready.
