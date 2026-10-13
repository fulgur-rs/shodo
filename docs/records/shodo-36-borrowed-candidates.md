# SHODO-36: bounded edge 候補評価の glyph 借用

2026-10-14。基準は `c892cae`、初期借用化は `be53bac`、候補結果の軽量化は `c26aa6f`。

`candidate()` と `delta()` は、幅差分と partial-edge coverage を調べるだけでも、従来は `measure()` から所有 `EdgeOverlay` を作っていた。キャッシュヒットの `WindowHandle::Shared` はその際 `GlyphStore` と `Vec<ShapedRun>` を deep clone していた。共有 ligature の候補では残り suffix の評価も同じコピーを行う。

幅変更列の算出は `WindowHandle::Shared` を借用して行う。その後の内部 `MeasuredWindow` は元 glyph 範囲・source text 範囲・glyph 数・幅変更列だけを保持し、glyph 強参照は残さない。幅差分・coverage・残り suffix の確認で glyph 所有化を行わない。共通の `materialize<T: WindowOutput>` / `measure_edit<T: WindowOutput>` が軽量結果と所有 `Window` を別に特殊化し、最終 `measure()` と生成 hyphen overlay は直接 `Vec<Window>` へ出力する。編集済み窓と非キャッシュ窓は元の owned vector を move する。

窓の拡張、右隣 probe、shape 呼出し・glyph budget・操作ごとの reshape charge、warning/saturation の演算順は変更していない。`expand_original` の反復削減やキャッシュ保持方針は別課題であり、この変更には含めない。キャッシュ miss の snapshot clone は従来どおり最大 1 回のまま残す。

## 回帰テスト

既存 edge windows 23 テストを先に成功させた。実 `GlyphStore::clone` を thread-local の既存 test-only probe で測るテストを追加し、旧実装で warmed `ffi` candidate が 2 clone、期待 0 に対して失敗することを確認した。

借用後の warmed candidate/delta は固定 Latin `ffi` と RTL Arabic `ببب` の両方で 0 clone。`ffi` は同じ GSUB cluster に属する選択部分と残り suffix を評価する。形状キャッシュが使用されていることと追加 shape 呼出しがないことも確認する。cold evaluation は cache retention に必要な snapshot 数を超えて clone しない。

初期借用化は大きな `WindowHandle` を候補結果に保持し、最終出力で別の `Vec<Window>` へ変換していた。幅と coverage の算出後には不要な glyph lifetime を延長していたため、キャッシュ解放後も測定結果が glyph を生かしてしまう回帰テストを追加し、初期版で失敗を確認した。軽量結果への特殊化後はキャッシュを解放すると glyph の weak reference は失効し、残った幅差分は引き続き読める。最終出力の Vec 変換も除去した。

最適化前と同じ所有出力経路による参照評価と比較して、candidate の delta/valid、正確な警告列、shape 回数、reshape 累積 charge、saturation counters を照合する。Latin/RTL、cold/warm、窓 bytes 制限、glyph budget、charge の上限 crossing と `u64::MAX`、warning suppression を含める。別テストでは edge の再 shaping に巨大 font size を与え、新しい saturation が発生し非キャッシュ扱いになった場合の結果と副作用も照合する。

既存の生成 SHY と対向 partial-edge の共通 glyph budget、shape 不能な ligature 内部・残り suffix、autospace、縦書き combined glyph、異なる font の隣接 unsafe edge のテストを維持する。

## 再現条件

固定フォントはリポジトリの Latin/Arabic fixture を使い、system discovery は無効。Rust `stable` 1.97.1、workspace lock、通常 debug assertions と overflow checks を使用する。環境の test runner による保護待ちを避けるため runner を `env` に上書きした。一時生成物は専用の `~/tmp/shodo-36-target` に作成する。最終 workspace 再検証は `RUST_TEST_THREADS=4`、CPU affinity `3-7,9-11` とし、別途行う性能測定の CPU と SMT 相方を避けた。

```sh
mkdir -p "$HOME/tmp"
export TMPDIR="$HOME/tmp"
export RUSTC_WRAPPER=
export CARGO_BUILD_JOBS=3
export CARGO_TARGET_DIR="$HOME/tmp/shodo-36-target"
cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' test -p shodo --lib line::windows
cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' test --workspace
cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' test -p shodo --features accesskit --lib line::windows
cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' test -p shodo --no-default-features --lib line::windows
cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' test -p shodo --no-default-features --features complex-scripts --lib line::windows
cargo +stable fmt --all --check
cargo +stable clippy --workspace --all-targets -- -D warnings
cargo +stable clippy --workspace --all-targets --features shodo-harness/accesskit -- -D warnings
```

clone 回数の削減は上記の実コード試験で確認する。時間・割当量・RSS の改善率はこの決定的テストから推定しない。長文を含む A/B は同一 CPU・build/relayout scope・context history を固定し、非 instrumented 時間と allocation counting を分けて測る。

## ローカル検証

初期借用化と追加 saturation 回帰の段階では workspace 1,914、accesskit 有効 workspace 1,931、既定 feature なし 1,376、complex-scripts のみ 1,379 テストが成功した。通常の診断用・外部 fixture 必須の ignored テストは各 full suite で 9 件。doctest runner は環境変数未指定の既存警告を出したが、全 doctest は成功した。

最終 `c26aa6f` では通常 workspace 全 suite が 1,916 テスト成功、失敗 0、ignored 9。借用・寿命・副作用同等性を含む edge windows 27 テストを accesskit 有効・既定 feature なし・complex-scripts のみでそれぞれ成功させた。通常/accesskit の workspace 全 target の Clippy（`-D warnings`）と fmt も成功した。固定画像 snapshot の期待値は更新していない。

今回新規に作成した専用 `~/tmp/shodo-36-target` は最終レビューと CI で再検証する可能性があるため保持し、統合後に親作業で削除する。その他の一時コード・性能ログはこの実装作業では作成していない。テスト自身が作る一時ディレクトリは正常終了時に解放される。実装 worktree はレビュー・後続 SHODO-35 の統合用に保持する。
