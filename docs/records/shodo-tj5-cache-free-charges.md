# shodo-tj5: ruby 行計測の課金をキャッシュ状態から切り離す

## 結論

採用する。`line::range` の範囲キャッシュ（`RangeCache`）に当たった問い合わせも、同じ段落・同じ範囲を冷えた状態で測ったときと同じだけ reshape 予算と `Saturation` を課金するようにした。課金はキャッシュの中身に依存しなくなったので、shodo-d77 の memo と shodo-2j6 の accumulator がこの非対称を避けるために持っていた世代（`generation`）と fill（`fills`）のゲートを削除した。無効化（`epoch`）は保守的な安全策として残した。

- 出力（行の geometry）は、比べたすべての probe とサイズで main と SHA-256 が一致した。
- 警告は、tab-size が飽和する新しいケース（`nestedhugetab`、`siblingshugetab`）だけで変わった。変わったのは `"N values saturated"` の件数 N だけで、警告の種類と数、ほかの文は同じ。それ以外の全ケースで警告の SHA-256 は一致した。
- 速度は、比べたケースで退行がない。memo と accumulator が冷えた記録も保存して再生するようになり、container の計測回数はむしろ減った（`nested` の memo で 48 → 32 per 16 段など。「操作数」参照）。
- shodo-b7d に残っていた「飽和する tab の step があると新しい start ごとに無効化されて二次に戻る」形が消えた。`nestedhugetab` 80 段は 861.74 ms から 5.98 ms（−99.3%、10/10 ラウンドで速い）。

## 原因

`RangeCache` の 3 か所が、初回の計測でだけ副作用を起こし、キャッシュに当たった問い合わせではそれを省いていた。

1. `blocks`（`block_size`）: lane の block size を初めて測るとき、`metric_index::measure` から `windows::measure` を通じて edge window の reshape 予算を課金し、`Saturation` も足す。キャッシュに当たると計測ごと省くので、課金も加算も起きなかった。issue の再現（siblings fixture で cold 2952 bytes、warm 2808 bytes）はこれ。
2. `sets`（`build`）: 段落全体の範囲コストを作るとき、全単位の幅の `Saturation` を、最初に問い合わせた呼び出し元に一度だけ課金していた。2 回目以降は 0。
3. tab prefix: tab の step は計算したときだけ `Saturation` を課金し、既にカバーされた step を使う問い合わせは課金しなかった。別の start のために prefix を作り直すと、また課金した。

`windows.rs` の edge window キャッシュは「ヒットでもミスと同じく課金する。結果が以前のレイアウトがコンテキストに残したものに依存しないように」という原則で書かれているが、`blocks` はその上の層でヒットを返していたので、この原則を破っていた。shodo-d77 と shodo-b7d は main と byte 一致を保つために、この非対称をそのままにして、memo の側で世代番号を見て避けていた（キャッシュを埋めた計測は memo しない、飽和する tab の step を捨てるときは無効化する）。

## 変更

- `blocks`: 初回の計測を `line::replay::begin` / `finish` で記録し、`Effects`（課金の集約、`Saturation`、警告の抑制状態）を高さと一緒に保存する。キャッシュに当たったら `line::replay::replay` で再生する。これは memo が使っている厳密なゲートで、今の `edge_reshape_spent` と抑制状態のもとで同じ課金結果（受理または拒否）と警告なしが保証できるときだけ再生する。ゲートが拒否したら、その場で計測し直してエントリを置き換える（冷えた計測と同じ動作）。計測中に警告が出たら（`finish` が `None`）保存しない。
  - メモリ: 副作用のないエントリ（課金なし、`Saturation` なし、抑制なしで記録。`Effects::is_empty`）は従来の `blocks` に高さだけで置き、副作用のあるエントリだけを別の map（`block_effects`）に `(高さ, Effects)` で置く。ふつうのエントリの大きさは変わらない。
