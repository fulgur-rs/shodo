# shodo S0-B: Line Box and Flow Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 仮フォント・仮シェーパーを維持したまま、行高・揃え・float の再試行・補助 API を実装し、S0 の walking skeleton を完成させる。

**Architecture:** 不変の `ParagraphData` と、段落を保持する `Line` を維持する。行組みを入力正規化、走査、行高、位置調整、出力に分け、float の最適化だけを `LayoutContext` の直近 1 件のキャッシュに置く。呼び出し側の float 配置はテストドライバーで検証し、ライブラリの副作用にはしない。

**Tech Stack:** Rust 2024、MSRV 1.89.0、既存の peniko 0.6 / unicode-bidi 0.3.18。性質テストは固定 seed の決定的な生成器を使い、新しい本番依存は追加しない。

**Spec:** `docs/superpowers/specs/2026-09-26-shodo-foundation-design.md`、特に §3.2、§3.3、§4.1、§5.3、§5.4、§6.5。S0-A 計画の「Left for plan S0-B」と beads `shodo-p2m.1.2` の申し送りも対象。

## Global Constraints

- `#![forbid(unsafe_code)]`。公開値は f32 px、内部は 1/64 px の i32 `LayoutUnit`。算術は飽和し、非有限値は 0 と警告。
- 入力は `from_f32_round`、内部で導出する行高だけ `from_f32_ceil`。font-size は 1e6 px 以下、符号付き長さは ±1e7 px。
- `next_line` は同じ入力で同じ結果。同じ token を別の条件で再試行できる。返す `Line` は必ず入力 token より進む。
- `Paragraph` / `Line` は Send + Sync、Line は所有型。通常行の glyph 本体を複製しない。justify の位置配列は 4 bytes / glyph。
- build とフォント登録は上限超過を `Err(LimitExceeded)` として返す。行組み系 API は資源エラーを返さず、計算の打ち切りは警告と安全な fallback。
- `max_warnings` の上限と抑制警告を維持する。float 再試行には新たな回数上限を導入せず、契約による停止性を検証する。
- 仮シェーパーは通常文字 1em、U+0300–U+036F は advance 0 / offset −0.5em。仮メトリクスは ascent 0.8em、descent 0.2em。
- 実フォント、UAX #14、text-transform、first-line データ集合、縦書きの実 shaping、ルビ、hit testing は S1 以降。S0-B はこれらを実装済みと宣伝しない。
- snapshot / ベンチ / Chrome 比較 / 共通 fixture / PNG サンプルは別 issue。S0-B のテストは画像・OS フォント・ネットワークに依存しない。
- 各 task の commit 前に `cargo fmt --check` と `cargo clippy --all-targets -- -D warnings`。ソースコメントに issue ID・レビュータグ・工程名を入れない。

## Review Focus

1. 同じ世代番号を持つ別の AtomicSizes を再利用したとき、前の寸法や改行計画が混入しない（Task 1 / 6 / 9）。
2. InlineBox の block padding / border は描画矩形に入るが行高を増やさない。空の inline の縁は `is_empty` 判定から落とさない（Task 2）。
3. 行内の長い単語に float がある場合、前の行で premature な報告・取り消しを繰り返さない（Task 5）。
4. cache の有無・tab・取り消し・別段落を交互に使っても出力が同じで、Line がキャッシュメモリを参照しない（Task 6 / 10）。
5. generated 境界、空白が消えた境界、u32::MAX 付近の source offset で mapping と文字範囲の演算が壊れない（Task 10）。

## Baseline and File Structure

調査対象は main `82e48df`。全 111 tests と Clippy は既存実装で成功。README の英語整備が入った状態から始める。

