# hanging-punctuation:first caller wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the representative raikiri caller (`dev/raikiri/examples/support/source_replay.rs`) pass the resolved `hanging-punctuation: none | first` from raikiri's `ComputedValues` into `shodo::style::LineOptions`, and produce evidence on the original `hanging-punctuation-first-002` WPT test.

**Architecture:** `source_replay::project` currently rejects a non-initial `hanging_punctuation` in its residual-field guard and never sets `LineOptions.hanging_punctuation`. Fix both there. A new development example `hanging_punctuation` holds CI-safe regression tests (literal HTML in a temp dir, fixed fixture font) and a CLI that replays the original WPT test/reference through the same path. Both S4 spikes stay untouched.

**Tech Stack:** Rust 2024 (MSRV 1.89), pinned raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8` (`raikiri-html`, `raikiri-style`, `raikiri-dom`, `raikiri-traits`), `shodo`, `shodo-fixtures`, `serde_json`.

**Spec:** Design field of beads issue `shodo-9an.1` (`bd show shodo-9an.1`). Two facts learned after the design was approved: (1) `raikiri_style::property::HangingPunctuation` is `#[non_exhaustive]`, so the cross-crate `match` needs a wildcard arm, which must return `Err` (not a default); (2) `text_style` in `source_replay.rs` also rejects the root when `hanging_punctuation` is non-initial, so that guard must be relaxed too.

## Global Constraints

- Source comments and docs are written in English (beads memory `shodo-doc-crate-bevy-2026-09-26`).
- Do not modify, push, PR or merge either S4 spike (`feat/s4-raikiri-integration`, `feat/s4-raikiri-integration-v2`) or the WPT baseline.
- Only `hanging-punctuation: none | first` are in scope. `last`, `force-end`, `allow-end` stay in parent `shodo-9an`; `LineOptions.hanging_punctuation.{last,force_end,allow_end}` must stay `false`.
- An unsupported value must never be treated as success: the wildcard arm returns `Err`.
- No new public API in `crates/shodo`. Changes are confined to `dev/raikiri/` and `docs/`.
- Committed tests must not depend on `~/.cache/raikiri/wpt` (CI does not have it). Only the CLI path reads the WPT checkout.
- Pin/revision facts for evidence: raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8`, WPT revision `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, screen viewport 800 x 600.

## Review Focus

- Value inherited from an ancestor (`#outer{hanging-punctuation:first}`, root declares nothing) must hang: covered by Task 1 test `inherited_first_reaches_the_ifc_root`.
- Explicit `none` on the root nested under a `first` ancestor must not hang: `explicit_none_overrides_inherited_first`.
- A line after a forced break must not hang even if it starts with U+3000: `line_after_forced_break_does_not_hang`.
- A U+3000 that is not at the line start must not move anything: `mid_line_u3000_is_not_hung`.
- RTL leading U+3000: shodo hangs at the inline-start (right) edge while native only handles LTR. Pinned as a characterization test `rtl_leading_u3000_is_shodo_only_behavior`; documented in Task 3.
- A future added `HangingPunctuation` variant must fail closed: guaranteed by the wildcard `Err` arm (verified by reading, no constructible test at this pin).

---

### Task 1: Wire `hanging_punctuation` into `LineOptions` with CI-safe regression tests

**Files:**
- Modify: `dev/raikiri/examples/support/source_replay.rs` (`text_style` residual guard ~line 28-37; `options` construction ~line 139-148)
- Create: `dev/raikiri/examples/hanging_punctuation.rs`
- Modify: `dev/raikiri/Cargo.toml` (add `[[example]]`)

**Interfaces:**
- Consumes: `replay::project(input: &offline::ScreenInput, root: usize, width: f32, context: &mut LayoutContext, fonts: &FontCollection, limits: &Limits) -> Result<Prepared, String>`; `Prepared { pub paragraph: Paragraph, pub options: s::LineOptions }`; `offline::ScreenInput { parsed, cascade, resources }`, `offline::parse_screen(root: &Path, id: &str)`.
- Produces (inside the example file, used by Task 2): `struct G { inline_position: f32, advance: f32, cluster: u32 }`, `struct LineMeasure { hang_start: f32, glyphs: Vec<G> }`, `fn measure(input: &offline::ScreenInput, root: usize, fonts: &FontCollection, width: f32, force_none: bool) -> Result<Vec<LineMeasure>, String>`, `fn find_by_attr(input: &offline::ScreenInput, name: &str, value: &str) -> Result<usize, String>`.

