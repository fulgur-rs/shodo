# Paint styles implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for native task-by-task implementation. One fresh whole-branch review follows all tasks.

**Goal:** Retain solid text color and underline/strike-through through accepted glyph and source-span output, with caller PNG evidence.
**Architecture:** InlineStyle owns resolved PaintStyle, paint stays outside shape compatibility, source spans reuse hit-index grapheme geometry and actual run metrics. Rasterization remains a caller example.
**Tech Stack:** Rust2024, MSRV1.89.0, existing Skrifa/Harfrust/tiny-skia fixtures; no new dependency.
**Spec:** docs/superpowers/specs/2026-09-28-paint-styles-design.md

## Global Constraints

- Input color is non-premultiplied sRGB RGBA8; default opaque black, no decorations.
- Decoration offset positive toward line-under; None uses actual font instance metrics. Thickness nonnegative, zero paints nothing; optional nonfinite lengths become None with warnings, finite magnitudes clamp to1e7px.
- Glyph paint belongs once to the first scalar's source. Paint-only boundaries do not split shaping.
- Span rects include block_offset, preserve accepted dataset/source ranges and final bidi/justification geometry. Indivisible source graphemes may overlap, no invented caret cuts.
- CSS decorating-box propagation/skip-ink and general rasterization remain caller responsibilities; public docs explain the boundary.
- No raikiri spike edits/merge or shodo-p2m.6 implementation. Existing snapshots remain unchanged.
- Autonomous user instruction covers native implementation and normal PR/CI-success merge; documents are self-reviewed, not human-approved.

## Review Focus

- Nonfinite/huge decoration lengths must not poison output or intern comparisons (Task1 sanitizer assertions).
- Shared combining graphemes with several source styles must not vanish or acquire caret cuts (Task2 source overlap assertions).
- Visible SHY overlays must keep source paint and actual fallback metrics (Task2 narrow SHY assertions).
- Vertical-lr/sideways and TCY axes must keep decoration attached to accepted text rather than container baseline (Task2 modes).
- Dropping context/fonts/paragraph must retain drawable output and paint ownership (Task3 lifetime/pixel assertions).

### Task 1: Retained paint input and glyph ownership

**Files:** src/style.rs, src/sanitize.rs, src/paragraph.rs, src/output.rs; create dev/fixtures/tests/paint_styles.rs; update docs/first-line-style-contract.md and related public comments.
**Interfaces:** Produces style::PaintStyle { color:[u8;4], underline:Option<TextDecoration>, strikethrough:Option<TextDecoration> }, TextDecoration { color:Option<[u8;4]>, offset:Option<f32>, thickness:Option<f32> }; InlineStyle.paint; GlyphRunView::paint_style(&self)->&PaintStyle. Task2 consumes all.

- [x] Step1 Write tests `default_and_explicit_paint_survive_both_builders`, `paint_boundaries_preserve_ffi_owner_and_arabic_joining`, `first_line_retains_resolved_and_legacy_paint`, `invalid_decoration_lengths_warn_and_normalize`. Assertions: default [0,0,0,255]/none; explicit [200,10,20,128], underline [0,0,255,255]/offset3/thickness2, strike offset-4/thickness1; ffi split f/f/i produces one glyph, owner0 and red even with blue successors; Arabic split glyph tuples equal one-span output; first accepted line green and later red at width60 with child-resolved blue on first line; NaN offset/inf thickness ->None and negative thickness ->0 with warnings; 2e7 ->1e7.
- [x] Step2 Run `cargo +stable test -p shodo-fixtures --test paint_styles --offline`.
  Expected: FAIL missing public PaintStyle/TextDecoration/paint_style API; record log.
- [x] Step3 Implement exact interfaces and normalization, extend first_line_properties with paint, keep shaping_compatible untouched; update accepted first-line docs.
- [x] Step4 Run the same focused command, then `cargo +stable test --workspace --offline`.
  Expected: all pass, no snapshot changes.
- [x] Step5 Run `cargo +stable fmt --all --check`, commit `feat: retain source paint styles through glyph output`; task-done command is the workspace test.
  Expected: terminal0 and task ledger complete.

### Task 2: Source spans and resolved decoration geometry