| ファイル | 責務 |
|---|---|
| `src/paragraph.rs` | token、atomic / intrinsic 入力型、BreakPlan と公開戻り値 |
| `src/sanitize.rs` | build 正規化と、行組み入力の共通正規化 |
| `src/line/mod.rs` | next_line の調停と、高さ・計画の適用 |
| `src/line/scan.rs`（新規） | greedy 走査、改行機会、幅・tab、float 報告 |
| `src/line/metrics.rs`（新規） | strut、inline / atomic の行高と baseline |
| `src/line/align.rs`（新規） | 揃え、justify、行専用位置配列 |
| `src/line/cache.rs`（新規） | 直近 1 件の部分走査の保持と破棄 |
| `src/line/intrinsic.rs`（新規） | atomic / float を含む intrinsic 幅 |
| `src/line/iter.rs`（新規） | lines / break_all のドライバー |
| `src/line/plan.rs`（新規） | balance / pretty 計画と適合判定 |
| `src/line/fragments.rs`, `src/output.rs` | 行内位置・block 位置・overlay を読むビュー |
| `src/context.rs` | キャッシュと scratch の管理・shrink_to |
| `src/shape.rs`, `src/analysis/units.rs` | stub の行端再 shaping、先読み用 unit 情報 |
| `src/builder.rs`, `src/analysis/whitespace.rs`, `src/mapping.rs`, `src/font/mod.rs`, `src/limits.rs` | S0-A 申し送りの補正 |
| `tests/line_box.rs`, `tests/alignment.rs`, `tests/floats.rs`, `tests/intrinsic.rs`, `tests/iteration.rs`, `tests/break_plan.rs`, `tests/properties.rs`（新規） | 公開 API の契約テスト |
| `tests/common/mod.rs`（新規） | 10px の仮フォント、入力構築、glyph 読み出し、最小 float 配置ドライバー |

テストヘルパーの公開面は `paragraph(text: &str) -> Paragraph`、`build(style: &ParagraphStyle, input: impl FnOnce(&mut ParagraphBuilder)) -> Paragraph`、`first_line(p: &Paragraph, width: f32, options: &LineOptions, atomics: &AtomicSizes) -> Line`、`glyphs(line: &Line) -> Vec<Glyph>`。`paragraph` は font-size 10px / normal line-height。強制改行・inline を含むテストは `build` を使う。

## Task 1: Normalize All Line Inputs and Identify Atomic Revisions

**Files:** `src/sanitize.rs`, `src/paragraph.rs`, `src/line/mod.rs`, `src/line/scan.rs`, `tests/lines.rs`, `tests/common/mod.rs`。

**Interfaces:** 内部 `NormalizedConstraint` は available / inline offset / block offset / max height の LayoutUnit、float cursor、借用した BreakPlan を持つ。`normalize_constraint(&LineConstraint, &mut WarningSink, &mut Saturation) -> NormalizedConstraint`、`normalize_atomic(AtomicSize, &mut WarningSink, &mut Saturation) -> AtomicSize` を追加。既存 scan / unit_width / tab_width を `scan.rs` に移す。

- [ ] **Step 1: Write failing tests.** `all_constraint_and_atomic_fields_are_finite` で NaN / ±∞ を各 float に渡し、返る寸法・glyph 座標・baseline が有限、非有限入力警告あり。`negative_sizes_but_signed_margins` で atomic 寸法 −5→0、有限の margin −2 と baseline −3 は保持。`caller_dimensions_round_to_nearest_unit` で `100.000001`→100px を確認。
- [ ] **Step 2: Run:** `cargo test --test lines`。未正規化の block offset / atomic baseline のケースが失敗すること。
- [ ] **Step 3: Implement.** 入口から geometry の読み出しまで同じ正規化済み atomic 値を使う。全 map の clone は避け、参照する atomic だけ正規化する。text-indent は ±1e7 に正規化。max height の負値は 0、負の block offset / baseline / margins は有効値として保持。public `generation()` は維持し、内部に instance をまたいで衝突しない revision を持たせる（EMPTY=0、insert 時に新しい revision、Clone は現 revision を共有）。
- [ ] **Step 4: Run:** `cargo test --test lines --test fragments --test bidi`。既存のぶら下がり・bidi regression を含め成功。
- [ ] **Step 5: Commit:** `fix: normalize all line layout inputs`。

## Task 2: Inline and Atomic Line Boxes

**Files:** `src/line/metrics.rs`, `src/line/mod.rs`, `src/line/fragments.rs`, `src/output.rs`, `src/paragraph.rs`, `tests/line_box.rs`。

