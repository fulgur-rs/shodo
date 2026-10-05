# Style ownership handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a main-side representative caller step that splits the flow/paint-owned CSS the frozen S4 style gate rejects as "residual" into typed per-owner handoffs that keep the resolved values, and prove it on the original 109 documents / 303 blocks.

**Architecture:** A new support module `style_handoff.rs` defines an ownership table and `split(cv, profile) -> Result<Handoff, String>`. It reuses the existing frozen-gate reproduction (`diagnostic::prepare_input` / `diagnostic::differences`) so the set of "residual" fields is exactly what S4 rejects; every residual field must have an owner or the split fails closed. A new example `style_handoff` holds CI-safe tests and a CLI that replays the original blocks. Both S4 spikes stay untouched.

**Tech Stack:** Rust 2024 (MSRV 1.89), pinned raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8` (`raikiri-style`, `raikiri-html`, `raikiri-dom`, `raikiri-traits`), `shodo`, `serde_json`, `sha2`.

**Spec:** Design field of beads issue `shodo-0zm` (`bd show shodo-0zm`). Facts established while planning: `dev/raikiri` has no `[lib]`, so shared code lives in `examples/support/*.rs` and is included with `#[path]`; the field owners already exist as `field_rules` in `dev/raikiri/data/raikiri-style-diagnostics.json`; `hanging_punctuation` is wired by shodo-9an.1 (done) and `cssom_writing_mode` / `text_orientation` belong to shodo-3v2 (deferred); `outline_offset` and the `text_decoration_{style,color,thickness}` / `text_underline_offset` fields are not in the frozen S4 reset list either, so they are added to the ownership table so a real underline or outline does not fail the split.

## Global Constraints

- Source comments and docs are written in English (beads memory `shodo-doc-crate-bevy-2026-09-26`).
- Do not modify, push, PR or merge either S4 spike (`feat/s4-raikiri-integration`, `feat/s4-raikiri-integration-v2`), and do not change the original WPT inputs, fonts or baseline.
- No painting, no positioned-box layout, no BFC engine, no whole-page work. No new public API in `crates/shodo`. Changes are confined to `dev/raikiri/` and `docs/`.
- Resolved values are never reset to initial to make a field "pass". Anything not in the ownership table fails closed with `unmapped: <field>`.
- Committed tests must not depend on `~/.cache/raikiri/wpt` or on the S4 comparison artifact (CI does not have them). Only the CLI path reads them.
- The evidence makes no WPT PASS/FAIL, image-verdict, page-paint or cutover-necessity claim.
- Pin/revision facts: raikiri `ab7e619a8f321f03de8b8c8b9342954868e044c8`, WPT `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`, original comparison SHA256 `67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d`.

## Review Focus

- A field the table does not know (for example `opacity`) must make `split` fail with `unmapped: opacity`, not pass silently: test `unknown_residual_field_fails_closed`.
- `position: absolute` / `fixed` must be flagged `out_of_flow` and keep its `left`/`top` values, not be dropped: test `absolute_is_out_of_flow_but_keeps_its_values`.
- Non-initial values must be retained, not reset: tests compare each handed-off value with `ComputedValues::initial()`.
- `hanging-punctuation` and `writing-mode` residuals must be accounted for under their sibling issues (9an.1 / 3v2) rather than treated as unmapped: test `sibling_issue_fields_are_accounted_not_unmapped`.
- Underline: only a solid underline converts to `shodo::style::TextDecoration`; overline / line-through / non-solid must be rejected, not silently dropped: test `only_solid_underline_converts`.
- The CLI's per-field block counts must equal the committed `raikiri-style-diagnostics.json` counts, or the run fails: enforced in Task 2 and tested with a mismatch case.

---

### Task 1: Ownership table and `split`, with CI-safe tests

**Files:**
- Create: `dev/raikiri/examples/support/style_handoff.rs`
- Create: `dev/raikiri/examples/style_handoff.rs` (stub `main` in this task, replaced in Task 2)
- Modify: `dev/raikiri/Cargo.toml` (add `[[example]]`)

**Interfaces:**
- Consumes: `super::diagnostic::{InputProfile, prepare_input, differences}` from `dev/raikiri/examples/support/raikiri_style_diffs.rs` (`differences(&ComputedValues) -> Vec<Difference>` where `Difference { field: String, value: String, initial: String }`; `prepare_input(&ComputedValues, InputProfile) -> ComputedValues`); `offline::parse_screen(root: &Path, id: &str) -> Result<ScreenInput, String>` with `ScreenInput { parsed, cascade, resources }` from `support/offline_wpt.rs`.
- Produces (in `support/style_handoff.rs`, used by Task 2): `pub enum Owner`, `pub fn owner(field: &str) -> Option<Owner>`, `Owner::name(self) -> &'static str`, `pub struct Handoff { flow, positioned, box_paint, decoration, residual: Vec<(String, Owner)> }`, `pub fn split(values: &ComputedValues, profile: diagnostic::InputProfile) -> Result<Handoff, String>`, `pub fn solid_underline(values: &ComputedValues) -> Result<Option<shodo::style::TextDecoration>, String>`.

- [ ] **Step 1: Register the example**

Append to `dev/raikiri/Cargo.toml`:

```toml
[[example]]
name = "style_handoff"
test = true
```

- [ ] **Step 2: Write the failing tests**

Create `dev/raikiri/examples/style_handoff.rs`:

```rust
//! Representative raikiri caller step: split the flow/paint-owned CSS that the
//! frozen S4 style gate rejects into typed per-owner handoffs.
//! Development-only; it does not adopt or change either S4 spike.
#[path = "support/raikiri_style_diffs.rs"]
#[allow(dead_code)]
mod diagnostic;
#[path = "support/offline_wpt.rs"]
#[allow(dead_code)]
mod offline;
#[path = "support/style_handoff.rs"]
mod handoff;

fn main() {
    eprintln!("Run `cargo test -p shodo-raikiri --example style_handoff`.");
}

#[cfg(test)]
mod tests {
    use super::{diagnostic::InputProfile, handoff, offline};
    use handoff::Owner;
    use raikiri_style::{ComputedValues, property as css};
    use raikiri_traits::Dom;
    use std::path::PathBuf;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(html: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("shodo-handoff-{}-{n}", std::process::id()));
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

    /// Resolved style of `<div id=root>` under `css`, through the real parser and cascade.
    fn root(css: &str) -> ComputedValues {
        let html = format!("<!doctype html><style>#root{{{css}}}</style><div id=root>x</div>");
        let dir = TempDir::new(&html);
        let input = offline::parse_screen(&dir.0, "index.html").unwrap();
        let dom = &input.parsed.dom;
        let id = (0..dom.node_count())
            .find(|&i| dom.get_node(i).is_some_and(|n| n.attribute("id") == Some("root")))
            .unwrap();
        input.cascade.computed[id].clone()
    }

    fn split(css: &str) -> Result<handoff::Handoff, String> {
        handoff::split(&root(css), InputProfile::MeasuredBlock)
    }

    #[test]
    fn box_paint_values_are_retained_not_reset() {
        let h = split(
            "background-color:#00f;background-image:linear-gradient(red,red);\
             background-size:20px 20px;background-repeat:no-repeat;background-position:0 0;\
             outline:2px solid red;overflow:hidden",
        )
        .unwrap();
        let initial = ComputedValues::initial();
        let p = &h.box_paint;
        assert_eq!((p.background_color.r, p.background_color.g, p.background_color.b), (0, 0, 255));
        assert_ne!(p.background_image, initial.background_image);
        assert_ne!(p.background_size, initial.background_size);
        assert_ne!(p.background_repeat, initial.background_repeat);
        assert_ne!(p.background_position, initial.background_position);
        assert_ne!(p.outline, initial.outline);
        assert_ne!(p.overflow, initial.overflow);
        for field in ["background_color", "background_image", "outline", "overflow"] {
            assert!(h.residual.iter().any(|(f, o)| f == field && *o == Owner::BoxPaint), "{field}");
        }
    }

    #[test]
    fn flow_values_are_retained() {
        let h = split("float:left;clear:both").unwrap();
        assert_ne!(h.flow.float, ComputedValues::initial().float);
        assert_eq!(h.flow.clear, css::ClearValue::Both);
        assert!(h.residual.iter().all(|(_, o)| *o == Owner::Flow));
    }

    #[test]
    fn relative_position_offsets_and_z_order_are_retained() {
        let h = split("position:relative;left:5px;top:6px;z-index:-1").unwrap();
        let initial = ComputedValues::initial();
        assert_eq!(h.positioned.position, css::PositionValue::Relative);
        assert_ne!(h.positioned.left, initial.left);
        assert_ne!(h.positioned.top, initial.top);
        assert_ne!(h.positioned.z_index, initial.z_index);
        assert!(!h.positioned.out_of_flow);
    }

    #[test]
    fn absolute_is_out_of_flow_but_keeps_its_values() {
        let h = split("position:absolute;left:1px;top:2px").unwrap();
        assert_eq!(h.positioned.position, css::PositionValue::Absolute);
        assert!(h.positioned.out_of_flow);
        assert_ne!(h.positioned.left, ComputedValues::initial().left);
    }

    #[test]
    fn sibling_issue_fields_are_accounted_not_unmapped() {
        let h = split("hanging-punctuation:first;writing-mode:vertical-rl").unwrap();
        assert!(h.residual.iter().any(|(f, o)| f == "hanging_punctuation" && *o == Owner::HangingPunctuation));
        assert!(h.residual.iter().any(|(f, o)| f == "cssom_writing_mode" && *o == Owner::VerticalText));
    }

    #[test]
    fn unknown_residual_field_fails_closed() {
        let error = split("opacity:0.5").unwrap_err();
        assert!(error.starts_with("unmapped: opacity"), "{error}");
    }

    #[test]
    fn initial_style_has_no_residual() {
        let h = split("").unwrap();
        assert!(h.residual.is_empty());
        assert!(!h.positioned.out_of_flow);
    }

    #[test]
    fn every_owned_field_name_has_an_owner() {
        for field in [
            "float", "clear", "position", "left", "top", "z_index", "background_color",
            "background_image", "background_position", "background_repeat", "background_size",
            "outline", "outline_offset", "overflow", "text_decoration_line",
            "text_decoration_style", "text_decoration_color", "text_decoration_thickness",
            "text_underline_offset", "hanging_punctuation", "cssom_writing_mode", "text_orientation",
        ] {
            assert!(handoff::owner(field).is_some(), "{field}");
        }
        assert!(handoff::owner("opacity").is_none());
    }

    #[test]
    fn only_solid_underline_converts() {
        let underline = handoff::solid_underline(&root(
            "text-decoration:underline;text-decoration-color:lime;text-decoration-thickness:2px",
        ))
        .unwrap()
        .expect("underline");
        assert_eq!(underline.color, Some([0, 255, 0, 255]));
        assert_eq!(underline.thickness, Some(2.0));
        assert!(handoff::solid_underline(&root("")).unwrap().is_none());
        for css in [
            "text-decoration:overline",
            "text-decoration:line-through",
            "text-decoration:underline dotted",
        ] {
            assert!(handoff::solid_underline(&root(css)).is_err(), "{css}");
        }
    }
}
```

- [ ] **Step 3: Run the tests and confirm they fail for the right reason**

Run: `cargo test -p shodo-raikiri --example style_handoff 2>&1 | tail -20`
Expected: FAIL to compile with `couldn't read .../support/style_handoff.rs` (file not found). That is the RED state.

- [ ] **Step 4: Implement the support module**

Create `dev/raikiri/examples/support/style_handoff.rs`:

```rust
//! Ownership split for CSS that the frozen S4 style gate rejects as residual.
//! Resolved values stay with their owner; nothing is reset to initial, and no
//! painting, positioning or BFC work happens here.
use super::diagnostic;
use raikiri_style::{
    ComputedBackgroundSize, ComputedCssPosition, ComputedLengthPercentageOrAuto, ComputedOutline,
    ComputedTextDecorationThickness, ComputedTextUnderlineOffset, ComputedValues, property as css,
};
use shodo::style::TextDecoration;

/// Who must consume a residual field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// Float placement and clearance in the block formatting context.
    Flow,
    /// Positioned-box placement and stacking.
    Positioned,
    /// Box background, outline and clip painting.
    BoxPaint,
    /// Text decoration lines carried by `PaintStyle`.
    Decoration,
    /// Mapped to `LineOptions` by shodo-9an.1.
    HangingPunctuation,
    /// Tracked by shodo-3v2 (vertical text).
    VerticalText,
}

impl Owner {
    pub fn name(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Positioned => "positioned",
            Self::BoxPaint => "box-paint",
            Self::Decoration => "decoration",
            Self::HangingPunctuation => "hanging-punctuation(shodo-9an.1)",
            Self::VerticalText => "vertical-text(shodo-3v2)",
        }
    }
}

/// Owner of every residual field this step or a sibling issue accounts for.
pub fn owner(field: &str) -> Option<Owner> {
    Some(match field {
        "float" | "clear" => Owner::Flow,
        "position" | "left" | "top" | "z_index" => Owner::Positioned,
        "background_color" | "background_image" | "background_position" | "background_repeat"
        | "background_size" | "outline" | "outline_offset" | "overflow" => Owner::BoxPaint,
        "text_decoration_line" | "text_decoration_style" | "text_decoration_color"
        | "text_decoration_thickness" | "text_underline_offset" => Owner::Decoration,
        "hanging_punctuation" => Owner::HangingPunctuation,
        "cssom_writing_mode" | "text_orientation" => Owner::VerticalText,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub float: css::FloatValue,
    pub clear: css::ClearValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Positioned {
    pub position: css::PositionValue,
    pub left: ComputedLengthPercentageOrAuto,
    pub top: ComputedLengthPercentageOrAuto,
    pub z_index: css::ZIndexValue,
    /// `absolute` / `fixed`: outside normal IFC flow. Legitimate for a single
    /// paragraph, but a whole page must still place the box, not drop it.
    pub out_of_flow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxPaint {
    pub background_color: css::CssColor,
    pub background_image: css::BackgroundImage,
    pub background_position: ComputedCssPosition,
    pub background_repeat: css::BackgroundRepeat,
    pub background_size: ComputedBackgroundSize,
    pub outline: ComputedOutline,
    pub overflow: css::OverflowXY,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decoration {
    pub line: css::TextDecorationLine,
}

/// Resolved values per owner, plus every residual field found and its owner.
#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    pub flow: Flow,
    pub positioned: Positioned,
    pub box_paint: BoxPaint,
    pub decoration: Decoration,
    pub residual: Vec<(String, Owner)>,
}

fn keep<T: Clone>(value: &T) -> T {
    value.clone()
}

/// Split the residual fields of `values`. Every field the frozen S4 gate would
/// reject must have an owner; otherwise this fails closed.
pub fn split(
    values: &ComputedValues,
    profile: diagnostic::InputProfile,
) -> Result<Handoff, String> {
    let prepared = diagnostic::prepare_input(values, profile);
    let mut residual = Vec::new();
    for difference in diagnostic::differences(&prepared) {
        let Some(owner) = owner(&difference.field) else {
            return Err(format!("unmapped: {}", difference.field));
        };
        residual.push((difference.field, owner));
    }
    Ok(Handoff {
        flow: Flow {
            float: keep(&values.float),
            clear: keep(&values.clear),
        },
        positioned: Positioned {
            position: keep(&values.position),
            left: keep(&values.left),
            top: keep(&values.top),
            z_index: keep(&values.z_index),
            out_of_flow: matches!(
                values.position,
                css::PositionValue::Absolute | css::PositionValue::Fixed
            ),
        },
        box_paint: BoxPaint {
            background_color: keep(&values.background_color),
            background_image: keep(&values.background_image),
            background_position: keep(&values.background_position),
            background_repeat: keep(&values.background_repeat),
            background_size: keep(&values.background_size),
            outline: keep(&values.outline),
            overflow: keep(&values.overflow),
        },
        decoration: Decoration {
            line: keep(&values.text_decoration_line),
        },
        residual,
    })
}

/// This node's own solid underline as the existing `PaintStyle.underline`
/// input. Propagating an ancestor's line to inline text descendants, and the
/// single drawing of a shared glyph, remain the caller's existing walk and
/// paint path (see `raikiri_contracts.rs`); nothing is drawn here.
pub fn solid_underline(values: &ComputedValues) -> Result<Option<TextDecoration>, String> {
    let line = &values.text_decoration_line;
    if line.overline || line.line_through || line.blink || line.spelling_error || line.grammar_error
    {
        return Err("only a solid underline can be handed to PaintStyle".into());
    }
    if !line.underline {
        return Ok(None);
    }
    if values.text_decoration_style != css::TextDecorationStyle::Solid {
        return Err("only a solid underline can be handed to PaintStyle".into());
    }
    let rgba = |c: css::CssColor| [c.r, c.g, c.b, c.a];
    let color = match values.text_decoration_color {
        css::TextDecorationColor::CurrentColor => rgba(keep(&values.color)),
        css::TextDecorationColor::Resolved(c) => rgba(c),
        _ => return Err("unsupported decoration color".into()),
    };
    let thickness = match values.text_decoration_thickness {
        ComputedTextDecorationThickness::Auto | ComputedTextDecorationThickness::FromFont => None,
        ComputedTextDecorationThickness::Length(v) => Some(v.0),
    };
    if values.text_underline_offset != ComputedTextUnderlineOffset::Auto {
        return Err("underline offset needs its own PaintStyle mapping".into());
    }
    Ok(Some(TextDecoration {
        color: Some(color),
        thickness,
        offset: None,
    }))
}
```

If a `use` path or a `Copy`/`Clone` detail does not compile, follow the compiler (type paths are: `Computed*` at the `raikiri_style` root, the enums under `raikiri_style::property`); do not change behavior. If `TextDecoration`'s field types differ from the test's `Some([u8; 4])` / `Some(f32)`, mirror `dev/raikiri/examples/support/raikiri_contracts.rs` (`fn style`, the `underline` block) which constructs the same struct.

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p shodo-raikiri --example style_handoff 2>&1 | tail -25`
Expected: 9 passed. If a CSS declaration is not accepted by the pinned parser (for example the shorthand `outline`, `background-*` or `text-decoration:underline dotted`), adjust only the test's CSS to an accepted spelling that still sets that value non-initial, and say so in the report. If `unknown_residual_field_fails_closed` does not fail with `opacity` because the frozen S4 reset list already covers it, pick another field from `dev/raikiri/examples/support/raikiri_style_fields.rs`'s `public_differences` list that is not in `reset_mapped` and not in the ownership table (for example `visibility`, `text_shadow`, `box_shadow`, `filter`, `transform`), and use it in both this test and `every_owned_field_name_has_an_owner`.

- [ ] **Step 6: Confirm no regression and clean output**

Run: `cargo test -p shodo-raikiri 2>&1 | grep -E "test result|FAILED|warning|error"` then `cargo clippy -p shodo-raikiri --example style_handoff -- -D warnings`
Expected: every `test result: ok`; clippy silent. (The non-test build of this example has dead code until Task 2 uses the module; if clippy reports it, put `#[allow(dead_code)]` on the `mod handoff;` declaration only and remove that allow in Task 2 if no longer needed.)

- [ ] **Step 7: Commit**

```bash
git add dev/raikiri/Cargo.toml dev/raikiri/examples/style_handoff.rs dev/raikiri/examples/support/style_handoff.rs
git commit -m "feat(raikiri): split flow/paint-owned residual CSS into typed handoffs"
```

---

### Task 2: Replay the original 303 blocks and record evidence

**Files:**
- Modify: `dev/raikiri/examples/style_handoff.rs` (replace the stub `main`, add helpers and unit tests)
- Create: `dev/raikiri/data/raikiri-style-handoff.json` (generated output, committed)

**Interfaces:**
- Consumes (Task 1): `handoff::{split, owner, Owner}`, `diagnostic::InputProfile`; `offline::{parse_screen, verify_original_resources, ResourceRecord}`. The committed classification `dev/raikiri/data/raikiri-style-diagnostics.json` (`fields.<name>.blocks`, `fields.<name>.documents`, top-level `blocks`, `documents`).
- Produces: `fn error_node(error: &str) -> Result<usize, String>`, `fn compare_counts(actual: &BTreeMap<String, usize>, expected: &Value) -> Result<(), String>`, and the CLI `cargo run -p shodo-raikiri --example style_handoff -- <wpt-root> <original-comparison.json> <output.json>`.

The replay mirrors `dev/raikiri/examples/raikiri_style_diffs.rs` (`inspect_case`, lines ~25-126): same target selection (blocks whose `error` contains `noninitial style not mapped yet`), same resource verification, same profile choice (root -> `MeasuredBlock`, `inline-block` -> `Atomic`, otherwise `Plain`).

- [ ] **Step 1: Write the failing unit tests**

Add to the `tests` module in `dev/raikiri/examples/style_handoff.rs`:

```rust
    #[test]
    fn error_node_is_parsed_from_the_original_error_text() {
        let text = "Unsupported { node: 42, reason: \"noninitial style not mapped yet\" }";
        assert_eq!(super::error_node(text), Ok(42));
        assert!(super::error_node("Unsupported { node: x }").is_err());
    }

    #[test]
    fn block_counts_must_equal_the_committed_classification() {
        use std::collections::BTreeMap;
        let expected = serde_json::json!({"position": {"blocks": 2}, "float": {"blocks": 1}});
        let mut actual = BTreeMap::new();
        actual.insert("position".to_owned(), 2);
        actual.insert("float".to_owned(), 1);
        assert!(super::compare_counts(&actual, &expected).is_ok());
        actual.insert("position".to_owned(), 3);
        assert!(super::compare_counts(&actual, &expected).is_err());
        actual.insert("position".to_owned(), 2);
        actual.insert("opacity".to_owned(), 1);
        assert!(super::compare_counts(&actual, &expected).is_err());
    }
```

Run: `cargo test -p shodo-raikiri --example style_handoff error_node 2>&1 | tail -15`
Expected: FAIL to compile, `cannot find function error_node`.

- [ ] **Step 2: Implement helpers and the CLI**

Replace the stub `main` (and add the `use` lines below the module declarations) in `dev/raikiri/examples/style_handoff.rs`:

```rust
use raikiri_traits::{Dom, NodeId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, collections::BTreeSet, path::Path};

const RAIKIRI_PIN: &str = "ab7e619a8f321f03de8b8c8b9342954868e044c8";
const REASON: &str = "noninitial style not mapped yet";
const CLASSIFICATION: &str = include_str!("../data/raikiri-style-diagnostics.json");

fn error_node(error: &str) -> Result<usize, String> {
    error
        .strip_prefix("Unsupported { node: ")
        .and_then(|s| s.strip_suffix(", reason: \"noninitial style not mapped yet\" }"))
        .ok_or("unrecognized original error format")?
        .parse()
        .map_err(|_| "invalid error node".into())
}

/// The handoff must account for exactly the fields, and the same number of
/// blocks per field, that the committed classification recorded.
fn compare_counts(actual: &BTreeMap<String, usize>, expected: &Value) -> Result<(), String> {
    let expected = expected.as_object().ok_or("classification has no fields")?;
    let names: BTreeSet<_> = expected.keys().cloned().chain(actual.keys().cloned()).collect();
    for name in names {
        let want = expected.get(&name).and_then(|f| f["blocks"].as_u64());
        let have = actual.get(&name).map(|&n| n as u64);
        if want != have {
            return Err(format!("field {name}: classified {want:?} blocks, handed off {have:?}"));
        }
    }
    Ok(())
}

fn targets(case: &Value) -> Vec<&Value> {
    case["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["error"].as_str().is_some_and(|e| e.contains(REASON)))
        .collect()
}

fn replay_case(
    wpt: &Path,
    case: &Value,
    fields: &mut BTreeMap<String, usize>,
    owners: &mut BTreeMap<&'static str, usize>,
) -> Result<Vec<Value>, String> {
    let id = case["id"].as_str().ok_or("missing original case ID")?;
    let expected: Vec<offline::ResourceRecord> =
        serde_json::from_value(case["resources"].clone()).map_err(|e| e.to_string())?;
    offline::verify_original_resources(wpt, &expected)?;
    let input = offline::parse_screen(wpt, id)?;
    let mut blocks = Vec::new();
    for target in targets(case) {
        let node = error_node(target["error"].as_str().ok_or("missing original error")?)?;
        let root = target["root"]
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid original root")?;
        let dom = &input.parsed.dom;
        if json!(dom.get_node(root).ok_or("original root no longer exists")?.tag_name())
            != target["tag"]
        {
            return Err("original root tag changed".into());
        }
        let values = input.cascade.computed.get(node).ok_or("missing node style")?;
        let profile = if node == root {
            diagnostic::InputProfile::MeasuredBlock
        } else if values.display == raikiri_style::property::DisplayValue::InlineBlock {
            diagnostic::InputProfile::Atomic
        } else {
            diagnostic::InputProfile::Plain
        };
        let handoff = handoff::split(values, profile).map_err(|e| format!("node {node}: {e}"))?;
        for (field, owner) in &handoff.residual {
            *fields.entry(field.clone()).or_default() += 1;
            *owners.entry(owner.name()).or_default() += 1;
        }
        blocks.push(json!({
            "root": root, "error_node": node, "input_profile": profile.name(),
            "out_of_flow": handoff.positioned.out_of_flow,
            "residual": handoff.residual.iter().map(|(f, o)| json!({"field": f, "owner": o.name()})).collect::<Vec<_>>(),
            "handed_off": {
                "flow": format!("{:?}", handoff.flow), "positioned": format!("{:?}", handoff.positioned),
                "box_paint": format!("{:?}", handoff.box_paint), "decoration": format!("{:?}", handoff.decoration),
            },
        }));
    }
    Ok(blocks)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: style_handoff <wpt-root> <original-comparison.json> <output.json>".into());
    }
    let wpt = Path::new(&args[0]);
    let source = std::fs::read(&args[1])?;
    let comparison: Value = serde_json::from_slice(&source)?;
    if comparison["raikiri_revision"] != RAIKIRI_PIN {
        return Err("comparison does not use the compiled raikiri pin".into());
    }
    let classification: Value = serde_json::from_str(CLASSIFICATION)?;
    let cases = comparison["cases"].as_array().ok_or("missing original cases")?;
    let selected: Vec<_> = cases.iter().filter(|c| !targets(c).is_empty()).collect();
    if selected.is_empty() {
        return Err("no original residual rejections selected".into());
    }
    let (mut fields, mut owners) = (BTreeMap::new(), BTreeMap::new());
    let (mut records, mut errors) = (Vec::new(), Vec::new());
    let mut block_count = 0;
    for case in &selected {
        let id = case["id"].as_str().ok_or("missing original case ID")?;
        match replay_case(wpt, case, &mut fields, &mut owners) {
            Ok(blocks) => {
                block_count += blocks.len();
                records.push(json!({"id": id, "blocks": blocks}));
            }
            Err(error) => errors.push(json!({"id": id, "error": error})),
        }
    }
    let counts = compare_counts(&fields, &classification["fields"]);
    let complete = errors.is_empty()
        && counts.is_ok()
        && selected.len() as u64 == classification["documents"].as_u64().unwrap_or(0)
        && block_count as u64 == classification["blocks"].as_u64().unwrap_or(0);
    let report = json!({
        "scope": "ownership split of the frozen S4 residual style gate over the original blocks; no layout, paint, WPT verdict or cutover-necessity claim",
        "raikiri_revision": RAIKIRI_PIN,
        "original_comparison_sha256": format!("{:x}", Sha256::digest(&source)),
        "documents": selected.len(), "blocks": block_count,
        "unmapped_errors": errors, "field_counts_match_classification": counts.is_ok(),
        "field_counts": fields, "owner_counts": owners,
        "complete": complete, "candidate_wpt_image_verdicts": 0, "pass_delta": null,
        "cases": records,
    });
    std::fs::write(&args[2], serde_json::to_string_pretty(&report)? + "\n")?;
    println!("{} documents, {block_count} blocks, {} errors", selected.len(), errors.len());
    if !complete {
        return Err(format!("handoff replay incomplete: {:?}", counts.err()).into());
    }
    Ok(())
}
```

Notes: `input.cascade.computed.get(node)` is used because `computed` is a `Vec`; keep `values` as `&ComputedValues`. If `#[allow(dead_code)]` on `mod handoff;` was added in Task 1, remove it now if the build no longer warns. Do not add other `#[allow]`.

- [ ] **Step 3: Run the unit tests**

Run: `cargo test -p shodo-raikiri --example style_handoff 2>&1 | tail -20`
Expected: 11 passed (9 from Task 1 + 2 new).

- [ ] **Step 4: Run the CLI against the real inputs**

Run: `cargo run -p shodo-raikiri --example style_handoff -- ~/.cache/raikiri/wpt target/worktrees/shodo-s4-v2/target/s4v2/wpt-batch-full/comparison.json dev/raikiri/data/raikiri-style-handoff.json`
Expected: prints `109 documents, 303 blocks, 0 errors`, exit 0. Before running, confirm `git -C ~/.cache/raikiri/wpt rev-parse HEAD` prints `97ea26e26a2aac3eec7e770650b25e7049ed4a4e` and `sha256sum` of the comparison file equals `67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d` (record both in the report). If a block fails with `unmapped: <field>` or the counts differ from the classification, report it with the exact message and stop (do not add a field to the ownership table to make it pass without reporting): that is a real finding about the ownership decisions.

- [ ] **Step 5: Confirm the evidence is reproducible and self-consistent**

Run the same command with output `target/handoff-second.json`, then `cmp target/handoff-second.json dev/raikiri/data/raikiri-style-handoff.json`; then `python3 -c "import json;d=json.load(open('dev/raikiri/data/raikiri-style-handoff.json'));print(d['complete'],d['documents'],d['blocks'],d['field_counts_match_classification'],d['owner_counts'])"`
Expected: `cmp` prints nothing; the Python line prints `True 109 303 True {...}`. Also verify the JSON contains no absolute paths or timestamps (`grep -c "/home/" dev/raikiri/data/raikiri-style-handoff.json` prints 0).

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all -- --check` and `cargo clippy -p shodo-raikiri --example style_handoff -- -D warnings` (both clean; run `cargo fmt --all` if formatting differs in the two new example/support files only).

```bash
git add dev/raikiri/examples/style_handoff.rs dev/raikiri/data/raikiri-style-handoff.json
git commit -m "feat(raikiri): replay original residual blocks through the ownership split"
```

---

### Task 3: Record the split, its limits and how to reproduce it

**Files:**
- Create: `docs/records/raikiri-style-handoff.md`
- Modify: `docs/records/raikiri-style-diagnostics.md` (the `shodo-0zm` row of the follow-up table, ~line 172, and the "Follow-up issues" prose if it mentions 0zm)
- Modify: `docs/README.md` (add a row next to the raikiri-style-diagnostics entry)

**Interfaces:**
- Consumes: Task 1 code/tests, Task 2's `dev/raikiri/data/raikiri-style-handoff.json` (read its `owner_counts`, `field_counts`, `documents`, `blocks` and quote the real numbers).
- Produces: documentation only.

- [ ] **Step 1: Write `docs/records/raikiri-style-handoff.md`**

English. Read the Task 1 and Task 2 code and the evidence JSON first; every number and claim must come from them. Sections:

1. **What it is**: `dev/raikiri/examples/support/style_handoff.rs` splits the residual (non-text) CSS the frozen S4 gate rejects into `Flow`, `Positioned`, `BoxPaint`, `Decoration` handoffs that keep the resolved values; hanging-punctuation and vertical-text residuals are accounted under shodo-9an.1 and shodo-3v2. Any residual field without an owner fails closed (`unmapped: <field>`). It is a representative caller step; it does not paint, place positioned boxes, or run a BFC.
2. **Ownership table**: field -> owner (14 fields plus `outline_offset` and the four extra decoration fields, and the two sibling-issue groups), noting that owners come from `field_rules` in `raikiri-style-diagnostics.json` and that `outline_offset` / extra decoration fields were added because they are not in the frozen reset list.
3. **Evidence and reproduce**: the exact CLI command and the `cmp`/JSON checks from Task 2 Step 4-5, the WPT revision, comparison SHA256, raikiri pin; results (documents, blocks, unmapped 0, per-owner counts from the JSON; per-field counts equal to the committed classification). State the local inputs are not in CI; CI runs only `cargo test -p shodo-raikiri --example style_handoff`.
4. **Semantics and limits**: `out_of_flow` for absolute/fixed keeps values and is not a drop; a node's own solid underline converts to `shodo::style::TextDecoration` for `PaintStyle.underline`, propagation to inline descendants and single drawing of a shared glyph are the existing caller path (`raikiri_contracts.rs`, v7f) and are not re-proved here; non-solid / overline / line-through are rejected. No claim about box painting correctness, z-order rendering, clip, gradients, whole-page layout, native/baseline comparison, WPT PASS/FAIL, image verdicts or cutover necessity; all stay with the S4 adoption decision / `shodo-p2m.6`.
5. **Deferred**: actual background/outline/gradient painting, relative-offset and stacking application, float/clear placement through Taffy `FloatContext` and shodo's `FloatCursor`/`LineConstraint`, overflow clipping. Neither S4 spike was changed.

- [ ] **Step 2: Update `raikiri-style-diagnostics.md`**

Change the `shodo-0zm` row's scope cell to say the representative caller step now retains these values per owner (link `raikiri-style-handoff.md`) while painting/placement and production adoption still wait for the S4 adoption policy. Keep the cutover-status wording accurate; do not claim the dependency is satisfied or decided.

- [ ] **Step 3: Update `docs/README.md`**

Add: `| Representative caller ownership split of flow/paint CSS | [raikiri style handoff](records/raikiri-style-handoff.md) |` in the neighbouring table's style.

- [ ] **Step 4: Full verification**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace 2>&1 | grep -E "FAILED|error" || echo "no failures"
git status --short
```

Expected: fmt/clippy silent, "no failures", only the planned files changed. If clippy reports a pre-existing warning in a file this branch does not touch, confirm with `git diff origin/main --stat` and report it rather than fixing it.

- [ ] **Step 5: Commit**

```bash
git add docs/records/raikiri-style-handoff.md docs/records/raikiri-style-diagnostics.md docs/README.md
git commit -m "docs: record raikiri flow/paint style ownership handoff and limits"
```