- [ ] **Step 1: Register the example**

Append to `dev/raikiri/Cargo.toml`:

```toml
[[example]]
name = "hanging_punctuation"
test = true
```

- [ ] **Step 2: Write the failing tests (and the measuring helpers they use)**

Create `dev/raikiri/examples/hanging_punctuation.rs`:

```rust
//! Representative raikiri caller check for `hanging-punctuation: none | first`.
//! Development-only; it does not adopt or change either S4 spike.
#[path = "support/offline_wpt.rs"]
mod offline;
#[path = "support/source_replay.rs"]
mod replay;

use raikiri_traits::Dom;
use shodo::{
    AtomicSizes, Fragment, LayoutContext, LineConstraint, LineResult, font::FontCollection,
    limits::Limits, style::HangingPunctuation,
};

#[derive(Clone, Copy, Debug)]
struct G {
    inline_position: f32,
    advance: f32,
    cluster: u32,
}

#[derive(Clone, Debug)]
struct LineMeasure {
    hang_start: f32,
    glyphs: Vec<G>,
}

fn find_by_attr(input: &offline::ScreenInput, name: &str, value: &str) -> Result<usize, String> {
    let dom = &input.parsed.dom;
    (0..dom.node_count())
        .find(|&id| dom.get_node(id).is_some_and(|n| n.attribute(name) == Some(value)))
        .ok_or_else(|| format!("no element with {name}={value}"))
}

/// Lay out the IFC rooted at `root` through the caller path. `force_none`
/// replaces the resolved options with the default (`none`) as a control.
fn measure(
    input: &offline::ScreenInput,
    root: usize,
    fonts: &FontCollection,
    width: f32,
    force_none: bool,
) -> Result<Vec<LineMeasure>, String> {
    let mut context = LayoutContext::new();
    let mut prepared = replay::project(input, root, width, &mut context, fonts, &Limits::default())?;
    if force_none {
        prepared.options.hanging_punctuation = HangingPunctuation::default();
    }
    let p = &prepared.paragraph;
    let events: Vec<_> = p
        .lines(
            &mut context,
            p.start_token(),
            &prepared.options,
            |_, offset| {
                let mut c = LineConstraint::new(width);
                c.block_offset = offset;
                c
            },
            &AtomicSizes::EMPTY,
        )
        .collect();
    let mut lines = Vec::new();
    for event in &events {
        if let LineResult::Line(line) = event {
            let mut glyphs = Vec::new();
            for fragment in line.fragments() {
                if let Fragment::GlyphRun(run) = fragment {
                    glyphs.extend(run.glyphs().map(|g| G {
                        inline_position: g.inline_position,
                        advance: g.advance,
                        cluster: g.cluster,
                    }));
                }
            }
            lines.push(LineMeasure {
                hang_start: line.hang_start(),
                glyphs,
            });
        }
    }
    Ok(lines)
}

fn main() {
    eprintln!("Run `cargo test -p shodo-raikiri --example hanging_punctuation`.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(html: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "shodo-hanging-{}-{n}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("index.html"), html).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const BASE: &str = "#root{font-family:'Shodo Fixture CJK';font-size:16px;line-height:1}";

    /// Lay out `<div id=root>{body}</div>` under `css`, 400px wide.
    fn lines(css: &str, body: &str, force_none: bool) -> Vec<LineMeasure> {
        let html = format!(
            "<!doctype html><style>{BASE}{css}</style><div id=outer><div id=root>{body}</div></div>"
        );
        let dir = TempDir::new(&html);
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let root = find_by_attr(&input, "id", "root").unwrap();
        let fonts = shodo_fixtures::load_fonts(&Limits::default()).unwrap();
        measure(&input, root, &fonts.collection, 400.0, force_none).unwrap()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0 / 64.0
    }

    #[test]
    fn first_on_root_hangs_the_leading_u3000_and_keeps_its_glyph() {
        let lines = lines("#root{hanging-punctuation:first}", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.glyphs.len(), 2, "the hung glyph is retained");
        assert!(line.hang_start > 0.0);
        assert!(close(line.hang_start, line.glyphs[0].advance));
        assert!(close(line.glyphs[0].inline_position, -line.glyphs[0].advance));
        assert!(close(line.glyphs[1].inline_position, 0.0));
    }

    #[test]
    fn absent_declaration_is_none() {
        let lines = lines("", "\u{3000}日", false);
        let line = &lines[0];
        assert_eq!(line.hang_start, 0.0);
        assert!(close(line.glyphs[0].inline_position, 0.0));
        assert!(close(line.glyphs[1].inline_position, line.glyphs[0].advance));
    }

    #[test]
    fn inherited_first_reaches_the_ifc_root() {
        let lines = lines("#outer{hanging-punctuation:first}", "\u{3000}日", false);
        assert!(lines[0].hang_start > 0.0);
    }

    #[test]
    fn explicit_none_overrides_inherited_first() {
        let lines = lines(
            "#outer{hanging-punctuation:first}#root{hanging-punctuation:none}",
            "\u{3000}日",
            false,
        );
        assert_eq!(lines[0].hang_start, 0.0);
    }

    #[test]
    fn force_none_control_moves_the_following_glyph_by_the_advance() {
        let css = "#root{hanging-punctuation:first}";
        let hung = lines(css, "\u{3000}日", false);
        let control = lines(css, "\u{3000}日", true);
        assert!(close(
            control[0].glyphs[1].inline_position - hung[0].glyphs[1].inline_position,
            hung[0].glyphs[0].advance
        ));
    }

    #[test]
    fn line_after_forced_break_does_not_hang() {
        let lines = lines(
            "#root{hanging-punctuation:first}",
            "\u{3000}日<br>\u{3000}日",
            false,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[0].hang_start > 0.0);
        assert_eq!(lines[1].hang_start, 0.0);
        assert!(close(lines[1].glyphs[0].inline_position, 0.0));
    }

    #[test]
    fn mid_line_u3000_is_not_hung() {
        let lines = lines("#root{hanging-punctuation:first}", "日\u{3000}日", false);
        assert_eq!(lines[0].hang_start, 0.0);
        assert_eq!(lines[0].glyphs[0].cluster, 0);
        assert!(close(lines[0].glyphs[0].inline_position, 0.0));
    }

    /// Characterization: raikiri-paint only hangs a leading U+3000 in LTR text.
    /// shodo mirrors the edge for RTL, so this is not native parity.
    #[test]
    fn rtl_leading_u3000_is_shodo_only_behavior() {
        let lines = lines(
            "#root{hanging-punctuation:first;direction:rtl}",
            "\u{3000}日",
            false,
        );
        assert!(lines[0].hang_start > 0.0);
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail for the right reason**

Run: `cargo test -p shodo-raikiri --example hanging_punctuation 2>&1 | tail -40`
Expected: compiles; `first_on_root_...`, `inherited_first_...`, `explicit_none_...`, `force_none_control_...`, `line_after_forced_break_...`, `rtl_...` FAIL because `project` returns `Err("computed style outside the verified ordinary-source input footprint")` (unwrap panics). `absent_declaration_is_none` and `mid_line_u3000_is_not_hung` may PASS or fail for the same reason if the root inherits nothing; either is fine. If they fail for any other reason (e.g. `br` display or font lookup), fix the fixture, not the production code.

- [ ] **Step 4: Implement the fix in `source_replay.rs`**

In `text_style`, extend the reset list (after `remaining.word_break = initial.word_break;`):

```rust
    // Block-container property; the IFC root's value is mapped into
    // `LineOptions` by `project`, so descendants' inherited copies are not
    // residual style.
    remaining.hanging_punctuation = initial.hanging_punctuation;