**Files:** create src/output/paint.rs, bridge src/hit/mod.rs, register exports src/lib.rs/output.rs; extend dev/fixtures/tests/paint_styles.rs and vertical tests as appropriate.
**Interfaces:** Consumes Task1 styles/getter. Produces Line::paint_spans()->Vec<PaintSpan<'_>>, PaintSpan public node/text_range/style/rect/font/font_size/metrics and underline()/strikethrough()->Option<DecorationRect>; DecorationRect public rect/color. Reuse crate-private finalized segments bridge from hit index. Rect coordinate convention exactly spec.

- [x] Step1 Write `source_spans_partition_ffi_and_resolve_real_metrics`: three intervals0..1/1..2/2..3 with nodes0/1/2, literal fixture GDEF boundaries checked against existing direct oracle and sum equals run width; underline color/position/thickness equals actual independent Skrifa metrics; explicit offset3/thickness2 rectangle centered baseline+3, strike-4/thickness1. Write `multiline_bidi_justification_and_whitespace_spans`: independently compare source coverage and rectangles to finalized selections at wraps/bidi/justified lines, tabs/internal preserved spaces remain, collapsed whitespace does not resurrect. Write `fallback_adjusted_metrics_and_first_line_spans`: actual selected-font metrics and size are retained, first-line paint styles follow accepted alternate text data. Write `combining_sources_and_shy_keep_paint`: grapheme overlapping intervals and unchanged caret cuts, visible SHY source style/metric; atomics excluded. Write `vertical_sideways_and_combined_decorations`: RL/LR normal offsets have correct sign, combined horizontal decoration follows accepted internal baseline/scale and source region axis, all logical rectangles finite.
- [x] Step2 Run focused paint_styles test.
  Expected: FAIL missing paint_spans/DecorationRect.
- [x] Step3 Implement ordered source/run intersection with hit source rectangles, actual metrics, tabs containing shift and orientation-specific decoration axes. Keep painted glyph count/IDs and hit behavior unchanged.
- [x] Step4 Run focused test then workspace tests.
  Expected: all pass including original snapshots.
- [x] Step5 Fmt check, commit `feat: expose source decoration spans in final layout`; task-done workspace test.
  Expected: terminal0 and task ledger complete.

### Task 3: Public painter example and complete gates

**Files:** create dev/fixtures/examples/paint_styles.rs and docs/paint-styles.md, register example test in dev/fixtures/Cargo.toml; extend dev/fixtures/examples/support/glyph_paint.rs with retained mode; README links. Legacy snapshot painter behavior stays unchanged.
**Interfaces:** Consumes Task1 glyph paint and Task2 rectangles; example uses accepted Line/PhysicalConverter and actual retained glyph font data only, RichText input.

- [x] Step1 Write example test `retained_paint_draws_colors_and_both_decorations`: glyph counts equal accepted once-only output; after fonts/context/paragraph drop PNG has opaque red/blue glyph pixels and exact independently selected underline/strike pixels with explicit green/yellow lengths; transparent text honors alpha. Tests also verify source style differs on a shared glyph without a second paint. Add multiple-line/RTL or vertical representative pixel evidence and output clipping checks.
- [x] Step2 Run `cargo +stable test -p shodo-fixtures --example paint_styles --offline`.
  Expected: FAIL missing renderer implementation.
- [x] Step3 Implement example with public output, write docs for all input/output/default/source/first-line/whitespace/orientation/fallback/lifetime contracts and caller CSS/skip-ink/overflow responsibilities. Add README link, run example and inspect PNG.
- [x] Step4 Run workspace tests, root no-default/all-feature tests, stable/MSRV1.89 tests/check, fmt, clippy workspace all-targets, rustdoc -Dwarnings, wasm no-default check and existing Python/fixture validators exactly matching repository CI.
  Expected: terminal0 on every gate, existing snapshots unchanged. Preserve source/fixture hashes and logs.
- [x] Step5 Commit `feat: demonstrate retained text colors and solid decorations`; task-done workspace tests.
  Expected: terminal0 and task ledger complete.

## Final phase

- [x] Fresh whole-branch most-capable reviewer once, one verified Critical/Important fix pass, defer minor findings with costs, archive evidence before own scratch removal.
- [ ] Push ordinary branch, create PR and require exact-head CI success before authorized merge. Read back merged state/tree, close shodo-ods, remove owned worktree/local branch after remote auto-delete, then select next ready implementable issue excluding shodo-p2m.6.