- `sets`: `build` は呼び出し元に何も課金しない。単位ごとに局所の `Saturation` で幅を測り、飽和した単位だけを `(単位番号, 累積の Saturation)` の疎な列に記録する。`width` は問い合わせた範囲の単位の分を `partition_point` 2 回で課金する。ふつうの段落では列は空。
- tab prefix: step ごとの `Saturation` の累積を `extra` と並べて持つ（すべての step が clean の間は空。最初に飽和した step で 0 を埋める）。step を計算するときには課金せず、問い合わせのたびに、end より前のカバー済みの tab の累積を課金する。prefix を別の start のために作り直しても、どの問い合わせの副作用も変わらない。
- `RangeCache::generation` と `fills` を削除した。memo（`ruby/measure.rs`）は世代の代わりに `epoch` で検証し、キャッシュを埋めた記録も保存する。accumulator（`ruby/accumulate.rs::live`）は `fills` のゲートを外した。`epoch` は `begin`（新しい root）と `vacate_slots` でしか動かない。

## 挙動の変化

課金は (段落, atomic revision, 範囲, `edge_reshape_spent`, 抑制状態) の関数になった。main からの違いは次のとおり。

- `"N values saturated"` の N: 飽和する tab の step は、カバー済みの問い合わせでも課金されるようになったので増える。`build` の飽和は最初の問い合わせにまとめてではなく、範囲ごとに課金される。tab の golden（`line/testdata/b7d_tab_golden.txt`）は `huge` の 19 行だけが変わり、値は 1 つも変わらず、飽和の件数だけが変わった（例: `0..6` が 1 → 6、`0..2` が 0 → 1）。probe では `nestedhugetab` 20 段が 10372 → 10493、`siblingshugetab` が 3 → 4。
- reshape 予算: `blocks` のヒットも課金するので、操作ごとの予算（`max_reshape_window_bytes × 64`、既定で 262,144 bytes）に以前より早く届きうる。届けば `"line edge reshape budget exceeded; keeping shared glyphs"` が早く出て、共有グリフを保つ（fail-closed の方向）。比べた probe とテストの fixture では予算に届かず、出力は変わらなかった。
- 警告を出した `blocks` の計測は保存しないので、同じ範囲を再び測ると再び警告する（ほかのキャッシュを持たない経路、たとえば `width` の `windows::delta` と同じ）。window 予算の警告のように毎回決まって出る警告は、main では 1 回だったものが上限まで繰り返されるので、警告の上限（既定 `max_warnings: Some(1024)`）に早く届き、ほかの種類の警告がそのぶん早く打ち切られうる。上限に達して sink が抑制されると、抑制状態つきで保存されるようになる。

## 方法

- baseline は `9f52ff2`（main。PR #230 のマージ後）に、candidate の `dev/bench/examples/tab_scale.rs` を上書きしたもの。candidate はブランチ fix/shodo-tj5-cache-free-charges の `6b37d7a`（ライブラリのコード）に同じ probe を置いたもの。target directory を分けた別々の release バイナリ（`tab_scale` の SHA-256 は baseline が `3c86ce73f98fb240…`、candidate が `31ff9ba6df23f436…`）。
- probe は `tab_scale`（shodo-b7d。`nestedhugetab` と `siblingshugetab` を本件で追加。`tab-size: 1e12px` で、tab の step が飽和する）、`sibling_scale`（shodo-2j6）、`d77_scale`（shodo-d77）。
- 出力と警告の比較: 各 probe の全ケースを 2 つのサイズ以上で 1 回ずつ実行し、`output_sha256` と `warning_sha256` を比べた。
- 時間: 13 ケースを 10 ラウンド、奇数ラウンドは ABBA、偶数ラウンドは BAAB の順で実行した。各ラウンドで同じラベルの 2 サンプルを平均し、ラウンドの中央値を比べた。時間は probe が測る `break_all` 1 回あたりの時間。`perf stat -r 1 -e task-clock` でプロセス全体（build を含む）の task-clock も取った。
- rustc 1.96.0 (ac68faa20 2026-05-25)、AMD Ryzen 5 5600G with Radeon Graphics、Linux 7.2.5-3-omarchy。scaling governor は `performance`。生データはローカルに置き、commit していない（#227 の方針）。

## 結果

`Δ` は candidate と baseline の中央値の差で、負値は candidate が速い。「速いラウンド」は 10 ラウンド中で candidate が速かった数。

