# 日本語横書き組版 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `shodo-unc.1` の禁則、約物詰め、ぶら下がり、日本語inter-characterを行組み全体に実装する。

**Architecture:** ICUのbreak analysisと既存spacing summaryを拡張する。
新しいpunctuation moduleが文字分類・font由来のblank・行端の差分を共通化し、
scan/cache/planned/intrinsicと採用行のglyph配置に同じ判断を適用する。

**Tech Stack:** Rust、ICU4X、skrifa、既存shaping/fixture fonts。

**Spec:** `docs/superpowers/specs/2026-09-28-japanese-horizontal-design.md`

## Global Constraints

- MSRV 1.89.0。新たなproduction dependencyを追加しない。
- raikiri統合spikeはマージせず、`shodo-p2m.6` に着手しない。
- 四項目すべてを実装し、paint-onlyの補正やcache回避で置き換えない。
- 段落準備は線形。候補評価で全文再走査やvisual reorderを追加しない。
- 所有workspaceは `/home/mitz/Work/oss/shodo/target/worktrees/shodo-japanese`、branch `feat/japanese-horizontal`、base `48a2aea9b2a65b2cb4a3ffadfaf2465362ee020d`。
- TMPDIRは `/home/mitz/Work/oss/shodo/target/rust-tmp`。一度に一つのCargoを実行する。
- 実行方法はnative。各taskを自身で実装し、最後にbranch全体の独立レビューを一度実施する。
- ユーザーのissue自律実装の指示を適用する。個別の文書承認を得たとは記録しない。

## Review Focus

1. CSSとICUのnormal/strict差：ICUの名前だけで準拠とせず要求matrixを直接検証する（Task 1）。
2. glyphのfallback/variation/proportional advance：primary faceのemを流用してinkを壊さない（Task 2）。
3. DOM境界と共有cluster：sourceは保持し、glyphの補正は一回だけ（Task 2）。
4. float/retry/first-line/planned/cache：同じaccepted lineのmeasureとinkが一致する（Task 2）。
5. 日本語とLatin/cursive混在：和文禁則をLatin全体に適用せず、結合文字内部を分離しない（Task 3）。

## ファイルと責務

- `src/analysis/breaks.rs`: CSS strictness tailoringとtable-driven unit tests。
- `src/style.rs`: TrimBoth/TrimAll/Autoと各値の公開説明。
- `src/line/punctuation.rs`（新規）: punctuation分類、実instanceのsafe blank、共通line-edge adjustments。
- `src/line/{spacing_summary,spacing,scan,cache,plan,intrinsic,mod}.rs`: 同じ境界コストの接続。
- `src/{paragraph,output}.rs`: prepared metadataとstart/end hang量の保持。
- `src/line/align.rs`: 日本語のjustification opportunity判定。
- `tests/japanese.rs`（新規）: public APIからのmetric/position/range/hit/intrinsic/retryの検証。
- `tests/{break_plan,alignment}.rs`: 既存契約との回帰検証。
- `dev/fixtures/examples/snapshot_cases.rs`、`dev/fixtures/snapshots/`: 固定fontでの画像とgeometry。
- `docs/japanese-layout.md`（新規）: 呼び出し方、仕様pin、判断規則と制限。

### Task 1: CSS禁則差を検証して不足をtailorする

**Files:** Modify/Test `src/analysis/breaks.rs`。

**Interfaces:**
- Consumes: `analyze_breaks(...) -> BreakAnalysis`、既存testsの `analyze(text, style, dom)`。
- Produces: 同じ `BreakAnalysis` とAPI。新しい公開APIは不要。

