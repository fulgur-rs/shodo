# S3 横書きの行組み implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** `shodo-p2m.4` の全項目を、実フォントの最終配置と一致する横書き行組み・hit/navigation 出力として完成させる。

**Spec:** `docs/superpowers/specs/2026-09-27-shodo-s3-horizontal-layout-design.md`

**Architecture:** immutable ParagraphData に解決済み metrics と spacing metadata を保持し、candidate/cache/intrinsic/plan と選択 Line の placement が同じ規則を使う。`hit::LineLayout` は受理済み Line を借りて最終配置を索引する。

**Tech:** Rust1.89+, ICU2.3, harfrust0.12, Skrifa0.44, existing fixed-font fixtures。native inline execution、最後に一回の fresh whole-branch review。

## Global Constraints

- base `52ed6d06e6d178815a608999d259a519702f617b`、worktree `/tmp/shodo-s3`、branch `feat/s3-horizontal-layout`。root checkout を編集しない。
- user の連続 issue 実装/PR/CI/merge/cleanup 承認に従い再承認待ちを設けない。外部 action は session の具体的な承認と automatic approval review に従う。
- S3 の項目を後続 issue へ押し出さない。既存 float/token/first-line/window/resource 契約を保持する。縦書き shaping/ルビ/raikiri production switch は既存 scope のまま。
- no normal library dependency on shodo-fixtures。fixed-font tests と dev-only dependencies は fixture crate。
- natural shaping data は immutable。spacing/justification を他の Line や Paragraph の glyph arrays へ書かない。行ごとの whole Paragraph clone、candidate ごとの full-paragraph reorder、無制限 recursion を導入しない。
- `CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/tmp/shodo-s3-target CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTFLAGS='-D warnings'`。Cargo は逐次、compiler 切替時はこの専用 target の生成物だけ clean。source/log/他 goal artifacts は削除しない。
- 各 task は task-start の brief を読み、実際に RED を確認してから production を書く。全 task-done のコマンド成功を読む。ledger と logs を /tmp に保全する。

## Files and responsibility

- `src/font/metrics.rs`, `font/matching.rs`: query による primary selection、実 font metrics/variation、ch/ic/space metrics。
- `src/shape/instance.rs`, new `src/line/font_metrics.rs`: shaping と同じ instance の metrics resolution と style/run cache。
- `src/line/metrics.rs`, `src/output.rs`: strut/inline/run/atomic の行高さ・baseline・公開 metrics/source。
- new `src/line/spacing.rs`, new `src/line/spacing_summary.rs`: typographic metadata、bounded bidi summary、actual layout spacing。
- `src/line/{scan,cache,intrinsic,plan,align,fragments,windows,hyphen}.rs`: 共通 spacing の候補測定と最終位置反映。
- new `src/hit/{mod,index,selection,navigation}.rs`: public types と借用 LineLayout、caret/selection/navigation。
- `src/analysis/breaks.rs`, `src/paragraph.rs`: source/transform に合法な caret cuts と必要な build-time metadata。
- root tests と `dev/fixtures/tests/{line_metrics,spacing,hit_test,horizontal_contracts}.rs`: fixed-font/public-contract regression。

## Review Focus

1. Bidi candidate spacing を後から見かけだけ補正して、scan/cache/intrinsic/Pretty と final width が異なる危険。Task2/3 の `bidi_spacing_candidates_match_final_geometry` と independently reordered small-prefix oracle、長い prefix の counter test を加える。
2. 行高・inline box に primary 登録順/stub metrics が残り、mixed fallback/variation/size-adjust が無視される危険。Task1 の `mixed_face_and_adjusted_run_metrics_drive_line_box` と全 vertical-align oracle。
3. first-line と normal offsets を混ぜ、expanded/collapsed source や owned ligature/SHY に不正な caret を作る危険。Task4 の `first_line_and_indivisible_transform_carets_use_actual_dataset`、Task6 の window/resource/source cases。
4. bidi double affinity/非連続 selection を単一 rect または単一offsetに潰す危険。Task4/5 の explicit positions と round trips、visual/logical traversal tests。
5. 空行・ゼロ幅・深い inline・長い controls が stop duplication/recursion/quadratic traversal を生む危険。Task5/6 の termination と counter/large-depth tests、小予算の whole-cluster fallback。

### Task 1: Actual font metrics and complete line-box alignment

**Files:** font/{metrics,matching,mod}.rs, shape/instance.rs, new line/font_metrics.rs, line/metrics.rs, paragraph.rs, output.rs, dev/fixtures/tests/line_metrics.rs, root line-box tests。

**Interfaces:** produces resolved style/run metrics and public `GlyphRunView::metrics()`, `FontMetrics::{x_height,cap_height}`, `Cluster::source_char`; keeps next_line and glyph geometry signatures。

