# 実 CSS first-line caller 接続 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** raikiri が実 CSS `::first-line` から同一 DOM の通常/先頭行スタイルを生成し、shodo caller の描画と固定 WPT 比較で検証する。

**Architecture:** raikiri-style が通常の宣言候補と first-line の継承規則を使って子孫の別スタイルを解決する。shodo の既存 first-line API に接続し、`dev/raikiri` の依存をこの変更を含む一つの確定 SHA に全体更新する。測定は既存の履歴資料と出所が区別できる新しい記録に保存する。

**Tech Stack:** Rust 2024、Rust 1.89.0、raikiri-style/html/dom、shodo、固定フォント、tiny-skia、raikiri の native paint、Python の記録検査。

**Spec:** [承認済み設計](../specs/2026-09-30-7ff-real-first-line-design.md)（`ab1c8bf`）。

## Global Constraints

- 既存の `dev/raikiri` の git pin は全体を更新する。
- HTML 書き換えや通常 CSS の根要素上書きは使わない。
- 通常 DOM、ノード ID、テキスト、セレクター一致結果は保持する。
- 子要素自身に勝った明示宣言はその要素で適用し、宣言がない継承プロパティだけ疑似要素または先頭行上の親から継承する。
- 相対値は各経路の継承元に対して一度だけ計算する。
- 表現できない値やレイアウト要素を受け取ったら理由付きで拒否する。
- 過去の結果に書かれた旧 SHA は履歴の出所なので上書きしない。
- 未計測の WPT 合格数は主張しない。
- upstream のソースコメントは英語、追加 unit test は別の `tests.rs`。raikiri の `docs/superpowers/` は追跡しない。
- shodo は現在の `target/worktrees/shodo-7ff`、raikiri は `/home/mitz/Work/oss/raikiri-spike/.worktrees/shodo-7ff` の作業ブランチを使う。保存済み S4 checkout を変更しない。

## Review Focus

- 子の `color:inherit` と同じ値の明示宣言を区別する（Task 2、3）。
- `var()`、非継承プロパティの `inherit`、除外された direction は通常 DOM の継承元を使う（Task 1、2）。
- `rem`/`rlh` が部分木の先頭で再初期化されない（Task 2）。
- 最初が空行、または一つのテキストノードが複数行に分かれる場合も次行は通常スタイルになる（Task 3）。
- WPT で unsupported や失敗を合格扱いせず、元の入力/フォント/参照が変わった場合は記録生成を拒否する（Task 4）。

---

## Task 1: raikiri の first-line 適用プロパティを制限する

**Files (raikiri):** Create `crates/raikiri-style/src/cascade/first_line.rs` と `cascade/first_line/tests.rs`。Modify `cascade.rs`、`cascade/inherit.rs` と既存の疑似要素テスト・doc。

**Interfaces:** `pub(crate) fn first_line_property_applies(key: PropertyKey) -> bool`。first-line の宣言候補だけを制限し、通常要素と before/after/marker の候補はそのまま扱う。

**Tests:** `first_line_filters_box_and_excluded_properties`、`first_line_keeps_inline_properties_and_vars`、`first_line_filters_deferred_shorthand`。解決した `first` と発生元の `normal` に対する主要 assertion:

```rust
assert_eq!(first.direction, normal.direction);
assert_eq!(first.display, ComputedValues::initial().display);
assert_eq!(first.opacity, 0.5);
```