```

Update the doc comment above it from "nine noninitial fields" to "ten noninitial fields" only if the count is stated there (it is: change "These are the nine" to "These are the ten"; the tenth is not among the original 167-IFC observations, so also say so):

```rust
    // Nine noninitial fields were observed across all 167 original IFCs, and
    // hanging-punctuation is mapped explicitly below. Root sizing is already
    // retained by the original measured width.
```

In `project`, replace the `options` construction with:

```rust
    let hanging_first = match root_cv.hanging_punctuation {
        css::HangingPunctuation::None => false,
        css::HangingPunctuation::First => true,
        // `HangingPunctuation` is non-exhaustive; fail closed for values
        // this caller does not map.
        _ => {
            return Err(
                "hanging-punctuation outside the verified source input footprint".into(),
            );
        }
    };
    let options = s::LineOptions {
        text_align: match root_cv.text_align {
            css::TextAlign::Start => s::TextAlign::Start,
            css::TextAlign::End => s::TextAlign::End,
            css::TextAlign::Left => s::TextAlign::Left,
            css::TextAlign::Right => s::TextAlign::Right,
            _ => return Err("text-align outside the verified source input footprint".into()),
        },
        hanging_punctuation: s::HangingPunctuation {
            first: hanging_first,
            ..Default::default()
        },
        ..Default::default()
    };
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p shodo-raikiri --example hanging_punctuation 2>&1 | tail -30`
Expected: 8 passed. If `rtl_leading_u3000_is_shodo_only_behavior` fails, print the observed `hang_start`/glyph positions, change the assertion to the observed shodo behavior, and keep the doc comment; record the observation for Task 3. If exact `0.0` comparisons on `hang_start` fail with tiny non-zero values, that is a real defect to investigate, not a reason to loosen them.

- [ ] **Step 6: Confirm no regression in the sibling examples**

Run: `cargo test -p shodo-raikiri 2>&1 | grep -E "test result|FAILED|error"`
Expected: every line `ok`, no `FAILED`. (`source_coverage` uses `replay::project`, so its 11 tests exercise the changed guard.)

- [ ] **Step 7: Commit**

```bash
git add dev/raikiri/Cargo.toml dev/raikiri/examples/hanging_punctuation.rs dev/raikiri/examples/support/source_replay.rs
git commit -m "feat(raikiri): pass resolved hanging-punctuation none/first to LineOptions"
```

---

### Task 2: Replay the original `hanging-punctuation-first-002` and record evidence

**Files:**
- Modify: `dev/raikiri/examples/hanging_punctuation.rs` (replace the stub `main`, add `use` lines, add a comparison function with unit tests)
- Create: `dev/raikiri/data/hanging-punctuation-first-002.json` (generated output, committed)

**Interfaces:**
- Consumes (Task 1): `measure`, `find_by_attr`, `LineMeasure`, `G`; plus `offline::parse_screen`, `offline::ResourceRecord` (Serialize), `fonts::load(dir: &Path, limits: &Limits) -> Result<OriginalFonts, String>` where `OriginalFonts { collection, hashes, generics }` from `#[path = "support/source_fonts.rs"] mod fonts;`.
- Produces: `fn arrows_aligned(test: &LineMeasure, reference: &LineMeasure) -> bool` and the CLI `cargo run -p shodo-raikiri --example hanging_punctuation -- <wpt-root> [output.json]`.

