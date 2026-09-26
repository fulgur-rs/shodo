# shodo S0-A: Walking Skeleton Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first half of the S0 walking skeleton: every foundational type, a stub font layer with fail-closed budgets, the builder, whitespace collapsing with OffsetMapping, a fixed-width stub shaper, and a float-free `next_line` that produces lines with glyph runs, inline-box fragments, an atomic, and bidi reordering.

**Architecture:** One crate `shodo`. `ParagraphBuilder` records raw items; `build` collapses whitespace (building `OffsetMapping`), shapes with a 1em fixed-advance stub into structure-of-arrays glyph storage, and flattens everything into "units" (one per cluster or control) held in an `Arc<ParagraphData>`. `Paragraph::next_line` is a pure greedy breaker over units that returns an owned `Line` holding the `Arc` plus a small fragment table. Plan S0-B (written after this plan is executed) adds line-box block sizing, `BlockSizeExceeded`, `BlockInInline` empty lines, justification, the full float protocol, intrinsic sizes, and property tests.

**Tech Stack:** Rust 2024 (MSRV 1.89), `unicode-bidi 0.3.18`, `peniko 0.6` (for `FontData` / `Blob`).

**Spec:** `docs/superpowers/specs/2026-09-26-shodo-foundation-design.md` (§6.5 lists the skeleton scope; this plan covers §1–§2, §2.4, §3.1, the float-free parts of §3.2, §4.1–§4.4, §5.2's structural check and budgets, §5.3 limits, §5.4).

## Global Constraints

- `#![forbid(unsafe_code)]` in `src/lib.rs`.
- Edition 2024, `rust-version = "1.89.0"`, license `MIT OR Apache-2.0` (already in `Cargo.toml`).
- Public API takes and returns `f32` px. Internal layout math uses `LayoutUnit` (1/64 px in `i32`, saturating). Caller-supplied `f32` values are converted with `LayoutUnit::from_f32_round`; `from_f32_ceil` is only for values shodo derives itself.
- Non-finite inputs become 0 and produce a `Warning`; nothing panics on any input.
- Resource limits are checked **before** allocating; on violation the builder enters an error state, later pushes allocate nothing, and `build` returns `Err(LimitExceeded)`. Nothing is silently truncated.
- Stub shaper: every character advances exactly **1em** (`font_size` px), like the Ahem font, except combining marks U+0300–U+036F which advance 0 and carry an inline offset of −0.5em. Tests rely on this: at `font_size = 10.0`, "abc" is 30px wide.
- Stub font metrics per em: ascent 0.8, descent 0.2, line gap 0, underline offset −0.1 (below baseline is positive downward: offset 0.1), thickness 0.05. `line-height: normal` = ascent + descent + line gap = 1.0em.
- Test convention: `pub(crate)` internals (`LayoutUnit`, glyph storage, units, the sfnt builder) are tested with `#[cfg(test)] mod tests` inside `src/`. Files under `tests/` use only the public API.
- Source comments cite specs (CSS sections, UAX numbers) and technical reasons only. Never put issue IDs, milestone names, or review tags (`[C1-M5]` etc.) in source comments.
- Every commit message ends with the line `Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA`.
- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before each commit; both must be clean.

## Review Focus

1. **Unbalanced builder calls** — `close_inline()` with nothing open is ignored with an `UnbalancedInline` warning; inline boxes still open at `build()` are auto-closed with the same warning. (Task 7, Task 9)
2. **Empty or fully collapsed paragraph** — `""` or `"   "` builds successfully and `next_line(start_token)` returns `Done` without panicking. (Task 10)
3. **Zero or negative available width** — every `Line` still consumes at least one unit, so a caller looping on `next_line` always terminates. (Task 10)
4. **A single word wider than the line** — the word overflows onto one line (CSS `overflow-wrap: normal`), `inline_size() > available`, and the following word starts a new line. (Task 10)
5. **An atomic missing from `AtomicSizes`** — it is laid out as 0×0 and a `MissingAtomicSize` warning is recorded. (Task 11)

## File Structure

| File | Responsibility |
|---|---|
| `src/lib.rs` | Module wiring, re-exports, `forbid(unsafe_code)` |
| `src/geometry/unit.rs` | `LayoutUnit`, `Saturation` |
| `src/geometry/mod.rs` | `WritingMode`, `Direction`, `BaselineKind`, logical/physical rects, `PhysicalConverter` |
| `src/limits.rs` | `Limits`, `LimitKind`, `LimitExceeded`, `Warning`, `WarningKind`, internal `WarningSink` |
| `src/node.rs` | `NodeId`, `TextSource`, `Sides`, `InlineEdges`, `OutOfFlowKind` |
| `src/style.rs` | `InlineStyle`, `ParagraphStyle`, `LineOptions` and their value enums |
| `src/font/sfnt.rs` | Minimal sfnt writer (stub face, tests) |
| `src/font/check.rs` | `FontError`, structural check of blobs before registration |
| `src/font/mod.rs` | `FontCollection` (shared + document layers), `FontId`, `FontMetrics` |
| `src/builder.rs` | `ParagraphBuilder`, `RichText`, raw item recording, incremental limits |
| `src/mapping.rs` | `OffsetMapping` and its public types |
| `src/analysis/mod.rs` | Internal `Item`/`ItemKind` and the processing pipeline entry |
| `src/analysis/whitespace.rs` | White-space collapsing, bidi control insertion, mapping construction |
| `src/analysis/units.rs` | Units, break classes, bidi levels, inline-box table |
| `src/shape.rs` | Stub shaper, `GlyphStore` (SoA), `ShapedRun` |
| `src/context.rs` | `LayoutContext` |
| `src/paragraph.rs` | `Paragraph`, `ParagraphData`, `BreakToken`, `FloatCursor`, `LineConstraint`, `AtomicSizes`, `LineResult`, `BreakPlan` |
| `src/line/mod.rs` | `next_line` (greedy breaker) |
| `src/line/fragments.rs` | Building fragment records, bidi reordering, inline-box grouping |
| `src/output.rs` | `Line`, `BreakReason`, `Fragment`, views |
| `tests/*.rs` | Public-API integration tests |

---

### Task 1: Crate scaffold and `LayoutUnit`

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/geometry/mod.rs`
- Create: `src/geometry/unit.rs`

**Interfaces:**
- Produces: `pub(crate) struct LayoutUnit` with `ZERO`, `MAX`, `MIN`, `SCALE`, `from_raw(i32)`, `raw()`, `from_f32_round(f32, &mut Saturation)`, `from_f32_ceil(f32, &mut Saturation)`, `to_f32()`, `add(Self, &mut Saturation)`, `sub(Self, &mut Saturation)`, `mul_i32(i32, &mut Saturation)`, `div_i32(i32)`; `pub(crate) struct Saturation { saturated: u32, non_finite: u32 }` with `is_clean()`. `LayoutUnit` also implements `Ord`, and saturating `std::ops::Add`/`Sub` for internal sums that cannot overflow in practice.

- [ ] **Step 1: Add dependencies**

Replace the empty `[dependencies]` section of `Cargo.toml` with:

```toml
[dependencies]
peniko = "0.6"
unicode-bidi = "0.3.18"
```

- [ ] **Step 2: Write the failing tests**

Create `src/geometry/unit.rs` containing only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_and_ceil_differ_on_fractions() {
        let mut sat = Saturation::default();
        // 100.004 px * 64 = 6400.256
        assert_eq!(LayoutUnit::from_f32_round(100.004, &mut sat).raw(), 6400);
        assert_eq!(LayoutUnit::from_f32_ceil(100.004, &mut sat).raw(), 6401);
        assert!(sat.is_clean());
    }

    #[test]
    fn non_finite_becomes_zero_and_is_counted() {
        let mut sat = Saturation::default();
        assert_eq!(LayoutUnit::from_f32_round(f32::NAN, &mut sat), LayoutUnit::ZERO);
        assert_eq!(LayoutUnit::from_f32_round(f32::INFINITY, &mut sat), LayoutUnit::ZERO);
        assert_eq!(sat.non_finite, 2);
    }

    #[test]
    fn out_of_range_saturates_and_is_counted() {
        let mut sat = Saturation::default();
        assert_eq!(LayoutUnit::from_f32_round(1.0e9, &mut sat), LayoutUnit::MAX);
        assert_eq!(LayoutUnit::from_f32_round(-1.0e9, &mut sat), LayoutUnit::MIN);
        assert_eq!(sat.saturated, 2);
    }

    #[test]
    fn arithmetic_saturates_instead_of_wrapping() {
        let mut sat = Saturation::default();
        let near_max = LayoutUnit::from_raw(i32::MAX - 1);
        assert_eq!(near_max.add(LayoutUnit::from_raw(10), &mut sat), LayoutUnit::MAX);
        assert_eq!(LayoutUnit::MIN.sub(LayoutUnit::from_raw(1), &mut sat), LayoutUnit::MIN);
        assert_eq!(near_max.mul_i32(2, &mut sat), LayoutUnit::MAX);
        assert_eq!(sat.saturated, 3);
        assert_eq!(LayoutUnit::MIN.div_i32(-1), LayoutUnit::MAX);
        assert_eq!(LayoutUnit::from_raw(64).div_i32(0), LayoutUnit::ZERO);
    }

    #[test]
    fn converts_back_to_px() {
        assert_eq!(LayoutUnit::from_raw(64).to_f32(), 1.0);
        assert_eq!(LayoutUnit::from_raw(32).to_f32(), 0.5);
    }
}
```

Create `src/geometry/mod.rs`:

```rust
//! Geometry: fixed-point units, writing modes, and logical/physical conversion.

mod unit;

pub(crate) use unit::{LayoutUnit, Saturation};
```

Replace `src/lib.rs` with:

```rust
//! shodo — inline formatting context engine.
//!
//! shodo lays out the inline content of one block container (a paragraph):
//! white-space processing, bidi, line breaking, alignment, inline boxes and
//! atomic inlines. Coordinates are logical (inline / block axes) and are
//! converted to physical coordinates with [`geometry::PhysicalConverter`].
#![forbid(unsafe_code)]

pub mod geometry;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib geometry::unit`
Expected: FAIL to compile with "cannot find type `LayoutUnit`" / "cannot find type `Saturation`".

- [ ] **Step 4: Implement `LayoutUnit`**

Insert above the `#[cfg(test)]` block in `src/geometry/unit.rs`:

```rust
//! Fixed-point layout unit: 1/64 px stored in an `i32`.
//!
//! All arithmetic saturates so that overflow can never wrap into a negative
//! width. Saturations and non-finite inputs are counted in [`Saturation`] so
//! the caller can report them as a single warning.

/// Counters for lossy conversions and saturating arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Saturation {
    pub(crate) saturated: u32,
    pub(crate) non_finite: u32,
}

impl Saturation {
    pub(crate) fn is_clean(&self) -> bool {
        self.saturated == 0 && self.non_finite == 0
    }
}

/// 1/64 px fixed-point value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LayoutUnit(i32);

impl LayoutUnit {
    pub(crate) const SCALE: i32 = 64;
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const MAX: Self = Self(i32::MAX);
    pub(crate) const MIN: Self = Self(i32::MIN);

    pub(crate) const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub(crate) const fn raw(self) -> i32 {
        self.0
    }

    /// Converts px to layout units, rounding to the nearest 1/64 px. Used for
    /// every caller-supplied value.
    pub(crate) fn from_f32_round(value: f32, sat: &mut Saturation) -> Self {
        Self::from_scaled(value, sat, f64::round)
    }

    /// Converts px to layout units, rounding up. Only for values shodo derives
    /// itself (for example line heights from font metrics).
    pub(crate) fn from_f32_ceil(value: f32, sat: &mut Saturation) -> Self {
        Self::from_scaled(value, sat, f64::ceil)
    }

    fn from_scaled(value: f32, sat: &mut Saturation, rounding: fn(f64) -> f64) -> Self {
        if !value.is_finite() {
            sat.non_finite += 1;
            return Self::ZERO;
        }
        let scaled = rounding(f64::from(value) * f64::from(Self::SCALE));
        if scaled > f64::from(i32::MAX) {
            sat.saturated += 1;
            Self::MAX
        } else if scaled < f64::from(i32::MIN) {
            sat.saturated += 1;
            Self::MIN
        } else {
            Self(scaled as i32)
        }
    }

    pub(crate) fn to_f32(self) -> f32 {
        self.0 as f32 / Self::SCALE as f32
    }

    pub(crate) fn add(self, rhs: Self, sat: &mut Saturation) -> Self {
        match self.0.checked_add(rhs.0) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_add(rhs.0))
            }
        }
    }

    pub(crate) fn sub(self, rhs: Self, sat: &mut Saturation) -> Self {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_sub(rhs.0))
            }
        }
    }

    pub(crate) fn mul_i32(self, rhs: i32, sat: &mut Saturation) -> Self {
        match self.0.checked_mul(rhs) {
            Some(v) => Self(v),
            None => {
                sat.saturated += 1;
                Self(self.0.saturating_mul(rhs))
            }
        }
    }

    /// Division by zero yields zero.
    pub(crate) fn div_i32(self, rhs: i32) -> Self {
        if rhs == 0 {
            Self::ZERO
        } else {
            Self(self.0.saturating_div(rhs))
        }
    }
}

impl std::ops::Add for LayoutUnit {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub for LayoutUnit {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib geometry::unit`
Expected: PASS (5 tests).

The skeleton is built bottom-up, so some `pub(crate)` items stay unused until later tasks (and, for `BreakPlan` and the float fields, until plan S0-B) read them. Add this line to `src/lib.rs` directly below `#![forbid(unsafe_code)]`:

```rust
// Internal items are wired up incrementally; removed once all are in use.
#![allow(dead_code, unused_imports)]
```

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add Cargo.toml src/lib.rs src/geometry
git commit -m "Add LayoutUnit fixed-point type

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 2: Writing modes and `PhysicalConverter`

**Files:**
- Modify: `src/geometry/mod.rs`
- Test: `tests/geometry.rs`

**Interfaces:**
- Consumes: nothing from Task 1 publicly.
- Produces (all `pub`, re-exported through `shodo::geometry`): `enum WritingMode { HorizontalTb, VerticalRl, VerticalLr, SidewaysRl, SidewaysLr }` with `is_vertical()`; `enum Direction { Ltr, Rtl }`; `enum BaselineKind { Alphabetic, Central, Ideographic, Hanging }`; `struct LogicalRect { inline_start, block_start, inline_size, block_size: f32 }`; `struct PhysicalRect { x, y, width, height: f32 }`; `struct PhysicalSize { width, height: f32 }`; `struct PhysicalConverter` with `new(WritingMode, Direction, PhysicalSize)` and `rect(LogicalRect) -> PhysicalRect`.

- [ ] **Step 1: Write the failing test**

Create `tests/geometry.rs`:

```rust
use shodo::geometry::{
    Direction, LogicalRect, PhysicalConverter, PhysicalRect, PhysicalSize, WritingMode,
};

const CONTAINER: PhysicalSize = PhysicalSize { width: 200.0, height: 100.0 };
const R: LogicalRect = LogicalRect {
    inline_start: 10.0,
    block_start: 20.0,
    inline_size: 30.0,
    block_size: 40.0,
};

fn convert(wm: WritingMode, dir: Direction) -> PhysicalRect {
    PhysicalConverter::new(wm, dir, CONTAINER).rect(R)
}

#[test]
fn horizontal_tb() {
    assert_eq!(
        convert(WritingMode::HorizontalTb, Direction::Ltr),
        PhysicalRect { x: 10.0, y: 20.0, width: 30.0, height: 40.0 }
    );
    // RTL: inline-start is the right edge.
    assert_eq!(
        convert(WritingMode::HorizontalTb, Direction::Rtl),
        PhysicalRect { x: 160.0, y: 20.0, width: 30.0, height: 40.0 }
    );
}

#[test]
fn vertical_rl_blocks_flow_right_to_left() {
    assert_eq!(
        convert(WritingMode::VerticalRl, Direction::Ltr),
        PhysicalRect { x: 140.0, y: 10.0, width: 40.0, height: 30.0 }
    );
    assert_eq!(
        convert(WritingMode::VerticalRl, Direction::Rtl),
        PhysicalRect { x: 140.0, y: 60.0, width: 40.0, height: 30.0 }
    );
    assert_eq!(
        convert(WritingMode::SidewaysRl, Direction::Ltr),
        convert(WritingMode::VerticalRl, Direction::Ltr)
    );
}

#[test]
fn vertical_lr_and_sideways_lr() {
    assert_eq!(
        convert(WritingMode::VerticalLr, Direction::Ltr),
        PhysicalRect { x: 20.0, y: 10.0, width: 40.0, height: 30.0 }
    );
    // sideways-lr: inline direction runs bottom to top for LTR.
    assert_eq!(
        convert(WritingMode::SidewaysLr, Direction::Ltr),
        PhysicalRect { x: 20.0, y: 60.0, width: 40.0, height: 30.0 }
    );
    assert_eq!(
        convert(WritingMode::SidewaysLr, Direction::Rtl),
        PhysicalRect { x: 20.0, y: 10.0, width: 40.0, height: 30.0 }
    );
}

#[test]
fn vertical_predicate() {
    assert!(!WritingMode::HorizontalTb.is_vertical());
    assert!(WritingMode::VerticalRl.is_vertical());
    assert!(WritingMode::SidewaysLr.is_vertical());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test geometry`
Expected: FAIL to compile ("unresolved imports").

- [ ] **Step 3: Implement**

Append to `src/geometry/mod.rs`:

```rust
/// CSS `writing-mode` (CSS Writing Modes 4 §3.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WritingMode {
    #[default]
    HorizontalTb,
    VerticalRl,
    VerticalLr,
    SidewaysRl,
    SidewaysLr,
}

impl WritingMode {
    pub fn is_vertical(self) -> bool {
        !matches!(self, Self::HorizontalTb)
    }
}

/// CSS `direction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// Baseline types used for alignment (CSS Inline 3 §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BaselineKind {
    Alphabetic,
    Central,
    Ideographic,
    Hanging,
}

/// A rectangle in logical coordinates (inline / block axes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LogicalRect {
    pub inline_start: f32,
    pub block_start: f32,
    pub inline_size: f32,
    pub block_size: f32,
}

/// A rectangle in physical coordinates (x grows right, y grows down).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhysicalSize {
    pub width: f32,
    pub height: f32,
}

/// Converts logical rectangles inside a container to physical ones
/// (CSS Writing Modes 4 §6).
#[derive(Clone, Copy, Debug)]
pub struct PhysicalConverter {
    writing_mode: WritingMode,
    direction: Direction,
    container: PhysicalSize,
}

impl PhysicalConverter {
    pub fn new(writing_mode: WritingMode, direction: Direction, container: PhysicalSize) -> Self {
        Self { writing_mode, direction, container }
    }

