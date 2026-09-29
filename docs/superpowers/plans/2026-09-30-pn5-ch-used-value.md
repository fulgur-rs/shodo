# ch Used-Value Resolution Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the production raikiri→shodo caller a neutral, tested way to resolve simple `ch` lengths (word-spacing, letter-spacing, text-indent, margin, padding) against the declaring font.

**Architecture:** shodo core gains a raikiri-independent `ChLength` (family query + size + factor → px + face id) built on the existing `FontCollection::resolve_ch`. The raikiri-typed conversion (`ChFontKey`/`ChLengthProvenance` → `ChLength`, physical→logical edge mapping, fail-closed rules) lives in `dev/raikiri/adapter/ch.rs`, shared by the `ch_units` example by `#[path]`, so it can be moved into the adopted S4 caller unchanged. Neither S4 spike is touched.

**Tech Stack:** Rust, `shodo` crate, `raikiri-style` (git-pinned dev-dependency, rev ab7e619), `shodo-fixtures`.

**Spec:** beads issue `shodo-pn5` (design + acceptance fields); background `docs/dev/ch-unit-resolution.md`.

## Global Constraints

- Source comments and docs are written in English (memory: shodo-doc-crate-bevy).
- `crates/shodo` must NOT depend on raikiri (git deps break crates.io publishing). raikiri stays a dev-dependency of `dev/raikiri` only.
- No new unbounded work or allocation per `ch` (low memory, fast cold start, fail-closed; memory: shodo-fulgur-raikiri-html-web-fail-closed-dos). Non-finite factor/size must not propagate NaN into layout.
- Do not modify or merge `feat/s4-raikiri-integration` or `feat/s4-raikiri-integration-v2`.
- `calc()` containing `ch` is out of scope (shodo-e7n). Vertical / sideways writing modes are explicitly `Unsupported`.
- Quality gates before finishing: `cargo test -p shodo`, `cargo test -p shodo-raikiri --examples`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.

## Review Focus

- Inherited `ch` (word-spacing/letter-spacing/text-indent) measured with the child font instead of the declaring font → tests compare parent Latin 20px vs child CJK 40px (Task 3).
- `ch` value present but provenance missing → error, never a silent cascade approximation (Task 2).
- RTL: margin/padding physical left/right must map to inline-end/start correctly (Task 2, Task 3).
- Font fallback and missing U+0030: 0.5em of the declaring size (Task 1, Task 3).
- NaN/inf/negative factor or size → `None`, not garbage px (Task 1).

---

### Task 1: Neutral `ChLength` in shodo core

**Files:**
- Create: `crates/shodo/src/font/ch.rs`
- Modify: `crates/shodo/src/font/mod.rs` (add `mod ch; pub use ch::ChLength;`)
- Test: unit tests in `crates/shodo/src/font/ch.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ChLength { pub query: FontQuery, pub size: f32, pub factor: f32 }
  impl ChLength { pub fn resolve(&self, fonts: &FontCollection) -> Option<FontUnit> }
  ```
  `FontUnit.advance` is `factor * advance('0')`; `FontUnit.id` is the face that supplied U+0030 (`None` = CSS 0.5em fallback).

- [ ] **Step 1: Write failing tests** in `crates/shodo/src/font/ch.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions, FontQuery};
    use crate::limits::Limits;

    fn empty() -> FontCollection {
        FontCollection::with_options(
            &Limits::default(),
            FontOptions { system_fonts: false, ..Default::default() },
        )
    }

    #[test]
    fn missing_zero_glyph_uses_half_em_times_factor() {
        let ch = ChLength { query: FontQuery::default(), size: 20., factor: 3. };
        let unit = ch.resolve(&empty()).unwrap();
        assert_eq!(unit.id, None);
        assert_eq!(unit.advance, 30.);
    }

    #[test]
    fn non_finite_or_negative_inputs_are_rejected() {
        let fonts = empty();
        for (size, factor) in [(f32::NAN, 1.), (20., f32::INFINITY), (20., f32::NAN), (-1., 1.)] {
            let ch = ChLength { query: FontQuery::default(), size, factor };
            assert!(ch.resolve(&fonts).is_none(), "{size} {factor}");
        }
    }
}
```

- [ ] **Step 2:** Run `cargo test -p shodo font::ch` → FAIL (`ChLength` undefined).
- [ ] **Step 3: Implement**

```rust
//! Neutral `ch` length: a factor of the declaring font's U+0030 advance.
use super::{FontCollection, FontQuery, FontUnit};

/// A CSS `ch` length bound to the font it was declared with. Callers keep the
/// declaring font for inherited values instead of re-resolving in a child font.
#[derive(Clone, Debug)]
pub struct ChLength {
    pub query: FontQuery,
    pub size: f32,
    pub factor: f32,
}

impl ChLength {
    /// Returns `factor` times the '0' advance of the face selected by `query`
    /// (CSS fallback 0.5em when no face has the glyph). `None` if `size` or
    /// `factor` is not finite or `size` is negative.
    pub fn resolve(&self, fonts: &FontCollection) -> Option<FontUnit> {
        if !(self.size.is_finite() && self.size >= 0. && self.factor.is_finite()) {
            return None;
        }
        let unit = fonts.resolve_ch(&self.query, self.size);
        Some(FontUnit { id: unit.id, advance: self.factor * unit.advance })
    }
}
```

- [ ] **Step 4:** Run `cargo test -p shodo font::ch` → PASS.
- [ ] **Step 5:** Commit `feat(font): add raikiri-independent ChLength resolution`.