**Interfaces:** `metrics::measure(data: &ParagraphData, scan: &Scan, atomics: &AtomicSizes, sat: &mut Saturation) -> LineMetrics`。内部 LineMetrics に ascent / descent / baseline / block size、各参加 inline / atomic の baseline shift、empty を保持。`FragmentRecord` に block 位置を持たせ、GlyphRunView::baseline は fragment の値を読む。

- [ ] **Step 1: Write failing tests.** root 10px と child 20px / normal→行高20px、baseline16px。atomic height30 / baseline20→行高30px、baseline20px。inline block padding100px→行高は10pxのまま、border rectにはpaddingを含む。`open_inline; block; close_inline` の縁なし空行→`is_empty=true`, height0、token前進。縁のある空inline→empty=false。直接 block token→空行を返さず BlockInInline。
- [ ] **Step 2: Run:** `cargo test --test line_box`。strutだけの現実装との差を確認。
- [ ] **Step 3: Implement.** root strut と各 inline の half-leading / atomic margin box を合成。baseline alignment と `VerticalAlign::Length` を実装し、Top / Bottom は行全体を測った後に解決する。TextTop / TextBottom は親の ascent / descent、Middle は親の baseline + 仮 x-height/2。仮 x-height=0.5em、Sub=−0.2em、Super=+0.3em を stub の明示的メトリクスに置き、S1で差し替え可能にする。中央baselineは required_baseline の種類から合成する。block側のpadding/borderは行高へ入れない。cloneのinline縁は各行へ適用、sliceは先頭/末尾のみ。
- [ ] **Step 4: Run:** `cargo test --test line_box --test fragments --test bidi`。nested align / Top / Bottom / central synthesis / clone継続 / 空白のみと強制改行の行高も検証。
- [ ] **Step 5: Commit:** `feat: measure inline and atomic line boxes`。

## Task 3: Height Constraints and Block Boundaries

**Files:** `src/line/mod.rs`, `src/paragraph.rs`, `tests/line_box.rs`, `tests/lines.rs`。

**Interfaces:** 既存 `LineResult::BlockSizeExceeded { needed_block_size }` と `BlockInInline { node, token_after }` を有効にする。`next_line` のシグネチャは変更しない。

- [ ] **Step 1: Write failing tests.** 10px行 / max height9→needed10、同じtokenをmax=Noneで再試行→Line。同じ9px制約→再度同じ結果。max10→受け入れ。block前の0px空行 / max0→受け入れ。`BlockInInline`後はFIRST_LINEとAFTER_FORCEDをクリアし、`text_indent.each_line=true`でもブロックだけでは改行後字下げを適用しない。
- [ ] **Step 2: Run:** `cargo test --test line_box --test lines`。高さ制約のケースで失敗。
- [ ] **Step 3: Implement.** 行高確定後に制約と比較し、超過ならLineを採用しない。paragraph / token を変更しない。分割コンテナ先頭での受け入れは呼び出し側がNoneで再試行するAPI契約としてdoc化。
- [ ] **Step 4: Run:** 同じテスト。30px atomic / max20 の再試行、別幅での再試行、0/負/NaN制約も成功。
- [ ] **Step 5: Commit:** `feat: enforce line block size constraints`。

## Task 4: Alignment and Justification Without Copying Glyphs

**Files:** `src/line/align.rs`, `src/line/fragments.rs`, `src/output.rs`, `tests/alignment.rs`, `tests/bidi.rs`。

**Interfaces:** `align::apply(data: &ParagraphData, scan: &mut Scan, options: &LineOptions, available: LayoutUnit, origin: LayoutUnit, sat: &mut Saturation) -> AlignmentResult`。Lineに `Option<Vec<LayoutUnit>>` の調整済みpen位置を持つ。Glyphs::getとIteratorが同じ位置経路を使う。

clusterの伸縮値を読む公開入口も追加する: `GlyphRunView::clusters() -> impl ExactSizeIterator<Item = Cluster> + '_`、`Cluster { text_range: Range<usize>, advance: f32, shaping_advance: f32 }`。advanceは行内の伸縮後、shaping_advanceは共有データの元値。結合markを別clusterにしない。