- [x] **Step 1: Write matrix tests** `css_japanese_strictness_matrix`。
  `ja/zh` の `日〜本`、`日゠本` はNormal/Looseでbyte3にAllowed、StrictでProhibited。
  `日ぁ本`、`日ー本`、`日々本`、`日…本` はNormal/StrictでProhibited、LooseでAllowed。
  Looseの `日・本`、`日：本`、`日％本`、`＄日本` と他言語での差を検証する。
  `日‐本`/`日–本` のID先行と `a‐b` の非ID先行を分ける。
  各rowは `class` と `min_content` を検証する。
- [x] **Step 2: Run RED** `cargo +stable test --lib css_japanese_strictness_matrix -- --nocapture`。
  未対応rowがassertion failureになることを記録する。既に通るrowは変更しない。
- [x] **Step 3: Implement missing CSS tailoring** ICU結果を既存のtypographic boundaryで補正する。
  strictness/lang/ICU LineBreak/EastAsianWidthの判定を使う。
  nowrap、anywhere、keep-all、mandatory、transform indivisibleの優先順位を維持する。
- [x] **Step 4: Add and pass contract regressions** inline境界にまたがる本文、ja-JP/zh-Hant、
  langなし、combining mark、nowrap/anywhere/keep-allをmatrixへ追加する。
  `cargo +stable test --lib analysis::breaks::tests` と既存workspaceが通る。
- [x] **Step 5: Commit** `feat: enforce CSS Japanese line-break strictness`。

### Task 2: 約物詰めとぶら下がりを共通コスト・配置に実装する

**Files:** Create `src/line/punctuation.rs`, `tests/japanese.rs`。
Modify `src/style.rs`, `src/paragraph.rs`, `src/output.rs`,
`src/line/{mod,spacing_summary,spacing,scan,cache,plan,intrinsic}.rs`。

**Interfaces:**
- Consumes: `ParagraphData` のunits/runs/font data/style、`LayoutUnit`、`Saturation`、`BreakToken`。
- Produces in `punctuation.rs`: `PunctuationClass` enum、`Punctuation` prepared metadata、
  `EdgeAdjustment { start_trim, end_trim, hang_start, hang_end: LayoutUnit }`。
  `classify(ch: char, language: Option<&str>) -> PunctuationClass`。
  `build(data: &ParagraphData, sat: &mut Saturation) -> Vec<Punctuation>`。
  `boundary(data: &ParagraphData, left: usize, right: usize) -> i64`（raw signed cost）。
  `edges(data: &ParagraphData, start: usize, end: usize, flags: u8,
  options: &LineOptions, available: LayoutUnit, natural: LayoutUnit,
  sat: &mut Saturation) -> EdgeAdjustment`。
  準備metadataが境界のfirst/last unitを持つため、edgesはrange再走査しない。
  `Line::hang_start()/hang_end()` は既存signatureで実量を返す。

- [ ] **Step 1: Write failing public layout tests** Normal/SpaceAll/TrimStart/SpaceFirst/
  TrimBoth/TrimAll/Autoのopening、closing、middle、adjacent pairs。
  全角advance16でtrim可能なblank8の場合、`「「日` のNormalは40、SpaceAllは48、
  TrimAllは32。行頭trimはglyph inkも8だけ移動する。font-sizeの等号と大小を別々に検証する。
  実font testsでは固定fontの観測値から期待advance/inkを導き、font_sizeを期待値の代用にしない。
- [ ] **Step 2: Run RED** `cargo +stable test --test japanese -- --nocapture`。
  未実装behaviorで失敗することを記録する。
- [ ] **Step 3: Implement prepared metadata and interior costs** specの分類・隣接表を実装し、
  実run/instanceでfullwidth/blankを判定する。proportional faceはno-trim。
  summaryとselected spacingの両方にsigned costを接続し、glyph leadingとadvanceを分ける。
- [ ] **Step 4: Write failing edge/hanging tests** first16の対象はhang_start16、
  force-end16はhang_end16、allow-endはnatural40/available36ならhang_end4、
  available40なら0。trim後の対象8はforce-end8であり16ではない。
  first/last、first U+3000、ASCII quotes、CSS stop list、padding blocker、
  強制/soft改行、min/maxのconditional hang差を検証する。
