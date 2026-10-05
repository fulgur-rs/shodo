# Quirks-mode per-line inline strut Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `ParagraphStyle::line_height_quirk` so that, per line, an inline box (root included) contributes its strut only when it has direct text, a border/padding inline edge on that line, a lone `<br>`, or (root only) ruby on the line — matching Chromium 152 quirks mode — in both retained line metrics and the ruby `MetricIndex` estimate.

**Architecture:** A new `line/quirk.rs` owns one unit-based predicate (`quirk::Struts::line`) and the width-free trailing-trim boundary (`whitespace::trailing_start`). Retained `line/metrics.rs::measure` consults it to skip extents of non-contributing boxes while still computing shifts. `MetricIndex` (ruby fit probes) gets a separate, quirk-only segment tree with a compact leaf whose union over a range equals the same predicate, so probes stay O(log n) and agree with retained lines.

**Tech Stack:** Rust (workspace MSRV 1.89.0), shodo crate. Integration tests in `crates/shodo/tests/line_height_quirk.rs` (built-in stub font, `system_fonts: false`); index parity tests in `crates/shodo/src/ruby/tests/measure.rs` style (CJK fixture font).

**Spec:** beads issue `shodo-e1h` design + acceptance fields (`bd show shodo-e1h`). Follow-ups: `shodo-9kt` (Blink pending vertical-align credit), `shodo-qu8` (list-item root strut).

## Global Constraints

- Source comments and docs in English.
- `line_height_quirk == false` must change nothing: same output, no new allocation or tree in `MetricIndex::new`, no extra pass in `measure` (gate everything on `data.style.line_height_quirk`).
- Edge predicate is **border + padding only**: `border.inline_start + padding.inline_start != 0` on the Open unit, `border.inline_end + padding.inline_end != 0` on the Close unit. Never use `InlineEdges::inline_start_total()/inline_end_total()` (they include margin).
- Edges are judged on the **unit**: the Open/Close unit must be inside the line's unit range. Cloned (`box-decoration-break: clone`) continuation fragments do not contribute (Chromium-conformant; diverges from CSS Inline 3 §5.3 wording — document this).
- Direct text = `UnitKind::Cluster` or `UnitKind::Tab` whose `parent_box` is the box (`None` = root), excluding units that are trimmed at the line end (`quirk::trims(data, i) && i >= trailing_start`). Hidden soft hyphens, ZWSP, LRM, nbsp, preserved spaces/tabs count.
- `<br>` rule: let `k` be the line's `ForcedBreak` unit and `p` its `parent_box`. `p` contributes iff no *content unit* lies in `[lo, k)` where `lo = units.start` for the root and `max(units.start, open(p) + 1)` otherwise. Content unit = direct-text unit (any owner), `Atomic`, Open/Close with nonzero border+padding edge.
- Ruby rule: if any ruby container's `units` intersects the line's units, the root contributes.
- Suppressed boxes keep their computed shift (`box_shift`) so descendants' vertical-align is unchanged.
- `ProfileResolver::record` is shared with ruby probes: do not change it.
- `size_of::<scalar::Summary>() <= 64` (pinned by `spacing_summary.rs`) must still hold: do not add fields to `Summary`.
- No O(k) scans over trailing runs in `MetricIndex::select` (transparent clusters can be unbounded); precompute in `new()`.
- Do not edit `CHANGELOG.md` by hand (release-plz).
- Raikiri changes are local verification only; nothing is committed to raikiri-spike.
- Commit messages: conventional (`feat:`, `fix:`, `test:`, `docs:`), no attribution lines.

## Review Focus

1. Zero-width trailing collapsible space (`x<span lh40 font-size:0> </span>`, Chromium W = 20): `scan.hang_start` is reset to `scan.end` by `whitespace::finalize` when the hanging advance is zero; the quirk must use `trailing_start`, not `scan.hang_start` (Task 2 test `trailing_start_ignores_zero_width_finalize_reset`, Task 3 test `zero_width_trailing_space_is_not_text`).
2. A top/bottom-aligned span with no contributing member must not panic in `measure` (`deltas[&g]`) nor add height (Task 3 test `empty_top_aligned_span_adds_no_height`; Task 4 parity sweep includes it).
3. Index/retained parity with reshape windows (hyphenation/overlay runs replace leaves of `tree` only): quirk struts must survive (Task 4 test `quirk_index_matches_retained_with_hyphenated_edges`).
4. Nested ruby annotations must also receive the flag (Task 1 test `annotation_paragraphs_inherit_line_height_quirk`, nested case).
5. Flag-off is untouched: existing suite passes unchanged and `MetricIndex::new` allocates no quirk data (Task 4 test `quirk_tree_is_absent_without_the_flag`).

---

### Task 1: API flag, annotation propagation, docs

**Files:**
- Modify: `crates/shodo/src/style.rs:579-597` (`ParagraphStyle`)
- Modify: `crates/shodo/src/ruby/prepare.rs` (next to `builder.style.writing_mode = data.style.writing_mode;`, ~line 263)
- Modify: `docs/guides/horizontal-layout-contracts.md` (new section "Quirks-mode line height")
- Test: `crates/shodo/src/ruby/tests/input.rs` (or the ruby test module that already builds `p.data.ruby.containers[..].lanes[..].paragraph`)

**Interfaces:**
- Produces: `pub line_height_quirk: bool` on `ParagraphStyle` (read as `data.style.line_height_quirk` inside the crate).

- [ ] **Step 1: Write the failing test**

Add to the ruby test module that has `fn ruby(...)`, `fn base(...)`, `fn fonts()` helpers (e.g. `crates/shodo/src/ruby/tests/measure.rs`):

```rust
#[test]
fn annotation_paragraphs_inherit_line_height_quirk() {
    for quirk in [false, true] {
        // Nested: the annotation itself carries ruby.
        let mut inner = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(12.0),
                ..Default::default()
            },
            &Limits::default(),
        );
        inner.push_ruby(NodeId(30), &style(12.0), ruby(base("に"), "ni"));
        let mut outer = ruby(base("日"), "x");
        outer.levels[0].annotations[0].content = RubyContent::from_builder(inner);
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                root: style(24.0),
                line_height_quirk: quirk,
                ..Default::default()
            },
            &Limits::default(),
        );
        b.push_ruby(NodeId(8), &style(24.0), outer);
        let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
        let lane = &p.data.ruby.containers[0].lanes[0].paragraph.data;
        assert_eq!(lane.style.line_height_quirk, quirk);
        let nested = &lane.ruby.containers[0].lanes[0].paragraph.data;
        assert_eq!(nested.style.line_height_quirk, quirk);
    }
}
```

If `outer.levels[0].annotations[0].content` is not assignable in this module, construct the `Ruby` with `Ruby::new` directly as `fn ruby` does, passing `RubyContent::from_builder(inner)` as the annotation content.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p shodo annotation_paragraphs_inherit_line_height_quirk`
Expected: compile error `struct ParagraphStyle has no field named line_height_quirk`.

- [ ] **Step 3: Add the field**

In `crates/shodo/src/style.rs`, inside `pub struct ParagraphStyle` after `first_line`:

```rust
    /// The line height calculation quirk of quirks and limited-quirks mode
    /// (Quirks Mode Standard §3.3-3.4; CSS Inline 3 §5.3). When set, on each
    /// line an inline box, the root inline box included, contributes its
    /// strut only if that line holds text it directly contains, its own
    /// inline-start or inline-end border or padding, a forced break with
    /// nothing else in the box on that line, or (root only) ruby. Its
    /// descendants still align to its metrics. As in Chromium, a cloned
    /// `box-decoration-break` edge repeated on a continuation line does not
    /// count, and margins never count.
    pub line_height_quirk: bool,