    pub fn rect(&self, r: LogicalRect) -> PhysicalRect {
        let PhysicalSize { width: w, height: h } = self.container;
        let ltr = self.direction == Direction::Ltr;
        match self.writing_mode {
            WritingMode::HorizontalTb => PhysicalRect {
                x: if ltr { r.inline_start } else { w - r.inline_start - r.inline_size },
                y: r.block_start,
                width: r.inline_size,
                height: r.block_size,
            },
            WritingMode::VerticalRl | WritingMode::SidewaysRl => PhysicalRect {
                x: w - r.block_start - r.block_size,
                y: if ltr { r.inline_start } else { h - r.inline_start - r.inline_size },
                width: r.block_size,
                height: r.inline_size,
            },
            WritingMode::VerticalLr => PhysicalRect {
                x: r.block_start,
                y: if ltr { r.inline_start } else { h - r.inline_start - r.inline_size },
                width: r.block_size,
                height: r.inline_size,
            },
            WritingMode::SidewaysLr => PhysicalRect {
                x: r.block_start,
                y: if ltr { h - r.inline_start - r.inline_size } else { r.inline_start },
                width: r.block_size,
                height: r.inline_size,
            },
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test geometry`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/geometry/mod.rs tests/geometry.rs
git commit -m "Add writing modes and PhysicalConverter

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 3: `Limits`, `LimitExceeded`, and warnings

**Files:**
- Create: `src/limits.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Saturation` (Task 1).
- Produces:
  - `pub struct Limits` — all fields `pub Option<u64>`: `max_text_bytes`, `max_items`, `max_styles`, `max_nesting_depth`, `max_shaped_glyphs`, `max_reshape_window_bytes`, `max_shaping_run_bytes`, `max_balance_iterations`, `max_pretty_window_lines`, `max_warnings`, `max_font_blob_bytes`, `max_ttc_faces`, `max_font_axes`, `max_layout_lookups`, `max_layout_subtables`, `max_faces_per_layer`, `max_layer_blob_bytes`, `max_shaper_cache_entries`. `Default` gives `Some(..)` values; `Limits::unlimited()` gives all `None`.
  - `pub(crate) fn Limits::check(limit: Option<u64>, kind: LimitKind, actual: u64) -> Result<(), LimitExceeded>`.
  - `#[non_exhaustive] pub enum LimitKind { TextBytes, Items, Styles, NestingDepth, ShapedGlyphs, FontBlobBytes, TtcFaces, FontAxes, LayoutLookups, LayoutSubtables, FacesPerLayer, LayerBlobBytes }`.
  - `pub struct LimitExceeded { pub kind: LimitKind, pub limit: u64, pub actual: u64 }` implementing `Display` and `std::error::Error`.
  - `#[non_exhaustive] pub enum WarningKind { NonFiniteInput, NegativeInput, Saturated, UnbalancedInline, MissingAtomicSize, Unsupported, Suppressed }`; `pub struct Warning { pub kind: WarningKind, pub message: String }`.
  - `pub(crate) struct WarningSink` with `new(Option<u64>)`, `set_max(Option<u64>)`, `push(WarningKind, impl Into<String>)`, `record_saturation(&Saturation)`, `as_slice() -> &[Warning]`, `take() -> Vec<Warning>`.

- [ ] **Step 1: Write the failing tests**

Create `src/limits.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Saturation;

    #[test]
    fn check_reports_kind_limit_and_actual() {
        assert_eq!(Limits::check(Some(10), LimitKind::Items, 10), Ok(()));
        assert_eq!(
            Limits::check(Some(10), LimitKind::Items, 11),
            Err(LimitExceeded { kind: LimitKind::Items, limit: 10, actual: 11 })
        );
        assert_eq!(Limits::check(None, LimitKind::Items, u64::MAX), Ok(()));
    }

    #[test]
    fn defaults_are_bounded_and_unlimited_is_not() {
        let d = Limits::default();
        assert!(d.max_text_bytes.is_some() && d.max_layout_subtables.is_some());
        assert_eq!(Limits::unlimited().max_text_bytes, None);
    }

    #[test]
    fn warning_sink_caps_and_adds_one_suppression_marker() {
        let mut sink = WarningSink::new(Some(2));
        for _ in 0..5 {
            sink.push(WarningKind::NegativeInput, "x");
        }
        let kinds: Vec<_> = sink.as_slice().iter().map(|w| w.kind).collect();
        assert_eq!(
            kinds,
            vec![WarningKind::NegativeInput, WarningKind::NegativeInput, WarningKind::Suppressed]
        );
        // take() resets the sink for the next operation.
        assert_eq!(sink.take().len(), 3);
        sink.push(WarningKind::NegativeInput, "y");
        assert_eq!(sink.as_slice().len(), 1);
    }

    #[test]
    fn saturation_becomes_at_most_two_warnings() {
        let mut sink = WarningSink::new(None);
        sink.record_saturation(&Saturation { saturated: 3, non_finite: 2 });
        sink.record_saturation(&Saturation::default());
        let kinds: Vec<_> = sink.as_slice().iter().map(|w| w.kind).collect();
        assert_eq!(kinds, vec![WarningKind::NonFiniteInput, WarningKind::Saturated]);
    }

    #[test]
    fn limit_exceeded_displays() {
        let e = LimitExceeded { kind: LimitKind::TextBytes, limit: 1, actual: 2 };
        assert_eq!(e.to_string(), "limit exceeded: TextBytes (limit 1, actual 2)");
    }
}
```

Add to `src/lib.rs` after `pub mod geometry;`:

```rust
pub mod limits;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib limits`
Expected: FAIL to compile ("cannot find type `Limits`").

- [ ] **Step 3: Implement**

Insert above the tests in `src/limits.rs`:

```rust
//! Resource limits (fail-closed) and warnings.

use std::fmt;

use crate::geometry::Saturation;

/// Upper bounds on resources consumed by untrusted input. `None` disables a
/// limit. Limits are checked before allocating; exceeding one is an error,
/// never a silent truncation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_text_bytes: Option<u64>,
    pub max_items: Option<u64>,
    pub max_styles: Option<u64>,
    pub max_nesting_depth: Option<u64>,
    pub max_shaped_glyphs: Option<u64>,
    pub max_reshape_window_bytes: Option<u64>,
    pub max_shaping_run_bytes: Option<u64>,
    pub max_balance_iterations: Option<u64>,
    pub max_pretty_window_lines: Option<u64>,
    pub max_warnings: Option<u64>,
    pub max_font_blob_bytes: Option<u64>,
    pub max_ttc_faces: Option<u64>,
    pub max_font_axes: Option<u64>,
    pub max_layout_lookups: Option<u64>,
    pub max_layout_subtables: Option<u64>,
    pub max_faces_per_layer: Option<u64>,
    pub max_layer_blob_bytes: Option<u64>,
    pub max_shaper_cache_entries: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        const MIB: u64 = 1024 * 1024;
        Self {
            max_text_bytes: Some(16 * MIB),
            max_items: Some(1 << 20),
            max_styles: Some(1 << 16),
            max_nesting_depth: Some(512),
            max_shaped_glyphs: Some(1 << 22),
            max_reshape_window_bytes: Some(4096),
            max_shaping_run_bytes: Some(64 * 1024),
            max_balance_iterations: Some(16),
            max_pretty_window_lines: Some(4),
            max_warnings: Some(1024),
            max_font_blob_bytes: Some(32 * MIB),
            max_ttc_faces: Some(64),
            max_font_axes: Some(64),
            max_layout_lookups: Some(4096),
            max_layout_subtables: Some(65_536),
            max_faces_per_layer: Some(256),
            max_layer_blob_bytes: Some(256 * MIB),
            max_shaper_cache_entries: Some(64),
        }
    }
}

impl Limits {
    /// Every limit disabled. Only for trusted input.
    pub fn unlimited() -> Self {
        Self {
            max_text_bytes: None,
            max_items: None,
            max_styles: None,
            max_nesting_depth: None,
            max_shaped_glyphs: None,
            max_reshape_window_bytes: None,
            max_shaping_run_bytes: None,
            max_balance_iterations: None,
            max_pretty_window_lines: None,
            max_warnings: None,
            max_font_blob_bytes: None,
            max_ttc_faces: None,
            max_font_axes: None,
            max_layout_lookups: None,
            max_layout_subtables: None,
            max_faces_per_layer: None,
            max_layer_blob_bytes: None,
            max_shaper_cache_entries: None,
        }
    }

    pub(crate) fn check(limit: Option<u64>, kind: LimitKind, actual: u64) -> Result<(), LimitExceeded> {
        match limit {
            Some(limit) if actual > limit => Err(LimitExceeded { kind, limit, actual }),
            _ => Ok(()),
        }
    }
}

/// Which limit was exceeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LimitKind {
    TextBytes,
    Items,
    Styles,
    NestingDepth,
    ShapedGlyphs,
    FontBlobBytes,
    TtcFaces,
    FontAxes,
    LayoutLookups,
    LayoutSubtables,
    FacesPerLayer,
    LayerBlobBytes,
}

/// A resource limit was exceeded; the operation allocated nothing further.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitExceeded {
    pub kind: LimitKind,
    pub limit: u64,
    pub actual: u64,
}

impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "limit exceeded: {:?} (limit {}, actual {})", self.kind, self.limit, self.actual)
    }
}

impl std::error::Error for LimitExceeded {}

/// Category of a non-fatal problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WarningKind {
    NonFiniteInput,
    NegativeInput,
    Saturated,
    UnbalancedInline,
    MissingAtomicSize,
    Unsupported,
    /// Further warnings were dropped because `Limits::max_warnings` was reached.
    Suppressed,
}

/// A non-fatal problem: the output is still produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
}

/// Bounded warning collector. After `max` warnings it records a single
/// `Suppressed` marker and drops the rest, so hostile input cannot grow it.
#[derive(Clone, Debug, Default)]
pub(crate) struct WarningSink {
    warnings: Vec<Warning>,
    max: Option<u64>,
    suppressed: bool,
}

impl WarningSink {
    pub(crate) fn new(max: Option<u64>) -> Self {
        Self { warnings: Vec::new(), max, suppressed: false }
    }

    pub(crate) fn set_max(&mut self, max: Option<u64>) {
        self.max = max;
    }

    pub(crate) fn push(&mut self, kind: WarningKind, message: impl Into<String>) {
        if self.suppressed {
            return;
        }
        if let Some(max) = self.max
            && self.warnings.len() as u64 >= max
        {
            self.warnings.push(Warning {
                kind: WarningKind::Suppressed,
                message: "further warnings suppressed".into(),
            });
            self.suppressed = true;
            return;
        }
        self.warnings.push(Warning { kind, message: message.into() });
    }

    pub(crate) fn record_saturation(&mut self, sat: &Saturation) {
        if sat.non_finite > 0 {
            self.push(
                WarningKind::NonFiniteInput,
                format!("{} non-finite values replaced with 0", sat.non_finite),
            );
        }
        if sat.saturated > 0 {
            self.push(WarningKind::Saturated, format!("{} values saturated", sat.saturated));
        }
    }

    pub(crate) fn as_slice(&self) -> &[Warning] {
        &self.warnings
    }

    pub(crate) fn take(&mut self) -> Vec<Warning> {
        self.suppressed = false;
        std::mem::take(&mut self.warnings)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib limits`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/limits.rs src/lib.rs
git commit -m "Add fail-closed Limits and bounded warnings

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 4: Node identity and style types

**Files:**
- Create: `src/node.rs`
- Create: `src/style.rs`
- Modify: `src/lib.rs`
- Test: `tests/style.rs`

**Interfaces:**
- Consumes: `WritingMode`, `Direction` (Task 2).
- Produces:
  - `node.rs`: `pub struct NodeId(pub u64)` (Copy, Eq, Ord, Hash); `pub enum TextSource { Dom { node: NodeId, offset: u32 }, Generated { node: NodeId } }`; `pub struct Sides { inline_start, inline_end, block_start, block_end: f32 }` (Default, Copy) with `inline_sum()`; `pub struct InlineEdges { margin, border, padding: Sides }` (Default, Copy) with `inline_start_total()`, `inline_end_total()`; `pub enum OutOfFlowKind { Float, Absolute }`.
  - `style.rs`: `InlineStyle`, `ParagraphStyle`, `LineOptions` (all `Default`, `Clone`, `Debug`, `PartialEq`) plus the value types listed in the code below. Field names are part of the public API used by every later task: notably `InlineStyle::{font_size, line_height, white_space_collapse, direction, unicode_bidi, text_orientation, tab_size, box_decoration_break}`, `ParagraphStyle::{writing_mode, direction, unicode_bidi_plaintext, root, first_line}`, `LineOptions::{text_align, text_align_last, text_justify, text_indent, hanging_punctuation, text_wrap_style, text_box_trim}`.

- [ ] **Step 1: Write the failing test**

Create `tests/style.rs`:

```rust
use shodo::geometry::{Direction, WritingMode};
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    InlineStyle, LineHeight, LineOptions, ParagraphStyle, TabSize, TextAlign, TextOrientation,
    UnicodeBidi, WhiteSpaceCollapse,
};

#[test]
fn defaults_match_css_initial_values() {
    let s = InlineStyle::default();
    assert_eq!(s.font_size, 16.0);
    assert_eq!(s.line_height, LineHeight::Normal);
    assert_eq!(s.white_space_collapse, WhiteSpaceCollapse::Collapse);
    assert_eq!(s.direction, Direction::Ltr);
    assert_eq!(s.unicode_bidi, UnicodeBidi::Normal);
    assert_eq!(s.text_orientation, TextOrientation::Mixed);
    assert_eq!(s.tab_size, TabSize::Spaces(8.0));

    let p = ParagraphStyle::default();
    assert_eq!(p.writing_mode, WritingMode::HorizontalTb);
    assert!(p.first_line.is_none());

    assert_eq!(LineOptions::default().text_align, TextAlign::Start);
}

#[test]
fn edges_sum_margin_border_padding_per_side() {
    let side = |v: f32| Sides { inline_start: v, inline_end: v * 2.0, block_start: 0.0, block_end: 0.0 };
    let e = InlineEdges { margin: side(1.0), border: side(2.0), padding: side(3.0) };
    assert_eq!(e.inline_start_total(), 6.0);
    assert_eq!(e.inline_end_total(), 12.0);
    assert_eq!(side(1.0).inline_sum(), 3.0);
}

#[test]
fn text_source_carries_node() {
    let s = TextSource::Dom { node: NodeId(7), offset: 3 };
    assert_eq!(s.node(), NodeId(7));
    assert_eq!(TextSource::Generated { node: NodeId(8) }.node(), NodeId(8));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test style`
Expected: FAIL to compile ("unresolved import `shodo::node`").

- [ ] **Step 3: Implement `node.rs`**

Create `src/node.rs`:

```rust
//! Identities and box edges supplied by the caller.

/// Opaque caller-defined identifier: a DOM node for CSS engines, or any span
/// tag (for example an ECS entity) for other users.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

/// Where a piece of text came from, for mapping offsets back to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSource {
    /// Text of a DOM text node, starting at `offset` (UTF-8 bytes) within it.
    Dom { node: NodeId, offset: u32 },
    /// Generated content (`::before`, list markers) with no DOM offsets.
    Generated { node: NodeId },
}

impl TextSource {
    pub fn node(self) -> NodeId {
        match self {
            Self::Dom { node, .. } | Self::Generated { node } => node,
        }
    }
}

/// Four logical sides, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides {
    pub inline_start: f32,
    pub inline_end: f32,
    pub block_start: f32,
    pub block_end: f32,
}

impl Sides {
    pub fn inline_sum(&self) -> f32 {
        self.inline_start + self.inline_end
    }
}

/// Margin, border and padding of an inline box or atomic inline. The inline
/// sides take space in the line; the block sides do not affect the line box
/// height (CSS 2.1 §10.8.1) but are part of the reported border box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InlineEdges {
    pub margin: Sides,
    pub border: Sides,
    pub padding: Sides,
}

impl InlineEdges {
    pub fn inline_start_total(&self) -> f32 {
        self.margin.inline_start + self.border.inline_start + self.padding.inline_start
    }

    pub fn inline_end_total(&self) -> f32 {
        self.margin.inline_end + self.border.inline_end + self.padding.inline_end
    }
}

/// Out-of-flow boxes anchored in the inline content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutOfFlowKind {
    Float,
    Absolute,
}
```

- [ ] **Step 4: Implement `style.rs`**

Create `src/style.rs`:

```rust
//! Computed style values. Lengths are resolved px; percentages and `em` are
//! resolved by the caller, except values that depend on the font actually
//! used (for example `line-height: normal`), which shodo resolves.

use crate::geometry::{Direction, WritingMode};

#[derive(Clone, Debug, PartialEq)]
pub enum FontFamily {
    Named(String),
    Generic(GenericFamily),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// Oblique angle in degrees.
    Oblique(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontVariation {
    pub tag: [u8; 4],
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontFeature {
    pub tag: [u8; 4],
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontKerning {
    #[default]
    Auto,
    Normal,
    None,
}

/// `font-synthesis`: which faces may be synthesized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSynthesis {
    pub weight: bool,
    pub style: bool,
    pub small_caps: bool,
}

impl Default for FontSynthesis {
    fn default() -> Self {
        Self { weight: true, style: true, small_caps: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontMetricKind {
    ExHeight,
    CapHeight,
    ChWidth,
    IcWidth,
    IcHeight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSizeAdjust {
    pub metric: FontMetricKind,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LineHeight {
    /// Resolved from the metrics of the font actually used.
    #[default]
    Normal,
    Px(f32),
    /// Multiple of the element's font size.
    Number(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpaceCollapse {
    #[default]
    Collapse,
    Preserve,
    PreserveBreaks,
    PreserveSpaces,
    BreakSpaces,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrapMode {
    #[default]
    Wrap,
    NoWrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
    AutoPhrase,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverflowWrap {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hyphens {
    None,
    #[default]
    Manual,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextTransform {
    #[default]
    None,
    Capitalize,
    Uppercase,
    Lowercase,
    FullWidth,
    FullSizeKana,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSize {
    /// Multiple of the advance of the space character.
    Spaces(f32),
    Px(f32),
}

impl Default for TabSize {
    fn default() -> Self {
        Self::Spaces(8.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAutospace {
    #[default]
    Normal,
    NoAutospace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSpacingTrim {
    #[default]
    Normal,
    SpaceAll,
    TrimStart,
    SpaceFirst,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    Top,
    Bottom,
    Length(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOrientation {
    #[default]
    Mixed,
    Upright,
    Sideways,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextCombineUpright {
    #[default]
    None,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEmphasisShape {
    Dot,
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
    Custom(char),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextEmphasisPosition {
    #[default]
    OverRight,
    UnderRight,
    OverLeft,
    UnderLeft,
}

/// `text-emphasis`; affects the line box height, so it is layout input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEmphasis {
    pub shape: TextEmphasisShape,
    pub filled: bool,
    pub position: TextEmphasisPosition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBoxEdge {
    #[default]
    Auto,
    Text,
    Ideographic,
    Alphabetic,
    Cap,
    Ex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxDecorationBreak {
    #[default]
    Slice,
    Clone,
}

/// Per-element style that affects shaping and line breaking.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineStyle {
    pub font_families: Vec<FontFamily>,
    pub font_size: f32,
    pub font_weight: f32,
    /// `font-width` as a percentage (100 = normal).
    pub font_width: f32,
    pub font_style: FontStyle,
    pub font_variations: Vec<FontVariation>,
    pub font_features: Vec<FontFeature>,
    pub font_kerning: FontKerning,
    pub font_optical_sizing: bool,
    pub font_synthesis: FontSynthesis,
    pub font_size_adjust: Option<FontSizeAdjust>,
    /// BCP 47 language tag.
    pub lang: Option<String>,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub white_space_collapse: WhiteSpaceCollapse,
    pub text_wrap_mode: TextWrapMode,
    pub line_break: LineBreak,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub hyphens: Hyphens,
    pub hyphenate_character: Option<String>,
    pub text_transform: TextTransform,
    pub tab_size: TabSize,
    pub text_autospace: TextAutospace,
    pub text_spacing_trim: TextSpacingTrim,
    pub vertical_align: VerticalAlign,
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    pub text_orientation: TextOrientation,
    pub text_combine_upright: TextCombineUpright,
    pub text_emphasis: Option<TextEmphasis>,
    pub text_box_edge: TextBoxEdge,
    pub box_decoration_break: BoxDecorationBreak,
}

impl Default for InlineStyle {
    fn default() -> Self {
        Self {
            font_families: vec![FontFamily::Generic(GenericFamily::SansSerif)],
            font_size: 16.0,
            font_weight: 400.0,
            font_width: 100.0,
            font_style: FontStyle::default(),
            font_variations: Vec::new(),
            font_features: Vec::new(),
            font_kerning: FontKerning::default(),
            font_optical_sizing: true,
            font_synthesis: FontSynthesis::default(),
            font_size_adjust: None,
            lang: None,
            line_height: LineHeight::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            white_space_collapse: WhiteSpaceCollapse::default(),
            text_wrap_mode: TextWrapMode::default(),
            line_break: LineBreak::default(),
            word_break: WordBreak::default(),
            overflow_wrap: OverflowWrap::default(),
            hyphens: Hyphens::default(),
            hyphenate_character: None,
            text_transform: TextTransform::default(),
            tab_size: TabSize::default(),
            text_autospace: TextAutospace::default(),
            text_spacing_trim: TextSpacingTrim::default(),
            vertical_align: VerticalAlign::default(),
            direction: Direction::default(),
            unicode_bidi: UnicodeBidi::default(),
            text_orientation: TextOrientation::default(),
            text_combine_upright: TextCombineUpright::default(),
            text_emphasis: None,
            text_box_edge: TextBoxEdge::default(),
            box_decoration_break: BoxDecorationBreak::default(),
        }
    }
}

/// Style of the block container. Changing it requires a rebuild.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphStyle {
    pub writing_mode: WritingMode,
    pub direction: Direction,
    /// `unicode-bidi: plaintext` on the block container.
    pub unicode_bidi_plaintext: bool,
    /// Style of the root inline box; also the source of the strut.
    pub root: InlineStyle,
    /// Style applied by `::first-line`, if any.
    pub first_line: Option<InlineStyle>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
    JustifyAll,
    MatchParent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignLast {
    #[default]
    Auto,
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextJustify {
    #[default]
    Auto,
    None,
    InterWord,
    InterCharacter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextIndent {
    pub length: f32,
    pub hanging: bool,
    pub each_line: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HangingPunctuation {
    pub first: bool,
    pub force_end: bool,
    pub allow_end: bool,
    pub last: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrapStyle {
    #[default]
    Auto,
    Balance,
    Pretty,
    Stable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextBoxTrim {
    #[default]
    None,
    TrimStart,
    TrimEnd,
    TrimBoth,
}

/// Options that only affect line layout; changing them needs no rebuild.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineOptions {
    pub text_align: TextAlign,
    pub text_align_last: TextAlignLast,
    pub text_justify: TextJustify,
    pub text_indent: TextIndent,
    pub hanging_punctuation: HangingPunctuation,
    pub text_wrap_style: TextWrapStyle,
    pub text_box_trim: TextBoxTrim,
}
```

Add to `src/lib.rs` after `pub mod limits;`:

```rust
pub mod node;
pub mod style;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --test style`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/node.rs src/style.rs src/lib.rs tests/style.rs
git commit -m "Add node identity and style types

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 5: Structural check of fonts before registration

Hostile fonts can make the shaper expand a small table into a huge cache: harfrust caches one entry per lookup *index* and one per subtable *reference*, and duplicate references are not rejected by the parser. So before a blob is registered, count lookups and subtables per reference (duplicates counted every time) and reject the font if the totals exceed `Limits::max_layout_lookups` / `max_layout_subtables`. The walk stops as soon as a limit is exceeded, so the check itself is bounded.

**Files:**
- Create: `src/font/sfnt.rs`
- Create: `src/font/check.rs`
- Create: `src/font/mod.rs` (module declarations only in this task)
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Limits`, `LimitExceeded`, `LimitKind` (Task 3).
- Produces:
  - `pub(crate) fn sfnt::build_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8>`
  - `pub(crate) fn sfnt::amplifying_layout_table(lookup_count: u16, subtable_count: u16) -> Vec<u8>` (`#[cfg(test)]`)
  - `pub enum FontError { Limit(LimitExceeded), Malformed(&'static str) }` with `Display`, `Error`, `From<LimitExceeded>`
  - `pub(crate) fn check::check_font(data: &[u8], limits: &Limits) -> Result<u32, FontError>` returning the face count

- [ ] **Step 1: Write the sfnt writer**

This is test infrastructure and the source of the built-in stub face, not the code under test. Create `src/font/sfnt.rs`:

```rust
//! Minimal sfnt (OpenType) writer. Produces the built-in stub face and fonts
//! for tests. Checksums and search hints are left zero: nothing in shodo
//! validates them.

pub(crate) fn build_sfnt(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        body.extend_from_slice(data);
        let pad = (4 - data.len() % 4) % 4;
        body.extend(std::iter::repeat_n(0u8, pad));
        offset += data.len() + pad;
    }
    out.extend_from_slice(&body);
    out
}

/// A GSUB/GPOS table whose LookupList has `lookup_count` entries that all
/// point at one Lookup, whose `subtable_count` subtable offsets all point at
/// one empty SingleSubst subtable. Tiny on disk, huge once expanded per
/// reference. Counts must be at most 16 000 so offsets fit in 16 bits.
#[cfg(test)]
pub(crate) fn amplifying_layout_table(lookup_count: u16, subtable_count: u16) -> Vec<u8> {
    assert!(lookup_count <= 16_000 && subtable_count <= 16_000);
    let mut t = Vec::new();
    let put = |t: &mut Vec<u8>, v: u16| t.extend_from_slice(&v.to_be_bytes());
    // Header: version 1.0, no ScriptList/FeatureList, LookupList at offset 10.
    for v in [1, 0, 0, 0, 10] {
        put(&mut t, v);
    }
    // LookupList: every entry points at the Lookup placed right after it.
    put(&mut t, lookup_count);
    let lookup_offset = 2 + 2 * lookup_count;
    for _ in 0..lookup_count {
        put(&mut t, lookup_offset);
    }
    // Lookup: type 1 (single substitution), flags 0.
    put(&mut t, 1);
    put(&mut t, 0);
    put(&mut t, subtable_count);
    let subtable_offset = 6 + 2 * subtable_count;
    for _ in 0..subtable_count {
        put(&mut t, subtable_offset);
    }
    // SingleSubst format 1 with an empty format-1 Coverage at offset 6.
    for v in [1, 6, 0, 1, 0] {
        put(&mut t, v);
    }
    t
}
```

Create `src/font/mod.rs`:

```rust
//! Fonts: registration, identity and metrics.

mod check;
pub(crate) mod sfnt;

pub use check::FontError;
```

Add to `src/lib.rs` after `pub mod limits;`:

```rust
pub mod font;
```

- [ ] **Step 2: Write the failing tests**

Create `src/font/check.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::sfnt::{amplifying_layout_table, build_sfnt};

    fn limits() -> Limits {
        Limits::default()
    }

    #[test]
    fn accepts_plain_font_and_counts_one_face() {
        let font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(2, 2))]);
        assert_eq!(check_font(&font, &limits()), Ok(1));
        assert_eq!(check_font(&build_sfnt(&[]), &limits()), Ok(1));
    }

    #[test]
    fn rejects_duplicate_subtable_references() {
        // About 48 KiB of GSUB that would expand to 8192 * 8192 cache entries.
        let font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(8192, 8192))]);
        let mut l = limits();
        l.max_layout_lookups = None;
        match check_font(&font, &l) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayoutSubtables),
            other => panic!("expected LayoutSubtables limit, got {other:?}"),
        }
    }

    #[test]
    fn rejects_duplicate_lookup_references() {
        let font = build_sfnt(&[(*b"GPOS", amplifying_layout_table(8192, 1))]);
        match check_font(&font, &limits()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayoutLookups),
            other => panic!("expected LayoutLookups limit, got {other:?}"),
        }
    }

    #[test]
    fn gsub_and_gpos_share_one_budget() {
        let font = build_sfnt(&[
            (*b"GSUB", amplifying_layout_table(1, 3)),
            (*b"GPOS", amplifying_layout_table(1, 3)),
        ]);
        let mut l = limits();
        l.max_layout_subtables = Some(5);
        assert!(matches!(check_font(&font, &l), Err(FontError::Limit(_))));
        l.max_layout_subtables = Some(6);
        assert_eq!(check_font(&font, &l), Ok(1));
    }

    #[test]
    fn rejects_truncated_and_out_of_bounds_data() {
        assert!(matches!(check_font(&[0, 1, 0], &limits()), Err(FontError::Malformed(_))));
        let mut font = build_sfnt(&[(*b"GSUB", amplifying_layout_table(2, 2))]);
        font.truncate(30);
        assert!(matches!(check_font(&font, &limits()), Err(FontError::Malformed(_))));
    }

    #[test]
    fn enforces_blob_size_axis_and_collection_limits() {
        let mut l = limits();
        l.max_font_blob_bytes = Some(4);
        assert!(matches!(check_font(&build_sfnt(&[]), &l), Err(FontError::Limit(_))));

        let mut fvar = vec![0u8; 16];
        fvar[8..10].copy_from_slice(&2u16.to_be_bytes());
        let mut l = limits();
        l.max_font_axes = Some(1);
        let font = build_sfnt(&[(*b"fvar", fvar)]);
        assert!(matches!(check_font(&font, &l), Err(FontError::Limit(_))));

        // 'ttcf' header claiming 3 faces.
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&3u32.to_be_bytes());
        let mut l = limits();
        l.max_ttc_faces = Some(2);
        match check_font(&ttc, &l) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::TtcFaces),
            other => panic!("expected TtcFaces limit, got {other:?}"),
        }
    }

    #[test]
    fn counts_faces_in_a_collection() {
        let face = build_sfnt(&[]);
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&2u32.to_be_bytes());
        let first = 12 + 8;
        ttc.extend_from_slice(&(first as u32).to_be_bytes());
        ttc.extend_from_slice(&((first + face.len()) as u32).to_be_bytes());
        ttc.extend_from_slice(&face);
        ttc.extend_from_slice(&face);
        assert_eq!(check_font(&ttc, &limits()), Ok(2));
    }
}
```

Note: the second face in `counts_faces_in_a_collection` is a copy whose table offsets are relative to the file start; since it has no tables, that does not matter.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test --lib font::check`
Expected: FAIL to compile ("cannot find function `check_font`").

- [ ] **Step 4: Implement the check**

Insert above the tests in `src/font/check.rs`:

```rust
//! Structural check of font blobs before registration.

use std::fmt;

use crate::limits::{LimitExceeded, LimitKind, Limits};

/// Why a font blob was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontError {
    Limit(LimitExceeded),
    Malformed(&'static str),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(e) => e.fmt(f),
            Self::Malformed(what) => write!(f, "malformed font: {what}"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<LimitExceeded> for FontError {
    fn from(e: LimitExceeded) -> Self {
        Self::Limit(e)
    }
}

const TRUNCATED: FontError = FontError::Malformed("truncated or out of bounds");

fn offset(base: usize, add: usize) -> Result<usize, FontError> {
    base.checked_add(add).ok_or(TRUNCATED)
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, FontError> {
    let bytes = data.get(at..offset(at, 2)?).ok_or(TRUNCATED)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, FontError> {
    let bytes = data.get(at..offset(at, 4)?).ok_or(TRUNCATED)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Checks a font file or collection against `limits` and returns its number
/// of faces.
pub(crate) fn check_font(data: &[u8], limits: &Limits) -> Result<u32, FontError> {
    Limits::check(limits.max_font_blob_bytes, LimitKind::FontBlobBytes, data.len() as u64)?;
    if data.get(0..4) == Some(b"ttcf".as_slice()) {
        let count = read_u32(data, 8)?;
        Limits::check(limits.max_ttc_faces, LimitKind::TtcFaces, u64::from(count))?;
        if count == 0 {
            return Err(FontError::Malformed("empty font collection"));
        }
        for i in 0..count as usize {
            let face = read_u32(data, offset(12, 4 * i)?)? as usize;
            check_face(data, face, limits)?;
        }
        Ok(count)
    } else {
        check_face(data, 0, limits)?;
        Ok(1)
    }
}

fn check_face(data: &[u8], face: usize, limits: &Limits) -> Result<(), FontError> {
    let num_tables = read_u16(data, offset(face, 4)?)? as usize;
    let mut lookups = 0u64;
    let mut subtables = 0u64;
    for i in 0..num_tables {
        let record = offset(offset(face, 12)?, 16 * i)?;
        let tag = data.get(record..offset(record, 4)?).ok_or(TRUNCATED)?;
        let start = read_u32(data, offset(record, 8)?)? as usize;
        let len = read_u32(data, offset(record, 12)?)? as usize;
        let table = data.get(start..offset(start, len)?).ok_or(TRUNCATED)?;
        match tag {
            b"GSUB" | b"GPOS" => count_layout(table, &mut lookups, &mut subtables, limits)?,
            b"fvar" => {
                let axes = read_u16(table, 8)?;
                Limits::check(limits.max_font_axes, LimitKind::FontAxes, u64::from(axes))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Counts lookups and subtables per reference: a Lookup referenced by N
/// LookupList entries is counted N times, and so are its subtables. This is
/// the number of cache entries the shaper creates. Extension subtables
/// resolve to exactly one subtable each, so the declared count is exact.
/// Stops at the first exceeded limit, so the walk is bounded.
fn count_layout(
    table: &[u8],
    lookups: &mut u64,
    subtables: &mut u64,
    limits: &Limits,
) -> Result<(), FontError> {
    let list = read_u16(table, 8)? as usize;
    if list == 0 {
        return Ok(());
    }
    let count = read_u16(table, list)?;
    *lookups += u64::from(count);
    Limits::check(limits.max_layout_lookups, LimitKind::LayoutLookups, *lookups)?;
    for i in 0..count as usize {
        let lookup = offset(list, read_u16(table, offset(list, 2 + 2 * i)?)? as usize)?;
        let declared = read_u16(table, offset(lookup, 4)?)?;
        *subtables += u64::from(declared);
        Limits::check(limits.max_layout_subtables, LimitKind::LayoutSubtables, *subtables)?;
    }
    Ok(())
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib font::check`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/font src/lib.rs
git commit -m "Reject fonts whose layout tables amplify through duplicate references

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 6: `FontCollection` stub with layer budgets

**Files:**
- Modify: `src/font/mod.rs`

**Interfaces:**
- Consumes: `check_font`, `build_sfnt`, `FontError` (Task 5); `Limits`, `LimitKind` (Task 3).
- Produces:
  - `pub struct FontId` (Copy, Eq, Ord, Hash) with `layer() -> u32`, `index() -> u32`.
  - `pub struct FontMetrics { pub ascent, descent, line_gap, underline_offset, underline_thickness, strikeout_offset, strikeout_thickness: f32 }` — px for the requested size; `descent` and `underline_offset` are positive downward from the baseline.
  - `pub struct FontCollection` (Clone, Send, Sync, Debug): `new(&Limits)`, `for_document(&FontCollection, &Limits)`, `register(Vec<u8>) -> Result<FontId, FontError>`, `font_data(FontId) -> Option<peniko::FontData>`, `generation() -> u64`, `metrics(FontId, f32) -> FontMetrics`; `pub(crate)`: `generations() -> (u64, Option<u64>)`, `primary_font() -> FontId`.

- [ ] **Step 1: Write the failing tests**

Append to `src/font/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{LimitKind, Limits};

    fn font_bytes() -> Vec<u8> {
        sfnt::build_sfnt(&[])
    }

    #[test]
    fn shared_layer_has_a_stub_face() {
        let fonts = FontCollection::new(&Limits::default());
        let id = fonts.primary_font();
        assert_eq!(id.index(), 0);
        assert!(fonts.font_data(id).is_some());
        assert_eq!(fonts.generations(), (0, None));
    }

    #[test]
    fn register_bumps_generation_and_returns_first_face() {
        let fonts = FontCollection::new(&Limits::default());
        let id = fonts.register(font_bytes()).unwrap();
        assert_eq!(id.index(), 1);
        assert_eq!(fonts.generation(), 1);
        assert_eq!(fonts.font_data(id).unwrap().index, 0);
    }

    #[test]
    fn rejected_fonts_leave_the_layer_unchanged() {
        let fonts = FontCollection::new(&Limits::default());
        assert!(fonts.register(vec![1, 2, 3]).is_err());
        assert_eq!(fonts.generation(), 0);
    }

    #[test]
    fn layer_budgets_are_checked_before_storing() {
        let mut limits = Limits::default();
        limits.max_faces_per_layer = Some(2); // the stub face counts
        let fonts = FontCollection::new(&limits);
        fonts.register(font_bytes()).unwrap();
        match fonts.register(font_bytes()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::FacesPerLayer),
            other => panic!("expected FacesPerLayer, got {other:?}"),
        }

        let mut limits = Limits::default();
        limits.max_layer_blob_bytes = Some(15);
        let doc = FontCollection::for_document(&FontCollection::new(&Limits::default()), &limits);
        doc.register(font_bytes()).unwrap(); // 12 bytes
        match doc.register(font_bytes()) {
            Err(FontError::Limit(e)) => assert_eq!(e.kind, LimitKind::LayerBlobBytes),
            other => panic!("expected LayerBlobBytes, got {other:?}"),
        }
    }

    #[test]
    fn document_layers_are_isolated() {
        let shared = FontCollection::new(&Limits::default());
        let a = FontCollection::for_document(&shared, &Limits::default());
        let b = FontCollection::for_document(&shared, &Limits::default());
        let id = a.register(font_bytes()).unwrap();
        assert!(a.font_data(id).is_some());
        assert!(b.font_data(id).is_none());
        assert!(shared.font_data(id).is_none());
        // Shared faces are visible through a document layer.
        assert!(a.font_data(shared.primary_font()).is_some());
        assert_eq!(a.primary_font(), id);
        assert_eq!(b.primary_font(), shared.primary_font());
        assert_eq!(a.generations(), (0, Some(1)));
    }

    #[test]
    fn stub_metrics_scale_with_size() {
        let fonts = FontCollection::new(&Limits::default());
        let m = fonts.metrics(fonts.primary_font(), 10.0);
        assert_eq!((m.ascent, m.descent, m.line_gap), (8.0, 2.0, 0.0));
    }

    #[test]
    fn collection_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FontCollection>();
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib font::tests`
Expected: FAIL to compile ("cannot find type `FontCollection`").

- [ ] **Step 3: Implement**

Insert into `src/font/mod.rs` between `pub use check::FontError;` and the tests:

```rust
use std::fmt;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use peniko::{Blob, FontData};

use crate::limits::{LimitKind, Limits};

static NEXT_LAYER_ID: AtomicU32 = AtomicU32::new(0);

/// Identifies a face: the layer it was registered in and its index there.
/// Stable for the lifetime of the layer; layer ids are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId {
    layer: u32,
    index: u32,
}

impl FontId {
    pub fn layer(self) -> u32 {
        self.layer
    }

    pub fn index(self) -> u32 {
        self.index
    }
}

/// Font metrics in px for a given size. `descent` and `underline_offset`
/// are measured downward from the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub underline_offset: f32,
    pub underline_thickness: f32,
    pub strikeout_offset: f32,
    pub strikeout_thickness: f32,
}

struct Layer {
    id: u32,
    limits: Limits,
    generation: AtomicU64,
    state: Mutex<LayerState>,
    parent: Option<FontCollection>,
}

struct LayerState {
    faces: Vec<FontData>,
    blob_bytes: u64,
}

/// A layer of fonts. The shared layer (created with [`FontCollection::new`])
/// holds application fonts; document layers (created with
/// [`FontCollection::for_document`]) hold `@font-face` fonts and see the
/// shared layer, but not each other. Cheap to clone.
#[derive(Clone)]
pub struct FontCollection {
    layer: Arc<Layer>,
}

impl fmt::Debug for FontCollection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontCollection").field("layer", &self.layer.id).finish()
    }
}

impl FontCollection {
    /// Creates the shared layer. It contains a built-in stub face at index 0,
    /// so layout works without any system or bundled fonts.
    pub fn new(limits: &Limits) -> Self {
        let stub = FontData::new(Blob::from(sfnt::build_sfnt(&[])), 0);
        Self::with_faces(limits, vec![stub], None)
    }

    /// Creates an empty document layer on top of `shared`.
    pub fn for_document(shared: &FontCollection, limits: &Limits) -> Self {
        Self::with_faces(limits, Vec::new(), Some(shared.clone()))
    }

    fn with_faces(limits: &Limits, faces: Vec<FontData>, parent: Option<FontCollection>) -> Self {
        Self {
            layer: Arc::new(Layer {
                id: NEXT_LAYER_ID.fetch_add(1, Ordering::Relaxed),
                limits: limits.clone(),
                generation: AtomicU64::new(0),
                state: Mutex::new(LayerState { faces, blob_bytes: 0 }),
                parent,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, LayerState> {
        // A panic while holding the lock cannot leave the state inconsistent
        // (every update is a single push), so a poisoned lock is recovered.
        self.layer.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Checks and registers a font file or collection. Returns the id of its
    /// first face. On error nothing is stored.
    pub fn register(&self, data: Vec<u8>) -> Result<FontId, FontError> {
        let limits = &self.layer.limits;
        let faces = check::check_font(&data, limits)?;
        let mut state = self.state();
        Limits::check(
            limits.max_faces_per_layer,
            LimitKind::FacesPerLayer,
            state.faces.len() as u64 + u64::from(faces),
        )?;
        let bytes = state.blob_bytes + data.len() as u64;
        Limits::check(limits.max_layer_blob_bytes, LimitKind::LayerBlobBytes, bytes)?;
        state.blob_bytes = bytes;
        let first = state.faces.len() as u32;
        let blob = Blob::from(data);
        for index in 0..faces {
            state.faces.push(FontData::new(blob.clone(), index));
        }
        self.layer.generation.fetch_add(1, Ordering::SeqCst);
        Ok(FontId { layer: self.layer.id, index: first })
    }

    /// Font data of a face in this layer or its shared layer.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        if id.layer == self.layer.id {
            self.state().faces.get(id.index as usize).cloned()
        } else {
            self.layer.parent.as_ref().and_then(|p| p.font_data(id))
        }
    }

    /// Incremented by every successful registration in this layer.
    pub fn generation(&self) -> u64 {
        self.layer.generation.load(Ordering::SeqCst)
    }

    /// (shared layer generation, document layer generation).
    pub(crate) fn generations(&self) -> (u64, Option<u64>) {
        match &self.layer.parent {
            Some(shared) => (shared.generation(), Some(self.generation())),
            None => (self.generation(), None),
        }
    }

    /// Placeholder for font matching: the first face of a document layer,
    /// otherwise the shared stub face.
    pub(crate) fn primary_font(&self) -> FontId {
        match &self.layer.parent {
            Some(shared) if self.state().faces.is_empty() => shared.primary_font(),
            _ => FontId { layer: self.layer.id, index: 0 },
        }
    }

    /// Metrics of a face at `size` px. Placeholder values per em: ascent 0.8,
    /// descent 0.2, no line gap.
    pub fn metrics(&self, _id: FontId, size: f32) -> FontMetrics {
        FontMetrics {
            ascent: 0.8 * size,
            descent: 0.2 * size,
            line_gap: 0.0,
            underline_offset: 0.1 * size,
            underline_thickness: 0.05 * size,
            strikeout_offset: -0.3 * size,
            strikeout_thickness: 0.05 * size,
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib font`
Expected: PASS (7 `font::check` tests + 7 `font::tests`).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/font/mod.rs
git commit -m "Add FontCollection stub with shared and document layers

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 7: `ParagraphBuilder` recording with incremental limits

`build()` is added in Task 10. This task records raw input and enforces limits on every push.

**Files:**
- Create: `src/builder.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `Limits`, `LimitExceeded`, `LimitKind`, `WarningKind`, `WarningSink` (Task 3); `NodeId`, `TextSource`, `InlineEdges`, `OutOfFlowKind` (Task 4); `InlineStyle`, `ParagraphStyle` (Task 4).
- Produces:
  - `pub struct ParagraphBuilder` with `new(&ParagraphStyle, &Limits)`, and chaining methods returning `&mut Self`: `open_inline(NodeId, &InlineStyle, InlineEdges)`, `close_inline()`, `push_text(TextSource, &str)`, `push_atomic(NodeId, &InlineStyle, InlineEdges)`, `push_out_of_flow(NodeId, OutOfFlowKind)`, `push_block_in_inline(NodeId)`, `push_forced_break(NodeId)`, `with_offset_mapping(bool)`; plus `error() -> Option<LimitExceeded>`.
  - Crate-visible fields used by Task 8/10: `style: ParagraphStyle`, `limits: Limits`, `text: String`, `items: Vec<RawItem>`, `styles: Vec<InlineStyle>` (index 0 is the root style), `stack: Vec<u32>`, `error: Option<LimitExceeded>`, `warnings: WarningSink`, `offset_mapping: bool`.
  - `pub(crate) enum RawItem { Text { source: TextSource, range: Range<u32>, style: u32 }, Open { node: NodeId, style: u32, edges: InlineEdges }, Close, Atomic { node: NodeId, style: u32, parent_style: u32, edges: InlineEdges }, OutOfFlow { node: NodeId, kind: OutOfFlowKind, style: u32 }, BlockInInline { node: NodeId, style: u32 }, ForcedBreak { node: NodeId, style: u32 } }` — `range` indexes `ParagraphBuilder::text`.

- [ ] **Step 1: Write the failing tests**

Create `src/builder.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{LimitKind, Limits, WarningKind};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle};

    fn dom(node: u64) -> TextSource {
        TextSource::Dom { node: NodeId(node), offset: 0 }
    }

    fn bold() -> InlineStyle {
        InlineStyle { font_weight: 700.0, ..InlineStyle::default() }
    }

    #[test]
    fn records_items_and_shares_equal_styles() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), "ab")
            .open_inline(NodeId(2), &bold(), InlineEdges::default())
            .push_text(dom(3), "cd")
            .close_inline()
            .open_inline(NodeId(4), &bold(), InlineEdges::default())
            .close_inline();
        assert_eq!(b.text, "abcd");
        assert_eq!(b.items.len(), 6);
        assert_eq!(b.styles.len(), 2, "root + one shared bold style");
        assert!(b.stack.is_empty());
        assert_eq!(b.error(), None);
    }

    #[test]
    fn empty_text_is_not_recorded() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), "");
        assert!(b.items.is_empty());
    }

    #[test]
    fn text_limit_stops_further_allocation() {
        let limits = Limits { max_text_bytes: Some(5), ..Limits::default() };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_text(dom(1), "abc").push_text(dom(1), "def").push_text(dom(1), "x");
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::TextBytes));
        assert_eq!(b.text, "abc");
        assert_eq!(b.items.len(), 1);
    }

    #[test]
    fn nesting_items_and_style_limits() {
        let limits = Limits { max_nesting_depth: Some(1), ..Limits::default() };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.open_inline(NodeId(1), &InlineStyle::default(), InlineEdges::default())
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::NestingDepth));
        assert_eq!(b.stack.len(), 1);

        let limits = Limits { max_items: Some(2), ..Limits::default() };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.push_text(dom(1), "a").push_text(dom(1), "b").push_text(dom(1), "c");
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::Items));
        assert_eq!(b.items.len(), 2);

        let limits = Limits { max_styles: Some(1), ..Limits::default() };
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
        b.open_inline(NodeId(1), &bold(), InlineEdges::default());
        assert_eq!(b.error().map(|e| e.kind), Some(LimitKind::Styles));
        assert_eq!(b.styles.len(), 1);
    }

    #[test]
    fn unbalanced_close_is_ignored_with_a_warning() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.close_inline();
        assert!(b.items.is_empty());
        assert_eq!(b.warnings.as_slice()[0].kind, WarningKind::UnbalancedInline);
    }

    #[test]
    fn atomics_remember_their_parent_style() {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.open_inline(NodeId(1), &bold(), InlineEdges::default())
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default());
        match &b.items[1] {
            RawItem::Atomic { parent_style, style, .. } => {
                assert_eq!(*parent_style, 1);
                assert_eq!(*style, 0, "default style is shared with the root");
            }
            _ => panic!("expected an atomic"),
        }
    }
}
```

Add to `src/lib.rs` after the `pub mod` lines:

```rust
mod builder;

pub use builder::ParagraphBuilder;
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib builder`
Expected: FAIL to compile ("cannot find type `ParagraphBuilder`").

- [ ] **Step 3: Implement**

Insert above the tests in `src/builder.rs`:

```rust
//! Paragraph builders: record the inline content of one block container.

use std::collections::HashMap;
use std::ops::Range;

use crate::limits::{LimitExceeded, LimitKind, Limits, WarningKind, WarningSink};
use crate::node::{InlineEdges, NodeId, OutOfFlowKind, TextSource};
use crate::style::{InlineStyle, ParagraphStyle};

/// Input as recorded, before white-space processing.
#[derive(Clone, Debug)]
pub(crate) enum RawItem {
    Text { source: TextSource, range: Range<u32>, style: u32 },
    Open { node: NodeId, style: u32, edges: InlineEdges },
    Close,
    Atomic { node: NodeId, style: u32, parent_style: u32, edges: InlineEdges },
    OutOfFlow { node: NodeId, kind: OutOfFlowKind, style: u32 },
    BlockInInline { node: NodeId, style: u32 },
    ForcedBreak { node: NodeId, style: u32 },
}

/// Builds a [`crate::Paragraph`] from the inline content of a block
/// container.
///
/// Limits are checked on every call. After the first violation the builder
/// ignores further input without allocating, and `build` returns the error.
pub struct ParagraphBuilder {
    pub(crate) style: ParagraphStyle,
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<RawItem>,
    /// Interned styles; index 0 is the paragraph's root style.
    pub(crate) styles: Vec<InlineStyle>,
    style_index: HashMap<String, u32>,
    /// Style indices of the currently open inline boxes.
    pub(crate) stack: Vec<u32>,
    pub(crate) error: Option<LimitExceeded>,
    pub(crate) warnings: WarningSink,
    pub(crate) offset_mapping: bool,
}

impl ParagraphBuilder {
    pub fn new(style: &ParagraphStyle, limits: &Limits) -> Self {
        let mut builder = Self {
            style: style.clone(),
            limits: limits.clone(),
            text: String::new(),
            items: Vec::new(),
            styles: Vec::new(),
            style_index: HashMap::new(),
            stack: Vec::new(),
            error: None,
            warnings: WarningSink::new(limits.max_warnings),
            offset_mapping: true,
        };
        builder.styles.push(style.root.clone());
        builder.style_index.insert(format!("{:?}", style.root), 0);
        builder
    }

    /// The first limit violation, if any.
    pub fn error(&self) -> Option<LimitExceeded> {
        self.error
    }

    /// Whether to build an [`crate::mapping::OffsetMapping`] (default true).
    /// Disable it when offsets are never mapped back, for example for PDF.
    pub fn with_offset_mapping(&mut self, enabled: bool) -> &mut Self {
        self.offset_mapping = enabled;
        self
    }

    pub fn open_inline(&mut self, node: NodeId, style: &InlineStyle, edges: InlineEdges) -> &mut Self {
        let depth = self.stack.len() as u64 + 1;
        if self.check(self.limits.max_nesting_depth, LimitKind::NestingDepth, depth)
            && self.reserve_item()
            && let Some(style) = self.intern(style)
        {
            self.items.push(RawItem::Open { node, style, edges });
            self.stack.push(style);
        }
        self
    }

    pub fn close_inline(&mut self) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        if self.stack.is_empty() {
            self.warnings.push(WarningKind::UnbalancedInline, "close_inline without open_inline ignored");
            return self;
        }
        if self.reserve_item() {
            self.items.push(RawItem::Close);
            self.stack.pop();
        }
        self
    }

    pub fn push_text(&mut self, source: TextSource, text: &str) -> &mut Self {
        if self.error.is_some() || text.is_empty() {
            return self;
        }
        let total = self.text.len() as u64 + text.len() as u64;
        // Offsets are stored as u32, so that is a hard ceiling as well.
        let limit = self.limits.max_text_bytes.map_or(u64::from(u32::MAX), |l| l.min(u64::from(u32::MAX)));
        if self.check(Some(limit), LimitKind::TextBytes, total) && self.reserve_item() {
            let start = self.text.len() as u32;
            self.text.push_str(text);
            let style = self.current_style();
            self.items.push(RawItem::Text { source, range: start..self.text.len() as u32, style });
        }
        self
    }

    pub fn push_atomic(&mut self, node: NodeId, style: &InlineStyle, edges: InlineEdges) -> &mut Self {
        if self.reserve_item()
            && let Some(style) = self.intern(style)
        {
            let parent_style = self.current_style();
            self.items.push(RawItem::Atomic { node, style, parent_style, edges });
        }
        self
    }

    pub fn push_out_of_flow(&mut self, node: NodeId, kind: OutOfFlowKind) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::OutOfFlow { node, kind, style });
        }
        self
    }

    /// A block-level box inside inline content (CSS 2.1 §9.2.1.1).
    pub fn push_block_in_inline(&mut self, node: NodeId) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::BlockInInline { node, style });
        }
        self
    }

    /// A forced line break such as `<br>`.
    pub fn push_forced_break(&mut self, node: NodeId) -> &mut Self {
        if self.reserve_item() {
            let style = self.current_style();
            self.items.push(RawItem::ForcedBreak { node, style });
        }
        self
    }

    pub(crate) fn current_style(&self) -> u32 {
        self.stack.last().copied().unwrap_or(0)
    }

    fn check(&mut self, limit: Option<u64>, kind: LimitKind, actual: u64) -> bool {
        if self.error.is_some() {
            return false;
        }
        match Limits::check(limit, kind, actual) {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    fn reserve_item(&mut self) -> bool {
        let count = self.items.len() as u64 + 1;
        self.check(self.limits.max_items, LimitKind::Items, count)
    }

    fn intern(&mut self, style: &InlineStyle) -> Option<u32> {
        let key = format!("{style:?}");
        if let Some(&index) = self.style_index.get(&key) {
            return Some(index);
        }
        let count = self.styles.len() as u64 + 1;
        if !self.check(self.limits.max_styles, LimitKind::Styles, count) {
            return None;
        }
        let index = self.styles.len() as u32;
        self.styles.push(style.clone());
        self.style_index.insert(key, index);
        Some(index)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib builder`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/builder.rs src/lib.rs
git commit -m "Add ParagraphBuilder with incremental limit checks

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 8: White-space collapsing and `OffsetMapping`

**Files:**
- Create: `src/mapping.rs`
- Create: `src/analysis/mod.rs`
- Create: `src/analysis/whitespace.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `RawItem` (Task 7); `InlineStyle`, `WhiteSpaceCollapse`, `UnicodeBidi` (Task 4); `NodeId`, `TextSource`, `InlineEdges`, `OutOfFlowKind` (Task 4); `Direction` (Task 2).
- Produces:
  - `mapping.rs` (public, `shodo::mapping`): `enum MappingKind { Identity, Collapsed, Expanded }`, `struct MappingUnit { pub kind, pub node: NodeId, pub dom: Range<u32>, pub text: Range<u32> }`, `enum Affinity { Upstream, Downstream }`, `enum TextOrigin { Dom { node, offset: u32 }, Generated { node } }`, `struct OffsetMapping` with `units() -> &[MappingUnit]`, `dom_to_text(NodeId, u32) -> Option<(u32, Affinity)>`, `text_to_dom(u32, Affinity) -> Option<TextOrigin>`, and crate-visible `push_unit(MappingUnit)`, `push_generated(Range<u32>, NodeId)`.
  - `analysis/mod.rs`: `pub(crate) struct Item { kind: ItemKind, text: Range<u32>, style: u32, node: Option<NodeId> }`; `pub(crate) enum ItemKind { Text, OpenInline { edges: InlineEdges }, CloseInline, Atomic { edges: InlineEdges, parent_style: u32 }, OutOfFlow { kind: OutOfFlowKind }, BlockInInline, ForcedBreak, Tab, BidiControl }`; re-export `process`, `Processed`.
  - `analysis/whitespace.rs`: `pub(crate) struct Processed { text: String, items: Vec<Item>, mapping: Option<OffsetMapping> }`, `pub(crate) fn process(raw_text: &str, raw: &[RawItem], styles: &[InlineStyle], with_mapping: bool) -> Processed`.
  - Processed text conventions used by later tasks: atomics and out-of-flow anchors are U+FFFC; `BlockInInline` is U+2029 and a forced break is `\n` (both bidi paragraph separators, UAX #9 class B); bidi controls are the UAX #9 characters.

- [ ] **Step 1: Write the failing tests**

Create `src/mapping.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn unit(kind: MappingKind, dom: Range<u32>, text: Range<u32>) -> MappingUnit {
        MappingUnit { kind, node: NodeId(1), dom, text }
    }

    #[test]
    fn identity_units_merge_and_map_both_ways() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..4, 2..4));
        assert_eq!(m.units().len(), 1);
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Downstream)));
        assert_eq!(m.text_to_dom(3, Affinity::Downstream), Some(TextOrigin::Dom { node: NodeId(1), offset: 3 }));
    }

    #[test]
    fn collapsed_offsets_map_to_the_end_of_the_gap() {
        let mut m = OffsetMapping::default();
        m.push_unit(unit(MappingKind::Identity, 0..2, 0..2)); // "a "
        m.push_unit(unit(MappingKind::Collapsed, 2..4, 2..2)); // "  " removed
        m.push_unit(unit(MappingKind::Identity, 4..5, 2..3)); // "b"
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((2, Affinity::Downstream)));
        assert_eq!(m.text_to_dom(2, Affinity::Downstream), Some(TextOrigin::Dom { node: NodeId(1), offset: 4 }));
        assert_eq!(m.text_to_dom(2, Affinity::Upstream), Some(TextOrigin::Dom { node: NodeId(1), offset: 2 }));
    }

    #[test]
    fn expanded_interiors_round_to_the_start() {
        let mut m = OffsetMapping::default();
        // "ß" (2 bytes) became "SS" (2 bytes) in one unit, then "x".
        m.push_unit(unit(MappingKind::Expanded, 0..2, 0..2));
        m.push_unit(unit(MappingKind::Identity, 2..3, 2..3));
        assert_eq!(m.dom_to_text(NodeId(1), 1), Some((0, Affinity::Downstream)));
        assert_eq!(m.text_to_dom(1, Affinity::Downstream), Some(TextOrigin::Dom { node: NodeId(1), offset: 0 }));
        assert_eq!(m.dom_to_text(NodeId(1), 3), Some((3, Affinity::Upstream)));
    }

    #[test]
    fn generated_text_has_no_dom_offset() {
        let mut m = OffsetMapping::default();
        m.push_generated(0..3, NodeId(9));
        assert_eq!(m.text_to_dom(1, Affinity::Downstream), Some(TextOrigin::Generated { node: NodeId(9) }));
        assert_eq!(m.dom_to_text(NodeId(9), 0), None);
    }
}
```

Create `src/analysis/whitespace.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::ParagraphBuilder;
    use crate::limits::Limits;
    use crate::mapping::{Affinity, TextOrigin};
    use crate::node::{InlineEdges, NodeId, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle, UnicodeBidi, WhiteSpaceCollapse};

    fn run(b: &ParagraphBuilder) -> Processed {
        process(&b.text, &b.items, &b.styles, true)
    }

    fn dom(node: u64) -> TextSource {
        TextSource::Dom { node: NodeId(node), offset: 0 }
    }

    fn builder() -> ParagraphBuilder {
        ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
    }

    #[test]
    fn collapses_across_element_boundaries_and_trims_the_start() {
        let mut b = builder();
        b.push_text(dom(1), "  a  ")
            .open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b")
            .close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a b");
        let m = p.mapping.unwrap();
        // The space in node 3 was collapsed into the one kept in node 1.
        assert_eq!(m.dom_to_text(NodeId(3), 0), Some((2, Affinity::Downstream)));
        assert_eq!(m.dom_to_text(NodeId(3), 1), Some((2, Affinity::Downstream)));
        assert_eq!(m.text_to_dom(2, Affinity::Downstream), Some(TextOrigin::Dom { node: NodeId(3), offset: 1 }));
        // Round trip for every kept DOM offset.
        for (node, offset) in [(1, 2), (1, 3), (3, 1)] {
            let (t, a) = m.dom_to_text(NodeId(node), offset).unwrap();
            assert_eq!(m.text_to_dom(t, a), Some(TextOrigin::Dom { node: NodeId(node), offset }));
        }
    }

    #[test]
    fn preserved_tabs_and_newlines_become_control_items() {
        let pre = InlineStyle { white_space_collapse: WhiteSpaceCollapse::Preserve, ..InlineStyle::default() };
        let mut b = builder();
        b.open_inline(NodeId(1), &pre, InlineEdges::default()).push_text(dom(2), "a\tb\nc").close_inline();
        let p = run(&b);
        assert_eq!(p.text, "a\tb\nc");
        let kinds: Vec<_> = p.items.iter().map(|i| std::mem::discriminant(&i.kind)).collect();
        use ItemKind::*;
        let expected: Vec<_> = [
            OpenInline { edges: InlineEdges::default() },
            Text,
            Tab,
            Text,
            ForcedBreak,
            Text,
            CloseInline,
        ]
        .iter()
        .map(std::mem::discriminant)
        .collect();
        assert_eq!(kinds, expected);
    }

    #[test]
    fn atomics_stop_collapsing_and_are_object_replacement_characters() {
        let mut b = builder();
        b.push_text(dom(1), "a ")
            .push_atomic(NodeId(2), &InlineStyle::default(), InlineEdges::default())
            .push_text(dom(3), " b");
        let p = run(&b);
        assert_eq!(p.text, "a \u{FFFC} b");
        let m = p.mapping.unwrap();
        assert_eq!(m.text_to_dom(2, Affinity::Downstream), Some(TextOrigin::Generated { node: NodeId(2) }));
    }

    #[test]
    fn isolates_insert_bidi_controls() {
        let iso = InlineStyle { unicode_bidi: UnicodeBidi::Isolate, ..InlineStyle::default() };
        let mut b = builder();
        b.open_inline(NodeId(1), &iso, InlineEdges::default()).push_text(dom(2), "x").close_inline();
        assert_eq!(run(&b).text, "\u{2066}x\u{2069}");
    }

    #[test]
    fn mapping_can_be_disabled() {
        let mut b = builder();
        b.push_text(dom(1), "a");
        assert!(process(&b.text, &b.items, &b.styles, false).mapping.is_none());
    }
}
```

Create `src/analysis/mod.rs`:

```rust
//! Text analysis: white-space processing and the data line breaking uses.

mod whitespace;

use std::ops::Range;

use crate::node::{InlineEdges, NodeId, OutOfFlowKind};

pub(crate) use whitespace::{Processed, process};

/// An item of the processed paragraph. `text` indexes the processed text.
#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    pub(crate) text: Range<u32>,
    pub(crate) style: u32,
    pub(crate) node: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub(crate) enum ItemKind {
    Text,
    OpenInline { edges: InlineEdges },
    CloseInline,
    Atomic { edges: InlineEdges, parent_style: u32 },
    OutOfFlow { kind: OutOfFlowKind },
    BlockInInline,
    ForcedBreak,
    Tab,
    BidiControl,
}
```

Add to `src/lib.rs`: `pub mod mapping;` with the other `pub mod` lines, and `mod analysis;` next to `mod builder;`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib mapping analysis`
Expected: FAIL to compile ("cannot find type `OffsetMapping`", "cannot find function `process`").

- [ ] **Step 3: Implement `OffsetMapping`**

Insert above the tests in `src/mapping.rs`:

```rust
//! Mapping between caller text offsets (UTF-8 bytes within a node's text)
//! and offsets in the processed text shodo lays out. White-space collapsing
//! removes characters and text-transform can change lengths, so the mapping
//! is a list of runs.

use std::ops::Range;

use crate::node::NodeId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MappingKind {
    /// Same length on both sides.
    Identity,
    /// Removed by white-space collapsing; `text` is empty.
    Collapsed,
    /// Length changed (for example `ß` to `SS`); indivisible.
    Expanded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingUnit {
    pub kind: MappingKind,
    pub node: NodeId,
    pub dom: Range<u32>,
    pub text: Range<u32>,
}

/// Which side of a boundary a position belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Affinity {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOrigin {
    Dom { node: NodeId, offset: u32 },
    /// Generated content, atomics and control characters have no DOM offset.
    Generated { node: NodeId },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OffsetMapping {
    units: Vec<MappingUnit>,
    generated: Vec<(Range<u32>, NodeId)>,
}

impl OffsetMapping {
    pub(crate) fn push_unit(&mut self, unit: MappingUnit) {
        if let Some(last) = self.units.last_mut()
            && last.kind == unit.kind
            && unit.kind != MappingKind::Expanded
            && last.node == unit.node
            && last.dom.end == unit.dom.start
            && last.text.end == unit.text.start
        {
            last.dom.end = unit.dom.end;
            last.text.end = unit.text.end;
            return;
        }
        self.units.push(unit);
    }

    pub(crate) fn push_generated(&mut self, text: Range<u32>, node: NodeId) {
        if let Some((last, last_node)) = self.generated.last_mut()
            && *last_node == node
            && last.end == text.start
        {
            last.end = text.end;
            return;
        }
        self.generated.push((text, node));
    }

    pub fn units(&self) -> &[MappingUnit] {
        &self.units
    }

    /// Processed-text offset of a caller offset. Offsets inside a collapsed
    /// run map to the end of the gap; offsets inside an expanded run round to
    /// its start.
    pub fn dom_to_text(&self, node: NodeId, offset: u32) -> Option<(u32, Affinity)> {
        let mut at_end = None;
        for u in self.units.iter().filter(|u| u.node == node) {
            if u.dom.start <= offset && offset < u.dom.end {
                let text = match u.kind {
                    MappingKind::Identity => u.text.start + (offset - u.dom.start),
                    MappingKind::Collapsed => u.text.end,
                    MappingKind::Expanded => u.text.start,
                };
                return Some((text, Affinity::Downstream));
            }
            if offset == u.dom.end {
                at_end = Some((u.text.end, Affinity::Upstream));
            }
        }
        at_end
    }

    /// Caller offset of a processed-text offset.
    pub fn text_to_dom(&self, offset: u32, affinity: Affinity) -> Option<TextOrigin> {
        if let Some((_, node)) = self.generated.iter().find(|(r, _)| r.start <= offset && offset < r.end) {
            return Some(TextOrigin::Generated { node: *node });
        }
        let mut downstream = None;
        let mut upstream = None;
        for u in self.units.iter().filter(|u| u.kind != MappingKind::Collapsed) {
            if downstream.is_none() && u.text.start <= offset && offset < u.text.end {
                let dom = match u.kind {
                    MappingKind::Identity => u.dom.start + (offset - u.text.start),
                    _ => u.dom.start,
                };
                downstream = Some(TextOrigin::Dom { node: u.node, offset: dom });
            }
            if u.text.end == offset {
                upstream = Some(TextOrigin::Dom { node: u.node, offset: u.dom.end });
            }
        }
        match affinity {
            Affinity::Upstream => upstream.or(downstream),
            Affinity::Downstream => downstream.or(upstream),
        }
    }
}
```

- [ ] **Step 4: Implement white-space processing**

Insert above the tests in `src/analysis/whitespace.rs`:

```rust
//! White-space processing (CSS Text 3 §4.1.1 phase I) and bidi control
//! insertion (CSS Writing Modes 4 §2.4), producing the processed text,
//! the item list and the offset mapping.
//!
//! Collapsing here covers spaces, tabs and segment breaks as plain spaces;
//! the language-dependent segment break transformation (§4.1.3) is not
//! applied yet.

use super::{Item, ItemKind};
use crate::builder::RawItem;
use crate::geometry::Direction;
use crate::mapping::{MappingKind, MappingUnit, OffsetMapping};
use crate::node::{NodeId, TextSource};
use crate::style::{InlineStyle, UnicodeBidi, WhiteSpaceCollapse};

const OBJECT_REPLACEMENT: char = '\u{FFFC}';
const PARAGRAPH_SEPARATOR: char = '\u{2029}';

pub(crate) struct Processed {
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) mapping: Option<OffsetMapping>,
}

pub(crate) fn process(raw_text: &str, raw: &[RawItem], styles: &[InlineStyle], with_mapping: bool) -> Processed {
    let mut p = Processor {
        out: String::with_capacity(raw_text.len()),
        items: Vec::with_capacity(raw.len()),
        mapping: with_mapping.then(OffsetMapping::default),
        // Collapsible spaces at the start of the paragraph are removed.
        after_space: true,
        open: Vec::new(),
    };
    for item in raw {
        match item {
            RawItem::Text { source, range, style } => {
                let text = &raw_text[range.start as usize..range.end as usize];
                p.text(text, *source, *style, &styles[*style as usize]);
            }
            RawItem::Open { node, style, edges } => {
                p.marker(ItemKind::OpenInline { edges: *edges }, *style, *node);
                for &c in bidi_open(&styles[*style as usize]) {
                    p.generated(ItemKind::BidiControl, c, *style, *node);
                }
                p.open.push((*node, *style));
            }
            RawItem::Close => {
                if let Some((node, style)) = p.open.pop() {
                    for &c in bidi_close(&styles[style as usize]) {
                        p.generated(ItemKind::BidiControl, c, style, node);
                    }
                    p.marker(ItemKind::CloseInline, style, node);
                }
            }
            RawItem::Atomic { node, style, parent_style, edges } => {
                let kind = ItemKind::Atomic { edges: *edges, parent_style: *parent_style };
                p.generated(kind, OBJECT_REPLACEMENT, *style, *node);
                p.after_space = false;
            }
            RawItem::OutOfFlow { node, kind, style } => {
                // Out-of-flow boxes are transparent to white-space collapsing.
                p.generated(ItemKind::OutOfFlow { kind: *kind }, OBJECT_REPLACEMENT, *style, *node);
            }
            RawItem::BlockInInline { node, style } => {
                p.generated(ItemKind::BlockInInline, PARAGRAPH_SEPARATOR, *style, *node);
                p.after_space = true;
            }
            RawItem::ForcedBreak { node, style } => {
                p.generated(ItemKind::ForcedBreak, '\n', *style, *node);
                p.after_space = true;
            }
        }
    }
    Processed { text: p.out, items: p.items, mapping: p.mapping }
}

struct Processor {
    out: String,
    items: Vec<Item>,
    mapping: Option<OffsetMapping>,
    after_space: bool,
    open: Vec<(NodeId, u32)>,
}

impl Processor {
    fn pos(&self) -> u32 {
        self.out.len() as u32
    }

    fn marker(&mut self, kind: ItemKind, style: u32, node: NodeId) {
        let at = self.pos();
        self.items.push(Item { kind, text: at..at, style, node: Some(node) });
    }

    fn generated(&mut self, kind: ItemKind, c: char, style: u32, node: NodeId) {
        let start = self.pos();
        self.out.push(c);
        let text = start..self.pos();
        if let Some(m) = &mut self.mapping {
            m.push_generated(text.clone(), node);
        }
        self.items.push(Item { kind, text, style, node: Some(node) });
    }

    fn map(&mut self, kind: MappingKind, node: NodeId, dom: Option<u32>, len: u32, text: std::ops::Range<u32>) {
        let Some(m) = &mut self.mapping else { return };
        match dom {
            Some(dom) => m.push_unit(MappingUnit { kind, node, dom: dom..dom + len, text }),
            None if !text.is_empty() => m.push_generated(text, node),
            None => {}
        }
    }

    fn text(&mut self, s: &str, source: TextSource, style_index: u32, style: &InlineStyle) {
        use WhiteSpaceCollapse::*;
        let collapse_spaces = matches!(style.white_space_collapse, Collapse | PreserveBreaks);
        let preserve_breaks = !matches!(style.white_space_collapse, Collapse);
        let node = source.node();
        let dom_base = match source {
            TextSource::Dom { offset, .. } => Some(offset),
            TextSource::Generated { .. } => None,
        };
        let mut segment: Option<u32> = None;
        for (i, c) in s.char_indices() {
            let dom = dom_base.map(|b| b + i as u32);
            let len = c.len_utf8() as u32;
            let control = match c {
                '\n' if preserve_breaks => Some(ItemKind::ForcedBreak),
                '\t' if !collapse_spaces => Some(ItemKind::Tab),
                _ => None,
            };
            if let Some(kind) = control {
                self.flush(&mut segment, style_index, node);
                let start = self.pos();
                self.out.push(c);
                self.map(MappingKind::Identity, node, dom, len, start..self.pos());
                self.items.push(Item { kind: kind.clone(), text: start..self.pos(), style: style_index, node: Some(node) });
                self.after_space = matches!(kind, ItemKind::ForcedBreak);
                continue;
            }
            let collapsible = collapse_spaces && matches!(c, ' ' | '\t' | '\n');
            if collapsible && self.after_space {
                let at = self.pos();
                self.map(MappingKind::Collapsed, node, dom, len, at..at);
                continue;
            }
            segment.get_or_insert(self.pos());
            let start = self.pos();
            // A collapsible tab or segment break is kept as one space (same length).
            self.out.push(if collapsible { ' ' } else { c });
            self.map(MappingKind::Identity, node, dom, len, start..self.pos());
            self.after_space = collapsible;
        }
        self.flush(&mut segment, style_index, node);
    }

    fn flush(&mut self, segment: &mut Option<u32>, style: u32, node: NodeId) {
        if let Some(start) = segment.take() {
            let end = self.pos();
            if end > start {
                self.items.push(Item { kind: ItemKind::Text, text: start..end, style, node: Some(node) });
            }
        }
    }
}

fn bidi_open(style: &InlineStyle) -> &'static [char] {
    let rtl = style.direction == Direction::Rtl;
    match (style.unicode_bidi, rtl) {
        (UnicodeBidi::Normal, _) => &[],
        (UnicodeBidi::Embed, false) => &['\u{202A}'],
        (UnicodeBidi::Embed, true) => &['\u{202B}'],
        (UnicodeBidi::Isolate, false) => &['\u{2066}'],
        (UnicodeBidi::Isolate, true) => &['\u{2067}'],
        (UnicodeBidi::BidiOverride, false) => &['\u{202D}'],
        (UnicodeBidi::BidiOverride, true) => &['\u{202E}'],
        (UnicodeBidi::IsolateOverride, false) => &['\u{2066}', '\u{202D}'],
        (UnicodeBidi::IsolateOverride, true) => &['\u{2067}', '\u{202E}'],
        (UnicodeBidi::Plaintext, _) => &['\u{2068}'],
    }
}

fn bidi_close(style: &InlineStyle) -> &'static [char] {
    match style.unicode_bidi {
        UnicodeBidi::Normal => &[],
        UnicodeBidi::Embed | UnicodeBidi::BidiOverride => &['\u{202C}'],
        UnicodeBidi::Isolate | UnicodeBidi::Plaintext => &['\u{2069}'],
        UnicodeBidi::IsolateOverride => &['\u{202C}', '\u{2069}'],
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib mapping analysis`
Expected: PASS (4 mapping tests + 5 whitespace tests). If `collapses_across_element_boundaries_and_trims_the_start` fails on the upstream/downstream expectation, re-check that `dom_to_text` returns `Downstream` for offsets strictly inside a run and `Upstream` only for the end offset.

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/mapping.rs src/analysis src/lib.rs
git commit -m "Collapse white space across elements and build OffsetMapping

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 9: Stub shaper with structure-of-arrays glyph storage

**Files:**
- Create: `src/shape.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `LayoutUnit`, `Saturation` (Task 1); `Limits`, `LimitExceeded`, `LimitKind` (Task 3); `FontId` (Task 6).
- Produces:
  - `pub(crate) const RUN_PEN_LIMIT: i32 = 1 << 30`.
  - `pub(crate) struct GlyphStore { id: Vec<u32>, advance: Vec<LayoutUnit>, pen: Vec<LayoutUnit>, offset_inline: Vec<LayoutUnit>, offset_block: Vec<LayoutUnit>, cluster: Vec<u32> }` with `len() -> usize`. `pen[g]` is the pen position before glyph `g`, relative to the start of its run. `cluster[g]` is the byte offset of the glyph's character in the processed text.
  - `pub(crate) struct ShapedRun { glyphs: Range<u32>, text: Range<u32>, item: u32, font: FontId, font_size: f32 }`.
  - `pub(crate) fn shape_item(store: &mut GlyphStore, runs: &mut Vec<ShapedRun>, text: &str, text_start: u32, item: u32, font: FontId, font_size: f32, limits: &Limits, sat: &mut Saturation) -> Result<(), LimitExceeded>` — shapes one text item; runs never mix items.
  - `pub(crate) fn is_mark(c: char) -> bool` (U+0300–U+036F).

- [ ] **Step 1: Write the failing tests**

Create `src/shape.rs` with only the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontCollection;

    fn shape(text: &str, size: f32, limits: &Limits) -> Result<(GlyphStore, Vec<ShapedRun>), LimitExceeded> {
        let font = FontCollection::new(&Limits::default()).primary_font();
        let mut store = GlyphStore::default();
        let mut runs = Vec::new();
        let mut sat = Saturation::default();
        shape_item(&mut store, &mut runs, text, 0, 0, font, size, limits, &mut sat)?;
        Ok((store, runs))
    }

    #[test]
    fn one_em_per_character() {
        let (g, runs) = shape("abc", 10.0, &Limits::default()).unwrap();
        assert_eq!(g.len(), 3);
        let px: Vec<f32> = g.pen.iter().map(|p| p.to_f32()).collect();
        assert_eq!(px, vec![0.0, 10.0, 20.0]);
        assert!(g.advance.iter().all(|a| a.to_f32() == 10.0));
        assert_eq!(g.cluster, vec![0, 1, 2]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, 0..3);
    }

    #[test]
    fn combining_marks_have_zero_advance_and_an_offset() {
        let (g, _) = shape("e\u{301}x", 10.0, &Limits::default()).unwrap();
        assert_eq!(g.advance[1], LayoutUnit::ZERO);
        assert_eq!(g.offset_inline[1].to_f32(), -5.0);
        assert_eq!(g.cluster[1], 1);
        assert_eq!(g.pen[2].to_f32(), 10.0);
    }

    #[test]
    fn pen_positions_restart_before_saturating() {
        // 1e6 px per glyph: 16 glyphs fit under 2^30 units (about 1.68e7 px).
        let text = "a".repeat(40);
        let (g, runs) = shape(&text, 1.0e6, &Limits::default()).unwrap();
        let sizes: Vec<u32> = runs.iter().map(|r| r.glyphs.end - r.glyphs.start).collect();
        assert_eq!(sizes, vec![16, 16, 8]);
        assert_eq!(g.pen[16], LayoutUnit::ZERO);
        // Differences inside a run stay exact.
        assert_eq!((g.pen[15] - g.pen[14]).to_f32(), 1.0e6);
        assert_eq!(runs[1].text, 16..32);
    }

    #[test]
    fn glyph_count_limit_is_checked_before_pushing() {
        let limits = Limits { max_shaped_glyphs: Some(2), ..Limits::default() };
        let err = shape("abc", 10.0, &limits).unwrap_err();
        assert_eq!(err.kind, LimitKind::ShapedGlyphs);
    }
}
```

Add `mod shape;` to `src/lib.rs` next to the other private modules.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib shape`
Expected: FAIL to compile ("cannot find type `GlyphStore`").

- [ ] **Step 3: Implement**

Insert above the tests in `src/shape.rs`:

```rust
//! Shaping into structure-of-arrays glyph storage.
//!
//! This is a placeholder shaper: one glyph per character with a 1em advance
//! (like the Ahem test font). Combining marks U+0300–U+036F get a zero
//! advance and a −0.5em inline offset, so that glyph offsets are exercised
//! separately from pen positions.

use std::ops::Range;

use crate::font::FontId;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::{LimitExceeded, LimitKind, Limits};

/// A run is closed before its pen position would exceed this value, so
/// differences between pen positions within a run never saturate.
pub(crate) const RUN_PEN_LIMIT: i32 = 1 << 30;

#[derive(Clone, Debug, Default)]
pub(crate) struct GlyphStore {
    pub(crate) id: Vec<u32>,
    pub(crate) advance: Vec<LayoutUnit>,
    /// Pen position before the glyph, relative to the start of its run.
    pub(crate) pen: Vec<LayoutUnit>,
    pub(crate) offset_inline: Vec<LayoutUnit>,
    pub(crate) offset_block: Vec<LayoutUnit>,
    /// Byte offset of the glyph's character in the processed text.
    pub(crate) cluster: Vec<u32>,
}

impl GlyphStore {
    pub(crate) fn len(&self) -> usize {
        self.id.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedRun {
    pub(crate) glyphs: Range<u32>,
    pub(crate) text: Range<u32>,
    pub(crate) item: u32,
    pub(crate) font: FontId,
    pub(crate) font_size: f32,
}

pub(crate) fn is_mark(c: char) -> bool {
    ('\u{300}'..='\u{36F}').contains(&c)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn shape_item(
    store: &mut GlyphStore,
    runs: &mut Vec<ShapedRun>,
    text: &str,
    text_start: u32,
    item: u32,
    font: FontId,
    font_size: f32,
    limits: &Limits,
    sat: &mut Saturation,
) -> Result<(), LimitExceeded> {
    let em = LayoutUnit::from_f32_round(font_size, sat);
    let half_em = em.div_i32(2);
    let mut run_glyphs = store.len() as u32;
    let mut run_text = text_start;
    let mut pen = LayoutUnit::ZERO;
    for (i, c) in text.char_indices() {
        Limits::check(limits.max_shaped_glyphs, LimitKind::ShapedGlyphs, store.len() as u64 + 1)?;
        let mark = is_mark(c);
        let advance = if mark { LayoutUnit::ZERO } else { em };
        let cluster = text_start + i as u32;
        let overflows = i64::from(pen.raw()) + i64::from(advance.raw()) > i64::from(RUN_PEN_LIMIT);
        if overflows && store.len() as u32 > run_glyphs {
            runs.push(ShapedRun { glyphs: run_glyphs..store.len() as u32, text: run_text..cluster, item, font, font_size });
            run_glyphs = store.len() as u32;
            run_text = cluster;
            pen = LayoutUnit::ZERO;
        }
        store.id.push(c as u32);
        store.advance.push(advance);
        store.pen.push(pen);
        store.offset_inline.push(if mark { LayoutUnit::ZERO - half_em } else { LayoutUnit::ZERO });
        store.offset_block.push(LayoutUnit::ZERO);
        store.cluster.push(cluster);
        pen = pen.add(advance, sat);
    }
    if store.len() as u32 > run_glyphs {
        let end = text_start + text.len() as u32;
        runs.push(ShapedRun { glyphs: run_glyphs..store.len() as u32, text: run_text..end, item, font, font_size });
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib shape`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src/shape.rs src/lib.rs
git commit -m "Add placeholder shaper with run-local pen positions

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 10: Units, bidi levels, and `build()` producing a `Paragraph`

**Files:**
- Create: `src/analysis/units.rs`
- Modify: `src/analysis/mod.rs`
- Create: `src/context.rs`
- Create: `src/paragraph.rs`
- Modify: `src/builder.rs`
- Modify: `src/lib.rs`
- Test: `tests/build.rs`

**Interfaces:**
- Consumes: `process`, `Item`, `ItemKind` (Task 8); `shape_item`, `GlyphStore`, `ShapedRun`, `is_mark` (Task 9); `FontCollection` (Task 6); `ParagraphBuilder` internals (Task 7); `WarningSink`, `Warning` (Task 3).
- Produces:
  - `analysis/units.rs`: `pub(crate) enum BreakClass { Prohibited, Allowed, Mandatory }`; `pub(crate) enum UnitKind { Cluster { run: u32, glyphs: Range<u32>, space: bool }, Open { box_index: u32 }, Close { box_index: u32 }, Atomic { node: NodeId }, Float { node: NodeId, ordinal: u32 }, Absolute { node: NodeId }, BlockInInline { node: NodeId }, ForcedBreak, Tab, BidiControl }`; `pub(crate) struct Unit { kind, item: u32, text: Range<u32>, break_after: BreakClass, level: u8, parent_box: Option<u32> }`; `pub(crate) struct InlineBoxInfo { node: NodeId, style: u32, edges: InlineEdges, parent: Option<u32> }`; `pub(crate) struct UnitList { units, boxes, float_count: u32 }`; `pub(crate) fn bidi_levels(text: &str, direction: Direction, plaintext: bool) -> Vec<u8>`; `pub(crate) fn build_units(text, items, runs, glyphs, levels, base_level: u8) -> UnitList`.
  - `context.rs`: `pub struct LayoutContext` (`Default`, `Debug`) with `new()`, `take_warnings() -> Vec<Warning>`, `shrink_to(usize)`; crate field `warnings: WarningSink`.
  - `paragraph.rs`: `pub struct BreakToken` (Copy, Eq, Hash; fields `para: u64`, `unit: u32`, `flags: u8` crate-visible; consts `FIRST_LINE = 1`, `AFTER_FORCED = 2`); `pub struct FloatCursor(pub(crate) u32)` with `before() -> Option<FloatCursor>`; `pub(crate) struct ParagraphData` (fields listed in Step 4); `pub struct Paragraph` (Clone, Debug, Send + Sync) with `id()`, `start_token()`, `font_generations() -> (u64, Option<u64>)`, `warnings() -> &[Warning]`, `offset_mapping() -> Option<&OffsetMapping>`, `text() -> &str`, `required_baseline(NodeId) -> Option<BaselineKind>`; `pub(crate) fn Paragraph::from_builder(ParagraphBuilder, &FontCollection) -> Result<Paragraph, LimitExceeded>`.
  - `builder.rs`: `ParagraphBuilder::build(self, &mut LayoutContext, &FontCollection) -> Result<Paragraph, LimitExceeded>`; `pub struct RichText` with `new(&ParagraphStyle)`, `with_limits(&ParagraphStyle, &Limits)`, `push(self, &str, &InlineStyle) -> Self`, `build(self, &mut LayoutContext, &FontCollection) -> Result<Paragraph, LimitExceeded>`. The n-th pushed span has `NodeId(n)` (starting at 0) and its text is `TextSource::Dom { node, offset: 0 }`.

- [ ] **Step 1: Write the failing integration tests**

Create `tests/build.rs`:

```rust
use shodo::font::FontCollection;
use shodo::geometry::{BaselineKind, WritingMode};
use shodo::limits::{LimitKind, Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, ParagraphStyle, TextOrientation};
use shodo::{LayoutContext, Paragraph, ParagraphBuilder, RichText};

fn fonts() -> FontCollection {
    FontCollection::new(&Limits::default())
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom { node: NodeId(node), offset: 0 }
}

#[test]
fn builds_and_keeps_identity_across_clones() {
    let mut cx = LayoutContext::new();
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(dom(1), "hello  world");
    let para = b.build(&mut cx, &fonts()).unwrap();
    assert_eq!(para.text(), "hello world");
    let clone = para.clone();
    assert_eq!(clone.id(), para.id());
    assert_eq!(clone.start_token(), para.start_token());

    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.push_text(dom(1), "hello  world");
    let rebuilt = b.build(&mut cx, &fonts()).unwrap();
    assert_ne!(rebuilt.id(), para.id(), "identical content still gets a new id");
    assert_ne!(rebuilt.start_token(), para.start_token());
}

#[test]
fn empty_and_all_space_paragraphs_build() {
    let mut cx = LayoutContext::new();
    for text in ["", "   "] {
        let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
        b.push_text(dom(1), text);
        assert_eq!(b.build(&mut cx, &fonts()).unwrap().text(), "");
    }
}

#[test]
fn builder_errors_surface_from_build() {
    let limits = Limits { max_text_bytes: Some(2), ..Limits::default() };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(dom(1), "abc");
    let err = b.build(&mut LayoutContext::new(), &fonts()).unwrap_err();
    assert_eq!(err.kind, LimitKind::TextBytes);
}

#[test]
fn shaped_glyph_limit_is_enforced_at_build() {
    let limits = Limits { max_shaped_glyphs: Some(2), ..Limits::default() };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &limits);
    b.push_text(dom(1), "abc");
    let err = b.build(&mut LayoutContext::new(), &fonts()).unwrap_err();
    assert_eq!(err.kind, LimitKind::ShapedGlyphs);
}

#[test]
fn unclosed_inline_boxes_are_closed_with_a_warning() {
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(NodeId(1), &InlineStyle::default(), InlineEdges::default()).push_text(dom(2), "x");
    let para = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(para.warnings().iter().any(|w| w.kind == WarningKind::UnbalancedInline));
}

#[test]
fn first_line_style_is_reported_as_unsupported() {
    let style = ParagraphStyle { first_line: Some(InlineStyle::default()), ..ParagraphStyle::default() };
    let para = ParagraphBuilder::new(&style, &Limits::default()).build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(para.warnings().iter().any(|w| w.kind == WarningKind::Unsupported));
}

#[test]
fn negative_and_non_finite_font_sizes_are_neutralized() {
    let bad = InlineStyle { font_size: f32::NAN, ..InlineStyle::default() };
    let mut b = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default());
    b.open_inline(NodeId(1), &bad, InlineEdges::default()).push_text(dom(2), "x").close_inline();
    let para = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    assert!(para.warnings().iter().any(|w| w.kind == WarningKind::NonFiniteInput));
}

#[test]
fn required_baseline_follows_the_parent_inline_box() {
    let build = |wm: WritingMode, orientation: TextOrientation| -> Paragraph {
        let style = ParagraphStyle { writing_mode: wm, ..ParagraphStyle::default() };
        let span = InlineStyle { text_orientation: orientation, ..InlineStyle::default() };
        let mut b = ParagraphBuilder::new(&style, &Limits::default());
        b.push_atomic(NodeId(1), &InlineStyle::default(), InlineEdges::default())
            .open_inline(NodeId(2), &span, InlineEdges::default())
            .push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default())
            .close_inline();
        b.build(&mut LayoutContext::new(), &fonts()).unwrap()
    };
    let horizontal = build(WritingMode::HorizontalTb, TextOrientation::Mixed);
    assert_eq!(horizontal.required_baseline(NodeId(1)), Some(BaselineKind::Alphabetic));

    let vertical = build(WritingMode::VerticalRl, TextOrientation::Sideways);
    assert_eq!(vertical.required_baseline(NodeId(1)), Some(BaselineKind::Central));
    assert_eq!(vertical.required_baseline(NodeId(3)), Some(BaselineKind::Alphabetic));
    assert_eq!(vertical.required_baseline(NodeId(99)), None);
}

#[test]
fn font_generations_record_both_layers() {
    let shared = fonts();
    let doc = FontCollection::for_document(&shared, &Limits::default());
    let para = ParagraphBuilder::new(&ParagraphStyle::default(), &Limits::default())
        .build(&mut LayoutContext::new(), &doc)
        .unwrap();
    assert_eq!(para.font_generations(), (0, Some(0)));
}

#[test]
fn rich_text_assigns_sequential_nodes() {
    let para = RichText::new(&ParagraphStyle::default())
        .push("Hello ", &InlineStyle::default())
        .push("世界", &InlineStyle { font_weight: 700.0, ..InlineStyle::default() })
        .build(&mut LayoutContext::new(), &fonts())
        .unwrap();
    assert_eq!(para.text(), "Hello 世界");
    let m = para.offset_mapping().unwrap();
    assert_eq!(m.dom_to_text(NodeId(1), 3).map(|(t, _)| t), Some(9));
}

#[test]
fn paragraph_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Paragraph>();
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test build`
Expected: FAIL to compile ("unresolved imports `shodo::LayoutContext`, `shodo::Paragraph`, `shodo::RichText`").

- [ ] **Step 3: Implement units and bidi levels**

Create `src/analysis/units.rs`:

```rust
//! Units: the sequence line breaking walks. There is one unit per cluster
//! (a base character with its combining marks) and one per non-text item.

use std::ops::Range;

use unicode_bidi::{BidiClass, BidiInfo, Level, bidi_class};

use super::{Item, ItemKind};
use crate::geometry::Direction;
use crate::node::{InlineEdges, NodeId, OutOfFlowKind};
use crate::shape::{GlyphStore, ShapedRun, is_mark};

/// Line break opportunity after a unit. Only spaces, tabs and atomic inlines
/// provide soft opportunities for now; UAX #14 comes later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BreakClass {
    Prohibited,
    Allowed,
    Mandatory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UnitKind {
    Cluster { run: u32, glyphs: Range<u32>, space: bool },
    Open { box_index: u32 },
    Close { box_index: u32 },
    Atomic { node: NodeId },
    Float { node: NodeId, ordinal: u32 },
    Absolute { node: NodeId },
    BlockInInline { node: NodeId },
    ForcedBreak,
    Tab,
    BidiControl,
}

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub(crate) kind: UnitKind,
    pub(crate) item: u32,
    pub(crate) text: Range<u32>,
    pub(crate) break_after: BreakClass,
    pub(crate) level: u8,
    /// Innermost inline box containing the unit (for `Open`/`Close`, the
    /// box's parent).
    pub(crate) parent_box: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct InlineBoxInfo {
    pub(crate) node: NodeId,
    pub(crate) style: u32,
    pub(crate) edges: InlineEdges,
    pub(crate) parent: Option<u32>,
}

pub(crate) struct UnitList {
    pub(crate) units: Vec<Unit>,
    pub(crate) boxes: Vec<InlineBoxInfo>,
    pub(crate) float_count: u32,
}

/// Bidi embedding level of every byte of `text` (UAX #9). Left-to-right
/// paragraphs without right-to-left characters or bidi controls skip the
/// algorithm.
pub(crate) fn bidi_levels(text: &str, direction: Direction, plaintext: bool) -> Vec<u8> {
    use BidiClass::*;
    let needs_bidi = plaintext
        || direction == Direction::Rtl
        || text.chars().any(|c| matches!(bidi_class(c), R | AL | RLE | RLO | RLI | LRE | LRO | LRI | FSI | PDF | PDI));
    if !needs_bidi {
        return vec![0; text.len()];
    }
    let base = match (plaintext, direction) {
        (true, _) => None,
        (false, Direction::Rtl) => Some(Level::rtl()),
        (false, Direction::Ltr) => Some(Level::ltr()),
    };
    BidiInfo::new(text, base).levels.iter().map(|l| l.number()).collect()
}

pub(crate) fn build_units(
    text: &str,
    items: &[Item],
    runs: &[ShapedRun],
    glyphs: &GlyphStore,
    levels: &[u8],
    base_level: u8,
) -> UnitList {
    let mut units: Vec<Unit> = Vec::with_capacity(glyphs.len() + items.len());
    let mut boxes: Vec<InlineBoxInfo> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    let mut float_count = 0u32;
    let mut run_index = 0usize;
    for (index, item) in items.iter().enumerate() {
        let index = index as u32;
        let parent_box = stack.last().copied();
        let node = item.node.unwrap_or(NodeId(0));
        let mut push = |kind: UnitKind, break_after: BreakClass, parent_box: Option<u32>| {
            units.push(Unit { kind, item: index, text: item.text.clone(), break_after, level: base_level, parent_box });
        };
        match &item.kind {
            ItemKind::Text => {
                while run_index < runs.len() && runs[run_index].item == index {
                    let run = &runs[run_index];
                    for g in run.glyphs.clone() {
                        let cluster = glyphs.cluster[g as usize];
                        let c = text[cluster as usize..].chars().next().unwrap_or(' ');
                        let end = cluster + c.len_utf8() as u32;
                        if is_mark(c)
                            && let Some(last) = units.last_mut()
                            && let UnitKind::Cluster { run: r, glyphs: range, .. } = &mut last.kind
                            && *r == run_index as u32
                        {
                            range.end = g + 1;
                            last.text.end = end;
                            continue;
                        }
                        let space = c == ' ';
                        units.push(Unit {
                            kind: UnitKind::Cluster { run: run_index as u32, glyphs: g..g + 1, space },
                            item: index,
                            text: cluster..end,
                            break_after: if space { BreakClass::Allowed } else { BreakClass::Prohibited },
                            level: base_level,
                            parent_box,
                        });
                    }
                    run_index += 1;
                }
            }
            ItemKind::OpenInline { edges } => {
                let box_index = boxes.len() as u32;
                boxes.push(InlineBoxInfo { node, style: item.style, edges: *edges, parent: parent_box });
                push(UnitKind::Open { box_index }, BreakClass::Prohibited, parent_box);
                stack.push(box_index);
            }
            ItemKind::CloseInline => {
                if let Some(box_index) = stack.pop() {
                    push(UnitKind::Close { box_index }, BreakClass::Prohibited, stack.last().copied());
                }
            }
            ItemKind::Atomic { .. } => {
                // UAX #14 class CB: break opportunities before and after.
                if let Some(prev) = units.last_mut()
                    && prev.break_after == BreakClass::Prohibited
                {
                    prev.break_after = BreakClass::Allowed;
                }
                units.push(Unit {
                    kind: UnitKind::Atomic { node },
                    item: index,
                    text: item.text.clone(),
                    break_after: BreakClass::Allowed,
                    level: base_level,
                    parent_box,
                });
            }
            ItemKind::OutOfFlow { kind } => {
                let unit = match kind {
                    OutOfFlowKind::Float => {
                        float_count += 1;
                        UnitKind::Float { node, ordinal: float_count - 1 }
                    }
                    OutOfFlowKind::Absolute => UnitKind::Absolute { node },
                };
                push(unit, BreakClass::Prohibited, parent_box);
            }
            ItemKind::BlockInInline => push(UnitKind::BlockInInline { node }, BreakClass::Prohibited, parent_box),
            ItemKind::ForcedBreak => push(UnitKind::ForcedBreak, BreakClass::Mandatory, parent_box),
            ItemKind::Tab => push(UnitKind::Tab, BreakClass::Allowed, parent_box),
            ItemKind::BidiControl => push(UnitKind::BidiControl, BreakClass::Prohibited, parent_box),
        }
    }
    let level_at = |pos: u32| levels.get(pos as usize).copied();
    for unit in &mut units {
        let level = match unit.kind {
            // An opening box takes the level of what follows it, a closing
            // box the level of what precedes it.
            UnitKind::Open { .. } => level_at(unit.text.start),
            UnitKind::Close { .. } => unit.text.start.checked_sub(1).and_then(level_at),
            _ => level_at(unit.text.start),
        };
        unit.level = level.unwrap_or(base_level);
    }
    UnitList { units, boxes, float_count }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_to_right_text_skips_the_algorithm() {
        assert_eq!(bidi_levels("abc", Direction::Ltr, false), vec![0, 0, 0]);
    }

    #[test]
    fn hebrew_gets_odd_levels() {
        let levels = bidi_levels("a \u{5D0}", Direction::Ltr, false);
        assert_eq!(levels[0], 0);
        assert_eq!(*levels.last().unwrap(), 1);
        assert!(bidi_levels("abc", Direction::Rtl, false).iter().all(|&l| l == 2));
    }
}
```

In `src/analysis/mod.rs`, add `pub(crate) mod units;` below `mod whitespace;`.

- [ ] **Step 4: Implement `LayoutContext`, `Paragraph`, `build()` and `RichText`**

Create `src/context.rs`:

```rust
//! Per-thread layout state.

use crate::limits::{Warning, WarningSink};

/// Scratch state for layout. Create one per thread and reuse it; it is
/// `Send` but not meant to be shared.
#[derive(Debug, Default)]
pub struct LayoutContext {
    pub(crate) warnings: WarningSink,
}

impl LayoutContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Warnings recorded by line layout since the last call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        self.warnings.take()
    }

    /// Releases retained scratch memory above `bytes`. Nothing is retained
    /// yet, so this is currently a no-op.
    pub fn shrink_to(&mut self, _bytes: usize) {}
}
```

Create `src/paragraph.rs`:

```rust
//! Paragraphs: immutable results of `ParagraphBuilder::build`.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::analysis::units::{InlineBoxInfo, Unit, UnitList, bidi_levels, build_units};
use crate::analysis::{Item, ItemKind, process};
use crate::builder::ParagraphBuilder;
use crate::font::FontCollection;
use crate::geometry::{BaselineKind, Direction, Saturation, WritingMode};
use crate::limits::{LimitExceeded, Limits, Warning, WarningKind};
use crate::mapping::OffsetMapping;
use crate::node::NodeId;
use crate::shape::{GlyphStore, ShapedRun, shape_item};
use crate::style::{InlineStyle, ParagraphStyle, TextOrientation};

static NEXT_PARAGRAPH_ID: AtomicU64 = AtomicU64::new(1);

/// Largest accepted font size in px; larger values are clamped.
const MAX_FONT_SIZE: f32 = 1.0e6;

/// Position in a paragraph where a line starts. Opaque and cheap to copy;
/// valid only for the paragraph that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BreakToken {
    pub(crate) para: u64,
    pub(crate) unit: u32,
    pub(crate) flags: u8,
}

impl BreakToken {
    pub(crate) const FIRST_LINE: u8 = 1;
    pub(crate) const AFTER_FORCED: u8 = 2;
}

/// Marks how many floats of a paragraph have been handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FloatCursor(pub(crate) u32);

impl FloatCursor {
    /// The cursor just before this float (for withdrawing it), or `None`
    /// for the first float.
    pub fn before(self) -> Option<FloatCursor> {
        self.0.checked_sub(1).map(FloatCursor)
    }
}

pub(crate) struct ParagraphData {
    pub(crate) id: u64,
    pub(crate) style: ParagraphStyle,
    pub(crate) limits: Limits,
    pub(crate) text: String,
    pub(crate) items: Vec<Item>,
    pub(crate) styles: Vec<InlineStyle>,
    pub(crate) glyphs: GlyphStore,
    pub(crate) runs: Vec<ShapedRun>,
    pub(crate) units: Vec<Unit>,
    pub(crate) boxes: Vec<InlineBoxInfo>,
    pub(crate) float_count: u32,
    pub(crate) base_level: u8,
    pub(crate) mapping: Option<OffsetMapping>,
    pub(crate) fonts: FontCollection,
    pub(crate) generations: (u64, Option<u64>),
    pub(crate) warnings: Vec<Warning>,
    pub(crate) baselines: Vec<(NodeId, BaselineKind)>,
}

/// The analyzed and shaped inline content of one block container.
/// Immutable; clones share data and keep the same id.
#[derive(Clone)]
pub struct Paragraph {
    pub(crate) data: Arc<ParagraphData>,
}

impl fmt::Debug for Paragraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Paragraph").field("id", &self.data.id).field("text", &self.data.text).finish()
    }
}

impl Paragraph {
    pub fn id(&self) -> u64 {
        self.data.id
    }

    pub fn start_token(&self) -> BreakToken {
        BreakToken { para: self.data.id, unit: 0, flags: BreakToken::FIRST_LINE }
    }

    /// (shared layer, document layer) font generations at build time. If
    /// the collection's current generations differ, rebuild the paragraph.
    pub fn font_generations(&self) -> (u64, Option<u64>) {
        self.data.generations
    }

    pub fn warnings(&self) -> &[Warning] {
        &self.data.warnings
    }

    pub fn offset_mapping(&self) -> Option<&OffsetMapping> {
        self.data.mapping.as_ref()
    }

    /// The processed text (after white-space collapsing).
    pub fn text(&self) -> &str {
        &self.data.text
    }

    /// Which baseline of atomic inline `node` the caller must supply in
    /// `AtomicSizes`: the dominant baseline of its parent inline box.
    pub fn required_baseline(&self, node: NodeId) -> Option<BaselineKind> {
        self.data.baselines.iter().find(|(n, _)| *n == node).map(|(_, kind)| *kind)
    }

    pub(crate) fn from_builder(b: ParagraphBuilder, fonts: &FontCollection) -> Result<Paragraph, LimitExceeded> {
        let ParagraphBuilder { style, limits, text, items, mut styles, mut warnings, offset_mapping, .. } = b;
        for s in &mut styles {
            s.font_size = sanitize_font_size(s.font_size, &mut warnings);
        }
        let processed = process(&text, &items, &styles, offset_mapping);
        let mut sat = Saturation::default();
        let font = fonts.primary_font();
        let mut glyphs = GlyphStore::default();
        let mut runs = Vec::new();
        for (index, item) in processed.items.iter().enumerate() {
            if matches!(item.kind, ItemKind::Text) {
                let s = &processed.text[item.text.start as usize..item.text.end as usize];
                let size = styles[item.style as usize].font_size;
                shape_item(&mut glyphs, &mut runs, s, item.text.start, index as u32, font, size, &limits, &mut sat)?;
            }
        }
        let levels = bidi_levels(&processed.text, style.direction, style.unicode_bidi_plaintext);
        let base_level = u8::from(style.direction == Direction::Rtl);
        let UnitList { units, boxes, float_count } =
            build_units(&processed.text, &processed.items, &runs, &glyphs, &levels, base_level);
        let baselines = processed
            .items
            .iter()
            .filter_map(|item| match item.kind {
                ItemKind::Atomic { parent_style, .. } => {
                    Some((item.node?, baseline_kind(style.writing_mode, &styles[parent_style as usize])))
                }
                _ => None,
            })
            .collect();
        warnings.record_saturation(&sat);
        Ok(Paragraph {
            data: Arc::new(ParagraphData {
                id: NEXT_PARAGRAPH_ID.fetch_add(1, Ordering::Relaxed),
                style,
                limits,
                text: processed.text,
                items: processed.items,
                styles,
                glyphs,
                runs,
                units,
                boxes,
                float_count,
                base_level,
                mapping: processed.mapping,
                fonts: fonts.clone(),
                generations: fonts.generations(),
                warnings: warnings.take(),
                baselines,
            }),
        })
    }
}

/// The dominant baseline of a parent inline box (CSS Writing Modes 4 §4.2):
/// central in vertical typographic modes unless the text is set sideways.
fn baseline_kind(writing_mode: WritingMode, parent: &InlineStyle) -> BaselineKind {
    match writing_mode {
        WritingMode::VerticalRl | WritingMode::VerticalLr if parent.text_orientation != TextOrientation::Sideways => {
            BaselineKind::Central
        }
        _ => BaselineKind::Alphabetic,
    }
}

fn sanitize_font_size(size: f32, warnings: &mut crate::limits::WarningSink) -> f32 {
    if !size.is_finite() {
        warnings.push(WarningKind::NonFiniteInput, "non-finite font-size replaced with 0");
        0.0
    } else if size < 0.0 {
        warnings.push(WarningKind::NegativeInput, "negative font-size replaced with 0");
        0.0
    } else if size > MAX_FONT_SIZE {
        warnings.push(WarningKind::Saturated, "font-size clamped to 1e6 px");
        MAX_FONT_SIZE
    } else {
        size
    }
}
```

Add to `src/builder.rs` (below the `impl ParagraphBuilder` block; add `use crate::context::LayoutContext;`, `use crate::font::FontCollection;` and `use crate::paragraph::Paragraph;` to the imports):

```rust
impl ParagraphBuilder {
    /// Analyzes and shapes the content. Fails only when a resource limit was
    /// exceeded; inline boxes left open are closed with a warning.
    pub fn build(mut self, _cx: &mut LayoutContext, fonts: &FontCollection) -> Result<Paragraph, LimitExceeded> {
        while !self.stack.is_empty() && self.error.is_none() {
            self.warnings.push(WarningKind::UnbalancedInline, "unclosed inline box closed at build");
            self.close_inline();
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        if self.style.first_line.is_some() {
            self.warnings.push(WarningKind::Unsupported, "::first-line style is not applied yet");
        }
        Paragraph::from_builder(self, fonts)
    }
}

/// Convenience builder for plain rich text (no DOM). The n-th pushed span
/// gets `NodeId(n)`, starting at 0, and offsets within the pushed string map
/// through [`crate::mapping::OffsetMapping`].
pub struct RichText {
    builder: ParagraphBuilder,
    next_node: u64,
}

impl RichText {
    pub fn new(style: &ParagraphStyle) -> Self {
        Self::with_limits(style, &Limits::default())
    }

    pub fn with_limits(style: &ParagraphStyle, limits: &Limits) -> Self {
        Self { builder: ParagraphBuilder::new(style, limits), next_node: 0 }
    }

    pub fn push(mut self, text: &str, style: &InlineStyle) -> Self {
        let node = NodeId(self.next_node);
        self.next_node += 1;
        self.builder
            .open_inline(node, style, InlineEdges::default())
            .push_text(TextSource::Dom { node, offset: 0 }, text)
            .close_inline();
        self
    }

    pub fn build(self, cx: &mut LayoutContext, fonts: &FontCollection) -> Result<Paragraph, LimitExceeded> {
        self.builder.build(cx, fonts)
    }
}
```

Update `src/lib.rs`: add `mod context;` and `mod paragraph;` to the private modules, and extend the re-exports to:

```rust
pub use builder::{ParagraphBuilder, RichText};
pub use context::LayoutContext;
pub use paragraph::{BreakToken, FloatCursor, Paragraph};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test build && cargo test --lib`
Expected: PASS (11 integration tests; all unit tests including the 2 new `analysis::units` tests).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src tests/build.rs
git commit -m "Build paragraphs: units, bidi levels, required baselines, RichText

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 11: `next_line` — greedy breaking, tokens, strut, tabs

This task implements line breaking and the `Line` metrics. Fragments come in Task 12. Floats are laid out as zero-width anchors here (the float protocol is plan S0-B).

**Files:**
- Modify: `src/paragraph.rs`
- Create: `src/line/mod.rs`
- Create: `src/output.rs`
- Modify: `src/lib.rs`
- Test: `tests/lines.rs`

**Interfaces:**
- Consumes: `Paragraph`, `ParagraphData`, `BreakToken`, `FloatCursor` (Task 10); `Unit`, `UnitKind`, `BreakClass` (Task 10); `LayoutContext` (Task 10); `LineOptions`, `TabSize`, `LineHeight` (Task 4); `Sides` (Task 4); `LayoutUnit`, `Saturation` (Task 1); `FontCollection::metrics` (Task 6).
- Produces:
  - `paragraph.rs`: `pub struct AtomicSize { pub inline_size: f32, pub block_size: f32, pub baseline: Option<f32>, pub margins: Sides }`; `pub struct AtomicSizes` with `const EMPTY`, `new()`, `insert(NodeId, AtomicSize)` (bumps the generation), `get(NodeId) -> Option<&AtomicSize>`, `generation() -> u64`; `pub struct BreakPlan` (no public constructor yet); `pub struct LineConstraint<'a> { pub available_inline_size, pub inline_start_offset, pub block_offset: f32, pub max_block_size: Option<f32>, pub floats_placed_through: Option<FloatCursor>, pub break_plan: Option<&'a BreakPlan> }` with `new(available: f32)`; `#[non_exhaustive] pub enum LineResult { Line(Line), Done, BlockSizeExceeded { needed_block_size: f32 }, FloatEncountered { node, line_start: BreakToken, inline_position: f32, float_cursor: FloatCursor }, BlockInInline { node, token_after: BreakToken }, InvalidToken }`.
  - `line/mod.rs`: `impl Paragraph { pub fn next_line(&self, &mut LayoutContext, BreakToken, &LineOptions, &LineConstraint<'_>, &AtomicSizes) -> LineResult }`; `pub(crate) struct Scan { end: usize, reason: BreakReason, widths: Vec<LayoutUnit>, content: LayoutUnit }` (`widths[k]` is the width of unit `start + k`).
  - `output.rs`: `pub enum BreakReason { Regular, Forced, Emergency, BlockInInline, End }`; `pub struct Line` (Clone, Debug, Send + Sync, `'static`) with `break_token()`, `break_reason()`, `is_last()`, `inline_size()`, `block_size()`, `block_offset()`, `baseline(BaselineKind) -> f32`, `text_range() -> Range<usize>`, `displaced_floats() -> &[(NodeId, FloatCursor)]`; crate fields `data: Arc<ParagraphData>`, `units: Range<u32>`, `widths: Vec<LayoutUnit>`, `origin: LayoutUnit` (inline position of the first unit), `ascent`, `descent`, `baseline`, `block_size`, `inline_size: LayoutUnit`; `pub(crate) fn Line::new(&Paragraph, BreakToken, Scan, origin: LayoutUnit, block_offset: f32, &mut Saturation) -> Line`.

- [ ] **Step 1: Write the failing tests**

Create `tests/lines.rs`:

```rust
use shodo::font::FontCollection;
use shodo::geometry::BaselineKind;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, LineHeight, LineOptions, ParagraphStyle, TabSize, TextIndent, WhiteSpaceCollapse};
use shodo::{AtomicSizes, BreakReason, LayoutContext, LineConstraint, LineResult, Paragraph, ParagraphBuilder};

fn root(line_height: LineHeight) -> ParagraphStyle {
    let root = InlineStyle { font_size: 10.0, line_height, ..InlineStyle::default() };
    ParagraphStyle { root, ..ParagraphStyle::default() }
}

fn para_with(style: &ParagraphStyle, build: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(style, &Limits::default());
    build(&mut b);
    b.build(&mut LayoutContext::new(), &FontCollection::new(&Limits::default())).unwrap()
}

fn para(text: &str) -> Paragraph {
    para_with(&root(LineHeight::Normal), |b| {
        b.push_text(TextSource::Dom { node: NodeId(1), offset: 0 }, text);
    })
}

fn lines(para: &Paragraph, width: f32, options: &LineOptions) -> Vec<shodo::Line> {
    let mut cx = LayoutContext::new();
    let mut token = para.start_token();
    let mut out = Vec::new();
    loop {
        match para.next_line(&mut cx, token, options, &LineConstraint::new(width), &AtomicSizes::EMPTY) {
            LineResult::Line(line) => {
                token = line.break_token();
                out.push(line);
            }
            LineResult::Done => return out,
            other => panic!("unexpected {other:?}"),
        }
        assert!(out.len() <= para.text().len() + 1, "no progress");
    }
}

fn texts(para: &Paragraph, width: f32) -> Vec<String> {
    lines(para, width, &LineOptions::default()).iter().map(|l| para.text()[l.text_range()].to_string()).collect()
}

#[test]
fn breaks_greedily_at_spaces() {
    let p = para("aaa bbb ccc");
    assert_eq!(texts(&p, 65.0), ["aaa ", "bbb ", "ccc"]);
    assert_eq!(texts(&p, 70.0), ["aaa bbb ", "ccc"]);
}

#[test]
fn trailing_spaces_hang() {
    let p = para("aaa bbb");
    let first = &lines(&p, 45.0, &LineOptions::default())[0];
    assert_eq!(first.inline_size(), 30.0);
    assert_eq!(first.break_reason(), BreakReason::Regular);
    assert!(!first.is_last());
}

#[test]
fn forced_breaks_end_lines() {
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.push_text(TextSource::Dom { node: NodeId(1), offset: 0 }, "ab")
            .push_forced_break(NodeId(2))
            .push_text(TextSource::Dom { node: NodeId(3), offset: 0 }, "cd");
    });
    let ls = lines(&p, 100.0, &LineOptions::default());
    assert_eq!(ls.len(), 2);
    assert_eq!(ls[0].break_reason(), BreakReason::Forced);
    assert!(ls[0].is_last());
    assert_eq!(ls[1].break_reason(), BreakReason::End);
}

#[test]
fn words_wider_than_the_line_overflow() {
    let p = para("aaaaaaaa b");
    let ls = lines(&p, 30.0, &LineOptions::default());
    assert_eq!(p.text()[ls[0].text_range()].to_string(), "aaaaaaaa ");
    assert_eq!(ls[0].inline_size(), 80.0);
    assert_eq!(ls.len(), 2);
}

#[test]
fn zero_and_negative_widths_still_progress() {
    let p = para("a b c");
    assert_eq!(texts(&p, 0.0), ["a ", "b ", "c"]);
    let mut cx = LayoutContext::new();
    let r = p.next_line(&mut cx, p.start_token(), &LineOptions::default(), &LineConstraint::new(-5.0), &AtomicSizes::EMPTY);
    assert!(matches!(r, LineResult::Line(_)));
    assert!(cx.take_warnings().iter().any(|w| w.kind == WarningKind::NegativeInput));
    for width in [-1.0, 0.0, 1.0, 7.0, 13.0, f32::NAN, 1.0e9] {
        lines(&para("a bb ccc dddd"), width, &LineOptions::default());
    }
}

#[test]
fn empty_paragraph_is_done_immediately() {
    let p = para("   ");
    let r = p.next_line(&mut LayoutContext::new(), p.start_token(), &LineOptions::default(), &LineConstraint::new(100.0), &AtomicSizes::EMPTY);
    assert!(matches!(r, LineResult::Done));
}

#[test]
fn tokens_are_tied_to_their_paragraph() {
    let a = para("aaa");
    let b = para("aaa");
    let r = b.next_line(&mut LayoutContext::new(), a.start_token(), &LineOptions::default(), &LineConstraint::new(100.0), &AtomicSizes::EMPTY);
    assert!(matches!(r, LineResult::InvalidToken));
    let r = a.clone().next_line(&mut LayoutContext::new(), a.start_token(), &LineOptions::default(), &LineConstraint::new(100.0), &AtomicSizes::EMPTY);
    assert!(matches!(r, LineResult::Line(_)));
}

#[test]
fn a_saved_token_resumes_at_another_width() {
    let p = para("aaa bbb ccc ddd");
    let mut cx = LayoutContext::new();
    let opts = LineOptions::default();
    let LineResult::Line(first) = p.next_line(&mut cx, p.start_token(), &opts, &LineConstraint::new(35.0), &AtomicSizes::EMPTY) else { panic!() };
    assert_eq!(&p.text()[first.text_range()], "aaa ");
    let LineResult::Line(rest) = p.next_line(&mut cx, first.break_token(), &opts, &LineConstraint::new(1000.0), &AtomicSizes::EMPTY) else { panic!() };
    assert_eq!(&p.text()[rest.text_range()], "bbb ccc ddd");
}

#[test]
fn text_indent_applies_to_the_first_line() {
    let p = para("aaa bbb");
    let indented = LineOptions { text_indent: TextIndent { length: 20.0, ..TextIndent::default() }, ..LineOptions::default() };
    assert_eq!(lines(&p, 70.0, &LineOptions::default()).len(), 1);
    assert_eq!(lines(&p, 70.0, &indented).len(), 2);
}

#[test]
fn strut_sets_block_size_and_baseline() {
    let normal = &lines(&para("a"), 100.0, &LineOptions::default())[0];
    assert_eq!((normal.block_size(), normal.baseline(BaselineKind::Alphabetic)), (10.0, 8.0));
    let p = para_with(&root(LineHeight::Px(20.0)), |b| {
        b.push_text(TextSource::Dom { node: NodeId(1), offset: 0 }, "a");
    });
    let tall = &lines(&p, 100.0, &LineOptions::default())[0];
    assert_eq!((tall.block_size(), tall.baseline(BaselineKind::Alphabetic)), (20.0, 13.0));
    assert_eq!(tall.baseline(BaselineKind::Central), 10.0);
}

#[test]
fn tab_stops_are_measured_from_the_content_edge() {
    let pre = InlineStyle {
        font_size: 10.0,
        white_space_collapse: WhiteSpaceCollapse::Preserve,
        tab_size: TabSize::Px(40.0),
        ..InlineStyle::default()
    };
    let p = para_with(&root(LineHeight::Normal), |b| {
        b.open_inline(NodeId(1), &pre, InlineEdges::default())
            .push_text(TextSource::Dom { node: NodeId(2), offset: 0 }, "a\tb")
            .close_inline();
    });
    let opts = LineOptions::default();
    let mut cx = LayoutContext::new();
    let LineResult::Line(line) = p.next_line(&mut cx, p.start_token(), &opts, &LineConstraint::new(1000.0), &AtomicSizes::EMPTY) else { panic!() };
    assert_eq!(line.inline_size(), 50.0);
    let shifted = LineConstraint { inline_start_offset: 5.0, ..LineConstraint::new(1000.0) };
    let LineResult::Line(line) = p.next_line(&mut cx, p.start_token(), &opts, &shifted, &AtomicSizes::EMPTY) else { panic!() };
    assert_eq!(line.inline_size(), 45.0);
}

#[test]
fn line_is_send_sync_and_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<shodo::Line>();
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test lines`
Expected: FAIL to compile ("unresolved imports `shodo::AtomicSizes`, `shodo::BreakReason`, ...").

- [ ] **Step 3: Add the constraint and result types**

Append to `src/paragraph.rs` (add `use std::collections::BTreeMap;`, `use crate::node::Sides;` and `use crate::output::Line;` to its imports):

```rust
/// Size of an atomic inline (image, inline-block), supplied by the caller
/// before line layout. `baseline` is measured from the top of the margin box
/// and must be of the kind reported by [`Paragraph::required_baseline`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AtomicSize {
    pub inline_size: f32,
    pub block_size: f32,
    pub baseline: Option<f32>,
    pub margins: Sides,
}

/// Sizes of atomic inlines by node. The generation changes on every insert.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtomicSizes {
    map: BTreeMap<NodeId, AtomicSize>,
    generation: u64,
}

impl AtomicSizes {
    pub const EMPTY: AtomicSizes = AtomicSizes { map: BTreeMap::new(), generation: 0 };

    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, node: NodeId, size: AtomicSize) {
        self.map.insert(node, size);
        self.generation += 1;
    }

    pub fn get(&self, node: NodeId) -> Option<&AtomicSize> {
        self.map.get(&node)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// A precomputed set of break positions for `text-wrap: balance | pretty`.
/// Produced by `Paragraph::plan_breaks` (not available yet).
#[derive(Clone, Debug, PartialEq)]
pub struct BreakPlan {
    pub(crate) para: u64,
    pub(crate) width: f32,
    pub(crate) atomics_generation: u64,
}

/// Space available to one line.
#[derive(Clone, Copy, Debug)]
pub struct LineConstraint<'a> {
    pub available_inline_size: f32,
    /// Inline-start inset from floats, relative to the content box.
    pub inline_start_offset: f32,
    /// Block position of the line within the container; copied to the line.
    pub block_offset: f32,
    pub max_block_size: Option<f32>,
    pub floats_placed_through: Option<FloatCursor>,
    pub break_plan: Option<&'a BreakPlan>,
}

impl LineConstraint<'_> {
    pub fn new(available_inline_size: f32) -> Self {
        Self {
            available_inline_size,
            inline_start_offset: 0.0,
            block_offset: 0.0,
            max_block_size: None,
            floats_placed_through: None,
            break_plan: None,
        }
    }
}

/// Outcome of [`Paragraph::next_line`].
#[derive(Debug)]
#[non_exhaustive]
pub enum LineResult {
    Line(Line),
    /// No content is left.
    Done,
    /// The line would be taller than `max_block_size`.
    BlockSizeExceeded { needed_block_size: f32 },
    /// A float was reached; place it and call again from `line_start`.
    FloatEncountered { node: NodeId, line_start: BreakToken, inline_position: f32, float_cursor: FloatCursor },
    /// A block-level box inside inline content was reached; lay it out and
    /// continue from `token_after`.
    BlockInInline { node: NodeId, token_after: BreakToken },
    /// The token belongs to another paragraph or is out of range.
    InvalidToken,
}
```

- [ ] **Step 4: Implement `Line`**

Create `src/output.rs`:

```rust
//! Line layout output.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::geometry::{BaselineKind, LayoutUnit, Saturation};
use crate::line::Scan;
use crate::node::NodeId;
use crate::paragraph::{BreakToken, FloatCursor, Paragraph, ParagraphData};
use crate::style::LineHeight;

/// Why a line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    /// At a soft break opportunity.
    Regular,
    /// At a forced break (`<br>`, preserved newline).
    Forced,
    /// Inside a word because of `overflow-wrap` (not produced yet).
    Emergency,
    /// Before a block-level box inside inline content.
    BlockInInline,
    /// At the end of the paragraph.
    End,
}

/// One laid-out line. Owns a reference to its paragraph's data, so it can
/// outlive the `Paragraph` handle and be sent between threads.
#[derive(Clone)]
pub struct Line {
    pub(crate) data: Arc<ParagraphData>,
    pub(crate) break_token: BreakToken,
    pub(crate) reason: BreakReason,
    pub(crate) units: Range<u32>,
    pub(crate) widths: Vec<LayoutUnit>,
    pub(crate) origin: LayoutUnit,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) block_size: LayoutUnit,
    pub(crate) baseline: LayoutUnit,
    pub(crate) ascent: LayoutUnit,
    pub(crate) descent: LayoutUnit,
    pub(crate) block_offset: f32,
    pub(crate) displaced: Vec<(NodeId, FloatCursor)>,
}

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Line")
            .field("units", &self.units)
            .field("reason", &self.reason)
            .field("inline_size", &self.inline_size())
            .finish()
    }
}

impl Line {
    pub(crate) fn new(
        para: &Paragraph,
        token: BreakToken,
        scan: Scan,
        origin: LayoutUnit,
        block_offset: f32,
        sat: &mut Saturation,
    ) -> Line {
        let data = &para.data;
        let root = &data.styles[0];
        let size = root.font_size;
        let m = data.fonts.metrics(data.fonts.primary_font(), size);
        let line_height = match root.line_height {
            LineHeight::Normal => m.ascent + m.descent + m.line_gap,
            LineHeight::Px(v) => v,
            LineHeight::Number(n) => n * size,
        };
        // CSS 2.1 §10.8.1: half the leading goes above the ascent.
        let half_leading = (line_height - (m.ascent + m.descent)) / 2.0;
        let flags = match scan.reason {
            BreakReason::Forced => BreakToken::AFTER_FORCED,
            _ => 0,
        };
        Line {
            data: Arc::clone(&para.data),
            break_token: BreakToken { para: data.id, unit: scan.end as u32, flags },
            reason: scan.reason,
            units: token.unit..scan.end as u32,
            widths: scan.widths,
            origin,
            inline_size: scan.content,
            block_size: LayoutUnit::from_f32_ceil(line_height, sat),
            baseline: LayoutUnit::from_f32_round(half_leading + m.ascent, sat),
            ascent: LayoutUnit::from_f32_round(m.ascent, sat),
            descent: LayoutUnit::from_f32_round(m.descent, sat),
            block_offset,
            displaced: Vec::new(),
        }
    }

    pub fn break_token(&self) -> BreakToken {
        self.break_token
    }

    pub fn break_reason(&self) -> BreakReason {
        self.reason
    }

    /// Whether this is the last line of the paragraph or of a forced-break
    /// section (the line `text-align-last` applies to).
    pub fn is_last(&self) -> bool {
        matches!(self.reason, BreakReason::Forced | BreakReason::End | BreakReason::BlockInInline)
    }

    /// Width of the content, excluding hanging trailing spaces, text-indent
    /// and the inline-start offset.
    pub fn inline_size(&self) -> f32 {
        self.inline_size.to_f32()
    }

    /// Line advance (distance to the next line).
    pub fn block_size(&self) -> f32 {
        self.block_size.to_f32()
    }

    pub fn block_offset(&self) -> f32 {
        self.block_offset
    }

    /// Position of a baseline, from the top of the line box. Only the
    /// alphabetic baseline comes from font data; the others are derived from
    /// the strut's ascent and descent.
    pub fn baseline(&self, kind: BaselineKind) -> f32 {
        let alphabetic = self.baseline.to_f32();
        let (ascent, descent) = (self.ascent.to_f32(), self.descent.to_f32());
        match kind {
            BaselineKind::Alphabetic => alphabetic,
            BaselineKind::Central => alphabetic - (ascent - descent) / 2.0,
            BaselineKind::Ideographic => alphabetic + descent,
            BaselineKind::Hanging => alphabetic - 0.8 * ascent,
        }
    }

    /// Range of the paragraph's processed text covered by this line.
    pub fn text_range(&self) -> Range<usize> {
        let units = &self.data.units[self.units.start as usize..self.units.end as usize];
        match (units.first(), units.last()) {
            (Some(first), Some(last)) => first.text.start as usize..last.text.end as usize,
            _ => 0..0,
        }
    }

    /// Floats reported for this line whose anchors ended up after its end.
    pub fn displaced_floats(&self) -> &[(NodeId, FloatCursor)] {
        &self.displaced
    }
}
```

- [ ] **Step 5: Implement `next_line`**

Create `src/line/mod.rs`:

```rust
//! Line breaking.

use crate::analysis::units::{BreakClass, Unit, UnitKind};
use crate::context::LayoutContext;
use crate::geometry::{LayoutUnit, Saturation};
use crate::limits::WarningKind;
use crate::output::{BreakReason, Line};
use crate::paragraph::{AtomicSizes, BreakToken, LineConstraint, LineResult, Paragraph, ParagraphData};
use crate::style::{LineOptions, TabSize};

/// Result of scanning one line.
pub(crate) struct Scan {
    pub(crate) end: usize,
    pub(crate) reason: BreakReason,
    /// Width of every unit in the line, in order.
    pub(crate) widths: Vec<LayoutUnit>,
    /// Content width, excluding text-indent and hanging trailing spaces.
    pub(crate) content: LayoutUnit,
}

impl Paragraph {
    /// Lays out the line starting at `token`. Pure: the same inputs always
    /// give the same result, so a token can be retried with other
    /// constraints.
    pub fn next_line(
        &self,
        cx: &mut LayoutContext,
        token: BreakToken,
        options: &LineOptions,
        constraint: &LineConstraint<'_>,
        atomics: &AtomicSizes,
    ) -> LineResult {
        let data = &*self.data;
        cx.warnings.set_max(data.limits.max_warnings);
        if token.para != data.id || token.unit as usize > data.units.len() {
            return LineResult::InvalidToken;
        }
        let start = token.unit as usize;
        if start == data.units.len() {
            return LineResult::Done;
        }
        if let UnitKind::BlockInInline { node } = data.units[start].kind {
            let token_after = BreakToken { para: data.id, unit: token.unit + 1, flags: BreakToken::AFTER_FORCED };
            return LineResult::BlockInInline { node, token_after };
        }
        let mut sat = Saturation::default();
        let available = non_negative(constraint.available_inline_size, "available_inline_size", cx, &mut sat);
        let offset = LayoutUnit::from_f32_round(constraint.inline_start_offset, &mut sat);
        let indent = text_indent(options, token.flags, &mut sat);
        let scan = scan(data, start, available, offset, indent, atomics, cx, &mut sat);
        let origin = offset.add(indent, &mut sat);
        let line = Line::new(self, token, scan, origin, constraint.block_offset, &mut sat);
        cx.warnings.record_saturation(&sat);
        LineResult::Line(line)
    }
}

fn non_negative(value: f32, what: &str, cx: &mut LayoutContext, sat: &mut Saturation) -> LayoutUnit {
    let v = LayoutUnit::from_f32_round(value, sat);
    if v < LayoutUnit::ZERO {
        cx.warnings.push(WarningKind::NegativeInput, format!("negative {what} replaced with 0"));
        LayoutUnit::ZERO
    } else {
        v
    }
}

/// CSS Text 3 §8.1: the first line (and, with `each-line`, lines after a
/// forced break) is indented; `hanging` inverts which lines are.
fn text_indent(options: &LineOptions, flags: u8, sat: &mut Saturation) -> LayoutUnit {
    let ti = options.text_indent;
    let first = flags & BreakToken::FIRST_LINE != 0;
    let after_forced = flags & BreakToken::AFTER_FORCED != 0;
    let applies = first || (ti.each_line && after_forced);
    if applies != ti.hanging { LayoutUnit::from_f32_round(ti.length, sat) } else { LayoutUnit::ZERO }
}

#[allow(clippy::too_many_arguments)]
fn scan(
    data: &ParagraphData,
    start: usize,
    available: LayoutUnit,
    offset: LayoutUnit,
    indent: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> Scan {
    let units = &data.units;
    let mut widths = Vec::new();
    let mut pos = indent;
    let mut last_break: Option<usize> = None;
    let mut overflowing = false;
    let mut i = start;
    let reason = loop {
        let Some(unit) = units.get(i) else { break BreakReason::End };
        match unit.kind {
            UnitKind::ForcedBreak => {
                widths.push(LayoutUnit::ZERO);
                i += 1;
                break BreakReason::Forced;
            }
            UnitKind::BlockInInline { .. } => break BreakReason::BlockInInline,
            _ => {}
        }
        let w = unit_width(data, unit, offset.add(pos, sat), atomics, cx, sat);
        // Trailing spaces hang and never cause a break (CSS Text 3 §4.1.3).
        let hangs = matches!(unit.kind, UnitKind::Cluster { space: true, .. });
        if !hangs && !overflowing && i > start && pos.add(w, sat) > available {
            if let Some(b) = last_break {
                widths.truncate(b - start);
                i = b;
                break BreakReason::Regular;
            }
            // No opportunity yet: the unbreakable run overflows
            // (`overflow-wrap: normal`) and the line ends at the next one.
            overflowing = true;
        }
        widths.push(w);
        pos = pos.add(w, sat);
        i += 1;
        if unit.break_after == BreakClass::Allowed {
            if overflowing {
                break BreakReason::Regular;
            }
            last_break = Some(i);
        }
    };
    // Inline box ends right after a soft break stay on the line that ends there.
    if reason == BreakReason::Regular {
        while let Some(unit) = units.get(i)
            && matches!(unit.kind, UnitKind::Close { .. })
        {
            widths.push(unit_width(data, unit, LayoutUnit::ZERO, atomics, cx, sat));
            i += 1;
        }
    }
    let total = widths.iter().fold(LayoutUnit::ZERO, |acc, w| acc.add(*w, sat));
    let mut trailing = LayoutUnit::ZERO;
    for (k, unit) in units[start..i].iter().enumerate().rev() {
        match unit.kind {
            UnitKind::Cluster { space: true, .. } => trailing = trailing.add(widths[k], sat),
            UnitKind::Close { .. }
            | UnitKind::BidiControl
            | UnitKind::Float { .. }
            | UnitKind::Absolute { .. }
            | UnitKind::ForcedBreak => {}
            _ => break,
        }
    }
    Scan { end: i, reason, widths, content: total.sub(trailing, sat) }
}

/// Inline advance of one unit. `content_pos` is the unit's position from the
/// content edge of the block container (tab stops are measured from it).
fn unit_width(
    data: &ParagraphData,
    unit: &Unit,
    content_pos: LayoutUnit,
    atomics: &AtomicSizes,
    cx: &mut LayoutContext,
    sat: &mut Saturation,
) -> LayoutUnit {
    match &unit.kind {
        UnitKind::Cluster { glyphs, .. } => {
            glyphs.clone().fold(LayoutUnit::ZERO, |acc, g| acc.add(data.glyphs.advance[g as usize], sat))
        }
        UnitKind::Open { box_index } => {
            LayoutUnit::from_f32_round(data.boxes[*box_index as usize].edges.inline_start_total(), sat)
        }
        UnitKind::Close { box_index } => {
            LayoutUnit::from_f32_round(data.boxes[*box_index as usize].edges.inline_end_total(), sat)
        }
        UnitKind::Atomic { node } => match atomics.get(*node) {
            Some(size) => {
                let inline = if size.inline_size < 0.0 {
                    cx.warnings.push(WarningKind::NegativeInput, "negative atomic inline size replaced with 0");
                    0.0
                } else {
                    size.inline_size
                };
                LayoutUnit::from_f32_round(inline + size.margins.inline_sum(), sat)
            }
            None => {
                cx.warnings.push(WarningKind::MissingAtomicSize, format!("no size for atomic inline {node:?}"));
                LayoutUnit::ZERO
            }
        },
        UnitKind::Tab => tab_width(data, unit, content_pos, sat),
        _ => LayoutUnit::ZERO,
    }
}

/// Distance to the next tab stop (CSS Text 3 §4.2). The placeholder shaper
/// gives the space character a 1em advance.
fn tab_width(data: &ParagraphData, unit: &Unit, content_pos: LayoutUnit, sat: &mut Saturation) -> LayoutUnit {
    let style = &data.styles[data.items[unit.item as usize].style as usize];
    let interval = match style.tab_size {
        TabSize::Spaces(n) => n * style.font_size,
        TabSize::Px(v) => v,
    };
    let interval = i64::from(LayoutUnit::from_f32_round(interval, sat).raw());
    if interval <= 0 {
        return LayoutUnit::ZERO;
    }
    let x = i64::from(content_pos.raw());
    let next = (x.div_euclid(interval) + 1) * interval;
    LayoutUnit::from_raw((next - x).clamp(0, i64::from(i32::MAX)) as i32)
}
```

Update `src/lib.rs`: add `mod line;` and `mod output;` to the private modules, and change the re-exports to:

```rust
pub use builder::{ParagraphBuilder, RichText};
pub use context::LayoutContext;
pub use output::{BreakReason, Line};
pub use paragraph::{
    AtomicSize, AtomicSizes, BreakPlan, BreakToken, FloatCursor, LineConstraint, LineResult, Paragraph,
};
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --test lines && cargo test`
Expected: PASS (12 tests in `tests/lines.rs`; everything else still green).

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src tests/lines.rs
git commit -m "Add greedy next_line with tokens, strut, indent and tab stops

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 12: Fragments — glyph runs, inline boxes, atomics, anchors

Fragments are built in logical order here; Task 13 reorders them visually for bidi.

**Files:**
- Create: `src/line/fragments.rs`
- Modify: `src/line/mod.rs`
- Modify: `src/output.rs`
- Modify: `src/lib.rs`
- Test: `tests/fragments.rs`

**Interfaces:**
- Consumes: `Line`, `Scan` (Task 11); `ParagraphData`, `AtomicSizes`, `AtomicSize` (Tasks 10–11); `Unit`, `UnitKind`, `InlineBoxInfo` (Task 10); `FontCollection::{metrics, font_data, primary_font}` (Task 6); `LogicalRect`, `BaselineKind` (Task 2).
- Produces:
  - `line/fragments.rs`: `pub(crate) struct FragmentRecord { kind: RecordKind, inline_start: LayoutUnit, inline_size: LayoutUnit, level: u8 }`; `pub(crate) enum RecordKind { Glyphs { run: u32, glyphs: Range<u32>, item: u32, text: Range<u32> }, Atomic { node: NodeId, size: AtomicSize }, InlineBox { box_index: u32, start_edge: bool, end_edge: bool, parent: Option<u32> }, Anchor { node: NodeId, kind: OutOfFlowKind } }`; `pub(crate) fn build(data: &ParagraphData, units: Range<usize>, widths: &[LayoutUnit], origin: LayoutUnit, atomics: &AtomicSizes) -> Vec<FragmentRecord>`. `InlineBox::parent` is the index of the parent inline-box record in the same vector.
  - `output.rs`: `Line` gains `fragments: Vec<FragmentRecord>` and public `fragments() -> impl ExactSizeIterator<Item = Fragment<'_>>`, `fragment(usize) -> Option<Fragment<'_>>`, `font_data(FontId) -> Option<peniko::FontData>`, `is_empty() -> bool`. `Line::new` gains an `atomics: &AtomicSizes` parameter (after `block_offset`).
  - Public views: `pub enum Fragment<'a> { GlyphRun(GlyphRunView<'a>), Atomic(AtomicFragment), InlineBox(InlineBoxFragment), OutOfFlowAnchor(AnchorFragment) }`; `pub struct GlyphRunView<'a>` with `node() -> Option<NodeId>`, `font() -> FontId`, `font_size() -> f32`, `font_data() -> Option<FontData>`, `bidi_level() -> u8`, `text_range() -> Range<usize>`, `inline_start() -> f32`, `inline_size() -> f32`, `baseline() -> f32`, `glyphs() -> Glyphs<'a>`; `pub struct Glyphs<'a>` (ExactSizeIterator of `Glyph`, plus `get(usize) -> Option<Glyph>`); `pub struct Glyph { pub id: u32, pub inline_position: f32, pub block_offset: f32, pub advance: f32, pub cluster: u32 }`; `pub struct InlineBoxFragment { pub node: NodeId, pub rect: LogicalRect, pub content_rect: LogicalRect, pub has_start_edge: bool, pub has_end_edge: bool, pub parent: Option<usize>, pub font: FontId, pub font_size: f32 }`; `pub struct AtomicFragment { pub node: NodeId, pub margin_rect: LogicalRect, pub border_rect: LogicalRect, pub baseline: f32 }`; `pub struct AnchorFragment { pub node: NodeId, pub kind: OutOfFlowKind, pub inline_position: f32 }`.
  - Coordinates: inline positions are from the content-box inline-start edge of the container (they include `inline_start_offset` and text-indent); block positions are from the top of the line box. `AtomicFragment::baseline` and `GlyphRunView::baseline` are line-relative.

- [ ] **Step 1: Write the failing tests**

Create `tests/fragments.rs`:

```rust
use shodo::font::FontCollection;
use shodo::limits::{Limits, WarningKind};
use shodo::node::{InlineEdges, NodeId, OutOfFlowKind, Sides, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{
    AtomicSize, AtomicSizes, Fragment, InlineBoxFragment, LayoutContext, Line, LineConstraint, LineResult, Paragraph,
    ParagraphBuilder,
};

fn style() -> ParagraphStyle {
    ParagraphStyle { root: InlineStyle { font_size: 10.0, ..InlineStyle::default() }, ..ParagraphStyle::default() }
}

fn span() -> InlineStyle {
    InlineStyle { font_size: 10.0, ..InlineStyle::default() }
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom { node: NodeId(node), offset: 0 }
}

fn para(build: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
    let mut b = ParagraphBuilder::new(&style(), &Limits::default());
    build(&mut b);
    b.build(&mut LayoutContext::new(), &FontCollection::new(&Limits::default())).unwrap()
}

fn all_lines(p: &Paragraph, width: f32, atomics: &AtomicSizes, cx: &mut LayoutContext) -> Vec<Line> {
    let mut token = p.start_token();
    let mut out = Vec::new();
    while let LineResult::Line(line) = p.next_line(cx, token, &LineOptions::default(), &LineConstraint::new(width), atomics) {
        token = line.break_token();
        out.push(line);
    }
    out
}

fn boxes(line: &Line) -> Vec<InlineBoxFragment> {
    line.fragments().filter_map(|f| if let Fragment::InlineBox(b) = f { Some(b) } else { None }).collect()
}

#[test]
fn glyph_runs_split_at_item_boundaries() {
    let p = para(|b| {
        b.push_text(dom(1), "ab")
            .open_inline(NodeId(2), &span(), InlineEdges::default())
            .push_text(dom(3), "cd")
            .close_inline()
            .push_text(dom(4), "ef");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let runs: Vec<_> = line.fragments().filter_map(|f| if let Fragment::GlyphRun(r) = f { Some(r) } else { None }).collect();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs.iter().map(|r| r.node()).collect::<Vec<_>>(), [Some(NodeId(1)), Some(NodeId(3)), Some(NodeId(4))]);
    let positions: Vec<f32> = runs.iter().flat_map(|r| r.glyphs().map(|g| g.inline_position)).collect();
    assert_eq!(positions, [0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
    assert_eq!(runs[1].glyphs().len(), 2);
    assert_eq!(runs[1].glyphs().get(1).unwrap().inline_position, 30.0);
    assert!(runs[0].font_data().is_some());
    assert!(line.font_data(runs[0].font()).is_some());
    assert_eq!(line.fragments().len(), 4, "3 runs + the span's inline box");
}

#[test]
fn glyph_offsets_are_applied_on_top_of_pen_positions() {
    let p = para(|b| {
        b.push_text(dom(1), "e\u{301}");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let Some(Fragment::GlyphRun(run)) = line.fragment(0) else { panic!() };
    let glyphs: Vec<_> = run.glyphs().collect();
    assert_eq!((glyphs[1].inline_position, glyphs[1].advance), (5.0, 0.0));
}

#[test]
fn inline_boxes_carry_edges_only_where_they_start_and_end() {
    let padded = InlineEdges { padding: Sides { inline_start: 5.0, inline_end: 5.0, block_start: 2.0, block_end: 2.0 }, ..InlineEdges::default() };
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), padded).push_text(dom(2), "aaa bbb").close_inline();
    });
    let lines = all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new());
    assert_eq!(lines.len(), 2);
    let first = boxes(&lines[0]);
    let second = boxes(&lines[1]);
    assert_eq!((first[0].has_start_edge, first[0].has_end_edge), (true, false));
    assert_eq!((second[0].has_start_edge, second[0].has_end_edge), (false, true));
    assert_eq!((first[0].rect.inline_start, first[0].rect.inline_size), (0.0, 45.0));
    assert_eq!(first[0].content_rect.inline_start, 5.0);
    assert_eq!((second[0].rect.inline_size, second[0].content_rect.inline_size), (35.0, 30.0));
    // Block padding extends the border box around the 10px content area.
    assert_eq!((first[0].rect.block_start, first[0].rect.block_size), (-2.0, 14.0));
    assert_eq!((first[0].content_rect.block_start, first[0].content_rect.block_size), (0.0, 10.0));
}

#[test]
fn nested_boxes_point_at_their_parent_fragment() {
    let p = para(|b| {
        b.open_inline(NodeId(1), &span(), InlineEdges::default())
            .open_inline(NodeId(2), &span(), InlineEdges::default())
            .push_text(dom(3), "aaa bbb")
            .close_inline()
            .close_inline();
    });
    for line in all_lines(&p, 45.0, &AtomicSizes::EMPTY, &mut LayoutContext::new()) {
        let fragments: Vec<_> = line.fragments().collect();
        let Fragment::InlineBox(outer) = &fragments[0] else { panic!() };
        let Fragment::InlineBox(inner) = &fragments[1] else { panic!() };
        assert_eq!((outer.node, outer.parent), (NodeId(1), None));
        assert_eq!((inner.node, inner.parent), (NodeId(2), Some(0)));
    }
}

#[test]
fn atomics_sit_on_the_baseline() {
    let p = para(|b| {
        b.push_text(dom(1), "a").push_atomic(NodeId(2), &span(), InlineEdges::default());
    });
    let mut atomics = AtomicSizes::new();
    let margins = Sides { inline_start: 1.0, inline_end: 1.0, ..Sides::default() };
    atomics.insert(NodeId(2), AtomicSize { inline_size: 20.0, block_size: 30.0, baseline: None, margins });
    let line = &all_lines(&p, 100.0, &atomics, &mut LayoutContext::new())[0];
    let atomic = line.fragments().find_map(|f| if let Fragment::Atomic(a) = f { Some(a) } else { None }).unwrap();
    assert_eq!((atomic.margin_rect.inline_start, atomic.margin_rect.inline_size), (10.0, 22.0));
    assert_eq!((atomic.border_rect.inline_start, atomic.border_rect.inline_size), (11.0, 20.0));
    // No baseline given: the bottom of the margin box sits on the baseline (8px).
    assert_eq!((atomic.margin_rect.block_start, atomic.margin_rect.block_size), (-22.0, 30.0));
    assert_eq!(atomic.baseline, 8.0);
}

#[test]
fn missing_atomic_sizes_warn_and_collapse_to_zero() {
    let p = para(|b| {
        b.push_atomic(NodeId(2), &span(), InlineEdges::default());
    });
    let mut cx = LayoutContext::new();
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut cx)[0];
    assert!(cx.take_warnings().iter().any(|w| w.kind == WarningKind::MissingAtomicSize));
    assert_eq!(line.inline_size(), 0.0);
    assert!(!line.is_empty(), "an atomic inline is content even when zero-sized");
}

#[test]
fn out_of_flow_boxes_leave_anchors() {
    let p = para(|b| {
        b.push_text(dom(1), "ab").push_out_of_flow(NodeId(2), OutOfFlowKind::Absolute).push_text(dom(3), "c");
    });
    let line = &all_lines(&p, 100.0, &AtomicSizes::EMPTY, &mut LayoutContext::new())[0];
    let anchor = line.fragments().find_map(|f| if let Fragment::OutOfFlowAnchor(a) = f { Some(a) } else { None }).unwrap();
    assert_eq!((anchor.node, anchor.kind, anchor.inline_position), (NodeId(2), OutOfFlowKind::Absolute, 20.0));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test fragments`
Expected: FAIL to compile ("unresolved imports `shodo::Fragment`, `shodo::InlineBoxFragment`").

- [ ] **Step 3: Implement fragment records**

Create `src/line/fragments.rs`:

```rust
//! Fragment records: the compact per-line table behind the public views.

use std::ops::Range;

use crate::analysis::units::UnitKind;
use crate::geometry::LayoutUnit;
use crate::node::{NodeId, OutOfFlowKind};
use crate::paragraph::{AtomicSize, AtomicSizes, ParagraphData};

#[derive(Clone, Debug)]
pub(crate) struct FragmentRecord {
    pub(crate) kind: RecordKind,
    pub(crate) inline_start: LayoutUnit,
    pub(crate) inline_size: LayoutUnit,
    pub(crate) level: u8,
}

#[derive(Clone, Debug)]
pub(crate) enum RecordKind {
    Glyphs { run: u32, glyphs: Range<u32>, item: u32, text: Range<u32> },
    Atomic { node: NodeId, size: AtomicSize },
    InlineBox { box_index: u32, start_edge: bool, end_edge: bool, parent: Option<u32> },
    Anchor { node: NodeId, kind: OutOfFlowKind },
}

/// Builds the records of one line in logical order.
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let mut out: Vec<FragmentRecord> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut pos = origin;
    let level_at_start = data.units.get(units.start).map_or(data.base_level, |u| u.level);

    // Boxes that are still open from the previous line continue here,
    // without their start edge.
    let mut chain = Vec::new();
    let mut parent = data.units.get(units.start).and_then(|u| u.parent_box);
    while let Some(b) = parent {
        chain.push(b);
        parent = data.boxes[b as usize].parent;
    }
    for &box_index in chain.iter().rev() {
        let parent = open.last().map(|&r| r as u32);
        out.push(FragmentRecord {
            kind: RecordKind::InlineBox { box_index, start_edge: false, end_edge: false, parent },
            inline_start: pos,
            inline_size: LayoutUnit::ZERO,
            level: level_at_start,
        });
        open.push(out.len() - 1);
    }

    for (k, i) in units.enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        match &unit.kind {
            UnitKind::Open { box_index } => {
                let parent = open.last().map(|&r| r as u32);
                out.push(FragmentRecord {
                    kind: RecordKind::InlineBox { box_index: *box_index, start_edge: true, end_edge: false, parent },
                    inline_start: pos,
                    inline_size: LayoutUnit::ZERO,
                    level: unit.level,
                });
                open.push(out.len() - 1);
                pos = pos + w;
            }
            UnitKind::Close { .. } => {
                pos = pos + w;
                if let Some(r) = open.pop() {
                    out[r].inline_size = pos - out[r].inline_start;
                    if let RecordKind::InlineBox { end_edge, .. } = &mut out[r].kind {
                        *end_edge = true;
                    }
                }
            }
            UnitKind::Cluster { run, glyphs, .. } => {
                if let Some(last) = out.last_mut()
                    && last.level == unit.level
                    && let RecordKind::Glyphs { run: r, glyphs: g, item, text } = &mut last.kind
                    && *r == *run
                    && *item == unit.item
                    && g.end == glyphs.start
                {
                    g.end = glyphs.end;
                    text.end = unit.text.end;
                    last.inline_size = last.inline_size + w;
                } else {
                    out.push(FragmentRecord {
                        kind: RecordKind::Glyphs { run: *run, glyphs: glyphs.clone(), item: unit.item, text: unit.text.clone() },
                        inline_start: pos,
                        inline_size: w,
                        level: unit.level,
                    });
                }
                pos = pos + w;
            }
            UnitKind::Atomic { node } => {
                let size = atomics.get(*node).copied().unwrap_or_default();
                out.push(FragmentRecord {
                    kind: RecordKind::Atomic { node: *node, size },
                    inline_start: pos,
                    inline_size: w,
                    level: unit.level,
                });
                pos = pos + w;
            }
            UnitKind::Float { node, .. } | UnitKind::Absolute { node } => {
                let kind = if matches!(unit.kind, UnitKind::Float { .. }) { OutOfFlowKind::Float } else { OutOfFlowKind::Absolute };
                out.push(FragmentRecord {
                    kind: RecordKind::Anchor { node: *node, kind },
                    inline_start: pos,
                    inline_size: LayoutUnit::ZERO,
                    level: unit.level,
                });
            }
            UnitKind::Tab => pos = pos + w,
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => {}
        }
    }
    // Boxes that continue on the next line end here without their end edge.
    for r in open {
        out[r].inline_size = pos - out[r].inline_start;
    }
    out
}
```

In `src/line/mod.rs`, add `pub(crate) mod fragments;` at the top, and pass the sizes to `Line::new`:

```rust
        let line = Line::new(self, token, scan, origin, constraint.block_offset, atomics, &mut sat);
```

- [ ] **Step 4: Implement the views**

In `src/output.rs`:

1. Add imports: `use peniko::FontData;`, `use crate::font::FontId;`, `use crate::geometry::LogicalRect;`, `use crate::line::fragments::{self, FragmentRecord, RecordKind};`, `use crate::node::OutOfFlowKind;`, `use crate::paragraph::AtomicSizes;`.
2. Add the field `pub(crate) fragments: Vec<FragmentRecord>,` to `Line`.
3. Change `Line::new` to take `atomics: &AtomicSizes` after `block_offset`, and build the records before constructing the struct:

```rust
        let origin_units = token.unit as usize..scan.end;
        let records = fragments::build(data, origin_units, &scan.widths, origin, atomics);
```

   then set `fragments: records,` in the struct literal.
4. Append:

```rust
/// A positioned piece of a line.
#[derive(Clone, Copy, Debug)]
pub enum Fragment<'a> {
    GlyphRun(GlyphRunView<'a>),
    Atomic(AtomicFragment),
    InlineBox(InlineBoxFragment),
    OutOfFlowAnchor(AnchorFragment),
}

/// The part of an inline box on one line. With `box-decoration-break:
/// slice`, only the first fragment has the start edge and only the last has
/// the end edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineBoxFragment {
    pub node: NodeId,
    /// Border box. Block sides cover the content area plus block padding and
    /// border, which do not affect the line height (CSS 2.1 §10.8.1).
    pub rect: LogicalRect,
    pub content_rect: LogicalRect,
    pub has_start_edge: bool,
    pub has_end_edge: bool,
    /// Index of the parent inline box fragment in [`Line::fragments`].
    pub parent: Option<usize>,
    /// Primary font of the box, for text-decoration metrics.
    pub font: FontId,
    pub font_size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtomicFragment {
    pub node: NodeId,
    pub margin_rect: LogicalRect,
    pub border_rect: LogicalRect,
    /// Baseline position from the top of the line box.
    pub baseline: f32,
}

/// Static position of an out-of-flow box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorFragment {
    pub node: NodeId,
    pub kind: OutOfFlowKind,
    pub inline_position: f32,
}

/// A run of glyphs from one font, one element and one bidi level.
#[derive(Clone, Copy, Debug)]
pub struct GlyphRunView<'a> {
    line: &'a Line,
    record: &'a FragmentRecord,
    run: u32,
    glyphs: (u32, u32),
    item: u32,
    text: (u32, u32),
}