Original inputs: `css/css-text/hanging-punctuation/hanging-punctuation-first-002.html` (`<div class=test>　↓</div>`, `font-size:40px; line-height:1`) and `css/css-text/hanging-punctuation/reference/hanging-punctuation-first-002-ref.html` (`<div>↓</div>`). WPT pass condition: the two arrows are aligned.

- [ ] **Step 1: Write the failing unit tests for the comparison**

Add to the `tests` module:

```rust
    fn line(hang: f32, xs: &[f32]) -> LineMeasure {
        LineMeasure {
            hang_start: hang,
            glyphs: xs
                .iter()
                .map(|&x| G { inline_position: x, advance: 40.0, cluster: 0 })
                .collect(),
        }
    }

    #[test]
    fn arrows_are_the_last_glyph_and_must_share_an_inline_position() {
        assert!(arrows_aligned(&line(40.0, &[-40.0, 0.0]), &line(0.0, &[0.0])));
        assert!(!arrows_aligned(&line(0.0, &[0.0, 40.0]), &line(0.0, &[0.0])));
        assert!(!arrows_aligned(&line(0.0, &[]), &line(0.0, &[0.0])));
    }
```

Run: `cargo test -p shodo-raikiri --example hanging_punctuation arrows 2>&1 | tail -15`
Expected: FAIL to compile, `cannot find function arrows_aligned`.

- [ ] **Step 2: Implement `arrows_aligned` and the CLI**

Add module lines at the top next to the existing ones:

```rust
#[path = "support/source_fonts.rs"]
mod fonts;
```

Add above `main` (and delete the stub `main`):

