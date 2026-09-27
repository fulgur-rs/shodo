# S2 Text Analysis and Shaping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-p2m.3 with IFC-wide CSS analysis and real-font harfrust shaping, preserving the foundation contracts.

**Architecture:** Keep immutable Paragraph/Line and the existing item/mapping model. Separate preprocessing, transform, break analysis, itemize, shape, and line-edge overlays; share font data and retain bounded per-context scratch/plans.

**Tech Stack:** Rust 1.89.0/edition2024, ICU4X2.3 compiled data, unicode-bidi0.3, harfrust0.12, skrifa0.44, shared shodo-fixtures.

**Spec:** `docs/superpowers/specs/2026-09-27-shodo-s2-text-shaping-design.md`; foundation `docs/superpowers/specs/2026-09-26-shodo-foundation-design.md`.

## Global Constraints

- Rust1.89.0, edition2024, unsafe禁止, wasm32, Send+SyncのParagraph/Line。
- max_shaping_run_bytes既定64KiB、max_shaped_glyphs既定2^22、max_reshape_window_bytes既定4096。確保前に検査。
- Glyph SoAは1回保持、Lineはshared rangeまたはowned bounded overlay。offsetはUTF-8 bytes。
- Existing default system-fonts/web-fonts; complex-scripts default on, off gives grapheme breaks for complex scripts.
- CI exact PR HEAD success before merge; exclude shodo-p2m.6; no reduction of S2 issue scope.

## Review Focus

1. Node splits inside words/graphemes must preserve contextual forms, single glyph ownership, all DOM offsets; Task4/6 pin cross-node Arabic and ligature mapping.
2. Alternating styles, controls and OOF must not make whitespace/transform/segmentation quadratic; Task1/2/3 pin long transparent-boundary inputs and counting/structural linear passes.
3. Extreme glyph expansion, zero/tiny run windows, large clusters must remain bounded and terminate; Task4/5 pin tiny budgets, aggregate first-line output and overflow.
4. RTL mark attachment and overlays with changed glyph counts must match public iterator positions, not just private IDs; Task4/5 pin direct shaper comparison and public source iteration.
5. Missing fonts, invalid lang, complex-scripts off and first-line with block interruption must degrade explicitly without data loss; Task2/3/6 pin each fallback and restore behavior.

---

### Task 1: IFC whitespace, segment breaks and bidi paragraph boundaries

**Files:** Modify `src/analysis/whitespace.rs`; Create `src/analysis/whitespace_context.rs`; Modify `src/analysis/mod.rs`.

**Interfaces:** Consumes `RawItem`, `InlineStyle`, Limits/Mapping. Produces existing `process(raw_text:&str,raw:&[RawItem],styles:&[InlineStyle],with_mapping:bool,limits:&Limits)->Result<Processed,LimitExceeded>` with exact scalar origin mappings; helper `whitespace_flags(raw_text:&str,raw:&[RawItem],styles:&[InlineStyle])->Vec<u8>` indexed by raw byte offset, REMOVE flag for discarded scalars.

- [x] Add failing unit tests: `segment_break_neighbors_cross_nodes` expects `日 \n 本`→`日本` across Open/Close/OOF; `hangul_keeps_segment_space` expects `한\n글`→`한 글`; `zero_width_space_removes_adjacent_break` expects `a\u{200B}\nb`→`a\u{200B}b`; `preserve_breaks_removes_surrounding_collapsible_spaces` expects `a \t\n \tb`→`a\nb`.
- [x] Add `dom_carriage_return_is_space` expects Preserve `a\r\nb`→`a \nb`, Collapse→`a b`; `bidi_scopes_restart_at_paragraph_boundaries` expects controls close before LF/U2029 and reopen after; map each generated control to owner node.
- [x] Add `segment_mapping_and_limits` checks removed LF/space maps to zero text interval, kept text roundtrip, generated controls exceeding max_text_bytes/item limit return Err; `transparent_boundaries_are_linear` uses 4096 spans/OOF and asserts correct transformed text and bounded item count.
- [x] Run `cargo test --offline -p shodo analysis::whitespace` and observe assertion failures against existing code.
- [x] Implement context flags in forward/backward linear scans, respecting Atomic/Block/ForcedBreak barriers and default-ignorables; update Processor to use flags, CSS CR→space, preserve collapsing state across controls, and close/reopen scopes at breaks.
- [x] Run above targeted tests then `cargo test --offline --workspace`, compare results and commit `feat: process IFC segment breaks and bidi boundaries`.

