# intrinsic計測のreshape予算の修正（shodo-sbp.1）

2026年9月30日。intrinsic計測の呼び出し回数によってmin-content幅が変わる問題を、`Paragraph::intrinsic_sizes` の公開入口で端窓reshape予算を初期化して修正した。first-line用のmin/max二回の計測は一つの予算を共有する。予算の上限、キャッシュ参照前の課金、単一窓の上限は維持している。

## 原因と回帰テスト

`LayoutContext` に保持される `edge_reshape_spent` は `next_line` では初期化されるが、intrinsic計測では前の操作から累積していた。固定Latinフォント、既定limits、同じcontextで `One  two\tthree\nFour five soft­hyphen.` を反復すると、旧実装では11,398回目にmin-contentが65.421875から89.609375 pxへ変わった。max-contentは271.140625 pxだった。

`dev/harness/tests/intrinsic_budget.rs` に三つの回帰テストを置いた。旧実装では三つとも出力幅の不一致で失敗し、修正後は成功した。

- 既定limitsで通常・first-lineそれぞれ30,000回、fresh contextのmin/max幅と厳密一致する。
- 実際のArabic行組みで予算を使い切った後でも、intrinsic計測のmin/max幅がfreshと一致する。
- 小さい窓上限では単一パスが予算内に収まり、first-line二パスの合計では予算超過を警告する。同じcontextで繰り返しても出力と警告が安定する。

計測の内部ヘルパーやSHY・rubyの候補ごとには予算を初期化しない。rubyは内部の範囲計測と再帰を使い、公開操作への再入による予算の再初期化はない。

## 独立probe

7条件それぞれ30,000回の全出力について、fresh contextのmin/maxとdigestに一致することを検証した。条件は同context再利用、毎回新規、毎回 `shrink_to(0)`、行組み挿入、窓上限解除、Criterion相当の事前操作、全操作の事前実行。全条件で幅は65.421875 / 271.140625 px、digestは `f4c4b1305b3f55fdd514bb69ef9f7cfa042846fa57867c0906869174e489d4de`、新規警告は0だった。

## 全体検証

以下のチェックが成功した。

- `cargo fmt --all --check`、workspace全targetのClippy（`-D warnings`）。
- `cargo test --workspace`: 1,055成功、失敗0。
- `cargo test -p shodo --no-default-features`: 602成功、失敗0。
- `cargo test -p shodo --no-default-features --features complex-scripts`: 605成功、失敗0。
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`。
- 固定フォントsnapshot比較: 成功。期待画像は更新していない。
- `python3 tools/bench/run.py --output /tmp/shodo-sbp1-matrix --quick --cold-samples 2`: 全54 workload、378 warm操作、108 cold process、54 memory processを収集し、runnerの全整合チェックが成功。以前停止した `latin-spacing/intrinsic/1` を含め、timingとmemoryのintrinsic digestが一致した。

Rustの通常検証は1.96.0、matrixと独立probeはstable 1.97.1。matrixはworkspaceの初回ビルドと一部並行したため、時間値を性能比較用baselineに使わない。全体CIのMSRV・Wasm・AccessKit組合せは今回のローカル検証には含めていない。

runnerの既存source hashには実装本体の欠落があるため、別途 `crates/shodo/src` の全Rustソース、crate/workspace manifest、lockの114ファイルを実行前後にhashし、一致を確認した。合成SHA256は `fccb98c7cab20b6be2dd048378d1c9cead14696039da487a734cd88682d94d03`。対象は `0fc9056dd4b9ae6c5387b52677856466e83f6968` を基点とした `fix/sbp1-intrinsic-budget` の修正ソースで、測定時点では未コミットだった。

[検証manifest](data/intrinsic-reshape-budget.json) に各コマンド、probe、file hashを記録し、[生データarchive](data/intrinsic-reshape-budget-raw.json.gz) に全matrixのraw samples・digest・metadata、独立probeのソースと出力、red/greenログ、全体検証ログ、snapshot結果を保存した。元runner成果物は `/tmp/shodo-sbp1-matrix`、snapshot比較レポートは `/tmp/shodo-sbp1-snapshots/index.html`。独立コードレビューで指摘はなかった。

この修正は出力の決定性を回復するもので、速度改善率は主張しない。raikiri本番切り替えやS4への性能ブロック依存は追加しない。runnerのsource fingerprint修正は独立issue `shodo-sbp.2` の対象として残す。