- [ ] **Step 1: Write failing tests.** "ab" 10px / width100→Start0、End80、Center40。offset10 / width80 / indent10 のCenter→最初glyph45（10 + 10 + (80 − 10 − 20) / 2）。RTLのStart/EndとLeft/Rightの対応を確認。"a b c" / width40のjustify通常行→a=0, b=30、最後のぶら下がり空白は伸ばさない。末尾行Auto→start、last=JustifyまたはJustifyAll→伸長。
- [ ] **Step 2: Run:** `cargo test --test alignment`。未実装の揃えで失敗。
- [ ] **Step 3: Implement.** 余白計算はぶら下がりと字下げを区別。Autoはstub段階ではInterWord、Noneは伸長なし、InterCharacterはbase cluster間（結合mark・control・atomic内部を分けない）。余剰は整数LayoutUnitで分配し残余も消費する。負の余白は伸長しない。MatchParentは呼び出し側で親styleが未解決ならStart相当とdoc化。fragment・box・atomicの位置も調整する。RTLは視覚順で位置を分配し、glyph offsetは別に加える。
- [ ] **Step 4: Run:** `cargo test --test alignment --test bidi --test fragments`。getとcollect同値、結合mark、split inline縁、2つの別幅のLineを同時保持して不変性を確認。内部testで通常行の位置配列None、justify行は4 bytes/glyph。
- [ ] **Step 5: Commit:** `feat: align and justify line fragments`。

## Task 5: Float Reporting, Lookahead, and Withdrawal

**Files:** `src/line/scan.rs`, `src/line/mod.rs`, `src/analysis/units.rs`, `src/paragraph.rs`, `src/output.rs`, `tests/floats.rs`, `tests/common/mod.rs`。

**Interfaces:** scan戻り値を `ScanOutcome::{Line(Scan), Float { node, ordinal, inline_position }}` にする。既存FloatCursor::beforeを使う。build時に通常の改行機会で区切る分割不能区間の終端を計算し、先読みでfloatの載る行を判断する。

- [ ] **Step 1: Write failing tests.** 行頭float→position0、indent5なら5。offset10を加えてもpositionに10を二重計上しない。報告cursorをconstraintへ渡して同じtokenを再試行→報告済みfloatを飛ばしてLine。別行でもcursorを保持。"aa b[F]bbbb" / 60px / 10px glyph / float30px→最初の行は"aa "でfloatを報告しない。
- [ ] **Step 2: Run:** `cargo test --test floats`。FloatEncounteredが返らず失敗。
- [ ] **Step 3: Implement.** floatを改行機会にしない。scanが通常のbreakへ巻き戻る場合、その先のfloatを先に報告しない。最初の分割不能区間はoverflowを許してfloatを報告する。cursor以下で、行頭token以降・最終break以降にあるfloatだけを `displaced_floats` に順序付きで返す。呼び出し側は逆順に1件ずつ撤回し、cursorをbeforeへ戻す。再報告された取り消し済みfloatは配置せず保留する。
- [ ] **Step 4: Run:** 同じテスト。共通ドライバーでspecの3反例を追加: tab後の長いspanが2行以上になりfloatがitemの載る行まで延期される; F1=20/F2=50でF2のみ取り消すとF1が残る; 単語内floatが次行へ送られる。各採用行のdisplacedは空、呼び出し回数≤3F+1、float重複配置なしをassert。高さ超過・先読み破棄時のtoken/cursor/配置/保留/取り消し記録の全復元も確認。
- [ ] **Step 5: Commit:** `feat: support incremental float line layout`。

## Task 6: Bounded Partial-Line Cache

**Files:** `src/line/cache.rs`, `src/line/scan.rs`, `src/context.rs`, `tests/floats.rs`。

**Interfaces:** LayoutContextに `Option<PartialLine>`。PartialLineはArc<ParagraphData>、start token、cursor直前、atomic revision、正規化options、位置非依存の区間情報を持つ。`LayoutContext::shrink_to(bytes)` を実効化する。

