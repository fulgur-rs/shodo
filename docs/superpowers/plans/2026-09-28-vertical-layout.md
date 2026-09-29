# 縦書き Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `shodo-unc.2` の全scopeをshodoコアと公開glyph出力に実装する。

**Architecture:** shaping itemでgraphemeの向きを保持し、HarfrustのTTBと水平shapingを切り替える。
論理advanceは共通line engineへ渡し、runの変換情報で物理glyph描画を可能にする。
縦中横は内部水平shapingと外部の1em Unitを区別する。

**Tech Stack:** Rust、既存ICU4X/Harfrust/skrifa、tiny-skia fixture renderer。

**Spec:** `docs/superpowers/specs/2026-09-28-vertical-layout-design.md`

## Global Constraints

- MSRVは1.89.0。production dependencyを追加しない。
- raikiri統合spikeはマージせず、`shodo-p2m.6` には着手しない。
- 独立worktreeは `/home/mitz/Work/oss/shodo/target/worktrees/shodo-vertical`、branchは `feat/vertical-layout`。
- baseは `1aa7e5d4900382b7306d11fe91e52670a359f119`。変更前workspace584テスト成功の記録を引き継ぐ。
- 全5 mode/3 orientation/TCY/vhea/vmtx/vert/vrt2/centralを実装する。paint-only回転で代替しない。
- 各taskをnativeで実行する。全task後にbranch全体の独立レビューを一度行う。
- ユーザーの自律実装指示を適用する。文書を個別に人間レビュー済みとは記録しない。
- 一度に一つのCargoのみ。共通env: `TMPDIR=/home/mitz/Work/oss/shodo/target/rust-tmp CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/mitz/Work/oss/shodo/target/first-line-contract CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 RUSTFLAGS='-D warnings'`。
- Cargoは `cargo +stable ... --offline`。長い出力は所有ledger/vertical-artifactsへ保存する。

## Review Focus

1. mark/selectorと透明TextSource境界の向きがbaseから分離しない（Task 1）。
2. sideways-lrとvertical-lrのline-over/物理回転を混同しない（Task 2/4）。
3. shared glyph/owned overlay/small window/first-lineで縦originが二重適用されない（Task 2/3）。
4. TCYのbox境界・部分width-feature coverage・内部caretでsourceが欠落しない（Task 3）。
5. 変更前horizontal26 snapshotsとworkspace584の意味を維持する（Task 4）。

---

### Task 1: graphemeの向きをshaping itemへ伝える

**Files:** Create `src/shape/orientation.rs`; Modify `src/analysis/itemize.rs`, `src/shape.rs`, `src/paragraph.rs`。

**Interfaces:**
- Consumes: `WritingMode`, `TextOrientation`, ICU `CodePointMapData<VerticalOrientation>`、既存grapheme window。
- Produces: `RunOrientation { Horizontal, Upright, SidewaysClockwise, SidewaysCounterClockwise }`。
  `orientation::resolve(mode: WritingMode, orientation: TextOrientation, base: char) -> RunOrientation`。
  `ShapeItem.orientation: RunOrientation`。
  `itemize(input, styles, bidi, breaks, fonts, mode: WritingMode) -> Vec<ShapeItem>`。
  shaping budget/reshape時に同属性を引き継ぐ。

- [x] **Step 1: Write failing itemization regression** real ParagraphBuilderで `a§b` をprepareし、HorizontalTbは1 item、VerticalRl/Mixedは3 itemとassertする。
  `§` はU、Latin a/bはR。同じscript/fontのorientation差がrun結合を止めることを検証する。
- [x] **Step 2: Run RED** `cargo +stable test --lib mixed_vertical_orientation_cuts_shaping_items --offline -- --nocapture`。
  Expected: assertionで実際のitem数1と期待3が異なる。
- [x] **Step 3: Implement classifier and item metadata** modeをitemizeに渡す。
  ICU分類はgrapheme先頭で一度決定し、style分割したmarkにも引き継ぐ。
  orientationをshape.rs内の二つのShapeItem clone/mergeに渡す。
- [x] **Step 4: Add orientation matrix and boundary regressions** 全5mode/3orientation、U/Tu/Tr/R、base+mark/VS16、透明nodeを検証する。
  Expected: literal orientation/rangeが一致し、horizontal compatibility/Arabic既存testが維持される。