### Task 2: Language-sensitive text transform and offset composition

**Files:** Create `src/analysis/transform.rs`, `src/analysis/transform_context.rs`; Modify `src/analysis/mod.rs`, `src/style.rs`, `src/mapping.rs`, `Cargo.toml`.

**Interfaces:** Consumes Task1 `Processed`; produces `transform(input:Processed,styles:&[InlineStyle],limits:&Limits,warnings:&mut WarningSink)->Result<Processed,LimitExceeded>`. Add combination TextTransform variants while retaining all existing variants. Add internal `OffsetMapping::remap_text(&[TransformSpan])` with `TransformSpan{old:Range<u32>,new:Range<u32>}` scalar ranges.

- [x] Add tests `turkic_case_cross_node_dot`, `lithuanian_above_marks`, `greek_final_sigma_and_tonos`, `dutch_capitalize_ij_cross_node`, `capitalize_preserves_tail_and_word_context`, `width_kana_combinations`, `halfwidth_kana_voicing`, `expanded_scalar_dom_roundtrip`, `transform_without_mapping`, `invalid_language_warns`, `transform_limits_before_append`.
- [x] Exact cases: tr `I\u0307 ıi` lowercase→`i ıi`, uppercase→`I\u0307 Iİ`; lt `I\u0301` lowercase→`i\u0307\u0301`; `ΟΣ ΟΣΑ` lowercase→`ος οσα`; nl capitalize `ijSSEL`→`IJSSEL`; uppercase `ß`→`SS` one indivisible Expanded interval; full-width `A ｶﾞ`→`Ａ　ガ`; full-size-kana U1B132→こ. Check all existing variant combinations and CJK-preserved spacing.
- [x] Run targeted test command and observe missing transform APIs/assertion failures.
- [x] Add icu_casemap2.3/icu_locale_core2.3/icu_segmenter2.3 and optional needed normalization data; use public ICU scalar APIs and precomputed contextual SpecialCasing conditions, no repeated whole-word scans. Update original-origin mapping after transformed item ranges; append checks before allocation.
- [x] Run `cargo test --offline --workspace`, fmt/Clippy; commit `feat: apply locale-sensitive text transforms with mappings`.

### Task 3: Grapheme, bidi paragraph and CSS break analysis

**Files:** Create `src/analysis/breaks.rs`, `src/analysis/bidi.rs`; Modify `src/analysis/units.rs`, `src/analysis/mod.rs`, `src/paragraph.rs`, `Cargo.toml`.

**Interfaces:** `analyze_breaks(processed:&Processed,styles:&[InlineStyle],warnings:&mut WarningSink)->BreakAnalysis{graphemes:Vec<u32>,opportunities:Vec<BreakOpportunity>}`. `BreakOpportunity{offset:u32,class:BreakClass,min_content:bool}`; extend BreakClass Prohibited/Allowed/Mandatory/Emergency/Hyphen. `analyze_bidi(text:&str,style:&ParagraphStyle,styles:&[InlineStyle])->BidiAnalysis{levels:Vec<u8>,paragraphs:Vec<BidiParagraph>}` with resolved per-paragraph base levels and plaintext inline direction; keep block coordinate direction independent.

- [x] RED tests `strictness_and_locale`, `break_all_keep_all_anywhere`, `nowrap_no_soft_breaks`, `overflow_wrap_intrinsic_distinction`, `manual_soft_hyphen`, `nbsp_word_joiner_zwj_graphemes`, `complex_scripts_feature_modes`, `plaintext_multiple_paragraph_directions`, `bidi_fast_path_conditions`, `many_styles_shared_segmenter_passes`.
- [x] ICU options: Auto/Normal→Normal, Loose/Strict/Anywhere matching; ja/zh locale; WordBreak BreakAll/KeepAll. Keep complete graphemes even Anywhere; retain internal transform-expanded ranges independently of optional DOM mapping and prohibit breaks inside them (test ß→SS with mapping on/off). SHY class only for manual/auto. AutoPhrase→Normal+warning and HyphensAuto→Manual+warning.
- [x] Configure complex-scripts default on and icu_segmenter auto/LSTM data via feature, compiled_data without auto when off. Group options without rerunning whole IFC once per style; context across boundaries retained.
- [x] Run workspace tests with default and no-default features, compare public text/token behavior, commit `feat: analyze graphemes and CSS line break opportunities`.