```

`ParagraphStyle` derives `Default`, so no other initializer changes are needed. Fix any exhaustive struct literals the compiler reports (`..Default::default()` is already used in most).

- [ ] **Step 4: Propagate into annotation builders**

In `crates/shodo/src/ruby/prepare.rs`, immediately before the `if normalized.style.position == RubyPosition::InterCharacter` block that sets `builder.style.writing_mode`, add:

```rust
                // Annotation boxes are inline content of the same document.
                builder.style.line_height_quirk = data.style.line_height_quirk;
```

Nested ruby inside the annotation is prepared from this builder's own `style`, so the value propagates recursively.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p shodo annotation_paragraphs_inherit_line_height_quirk`
Expected: PASS.

- [ ] **Step 6: Document**

Append to `docs/guides/horizontal-layout-contracts.md`:

```markdown
## Quirks-mode line height

`ParagraphStyle::line_height_quirk` implements the line height calculation
quirk of quirks and limited-quirks documents. It is decided per line and per
inline box, the root inline box included. A box contributes its strut to a
line only when, on that line:

- it directly contains text or preserved white space (collapsible spaces
  removed at the line end do not count; hidden soft hyphens and other
  zero-width characters do);
- its own inline-start border or padding (on the line holding its start) or
  inline-end border or padding (on the line holding its end) is nonzero —
  margins never count;
- it holds a forced break and nothing else of its own content on that line;
- it is the root inline box and the line holds ruby.

A box that does not contribute is ignored only for sizing the line box;
descendants still align to its font metrics. Matching Chromium, a
`box-decoration-break: clone` edge repeated on a continuation line does not
count, although CSS Inline 3 §5.3 speaks of fragments. Known differences from
Chromium: descendants aligned `top`/`bottom` (and empty `text-top` /
`text-bottom` children) do not yet credit their ancestors for the forced-break
rule (shodo-9kt), and list-item lines do not force the root strut
(shodo-qu8).
```

- [ ] **Step 7: Commit**

```bash
git add crates/shodo/src/style.rs crates/shodo/src/ruby/prepare.rs crates/shodo/src/ruby/tests docs/guides/horizontal-layout-contracts.md
git commit -m "feat: add ParagraphStyle::line_height_quirk"
```

---

### Task 2: Shared quirk predicate (`line/quirk.rs`) and width-free trailing start