| probe | ケース | size | baseline | candidate | Δ | 速いラウンド | task-clock Δ | 出力 | 警告 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- |
| d77_scale | nested | 80 | 6.13 ms | 6.24 ms | +1.8% | 4/10 | −0.5% | 一致 | 一致 |
| d77_scale | ordinary | 800 | 35.22 ms | 34.02 ms | −3.4% | 9/10 | +0.7% | 一致 | 一致 |
| sibling_scale | outer | 800 | 44.57 ms | 38.47 ms | −13.7% | 10/10 | −5.8% | 一致 | 一致 |
| sibling_scale | rtl | 800 | 28.88 ms | 27.21 ms | −5.8% | 9/10 | −2.9% | 一致 | 一致 |
| sibling_scale | siblings | 1600 | 62.17 ms | 57.97 ms | −6.8% | 9/10 | −2.6% | 一致 | 一致 |
| sibling_scale | valign | 800 | 28.80 ms | 26.77 ms | −7.1% | 10/10 | −3.4% | 一致 | 一致 |
| tab_scale | nested | 160 | 15.76 ms | 15.64 ms | −0.7% | 6/10 | +0.0% | 一致 | 一致 |
| tab_scale | nestedhugetab | 80 | 861.74 ms | 5.98 ms | −99.3% | 10/10 | −98.6% | 一致 | 件数のみ |
| tab_scale | nestedtab | 160 | 17.69 ms | 15.23 ms | −13.9% | 10/10 | −7.0% | 一致 | 一致 |
| tab_scale | plain | 800 | 1.54 ms | 1.52 ms | −1.1% | 8/10 | −0.6% | 一致 | 一致 |
| tab_scale | siblings | 800 | 30.05 ms | 27.91 ms | −7.1% | 8/10 | −2.9% | 一致 | 一致 |
| tab_scale | siblingshugetab | 400 | 15.64 ms | 14.71 ms | −5.9% | 8/10 | −2.0% | 一致 | 件数のみ |
| tab_scale | siblingstab | 800 | 35.67 ms | 32.78 ms | −8.1% | 10/10 | −2.3% | 一致 | 一致 |

`d77_scale nested 80` の +1.8% は 4/10 ラウンドで、task-clock は −0.5%。揺らぎの範囲と考える。出力と警告の 1 回ずつの比較は、上の表のほかに `tab_scale` の nested / nestedtab 20、siblings / siblingstab 200、plain 200、nestedhugetab 20 / 40、siblingshugetab 100 / 200、`sibling_scale` の siblings 200、outer 100、valign / rtl 200、ordinary 200 / 800、plain 200 / 800、`d77_scale` の nested / nestedtab 20 / 160、siblings 100 / 400、ordinary 200 / 800、plain 200 / 800 で行い、hugetab 以外はすべて一致した。

`nestedhugetab` の baseline は 20 / 40 / 80 段で 26.4 / 163.9 / 886.9 ms（1 回ずつの比較）と 2 乗以上で伸び、candidate は 0.82 / 2.23 / 5.72 ms。`siblingshugetab` は baseline でも 2 乗にならなかった。先頭の tab の位置が 0 で、その step は飽和せず、ruby の内側に tab がないためと考える（確認はしていない）。

## 操作数

ignored テスト `accumulate_tests::b7d_operation_counts_report`（`fills` の列を外し、hugetab の 2 つの形を足した）。1 行の `break_all` の container の計測回数（cm）、accumulator が再生した position（rep）、memo のヒット（hits）、`width` の呼び出し（w）、`epoch`。

| 形 | 経路 | r=16 | r=32 | r=64 | r=128 | epoch |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| nested | 参照 cm | 3,104 | 12,352 | 49,280 | — | 1 |
| nested | memo cm（shodo-b7d の記録） | 32（48） | 64（96） | 128（192） | 256（384） | 1 |
| nested | accumulator cm | 32 | 64 | 128 | 256 | 1 |
| nestedhugetab | 参照 cm | 3,136 | 12,416 | 49,408 | — | 1 |
| nestedhugetab | memo / accumulator cm | 32 | 64 | 128 | 256 | 1 |
| siblings | memo cm（shodo-b7d の記録） | 152（288） | 560（1,088） | 2,144（4,224） | 8,384（16,640） | 1 |
| siblings | accumulator cm（shodo-b7d の記録） | 33（49） | 65（97） | 129（193） | 257（385） | 1 |
| siblingstab | accumulator cm | 37 | 69 | 133 | 261 | 1 |
| siblingshugetab | accumulator cm | 34 | 66 | 130 | 258 | 1 |

