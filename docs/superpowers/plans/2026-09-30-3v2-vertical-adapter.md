# Vertical writing-mode / text-orientation adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire raikiri's resolved `cssom_writing_mode`, `text_orientation`, `text_combine_upright` and `text_autospace` into shodo's existing vertical API through a movable adapter, and verify it on the four WPT documents from shodo-zt3.

**Architecture:** `dev/raikiri/adapter/vertical.rs` converts raikiri computed values to shodo `ParagraphStyle`/`InlineStyle` fields, failing closed on anything it cannot map. An example (`vertical_wiring`) runs raikiri cascade → adapter → shodo on vendored copies of the four documents and uses the WPT reference as the oracle: test and reference must yield identical lines and glyph geometry. No change to `crates/shodo` or the S4 spikes.

**Tech Stack:** Rust, `shodo`, `raikiri-style`/`raikiri-html` (git-pinned dev-deps, rev ab7e619), `shodo-fixtures`.

**Spec:** beads issue `shodo-3v2` (design + acceptance).

## Global Constraints

- Comments/docs in English (memory: shodo-doc-crate-bevy).
- `crates/shodo` must not depend on raikiri; raikiri stays a dev-dependency of `dev/raikiri`.
- Read `cssom_writing_mode`, never `writing_mode` (the renderer-facing field is normalized to horizontal).
- Unknown/unsupported values fail closed: `text-autospace: auto|custom`, and non-exhaustive raikiri enums use an error arm, not a default.
- Horizontal LTR/RTL behavior must not regress: existing `ch_units` tests stay green.
- Gates: `cargo test -p shodo-raikiri --examples`, `cargo test -p shodo`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
- Do not modify the S4 spikes. WPT baseline change and native image comparison are not claimed (need S4's adopted caller).

## Review Focus

- Vertical silently dropped to horizontal because `writing_mode` is read instead of `cssom_writing_mode` → Task 1 test with a computed-values-independent mapping and Task 2 test asserting the line is vertical.
- `text-autospace:auto`/`custom` accepted as `normal` → Task 1 rejection tests.
- `text-orientation: sideways` vs `mixed` confusion → Task 1 mapping test for all three.
- `text-combine-upright: all` on a span that reaches shodo as ordinary text (TCY supply lost) → Task 2 test that the combined span occupies one em.
- Reference/test mismatch hidden by comparing only line counts → Task 2 compares per-glyph geometry.

---

### Task 1: Value mapping (`adapter/vertical.rs`)

**Files:**
- Create: `dev/raikiri/adapter/vertical.rs`
- Modify: `dev/raikiri/Cargo.toml` (add `[[example]] name = "vertical_wiring" test = true`, created in Task 2; keep the adapter tests compiled through that example)

**Interfaces:**
- Produces:
  ```rust
  pub fn writing_mode(m: raikiri_style::property::WritingMode) -> Result<shodo::geometry::WritingMode, String>
  pub fn text_orientation(o: raikiri_style::property::TextOrientation) -> Result<shodo::style::TextOrientation, String>
  pub fn text_combine_upright(t: raikiri_style::property::TextCombineUpright) -> Result<shodo::style::TextCombineUpright, String>
  pub fn text_autospace(a: raikiri_style::property::TextAutospace) -> Result<shodo::style::TextAutospace, String>
  ```
  Mapping: `HorizontalTb→HorizontalTb, VerticalRl→VerticalRl, VerticalLr→VerticalLr, SidewaysRl→SidewaysRl, SidewaysLr→SidewaysLr`; `Mixed/Upright/Sideways` 1:1; `None/All` 1:1; `Normal→Normal, NoAutospace→NoAutospace`, `Auto`/`Custom{..}`/unknown → `Err`.

- [ ] **Step 1: Write failing tests** in the module: one test per function asserting every mapped variant, and `text_autospace(TextAutospace::Auto)` and a `Custom { ideograph_alpha: true, ideograph_numeric: true, punctuation: true, mode: TextAutospaceMode::None }` returning `Err`.
- [ ] **Step 2:** Include the module from a stub `vertical_wiring.rs` (`#[path = "../adapter/vertical.rs"] mod vertical_adapter; fn main() {}`) and run `cargo test -p shodo-raikiri --example vertical_wiring` → FAIL (functions missing).
- [ ] **Step 3: Implement** the four functions with explicit `match` arms and a trailing `_ => Err("… unsupported".into())` arm (the enums are `#[non_exhaustive]`).
- [ ] **Step 4:** Run the tests → PASS.
- [ ] **Step 5:** Commit `feat(raikiri): map vertical style values into shodo (fail closed)`.

### Task 2: Vendored WPT documents and wiring example

**Files:**
- Create: `dev/raikiri/data/vertical/text-autospace-vertical-combine-001.html`, `…-combine-001-ref.html`, `…-upright-001.html`, `…-upright-001-ref.html` (copied unmodified from `~/.cache/raikiri/wpt/css/css-text/text-autospace/`; verify SHA-256 against `dev/raikiri/data/raikiri-style-diagnostics.json` before committing)
- Modify: `dev/raikiri/examples/vertical_wiring.rs`

**Interfaces:**
- Consumes: Task 1 mappers; `parse_html`, `doc.cascade().computed[node]` as in `ch_units.rs`.
- Produces: `fn lines(html: &str, fonts: &FontCollection) -> Result<Vec<LineSummary>, String>` with `LineSummary { inline_size: f32, block_extent: f32, glyphs: Vec<(FontId, u32 /*glyph id*/, f32 /*advance*/, f32 /*x*/, f32 /*y*/)> }`, one entry per `#container > div`. Use the public `Glyph`/`GlyphRun` accessors (see `crates/shodo/tests/common/mod.rs` `glyphs()` and `tests/vertical.rs` for the accessors used to read glyph geometry).

- [ ] **Step 1:** Copy the four files; `sha256sum` each and compare with the diagnostics JSON (`sha256` fields). Abort with a ledger note if a hash differs.
- [ ] **Step 2: Write failing tests** (`#[cfg(test)]`):
  - `combine_test_matches_reference`: `lines(combine)` equals `lines(combine_ref)` (per-glyph comparison within 1/64px).
  - `upright_test_matches_reference`: same for upright.
  - `vertical_is_not_dropped`: for the combine doc, every line's block/inline axes are those of `VerticalRl` (assert `line.inline_size()` equals the summed glyph advances along the vertical axis and the paragraph reported writing mode is `VerticalRl`; if no such accessor exists, compare against the same text built with `HorizontalTb` and assert the geometry differs).
  - `tcy_span_occupies_one_em`: in the combine doc, the `.tcy` span's glyphs together span exactly `font_size` (20px) along the inline axis.
  - `autospace_auto_is_rejected`: the upright doc with `text-autospace:auto` returns `Err`.
- [ ] **Step 3:** Run → FAIL (`lines` not implemented).
- [ ] **Step 4: Implement** `lines`: parse, find `#container`, and for each child `div` build a `ParagraphBuilder`: paragraph style `writing_mode(cssom_writing_mode of #container)`, root `InlineStyle` from the div's computed values (font family/size via the `ch_units` adapter helpers, `text_orientation`, `text_combine_upright`, `text_autospace`); for each child span `open_inline` with the span's own mapped `InlineStyle`, push its text, `close_inline`. Take the first line at a large constraint (e.g. 10000px) and summarize it. Fonts come from `shodo_fixtures::load_fonts` (the fixtures cover Latin and CJK).
- [ ] **Step 5:** Run → PASS. If `*_matches_reference` fails, that is a finding, not a test to weaken: record it in the ledger, keep the test as the oracle, and route the cause through systematic-debugging (adapter bug vs shodo behavior vs fixture font).
- [ ] **Step 6: Mutation check:** temporarily make `writing_mode` return `HorizontalTb` for `VerticalRl`; confirm `vertical_is_not_dropped` and the match tests fail; restore.
- [ ] **Step 7:** Commit `test(raikiri): verify vertical wiring on the four autospace-vertical documents`.

### Task 3: Record and classification

**Files:**
- Create: `docs/records/raikiri-vertical-wiring.md`

- [ ] **Step 1:** Write the record: what was verified (Task 2 results with numbers), what fails closed, the `cssom_writing_mode` pitfall, TCY behavior, and the provisional classification. State explicitly: native image comparison and WPT baseline movement are **not measured**; classification of "required for p2m.6" is provisional, pending S4 adoption. List the unsupported declarations (`text-autospace:auto|custom`).
- [ ] **Step 2:** Run all gates; commit `docs(records): record vertical wiring evidence`.
- [ ] **Step 3:** Update `bd` notes on shodo-3v2 with results and what remains blocked.

## Self-Review

- Coverage: cssom read (T1/T2 mutation), orientation/TCY/autospace mapping and fail-closed (T1/T2), four documents with reference oracle (T2), non-regression (gates), classification with honest not-measured (T3).
- Known limits: the production connection and native comparison depend on S4's adoption decision.