- [x] **Step 5: Verify and commit** `cargo +stable test --workspace --offline`。
  Expected: 全workspace成功。`feat: classify vertical grapheme orientation` をcommitする。
  `task-done`も同じworkspace commandを使う。

### Task 2: 縦shaping、baseline、公開glyph transform

**Files:** Modify `src/shape.rs`, `src/shape/{orientation,features,instance,cache}.rs`,
`src/line/{font_metrics,metrics}.rs`, `src/output.rs`, `src/geometry/mod.rs`, `src/lib.rs`。
Create `tests/vertical.rs`;必要な固定font table fixtureを既存fixture生成工程に追加。

**Interfaces:**
- Consumes: Task 1 `ShapeItem.orientation`。
- Produces: ShapedRunのorientation/scale metadata、公開 `GlyphOrientation`、
  `GlyphTransform { inline_x, inline_y, block_x, block_y: f32 }`。
  `GlyphRunView::orientation() -> GlyphOrientation`, `glyph_transform() -> GlyphTransform`。
  `GlyphRunView::glyph_origin(index: usize) -> Option<(f32, f32)>` は、`Line::used_direction()` がRTLで
  orientationが `Combined` でない場合だけ、inline_positionにGlyphStoreの元のshaping advanceを加える。
  TCY（`Combined`）とSidewaysLr/LTRでは加算しない。物理inline軸の符号やcomputed directionだけで判定しない。
  layout spacing込みの `Glyph.advance`、CSS trackingやjustificationを再加算しない。
  この条件は2026-09-29（`shodo-hz6`）に[公開ガイド](../../vertical-layout.md)と現行実装に合わせて訂正した。
  以下の完了チェックと過去の検証記録は当時のまま保持する。
  `PhysicalConverter::point(inline: f32, block: f32) -> (f32, f32)`、
  `vector(inline: f32, block: f32) -> (f32, f32)` と逆point変換。
  glyph originにmatrixを加えphysical point/vectorを合成する。

- [x] **Step 1: Write actual-font failing tests** cjkの句読点vert glyph、TTB advance、vmtx top bearing、Latin font欠落時合成、
  VORG、VVAR、vhea global metrics、明示vert disable/vrt2 enable、vkrn disableを検証する。
  `cargo +stable test --test vertical --offline -- --nocapture`。
  Expected: 水平advance/glyphのままでassertが失敗する。
- [x] **Step 2: Implement TTB and axis normalization** direction/features/instanceをcacheに渡す。
  y advanceをinlineへ、縦originをlogicaloffsetへ一度変換する。runbudget/overlay/windowへ同じ情報を伝える。
  Expected: 上記font testが成功し、font fallbackとresource limitを維持する。
- [x] **Step 3: Write failing baseline/transform matrix tests** 全modeとLTR/RTL、mixed sizes/fonts、atomics、VerticalAlignを検証する。
  Expected: central位置、outline局所(1,0)/(0,1)の物理方向が独立したliteral表と異なる。
- [x] **Step 4: Implement baseline metrics and public contract** 直立em中央とsideways alphabeticを分ける。
  line-over対応、runmetrics、point/vector/inverse、public docs/exportsを実装する。
  Expected: matrixとbaseline test成功。水平glyphをRTLでmirrorしない。
- [x] **Step 5: Write and pass interaction regressions** upright Arabic isolated/LTR、mixed Arabic joining、marks、shared cluster、
  planned/cache/fresh、float retry、first-line、小さなbyte/glyph予算、font-size-adjustを検証する。
  Expected: accepted geometry/glyph/transform/sourceが一致する。
- [x] **Step 6: Verify and commit** `cargo +stable test --workspace --offline`。
  Expected: 全workspace成功。`feat: shape and expose vertical glyph runs` をcommit、同commandでtask-done。

### Task 3: 縦中横のcompositionとsource mapping

**Files:** Create `src/analysis/combine.rs`; Modify `src/analysis/{mod,whitespace,transform,itemize,units,bidi,breaks}.rs`,
`src/paragraph.rs`, `src/shape.rs`, `src/shape/features.rs`, `src/line/{spacing,spacing_summary,intrinsic,metrics}.rs`,
`src/output.rs`, `src/hit.rs`、`tests/vertical.rs`。

