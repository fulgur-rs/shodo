# Styled break quirks correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Match the nine Chromium cases in shodo-r8t and publish a version Raikiri can pin.

**Architecture:** Reuse line::quirk's ContentCredit and strut credits to recognize an ending break parent with no metrics. Per the user's explicit clarification, Chromium's local parent credit rule governs pending aligned descendants and content outside the parent. Retained metrics conditionally apply explicit break profiles. The scalar index omits unconditional break profiles under the quirk and selects conditional break summaries only when the ending break parent lacks metrics. Synthetic retained ranges may span several breaks, so conditional summaries include every break and group extent without rescanning units.

**Tech Stack:** Rust 2024, existing public builder tests and indexed/retained parity tests.

**Spec:** Beads issue shodo-r8t (`bd show shodo-r8t`).

## Global Constraints

- Chromium's measured parent-credit rule takes precedence over the ticket's initial global-content generalization (user clarification).
- Quirk-disabled, plain forced breaks and preserved newline behavior remain unchanged.
- All nine Chromium heights: 22, 22, 40, 42, 60, 40, 60, 30, 60 px.
- No public API shape changes, dependency changes or MSRV increase (Rust 1.89).
- Preserve root-strut opt-in, first-line styles, vertical alignment and writing modes.
- Use a dedicated worktree and task-owned temporary files under ~/tmp; retain evidence before cleanup.

## Review Focus

- Trimmed collapsible spaces must permit a break strut; preserved spaces must suppress it.
- Inline border/padding and pending alignment must suppress a styled break when they credit another ancestor.
- Identically interned styles must still obey the explicit/inherited break distinction.
- Break font/alignment metadata must remain available when its metric profile is suppressed.
- Indexed selection must remain bounded for long prefixes, and must apply group profiles only when eligible.

### Task 1: Conditional styled-break metrics and shipping evidence

**Files:**
- Modify: crates/shodo/src/line/quirk.rs, line/metrics.rs, line/metric_index.rs, line/metric_index/quirk.rs, line/metric_index/scalar.rs, builder.rs.
- Test: crates/shodo/tests/line_height_quirk.rs and annotation_metrics.rs; existing crates/shodo/src/ruby/tests/quirk.rs parity/scaling tests.
- Correct: docs/records/shodo-47n-styled-break.md.
- Create: docs/records/shodo-r8t-styled-break.md.

**Interfaces:**
- Consumes: ParagraphBuilder::push_forced_break_with_style and Line::forced_break.
- Produces: same APIs, with Chromium-compatible metrics when line_height_quirk is enabled.

- [ ] Step 1: Add a literal nine-row public API height matrix and mode controls. Update prior contradictory mixed-content expectations. Add whitespace/edge/pending/font and metadata regressions.
- [ ] Step 2: Run `cargo test -p shodo --test line_height_quirk`. Expected: mixed-content styled-break assertions fail against main.
- [ ] Step 3: Expose the retained line's styled-break parent-credit condition, gate retained break profiles, omit unconditional indexed break profiles under the quirk, and conditionally select break summaries in scalar/group metrics, including synthetic ranges spanning several breaks.
- [ ] Step 4: Run focused public tests and `cargo test -p shodo --lib styled_break`. Expected: all pass; bounded query test and baseline parity remain passing.
- [ ] Step 5: Correct API docs/old record, write red/green and Chromium results. Run fmt, workspace tests, Clippy -D warnings, no-default/complex-script tests, rustdoc -D warnings and diff check. Expected: all pass.
- [ ] Step 6: Commit and obtain one independent read-only whole-branch review, resolve blocking findings with regressions, push PR and wait for all CI. Merge the reviewed head.
- [ ] Step 7: Review updated Release PR, submit required current-head approval, wait all CI, merge and approve the existing release environment. Verify public release, registry archive checksum, tag/VCS commit, identical release/merge trees and all published Rust sources.
- [ ] Step 8: Record published pin and evidence in shodo-r8t, close it and remove only owned temporary artifacts/worktree/branch.