- [ ] first-line の font/color/opacity/background/typesetting/text-decoration/inline-layout 候補が採用され、`display:block`、margin、width、writing-mode、direction、text-orientation が採用されないテストを書く。基準は [CSS Pseudo §2.1.2](https://drafts.csswg.org/css-pseudo/#first-line-styling) と各プロパティの適用対象。カスタムプロパティを使った font-size の解決と、無効 CSS の宣言破棄も確認する。
- [ ] `cargo test -p raikiri-style first_line` を実行し、現在の無制限適用が期待と違って失敗することを確認する。
- [ ] 候補収集後、winner 選択前に first-line の許可判定を適用する。展開済み shorthand と deferred declaration にも同じ制限を適用する。除外プロパティは発生元からの継承値を使う。
- [ ] 上記テストと `cargo test -p raikiri-style`、`scripts/orphan-tests-lint.sh` を通す。公開 doc の変更は `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps -p raikiri-style` と upstream AGENTS の補助 doc 検査で確認する。
- [ ] この独立した変更を作業ブランチにコミットする。

## Task 2: 同一 DOM の子孫 first-line cascade API

**Files (raikiri):** Modify `cascade/first_line.rs`、`cascade/first_line/tests.rs`、`cascade.rs`、`cascade/inherit.rs`、`lib.rs`。既存の `collect`、`SpecifiedValues::finalize` と winner 処理を再利用する。

**Interfaces:** crate root から次を公開する。`root` は発生元の実 DOM ID。`computed[root]` は疑似要素の inline スタイルであり、ブロック geometry は `normal` を使う。

```rust
pub struct FirstLineCascade {
    pub normal: CascadeResult,
    pub first_line: Option<FirstLineStyles>,
}
pub struct FirstLineStyles {
    pub root: StyleNodeId,
    pub computed: Vec<Option<ComputedValues>>,
}
pub fn cascade_with_first_line<D: StyleDom>(
    dom: &D, rule_tree: &RuleTree, media: &MediaContext, root: StyleNodeId,
) -> Result<FirstLineCascade, CascadeError>;
```

**Tests:** `first_line_child_winners_preserve_provenance`、`first_line_preserves_normal_inheritance_channels`、`first_line_preserves_document_relative_units`、`first_line_rejects_unsupported_roots_and_subtrees`。以下の値は5つの子を fixture 順で収集して比較する。

```rust
assert_eq!(normal_sizes, [16.0, 16.0, 20.0, 30.0, 24.0]);
assert_eq!(first_sizes, [32.0, 16.0, 20.0, 30.0, 48.0]);
assert_eq!(resolved.normal.computed.len(), dom.node_count());
```

- [ ] CSS root `16px/blue`、first-line `32px/red/uppercase` を使い、子の normal/first-line font-size が順に `16/32`（宣言なし）、`16/16`（同値明示）、`20/20`（異値明示）、`30/30`（20px の親の150%）、`24/48`（root 直下の150%）となるテストを書く。`color:inherit` は first-line の red、明示 blue は blue のまま確認する。
- [ ] Review Focus のテストを書く。first-line 内のカスタムプロパティ変更が子の `var()` に漏れないこと、子の `background-color:inherit` が通常の親を参照すること、direction の除外、root が `20px` の `1rem` と `rlh` の部分木計算を確認する。通常 computed 値と DOM は別の通常 cascade と一致させる。
- [ ] 疑似要素規則なしは `first_line:None`、存在しない/非ブロック root はエラー、対象外 sibling・detached・display:none 部分木は `None`、nested block/inline-block は範囲外エラーとなるテストを書く。
- [ ] `cargo test -p raikiri-style first_line` を実行し、未実装 API または期待差分で失敗することを確認する。
- [ ] 通常 cascade と同じ宣言候補から二つの経路を計算する。子の継承用ベースは通常親から作り、標準の継承プロパティ（除外3種以外）だけ先頭行の親の値に差し替える。子の winner 適用と絶対値化は既存処理で行い、custom-property environment と document root の `rem`/`rlh` context は通常経路から使う。
- [ ] 全 style テスト、html の parse/cascade テスト、fmt/clippy、Task 1 の doc 検査を通し、upstream の必要な gate を確認する。API doc にサポートする一つの IFC とエラー条件を明記し、作業ブランチにコミットする。この commit SHA を以後の依存更新に使う。

## Task 3: 全体の pin 更新と代表 caller 接続

**Files (shodo):** Modify `dev/raikiri/Cargo.toml`、`Cargo.lock`、`dev/raikiri/tests/first_line_cascade.rs`、`examples/support/raikiri_contracts.rs`、`examples/raikiri_contracts.rs`。pin 更新で API 変更に影響される他の `dev/raikiri` 実行対象も意味を保って移行する。

**Interfaces:** `ResolvedInput` は DOM、Task 2 の通常/先頭行スタイル、root ID を保持する。`resolve_html(html: &str, root_id: &str) -> Result<ResolvedInput, String>` は実 CSS を読む。`resolve_document(parsed: UncascadedDocument, root: StyleNodeId, media: &MediaContext) -> Result<ResolvedInput, String>` は WPT の元 DOM を受け取る。`layout(input: &ResolvedInput, fonts: &FontCollection, width: f32) -> Result<Output, String>` は保持した root を使用する。

**Tests:** `real_css_first_line_reaches_glyph_paint_and_mapping`、`empty_first_line_leaves_following_text_normal`、`wrapped_single_text_node_preserves_later_source_ranges`、`unsupported_first_line_values_are_rejected`。`br` を挟むリンク fixture の主要 assertion:

```rust
assert_eq!(accepted_texts, ["SS", "ß"]);
assert_eq!(line_font_sizes, [32.0, 16.0]);
assert_eq!(output.links[0].kind, MappingKind::Expanded);
assert_eq!(output.links[1].kind, MappingKind::Identity);
```

- [ ] 通常 root 上書き fixture を `#root::first-line` に変え、旧供給関数を使わないテストを書く。Task 2 の5例を glyph font-size、color、実際の paint、DOM mapping まで検証する。`ß` の先頭行は `SS/32px/red`、`br` 後は `ß/16px/normal color`、リンクの先頭行は expanded mapping、次行は identity mapping を確認する。
- [ ] 空の先頭行 `<br>ß` と、一つのリンク内のテキストが幅で2行以上になるケースを追加する。先頭行だけ transform/font/paint が変わり、以後の source 範囲が通常経路で正しく切られることを検証する。非対応 font/transform/opacity/background/構造を黙って失わずエラーにする。
- [ ] `cargo test -p shodo-raikiri --test first_line_cascade --example raikiri_contracts` の失敗を確認する。
- [ ] 全 raikiri git 依存を Task 2 の確定 SHA に更新し、lock を解決する。実 CSS 供給と `open_inline_with_first_line` を接続する。root の display 検査は通常スタイルに行い、疑似要素の inline display をブロック geometry と混同しない。CLI JSON の供給元表示を実 CSS に更新する。
- [ ] 対象テスト、`cargo test -p shodo-raikiri --all-targets`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets` を通す。`cargo run -p shodo-raikiri --example raikiri_contracts -- target/first-line-caller` で JSON と PNG を確認する。
- [ ] manifest と lock の raikiri SHA が一つに揃っていることと、過去の pin/測定記録の出所が維持されていることを確認し、コミットする。

## Task 4: 固定 WPT の native/candidate 比較と記録

**Files (shodo):** Create `dev/raikiri/examples/first_line_wpt.rs`、`examples/support/first_line_wpt.rs`、`dev/raikiri/data/first-line-wpt-pins.json`、`first-line-wpt-results.json`、`docs/records/raikiri-first-line.md`。Modify `examples/support/raikiri_contracts.rs`、`dev/raikiri/Cargo.toml` と lock。必要な native paint の dev 依存も Task 2 と同じ pin にする。

**Interfaces:** `first_line_wpt <wpt-root> <output-dir>`。既存 `offline_wpt::parse_screen` と `source_fonts::load` を使う。caller に `FontPolicy::{FixtureLatin, BundledWpt}` と `layout_with_font_policy(input: &ResolvedInput, fonts: &FontCollection, width: f32, policy: FontPolicy) -> Result<Output, String>` を追加し、WPT の登録済み静的 generic/named font を使えるようにする。Task 3 の `layout` は `FixtureLatin` を選ぶ wrapper、WPT は `BundledWpt` を選ぶ。

**Tests:** `changed_original_resource_is_rejected`、`unsupported_and_failed_rows_do_not_count_as_pass`、`no_first_line_rule_keeps_control_output`。記録分類の主要 assertion:

```rust
assert_eq!(unsupported_row["status"], "unsupported");
assert_eq!(failed_row["status"], "failed");
assert_eq!(unsupported_row["counted_as_pass"], false);
assert_eq!(failed_row["counted_as_pass"], false);
```

- [ ] 比較・記録検査のテストを書く。変更した HTML/参照/font SHA は拒否、unsupported candidate は `unsupported`、native 失敗は `failed` と記録し合格数に入れない。first-line 規則がない control は通常描画と同じになることを確認する。
- [ ] WPT commit `97ea26e26a2aac3eec7e770650b25e7049ed4a4e` の `css/CSS2/selectors/first-line-001.xht` と `first-line-pseudo-021.xht` を正の対象として固定する。前者は body の color、後者は子の明示 `color:inherit`。それぞれ元の `rel=match` 参照も固定する。`css/css-pseudo/first-line-opacity-001.html` は未対応値、`css/CSS2/selectors/first-line-inherit-003.xht` は nested block の対象外を記録するケースにする。
- [ ] テストを実行して記録検査の失敗を確認し、候補描画を実装する。DOM と CSS は元入力のまま、leaf IFC は同じ DOM ID で選ぶ。正の2例では native の通常 layout が返す block の位置/content 幅を共通の測定境界として使い、candidate の文字描画・geometry は shodo の実出力にする。全ページ WPT 合格と、共通 block 境界内の描画比較を混同しない。
- [ ] native は同じ pin の raikiri-dom 通常 layout と raikiri-paint を使う。candidate は Task 3 の実 CSS caller を使う。双方とも元の bundled WPT font registry を system fonts 無効で使用し、800×600、screen media、同じ文字と resources を記録する。PNG、glyph/font/color/source と line geometry、pixel 差分、入力 SHA、エンジン SHA、lock/toolchain と exact argv を出力する。
- [ ] `cargo test -p shodo-raikiri --example first_line_wpt` と `cargo run -p shodo-raikiri --example first_line_wpt -- /home/mitz/.cache/raikiri/wpt target/first-line-wpt` を実行する。入力/参照/フォントの pin と raw 結果を資料に保存し、native が対応しているかを結果で判定して必須/追加能力の分類を記録する。
- [ ] 最終変更に対し CI 相当の workspace tests、accesskit tests、Rust 1.89.0 tests、Python tests、fmt/clippy/doc と wasm build を実行する。変更に影響された snapshot は差分の原因を確認する。新しい結果の記録と caller の説明をコミットする。
- [ ] raikiri と shodo の変更をレビュー可能な draft PR として提出する。`shodo-7ff` に確定 SHA、検証結果、比較結果、対象範囲を記録し、受け入れ条件がすべて揃ったことを確認して完了処理する。

## 実行方式

4 task は API と確定 SHA の順序に依存するため、同じセッションで私が順番に実装する Native 方式を推奨する。実装開始前にこの計画のレビューと実行方式の選択を受ける。
