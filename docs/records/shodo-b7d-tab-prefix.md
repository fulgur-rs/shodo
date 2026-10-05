# shodo-b7d: 保存された tab による ruby 測定の 2 乗化

## 結論

採用する。`white-space: pre` の tab を 1 つ含む ruby の `break_all` から 2 乗の項が消えた。測定前に固定した倍化比 2.6 以下の基準は `nestedtab` では文字どおりには満たせなかったので、その基準を下の理由で言い直して採用の根拠にする。

`white-space: pre` の tab を 1 つ含む ruby の `break_all` から 2 乗の項が消えた。`nestedtab`（入れ子の ruby の最内側が `"日\t日"`）は depth 160 で baseline 5.04 s（BAAB 5.06 s）が 17.78 ms（17.37 ms）になり（−99.6% / −99.7%）、`siblingstab`（先頭に `"\t"` を置いた兄弟 ruby）は 800 で 13.25 s（13.29 s）が 34.12 ms（34.30 ms）になった（−99.7%）。tab を含む全 7 点で、ABBA・BAAB とも 12/12 ラウンドで candidate が速く、遅いラウンドは 0。出力と警告の SHA-256 は全 1536 サンプルで一致した（`digest_mismatch` は空）。操作数では、`nestedtab` の `container_measures` が参照 3,136 / 12,416 / 49,408（depth 16 / 32 / 64）に対し memo と accumulator が 48 / 96 / 192（384 まで正確に 2.0 倍ずつ）で、キャッシュの世代（epoch）は 1 のまま動かない。

基準の言い直し。計画の倍化比 2.6 以下は、wall clock の比で線形性を見るための代理の基準だった。`siblingstab` の candidate は ABBA 2.18 / 2.21、BAAB 2.22 / 2.18 で 2.6 以下を満たす。`nestedtab` の candidate は ABBA 2.91 / 2.57 / 2.62、BAAB 2.90 / 2.66 / 2.49 で、20→40 の 2.91 / 2.90、ABBA の 80→160 の 2.62、BAAB の 40→80 の 2.66 が 2.6 を超える。しかし tab のない `nested` の対照（変更していない経路）が同じ曲線を示す。candidate の `nested` は ABBA 3.12 / 2.71 / 2.56、BAAB 3.15 / 2.72 / 2.63 で、baseline も 3.18 / 2.66 / 2.54 と 3.08 / 2.73 / 2.56。shodo-d77 の記録の `nested` の candidate も 3.27 / 2.73 / 2.58（BAAB 3.29 / 2.70 / 2.67）だった。`nestedtab` の倍化比を `nested` の倍化比で割ると ABBA が 0.93 / 0.95 / 1.02、BAAB が 0.92 / 0.98 / 0.95 で、tab による上乗せの増加は見えない（同じ size の時間の比 `nestedtab` / `nested` は ABBA が 1.25 / 1.16 / 1.10 / 1.13、BAAB が 1.27 / 1.17 / 1.14 / 1.08 と、size とともに縮む）。操作数は `nested` も `nestedtab` も 48 / 96 / 192 / 384 とちょうど 2.0 倍で線形で、epoch は 1 のまま。したがって 2.6 を超える分は、tab と無関係な、すでにある深さに比例する wall clock の要因（shodo-d77 と shodo-2j6 の記録でも未解決として挙げている）で、tab prefix の修正の効果を損なうものではない。この言い直しは測定後のものであることを明記しておく。

対照は ±3% に収まった。`nested` は −2.2%〜+2.3%、`siblings` は −0.7%〜+1.5%、`plain` は −1.0%〜+0.5%（個別の最大 Δ は `nested` 160 の ABBA +2.0% と BAAB +2.3%。candidate が速いラウンドは 4/12 だった）。ruby も tab もない経路は変わらず、この差は揺らぎの範囲と考える。`plain` 800 の BAAB には、1 ラウンドだけ比 1.40 の外れ値があった。

## 方法

