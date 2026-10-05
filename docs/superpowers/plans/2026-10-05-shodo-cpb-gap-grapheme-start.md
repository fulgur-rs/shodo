# shodo-cpb Grapheme Starts After Transparent Gaps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (inline; the change is one comparison) or superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A scalar that begins a grapheme right after a transparent gap (the isolate controls around a ruby base, an out-of-flow placeholder, a `unicode-bidi: isolate` inline, an authored bidi control) gets `grapheme_start == true`, so a text-combine-upright group at the start of a (nested) ruby base is counted as 2–4 graphemes and receives `hwid`/`twid`/`qwid` like any other group.

**Architecture:** `itemize` currently matches each local grapheme's first scalar against `breaks.graphemes`. Those cuts come from `Projection::upstream` and deliberately sit before transparent markers (at the end of the preceding content), so after a gap they never equal the next scalar's offset. `breaks.typographic_starts` is the parallel vector of actual character starts after the markers; `graphemes.rs` and `revert_width` already use it for the same question. The fix swaps the vector the linear cursor walks. No new state, no allocation.

**Tech Stack:** Rust (crate `shodo`), unit tests in `analysis/itemize.rs` and `shape/tests.rs`.

**Spec:** the `design` and `acceptance` fields of beads issue `shodo-cpb` (`bd show shodo-cpb`). Records in Japanese, source comments in English.

## Global Constraints

- Paragraphs without transparent gaps keep byte-identical shape items (there `graphemes == typographic_starts`); all existing tests pass unchanged except comments that described the old behavior.
- Exactly one scalar per projected grapheme is flagged; authored bidi control scalars (excluded from the projection) are not grapheme starts anywhere in the paragraph, matching the paragraph-start case today.
- `CombinedWidthProbeMode::CloneReference` and `ScopedReuse` agree on output, warnings and limit errors.
- The comparison count stays linear (`paragraph_grapheme_start_matching_uses_linear_comparisons`).
- Gates: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- No attribution lines in commits. Never touch untracked files in the main checkout.

## Review Focus

1. Authored bidi controls mid-paragraph flip from `true` to `false`: `leading_end` (`shape.rs`) now separates a leading removed control from the next glyph as its comment intends; the glyph-free prefix loop and the run-byte splitter see one more boundary after a gap.
2. The run-byte splitter may now cut right after a gap instead of warning "giant grapheme"; pinned by a test.
3. Reference vs reuse equivalence for a base-leading group across glyph limits.

---

### Task 1: Failing tests

**Files:** `crates/shodo/src/analysis/itemize.rs` (tests), `crates/shodo/src/shape/tests.rs`.

- [ ] **Step 1:** itemize test `grapheme_starts_follow_transparent_gaps`: build `"a"`, out-of-flow, `"b"`, an isolate inline with `"c"`, then `"\u{200e}d"`; assert scalar flags `[a:true, b:true, c:true, LRM:false, d:true]`. A second paragraph `"\u{200e}a"` pins the paragraph-start control as `false` (unchanged).
- [ ] **Step 2:** shape test `base_leading_combined_group_selects_a_width_feature`: the nested builder with inner base content `"12"` only. In both probe modes the `'1'` item has both scalars flagged and `width_feature == Some(*b"hwid")`, snapshots are equal, and a sweep of outer/inner glyph limits gives equal outcomes.
- [ ] **Step 3:** shape test `combined_group_after_out_of_flow_selects_a_width_feature` (ruby-free): `"あ"`, out-of-flow, TCY `"12"` gets `hwid`.
- [ ] **Step 4:** shape test `run_byte_budget_splits_after_a_transparent_gap`: `"a"`, out-of-flow, `"b"` in one style with `max_shaping_run_bytes: Some(1)` builds with no "giant grapheme" warning.
- [ ] **Step 5:** run them; Steps 1–4 fail on the old code (Step 1 on `b`/`c`/`d`, Steps 2–3 on `None`, Step 4 on the warning).

### Task 2: Fix

**Files:** `crates/shodo/src/analysis/itemize.rs` (`flush`).

- [ ] **Step 1:** Replace `breaks.graphemes` with `breaks.typographic_starts` in the `grapheme_cursor` loop and the equality check; rename nothing else; add a comment explaining why upstream cuts are wrong here.
- [ ] **Step 2:** Update the t3t builder comments in `shape/tests.rs` that say a base's first scalar is not a grapheme start; keep the leading `"x"` (the prefix glyph count needs it).
- [ ] **Step 3:** Run the tests from Task 1, then the full gates. Commit `fix:` (code) and `test:` separately if the diff allows; otherwise one `fix:` commit with its tests.

### Task 3: Record, review, PR

- [ ] **Step 1:** `docs/records/shodo-cpb-gap-grapheme-start.md` (Japanese): cause, decision, affected inputs, equivalence checks.
- [ ] **Step 2:** Dispatch a code reviewer; address findings.
- [ ] **Step 3:** Push, open the PR, wait for CI, merge, clean up the worktree, close the issue.