```rust
const TEST: &str = "css/css-text/hanging-punctuation/hanging-punctuation-first-002.html";
const REFERENCE: &str =
    "css/css-text/hanging-punctuation/reference/hanging-punctuation-first-002-ref.html";
const RAIKIRI_PIN: &str = "ab7e619a8f321f03de8b8c8b9342954868e044c8";
/// The pinned screen viewport used by the original comparison.
const VIEWPORT_WIDTH: f32 = 800.0;

/// The WPT pass condition: the arrow (last glyph on the line) sits at the
/// same inline position in the test and in the reference.
fn arrows_aligned(test: &LineMeasure, reference: &LineMeasure) -> bool {
    match (test.glyphs.last(), reference.glyphs.last()) {
        (Some(t), Some(r)) => (t.inline_position - r.inline_position).abs() <= 1.0 / 64.0,
        _ => false,
    }
}

fn run(wpt: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
    let registry = fonts::load(&wpt.join("fonts"), &Limits::default())?;
    let test = offline::parse_screen(wpt, TEST)?;
    let reference = offline::parse_screen(wpt, REFERENCE)?;
    let test_root = find_by_attr(&test, "class", "test")?;
    let dom = &reference.parsed.dom;
    let reference_root = (0..dom.node_count())
        .find(|&id| dom.get_node(id).is_some_and(|n| n.tag_name() == Some("div")))
        .ok_or("reference has no div")?;
    let width = VIEWPORT_WIDTH;
    let hung = measure(&test, test_root, &registry.collection, width, false)?;
    let control = measure(&test, test_root, &registry.collection, width, true)?;
    let reference_lines = measure(&reference, reference_root, &registry.collection, width, false)?;
    let (hung, control, reference_line) = match (hung.first(), control.first(), reference_lines.first()) {
        (Some(a), Some(b), Some(c)) => (a, b, c),
        _ => return Err("expected one accepted line in every replay".into()),
    };
    let hung_glyph = hung.glyphs.first().ok_or("hung line has no glyph")?;
    let checks = serde_json::json!({
        "glyph_retained": hung.glyphs.len() == control.glyphs.len() && hung.glyphs.len() >= 2,
        "hang_start_equals_leading_advance": (hung.hang_start - hung_glyph.advance).abs() <= 1.0 / 64.0 && hung.hang_start > 0.0,
        "arrows_aligned_with_reference": arrows_aligned(hung, reference_line),
        "control_none_is_not_aligned": !arrows_aligned(control, reference_line),
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    let describe = |l: &LineMeasure| {
        serde_json::json!({"hang_start": l.hang_start,
            "glyphs": l.glyphs.iter().map(|g| serde_json::json!({
                "inline_position": g.inline_position, "advance": g.advance, "cluster": g.cluster
            })).collect::<Vec<_>>()})
    };
    let report = serde_json::json!({
        "scope": "shodo layout of the original static test/reference through the representative caller; not a WPT verdict, page paint or baseline PASS count",
        "raikiri_pin": RAIKIRI_PIN, "viewport_width": width,
        "test": TEST, "reference": REFERENCE,
        "font_registry_sha256": registry.hashes,
        "resources": {"test": test.resources, "reference": reference.resources},
        "resolved": {"test": describe(hung), "control_none": describe(control), "reference": describe(reference_line)},
        "checks": checks, "passed": passed,
    });
    std::fs::write(output, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| e.to_string())?;
    if passed { Ok(()) } else { Err("hanging-punctuation-first-002 replay did not reproduce the expected behavior".into()) }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let wpt = args.next().ok_or("usage: hanging_punctuation <wpt-root> [output.json]")?;
    let output = args.next().unwrap_or_else(|| "hanging-punctuation-first-002.json".into());
    run(std::path::Path::new(&wpt), std::path::Path::new(&output))?;
    println!("{output}: original test aligns with its reference; none control does not");
    Ok(())
}
```

