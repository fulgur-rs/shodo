# Complex-script diagnostics implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve shodo-0ce using measured model availability and a regression-proven constructor fix.
**Architecture:** Keep existing feature boundaries. Change only transformation word segmentation; line analysis already loads the models.
**Tech Stack:** Rust, ICU4X 2.3, existing compiled_data and auto features.
**Spec:** docs/superpowers/specs/2026-09-29-complex-script-diagnostics-design.md

## Global Constraints

No dependency/public API/provider changes. Keep no-default behavior. Do not
change/merge/clean either raikiri spike or the independently owned root.

## Review Focus

- Japanese/Khmer content_locale line boundaries: literal characterization test.
- Transformed versus plain uncased text: real ParagraphBuilder subprocess.
- Logging disabled/release: literal capitalization word-head context test.
- Feature disabled: retain constructor; exercise no-default suite.
- Cased scripts, mapping, source splits: existing full text-transform suites.

### Task 1: Reproduce and repair transformation model selection

**Files:** Modify src/analysis/transform_context.rs and tests/text_transform.rs;
create tests/complex_script_segmentation.rs and docs/complex-script-segmentation.md.
**Interfaces:** consumes context(&str)->Vec<u16> and public ParagraphBuilder;
produces unchanged APIs with feature-correct word boundaries and no spurious
model diagnostics when complex-scripts is enabled.

- [ ] Add context_word_heads_use_complex_models with literal Japanese [0,15],
  Thai [0,12], Khmer [0,12,27,33] expectations and transformed_paragraphs_load_complex_models.
- [ ] Run both against the old constructor; expect wrong heads and model errors.
- [ ] Select new_auto under complex-scripts, existing non-complex constructor otherwise.
- [ ] Verify targeted GREEN. Add content_locale characterization with literal
  Japanese [0,3,6,9,12,15,18,21], Khmer [0,12,27,33,57]; document numeric before/after and stderr limitations.
- [ ] Run required full gates on frozen source, read-only final review, commit.

### Task 2: Integrate and clean up

**Files:** artifact ledger and issue state only.
**Interfaces:** consumes frozen verified patch; produces merged PR, closed
shodo-0ce and removal of owned branch/worktree only.

- [ ] Push/create PR, verify check/msrv/wasm all succeed for this head/merge tree.
- [ ] Merge matching exact head, verify actual tree, close issue and archive ledger.
- [ ] Remove owned worktree/local branch and verify automatic remote deletion.

Self-review: the measured emitter supersedes the issue's suspected provider
cause. Both original numerical LineSegmenter acceptance and actual transform
regression are covered; no unverified full-WPT success claim.