/// One positioned glyph. `inline_position` is the glyph origin from the
/// container's content edge; `block_offset` is relative to the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub inline_position: f32,
    pub block_offset: f32,
    pub advance: f32,
    pub cluster: u32,
}

impl<'a> GlyphRunView<'a> {
    fn data(&self) -> &'a ParagraphData {
        &self.line.data
    }

    pub fn node(&self) -> Option<NodeId> {
        self.data().items[self.item as usize].node
    }

    pub fn font(&self) -> FontId {
        self.data().runs[self.run as usize].font
    }

    pub fn font_size(&self) -> f32 {
        self.data().runs[self.run as usize].font_size
    }

    pub fn font_data(&self) -> Option<FontData> {
        self.data().fonts.font_data(self.font())
    }

    pub fn bidi_level(&self) -> u8 {
        self.record.level
    }

    pub fn text_range(&self) -> Range<usize> {
        self.text.0 as usize..self.text.1 as usize
    }

    pub fn inline_start(&self) -> f32 {
        self.record.inline_start.to_f32()
    }

    pub fn inline_size(&self) -> f32 {
        self.record.inline_size.to_f32()
    }

    /// Alphabetic baseline from the top of the line box.
    pub fn baseline(&self) -> f32 {
        self.line.baseline.to_f32()
    }

    pub fn glyphs(&self) -> Glyphs<'a> {
        Glyphs { view: *self, next: self.glyphs.0, end: self.glyphs.1 }
    }

    fn glyph(&self, g: u32) -> Glyph {
        let store = &self.data().glyphs;
        let gi = g as usize;
        let rel = store.pen[gi] - store.pen[self.glyphs.0 as usize];
        let advance = store.advance[gi];
        // Odd bidi levels run right to left within the run (UAX #9 L2).
        let pen = if self.record.level % 2 == 1 { self.record.inline_size - rel - advance } else { rel };
        let position = self.record.inline_start + pen + store.offset_inline[gi];
        Glyph {
            id: store.id[gi],
            inline_position: position.to_f32(),
            block_offset: store.offset_block[gi].to_f32(),
            advance: advance.to_f32(),
            cluster: store.cluster[gi],
        }
    }
}