- baseline は `1b2e1b7`（main。PR #224 のマージ後）に probe の example と `Cargo.toml` の `[[example]]` を足したもの。candidate は `5cd6efaf92d5d3240f4bd34c0b8ace67f24ffa44`（ブランチ fix/shodo-b7d-tab-prefix のコード。probe を足した commit はこの後で、ライブラリのコードは変わらない）。
- baseline と candidate は target directory を分けた別々の release バイナリ。SHA-256 は baseline が `dbc86c128591f85e796a329c47e17d04942209fbdf91f85b6f6861a0367efdf3`、candidate が `01dfe6806a9245c95a45e1770a9c323e1cca28d9d42330ee08c1d45eba736344`。
- `dev/bench/examples/tab_scale.rs` の 5 ケース。すべてのテキストと ruby のスタイル（段落の root と ruby の content builder を含む）が `white-space-collapse: Preserve`（`white-space: pre`）と `tab-size: 40px`、`INLINE_SIZE = 96`、フォントは固定 CJK の `FONTS[1]`、annotation は `"日"`。
  - `nested`: `size` 個の ruby を 1 つの base に入れ子にし、最内側は `"日"`。
  - `nestedtab`: `nested` と同じで、最内側が `"日\t日"`。
  - `siblings`: base `"12"` の ruby を `size` 個並べる。分割不能な 1 行。
  - `siblingstab`: 先頭に `"\t"` を置いた `siblings`。
  - `plain`: ruby なし。`"日日日\t"` を `size` 回繰り返す対照。
- 計時範囲は `break_all` の `reps` 回と、各回の結果と `LayoutContext` の解放。build、出力 digest、`LayoutContext` の準備は範囲外。解放を計時に含めるのは shodo-2j6 と同じで、より速い arm に不利な向きに働く。
- 12 ラウンドを ABBA 順で測った後、BAAB 順で独立に 12 ラウンド測った。各ラウンドでは同じラベルの 2 サンプルを平均した。全ケースを同じ回数で測り、ラウンドも点も減らしていない。測定前に、両バイナリが全ケースの最小サイズで同じ `output_sha256` と `warning_sha256` を出すことを確かめた。
- rustc 1.96.0 (ac68faa20 2026-05-25)、AMD Ryzen 5 5600G with Radeon Graphics、Linux 7.2.5-3-omarchy。scaling governor は `performance`。測定中は他の重い処理を走らせていない。全サンプルと環境情報は [raw JSON](data/shodo-b7d-tab-prefix.json) に記録した。

## 結果

`Δ` は candidate と baseline の中央値の差で、負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。時間は 1 回の `break_all` あたり。

| ケース | size | reps | baseline (ABBA) | candidate (ABBA) | ABBA Δ | ABBA 速い比較 | baseline (BAAB) | candidate (BAAB) | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| nested | 20 | 4 | 718.6 µs | 727.2 µs | +1.2% | 5/12 | 731.3 µs | 715.5 µs | −2.2% | 9/12 |
| nested | 40 | 2 | 2.28 ms | 2.27 ms | −0.6% | 7/12 | 2.25 ms | 2.25 ms | −0.0% | 7/12 |
| nested | 80 | 1 | 6.08 ms | 6.15 ms | +1.2% | 5/12 | 6.15 ms | 6.13 ms | −0.2% | 6/12 |
| nested | 160 | 1 | 15.41 ms | 15.73 ms | +2.0% | 4/12 | 15.75 ms | 16.12 ms | +2.3% | 4/12 |
| nestedtab | 20 | 4 | 28.79 ms | 908.9 µs | −96.8% | 12/12 | 28.79 ms | 905.3 µs | −96.9% | 12/12 |
| nestedtab | 40 | 2 | 171.80 ms | 2.64 ms | −98.5% | 12/12 | 171.37 ms | 2.63 ms | −98.5% | 12/12 |
| nestedtab | 80 | 1 | 937.14 ms | 6.79 ms | −99.3% | 12/12 | 937.98 ms | 6.99 ms | −99.3% | 12/12 |
| nestedtab | 160 | 1 | 5.04 s | 17.78 ms | −99.6% | 12/12 | 5.06 s | 17.37 ms | −99.7% | 12/12 |
| siblings | 200 | 1 | 5.95 ms | 6.04 ms | +1.5% | 4/12 | 6.06 ms | 6.03 ms | −0.5% | 8/12 |
| siblings | 400 | 1 | 13.00 ms | 12.96 ms | −0.3% | 5/12 | 13.04 ms | 13.11 ms | +0.6% | 3/12 |
| siblings | 800 | 1 | 28.51 ms | 28.46 ms | −0.2% | 7/12 | 28.79 ms | 28.59 ms | −0.7% | 5/12 |
| siblingstab | 200 | 1 | 737.41 ms | 7.08 ms | −99.0% | 12/12 | 745.94 ms | 7.08 ms | −99.1% | 12/12 |
| siblingstab | 400 | 1 | 3.12 s | 15.42 ms | −99.5% | 12/12 | 3.13 s | 15.72 ms | −99.5% | 12/12 |
| siblingstab | 800 | 1 | 13.25 s | 34.12 ms | −99.7% | 12/12 | 13.29 s | 34.30 ms | −99.7% | 12/12 |
| plain | 200 | 50 | 376.3 µs | 374.1 µs | −0.6% | 9/12 | 378.3 µs | 374.7 µs | −1.0% | 8/12 |
| plain | 800 | 10 | 1.50 ms | 1.50 ms | +0.3% | 5/12 | 1.50 ms | 1.50 ms | +0.5% | 3/12 |