- [ ] **Step 1: Write failing tests.** 内部testでcached/uncachedのglyph・range・結果variantが同じ。別Paragraph、同世代の異なるAtomicSizes、変わったoptions、cursor撤回は再検査。tabを含む区間はcacheを使わず再計算。shrink_to(0)で保持Arcを解放。
- [ ] **Step 2: Run:** `cargo test line::cache`。cache型がないため失敗。
- [ ] **Step 3: Implement.** cacheは1件のみ。幅変更時は固定advanceのprefixを再走査せず、新しい幅へ再評価できる累積幅とbreak一覧を用いる。floatが載る区間の先読み結果を保持し、位置依存のtab区間は再計算。取り消し・条件不一致では正しいprefixへ戻すか無効化。メモリ見積もりがbytesを超えたら解放。キャッシュはLineの所有データにしない。
- [ ] **Step 4: Run:** `cargo test line::cache -- --nocapture` と `cargo test --test floats`。内部のunit訪問counterでtabなしのN units/F floatsの再試行総走査をO(N+F)に抑える（入力サイズを倍増して確認）。結果同値に加え、保持件数≤1をassert。
- [ ] **Step 5: Commit:** `perf: cache partial lines during float retries`。

## Task 7: break_all and Lookahead Iterator

**Files:** `src/line/iter.rs`, `src/paragraph.rs`, `src/lib.rs`, `tests/iteration.rs`。

**Interfaces:** `Paragraph::break_all(&self, cx: &mut LayoutContext, options: &LineOptions, width: f32, atomics: &AtomicSizes) -> Vec<Line>`。`Paragraph::lines<'p, 'cx, F>(&'p self, cx: &'cx mut LayoutContext, token: BreakToken, options: &LineOptions, constraint_fn: F, atomics: &AtomicSizes) -> impl Iterator<Item=LineResult>`、Fは `FnMut(Option<&LineResult>, f32) -> LineConstraint<'p>`。BreakPlanの借用寿命も'pに合わせる。

- [ ] **Step 1: Write failing tests.** break_allのrange/glyphが手動next_lineループと一致。floatはwidth0のanchor、blockは境界で行分割。linesはLine/BlockInInline/Doneをyieldし、Doneの次はNone。Float/BlockSizeExceededはcallbackへ渡され、条件更新して同じ行頭から再試行。invalid token→InvalidToken1回で終了。
- [ ] **Step 2: Run:** `cargo test --test iteration`。未定義メソッドで失敗。
- [ ] **Step 3: Implement.** iteratorは最初callback(None, offset0)、Line採用後block_offsetを行高分進める。BlockInInline後はtoken_afterで続け、blockの占有量はcallbackがconstraintへ反映する。callbackで条件を変えない場合の無限再試行は利用者の契約違反としてdoc化し、勝手な制限/float配置は追加しない。break_allはfloat_countの最後まで処理済みとし、height制約なしでドライブする。
- [ ] **Step 4: Run:** 同じテスト。多段落・別幅・0px・NaN幅、ページより高いatomicのNone再試行を確認。
- [ ] **Step 5: Commit:** `feat: add paragraph line iteration helpers`。

## Task 8: Intrinsic Widths Including Float Side and Clear

**Files:** `src/line/intrinsic.rs`, `src/paragraph.rs`, `src/lib.rs`, `tests/intrinsic.rs`。

**Interfaces:** `IntrinsicSizes { min_content: f32, max_content: f32 }`、`AtomicIntrinsic { min_content, max_content }`、`FloatSide::{Left,Right,InlineStart,InlineEnd}`、`FloatClear::{None,Left,Right,Both,InlineStart,InlineEnd}`、`FloatIntrinsic { min_content, max_content, side, clear }`。`AtomicIntrinsics`にnode別atomic/float入力とEMPTY、`insert_atomic` / `insert_float`。`Paragraph::intrinsic_sizes(&self, cx: &mut LayoutContext, options: &LineOptions, atomic_intrinsics: &AtomicIntrinsics) -> IntrinsicSizes`。

