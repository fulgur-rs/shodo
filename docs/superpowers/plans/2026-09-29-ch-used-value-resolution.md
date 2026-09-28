# Selected-font ch caller reproduction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan inline. Steps use checkbox syntax for tracking.

**Goal:** Fulfil shodo-3fl with real CSS-to-accepted-line tests for word-spacing, text-indent, margin and padding, root-cause classification and a recorded production dependency.

**Architecture:** Use the existing fixture example flow, pinned raikiri parser/cascade and existing FontCollection::resolve_ch. Preserve declaring-font provenance before passing absolute lengths into shodo; do not change the public core API.

**Tech Stack:** Rust1.89 minimum, existing shodo-fixtures/raikiri development dependencies, real pinned Latin/CJK fonts.

**Spec:** docs/ch-unit-resolution.md; shodo-3fl acceptance remains the binding scope.

## Global Constraints

- Preserve both unmerged S4 spikes and the other thread's root worktree.
- No dependency updates, host font discovery or WPT baseline rewriting.
- Cover all four required properties and differing parent/child fonts.
- A fixed example is evidence for root-cause investigation, not production wiring.

## Review Focus

- Inherited spacing must retain the declaring font rather than child's selected font.
- Missing named faces must select the real zero fallback; absent zero must use0.5em.
- Physical edge conversion must not silently accept RTL/vertical inputs.
- `calc` provenance loss must not be represented as a fixed measurement issue.
- Table-derived numeric oracles must allow the documented1/64px layout rounding.

### Task 1: Executable caller evidence and classification

**Files:** Create dev/fixtures/examples/ch_units.rs and docs/ch-unit-resolution.md; modify dev/fixtures/Cargo.toml to run example tests.

**Interfaces:** Consumes raikiri computed ChFontKey/ChLengthProvenance, shodo FontCollection::resolve_ch, ParagraphBuilder, InlineStyle/InlineEdges/LineOptions. Produces fixed-example layout(html,&FontCollection)->Result<Line,String> and a JSON CLI report. No public API changes.

- [x] Step1: Write actual-line tests with literal word22.88px, indent34.32px, per-side margin88.8px and padding111px; check child CJK glyph font.
- [x] Step2: Observe missing caller implementation RED, implement the minimal pinned CSS-to-shodo example, and investigate observed layout rounding rather than change metric expectations.
- [x] Step3: Observe runtime RED with real declaring-font measurement bypassed; restore measurement and verify GREEN. Add fallback and out-of-scope direction/writing-mode cases; reject unsupported geometry explicitly.
- [x] Step4: Run cargo test -p shodo-fixtures --example ch_units --offline and cargo run -p shodo-fixtures --example ch_units --offline; inspect the actual outputs.
- [x] Step5: Record existing core API vs caller root cause and mandatory production wiring dependency; read back the issue graph.

## Integration checks

 Freeze the exact patch/source hashes, run fmt, workspace clippy/tests, AccessKit workspace tests, no-default/complex core tests, docs, allocator tests and release lib tests. Obtain one fresh whole-change review, address required findings and record deferred issues.

 Commit, push/PR, verify all CI jobs for the exact feature HEAD, merge after successful CI and matching merge tree, close3fl and clean only owned worktree/branch.