| ケース | 側 | ABBA | BAAB |
| --- | --- | --- | --- |
| nested | baseline | 3.18 / 2.66 / 2.54 | 3.08 / 2.73 / 2.56 |
| nested | candidate | 3.12 / 2.71 / 2.56 | 3.15 / 2.72 / 2.63 |
| nestedtab | baseline | 5.97 / 5.45 / 5.38 | 5.95 / 5.47 / 5.40 |
| nestedtab | candidate | 2.91 / 2.57 / 2.62 | 2.90 / 2.66 / 2.49 |
| siblings | baseline | 2.18 / 2.19 | 2.15 / 2.21 |
| siblings | candidate | 2.15 / 2.20 | 2.18 / 2.18 |
| siblingstab | baseline | 4.23 / 4.25 | 4.20 / 4.24 |
| siblingstab | candidate | 2.18 / 2.21 | 2.22 / 2.18 |

後半の表はサイズを倍にしたときの時間の比（各サイズの中央値の比を小さいサイズから順に。`nested` と `nestedtab` は 20→40→80→160、`siblings` と `siblingstab` は 200→400→800）。baseline の `nestedtab` は 5.4–6.0 倍、`siblingstab` は 4.2 倍で 2 乗のまま。candidate の `siblingstab` は 2.2 倍前後で、`nestedtab` は `nested` の対照と同じ曲線（上の結論に書いた理由で 2.6 を超える点がある）。

## 操作数

wall clock に依存しない指標として、ignored テスト `accumulate_tests::b7d_operation_counts_report` が 1 行の走査で container の計測回数と、範囲キャッシュの世代 `epoch` と `fills`（飽和を埋めた tab step の数）を数える。