- [x] Add fixed-font `normal_line_height_uses_selected_font`, `mixed_face_and_adjusted_run_metrics_drive_line_box`, `parent_x_height_drives_middle_and_text_edges`; compare existing baseline/height to direct Skrifa before changes, run fixture test and confirm numeric RED。
- [x] Add primary-face query without depending on requested character coverage for strut; reuse the same normalized style/variation/size-adjust resolution as shaping. Cache resolved style/run metrics once at build, expose run metrics and processed source_char。
- [x] Replace fixed extents and parent shifts; support all VerticalAlign values, normal and explicit half-leading, actual fallback extents, inline content rect, empty box/tab/br, required atomic baseline and top/bottom subtree placement. Replace recursive ancestor aggregation with iterative cache。
- [x] Add `all_vertical_align_values_match_font_and_atomic_oracles`, `variation_and_decoration_metrics_match_run_instance`, `deep_inline_metrics_do_not_recurse`, empty/zero-height/negative-margin checks. Existing stub-font numeric tests must keep documented fallback results。
- [x] Run workspace, fixture metrics and root line-box tests; commit `feat: resolve horizontal line boxes from actual font metrics`。

### Task 2: Letter/word spacing and actual tab intervals

**Files:** new line/{spacing,spacing_summary}.rs, line/{scan,cache,intrinsic,plan,align,fragments,reshape,windows,hyphen}.rs, paragraph.rs, output.rs, dev/fixtures/tests/spacing.rs, root alignment/lines/intrinsic tests。

**Interfaces:** consumes Task1 style/space metrics; produces one candidate-spacing/placement contract used by all width consumers, with natural versus layout advances preserved。

- [x] Write and run RED `tracking_changes_breaks_and_glyph_advances`, `different_style_tracking_uses_visual_half_spacing`, `word_spacing_and_tabs_use_real_space_metrics`. For real Latin `ab`, tracking2 adds exactly2 total, not4; first/last outside halves absent. Nonzero word spacing is observable on U+0020/NBSP. Spaces tab uses direct selected space advance, Px tab unaffected。
- [x] Build typographic metadata from processed graphemes and markers. Implement bounded bidi-level summaries that combine visual boundary spacing without full-prefix reordering per candidate. Handle consecutive atomics, ignored formatting, grapheme marks, low-level forced ligatures, signed spacing and saturated arithmetic。
- [x] Integrate exact spacing into scan/cache/float positions/intrinsic/Balance/Pretty, including contextual and synthetic SHY widths, hanging spaces, whole-cluster fallback and source continuations. Retain cache frontier/tab re-evaluation bounds。
- [x] Apply the same spacing to shared and owned final positions/Cluster.advance. Compose justification afterwards, preserve marks and immutable other-line positions. Test both block directions and mixed levels。
- [x] Add `bidi_spacing_candidates_match_final_geometry` with independent small-prefix visual oracle; add large alternating-level/style operation-count check. Add cached/cold float/tab retries, negative spacing and explicit ffi feature tests; run full workspace and no-default; commit `feat: apply consistent character and word spacing to line layout`。

### Task 3: Visual inter-script autospace

**Files:** line/spacing.rs, line/spacing_summary.rs, font/metrics.rs, paragraph.rs, dev/fixtures/tests/spacing.rs。

**Interfaces:** consumes Task2 visual boundary summary and Task1 ic metrics; extends the same spacing contract, no text insertion or separate glyph shaping。

- [x] Write/run numeric RED `autospace_uses_visual_classes_and_real_ic` for CJK/Latin and CJK/decimal boundaries, no-autospace comparison, direct 0.125ic expectation; include mixed bidi whose visual neighbor differs from logical neighbor。
- [x] Implement current CSS ideograph/script-extension/EAW/category classification and boundary-owning innermost inline style. Cache ic measure by resolved style; ignore transparent OOF/empty boundaries, stop across nonzero edges, whitespace, punctuation, atomic/tab/forced/block。
- [x] Add `autospace_cross_node_boundary_uses_containing_style`, `autospace_removed_at_soft_wrap`, `autospace_and_tracking_compose_with_justification`, first-line/transformed text, marks/fullwidth/halfwidth classification and float callback tests。
- [x] Verify all width consumers and selected glyph output agree with actual autospace, warm/cold cache and tiny windows preserve source. Run workspace/spacing; commit `feat: add bidi-aware inter-script text autospace`。

### Task 4: Caret index and coordinate hit testing

**Files:** new hit/{mod,index}.rs, lib.rs, output.rs, paragraph.rs, analysis/breaks.rs, dev/fixtures/tests/hit_test.rs。

**Interfaces:** produces `hit::{LineLayout,TextPosition,HitResult,Caret,CaretDirection,NavigationOrder}` per spec. `LineLayout::new(&[Line])`, `hit_test(inline,block)`, `caret(position)` use finalized accepted Line coordinates and datasets。