参照の値は shodo-b7d の記録と同じ。memo と accumulator は、キャッシュを埋めた最初の計測も記録して再生するので、どの形でも計測回数が減った。倍化ごとの増え方は accumulator と nested 系の memo で 2.0 倍のまま（兄弟 ruby の memo は従来どおり 2 乗で、線形なのは accumulator だけ）。`epoch` はどの形でも 1 で、飽和する tab の prefix を作り直しても動かない。

## テスト

- `line::range::tests::block_size_charges_the_same_cold_and_warm`: 全範囲で、cold、warm、`vacate_slots` の後、新しいコンテキストの `(高さ, Saturation, spent)` が同じ。予算を課金する範囲が実際にあることも確かめる。
- `line::range::tests::refused_block_replays_measure_like_a_fresh_context`: 予算の上限の直前（`spent = 上限 − 1`）と抑制された sink のもとで、ゲートが再生を拒否したエントリが計測し直され、キャッシュを持たないコンテキストと `(高さ, Saturation, spent, 警告)` が同じになる。警告を出した計測の次の問い合わせも同じ。
- `line::replay::tests::only_unsuppressed_recordings_without_effects_are_empty` と、tab の golden の中で同じ (fixture, 範囲) の行が同じ値と件数を持つことの確認。
- `line::range::tests::width_charges_do_not_depend_on_query_order`: 全範囲の履歴を前から、後ろから、1 つずつ新しいコンテキストで問い合わせて、範囲ごとの `(値, Saturation, spent)` が同じ。tab-size 40px、飽和する単位を 1 つ持つ段落、tab-size 1e12px の 3 通り。
- `line::range::tests::covered_tab_steps_charge_like_computed_ones`、`only_cleared_caches_move_the_epoch`: 旧規則のテスト（fill と無効化の規則）を新しい規則に書き換えたもの。
- `ruby::memo_tests::cold_cache_fills_are_memoized`（旧 `cold_cache_fills_are_not_memoized`）、`ruby::accumulate_tests::entries_recorded_while_caches_fill_are_stored`（旧 `..._unstored`）、`effectful_tab_prefix_replacement_keeps_replay`（旧 `..._stops_replay`）。
- 線形のガード `saturating_tabs_keep_measures_linear` と、参照との同値性 `tab_fixtures_match_reference` に hugetab の形を追加。shodo-d77 / shodo-2j6 / shodo-b7d の線形ガードはそのまま通る。
- memo のヒット数を数えるテスト（`exact_probes_replay_look_ahead_entries` 12 → 13 per 兄弟、`memo_bounds_keep_look_ahead_hits` 16 → 19、`intrinsic_min_and_max_atomics_keep_separate_memo_entries`）は、冷えた記録も再生されるようになった分だけ期待値を更新した。

## 残る最悪形

- `blocks` の map には上限がない（main から）。副作用のあるエントリは `Effects` の分だけ大きい（1 エントリ数十バイト）。
- 警告を出す `blocks` の計測（edge window が window 予算に入らないなど）は保存しないので、`max_warnings: None` のとき同じ範囲を毎回計測し直す。`width` の `windows::delta` も同じ。上限は shodo-mc0 で扱う。
- 予算の境界をまたいだ計測（受理と拒否が混じる）は再生できないので、そのたびに計測し直す。各計測が予算を消費するので、回数は計測 1 回分の課金の数で抑えられる。
- shodo-b7d の残りのうち「新しい start ごとに O(範囲内の tab 数 · log n)」はそのまま。飽和する tab による無効化は本件で消えた。

## 再計測

```sh
# 操作数
cargo test -p shodo --lib accumulate_tests::b7d_operation_counts_report -- --ignored --nocapture | grep '^{'
# probe（1 サンプル 1 行の JSON）
cargo build --release -p shodo-bench --example tab_scale --example sibling_scale --example d77_scale
target/release/examples/tab_scale sample nestedhugetab 80 candidate 0 1
# baseline: 9f52ff2 の worktree に candidate の tab_scale.rs を置き、別の CARGO_TARGET_DIR で build する
```