| shape | 経路 | r | container_measures | replayed | width_calls | epoch | fills |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| nested | 参照 | 16 | 3,104 | 0 | 6,592 | 1 | 33 |
| nested | 参照 | 32 | 12,352 | 0 | 25,472 | 1 | 65 |
| nested | 参照 | 64 | 49,280 | 0 | 100,096 | 1 | 129 |
| nested | memo | 16 | 48 | 0 | 480 | 1 | 33 |
| nested | memo | 32 | 96 | 0 | 960 | 1 | 65 |
| nested | memo | 64 | 192 | 0 | 1,920 | 1 | 129 |
| nested | memo | 128 | 384 | 0 | 3,840 | 1 | 257 |
| nested | accumulator | 16 | 48 | 0 | 480 | 1 | 33 |
| nested | accumulator | 32 | 96 | 0 | 960 | 1 | 65 |
| nested | accumulator | 64 | 192 | 0 | 1,920 | 1 | 129 |
| nested | accumulator | 128 | 384 | 0 | 3,840 | 1 | 257 |
| nestedtab | 参照 | 16 | 3,136 | 0 | 6,660 | 1 | 33 |
| nestedtab | 参照 | 32 | 12,416 | 0 | 25,604 | 1 | 65 |
| nestedtab | 参照 | 64 | 49,408 | 0 | 100,356 | 1 | 129 |
| nestedtab | memo | 16 | 48 | 0 | 484 | 1 | 33 |
| nestedtab | memo | 32 | 96 | 0 | 964 | 1 | 65 |
| nestedtab | memo | 64 | 192 | 0 | 1,924 | 1 | 129 |
| nestedtab | memo | 128 | 384 | 0 | 3,844 | 1 | 257 |
| nestedtab | accumulator | 16 | 48 | 0 | 484 | 1 | 33 |
| nestedtab | accumulator | 32 | 96 | 0 | 964 | 1 | 65 |
| nestedtab | accumulator | 64 | 192 | 0 | 1,924 | 1 | 129 |
| nestedtab | accumulator | 128 | 384 | 0 | 3,844 | 1 | 257 |
| siblings | 参照 | 16 | 1,920 | 0 | 4,256 | 1 | 33 |
| siblings | 参照 | 32 | 7,424 | 0 | 15,680 | 1 | 65 |
| siblings | 参照 | 64 | 29,184 | 0 | 60,032 | 1 | 129 |
| siblings | memo | 16 | 288 | 0 | 992 | 1 | 33 |
| siblings | memo | 32 | 1,088 | 0 | 3,008 | 1 | 65 |
| siblings | memo | 64 | 4,224 | 0 | 10,112 | 1 | 129 |
| siblings | memo | 128 | 16,640 | 0 | 36,608 | 1 | 257 |
| siblings | accumulator | 16 | 49 | 239 | 514 | 1 | 33 |
| siblings | accumulator | 32 | 97 | 991 | 1,026 | 1 | 65 |
| siblings | accumulator | 64 | 193 | 4,031 | 2,050 | 1 | 129 |
| siblings | accumulator | 128 | 385 | 16,255 | 4,098 | 1 | 257 |
| siblingstab | 参照 | 16 | 1,965 | 0 | 4,400 | 1 | 33 |
| siblingstab | 参照 | 32 | 7,469 | 0 | 15,824 | 1 | 65 |
| siblingstab | 参照 | 64 | 29,229 | 0 | 60,176 | 1 | 129 |
| siblingstab | memo | 16 | 291 | 0 | 1,052 | 1 | 33 |
| siblingstab | memo | 32 | 1,091 | 0 | 3,068 | 1 | 65 |
| siblingstab | memo | 64 | 4,227 | 0 | 10,172 | 1 | 129 |
| siblingstab | memo | 128 | 16,643 | 0 | 36,668 | 1 | 257 |
| siblingstab | accumulator | 16 | 52 | 239 | 574 | 1 | 33 |
| siblingstab | accumulator | 32 | 100 | 991 | 1,086 | 1 | 65 |
| siblingstab | accumulator | 64 | 196 | 4,031 | 2,110 | 1 | 129 |
| siblingstab | accumulator | 128 | 388 | 16,255 | 4,158 | 1 | 257 |

`nestedtab` の参照は depth 16 / 32 / 64 で 3,136 / 12,416 / 49,408 container の計測で、これは修正前の使い捨ての probe で測った値と一致する。memo と accumulator は 48 / 96 / 192 / 384 と、倍化ごとにちょうど 2.0 倍になる。`nested` と比べた増分は width_calls の 4（tab を含む `"日\t日"` の幅問い合わせ）だけ。`epoch` はどの経路・どの size でも 1 で、新しい range の開始で世代が動かなくなったことを示す。`siblingstab` も同じで、memo が 291 / 1,091 / 4,227、accumulator が 52 / 100 / 196 と `siblings` に tab 分の 3 を足しただけの値になる。

## 原因

`white-space: pre` の tab を 1 つ置くだけで、`line::range::width` の tab prefix が新しい range の開始のたびに作り直されていた。その作り直しが範囲キャッシュの世代（epoch）を毎回進め、ruby の測定が持つ memo / accumulator のエントリがすべて無効になって、参照と同じ 2 乗の再計測に落ちていた。shodo-d77 の `nestedtab` probe は既定の `white-space: normal` で測ったため、tab が空白に畳まれて tab prefix に届かず、2 乗を再現できなかった。