### Task 4: Font/script itemization, real shaping and retained context

**Files:** Create `src/analysis/itemize.rs`, `src/shape/features.rs`, `src/shape/cache.rs`; Modify `src/shape.rs`, `src/context.rs`, `src/style.rs`, `src/paragraph.rs`, `src/builder.rs`, `src/output.rs`, `src/analysis/units.rs`; tests in `dev/fixtures/tests/shaping.rs`.

**Interfaces:** `itemize(processed:&Processed,styles:&[InlineStyle],bidi:&BidiAnalysis,breaks:&BreakAnalysis,fonts:&FontCollection)->Vec<ShapeItem>`; ShapeItem text range/owner slices/script/level/style/FontMatch. `shape_items(cx:&mut LayoutContext,items:&[ShapeItem],processed:&Processed,styles:&[InlineStyle],fonts:&FontCollection,limits:&Limits,warnings:&mut WarningSink)->Result<(GlyphStore,Vec<ShapedRun>),LimitExceeded>`. `Paragraph::from_builder(b:ParagraphBuilder,cx:&mut LayoutContext,fonts:&FontCollection)` uses all prior outputs. ShapedRun retains variations/normalized coords/synthesis/script/lang/features; GlyphStore adds glyph flags.

- [x] RED public fixture tests `paragraph_matches_direct_arabic_shaper`, `latin_ligatures_features_and_kerning`, `font_fallback_per_grapheme`, `node_split_preserves_arabic_joining`, `cross_node_ligature_single_owner_and_mapping`, `common_inherited_script`, `rtl_mark_positions_and_visual_order`, `language_feature_precedence`, `missing_font_notdef_warning`, `plan_cache_reuse_and_shrink`, `run_byte_limits_and_giant_grapheme`, `shaped_output_limit_before_retention`, `font_layer_survives_owner_drop`.
- [x] Add FontVariantLigatures/Caps/Numeric/EastAsian/Position/Alternates style fields and flatten to features with user-feature last precedence. Tag ranges relative to run. Expose run coords/synthesis, account FontMatch variations/opsz/size-adjust.
- [x] Use S1 matcher per ICU grapheme, Common/Inherited script resolution, shared ShaperData and bounded64-entry ShapePlan cache keyed by FontId+instance+ShapePlanKey properties. Reuse UnicodeBuffer and make LayoutContext !Sync. Honor pre/post context and identical-style node transparency. Real missing-font glyph ID0 with deterministic fallback advances.
- [x] Normalize RTL cluster groups while preserving intra-cluster positioning; units consume actual cluster ranges and flags, not scalar-count assumptions. Public output reverses only logical cluster groups as needed, avoiding double reversal.
- [x] Split max64KiB at grapheme boundaries; force scalar progress for giant cluster/tiny limits with warning. Check aggregate output glyph count before SoA append, split pen before2^30. Release retained scratch based on limits/shrink.
- [x] Run workspace default/no-default suites, direct harfrust comparison for fixed corpus, record short cold/warm measurements without adding word cache; commit `feat: shape paragraphs with matched fonts and OpenType features`.

### Task 5: CSS break consumption, variable overlays and discretionary hyphens

**Files:** Modify `src/analysis/units.rs`, `src/paragraph.rs`, `src/line/scan.rs`, `src/line/cache.rs`, `src/line/intrinsic.rs`, `src/line/plan.rs`, `src/line/reshape.rs`, `src/line/fragments.rs`, `src/output.rs`, `src/shape.rs`; fixture tests `line_shaping.rs`.

**Interfaces:** `GlyphSource::Overlay{glyphs:Range<u32>}` independent from shared glyph range; `reshape_window(data:&ParagraphData,text:Range<u32>,edge:ShapeEdge,cx:&mut LayoutContext)->Option<ReshapedWindow>` complete cluster range/new GlyphStore. RecordKind::Glyphs retains item/run/text and actual-source glyph range. Break consumers use Task3 classes and min_content distinction. Selectable source slices inside one real shaping cluster share an immutable SharedCluster descriptor and prefix advances measured with bounded real shaping; opaque BreakToken unit indices select these slices. Retain original ShapeItem scalars/context for line windows so transparent anchors/controls never become glyph input. Unbroken public glyph/cluster ownership remains one shared source; selected partial windows own their actual glyphs and text spans. Widths for continuation prefixes are remeasured from the actual line start, and cache/plans/intrinsics consume the same source slices.