/// Iterator over the glyphs of a run, with random access through `get`.
#[derive(Clone, Debug)]
pub struct Glyphs<'a> {
    view: GlyphRunView<'a>,
    next: u32,
    end: u32,
}

impl Glyphs<'_> {
    /// The `index`-th glyph of the run, independent of iteration.
    pub fn get(&self, index: usize) -> Option<Glyph> {
        let g = self.view.glyphs.0 as usize + index;
        (g < self.view.glyphs.1 as usize).then(|| self.view.glyph(g as u32))
    }
}

impl Iterator for Glyphs<'_> {
    type Item = Glyph;

    fn next(&mut self) -> Option<Glyph> {
        (self.next < self.end).then(|| {
            self.next += 1;
            self.view.glyph(self.next - 1)
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = (self.end - self.next) as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for Glyphs<'_> {}

impl Line {
    /// Fragments in visual order.
    pub fn fragments(&self) -> impl ExactSizeIterator<Item = Fragment<'_>> + '_ {
        (0..self.fragments.len()).map(move |i| self.view(i))
    }

    pub fn fragment(&self, index: usize) -> Option<Fragment<'_>> {
        (index < self.fragments.len()).then(|| self.view(index))
    }

    /// Font data of any face used by the paragraph; works without the
    /// `FontCollection`, which the line keeps alive.
    pub fn font_data(&self, id: FontId) -> Option<FontData> {
        self.data.fonts.font_data(id)
    }

    /// True when the line has no glyphs and no atomic inlines.
    pub fn is_empty(&self) -> bool {
        !self.fragments.iter().any(|r| matches!(r.kind, RecordKind::Glyphs { .. } | RecordKind::Atomic { .. }))
    }

    fn view(&self, index: usize) -> Fragment<'_> {
        let record = &self.fragments[index];
        let rect = |block_start: f32, block_size: f32, start: f32, size: f32| LogicalRect {
            inline_start: start,
            block_start,
            inline_size: size,
            block_size,
        };
        match &record.kind {
            RecordKind::Glyphs { run, glyphs, item, text } => Fragment::GlyphRun(GlyphRunView {
                line: self,
                record,
                run: *run,
                glyphs: (glyphs.start, glyphs.end),
                item: *item,
                text: (text.start, text.end),
            }),
            RecordKind::Atomic { node, size } => {
                let margin_block = size.margins.block_start + size.margins.block_end;
                let height = size.block_size + margin_block;
                let kind = self.data.baselines.iter().find(|(n, _)| n == node).map(|(_, k)| *k);
                // Missing baselines are synthesized from the margin box
                // (CSS Inline 3): bottom for alphabetic, middle for central.
                let baseline_from_top = size.baseline.unwrap_or(match kind {
                    Some(BaselineKind::Central) => height / 2.0,
                    _ => height,
                });
                let line_baseline = self.baseline.to_f32();
                let top = line_baseline - baseline_from_top;
                let start = record.inline_start.to_f32();
                let width = record.inline_size.to_f32();
                let m = size.margins;
                Fragment::Atomic(AtomicFragment {
                    node: *node,
                    margin_rect: rect(top, height, start, width),
                    border_rect: rect(
                        top + m.block_start,
                        size.block_size,
                        start + m.inline_start,
                        width - m.inline_start - m.inline_end,
                    ),
                    baseline: line_baseline,
                })
            }
            RecordKind::InlineBox { box_index, start_edge, end_edge, parent } => {
                let info = &self.data.boxes[*box_index as usize];
                let style = &self.data.styles[info.style as usize];
                let font = self.data.fonts.primary_font();
                let m = self.data.fonts.metrics(font, style.font_size);
                let e = info.edges;
                let pick = |on: bool, v: f32| if on { v } else { 0.0 };
                let margin_start = pick(*start_edge, e.margin.inline_start);
                let margin_end = pick(*end_edge, e.margin.inline_end);
                let inner_start = pick(*start_edge, e.border.inline_start + e.padding.inline_start);
                let inner_end = pick(*end_edge, e.border.inline_end + e.padding.inline_end);
                let border_start = record.inline_start.to_f32() + margin_start;
                let border_size = record.inline_size.to_f32() - margin_start - margin_end;
                let content_top = self.baseline.to_f32() - m.ascent;
                let content_height = m.ascent + m.descent;
                let above = e.padding.block_start + e.border.block_start;
                let below = e.padding.block_end + e.border.block_end;
                Fragment::InlineBox(InlineBoxFragment {
                    node: info.node,
                    rect: rect(content_top - above, content_height + above + below, border_start, border_size),
                    content_rect: rect(
                        content_top,
                        content_height,
                        border_start + inner_start,
                        border_size - inner_start - inner_end,
                    ),
                    has_start_edge: *start_edge,
                    has_end_edge: *end_edge,
                    parent: parent.map(|p| p as usize),
                    font,
                    font_size: style.font_size,
                })
            }
            RecordKind::Anchor { node, kind } => Fragment::OutOfFlowAnchor(AnchorFragment {
                node: *node,
                kind: *kind,
                inline_position: record.inline_start.to_f32(),
            }),
        }
    }
}
```

Update the re-exports in `src/lib.rs`:

```rust
pub use output::{
    AnchorFragment, AtomicFragment, BreakReason, Fragment, Glyph, GlyphRunView, Glyphs, InlineBoxFragment, Line,
};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test fragments && cargo test`
Expected: PASS (7 tests in `tests/fragments.rs`; all earlier tests still green).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src tests/fragments.rs
git commit -m "Add line fragments: glyph runs, inline boxes, atomics and anchors

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 13: Bidi reordering and split inline boxes

Inline positions are measured from the inline-start edge, so in a right-to-left paragraph they grow leftward. A glyph run is drawn in reverse only when its bidi level's parity differs from the paragraph's base level. An inline box can end up as several visual pieces on one line; each piece is its own `InlineBoxFragment` (CSS Writing Modes 4 §2.4.1).

**Files:**
- Modify: `src/line/fragments.rs`
- Modify: `src/output.rs`
- Test: `tests/bidi.rs`

**Interfaces:**
- Consumes: everything from Task 12; `unicode_bidi::{BidiInfo, Level}`.
- Produces: `fragments::build` keeps its signature but now returns records in visual order; `RecordKind::InlineBox` gains `reversed: bool` (the box's level parity differs from the base level, so its start edge is on the inline-end side). `build_logical` (the Task 12 body, renamed) is used when every unit in the line has the base level.

- [ ] **Step 1: Write the failing tests**

Create `tests/bidi.rs`:

```rust
use shodo::font::FontCollection;
use shodo::geometry::Direction;
use shodo::limits::Limits;
use shodo::node::{InlineEdges, NodeId, TextSource};
use shodo::style::{InlineStyle, LineOptions, ParagraphStyle};
use shodo::{AtomicSizes, Fragment, GlyphRunView, InlineBoxFragment, LayoutContext, Line, LineConstraint, LineResult, ParagraphBuilder};

fn style(direction: Direction) -> ParagraphStyle {
    ParagraphStyle {
        direction,
        root: InlineStyle { font_size: 10.0, ..InlineStyle::default() },
        ..ParagraphStyle::default()
    }
}

fn dom(node: u64) -> TextSource {
    TextSource::Dom { node: NodeId(node), offset: 0 }
}

fn one_line(direction: Direction, build: impl FnOnce(&mut ParagraphBuilder)) -> Line {
    let mut b = ParagraphBuilder::new(&style(direction), &Limits::default());
    build(&mut b);
    let p = b.build(&mut LayoutContext::new(), &FontCollection::new(&Limits::default())).unwrap();
    let r = p.next_line(&mut LayoutContext::new(), p.start_token(), &LineOptions::default(), &LineConstraint::new(1000.0), &AtomicSizes::EMPTY);
    let LineResult::Line(line) = r else { panic!("{r:?}") };
    line
}

fn runs(line: &Line) -> Vec<GlyphRunView<'_>> {
    line.fragments().filter_map(|f| if let Fragment::GlyphRun(r) = f { Some(r) } else { None }).collect()
}