## 変更

- tab prefix を疎にした。tab の step だけを保存し、tab のない文字ごとに要素を持たない。
- 範囲キャッシュの世代は、tab の step が `Saturation` を変えるときだけ動かす。prefix の計算で飽和が出る step は世代を進めずに「埋める」（fill）。飽和を含む prefix を捨てるときだけ世代を「無効化」する（invalidate）。tab の幅が有効（effectful）でない通常の場合、世代は動かない。
- tab 幅の課金は変えていない。出力はバイト単位で同一で、`line::range::tests::tab_width_queries_match_the_golden_record`（旧コードの tab 幅と飽和の回数を固定したテスト）が変更前後で同じ結果を返すことで固定している。

## 同値性

- 成長のガード: `nestedtab` の memo と accumulator が depth 16 / 32 / 64 で 48 / 96 / 192、epoch 1 を固定するテスト、`preserved_tabs_keep_nested_measures_linear`、`preserved_tabs_keep_sibling_measures_linear`。
- reference との比較: Reference / Memo / Accumulate / Verify の 4 つの経路が同じ結果を返すことを確かめる同値性テスト、`tab_fixtures_match_reference`。
- 置き換え: `effectful_tab_prefix_replacement_stops_replay`（有効な tab prefix が途中で置き換わると再生を止める）、`clean_tab_prefix_replacement_keeps_replay`（飽和のない置き換えでは再生を続ける）。
- 世代の規則: `line::range::tests::generation_splits_into_monotone_fills_and_invalidating_epochs`、`line::range::tests::effectful_tab_steps_fill_and_their_discard_invalidates`、tab 幅の固定は上述の `tab_width_queries_match_the_golden_record`。
- 出力の同一性は、baseline と candidate の出力・警告の SHA-256 が全 1536 サンプルで一致したことでも確かめた。

## 残る最悪形

- 新しい start ごとに、range 内の tab の数に比例して `O(range 内の tab 数 · log n)` の処理が残る。D 個の異なる start と T 個の tab が range に入る形では、全体で `O(D · T · log n)` になる。tab が多い段落では、この項が支配的になりうる。この記録では計測していない。
- tab の大きさが有効な値（effectful。飽和を出しうる tab-size）の場合は、従来どおり無効化が起きる。この記録の probe は `40px` の通常値で、この形は測っていない。
- この 2 つの作業量の上限と fail-closed の扱いは shodo-mc0 に引き継ぐ。

## 再計測

```sh
# 操作数
cargo test -p shodo --lib accumulate_tests::b7d_operation_counts_report -- --ignored --nocapture | grep '^{' > <dir>/counts.jsonl
# probe（1 サンプル 1 行の JSON）
cargo build --release -p shodo-bench --example tab_scale
target/release/examples/tab_scale sample <case> <size> <label> <index> <reps>
# baseline: 1b2e1b7 の worktree に probe と [[example]] name = "tab_scale" を足し、別の CARGO_TARGET_DIR で build
S=dev/bench/scripts/shodo-b7d
# 12 ラウンド ABBA + 12 ラウンド BAAB（jq が必要。約 30–60 分）
BASE=<baseline の tab_scale> CAND=<candidate の tab_scale> OUT=<dir>/samples.jsonl bash $S/run.sh
python3 $S/summarize.py <dir>/samples.jsonl > <dir>/summary.json
python3 $S/tables.py <dir>/summary.json > <dir>/tables.md
# <dir> には binaries.txt（baseline/ と candidate/ という名前のディレクトリに置いた両バイナリの sha256sum 出力）、counts.jsonl、candidate_commit.txt も置く
python3 $S/assemble.py <decision> <candidate commit> <dir> <baseline worktree> docs/records/data/shodo-b7d-tab-prefix.json
```

`<case>` は `nested`, `nestedtab`, `siblings`, `siblingstab`, `plain` のいずれか。本記録の size と reps は、nested / nestedtab が 20:4、40:2、80:1、160:1、siblings / siblingstab が 200 / 400 / 800 でどれも 1、plain が 200:50 と 800:10。