- [ ] **Step 5: Implement common edges** scan/cache/selected/intrinsicに同じルールを接続する。
  cacheのfrontierコストにも反映し、first/after-forcedを渡す。
  accepted lineにstart/end hangを保持し、alignment前にcontentを減じてinkを移動する。
  trailing whitespaceとの合成とhit/source ownershipを保持する。
- [ ] **Step 6: Add and pass interaction regressions** mixed style、fallback、variation、
  proportional、mark/shared cluster、RTL visual edges、inline border/padding、
  floatで幅変更、height rejection/retry、first-line、planned/greedy一致。
  long punctuation paragraphのcandidate visitsが線形の既存上限内であることを検証する。
  `cargo +stable test --test japanese --test break_plan --test intrinsic --test fragments --test floats`
  とworkspaceを通す。
- [ ] **Step 7: Commit** `feat: integrate punctuation trimming and hanging into line layout`。

### Task 3: 日本語両端揃えと画像による統合検証

**Files:** Modify `src/line/align.rs`, `tests/japanese.rs`, `tests/alignment.rs`,
`dev/fixtures/examples/snapshot_cases.rs`, `dev/fixtures/snapshots/`。
Create `docs/japanese-layout.md`。

**Interfaces:**
- Consumes: Task 2 `PunctuationClass`/prepared metadata、既存alignment `Point`/`Opportunity`。
- Produces: `punctuation::justify_boundary(data: &ParagraphData, left: u32, right: u32) -> bool`。
  left/rightはtypographic source offset。内部・cluster間の双方が同じ分類を使う。
  既存public Lineとtext_justify APIは変更しない。

- [ ] **Step 1: Write failing justify tests** `日「日本」語` は括弧の両側に追加空きを置かず、
  `日本語` は二箇所に均等分配する。comma/stop/middle/hyphen/和字間隔の両側、
  連続dash/ellipsisを分離しない。Latinの明示inter-characterとcursive/marksの既存契約を維持する。
- [ ] **Step 2: Run RED** `cargo +stable test --test japanese japanese_justification -- --nocapture`。
- [ ] **Step 3: Implement opportunity filter** cluster間と圧縮cluster内部の双方に反映し、
  last-lineと機会なしfallbackを既存のalignmentで処理する。
- [ ] **Step 4: Run GREEN** Japanese/alignment/bidi/text_transform testsとworkspaceを通す。
- [ ] **Step 5: Add fixed-font snapshots** trim、first/end hanging、inter-characterの各caseを追加する。
  通常checkで既存default Normalの差分を取得する。PNG/geometryを目視して意図した差のみ
  full updateし、通常read-only checkを再実行する。仕様判断と例をdocsへ記す。
- [ ] **Step 6: Commit** `feat: preserve Japanese punctuation boundaries during justification`。

## 最終検証と統合

- [ ] fmt、stable/MSRV workspace、Clippy `-D warnings`、doc、wasmと既存dev gatesを実行する。
  CI workflowを読んで対象を列挙し、結果をartifactに記録する。
- [ ] whole-branchのfresh reviewerを一度だけ依頼する。重要指摘があればRED→GREENで修正して全gateを実行する。
- [ ] pushしてPRを作成し、exact HEADに対する全required CI成功を確認する。
- [ ] PRをmergeし、remote mainにcommitが含まれることを確認して `bd close shodo-unc.1`。
- [ ] ledgerと検証記録を保存後、所有worktree/local branchを片付け、`bd ready` で次のissueへ進む。

## 自己レビュー

specの四項目はTasks 1–3で覆う。conditional hang、font安全性、cache/plan/intrinsic、
glyph leadingとsource契約はTask 2に明記した。Review Focusの各項目に対応testがある。
新規interfaceの型は上記で統一し、製品実装に入る前に既存型とpreflightする。