fn positions(run: &GlyphRunView<'_>) -> Vec<f32> {
    run.glyphs().map(|g| g.inline_position).collect()
}

#[test]
fn right_to_left_runs_reverse_inside_a_left_to_right_paragraph() {
    let line = one_line(Direction::Ltr, |b| {
        b.push_text(dom(1), "abc \u{5D0}\u{5D1}\u{5D2}");
    });
    let runs = runs(&line);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].bidi_level(), 1);
    assert_eq!(runs[1].inline_start(), 40.0);
    // Logical order alef, bet, gimel is drawn right to left.
    assert_eq!(positions(&runs[1]), [60.0, 50.0, 40.0]);
}

#[test]
fn right_to_left_paragraphs_measure_from_the_right() {
    let line = one_line(Direction::Rtl, |b| {
        b.push_text(dom(1), "\u{5D0}\u{5D1} ab");
    });
    let runs = runs(&line);
    assert_eq!(runs.len(), 2);
    // The Hebrew run comes first from the inline-start (right) edge and
    // needs no reversal; the embedded Latin run does.
    assert_eq!((runs[0].inline_start(), positions(&runs[0])), (0.0, vec![0.0, 10.0, 20.0]));
    assert_eq!((runs[1].inline_start(), positions(&runs[1])), (30.0, vec![40.0, 30.0]));
}