- [ ] **Step 1: Write failing tests.** "aa bbb" / 10px→min30/max60、forced break→maxは各区間の最大。atomic min20/max50はmargin込みで二重加算しない。float左20/右50/clearなし→max70、2番目clearBoth→max50。同方向でclearLeft、RTL論理side、欠落入力、非有限値を検証。
- [ ] **Step 2: Run:** `cargo test --test intrinsic`。型・メソッド未定義で失敗。
- [ ] **Step 3: Implement.** unit列を1回走査し、minは分割不能区間の最大、maxはforced/block境界までの最大幅。tabは現位置から計算、ぶら下がり空白・indent・inlineの縁を通常行と同じ規則で扱う。floatは左右累積幅を保ち、clear対象が変わる前に現在の最大を確定して合成する。first-line未実装時は既存Unsupported警告を維持し、別集合へのアクセスを仮造しない。
- [ ] **Step 4: Run:** 同じテストと既存lines。内部unit訪問counterで入力長に線形、min≤max、値有限、widthとLineOptionsの変更でParagraphが不変を確認。
- [ ] **Step 5: Commit:** `feat: measure atomic and float intrinsic widths`。

## Task 9: Break Plans and Safe Mismatch Fallback

**Files:** `src/line/plan.rs`, `src/paragraph.rs`, `src/line/mod.rs`, `src/line/scan.rs`, `tests/break_plan.rs`。

**Interfaces:** `Paragraph::plan_breaks(&self, cx: &mut LayoutContext, options: &LineOptions, width: f32, atomics: &AtomicSizes) -> BreakPlan`。BreakPlanにparagraph id / 正規化幅 / 全LineOptions / atomic generationとrevision / break unit列を保持する。`plan::matches`とbreak lookupは内部API。

- [ ] **Step 1: Write failing tests.** 同じ入力で計画位置を再現。Paragraph、幅、options、atomic値を各1つ変えた計画→無視してautoと一致。同じgenerationの異なるAtomicSizesも不一致。NaN幅は正規化され有限。balance iterations=0、pretty window=0はautoと警告。
- [ ] **Step 2: Run:** `cargo test --test break_plan`。未定義メソッドで失敗。
- [ ] **Step 3: Implement.** Auto/Stableはgreedy計画。Balanceはgreedy行数を維持できる最小幅を整数LayoutUnitの二分探索で求め、Limits.max_balance_iterations（既定16）で打ち切る。PrettyはLimits.max_pretty_window_lines（既定4）内で、通常改行候補のraggedness二乗を小さくする有界探索を行い、同点は後ろのbreakを採用。forced/block境界を越えず、atomicの実寸を使う。候補数が増える場合は窓内の各行にgreedy位置とその直前の候補だけを残し、無制限な全候補DPにしない。floatはbreak_all同様anchor扱いの計画とし、next_lineで未処理floatまたはinset付き制約に遭遇する場合は計画を使わずautoと警告。制約が変わった行に無理なbreakを強制しない。
- [ ] **Step 4: Run:** 同じテスト。同段落の計画を複数同時保持、無改行の巨大単語、空段落、forced boundaryを確認。探索回数と窓を内部counterでassert。
- [ ] **Step 5: Commit:** `feat: plan balanced and pretty line breaks`。

## Task 10: Stub Line-Edge Reshaping and S0-A Follow-ups

**Files:** `src/shape.rs`, `src/output.rs`, `src/line/mod.rs`, `src/analysis/units.rs`, `src/builder.rs`, `src/analysis/whitespace.rs`, `src/paragraph.rs`, `src/mapping.rs`, `src/font/mod.rs`, `src/limits.rs`, `tests/build.rs`, `tests/fragments.rs`, `tests/properties.rs`。

**Interfaces:** Lineのoptional overlayは既存GlyphStore形式、fragmentのglyph sourceはShared/Overlay。`shape_line_edge` は `&ParagraphData` の保持フォントからfont idを取り、完全cluster単位のglyph列を返す内部関数。呼び出し側にFontCollectionを追加要求しない。