- [x] Add public tests for glyph midpoint hits, coordinate→position→caret round trip, mapping-disabled operation and bidi double affinity. Confirm missing-API RED; use explicit numeric assertions once API compiles, no placeholder methods counted as completion。
- [x] Retain legal caret cuts excluding indivisible transformed spans at build without requiring DOM mapping. Construct index from final shared/owned clusters, actual advances and atomic/tab/empty boundaries; source node mapping is optional and dataset-specific。
- [x] Support ligature grapheme interiors using available GDEF scaled/variable caret values with documented proportional fallback; no internal mark/ZWJ/expanded-transform stop. Invalid/contour-point unsupported caret data falls back, not panic or per-query reparsing。
- [x] Add `first_line_and_indivisible_transform_carets_use_actual_dataset`, `ligature_and_combining_carets_preserve_graphemes`, `bidi_affinity_has_two_visual_locations`, `justified_and_owned_overlay_hit_geometry_matches_glyphs`, tabs/hanging/SHY/atomic/empty/nonfinite cases. Verify multiple lines use block_offset and line-specific processed offsets。
- [x] Run workspace/hit tests, document public byte/coordinate/affinity contract; commit `feat: expose caret geometry and coordinate hit testing`。

### Task 5: Selection rectangles and logical/visual navigation

**Files:** new hit/{selection,navigation}.rs, hit/mod.rs, dev/fixtures/tests/hit_test.rs, root tests/hit.rs。

**Interfaces:** consumes Task4 index; adds `selection_rects(start,end)` and `move_caret(position,direction,order)` without rebuilding glyph/Paragraph data。

- [x] Write/run RED for RTL discontiguous selection, partial ligature selection, logical-versus-visual mixed-bidi movement and cross-line transitions. Assert exact intervals/stops, not only nonempty output。
- [x] Implement source-order range normalization and visual interval grouping; preserve bidi gaps, zero selection, invalid endpoints, atomic/tab/visible SHY and font/line-height rects. Merge only touching same-line intervals。
- [x] Implement separate logical and visual index traversal, canonical duplicate-position handling, source-order line transition and terminal None. Retain distinct affinity locations without repeated same-location loops。
- [x] Add collapsed/expanded mapping and first-line multi-line selection/navigation, reverse endpoints, zero-width/control-only/forced/block boundaries and dropped Paragraph/FontCollection lifetime cases. Counter test ensures repeated hit/navigation does not rescan glyphs。
- [x] Run workspace and feature-mode tests; commit `feat: add visual selection and caret navigation`。

### Task 6: Full horizontal contract and resource audit

**Files:** dev/fixtures/tests/horizontal_contracts.rs, root tests/{floats,line_box,intrinsic,properties,iteration}.rs, owning production modules as regressions require。

**Interfaces:** consumes Tasks1–5 and existing S0/S2 contracts; adds no alternate layout implementation or renderer dependency。

- [x] Map every issue/foundation S3 requirement to current public tests. Add missing fixed-font coverage for pure repeated next_line, token ownership, unbreakable float lookahead, displaced cursor restoration, page-height rollback, missing atomic sizes and BlockInInline empty/first-line boundaries。
- [x] Verify indent hanging/each-line, tab offsets, all align/text-align-last/justify-all values, hanging preserved spaces, actual min/max (float/first-line), Balance/Pretty actual spacing, glyph data/font/coords/synthesis/metrics/source and no mutation of prior Lines。
- [x] Exercise all adverse numeric inputs through style/constraints/atomic/hit; tiny/max-zero reshape budgets and aggregate glyph budgets; deeply nested inline; long controls/styles/bidi with operation-count assertions; every accepted result progresses through source exactly once。
- [x] Run stable workspace, root no-default and explicit complex-scripts; fix only actual contract regressions with RED/GREEN and full suite. Commit `test: verify horizontal layout and hit resource contracts`。

### Task 7: Documentation, final review and exact-HEAD integration

**Files:** README, public docs/examples, CI if needed, task ledger/review artifact。

**Interfaces:** complete S3 implementation→reviewable branch→verified main integration。

- [x] Document metrics, spacing/autospace, line-specific hit source/affinity, GDEF fallback, selection/navigation, coordinate conversion and ownership. Add a small public-API example without normal fixture dependency; examples must run against the real output and label unsupported renderer responsibilities。
- [x] Verify fmt/Clippy workspace all-targets warnings deny; stable/MSRV1.89 workspace; root no-default/explicit complex; wasm root; rustdoc warnings deny; Python5 and offline fixture check. Confirm normal dependency graph has no fixture/dev renderer dependencies。
- [x] Package whole base..HEAD diff, dispatch exactly one fresh reviewer per requesting-code-review skill. Regrade findings by user effect; one complete Important/Critical RED/GREEN fix pass with full suite; record Minor/rulings/costs, no repeat review。
- [ ] Push branch to approved fulgur-rs/shodo, create PR with actual final behavior/validation, check all CI success on exact latest HEAD, merge matching HEAD using allowed repository method. Verify main contains tested tree, close issue, preserve ledger/logs, remove clean owned worktree/local branch, select next bd ready excluding p2m.6. Remote deletion only if specifically approved; it does not block required worktree cleanup。