If `resources` do not implement `Serialize` for `serde_json::json!` (they derive `Serialize` in `offline_wpt.rs`), no change is needed; if `dead_code` warnings appear for fields that are only used by tests, keep them used or remove them, do not add `#[allow]`.

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p shodo-raikiri --example hanging_punctuation 2>&1 | tail -20`
Expected: 9 passed.

- [ ] **Step 4: Run the CLI against the real WPT checkout**

Run: `cargo run -p shodo-raikiri --example hanging_punctuation -- /home/mitz/.cache/raikiri/wpt dev/raikiri/data/hanging-punctuation-first-002.json`
Expected: prints `...: original test aligns with its reference; none control does not`, exit 0. If it fails, read the JSON `checks`/`resolved` (the file is written before the error) and diagnose. Likely causes: the default WPT font lacks U+2193 so the arrow is a fallback glyph in a second run (still the last glyph, fine); `project` rejecting another non-initial field of the original documents (report it, do not silently reset it); a genuine shodo defect (stop and report, do not weaken checks). Also run `git -C /home/mitz/.cache/raikiri/wpt rev-parse HEAD` and confirm it prints `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`; if not, the evidence is not comparable and must be reported.

- [ ] **Step 5: Confirm the evidence is reproducible**

Run the same command with output `target/hp-second.json`, then `cmp target/hp-second.json dev/raikiri/data/hanging-punctuation-first-002.json`
Expected: no output (identical).

- [ ] **Step 6: Commit**

```bash
git add dev/raikiri/examples/hanging_punctuation.rs dev/raikiri/data/hanging-punctuation-first-002.json
git commit -m "feat(raikiri): replay original hanging-punctuation-first-002 through the caller"
```

---

### Task 3: Record semantics, limits and reproduction

**Files:**
- Create: `docs/records/raikiri-hanging-punctuation.md`
- Modify: `docs/records/raikiri-style-diagnostics.md` (the `shodo-9an.1` table row, currently line ~170, and the paragraph "The original `shodo-9an` diagnosis...")
- Modify: `docs/README.md` (add a row next to the line ~54 raikiri-style-diagnostics entry)

**Interfaces:**
- Consumes: the observed outputs of Task 1 (RTL characterization result) and Task 2 (`dev/raikiri/data/hanging-punctuation-first-002.json`).
- Produces: documentation only.

- [ ] **Step 1: Write `docs/records/raikiri-hanging-punctuation.md`**

English. Sections, each backed by facts already established in this plan:

1. **What is wired**: `source_replay::project` maps the IFC root's resolved `ComputedValues.hanging_punctuation` (`None` -> `first: false`, `First` -> `first: true`) into `LineOptions.hanging_punctuation.first`; unknown (non-exhaustive) values return an error; `last`/`force_end`/`allow_end` stay false. Descendants' inherited copies are not residual style; only the root's value is used because the property applies to the block container.
2. **Reproduce**: the exact `cargo run` and `cmp` commands from Task 2, the WPT revision, the raikiri pin, viewport 800, and the note that the WPT checkout is a local input (CI runs only the `cargo test --example hanging_punctuation` tests).
3. **What the evidence shows**: the U+3000 glyph is retained, `hang_start` equals its advance, the arrow aligns with the reference, and the `none` control does not. State plainly this is a shodo layout comparison in the caller, not raikiri page paint, not a WPT PASS verdict, and does not change the baseline PASS count. The candidate page paint remains unavailable in the frozen S4 spike.
4. **Semantics and limits**: applies to the first formatted line only (later lines and lines after a forced break do not hang); default and explicit `none` do not hang; an inherited `first` does; a non-leading U+3000 does not move. Native (raikiri-paint `text.rs:379-390`) hangs only an authored leading U+3000 in an LTR text node; shodo additionally supports opening brackets/quotes and mirrors the edge for RTL (state the Task 1 RTL characterization result). Those extra behaviors are not native-parity claims and no quote/RTL WPT verdict is made.
5. **Deferred**: `last`, `force-end`, `allow-end` and combined values need raikiri-style parser support and stay in `shodo-9an`; production adoption of this mapping remains part of S4/`shodo-p2m.6`. Neither S4 spike was changed.

- [ ] **Step 2: Update `raikiri-style-diagnostics.md`**

Change the `shodo-9an.1` row's scope cell to state the representative caller now wires None/First (link `raikiri-hanging-punctuation.md`) while the production adoption "waits for S4 adoption policy". Keep the cutover-status wording accurate; do not claim the cutover dependency is satisfied.

- [ ] **Step 3: Add the `docs/README.md` row**

Add: `| Representative caller wiring of hanging-punctuation none/first | [raikiri hanging-punctuation](records/raikiri-hanging-punctuation.md) |` in the same table style as the neighbouring rows.

- [ ] **Step 4: Full verification**

Run each and confirm clean output:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|error" || echo "no failures"
git status --short
```

Expected: fmt/clippy silent, "no failures", only the planned files changed. The clippy `nonminimal_bool` warning in `spacing.rs` mentioned in beads history is not present at current main; if clippy reports an unrelated pre-existing warning, confirm with `git stash`-free means (`git diff main --stat` shows the file untouched) and report it rather than fixing unrelated code.

- [ ] **Step 5: Commit**

```bash
git add docs/records/raikiri-hanging-punctuation.md docs/records/raikiri-style-diagnostics.md docs/README.md
git commit -m "docs: record raikiri hanging-punctuation caller wiring and limits"
```