処理後長さの上限検査は `analysis::process(raw_text, raw_items, styles, with_mapping, limits) -> Result<Processed, LimitExceeded>` に統一する。生成文字・処理後itemの追加前に照合し、Paragraph::from_builderがエラーを伝播する。Unitに内部のunsafe-to-break / unsafe-to-concat flagを持たせ、stubでは既定false、内部testからのみtrueを設定する。

- [ ] **Step 1: Write failing tests.** 内部testでsafeでない境界flagを注入し、両側の再shapingを実行。元FontCollection/Paragraph handleをdropしてもLineのfont_dataとglyphを取得できる。reshape window超過→警告/元結果、欠落・重複なし。generated範囲の先頭でUpstream→直前DOM、Downstream→Generated。深い文書フォント層を作っても2層世代が壊れない。
- [ ] **Step 2: Run:** `cargo test --test build --test fragments --test properties` と内部shape/mapping tests。未実装overlay・mapping境界で失敗。
- [ ] **Step 3: Implement.** stubは自然なUNSAFE境界を生成しないので、内部flag注入で§6.5の経路を証明する。出力glyph予算は既存max_shaped_glyphsに加え元の置換範囲のglyph数以内とし、超過は元のclusterへfallback。生成後textの長さをchecked演算で照合し、u32表現上限とmax_text_bytesを超える前にbuild Err。処理後item数も増分照合。max_text_bytes16MiB / max_shaped_glyphs2^22は別の資源上限として維持し、ASCIIでも後者に達する場合をdoc化。for_documentへ文書層を渡した場合は根の共有層へ正規化して2層を維持する。
- [ ] **Step 4: Implement remaining follow-ups.** Limits各fieldの単位・既定値・適用工程をdoc化。atomic直前のOpen縁をatomicと同じ区間へ束ね、前行に孤立させない。public StyleIndex / intern APIはspec本文が公開を要求していないため導入しないと決定し、内部style interningを維持する。generatedとDOMが接する境界のAffinityを対称にし、generated内部は常にGenerated。
- [ ] **Step 5: Run:** `cargo test`。内部小さな長さ制限で生成文字の追加前エラーを確認し、巨大な実メモリ確保を要しない。clone/cache/overlayを交互に利用してsource範囲とfont保持を検証。
- [ ] **Step 6: Commit:** `feat: complete bounded line edge reshaping`。

## Task 11: Properties, Warning Cleanup, and Public Documentation

**Files:** `tests/properties.rs`, `src/lib.rs` と警告が出た定義、`README.md`, `.github/workflows/ci.yml`（検証不足がある場合のみ）。

**Interfaces:** 公開APIを通す決定的生成器。入力にはASCII、RTL、結合mark、inline開始/終了、atomic、float、forced/block、有限/非有限寸法を含める。fuzz継続基盤は別途S2以降へ申し送りし、ここは再現可能な通常testにする。

- [ ] **Step 1: Write properties.** 256 seeds / 各入力最大128 unitsで、Line token前進、終了、有限geometry、mappingのIdentity内部往復、Collapsed/Expandedの規定丸め、float retry≤3F+1、元glyphとjustify/overlayのcluster範囲同値、Limits超過Errをassert。失敗時はseedと入力を表示する。
- [ ] **Step 2: Run:** `cargo test --test properties`。各taskで残った契約違反を特定し、所有taskの実装を修正する。
- [ ] **Step 3: Remove** crate全体の `allow(dead_code, unused_imports)`。不要な内部定義を削除し、必要な予約は狭い定義に理由付きallowを付ける。公開の予約APIは未実装の警告・docを維持し、空実装で警告を消さない。
- [ ] **Step 4: Update** 英語READMEの実装状況・制約・break_all使用例。floatドライバーとiterator再試行の呼び出し側責務をrustdocに記述。Issue `shodo-p2m.12` の本格的な配置ハーネスとは別であることをhandoffに記録。
- [ ] **Step 5: Verify:** `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings`; `cargo test`; `cargo +1.89.0 test`（未導入ならMSRV CIの結果を確認）; `cargo build --target wasm32-unknown-unknown`; `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps`; `git diff --check`。すべてexit0、公開API docリンク切れなし。長時間・数値境界・変更APIに関する失敗はclose前に解消。
- [ ] **Step 6: Commit:** `test: verify line flow contracts and document APIs`。全taskのレビューとspec §6.5 checklistが揃ってからPRを作成し、マージ確認後にS0-Bのclose可否を判断する。