- [x] RED `emergency_break_only_when_needed`, `break_word_vs_anywhere_min_content`, `soft_hyphen_only_when_taken`, `unsafe_break_reshapes_both_sides`, `overlay_changed_glyph_count_public_iterators`, `overlay_budget_retains_whole_cluster`, `tiny_window_progress_and_warning`, `rtl_overlay_attachments`, `overlay_source_clusters_and_justification_positions`.
- [x] Select normal/Hyphen breaks before emergency; include hyphen advance while choosing break. Re-shape unsafe edges to safe boundaries with UNSAFE_TO_CONCAT join validation and bounded expansion. Match direct harfrust line-window results with original pre/post context for Arabic soft-break joining; forced breaks stop context, and split ligatures are reshaped.
- [x] Move glyph/cluster access to actual source, support different overlay glyph count and all public iterators/get/ExactSizeIterator. Keep shared Paragraph unchanged and ownership stable; no missing/duplicate cluster at budget fallback. Intrinsic/balance/pretty/cache recognize all break classes consistently.
- [x] Run targeted fixture line tests and workspace suite, commit `feat: consume CSS breaks and reshape bounded line edges`.

### Task 6: First-line sets, adversarial budgets and complete public regression coverage

**Files:** Modify `src/paragraph.rs`, `src/builder.rs`, `src/line/mod.rs`, `src/output.rs`, `src/limits.rs`, `src/line/tests.rs`; add fixture tests `first_line.rs`, `analysis_limits.rs`.

**Interfaces:** ParagraphData retains `first_line:Option<Arc<FirstLineData>>` with transformed text/items/mapping/styles/glyphs/runs/units; main and alternate offset/token correspondence from source positions. Build budgets sum all raw/processed bytes and glyphs; first_line selection uses BreakToken flags and discontinues after BlockInInline.

- [x] RED `first_line_features_transform_and_fonts`, `first_line_then_normal_token_correspondence`, `block_in_inline_disables_first_line`, `first_line_aggregate_text_glyph_limits`, `tiny_limit_combining_sequence_terminates`, `out_of_flow_cross_node_cluster_no_duplicates`, `shrink_zero_releases_layers_and_buffers`, `all_corpus_public_glyph_ids_have_font_data`.
- [x] Implement alternate first-line set with exact source token correspondence, no copying whole Paragraph per line, no first-line restart after forced blocks. Sum alternate resources before retention and obey warning caps. Replace obsolete stub-ID expectations with meaningful real-font or explicit missing-font tests; preserve all float rollback/cache/progress checks.
- [x] Confirm root/fixture graph remains unchanged except development APIs; no normal dependency on shodo-fixtures. Run entire workspace default/no-default/complex-feature combinations and property-style cases, commit `feat: preserve first-line analysis and shaping resource contracts`.

### Task 7: Documentation, verification, final review and integration

**Files:** README.md, `dev/fixtures` docs/examples/tests, CI as needed; Create `docs/harfrust-shaping-status-proposal.md` (draft only, no external message).

**Interfaces:** Consumes complete public behavior of Tasks1–6; produces reproducible validation and exact-HEAD integration evidence.

- [x] Update examples to remove S0 shaping caveat only once real corpus glyphs pass. Document locale/unknown font/AutoPhrase/automatic hyphenation degradation, complex-scripts mode, cache/budget policy, first-line and public coords/synthesis.
- [x] Record fixed-font cold/warm shaping measurements, retained plan reuse and working-set shrink; explain word-cache decision and upstream failure-status draft.
- [ ] Verify `cargo fmt --all --check`, Clippy workspace all-targets warnings deny, stable/MSRV1.89 workspace, root no-default and explicit complex-scripts mode, wasm-p shodo, rustdoc workspace, Python5 and fixture asset check.
- [x] Package whole branch with skill review-package and dispatch one fresh most-capable reviewer. Regrade findings by user effect, ledger all rulings/deferred minors; fix Important/Critical once with reproduced RED→GREEN and full suite.
- [ ] Push feat/s2-text-shaping, create PR, verify exact current HEAD all CI success, merge matching HEAD, close shodo-p2m.3, preserve decision record, remove clean worktree/branch, proceed to bd ready next issue.
