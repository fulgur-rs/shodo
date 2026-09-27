# Browser Comparison Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement inline; one fresh final whole-branch review.

**Goal:** Recollect fixed-font Chromium breaks/boundaries and compare saved numeric data with public shodo next_line offline.
**Architecture:** Shared materialized cases; Python/JS headless recorder; public Rust builder/comparator; explicit measured difference ledger.
**Tech Stack:** Rust 1.89+, existing serde/sha2, Python standard library, installed Chromium.
**Spec:** docs/superpowers/specs/2026-09-27-browser-comparison-design.md

## Global Constraints

- Root API/normal dependency graph unchanged. No new browser automation package.
- Existing font bytes, family IDs, hashes and corpus shared; no system fonts.
- No Chrome font-size quantization or width-offset imitation.
- UTF-16/source UTF-8 endpoints explicit; reject surrogate interiors.
- S4 unmerged and .6 excluded; unknown comparison differences fail.

## Review Focus

- A trailing space or preserved tab assigned to the wrong DOM line must not manufacture a break mismatch.
- A supplementary scalar or nested text node endpoint must map to source bytes correctly.
- RTL visual order and vertical alignment must not masquerade as logical line order.
- Missing browser/font/result or changed input/hash must fail before replacing saved data.
- An existing exception must fail when either side changes; no broad tolerance or silent case skipping.

### Task 1: Shared input generation and public builder

**Files:** tools/browser_cases.py, tools/test_browser_cases.py, assets/browser-inputs.json, src/browser.rs, src/lib.rs, tests/browser_inputs.rs (all under dev/fixtures).
**Interfaces:** BrowserCase materialized ID/seed/text/font/size/direction/whitespace/parts/initial width; build(case,cx,fonts)->paragraph+AtomicSizes; checked UTF-16/UTF-8 conversion.
- [x] Write generator determinism/corpus/font/priority coverage tests and UTF-16/nested/atomic builder tests; observe missing behavior RED.
- [x] Implement stable versioned seed generation and matching public builder, with strict validation.
- [x] Run Python tests and focused Rust tests GREEN; record the exact corpus/input hashes.
- [x] Commit task and ledger result.

### Task 2: Actual browser recorder

**Files:** tools/collect_browser.py, tools/browser_recorder.js, tools/test_collect_browser.py, assets/browser/chromium.json.
**Interfaces:** collector consumes inputs+manifest, output metadata and records with source UTF-16/UTF-8 endpoints, initial and adjacent boundary probes, no-transition status and atomic baseline geometry.
- [x] Add RED validation/result/Unicode/source-line tests before implementation; no mocked browser verdicts.
- [x] Implement loopback fixed-font headless collection, explicit errors/timeouts, disposable profile and atomic validated output.
- [x] Collect actual browser data; inspect priority cases and whitespace/RTL/atomic source endpoints, then recollect and compare measurements exactly.
- [x] Run Python test suite GREEN and commit task/evidence.

### Task 3: Offline numeric comparison and diagnostic report

**Files:** src/browser.rs, tests/browser_comparison.rs, examples/browser_compare.rs, assets/browser/differences.json.
**Interfaces:** compare saved browser record with next_line at raw width; strict differences keyed by case/probe and exact expected/actual, transition classification and reproducer report.
- [x] Add RED tests for exact break checks, malformed/stale exceptions, metadata mismatch and useful diagnostics.
- [x] Implement raw-width comparison and boundary analysis; inspect every mismatch with measured probes, classify numeric/CSS/collector errors and fix collector errors rather than blessing them.
- [x] Add only justified exact known differences with evidence and issue links; no automatic blessing or omission.
- [x] Run offline comparison GREEN, verify full diagnostic report and commit.

### Task 4: Documentation and integration gates

**Files:** docs/browser-comparison.md, dev/fixtures/README.md, README.md, CI only if necessary for normal saved-data checks.
- [x] Document exact collection/update commands, browser/font/input metadata, source conversion, measured tolerance/difference rationale and supported scope.
- [x] Stable/MSRV workspace tests, Python fixture tests, Clippy/docs/fmt/diff; actual collection reproducibility complete.
- [x] One fresh final whole-branch reviewer, meaningful RED/GREEN fixes for material findings.
- [ ] Push/PR; exact HEAD full CI SUCCESS; merge/readback/ancestor verification, bd close and owned workspace cleanup; resume bd ready.
