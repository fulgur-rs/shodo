# Emoji Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Complete shodo-7zg with reproducible real emoji fixtures, sequence/boundary/fallback verification and a working caller color renderer.

**Architecture:** The dev fixture crate adds opt-in color and mono fonts without changing its original three-font loader. Core accepted glyph outputs drive CBDT PNG drawing; existing outlines draw surrounding text. Correctness defects found by real-font tests receive core regressions and fixes.

**Tech Stack:** Rust1.89+, skrifa0.44, tiny-skia0.12, harfrust0.12, FontTools4.61.1.

**Spec:** docs/superpowers/specs/2026-09-28-emoji-support-design.md

## Global Constraints

- Keep root dependencies unchanged; system font discovery disabled.
- Original FONTS/load_fonts/cases registrations and original font checksums unchanged.
- Pin color aac7ccaa4d1dea4543453b96f7d6fc47066a57ff and mono23e54b51ddffbc7713c583748e3bd86f62b1fa4a; FontTools4.61.1; total assets <=768KiB.
- Both added faces use family Shodo Fixture Emoji; color then mono IDs.
- No shaping in bitmap drawing; core glyph origins/advances/baselines authoritative.
- Whole grapheme boundaries retained even for missing or unsupported sequences.
- S4 spike untouched/unmerged; exclude shodo-p2m.6; existing push/PR/CI-success merge authorization applies.

## Review Focus

- Combining emoji components split across DOM nodes must retain both source anchors and one editable grapheme (Task2).
- A nominally covered but uncomposed ZWJ sequence must not be counted as composed or split during selection (Task2).
- Generic-family fallback and explicit Text/Emoji presentation must choose the same instance later painted (Tasks1/2).
- Corrupt/oversized PNG dimensions and off-canvas placement must fail before uncontrolled allocation or partial drawing (Task3).
- Vertical upright placement and intrinsic bitmap colors must follow glyph transforms while preserving caller text colors (Task3).

### Task 1: Reproducible real fonts and opt-in loader

**Files:** Modify dev/fixtures/tools/regenerate.py, tools/test_regenerate.py, src/lib.rs, assets/manifest.json, README.md. Create assets/emoji-cases.json, fonts/emoji-color.ttf, fonts/emoji-mono.ttf, licenses/noto-color-emoji-OFL.txt, licenses/noto-emoji-OFL.txt, tests/emoji_fixtures.rs.

**Interfaces:** Consumes existing load_fonts(&Limits)->Result<FixtureFonts,FontError> and FixtureCase. Produces EMOJI_FONTS, EmojiFixtureFonts { base:FixtureFonts, emoji_ids:[FontId;2] }, load_emoji_fonts(&Limits)->Result<EmojiFixtureFonts,FontError>, emoji_cases()->&'static[FixtureCase]. font(id) includes new fixtures. Optional manifest corpus defaults to assets/cases.json.

- [x] Step1: Add Python behavioral tests: separate emoji corpus is actually used in rebuild; over-budget offline check and update rejected without mutation; original IDs remain first and licensed metadata validates. Add real loader consumer test `opt_in_fonts_shape_real_color_and_text_faces` asserting emoji and VS15/16 selected-font bytes, color tables/mono outlines and no missing-font warnings. Base loader still has original three faces.
- [x] Step2: Run `python3 -m unittest discover -s dev/fixtures/tools -v` and `cargo +stable test --offline -p shodo-fixtures --test emoji_fixtures`; expect new asset/loader/corpus/budget failures before implementation. Distinguish unavailable APIs from actual runtime failures; the real color paint runtime RED belongs to Task3.
- [x] Step3: Implement per-font corpus reading and 768KiB checks in regenerate.py; add pinned corpus/assets/licenses/manifest. Rebuild all five from original sources using FontTools4.61.1, preserving all GSUB features and original three byte hashes. Implement opt-in loader/interface in src/lib.rs. Update README provenance/budget and inspect variable names.
- [x] Step4: Run Python suite with FontTools; asset check; all-five rebuild without mutation; stable workspace suite. Expected all pass; exact original three hashes unchanged and total <=786432 bytes.
- [x] Step5: Commit `feat(fixtures): add pinned real emoji fonts and opt-in loading`.
- [x] Step6: Task-done command runs Python suite, asset check and stable workspace suite; every named test present and pass.

### Task 2: Real sequence, source, editing and fallback contracts

**Files:** Create dev/fixtures/tests/emoji_layout.rs and test utilities if needed. Modify src/font/matching.rs public documentation; fix any demonstrated core defects in their owning modules with regressions.

**Interfaces:** Consumes Task1 load_emoji_fonts/emoji_cases and existing public Paragraph/Line editing outputs. Produces independent font-table-derived sequence expectations and passing public contract tests; no new core API is planned.