**Interfaces:**
- Consumes: Task 1 orientation、Task 2 transformとTTB/水平shaping、Processed items/offset map。
- Produces: `RunOrientation::Combined`、`CombineSpan { text: Range<u32>, item: u32, em: f32 }`、
  `combine::prepare(processed, styles, mode) -> Vec<CombineSpan>` とcomposition単位のUnit/paint mapping。
  external Unitは1em、内部glyphは水平advanceのまま。transformに圧縮scaleを含める。
  composition idはsource-preserving spansであり、glyph ownershipを一つのnodeへ偽装しない。

- [x] **Step 1: Write TCY failing public tests** 1/2/3/4/長い数字、1em width、短い内容の中央配置、全width feature/部分coverage、
  fullwidth逆変換、mark、fallback、LTR/RTL内部bidiを検証する。
  `cargo +stable test --test vertical combine --offline -- --nocapture`。
  Expected: 外側advanceとglyph transformが期待値と異なる。
- [x] **Step 2: Implement preprocessing and horizontal composition** boxのlookaroundとwhitespace/hardbreak処理を先に準備する。
  applicable font feature coverageを確認し、1emへ圧縮、letter-spacing無視、外側内部break禁止。
  Expected: 単純TCY matrix成功、horizontal/sideways modeでは無効。
- [x] **Step 3: Write boundary and ownership failures** `12<span>34</span>`、空box、別All祖先、複数TextSource、
  composition先頭末尾空白、forcedbreak、複数style/font、line-edgebreakclass、内部caret/selectionを検証する。
  Expected: source byte/grapheme範囲、glyph描画一回、lookaroundが一致しない。
- [x] **Step 4: Connect Unit costs and mapping** 外側spacing/emphasisはU+FFFC単位、breakclassは内容のedge。
  intrinsic/planned/cache/float/first-lineと所有overlayへ同じcompositionを渡す。
  Expected: fresh/cached/planned結果一致、内部caretのsource offsetが復元できる。
- [x] **Step 5: Verify and commit** `cargo +stable test --workspace --offline`。
  Expected: 全workspace成功。`feat: compose text-combine-upright runs` をcommit、同commandでtask-done。

### Task 4: 公開API描画と全gate

**Files:** Modify `dev/fixtures/examples/support/glyph_paint.rs`、
`dev/fixtures/examples/support/snapshot_cases.rs`, fixture snapshot/geometry/public docs/README。

**Interfaces:**
- Consumes: 公開Task 2 transform/point/vector、Task 3 TCY metadataのみ。
- Produces: new vertical snapshots、再生成可能SFNT、実行済みgate記録、PR。

- [x] **Step 1: Write renderer failure regression** horizontalと縦glyphの非対称outlineで90°/-90°/直立/圧縮を検証する。
  Expected: 現rendererは縦origin/向きを処理せずpixel位置が異なる。
- [x] **Step 2: Implement renderer and new cases** vertical-rl/lr、sideways-rl/lr、mixed、upright、TCY画像とgeometryを追加する。
  Expected: 実glyphが意図した向きであり、既存26画像とgeometryは変更なし。
  実PNGをview_imageで確認し、その結果を記録する。
- [x] **Step 3: Run final gates** fmt、workspace、Clippy、doc、MSRV1.89 workspace、wasm32 check、no-default/features、
  allocator、snapshot matrix/Python、SFNT再生成、benchmark。
  commandは各既存harnessのhelp/CI定義に合わせ、exact commandとexitをfinal-gates.jsonへ記録する。
  Expected: 全gate成功。MSRVは専用target dir、snapshot出力は新規所有directoryを使用する。
- [x] **Step 4: Commit and task-done** `feat: render and verify vertical layouts` をcommit。
  task-doneは `cargo +stable test --workspace --offline`、Expected: 全workspace成功。
## Final branch review and integration（Task 4 の実装完了後）

- [ ] **Step 5: Whole branch review and integration** 独立reviewは一度だけ。重大指摘の一回fix passは各RED→GREENと全workspace成功を伴う。
  exact HEADをpushしてPR作成、全CIがsuccessとなったexact HEADをmerge。
  Expected: PR MERGED、origin/mainにmergeを含む、issue CLOSED、所有worktree/local branch削除済み。
  全Ruling/cost/deferred minorsを報告し、ledgerをartifactへ保存して所有ledger directoryを削除する。