#[test]
fn an_inline_box_split_by_bidi_becomes_two_fragments() {
    let line = one_line(Direction::Ltr, |b| {
        b.open_inline(NodeId(1), &InlineStyle { font_size: 10.0, ..InlineStyle::default() }, InlineEdges::default())
            .push_text(dom(2), "ab \u{5D0}\u{5D1}")
            .close_inline()
            .push_text(dom(3), " \u{5D2}\u{5D3}");
    });
    let boxes: Vec<InlineBoxFragment> =
        line.fragments().filter_map(|f| if let Fragment::InlineBox(b) = f { Some(b) } else { None }).collect();
    assert_eq!(boxes.len(), 2);
    assert!(boxes.iter().all(|b| b.node == NodeId(1)));
    // Visual order: [ab ] [ gimel-dalet reversed] [alef-bet reversed].
    assert_eq!((boxes[0].rect.inline_start, boxes[0].rect.inline_size), (0.0, 30.0));
    assert_eq!((boxes[0].has_start_edge, boxes[0].has_end_edge), (true, false));
    assert_eq!((boxes[1].rect.inline_start, boxes[1].rect.inline_size), (60.0, 20.0));
    assert_eq!((boxes[1].has_start_edge, boxes[1].has_end_edge), (false, true));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test bidi`
Expected: 1 passed, 2 failed.
- `right_to_left_runs_reverse_inside_a_left_to_right_paragraph` already passes: Task 12 reverses odd-level runs. It stays as a guard for the parity change in Step 4.
- `right_to_left_paragraphs_measure_from_the_right` fails: Task 12 reverses the Hebrew run (level 1) instead of the Latin run (level 2).
- `an_inline_box_split_by_bidi_becomes_two_fragments` fails: Task 12 produces one inline-box fragment.

- [ ] **Step 3: Implement visual reordering**

In `src/line/fragments.rs`:

1. Add `reversed: bool` to `RecordKind::InlineBox` and set `reversed: false` in the two places `build` creates inline-box records.
2. Rename the existing `build` function to `build_logical` (same parameters).
3. Add `use unicode_bidi::{BidiInfo, Level};` and append:

```rust
/// Builds the records of one line in visual order (UAX #9 L2).
pub(crate) fn build(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    if data.units[units.clone()].iter().all(|u| u.level == base) {
        return build_logical(data, units, widths, origin, atomics);
    }
    build_bidi(data, units, widths, origin, atomics)
}

/// A reorderable piece of a line: a glyph run segment, an atomic, an
/// anchor, a tab, or an inline box edge.
struct Piece {
    record: Option<FragmentRecord>,
    width: LayoutUnit,
    level: u8,
    /// Innermost inline box the piece belongs to (the box itself for edges).
    owner: Option<u32>,
    /// `(box, is_start)` for inline box edges.
    edge: Option<(u32, bool)>,
}

fn inside(data: &ParagraphData, mut b: Option<u32>, target: u32) -> bool {
    while let Some(x) = b {
        if x == target {
            return true;
        }
        b = data.boxes[x as usize].parent;
    }
    false
}

fn build_bidi(
    data: &ParagraphData,
    units: Range<usize>,
    widths: &[LayoutUnit],
    origin: LayoutUnit,
    atomics: &AtomicSizes,
) -> Vec<FragmentRecord> {
    let base = data.base_level;
    let mut pieces: Vec<Piece> = Vec::new();
    for (k, i) in units.clone().enumerate() {
        let unit = &data.units[i];
        let w = widths[k];
        let record = |kind: RecordKind| FragmentRecord { kind, inline_start: LayoutUnit::ZERO, inline_size: w, level: unit.level };
        let piece = match &unit.kind {
            UnitKind::Open { box_index } => {
                Piece { record: None, width: w, level: unit.level, owner: Some(*box_index), edge: Some((*box_index, true)) }
            }
            UnitKind::Close { box_index } => {
                Piece { record: None, width: w, level: unit.level, owner: Some(*box_index), edge: Some((*box_index, false)) }
            }
            UnitKind::Cluster { run, glyphs, .. } => {
                if let Some(last) = pieces.last_mut()
                    && last.level == unit.level
                    && last.owner == unit.parent_box
                    && let Some(FragmentRecord { kind: RecordKind::Glyphs { run: r, glyphs: g, item, text }, inline_size, .. }) =
                        &mut last.record
                    && *r == *run
                    && *item == unit.item
                    && g.end == glyphs.start
                {
                    g.end = glyphs.end;
                    text.end = unit.text.end;
                    *inline_size = *inline_size + w;
                    last.width = last.width + w;
                    continue;
                }
                let kind = RecordKind::Glyphs { run: *run, glyphs: glyphs.clone(), item: unit.item, text: unit.text.clone() };
                Piece { record: Some(record(kind)), width: w, level: unit.level, owner: unit.parent_box, edge: None }
            }
            UnitKind::Atomic { node } => {
                let size = atomics.get(*node).copied().unwrap_or_default();
                let kind = RecordKind::Atomic { node: *node, size };
                Piece { record: Some(record(kind)), width: w, level: unit.level, owner: unit.parent_box, edge: None }
            }
            UnitKind::Float { node, .. } | UnitKind::Absolute { node } => {
                let kind = if matches!(unit.kind, UnitKind::Float { .. }) { OutOfFlowKind::Float } else { OutOfFlowKind::Absolute };
                let mut r = record(RecordKind::Anchor { node: *node, kind });
                r.inline_size = LayoutUnit::ZERO;
                Piece { record: Some(r), width: LayoutUnit::ZERO, level: unit.level, owner: unit.parent_box, edge: None }
            }
            UnitKind::Tab => Piece { record: None, width: w, level: unit.level, owner: unit.parent_box, edge: None },
            UnitKind::ForcedBreak | UnitKind::BidiControl | UnitKind::BlockInInline { .. } => continue,
        };
        pieces.push(piece);
    }

    // Visual order, left to right; from the inline-start edge that is the
    // reverse order when the paragraph is right-to-left.
    let levels: Vec<Level> = pieces.iter().map(|p| Level::new(p.level).unwrap_or_else(|_| Level::ltr())).collect();
    let mut order = BidiInfo::reorder_visual(&levels);
    if base % 2 == 1 {
        order.reverse();
    }
    let mut starts = vec![LayoutUnit::ZERO; pieces.len()];
    let mut pos = origin;
    for &p in &order {
        starts[p] = pos;
        pos = pos + pieces[p].width;
    }

    // Inline boxes: one fragment per visually contiguous group of members.
    let mut box_ids: Vec<u32> = Vec::new();
    for piece in &pieces {
        let mut b = piece.owner;
        while let Some(x) = b {
            if !box_ids.contains(&x) {
                box_ids.push(x);
            }
            b = data.boxes[x as usize].parent;
        }
    }
    box_ids.sort_unstable();
    let mut boxes: Vec<(FragmentRecord, u32)> = Vec::new();
    for &b in &box_ids {
        let open_level = pieces.iter().find(|p| p.edge == Some((b, true))).map(|p| p.level);
        let mut group: Option<(LayoutUnit, LayoutUnit, bool, bool, u8)> = None;
        let flush = |group: &mut Option<(LayoutUnit, LayoutUnit, bool, bool, u8)>, boxes: &mut Vec<(FragmentRecord, u32)>| {
            if let Some((start, size, start_edge, end_edge, level)) = group.take() {
                let reversed = open_level.unwrap_or(level) % 2 != base % 2;
                let kind = RecordKind::InlineBox { box_index: b, start_edge, end_edge, parent: None, reversed };
                boxes.push((FragmentRecord { kind, inline_start: start, inline_size: size, level }, b));
            }
        };
        for &p in &order {
            let piece = &pieces[p];
            if inside(data, piece.owner, b) {
                let g = group.get_or_insert((starts[p], LayoutUnit::ZERO, false, false, piece.level));
                g.1 = g.1 + piece.width;
                g.2 |= piece.edge == Some((b, true));
                g.3 |= piece.edge == Some((b, false));
            } else {
                flush(&mut group, &mut boxes);
            }
        }
        flush(&mut group, &mut boxes);
    }

    // Output: boxes and content sorted by position; a box precedes the
    // content it starts with, and an outer box precedes an inner one.
    let depth = |b: u32| {
        let mut d = 0;
        let mut x = data.boxes[b as usize].parent;
        while let Some(p) = x {
            d += 1;
            x = data.boxes[p as usize].parent;
        }
        d
    };
    let mut out: Vec<(FragmentRecord, Option<u32>, u32)> =
        boxes.into_iter().map(|(r, b)| (r, Some(b), depth(b))).collect();
    for &p in &order {
        if let Some(mut r) = pieces[p].record.clone() {
            r.inline_start = starts[p];
            out.push((r, None, u32::MAX));
        }
    }
    out.sort_by_key(|(r, _, d)| (r.inline_start, *d));

    // Parent links: the enclosing box fragment of the parent box.
    let spans: Vec<(Option<u32>, LayoutUnit, LayoutUnit)> =
        out.iter().map(|(r, b, _)| (*b, r.inline_start, r.inline_start + r.inline_size)).collect();
    for (r, b, _) in &mut out {
        let Some(b) = *b else { continue };
        let Some(parent_box) = data.boxes[b as usize].parent else { continue };
        let (start, end) = (r.inline_start, r.inline_start + r.inline_size);
        let parent = spans.iter().position(|(pb, ps, pe)| *pb == Some(parent_box) && *ps <= start && end <= *pe);
        if let RecordKind::InlineBox { parent: slot, .. } = &mut r.kind {
            *slot = parent.map(|p| p as u32);
        }
    }
    out.into_iter().map(|(r, _, _)| r).collect()
}
```

- [ ] **Step 4: Update the views**

In `src/output.rs`:

1. In `GlyphRunView::glyph`, reverse only when the run's parity differs from the base level:

```rust
        let reversed = self.record.level % 2 != self.line.data.base_level % 2;
        let pen = if reversed { self.record.inline_size - rel - advance } else { rel };
```

   (replacing the `// Odd bidi levels ...` line and the `let pen = ...` line).
2. In `Line::view`, match the new field and swap which side gets the start edge when the box is reversed:

```rust
            RecordKind::InlineBox { box_index, start_edge, end_edge, parent, reversed } => {
```

   and replace the four `pick(...)` lines plus the two `border_*` lines with:

```rust
                let margin_start = pick(*start_edge, e.margin.inline_start);
                let margin_end = pick(*end_edge, e.margin.inline_end);
                let inner_start = pick(*start_edge, e.border.inline_start + e.padding.inline_start);
                let inner_end = pick(*end_edge, e.border.inline_end + e.padding.inline_end);
                // A box whose direction opposes the paragraph's has its start
                // edge on the inline-end side.
                let (lead_margin, trail_margin, lead_inner, trail_inner) = if *reversed {
                    (margin_end, margin_start, inner_end, inner_start)
                } else {
                    (margin_start, margin_end, inner_start, inner_end)
                };
                let border_start = record.inline_start.to_f32() + lead_margin;
                let border_size = record.inline_size.to_f32() - lead_margin - trail_margin;
```

   and use `lead_inner` / `trail_inner` in place of `inner_start` / `inner_end` when computing `content_rect`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --test bidi && cargo test`
Expected: PASS (3 tests in `tests/bidi.rs`; all earlier tests unchanged because all-LTR lines still take `build_logical`).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add src tests/bidi.rs
git commit -m "Reorder line fragments for bidi and split inline boxes into pieces

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

### Task 14: CI with a wasm32 build

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the whole crate.
- Produces: a CI workflow running fmt, clippy, tests, and a `wasm32-unknown-unknown` build with default features.

- [ ] **Step 1: Check the wasm build locally**

Run:

```bash
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown
```

Expected: the build succeeds. If a dependency fails on wasm32, stop and report which crate and error — do not add feature flags to work around it without review.

- [ ] **Step 2: Add the workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --check
      - run: cargo clippy --all-targets
      - run: cargo test

  msrv:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.89.0
      - run: cargo test

  wasm:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
      - run: cargo build --target wasm32-unknown-unknown
```

- [ ] **Step 3: Run the full local check**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --target wasm32-unknown-unknown`
Expected: all succeed.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "Add CI: fmt, clippy, tests, MSRV and wasm32 build

Claude-Session: https://claude.ai/code/session_019U3hQq5cT6cQJi2RsosYWA"
```

---

## Left for plan S0-B

These parts of spec §6.5 are intentionally not in this plan; S0-B is written after this plan is executed, against the code as it then exists.

- Line box block size from inline boxes and atomics (currently the strut only), `BlockSizeExceeded` including acceptance at the top of a fragmentainer.
- `BlockInInline` empty lines (`is_empty()` lines with zero block size that consume an item).
- `text-align` / `text-align-last` / justification with the per-line position array; `break_all`.
- The float protocol: `FloatEncountered`, `floats_placed_through`, look-ahead over unbreakable ranges, `displaced_floats` and withdrawal, the partial-line cache, and the three counterexample tests (tab + span, two floats, float inside a word).
- `intrinsic_sizes` with `FloatIntrinsic` (side and clear), `lines()`, `plan_breaks` and `BreakPlan` mismatch detection.
- `next_line` input normalization for every `LineConstraint` / `AtomicSizes` field, property tests (token progress, mapping round trip, no panics), and removal of the crate-level `allow(dead_code, unused_imports)`.