### Task 2: raikiri adapter module (dev/raikiri/adapter/ch.rs)

**Files:**
- Create: `dev/raikiri/adapter/ch.rs`
- Test: `#[cfg(test)]` inside that file, compiled through the example (Task 3 adds `#[path = "../adapter/ch.rs"] mod ch_adapter;`). Until then verify by temporarily including it from `ch_units.rs`.

**Interfaces:**
- Consumes: `shodo::font::{ChLength, FontCollection, FontQuery}`, `raikiri_style::{ChFontKey, ChLengthProvenance}`.
- Produces:
  ```rust
  pub fn ch_length(key: &ChFontKey, factor: f32) -> Result<ChLength, String>
  pub fn resolve_px(fonts: &FontCollection, factor: Option<f32>, key: Option<&ChFontKey>, fallback_px: f32) -> Result<f32, String>
  pub fn resolve_edge(fonts: &FontCollection, p: Option<&ChLengthProvenance>, fallback_px: f32) -> Result<f32, String>
  pub struct Physical<T> { pub top: T, pub right: T, pub bottom: T, pub left: T }
  pub fn to_logical<T>(dir: Direction, wm: WritingMode, p: Physical<T>) -> Result<shodo::node::Sides<T>, String>
  ```
  `to_logical`: horizontal-tb LTR → inline_start=left, inline_end=right; RTL → inline_start=right, inline_end=left; block_start=top, block_end=bottom; any other writing mode → `Err("vertical writing modes unsupported")`.

- [ ] **Step 1:** Move `family`, `font_style`, `ch`→`resolve_px`, `edge`→`resolve_edge` from `dev/raikiri/examples/ch_units.rs` into the adapter, building `ChLength` instead of calling `resolve_ch` directly. `resolve_px` with `factor: Some` and `key: None` must return `Err("ch value lost its declaring-font key")`. Document at the top that `ChFontKey` covers family/size/weight/style only, so variation axes and vertical orientation are not represented and callers must reject them (see Task 4).
- [ ] **Step 2: Write tests** for: missing key → Err; RTL swaps left/right (use `Physical<i32>` with 1,2,3,4 and assert `inline_start == 2` for RTL where right=2); `WritingMode::VerticalRl` → Err; `to_logical` LTR identity mapping.
- [ ] **Step 3:** Run `cargo test -p shodo-raikiri --example ch_units` → tests pass.
- [ ] **Step 4:** Commit `test(raikiri): extract ch adapter with fail-closed provenance and logical edge mapping`.

### Task 3: Rewire `ch_units` through the adapter and extend evidence

**Files:**
- Modify: `dev/raikiri/examples/ch_units.rs` (delete moved helpers, call `ch_adapter::*`, use `to_logical` for `InlineEdges`, accept RTL horizontal; keep vertical rejection)
- Modify: `docs/dev/ch-unit-resolution.md` (describe the neutral API and the adapter boundary)

- [ ] **Step 1:** Add RTL test: `#child{direction:rtl;margin-left:4ch;margin-right:0px}` → line geometry offset equals 88.8px on the inline-end side (assert against the independent oracle 4 × 0.555 × 40). Run: expect FAIL (RTL currently rejected).
- [ ] **Step 2:** Implement the wiring so existing 5 tests plus the RTL test pass. Update `fixed_example_rejects_other_directions_and_writing_modes` to drop the `direction:rtl` cases and keep the vertical ones.
- [ ] **Step 3: Mutation check.** Temporarily make `resolve_edge` return `factor * 0.5 * size`; confirm the inherited/edge tests fail (word 20 / indent 30 style); restore and confirm GREEN.
- [ ] **Step 4:** `cargo test -p shodo-raikiri --examples`, clippy, fmt; commit `feat(raikiri): route ch_units through the shared ch adapter`.

### Task 4: Variation/orientation evaluation and WPT evidence

**Files:**
- Create: `docs/records/raikiri-ch-used-value.md`

- [ ] **Step 1: Variation/orientation.** Add a test using a variable fixture face if `shodo-fixtures` has one (check `dev/fixtures`); otherwise construct the comparison with `FontQuery` weight variation via skrifa and record whether U+0030's advance changes with `wght`. Record the conclusion: representable by `ChFontKey.weight` (covered) vs. axes not in the key (`font-variation-settings`, `font-stretch`) → adapter must return `Err` when the cascade sets them, or a raikiri issue is opened. State orientation: horizontal only, vertical rejected.
- [ ] **Step 2: WPT.** The WPT harness (`raikiri-wpt`, `word-spacing-003` etc.) lives in the raikiri repo, not here. Run the native baseline at the pinned rev and save pass/fail counts. The candidate run requires the S4-adopted caller; until S4 is decided, record it as **not run** with the reason. Do not claim baseline changes without a run.
- [ ] **Step 3:** Compare against native `measure_ch_advance_for_font_key` (`raikiri-dom/src/layout/inline_text.rs:2837`) on the fixture fonts for parent Latin 20px and child CJK 40px, and record the numbers (expected 11.44 / 22.2 px).
- [ ] **Step 4:** Commit the record; update `bd` notes on shodo-pn5 with what is verified and what remains blocked on S4.

## Self-Review

- Spec coverage: neutral API (T1), inherited vs. own provenance and fail-closed (T2/T3), logical edge mapping incl. RTL and vertical rejection (T2/T3), variation/orientation evaluation (T4), WPT native vs. candidate with honest not-run (T4), no spike changes (constraints).
- Known limit: the candidate WPT run and final production-caller connection depend on the S4 adoption decision; the plan delivers the adoptable adapter and evidence, not that connection.