**Files:**
- Create: `crates/shodo/src/line/quirk.rs`
- Modify: `crates/shodo/src/line/mod.rs` (add `pub(crate) mod quirk;`)
- Modify: `crates/shodo/src/line/whitespace.rs` (add `trailing_start`)
- Modify: `crates/shodo/src/line/fragments.rs:86-101` (`trims_box` calls `quirk::trims`)
- Test: unit tests inside `crates/shodo/src/line/quirk.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `ParagraphStyle::line_height_quirk` (Task 1).
- Produces:
  - `pub(super) fn whitespace::trailing_start(data: &ParagraphData, start: usize, end: usize) -> usize` — the begin index `trailing()` would return, without widths and without `finalize`'s reset. Precisely: the index of the first unit of the maximal backward run scanned by `trailing()`'s loop (the unit after the one that `break`s it), clamped to `start`.
  - `pub(crate) fn quirk::trims(data: &ParagraphData, i: usize) -> bool` — the static half of `fragments::trims_box` (`combine.is_none()`, space cluster or tab, `Collapse | PreserveBreaks`).
  - `pub(crate) fn quirk::start_edge(data: &ParagraphData, b: u32) -> bool` / `end_edge` — border+padding inline edge nonzero.
  - `pub(crate) fn quirk::content(data: &ParagraphData, i: usize, t: usize) -> bool` — content-unit predicate given the trailing start `t`.
  - `pub(crate) struct quirk::Struts { pub(crate) root: bool, pub(crate) boxes: crate::hashing::FastSet<u32> }` with `pub(crate) fn line(data: &ParagraphData, units: Range<usize>) -> Self` and `pub(crate) fn contributes(&self, b: Option<u32>) -> bool`.

- [ ] **Step 1: Write the failing tests**

Create `crates/shodo/src/line/quirk.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontOptions};
    use crate::limits::Limits;
    use crate::node::{InlineEdges, NodeId, Sides, TextSource};
    use crate::style::{InlineStyle, ParagraphStyle, WhiteSpaceCollapse};
    use crate::{LayoutContext, Paragraph, ParagraphBuilder};

    fn build(input: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph {
        let mut b = ParagraphBuilder::new(
            &ParagraphStyle {
                line_height_quirk: true,
                ..Default::default()
            },
            &Limits::default(),
        );
        input(&mut b);
        b.build(
            &mut LayoutContext::new(),
            &FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            ),
        )
        .unwrap()
    }

    fn text(b: &mut ParagraphBuilder, s: &str) {
        b.push_text(TextSource::Generated { node: NodeId(1) }, s);
    }

    /// `trailing()` before `finalize`, recomputed with real zero widths.
    fn reference(data: &crate::paragraph::ParagraphData, start: usize, end: usize) -> usize {
        let widths = vec![crate::geometry::LayoutUnit::ZERO; end - start];
        let mut sat = Default::default();
        crate::line::whitespace::trailing(data, start, end, &widths, &mut sat).0
    }

    #[test]
    fn trailing_start_matches_trailing_on_every_range() {
        let pre = InlineStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..Default::default()
        };
        let padded = InlineEdges {
            padding: Sides {
                inline_end: 1.0,
                ..Sides::default()
            },
            ..InlineEdges::default()
        };
        let p = build(|b| {
            text(b, "a b ");
            b.open_inline(NodeId(2), &pre, padded);
            text(b, "  ");
            b.close_inline();
            text(b, "\u{200e} c ");
            b.push_forced_break(NodeId(3));
            text(b, "d");
        });
        let n = p.data.units.len();
        for start in 0..n {
            for end in start + 1..=n {
                let begin = reference(&p.data, start, end);
                let t = crate::line::whitespace::trailing_start(&p.data, start, end);
                // Every trimmed unit at or after `begin` is at or after `t`
                // and vice versa: the trimmed sets coincide.
                for i in start..end {
                    assert_eq!(
                        i >= begin && trims(&p.data, i),
                        i >= t && trims(&p.data, i),
                        "{start}..{end} unit {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn trailing_start_ignores_zero_width_finalize_reset() {
        let zero = InlineStyle {
            font_size: 0.0,
            ..Default::default()
        };
        let p = build(|b| {
            text(b, "x");
            b.open_inline(NodeId(2), &zero, InlineEdges::default());
            text(b, " ");
            b.close_inline();
        });
        let n = p.data.units.len();
        let space = (0..n)
            .find(|i| matches!(p.data.units[*i].kind, crate::analysis::units::UnitKind::Cluster { space: true, .. }))
            .unwrap();
        assert!(crate::line::whitespace::trailing_start(&p.data, 0, n) <= space);
        let struts = Struts::line(&p.data, 0..n);
        // The zero-size span's only text is a trailing collapsible space.
        assert!(!struts.contributes(Some(0)));
        assert!(struts.contributes(None));
    }

    #[test]
    fn br_credits_its_parent_only_without_other_parent_content() {
        // <span><img><br></span>: the atomic is content of the span.
        let p = build(|b| {
            b.open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
            b.push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default());
            b.push_forced_break(NodeId(4));
            b.close_inline();
        });
        let n = p.data.units.len();
        let s = Struts::line(&p.data, 0..n);
        assert!(!s.contributes(Some(0)) && !s.contributes(None));
        // x<span><br></span>: root text does not stop the span's br.
        let p = build(|b| {
            text(b, "x");
            b.open_inline(NodeId(2), &InlineStyle::default(), InlineEdges::default());
            b.push_forced_break(NodeId(4));
            b.close_inline();
        });
        let n = p.data.units.len();
        let s = Struts::line(&p.data, 0..n);
        assert!(s.contributes(Some(0)) && s.contributes(None));
    }

    #[test]
    fn margins_are_not_quirk_edges() {
        let p = build(|b| {
            b.open_inline(
                NodeId(2),
                &InlineStyle::default(),
                InlineEdges {
                    margin: Sides {
                        inline_start: 1.0,
                        inline_end: 1.0,
                        ..Sides::default()
                    },
                    ..InlineEdges::default()
                },
            );
            b.push_atomic(NodeId(3), &InlineStyle::default(), InlineEdges::default());
            b.close_inline();
        });
        let n = p.data.units.len();
        assert!(!Struts::line(&p.data, 0..n).contributes(Some(0)));
    }
}
```

(Adjust field names `margin`/`padding`/`border` to the actual `InlineEdges` fields in `crates/shodo/src/node.rs`. If `whitespace::trailing` is `pub(super)`, make it `pub(crate)` for the test.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p shodo line::quirk`
Expected: compile errors (`trims`, `Struts`, `trailing_start` not defined).

- [ ] **Step 3: Implement `trailing_start`**

In `crates/shodo/src/line/whitespace.rs`, next to `trailing`:

```rust
/// Start of the backward run `trailing` scans, without widths and without
/// `finalize` resetting it when the hanging advance is zero. Every unit in
/// `start..end` that `trailing` would hang lies at or after it; quirks-mode
/// text presence (`line::quirk`) uses it so zero-width spaces still trim.
pub(crate) fn trailing_start(data: &ParagraphData, start: usize, end: usize) -> usize {
    let mut begin = end;
    let mut blocked = obstructed(data, end);
    for i in (start..end).rev() {
        if let UnitKind::Close { box_index } = data.units[i].kind {
            let e = data.boxes[box_index as usize].edges;
            blocked |= e.padding.inline_end != 0.0 || e.border.inline_end != 0.0;
        } else if !(hangable(data, i) && (!preserved(data, i) || !blocked))
            && !transparent(data, i)
        {
            break;
        }
        begin = i;
    }
    begin
}
```

Note `begin` here is the run start (it may sit on a Close or transparent unit), which differs from `trailing().0` only on units that `trims` rejects, so the trimmed sets coincide (the test asserts exactly that).

- [ ] **Step 4: Implement the predicate module**

Fill `crates/shodo/src/line/quirk.rs` above the tests:

```rust
//! Quirks-mode line height calculation (Quirks Mode Standard §3.3-3.4,
//! CSS Inline 3 §5.3), decided per line from units so that retained line
//! metrics and the ruby metric index share one definition.
use crate::analysis::units::UnitKind;
use crate::paragraph::ParagraphData;
use crate::style::WhiteSpaceCollapse;
use std::ops::Range;

/// A collapsible space or tab that is removed when it ends a line. Whether
/// it actually ends the line is decided by `whitespace::trailing_start`.
pub(crate) fn trims(data: &ParagraphData, i: usize) -> bool {
    let unit = &data.units[i];
    unit.combine.is_none()
        && matches!(
            unit.kind,
            UnitKind::Cluster { space: true, .. } | UnitKind::Tab
        )
        && matches!(
            data.styles[data.items[unit.item as usize].style as usize].white_space_collapse,
            WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
        )
}

/// Inline-start border or padding; margins never keep the strut.
pub(crate) fn start_edge(data: &ParagraphData, b: u32) -> bool {
    let e = data.boxes[b as usize].edges;
    e.border.inline_start + e.padding.inline_start != 0.0
}

pub(crate) fn end_edge(data: &ParagraphData, b: u32) -> bool {
    let e = data.boxes[b as usize].edges;
    e.border.inline_end + e.padding.inline_end != 0.0
}

/// Text the unit's `parent_box` directly contains on a line whose trailing
/// run starts at `t`.
pub(crate) fn text(data: &ParagraphData, i: usize, t: usize) -> bool {
    matches!(
        data.units[i].kind,
        UnitKind::Cluster { .. } | UnitKind::Tab
    ) && !(i >= t && trims(data, i))
}

/// Content that keeps a forced break from contributing its parent's strut.
pub(crate) fn content(data: &ParagraphData, i: usize, t: usize) -> bool {
    match data.units[i].kind {
        UnitKind::Atomic { .. } => true,
        UnitKind::Open { box_index } => start_edge(data, box_index),
        UnitKind::Close { box_index } => end_edge(data, box_index),
        _ => text(data, i, t),
    }
}

/// Inline boxes whose strut contributes to one line.
#[derive(Debug, Default)]
pub(crate) struct Struts {
    pub(crate) root: bool,
    pub(crate) boxes: crate::hashing::FastSet<u32>,
}

impl Struts {
    pub(crate) fn contributes(&self, b: Option<u32>) -> bool {
        b.map_or(self.root, |b| self.boxes.contains(&b))
    }

    fn mark(&mut self, b: Option<u32>) {
        match b {
            Some(b) => {
                self.boxes.insert(b);
            }
            None => self.root = true,
        }
    }

    pub(crate) fn line(data: &ParagraphData, units: Range<usize>) -> Self {
        let mut s = Self::default();
        let t = super::whitespace::trailing_start(data, units.start, units.end);
        let mut forced = None;
        for i in units.clone() {
            let u = &data.units[i];
            match u.kind {
                UnitKind::Open { box_index } if start_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                UnitKind::Close { box_index } if end_edge(data, box_index) => {
                    s.mark(Some(box_index))
                }
                UnitKind::ForcedBreak => forced = Some(i),
                _ if text(data, i, t) => s.mark(u.parent_box),
                _ => {}
            }
        }
        if let Some(k) = forced {
            let p = data.units[k].parent_box;
            let lo = p.map_or(units.start, |b| {
                (units.start..k)
                    .rev()
                    .find(|i| matches!(data.units[*i].kind, UnitKind::Open { box_index } if box_index == b))
                    .map_or(units.start, |open| open + 1)
            });
            if !(lo..k).any(|i| content(data, i, t)) {
                s.mark(p);
            }
        }
        if !s.root && ruby_on_line(data, &units) {
            s.root = true;
        }
        s
    }
}

/// Chromium forces the root strut on a line holding a ruby column.
fn ruby_on_line(data: &ParagraphData, units: &Range<usize>) -> bool {
    let containers = &data.ruby.containers;
    if containers.is_empty() {
        return false;
    }
    let mut found = false;
    let mut through = units.end;
    data.ruby
        .intervals
        .intersecting(containers, units.start, &mut through, |_, _| found = true);
    found
}
```

Check `ContainerIndex::intersecting` semantics (`crates/shodo/src/ruby/index.rs:472`): it visits containers intersecting `start..through`. If it requires a non-empty container list or a budgeted structure, keep the `is_empty` early return.

Then make `fragments::trims_box` delegate:

```rust
fn trims_box(data: &ParagraphData, i: usize, hang_start: usize) -> bool {
    i >= hang_start && super::quirk::trims(data, i)
}
```

and register `pub(crate) mod quirk;` in `crates/shodo/src/line/mod.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p shodo line::quirk && cargo test -p shodo fragments`
Expected: PASS (and existing fragment tests unchanged).

- [ ] **Step 6: Commit**

```bash
git add crates/shodo/src/line/quirk.rs crates/shodo/src/line/mod.rs crates/shodo/src/line/whitespace.rs crates/shodo/src/line/fragments.rs
git commit -m "feat: add the quirks-mode line strut predicate"
```

---

### Task 3: Retained line metrics honor the quirk

**Files:**
- Modify: `crates/shodo/src/line/metrics.rs:323-503` (`measure`)
- Create: `crates/shodo/tests/line_height_quirk.rs`

**Interfaces:**
- Consumes: `quirk::Struts::line`, `Struts::contributes` (Task 2); `ParagraphStyle::line_height_quirk` (Task 1).
- Produces: `measure` behavior only (signature unchanged).

Test geometry: root font-size 10, `line-height: 20px`; spans use font-size 10 and `line-height: Npx`; "img" = atomic 2×2 with `baseline: None` (bottom on the baseline). With equal font sizes all struts share one center, so a line's height is the largest contributing `line-height`, or 2 for an img-only line, matching Chromium px values.

- [ ] **Step 1: Write the failing tests**

Create `crates/shodo/tests/line_height_quirk.rs`:

```rust
mod common;
use common::*;
use shodo::node::{InlineEdges, NodeId, Sides, TextSource};
use shodo::style::{
    BoxDecorationBreak, InlineStyle, LineHeight, LineOptions, ParagraphStyle, VerticalAlign,
    WhiteSpaceCollapse,
};
use shodo::{AtomicSize, AtomicSizes, LayoutContext, LineConstraint, LineResult, ParagraphBuilder};

fn root(quirk: bool) -> ParagraphStyle {
    let mut s = style();
    s.root.line_height = LineHeight::Px(20.0);
    s.line_height_quirk = quirk;
    s
}

fn span(lh: f32) -> InlineStyle {
    InlineStyle {
        font_size: 10.0,
        line_height: LineHeight::Px(lh),
        ..Default::default()
    }
}

fn edges(start: f32, end: f32) -> InlineEdges {
    InlineEdges {
        padding: Sides {
            inline_start: start,
            inline_end: end,
            ..Sides::default()
        },
        ..InlineEdges::default()
    }
}

struct Doc {
    next: u64,
    atomics: AtomicSizes,
}

impl Doc {
    fn new() -> Self {
        Self { next: 100, atomics: AtomicSizes::new() }
    }
    fn img(&mut self, b: &mut ParagraphBuilder, align: VerticalAlign) {
        self.next += 1;
        let id = NodeId(self.next);
        self.atomics.insert(
            id,
            AtomicSize {
                inline_size: 2.0,
                block_size: 2.0,
                ..Default::default()
            },
        );
        b.push_atomic(id, &InlineStyle { vertical_align: align, ..span(20.0) }, InlineEdges::default());
    }
}

fn text(b: &mut ParagraphBuilder, s: &str) {
    b.push_text(TextSource::Generated { node: NodeId(1) }, s);
}

fn heights(quirk: bool, width: f32, input: impl FnOnce(&mut ParagraphBuilder, &mut Doc)) -> Vec<f32> {
    let mut doc = Doc::new();
    let p = build(&root(quirk), |b| input(b, &mut doc));
    let mut cx = LayoutContext::new();
    let mut token = p.start_token();
    let mut out = Vec::new();
    loop {
        match p.next_line(&mut cx, token, &LineOptions::default(), &LineConstraint::new(width), &doc.atomics) {
            LineResult::Line(line) => {
                token = line.break_token();
                out.push(line.block_size());
            }
            LineResult::Done => return out,
            other => panic!("unexpected {other:?}"),
        }
        assert!(out.len() < 64, "no progress");
    }
}

fn q(width: f32, input: impl FnOnce(&mut ParagraphBuilder, &mut Doc)) -> Vec<f32> {
    heights(true, width, input)
}

const WIDE: f32 = 1000.0;

#[test]
fn issue_repro_first_fragment_is_text_free() {
    // a: <span lh40><img><br>x</span> = 2 + 40
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            text(b, "x");
            b.close_inline();
        }),
        [2.0, 40.0]
    );
}

#[test]
fn flag_off_keeps_struts() {
    assert_eq!(
        heights(false, WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
            d.img(b, VerticalAlign::Baseline);
            b.push_forced_break(NodeId(3));
            text(b, "x");
            b.close_inline();
        }),
        [40.0, 40.0]
    );
}

#[test]
fn text_free_boxes_and_root() {
    // b: <span lh40><img></span> = 2
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.close_inline(); }), [2.0]);
    // f: <img><br>x = 2 + 20 (root per line)
    assert_eq!(q(WIDE, |b, d| { d.img(b, VerticalAlign::Baseline); b.push_forced_break(NodeId(3)); text(b, "x"); }), [2.0, 20.0]);
    // n: middle-aligned img in a text-free span = 2
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Middle); b.close_inline(); }), [2.0]);
}

#[test]
fn forced_break_rule() {
    // c: <span lh40><img><br></span> = 2
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.push_forced_break(NodeId(3)); b.close_inline(); }), [2.0]);
    // d: <span lh40><br></span> = 40
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.push_forced_break(NodeId(3)); b.close_inline(); }), [40.0]);
    // h: x<br><br>x = 20 20 20
    assert_eq!(q(WIDE, |b, _| { text(b, "x"); b.push_forced_break(NodeId(3)); b.push_forced_break(NodeId(4)); text(b, "x"); }), [20.0, 20.0, 20.0]);
    // A: x<span lh40><br></span> = 40
    assert_eq!(q(WIDE, |b, _| { text(b, "x"); b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.push_forced_break(NodeId(3)); b.close_inline(); }), [40.0]);
    // B: <span lh40>x<span lh60><br></span></span> = 60
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); text(b, "x"); b.open_inline(NodeId(4), &span(60.0), InlineEdges::default()); b.push_forced_break(NodeId(3)); b.close_inline(); b.close_inline(); }), [60.0]);
    // D: <span lh40><img></span><br> = 2
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.close_inline(); b.push_forced_break(NodeId(3)); }), [2.0]);
    // E: <img><span lh40><br></span> = 40
    assert_eq!(q(WIDE, |b, d| { d.img(b, VerticalAlign::Baseline); b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.push_forced_break(NodeId(3)); b.close_inline(); }), [40.0]);
    // BD: <span lh40><span lh10>x</span><br></span> = 10
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.open_inline(NodeId(4), &span(10.0), InlineEdges::default()); text(b, "x"); b.close_inline(); b.push_forced_break(NodeId(3)); b.close_inline(); }), [10.0]);
    // Y: <span lh10><br></span> = 10 (root not credited)
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(10.0), InlineEdges::default()); b.push_forced_break(NodeId(3)); b.close_inline(); }), [10.0]);
}

#[test]
fn edges_are_border_and_padding_on_the_unit_line() {
    // l: padding-right keeps the strut = 40
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), edges(0.0, 1.0)); d.img(b, VerticalAlign::Baseline); b.close_inline(); }), [40.0]);
    // H: margins do not = 2
    assert_eq!(
        q(WIDE, |b, d| {
            b.open_inline(NodeId(2), &span(40.0), InlineEdges { margin: Sides { inline_start: 1.0, inline_end: 1.0, ..Sides::default() }, ..InlineEdges::default() });
            d.img(b, VerticalAlign::Baseline);
            b.close_inline();
        }),
        [2.0]
    );
    // m/CI: wrapping span, padding on the end only: only the last line keeps it.
    for decoration in [BoxDecorationBreak::Slice, BoxDecorationBreak::Clone] {
        let s = InlineStyle { box_decoration_break: decoration, ..span(40.0) };
        let lines = q(3.0, |b, d| {
            b.open_inline(NodeId(2), &s, edges(0.0, 1.0));
            for _ in 0..3 { d.img(b, VerticalAlign::Baseline); }
            text(b, "x");
            b.close_inline();
        });
        assert_eq!(lines.first(), Some(&2.0), "{decoration:?}: {lines:?}");
        assert_eq!(lines.last(), Some(&40.0), "{decoration:?}: {lines:?}");
        // CG/CH: padding-left: the first and the text line keep it.
        let lines = q(3.0, |b, d| {
            b.open_inline(NodeId(2), &s, edges(1.0, 0.0));
            for _ in 0..3 { d.img(b, VerticalAlign::Baseline); }
            text(b, "x");
            b.close_inline();
        });
        assert_eq!(lines.first(), Some(&40.0), "{decoration:?}: {lines:?}");
        assert!(lines[1..lines.len() - 1].iter().all(|h| *h == 2.0), "{decoration:?}: {lines:?}");
    }
}

#[test]
fn white_space_presence() {
    // k: <span lh40 pre><img> </span> = 40
    let pre = InlineStyle { white_space_collapse: WhiteSpaceCollapse::Preserve, ..span(40.0) };
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &pre, InlineEdges::default()); d.img(b, VerticalAlign::Baseline); text(b, " "); b.close_inline(); }), [40.0]);
    // K: <span lh40 pre>\t</span> = 40
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &pre, InlineEdges::default()); text(b, "\t"); b.close_inline(); }), [40.0]);
    // X: x<span lh40> </span>y = 40 (interior collapsible space)
    assert_eq!(q(WIDE, |b, _| { text(b, "x"); b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); text(b, " "); b.close_inline(); text(b, "y"); }), [40.0]);
    // L: <span lh40><img> </span>x = 40
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Baseline); text(b, " "); b.close_inline(); text(b, "x"); }), [40.0]);
    // F: <span lh40>&shy;</span> = 40
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); text(b, "\u{ad}"); b.close_inline(); }), [40.0]);
    // BI: x<span lh40 pre-line> </span> = 20 (trailing space removed)
    let pre_line = InlineStyle { white_space_collapse: WhiteSpaceCollapse::PreserveBreaks, ..span(40.0) };
    assert_eq!(q(WIDE, |b, _| { text(b, "x"); b.open_inline(NodeId(2), &pre_line, InlineEdges::default()); text(b, " "); b.close_inline(); }), [20.0]);
}

#[test]
fn zero_width_trailing_space_is_not_text() {
    // W: x<span lh40 font-size:0> </span> = 20
    let zero = InlineStyle { font_size: 0.0, ..span(40.0) };
    assert_eq!(q(WIDE, |b, _| { text(b, "x"); b.open_inline(NodeId(2), &zero, InlineEdges::default()); text(b, " "); b.close_inline(); }), [20.0]);
}

#[test]
fn trailing_space_at_a_soft_wrap_is_not_text() {
    // j: wrapping <img><img> <img><img>x keeps 2px image lines.
    let lines = q(3.0, |b, d| {
        b.open_inline(NodeId(2), &span(40.0), InlineEdges::default());
        d.img(b, VerticalAlign::Baseline);
        d.img(b, VerticalAlign::Baseline);
        text(b, " ");
        d.img(b, VerticalAlign::Baseline);
        d.img(b, VerticalAlign::Baseline);
        text(b, "x");
        b.close_inline();
    });
    assert!(lines[..lines.len() - 1].iter().all(|h| *h == 2.0), "{lines:?}");
    assert_eq!(lines.last(), Some(&40.0));
}

#[test]
fn empty_top_aligned_span_adds_no_height() {
    // S: <span lh40 va:top></span>x = 20, and no panic.
    let top = InlineStyle { vertical_align: VerticalAlign::Top, ..span(40.0) };
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &top, InlineEdges::default()); b.close_inline(); text(b, "x"); }), [20.0]);
    // R: <span lh40 va:top><img></span> = 2
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &top, InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.close_inline(); }), [2.0]);
}

#[test]
fn vertical_align_children_of_suppressed_boxes_keep_matching_cases() {
    // CC/DG/DH/CB/CD/CM/BP/BQ/CA: Chromium 2.
    for (outer, inner) in [
        (VerticalAlign::Baseline, VerticalAlign::TextTop),
        (VerticalAlign::Baseline, VerticalAlign::TextBottom),
        (VerticalAlign::Baseline, VerticalAlign::Middle),
        (VerticalAlign::Baseline, VerticalAlign::Sub),
        (VerticalAlign::TextTop, VerticalAlign::Baseline),
        (VerticalAlign::Top, VerticalAlign::Baseline),
        (VerticalAlign::Bottom, VerticalAlign::Baseline),
    ] {
        let s = InlineStyle { vertical_align: outer, ..span(40.0) };
        assert_eq!(
            q(WIDE, |b, d| { b.open_inline(NodeId(2), &s, InlineEdges::default()); d.img(b, inner); b.push_forced_break(NodeId(3)); b.close_inline(); }),
            [2.0],
            "{outer:?}/{inner:?}"
        );
    }
    // CE: <span lh60><span lh40 va:top><img></span><br></span> = 60
    let top = InlineStyle { vertical_align: VerticalAlign::Top, ..span(40.0) };
    assert_eq!(
        q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(60.0), InlineEdges::default()); b.open_inline(NodeId(4), &top, InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.close_inline(); b.push_forced_break(NodeId(3)); b.close_inline(); }),
        [60.0]
    );
}

/// Known differences from Chromium 152 (shodo-9kt): Blink credits a parent
/// through pending top/bottom (and empty text-top/bottom) descendants. Flip
/// these when shodo-9kt lands.
#[test]
fn known_pending_vertical_align_differences() {
    let top = InlineStyle { vertical_align: VerticalAlign::Top, ..span(20.0) };
    // BA: <span lh40><span va:top><img></span><br></span> — Chromium 40.
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.open_inline(NodeId(4), &top, InlineEdges::default()); d.img(b, VerticalAlign::Baseline); b.close_inline(); b.push_forced_break(NodeId(3)); b.close_inline(); }), [2.0]);
    // BB: <span lh40><img va:top><br></span> — Chromium 40.
    assert_eq!(q(WIDE, |b, d| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); d.img(b, VerticalAlign::Top); b.push_forced_break(NodeId(3)); b.close_inline(); }), [2.0]);
    // CP: <span lh40><span va:top></span><br></span> — Chromium 40.
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &span(40.0), InlineEdges::default()); b.open_inline(NodeId(4), &top, InlineEdges::default()); b.close_inline(); b.push_forced_break(NodeId(3)); b.close_inline(); }), [40.0]);
    // CO: <span lh40 va:top></span><br> — Chromium 0; shodo credits the root.
    let top40 = InlineStyle { vertical_align: VerticalAlign::Top, ..span(40.0) };
    assert_eq!(q(WIDE, |b, _| { b.open_inline(NodeId(2), &top40, InlineEdges::default()); b.close_inline(); b.push_forced_break(NodeId(3)); }), [20.0]);
}
```

The `known_pending_vertical_align_differences` expectations are what the minimal rule produces; if the implementation yields something else for one of them, stop and report rather than editing the expectation (BO and CL behave like BA). CP is pinned at 40 because the empty top span is not content under the minimal rule; confirm and keep.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p shodo --test line_height_quirk`
Expected: `issue_repro_first_fragment_is_text_free` etc. FAIL with left `[40.0, 40.0]` (strut never suppressed); `flag_off_keeps_struts` passes.

- [ ] **Step 3: Implement in `measure`**

In `crates/shodo/src/line/metrics.rs::measure`:

1. After computing `root_upright`, build the struts once:

```rust
    let quirk = data
        .style
        .line_height_quirk
        .then(|| super::quirk::Struts::line(data, units.clone()));
    let root_strut = quirk.as_ref().is_none_or(|q| q.root);
    let (mut above, mut below) = if root_strut {
        extents(root, root_metrics.metrics, root_metrics.vertical_metrics, root_upright)
    } else {
        // Nothing has sized the line yet; real contributions replace these.
        (f32::NEG_INFINITY, f32::NEG_INFINITY)
    };
```

2. In the record loop, after obtaining `profile`, decide whether the record sizes the line:

```rust
        let sizes = match (&r.kind, &quirk) {
            (RecordKind::InlineBox { box_index, .. }, Some(q)) => q.contributes(Some(*box_index)),
            // A space removed at the line end sizes nothing, not even with
            // its own glyph extents (Chromium W/BI/j).
            (RecordKind::Glyphs { text, .. }, Some(q)) => !q.trimmed(data, text),
            _ => true,
        };
```

Add to `quirk::Struts` (Task 2 module) a stored trailing start and the helper:

```rust
    // in Struts: pub(crate) trailing: usize, set from `t` in `line()`.

    /// Whether every unit of a glyph record's text is trimmed at the line end.
    pub(crate) fn trimmed(&self, data: &ParagraphData, text: &Range<u32>) -> bool {
        let first = data.units.partition_point(|u| u.text.end <= text.start);
        first >= self.trailing
            && data.units[first..]
                .iter()
                .enumerate()
                .take_while(|(_, u)| u.text.start < text.end)
                .all(|(k, _)| trims(data, first + k))
    }
```

(`Struts` then derives `Default` with `trailing: 0`; set `s.trailing = t` in `line()`. Add a unit test in `quirk.rs` that a paragraph `"x "` has its space record trimmed and `"x"` not.)

and replace the group/own/normal accumulation with:

```rust
        if !sizes {
            if let Some(g) = group {
                // Position-only bounds for a group no member sizes.
                let v = ghosts.entry(g).or_insert((top, bottom));
                v.0 = v.0.min(top);
                v.1 = v.1.max(bottom);
            }
        } else if let Some(g) = group {
            let v = groups.entry(g).or_insert((top, bottom));
            v.0 = v.0.min(top);
            v.1 = v.1.max(bottom);
        } else if let Some(bottom_align) = own_group {
            own_groups.insert(i, (top, bottom, bottom_align));
        } else {
            above = above.max(-top);
            below = below.max(bottom);
        }
```

with `let mut ghosts: crate::hashing::FastMap<u32, (f32, f32)> = FastMap::default();` declared next to `groups`. (`own_group` is only set for atomics, which always size the line.)

3. After the loop, normalize the "nothing contributed" case before heights are computed:

```rust
    if above == f32::NEG_INFINITY {
        above = 0.0;
        below = 0.0;
    }
```

(The combination pre-pass above the record loop also updates `above`/`below` via `max`, so it composes with `NEG_INFINITY`.)

4. Where `deltas` is filled, also give ghost-only groups a delta (positioning only, no height):

```rust
    for (g, (top, bottom)) in ghosts {
        if deltas.contains_key(&g) {
            continue;
        }
        let align = data.styles[data.boxes[g as usize].style as usize].vertical_align;
        deltas.insert(
            g,
            if align == VerticalAlign::Bottom {
                height - above - bottom
            } else {
                -above - top
            },
        );
    }
```

Place this loop after the existing `for (g, (top, bottom)) in groups` loop (which consumes `groups`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p shodo --test line_height_quirk && cargo test -p shodo`
Expected: all PASS. If a matrix case fails, re-check the predicate in Task 2 against the Global Constraints before touching expectations.

- [ ] **Step 5: Commit**

```bash
git add crates/shodo/src/line/metrics.rs crates/shodo/tests/line_height_quirk.rs
git commit -m "fix: suppress quirks-mode struts per line in retained metrics"
```

---

### Task 4: `MetricIndex` quirk tree and parity

**Files:**
- Create: `crates/shodo/src/line/metric_index/quirk.rs` (compact leaf, tree, precomputed arrays, queries)
- Modify: `crates/shodo/src/line/metric_index.rs` (`mod quirk;`, `Group.bounds: Option<Bounds>` + `ghost: Bounds`, `MetricIndex::new`, `select`)
- Test: `crates/shodo/src/ruby/tests/measure.rs` (parity sweep, same style as `indexed_annotation_heights_match_retained_lines_for_clipped_alignment_groups`)

**Interfaces:**
- Consumes: `line::quirk::{trims, text, content, start_edge, end_edge}` and `whitespace::trailing_start` semantics (Task 2); retained behavior (Task 3) is the oracle.
- Produces: `MetricIndex { quirk: Option<Box<quirk::QuirkIndex>>, .. }`; behavior only.

Design (from the approved spec):

```rust
//! Quirks-mode strut contributions for range queries. Kept out of
//! `Summary` so the shared trees stay compact when the quirk is off.
use super::scalar::{Bounds, union};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Side {
    pub(super) normal: Option<Bounds>,
    pub(super) raw: Option<Bounds>,
    pub(super) content: bool,
}
impl Side {
    fn join(self, o: Self) -> Self {
        Self {
            normal: union(self.normal, o.normal),
            raw: union(self.raw, o.raw),
            content: self.content || o.content,
        }
    }
}

/// `all`: as if no unit were trimmed; `kept`: excluding units that trim
/// when they end a line. A range query reads `all` before the trailing
/// start and `kept` from it on.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Leaf {
    pub(super) all: Side,
    pub(super) kept: Side,
}

pub(super) struct QuirkIndex {
    tree: Vec<Leaf>,
    /// `stop[blocked][e]`: start of the backward run `trailing_start(_, 0, e)`
    /// scans when entered in state `blocked` at `e`.
    stop: [Vec<u32>; 2],
    /// `obstructed(data, e)` for every `e`.
    obstructed: Vec<bool>,
    /// Nearest ForcedBreak at or before `i` with only Close units after it.
    forced: Vec<Option<u32>>,
    /// Open unit of each box.
    opens: Vec<u32>,
    /// Prefix count of units covered by a ruby container.
    ruby: Vec<u32>,
    /// Strut summaries of each box (`MetricIndex::boxes`) and of the root.
    root: RecordProfile, // group None, shift 0
}
```

Leaf contents (built in `MetricIndex::new` only when `data.style.line_height_quirk`):
- Cluster/Tab unit `i` with owner `o = parent_box`: strut `s = o.map_or(root, |b| boxes[b])`; `all = Side::from(s, content: true)`; `kept = if quirk::trims(data, i) { Side::default() } else { all }`.
- Atomic: `all = kept = Side { content: true, .. }` (its own extents stay in the shared `tree`).
- Open{b} with `start_edge(b)`, Close{b} with `end_edge(b)`: `all = kept = Side::from(boxes[b], content: true)`.
- `Side::from(p, content)`: `normal = (p.group.is_none() && p.own_group.is_none()).then(bounds)`, `raw = Some(bounds)` — mirror `Summary::profile`.

`trailing_start(start, end)` from the arrays: `max(start, stop[obstructed[end] as usize][end])`, where the DP over `e` follows the `trailing_start` loop: for `e = 0` → `0`; unit `u = e-1`: Close{b} → `stop[s][e] = stop[s || end_edge_inline(b)][e-1]` (blocked uses `padding.inline_end != 0 || border.inline_end != 0`, as in `whitespace.rs`); else if `hangable(u) && (!preserved(u) || !s)` or `transparent(u)` → `stop[s][e] = stop[s][e-1]`; else `stop[s][e] = e`. `obstructed[e]` is computed by a backward pass for the forward-scan part plus a running count of open boxes that are cloned with a nonzero inline-end edge (the `decoration::chain` part); assert equality with `whitespace::obstructed` in a debug test over every `e`.

Changes in `MetricIndex::new` when the quirk is on:
- A `quirk::trims` cluster/tab unit's own glyph (and combination) profile moves out of `tree` into its quirk leaf's `all` side (joined with its owner strut); `kept` stays empty. Its `tree`/`nonglyph` leaf keeps only `active`. In `select`, skip `replacements` entries whose owner unit is `>= t` and `trims` (an overlay glyph of a removed trailing space sizes nothing), and for owners `< t` that trim, put the replacement profile into the quirk side rather than `replacements` only if needed for parity — the sweep decides.
- Do not join box profiles into `tree`/`nonglyph` leaves for Open/Close units (`for p in profile.into_iter().chain(combination)`: skip `profile` when the unit is Open/Close); still store `boxes[box_index] = Some(p)` and keep `active`.
- Group bounds: compute from contributing members only — glyph/atomic/combination profiles plus `Leaf.all.raw` of member units; box profiles go to `Group.ghost` only. `Group.bounds: Option<Bounds>`; when `None`, bake no height into the leaf and use `ghost` for content shifts (`leaf.top/bottom` at `:236-245`).
- Join `Leaf.all` (raw/normal) of member units into each group's bounds before the height bake at `:206-219`.

Changes in `select` when the quirk is on (`let Some(q) = &self.quirk`):
- `t = q.trailing_start(range.start, range.end)`; `side = q.query(range.start..t, all).join(q.query(t..range.end, kept))`.
- Mark as affected every group intersecting `t..range.end`, and the group of the forced break's parent (below).
- `extra`: join `Summary { active, ..Default::default() }` only (no strut); skip `group_extra`.
- Forced break: if `q.forced[range.end - 1] = Some(k)` with `k >= range.start`, `p = data.units[k].parent_box`, `lo = p.map_or(range.start, |b| range.start.max(q.opens[b] + 1))`; content = `q.query(lo..min(k,t), all).content || q.query(max(lo,t)..k, kept).content`; if false, join `p`'s strut (`Side::from`) into `side` (or into `group_extra` when the strut has a group).
- Ruby: if `q.ruby[range.end] > q.ruby[range.start]`, join the root strut.
- Replace the unconditional root strut union at `:498-507` with `union(side.normal, summary.normal)` (root text leaves already carry the root strut); if the union is `None`, use `Bounds { top: 0.0, bottom: 0.0 }`.
- For each affected group's partial bounds (`:427-440` partial query), also join `q.query(selected ∩ ..t, all).raw` and `q.query(selected ∩ t.., kept).raw`.

- [ ] **Step 1: Write the failing parity tests**

Add to `crates/shodo/src/ruby/tests/measure.rs`:

```rust
fn quirk_paragraph(build_content: impl FnOnce(&mut ParagraphBuilder)) -> crate::Paragraph {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle {
            root: InlineStyle {
                line_height: crate::style::LineHeight::Px(30.0),
                ..style(24.0)
            },
            line_height_quirk: true,
            ..Default::default()
        },
        &Limits::default(),
    );
    build_content(&mut b);
    b.build(&mut LayoutContext::new(), &fonts()).unwrap()
}

fn assert_quirk_parity(p: &crate::Paragraph, atomics: &AtomicSizes) {
    let n = p.data.units.len();
    let mut cx = LayoutContext::new();
    cx.ruby_ranges.begin(&p.data, atomics);
    for start in 0..n {
        for end in start + 1..=n {
            let range = start..end;
            // Only ranges a line can hold: skip ranges that split a cluster
            // or start inside a ruby base, as the existing sweep does by
            // choosing owner units.
            if !matches!(
                p.data.units[start].kind,
                crate::analysis::units::UnitKind::Cluster { .. }
                    | crate::analysis::units::UnitKind::Atomic { .. }
                    | crate::analysis::units::UnitKind::Open { .. }
            ) {
                continue;
            }
            let indexed = crate::line::range::block_size(
                &p.data,
                range.clone(),
                &Default::default(),
                atomics,
                &mut cx,
                &mut Saturation::default(),
            );
            let actual = p.ruby_line(
                &mut LayoutContext::new(),
                range.clone(),
                10000.0,
                atomics,
                crate::ruby::align::AnnotationAlign::Policy(RubyAlign::Start),
            );
            assert_eq!(indexed.to_f32(), actual.block_size(), "{range:?}");
        }
    }
}

#[test]
fn quirk_index_matches_retained_lines() {
    use crate::style::{LineHeight, VerticalAlign, WhiteSpaceCollapse};
    let tall = |align| InlineStyle {
        line_height: LineHeight::Px(80.0),
        vertical_align: align,
        ..style(24.0)
    };
    let mut atomics = AtomicSizes::new();
    for id in [90, 91, 92] {
        atomics.insert(
            NodeId(id),
            crate::AtomicSize {
                inline_size: 4.0,
                block_size: 4.0,
                ..Default::default()
            },
        );
    }
    let p = quirk_paragraph(|b| {
        b.open_inline(NodeId(100), &tall(VerticalAlign::Baseline), Default::default());
        b.push_atomic(NodeId(90), &style(24.0), Default::default());
        b.push_forced_break(NodeId(3));
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日 ");
        b.close_inline();
        b.open_inline(NodeId(101), &tall(VerticalAlign::Top), Default::default());
        b.push_atomic(NodeId(91), &style(24.0), Default::default());
        b.close_inline();
        b.open_inline(
            NodeId(102),
            &InlineStyle {
                white_space_collapse: WhiteSpaceCollapse::Preserve,
                ..tall(VerticalAlign::Bottom)
            },
            crate::node::InlineEdges {
                padding: crate::node::Sides {
                    inline_end: 1.0,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        b.push_text(TextSource::Generated { node: NodeId(2) }, "本 ");
        b.close_inline();
        b.open_inline(NodeId(103), &InlineStyle { font_size: 0.0, ..tall(VerticalAlign::Baseline) }, Default::default());
        b.push_text(TextSource::Generated { node: NodeId(4) }, " ");
        b.close_inline();
        b.push_atomic(NodeId(92), &style(24.0), Default::default());
        b.push_ruby(NodeId(8), &style(24.0), ruby(base("語"), "ご"));
    });
    assert_quirk_parity(&p, &atomics);
}

#[test]
fn quirk_index_matches_retained_with_hyphenated_edges() {
    // Same sweep over a paragraph whose line edges reshape (soft hyphen
    // and a ligature-forming pair), so `removed`/`replacements` run.
    let p = quirk_paragraph(|b| {
        b.open_inline(NodeId(100), &InlineStyle { line_height: crate::style::LineHeight::Px(80.0), ..style(24.0) }, Default::default());
        b.push_text(TextSource::Generated { node: NodeId(1) }, "日\u{ad}本\u{ad}語");
        b.close_inline();
    });
    assert_quirk_parity(&p, &AtomicSizes::EMPTY);
}

#[test]
fn quirk_tree_is_absent_without_the_flag() {
    let mut b = ParagraphBuilder::new(
        &ParagraphStyle { root: style(24.0), ..Default::default() },
        &Limits::default(),
    );
    b.push_ruby(NodeId(8), &style(24.0), ruby(base("語"), "ご"));
    let p = b.build(&mut LayoutContext::new(), &fonts()).unwrap();
    let index = crate::line::metric_index::MetricIndex::for_tests(&p.data, &AtomicSizes::EMPTY);
    assert!(!index.has_quirk());
}
```

Add `#[cfg(test)] pub(crate) fn for_tests(...) -> Self` and `#[cfg(test)] pub(crate) fn has_quirk(&self) -> bool` on `MetricIndex` (it is `pub(super)`; widen to `pub(crate)` under `cfg(test)` or add the test inside `metric_index.rs`'s test module instead — prefer the latter if visibility gets awkward). If the soft-hyphen paragraph does not produce a `removed` window with the stub fixture, use the ruby measure helper that existing reshape tests use (`grep -n "hyphen" crates/shodo/src/ruby/tests/*.rs`) and keep the sweep.

Also add a debug equality test in `metric_index/quirk.rs` asserting `QuirkIndex::trailing_start(s, e) == whitespace::trailing_start(data, s, e)` and `obstructed[e] == whitespace::obstructed(data, e)` for every `s <= e` on the Task 2 test paragraph.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p shodo quirk_index`
Expected: parity FAIL (index includes unconditional box/root struts) or compile errors for the new helpers.

- [ ] **Step 3: Implement `metric_index/quirk.rs`** per the design block above (leaf, `join`, tree build bottom-up like `tree`, iterative or recursive `query(range, pick: fn(&Leaf) -> Side)` mirroring `query_node`, DP arrays, `trailing_start`).

- [ ] **Step 4: Wire `new()` and `select()`** per the bullet lists above. Keep every quirk branch behind `if let Some(q) = &self.quirk` / `data.style.line_height_quirk` so the flag-off path is byte-for-byte the old one.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p shodo quirk_index && cargo test -p shodo metric_index && cargo test -p shodo ruby`
Expected: PASS.

- [ ] **Step 6: Bound check**

Run: `cargo test -p shodo actual_annotation_block_profiles_do_not_rescan_long_prefixes nested_annotation_profiles_keep_actual_metrics_and_bounded_probes`
Then add a quirk variant of `actual_annotation_block_profiles_do_not_rescan_long_prefixes` (same body, `line_height_quirk: true` on the paragraph so lanes inherit it) asserting the same `visits[1] <= visits[0] * 3` bound. Count `QuirkIndex::query` node visits in `cx.ruby_measure_visits` under `cfg(test)` like `query_node` does.

- [ ] **Step 7: Commit**

```bash
git add crates/shodo/src/line/metric_index.rs crates/shodo/src/line/metric_index/quirk.rs crates/shodo/src/ruby/tests/measure.rs
git commit -m "fix: apply the quirks-mode strut rule to ruby metric probes"
```

---

### Task 5: Verification (gates, Raikiri WPT, bench build)

**Files:** none committed beyond fixes found here.

- [ ] **Step 1: Quality gates**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench --no-run -p shodo-bench 2>/dev/null || cargo bench --no-run
```
Expected: all clean.

- [ ] **Step 2: Flag-off performance sanity**

Run the layout criterion bench once before/after on `main` vs branch with `perf stat -r 1 -e task-clock` interleaved (see memory `shodo-perf-profiling-recipe-2026-09-30`); expect parity (only a `bool` check per line).

- [ ] **Step 3: Raikiri WPT local check (not committed)**

Per memory `shodo-raikiri-wpt-local-verify`: create `raikiri-spike/.worktrees/e1h-check` from `origin/main` (detached), symlink `target/wpt`, append `[patch.crates-io] shodo = { path = "<shodo worktree>/crates/shodo" }`, and in `crates/raikiri-dom/src/layout/ifc/projection.rs` set `line_height_quirk: line_height_quirk(doc)` on the paragraph style and remove `apply_line_height_quirk`/`apply_inline_line_height_quirk` calls. Add the issue repro (`<span style="line-height:40px"><img style="width:2px;height:2px"><br>x</span>` vs a reference with explicit 2px + 40px blocks) to `crates/raikiri-wpt/testdata/` locally and run `cargo test -p raikiri-wpt --test css_quirks_reftests -- --ignored --nocapture`, plus the existing `quirks-inline-strut` reftest. Record results and the raikiri diff path in the task report; do not commit in raikiri-spike.

- [ ] **Step 4: Report** results (test counts, clippy, bench, Raikiri reftests) in the task report.