- [x] Step1: Independently inspect cmap/GSUB/hmtx/hhea tables and record glyph IDs/advances for 😀,☺︎,☺️,👍🏽,👩‍💻,👨‍👩‍👧‍👦,🇯🇵,1️⃣,Scotland flag. Literal source lengths respectively4/6/6/8/11/25/8/7/28; source ranges cover entire inputs. Assert each accepted glyph's font_data identity and selected instance; mono weights400/700 remain selectable.
- [x] Step2: Add `compound_emoji_preserve_break_and_edit_boundaries` for normal/emergency widths smaller than one emoji, mixed 日本語/Latin, caret stops, hit testing and selection rectangles. Each representative is indivisible; finite line geometry, independently computed sum of advances and max face ascent/descent, exact UTF8 offsets. Add split-DOM ZWJ/VS/flag tests with both source origins.
- [x] Step3: Add `unsupported_sequence_keeps_one_grapheme` for 😀ZWJ😀 and a test font made by removing GSUB from the real face. Nominal selection remains single face with component output. Add `missing_emoji_warns_and_retains_sources` for old load_fonts and an absent newer emoji: Unsupported warning, glyph0, no component-level breaks or selection stops. Verify fallback through generic families and explicit presentation. Run tests before any core changes; preexisting passes are characterization evidence, not invented RED.
- [x] Step4: For real failures use systematic-debugging, record smallest fix decision, observe runtime RED then GREEN. Run stable workspace and all-feature core suite. Expected all named public behaviors pass with original snapshot cases unchanged.
- [x] Step5: Document nominal selection vs actual composition in matching.rs and commit `test(emoji): verify real sequences, boundaries and fallback contracts` (or fix title if core fixes required).
- [x] Step6: Task-done command runs stable workspace suite and core all-features suite; all named tests pass.

### Task 3: Caller CBDT bitmap drawing, docs and final integration

**Files:** Create dev/fixtures/examples/support/bitmap_paint.rs, examples/emoji_png.rs, tests/emoji_paint.rs, docs/emoji.md. Modify examples/support/glyph_paint.rs, dev/fixtures/Cargo.toml, README.md, .github/workflows/ci.yml.

**Interfaces:** Consumes Task1 fonts, existing GlyphRunView font_data/normalized_coords/font_size/glyph_origin/glyph_transform and PhysicalConverter. Existing paint entrypoints retain signatures. A private bitmap helper consumes accepted FontRef/GlyphId/font size/physical transform/canvas and returns drawn-or-no-bitmap Result; expose errors through existing PaintError.

- [ ] Step1: Add `real_color_glyph_paints_intrinsic_pixels` using actual accepted glyph outputs through existing try_paint/try_paint_styled_on_canvas. Run it and observe MissingOutline failure for a real CBDT font. Add origin/glyph-ID relocation tests with literal pixel bounds derived from bitmap bearings/ppem, plus vertical transform, caller text colors/decoration, clipping, malformed PNG, metrics mismatch, unsupported encoding/synthesis and nonfinite placement cases.
- [ ] Step2: Implement only caller bitmap decoding/placement using skrifa BitmapFormat::Cbdt and PNG decoding (existing transitive dependency may need explicit dev-dependency). Normalize colors to premultiplied RGBA; validate output bounds/dimensions before allocating. Transform bitmap y-down local bearing coordinates without outline y-flip. Preserve core advance/baseline and reject invalid/off-canvas images. Existing outlines remain fallback for glyphs without bitmap.
- [ ] Step3: Add runnable emoji_png example with fixed corpus, explicit output path, actual selected-font/glyph output and visible intrinsic colors. Run `cargo +stable run --offline -p shodo-fixtures --example emoji_png -- OUTPUT.png` and inspect PNG; run same on MSRV. CI runs example on check and msrv jobs. Docs describe all sequence limitations, .notdef warning behavior, byte/instance/coordinate handoff, actual CBDT PNG support and COLR/CPAL/sbix/SVG caller responsibility.
- [ ] Step4: Run full verification matrix from spec (stable/MSRV workspace and AccessKit, core feature modes, wasm, fmt/Clippy/doc, allocator/bench, fixed snapshots, Python/assets/all-five reproduction, color example). Expected terminal0 all gates; original snapshot fixtures unchanged.
- [ ] Step5: Commit `feat(examples): render accepted color emoji bitmaps`.
- [ ] Step6: Task-done command runs stable workspace suite; record all additional gate commands/results in ledger and root artifacts.
- [ ] Step7: One fresh whole-branch review with spec, plan, Rulings and Review Focus. Regrade by effect, one Important/Critical runtime RED→GREEN fix pass, defer Minors. Push branch, create PR, require exact-head check/msrv/wasm success, merge, verify tree/head identity, close issue. Archive/report exhaustive Rulings/minors before owned worktree/branch cleanup; pick next bd ready issue excluding shodo-p2m.6.

## Author self-review

Spec coverage: fixed assets/interface Task1, selection/sequence/boundaries/source/fallback Task2, formats/actual color/sample/docs Task3, final matrix/review/workflow Task3. Shared interface names and three original IDs agree. Review Focus inputs mapped to owning task tests. Plan is author-reviewed under existing autonomous authorization, without asserting human review. Reusable gates may run via a task-owned script with explicit commands recorded in artifacts.
