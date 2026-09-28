# Accessibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete shodo-1bn with backend-independent retained accessibility output and a usable optional AccessKit bridge, without editor/IME dependencies.

**Architecture:** Borrow accepted Lines and their single cached hit index; emit source-ordered selectable characters and actual attributes. A persistent optional adapter exports those units to AccessKit, with caller semantics and IDs; a real consumer validates the integration.

**Tech Stack:** Rust1.89, existing ICU2.3/geometry/mapping, accesskit=0.24.1, accesskit_consumer=0.38.0, checked-in fixture fonts.

**Spec:** docs/superpowers/specs/2026-09-28-accessibility-design.md

## Global Constraints

- Preserve root main7225194 and unmerged raikiri spike3ef5acf; shodo-p2m.6 excluded.
- Worktree target/worktrees/shodo-a11y, branch feat/accessibility, base ca3c7e1b055280d84c3bb239a5e86d0cd6a3482f. Baseline671passed/0failed/51binaries already verified.
- Core API always available; accesskit=0.24.1 optional/default-features=false; development consumer0.38.0 only behind fixture feature. No OS backend/editing dependency.
- Copy exact fields/signatures from the spec. No scalar/glyph approximations, reshaping, snapshot changes or fake DOM offsets.
- Source inverses are one-to-many; failed adapter exports retain all previous current-position/identity state. All positions are snapshot-scoped.
- Reuse existing ICU auto vs non-complex-script policy; segmentation uses accepted logical text, not separately split runs/lines.
- Cargo environment: TMPDIR=/home/mitz/Work/oss/shodo/target/rust-tmp, CARGO_BUILD_JOBS=1, CARGO_TARGET_DIR=/home/mitz/Work/oss/shodo/target/first-line-contract, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_INCREMENTAL=0, RUSTFLAGS='-D warnings'. Long output goes to owned SDD/artifact logs.
- Native execution/TDD; one fresh whole-branch reviewer only after all tasks. One Critical/Important RED→GREEN fix pass plus full suite; exhaustive ruling/cost and deferred minor reporting before scratch deletion.

## Review Focus

- First-line and normal UTF-8 datasets differ in length: use each accepted Line's own text and source map (Task1 first_line_uses_its_own_utf8_dataset).
- Nonpainting controls, hard breaks and empty lines: preserve every accepted byte and only actual caret boundaries (Task1 nonpainting_breaks_tabs_controls_and_empty_lines).
- A grapheme spans source/style boundaries: keep it one selectable unit with first-scalar attributes and full source mapping (Task1 indivisible_sources_and_large_graphemes_remain_whole).
- Reflow leaves stale positions/removed IDs: reject them, retain source anchors and stable start identities (Task1 reflow_rejects_stale_positions_and_resolves_source_anchors; Task2 adapter_reuses_ids_and_rejects_removed_positions_after_reflow).
- A selectable unit exceeds255bytes/export fails: core remains valid, adapter reports an error and previous successful state remains usable (Task2 failed_exports_preserve_last_successful_state).

## File responsibilities

- Create src/accessibility/mod.rs: public output/position types and facade.
- Create src/accessibility/output.rs: accepted-character geometry, actual attribute grouping and word starts.
- Modify src/hit/mod.rs: crate-private cached index access only; src/lib.rs: module export.
- Create src/accessibility/accesskit.rs and src/accessibility/accesskit/{nodes,positions}.rs: SDK bridge, tree construction and current position/identity state; no platform integration.
- Create dev/fixtures/tests/accessibility.rs and accessibility_accesskit.rs: actual font-backed core/consumer contract tests.
- Modify Cargo.toml/dev/fixtures/Cargo.toml: optional exact dependency/features and example configuration.
- Create dev/fixtures/examples/accessibility.rs and docs/accessibility.md; modify README.md/.github/workflows/ci.yml for usage and explicit feature gates.

### Task 1: Retained core output and conversions

**Files:** src/accessibility/{mod,output}.rs, src/hit/mod.rs, src/lib.rs, dev/fixtures/tests/accessibility.rs.

**Interfaces:** Consumes Line, hit::LineLayout::{caret,selection_rects,hit_test}, OffsetMapping::{text_to_dom,dom_to_text}, actual Fragment/GlyphRunView attributes. Produces every always-available core type/method in the spec, including AccessibleLine.word_starts; borrowed lifetimes originate in accepted Lines. A crate-private hit index bridge returns actual caret offsets and cached segment geometry without rebuilding per character.

- [ ] **Step 1: Write real fixture tests first.** Breaks caught: wrong source/visual order, glyph/scalar units, dataset aliasing, control loss, invented cuts, stale positions, incorrect source normalization and TCY axes. Test names/assertions:

```rust
// logical_reading_order_and_actual_attributes
assert_eq!(layout.logical_text(), "a سلام b"); // source order; Arabic actual font and bidi odd
// ligatures_emoji_and_combining_use_selectable_characters
assert_eq!(cuts, [0,1,2,3,14,17]); // ffi👩‍💻a\u{0301}
assert_close(ffi_widths[0], 5.04); assert_close(ffi_widths[1], 5.056);
// source_positions_normalize_collapsed_and_expanded_text
assert_eq!(layout.logical_text(), "SS A"); // raw "  ß a", uppercase/collapse
assert_eq!(units, ["SS", " ", "A"]); // processed1 Upstream→0 / Downstream→2
// first_line_uses_its_own_utf8_dataset
assert_eq!(layout.logical_text(), "S\nb"); // raw ſ\nb, first-line uppercase
assert_eq!(line_ranges, [0..2,3..4]); // DOMnode42 start10, b sourceoffset13
// nonpainting_breaks_tabs_controls_and_empty_lines
assert_eq!(hard_break_count, 2); assert_eq!(tab_count, 1); // a\n\nb\tc, isolate controls retained
assert!(empty.position(0,0,Affinity::Downstream).is_none());
// positions_and_selection_follow_final_bidi_geometry
assert_eq!(selected_rects.len(), 2); // ab سلام cd, char1→6 excludes RTL م
// reflow_rejects_stale_positions_and_resolves_source_anchors
assert!(narrow.to_text_position(old).is_none()); // a b c, width1000→16
assert!(new_source_positions.iter().any(|p| p.line==1 && p.character==0));
// indivisible_sources_and_large_graphemes_remain_whole
assert_eq!(characters.len(),1); assert_eq!(characters[0].text.len(),401); // a+200combining acute, split sources
// vertical_and_combined_characters_keep_physical_axes
assert_eq!(tcy.leading.0,tcy.trailing.0); // 12 TCY; block sign RLnegative/LRpositive
// word_boundaries_cross_soft_wraps_and_attributes
assert_eq!(starts, [0,6]); // hello world; style split hello, softwrap/chunks add none
// retained_lines_outlive_layout_owners
assert_eq!(layout.logical_text(), "ab"); // paragraph/context/font owners dropped before accessibility
```

- [ ] **Step 2: Run** `cargo +stable test -p shodo-fixtures --test accessibility --offline`. Expected: RED because accessibility module/API absent; then interface scaffold and rerun to observe runtime assertions fail where needed. Save logs.
- [ ] **Step 3: Implement exact core facade/output.** Create one LineLayout; its real cached offsets bound units. Choose first text scalar's source/style; actual glyph font/size/bidi/orientation from overlapping accepted output, including SHY overlays. Atomics use accepted unit/fragments. Nonpainting geometry retains bytes. Normalize conversions with caret, use accepted line map independently, preserve source-affinity and one-to-many inverse ambiguity. Compute ICU word starts once across accepted logical text and map to character units; reject invalid positions and snapshot mismatch.
- [ ] **Step 4: Run** same focused command. Expected: all11named tests pass; run `cargo +stable fmt --all -- --check` and `cargo +stable clippy --workspace --all-targets --offline -- -D warnings`. Expected: exit0.
- [ ] **Step 5: Commit** `feat: expose retained accessibility text and positions`. Run task-done with `cargo +stable test --workspace --offline`. Expected: complete suite exit0; all named tests present and all Expected assertions compared to real results.

### Task 2: Optional AccessKit tree and real consumer

**Files:** Cargo.toml, dev/fixtures/Cargo.toml, src/accessibility/accesskit.rs, src/accessibility/accesskit/{nodes,positions}.rs, dev/fixtures/tests/accessibility_accesskit.rs.

**Interfaces:** Consumes Task1 AccessibleLayout/Line/Character/Run/Position/Selection/SourcePosition. Produces AccessKitAdapter::{new,update,to_position,from_position}, NodeSemantics and AccessKitError exactly as spec; reexport SDK as `accessibility::accesskit::types`. Error variants CharacterTooLong, InvalidFrame, InvalidSelection, DuplicateNodeId. Root feature accesskit=[dep:accesskit], fixture feature accesskit=[shodo/accesskit,dep:accesskit_consumer].

- [ ] **Step 1: Write consumer tests first**, feature-gated file. Breaks caught: schema byte counts, missing text/geometry/selection, scalar atomics, wrong newlines, stale IDs and nontransactional failed updates. Required names/assertions:

```rust
// consumer_reads_logical_text_geometry_and_selection
assert_eq!(document.document_range().unwrap().text(), "ffi سلام b");
assert_eq!(selected.text(), "fi"); // actual consumer selection and glyph caret boxes
// consumer_preserves_hard_breaks_and_first_line_datasets
assert_eq!(document.document_range().unwrap().text(), "S\nb"); // softwrap adds no LF
// atomic_alternatives_and_caller_roles_are_exposed
assert_eq!(document.document_range().unwrap().text(), "a写真b");
assert_eq!(alternative.character_lengths(), &[6]); // one unit, width20 height12
// adapter_reuses_ids_and_rejects_removed_positions_after_reflow
assert_eq!(new_first_node, old_first_node); // source-start key, no line number
assert!(adapter.from_position(removed,Affinity::Downstream).is_none());
// failed_exports_preserve_last_successful_state
assert!(matches!(huge,Err(AccessKitError::CharacterTooLong))); //401byte unit
assert!(matches!(nan,Err(AccessKitError::InvalidFrame)));
assert!(matches!(stale_selection,Err(AccessKitError::InvalidSelection)));
assert!(matches!(duplicate,Err(AccessKitError::DuplicateNodeId)));
assert_eq!(adapter.to_position(old_position), previous); // each error leaves state
// adapter_chunks_long_runs_without_splitting_characters
assert_eq!(chunk_counts, [255,255,90]); //600a, word starts onlyfirstchunk
// consumer_geometry_follows_rtl_vertical_and_tcy
// actual consumer bounding boxes, directions/coordinate origins match accepted carets
// consumer_word_navigation_crosses_chunks_and_soft_wraps
assert_eq!(next_word_text, "world"); // hello world with style/line/chunk split
```

- [ ] **Step 2: Add only optional dependency/config and run** `cargo +stable test -p shodo-fixtures --features accesskit --test accessibility_accesskit`. Expected: RED missing adapter API; runtime RED after scaffold; use real pinned consumer, not mocks. SDK/API source already inspected; download matching dependencies normally if cache lacks them.
- [ ] **Step 3: Implement** adapter. Build prospective nodes/registry/maps locally; publish state only on success. Use active source-start identity+occurrence keys, caller fresh allocator (do not retain all retired IDs indefinitely), reject current/new/root collisions. Split <=255characters; each byte length<=255. Same-line links, logical children, actual physical direction/positions/widths with frame origin. Caller role/label/description wrappers and atomic one-unit alternative child. Export font/language/solid paint/decorations where represented by SDK. Canonical hardbreak LF/endcaret; word starts preserved across splits. Standalone Tree/focus=root; host embedding documented caller responsibility.
- [ ] **Step 4: Run** feature-focused test above plus `cargo +stable clippy --workspace --all-targets --features shodo-fixtures/accesskit --offline -- -D warnings`. Expected:8named tests pass, Clippy0.
- [ ] **Step 5: Commit** `feat: add optional AccessKit layout adapter`. task-done `cargo +stable test --workspace --features shodo-fixtures/accesskit --offline`. Expected: complete suite exit0.

### Task 3: Reproducible integration, docs and CI

**Files:** dev/fixtures/examples/accessibility.rs, dev/fixtures/Cargo.toml, docs/accessibility.md, README.md, .github/workflows/ci.yml.

**Interfaces:** Consumes Task1 core and Task2 adapter+real consumer. Produces runnable `cargo run -p shodo-fixtures --features accesskit --example accessibility`, read-only document text/range/bounds and selection-action conversion output; no platform/editor setup.

- [ ] **Step 1: Write example consumer contract test** `read_only_example_routes_selection_actions_to_sources`: construct real Tree, request/set a selection via AccessKit TextSelection/action payload, convert focus/anchor to shodo source endpoints and print selected text/bounds. Assert selected `fi` and original DOMbyte offsets1/3. Run feature example command. Expected: RED absent example; add implementation and run again Expected: GREEN with selected=fi/source offsets1/3 and nonempty physical bounds.
- [ ] **Step 2: Document** all core fields/methods and caller responsibilities, source non-bijection, per-line datasets, reflow/IDs, absent mapping, word policy, error rollback/255byte restriction and standalone/host integration. Link example. Feature CI must run actual consumer tests/example on stable and MSRV1.89. No prose-text tests.
- [ ] **Step 3: Run final gates**: fmt, workspace all-target Clippy default and AccessKit, stable workspace default and feature, MSRV1.89 workspace default and feature, no-default and complex-scripts core tests, wasm32-unknown-unknown build default/no-default/accesskit, rustdoc warnings denied, existing snapshot/allocator/benchmark/assets checks matching CI. Expected: every command terminal0, all snapshots unchanged. Preserve source hashes and all logs.
- [ ] **Step 4: Commit** `docs: demonstrate accessibility integration and add feature CI`. task-done `cargo +stable test --workspace --features shodo-fixtures/accesskit --offline`. Expected: all consumer/core/example tests exit0.
- [ ] **Step 5: Whole branch review** using review-package and one fresh most-capable reviewer; review all5Focus inputs and ledger rulings. Regrade by user impact; one Important/Critical RED→GREEN fix pass+fullsuite. Record/report every ruling with cost and every deferred minor before owned scratch deletion.
- [ ] **Step 6: Authorized finish** push feat/accessibility, create PR referencing issue18/shodo-1bn, inspect exactHEAD allCIterminalsuccess, merge, verify tested-tree equality/ancestry, close bdissue and verify closed, archive owned artifacts, remove owned worktree/local branch using normal deletion. Remote branch auto-deletion is repository policy; verify. Preserve root main and raikiri spike; then bd ready for next eligible issue.