## Coverage and Deferred Work

主要なテストのassertion例（前述の各taskのStep 1に対応。入力の組み立て・importsはテストヘルパーを使う）:

```rust
// Task 1: NaNの幅は0へ正規化されても単語を消費して進行する。
let p = paragraph("ab");
let line = first_line(&p, f32::NAN, &LineOptions::default(), &AtomicSizes::EMPTY);
assert_eq!(line.inline_size(), 20.0);
assert!(glyphs(&line).iter().all(|g| g.inline_position.is_finite()));

// Task 2–3: 高さ30px、baseline20pxのatomicを持つ行。
assert_eq!(atomic_line.block_size(), 30.0);
assert_eq!(atomic_line.baseline(BaselineKind::Alphabetic), 20.0);
assert!(matches!(height_limited_result,
    LineResult::BlockSizeExceeded { needed_block_size: 30.0 }));
assert_eq!(empty_before_block.block_size(), 0.0);
assert_ne!(empty_before_block.break_token(), token_before_empty);

// Task 4: "ab"、10px、幅100px、center。
assert_eq!(glyphs(&center_line)[0].inline_position, 40.0);
assert_eq!(run.glyphs().get(0), run.glyphs().next());
assert_eq!(run.clusters().map(|c| c.advance).sum::<f32>(), run.inline_size());

// Task 5–6: float入りの入力をドライバーで組み、cache有無を比較する。
assert!(accepted_lines.iter().all(|l| l.displaced_floats().is_empty()));
assert!(retry_count <= 3 * floats_in_line + 1);
assert_eq!(cached_dump, uncached_dump);

// Task 7: 手動ドライバーと便利APIの結果は同じ。
assert_eq!(break_all_dump, manual_dump);
assert!(matches!(iterator.next(), Some(LineResult::Done)));
assert!(iterator.next().is_none());

// Task 8: "aa bbb"、10px。
assert_eq!(sizes.min_content, 30.0);
assert_eq!(sizes.max_content, 60.0);

// Task 9: 不一致の計画はautoと同じ結果。
assert_eq!(mismatched_plan_dump, auto_dump);

// Task 10–11: sourceとfontの保持、token前進、有限な幾何。
assert!(line.font_data(font_id).is_some());
assert_ne!(line.break_token(), input_token);
assert!(line.block_size().is_finite());
assert!(line.inline_size().is_finite());
```

| 要求 | Task |
|---|---|
| 全入力正規化・atomic識別 | 1 |
| 行高・baseline・空のBlockInInline・inline縁 | 2–3 |
| 揃え・justify・行専用位置配列 | 4 |
| float報告・先読み・displaced・3反例・停止性 | 5 |
| 部分走査cacheとshrink_to | 6 |
| lines / break_all | 7 |
| intrinsic / float sideとclear | 8 |
| plan_breaks / 不一致fallback / Limits | 9 |
| 行端再shaping・S0-A追加とMinor申し送り | 3 / 10 |
| 性質テスト・全体allow撤去・MSRV/wasm/docs | 11 |

承認済みspec全体のS1〜S8は対象外。first-line用集合、実シェーパー、フォントmatcher、Unicode改行・full CSS spacing、hit testing、縦書き・ルビは既存後続issueへ残す。S4は現在「統合検証」、本番切り替えは `shodo-p2m.6` であり、この計画の完了だけで切り替え可能とはしない。

## Execution Handoff

この計画はレビュー待ち。まず仕様・公開型・検証条件を確認し、その後に実行方法を選択する。Native（同じセッションで順に実装）を推奨する。scan / metrics / alignment / cache のinterfaceが連続して変わるため、実装を順番に進めると調整を追いやすい。Subagent-drivenも選択可能だが、taskごとの実装・review用コンテキストが増える。
