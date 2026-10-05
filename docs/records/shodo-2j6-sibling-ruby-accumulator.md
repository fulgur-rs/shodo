# shodo-2j6: 兄弟 ruby の container 単位差分 accumulator

## 判断

採用する。兄弟 ruby（base `"12"` の ruby を横に並べた 1 本の分割不能な行）の `break_all` から 2 乗の項が消え、R=800 で baseline の 1.41 s（BAAB は 1.36 s）が 29.57 ms（29.21 ms）に、R=1600 で 6.97 s（6.72 s）が 61.32 ms（61.59 ms）になった（−99.1%、ABBA・BAAB とも 12/12 ラウンドで candidate が速い）。サイズを倍にしたときの時間の比は、`siblings` の baseline が 4.03–4.96（ABBA）/ 4.05–4.95（BAAB）、candidate が 2.07–2.24 / 2.11–2.23。800→1600 の比は baseline 4.96 / 4.95、candidate 2.07 / 2.11 で、どの倍化も採否基準の 2.6 を下回る。外側 ruby の base に兄弟を入れた `outer` も 800 で −97.7% / −97.1%、倍化比は candidate が 2.05–2.18、baseline が 3.81–5.18 / 3.82–4.10。`valign`（上端揃えの背の高いグリフが同じ行にある兄弟）と `rtl`（RTL 段落の兄弟）も同じ形で、800 で −97.9% / −98.0%、倍化比は candidate が 2.08–2.25。

採否基準は測定前に固定した（計画の Task 11 Step 5）。(1) baseline/candidate の出力・警告の SHA-256 が全サンプルで一致する（`digest_mismatch` は空。全 1728 サンプル = 18 点 × 4 サンプル × 24 ラウンド）。(2) `siblings`/`outer` の candidate の倍化比が両順序のすべての倍化で 2.6 以下で、サイズ 400 以上のすべての点で両順序とも 12/12 ラウンド candidate が速い。(3) `plain`/`ordinary` で「両順序の Δ が +3% を超え、かつ 10/12 以上のラウンドで遅い」ことがない。(4) `valign`/`rtl` が同じ基準で baseline より遅くない。4 つとも満たした。

制御（ruby なしの `plain` と通常の breakable な `ordinary`）は、基準の意味では退行していないが、ごく小さい遅れが一貫して出ている点は記録しておく。Δ は `ordinary` 200/800 が +0.3%/+0.2%（ABBA）と +0.4%/+1.5%（BAAB）、`plain` 200/800 が +0.8%/+0.5% と +2.2%/+1.2% で、どれも +3% 未満。ただし candidate が速いラウンドは 2–4/12 にとどまる（遅いラウンドは 8–10/12）。個々のラウンドの最大比は 1.02–1.04（BAAB の `ordinary` 200 で 1.28、`plain` 800 で 1.39 の外れ値が 1 ラウンドずつあった）。ruby を含まない経路に accumulator は入らないため、原因は特定していない（コード配置やアライメントの違いによる定数程度の差と推定するが、確認していない）。再現性のある退行の基準（+3% 超かつ 10/12 以上）には届かないが、サイズが 4 倍になっても Δ が 0.2–2.2% の範囲に収まり、増えていない。

## 方法

- baseline は `70b3568d2f41c6cde947be7099a46d7f552d996d`（`70b3568`、main。PR #223 のマージ後）。candidate は `8a2bf99e436045d876d656b263bc8b322380ec2d`（ブランチ perf/shodo-2j6-sibling-ruby。probe と報告用テストを追加した commit で、この commit からバイナリを build した。その後の commit は計測スクリプトだけで、ライブラリのコードは変わらない）。
- baseline/candidate は target directory を分けた別々の release バイナリ。SHA-256 は baseline が `f07aac0af3c9c6d73010749e7dad6a37878ad9b48a1426646bfde1184104a079`、candidate が `3293cdf2a77369de0ee6a39f94fdc4fdf221cd0af55608dde0e5176ea2896e03`。baseline 側には probe の example と `Cargo.toml` の `[[example]]` だけを追加した。
- `dev/bench/examples/sibling_scale.rs` の 6 ケース（`INLINE_SIZE = 96`、`outer` だけ全候補が 1 行に収まる `1.0e7`、フォントは固定 CJK の `FONTS[1]`、annotation は `"日"`）。
  - `siblings`: base `"12"` の ruby を `size` 個並べる。分割不能な 1 行になる。
  - `outer`: 1 つの外側 ruby の base に `size` 個の（`"日"` + `"12"` の ruby）を入れ、reading は `"日"` × `size`（base 内に対になる cut がある）。1 行をスキャンする。
  - `valign`: `vertical-align: top` の 48 px の `"1"`（数字の base の前に break の機会がないので同じ行になる）の後に `siblings`。1 行であることを smoke 実行で確認した。
  - `rtl`: RTL 段落の `siblings`。
  - `ordinary`: `"日日"` の通常テキストと base `"日日"` の ruby の組を `size` 個並べる。breakable（200 組で 134 行）。
  - `plain`: ruby なし。`"日"` を `4 * size` 個並べる対照。
- 計時範囲は `break_all` の `reps` 回だけ。build、出力 digest、`LayoutContext` の準備は範囲外。`reps` は表のとおり（`siblings` 200 の baseline が 80 ms、1600 で 7 s など、遅い点は 1 回で十分長く、candidate 側は 4–61 ms。baseline と candidate には同じ reps を使った）。計画にあった「siblings 1600 の baseline が約 45 s」は実機では約 7 s だった。
- 12 ラウンドを ABBA 順で測定した後、BAAB 順で独立に 12 ラウンド測定した。各ラウンドでは同じラベルの 2 サンプルを平均した。全ケースを同じ回数で測り、ラウンドも点も減らしていない。
- rustc 1.96.0 (ac68faa20 2026-05-25)、AMD Ryzen 5 5600G with Radeon Graphics、Linux 7.2.5-3-omarchy。scaling governor は `performance`。測定中は他の重い処理を走らせていない。全サンプルと環境情報は [raw JSON](data/shodo-2j6-sibling-ruby-accumulator.json) に記録した。

## 時間

`Δ` は candidate と baseline の中央値の差で、負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。時間は 1 回の `break_all` あたり。

| ケース | size | reps | baseline (ABBA) | candidate (ABBA) | ABBA Δ | ABBA 速い比較 | baseline (BAAB) | candidate (BAAB) | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| siblings | 200 | 1 | 80.38 ms | 6.19 ms | −92.3% | 12/12 | 80.14 ms | 6.02 ms | −92.5% | 12/12 |
| siblings | 400 | 1 | 323.78 ms | 13.90 ms | −95.7% | 12/12 | 324.72 ms | 13.45 ms | −95.9% | 12/12 |
| siblings | 800 | 1 | 1.41 s | 29.57 ms | −97.9% | 12/12 | 1.36 s | 29.21 ms | −97.8% | 12/12 |
| siblings | 1600 | 1 | 6.97 s | 61.32 ms | −99.1% | 12/12 | 6.72 s | 61.59 ms | −99.1% | 12/12 |
| outer | 100 | 1 | 23.90 ms | 4.59 ms | −80.8% | 12/12 | 23.74 ms | 4.20 ms | −82.3% | 12/12 |
| outer | 200 | 1 | 91.19 ms | 9.89 ms | −89.2% | 12/12 | 90.77 ms | 9.12 ms | −90.0% | 12/12 |
| outer | 400 | 1 | 365.52 ms | 20.24 ms | −94.5% | 12/12 | 362.71 ms | 19.90 ms | −94.5% | 12/12 |
| outer | 800 | 1 | 1.89 s | 44.17 ms | −97.7% | 12/12 | 1.49 s | 43.12 ms | −97.1% | 12/12 |
| valign | 200 | 1 | 80.13 ms | 6.10 ms | −92.4% | 12/12 | 79.53 ms | 5.89 ms | −92.6% | 12/12 |
| valign | 400 | 1 | 325.15 ms | 13.48 ms | −95.9% | 12/12 | 321.91 ms | 13.24 ms | −95.9% | 12/12 |
| valign | 800 | 1 | 1.35 s | 28.27 ms | −97.9% | 12/12 | 1.33 s | 28.50 ms | −97.9% | 12/12 |
| rtl | 200 | 1 | 80.89 ms | 6.46 ms | −92.0% | 12/12 | 79.77 ms | 6.24 ms | −92.2% | 12/12 |
| rtl | 400 | 1 | 326.73 ms | 13.42 ms | −95.9% | 12/12 | 322.06 ms | 13.21 ms | −95.9% | 12/12 |
| rtl | 800 | 1 | 1.50 s | 29.51 ms | −98.0% | 12/12 | 1.32 s | 28.73 ms | −97.8% | 12/12 |
| ordinary | 200 | 10 | 7.81 ms | 7.83 ms | +0.3% | 4/12 | 7.58 ms | 7.61 ms | +0.4% | 2/12 |
| ordinary | 800 | 2 | 35.12 ms | 35.17 ms | +0.2% | 4/12 | 34.58 ms | 35.11 ms | +1.5% | 2/12 |
| plain | 200 | 200 | 382.5 µs | 385.5 µs | +0.8% | 3/12 | 379.3 µs | 387.5 µs | +2.2% | 2/12 |
| plain | 800 | 50 | 1.52 ms | 1.53 ms | +0.5% | 3/12 | 1.51 ms | 1.53 ms | +1.2% | 3/12 |

| ケース | 側 | ABBA | BAAB |
| --- | --- | --- | --- |
| siblings | baseline | 4.03 / 4.34 / 4.96 | 4.05 / 4.18 / 4.95 |
| siblings | candidate | 2.24 / 2.13 / 2.07 | 2.23 / 2.17 / 2.11 |
| outer | baseline | 3.81 / 4.01 / 5.18 | 3.82 / 4.00 / 4.10 |
| outer | candidate | 2.15 / 2.05 / 2.18 | 2.17 / 2.18 / 2.17 |
| valign | baseline | 4.06 / 4.16 | 4.05 / 4.13 |
| valign | candidate | 2.21 / 2.10 | 2.25 / 2.15 |
| rtl | baseline | 4.04 / 4.58 | 4.04 / 4.11 |
| rtl | candidate | 2.08 / 2.20 | 2.12 / 2.18 |

後半の表はサイズを倍にしたときの時間の比（各サイズの中央値の比を小さいサイズから順に。`siblings` は 200→400→800→1600、`outer` は 100→200→400→800、`valign` と `rtl` は 200→400→800）。baseline は約 4 倍から、大きいサイズで 5 倍近くまで伸びる。candidate は約 2.1–2.2 倍で、2 を少し超える分は、d77 の記録で述べた既存の O(D) 処理など計測の外にある定数の影響と考えている（確認はしていない）。

## 操作数

wall clock に依存しない指標として、ignored テスト `accumulate_tests::j6_operation_counts_report` が 1 行の `break_all` 相当の走査（`one_line`）で container の計測回数などを数える。`replayed` は accumulator が再生した container 数。`churn` は後述の残る最悪形。

| shape | 経路 | r | container_measures | replayed | width_calls | scalar_calls |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| siblings | 参照 | 16 | 1,920 | 0 | 4,256 | 1,936 |
| siblings | 参照 | 32 | 7,424 | 0 | 15,680 | 7,456 |
| siblings | 参照 | 64 | 29,184 | 0 | 60,032 | 29,248 |
| siblings | memo | 16 | 288 | 0 | 992 | 304 |
| siblings | memo | 32 | 1,088 | 0 | 3,008 | 1,120 |
| siblings | memo | 64 | 4,224 | 0 | 10,112 | 4,288 |
| siblings | accumulator | 16 | 49 | 239 | 514 | 65 |
| siblings | accumulator | 32 | 97 | 991 | 1,026 | 129 |
| siblings | accumulator | 64 | 193 | 4,031 | 2,050 | 257 |
| siblings | accumulator | 128 | 385 | 16,255 | 4,098 | 513 |
| outer | 参照 | 16 | 2,411 | 0 | 5,294 | 2,443 |
| outer | 参照 | 32 | 8,643 | 0 | 18,206 | 8,707 |
| outer | 参照 | 64 | 32,627 | 0 | 67,070 | 32,755 |
| outer | memo | 16 | 321 | 0 | 1,114 | 353 |
| outer | memo | 32 | 1,153 | 0 | 3,226 | 1,217 |
| outer | memo | 64 | 4,353 | 0 | 10,522 | 4,481 |
| outer | accumulator | 16 | 81 | 240 | 634 | 113 |
| outer | accumulator | 32 | 161 | 992 | 1,242 | 225 |
| outer | accumulator | 64 | 321 | 4,032 | 2,458 | 449 |
| outer | accumulator | 128 | 641 | 16,256 | 4,890 | 897 |
| churn | 参照 | 16 | 2,344 | 0 | 5,104 | 2,360 |
| churn | 参照 | 32 | 9,040 | 0 | 18,912 | 9,072 |
| churn | 参照 | 64 | 35,488 | 0 | 72,640 | 35,552 |
| churn | memo | 16 | 712 | 0 | 1,840 | 728 |
| churn | memo | 32 | 2,704 | 0 | 6,240 | 2,736 |
| churn | memo | 64 | 10,528 | 0 | 22,720 | 10,592 |
| churn | accumulator | 16 | 233 | 479 | 882 | 249 |
| churn | accumulator | 32 | 721 | 1,983 | 2,274 | 753 |
| churn | accumulator | 64 | 2,465 | 8,063 | 6,594 | 2,529 |
| churn | accumulator | 128 | 9,025 | 32,511 | 21,378 | 9,153 |

`container_measures` の倍化ごとの比は次のとおり。

| shape | 参照 | memo | accumulator |
| --- | --- | --- | --- |
| siblings | 3.87 / 3.93 | 3.78 / 3.88 | 1.98 / 1.99 / 1.99 |
| outer | 3.58 / 3.77 | 3.59 / 3.78 | 1.99 / 1.99 / 2.00 |
| churn | 3.86 / 3.93 | 3.80 / 3.89 | 3.09 / 3.42 / 3.66 |

`siblings`/`outer` の accumulator は倍化ごとにほぼ 2.0 倍（線形）で、参照と memo は約 3.6–3.9 倍（2 乗）。r=64 の `siblings` で container の計測は参照が 29,184、memo が 4,224、accumulator が 193。これを wall clock なしで固定するテストは、`sibling_container_measures_are_quadratic_without_the_accumulator`（参照と memo が 2 乗で伸びる。計数が仕事を取りこぼしていないことの確認）、`sibling_container_measures_grow_linearly`（と `sibling_container_measures_grow_linearly_with_the_accumulator`）、`outer_base_siblings_grow_linearly`（外側の base 内の兄弟）、`outer_base_descendant_reads_grow_linearly`（T5 の descendant 集計）、`accumulator_never_measures_more_containers_than_the_memo`（どの fixture でも memo 経路より多く計測しない）。

## メモリ

accumulator は 1 つの `LayoutContext` あたり最大 2 つ（`MAX_ACCUMULATORS`。intrinsic のスキャンは 2 つの start を交互に使う）で、それぞれ最大 16,384 container（`MAX_CONTAINERS`）。それを超える走査は accumulator を使わず through memo の経路になる。1 container あたりの上限は `BYTES_PER_CONTAINER = 640` バイトで、定常状態では約 300–450 バイト（設計の見積もり 300 バイトより大きいのは `Vec` の倍々確保の余裕と 160 バイトの木のノードによる）。`begin_reshape_operation`（`ruby_line` による操作途中のリセットを含む）で消去し、`RETAINED_CONTAINERS` を超えて伸びたベクタは解放する。`shrink_to` で accumulator を捨てる。固定するテストは `accumulator_memory_is_bounded_and_released_per_operation`、`cap_overflow_falls_back_and_releases_the_accumulator`、`reset_releases_large_allocations_only`、`accumulators_are_kept_for_the_two_latest_keys`。時間の測定ではメモリの実測（RSS など）は取っていない。

## 同値性

出力と挙動の同値性は reference 経路との比較で固定した。

- reference 比較: `accumulator_matches_reference_on_shodo_d77_fixtures`（d77 の fixture）、`sibling_fixtures_match_reference_with_the_step_oracle`（兄弟の fixture を 8 チャンクに分けて実行。`Mode::Verify` の step oracle が、再生される予定の汚れていない container をすべて live で計測してエントリと一致することを確かめる）、`neighbour_rule_matches_reference_on_overhang_siblings`、`profile_and_edge_rules_match_reference`、`outer_base_siblings_match_reference`、`narrow_retry_rollback_with_siblings_matches_reference`。
- オラクル: `growing_selections_change_recorded_neighbours_only_through_new_event_units`（と `_sweep`。D3 の前提を総当たりで確かめる）、`prefix_guard_is_exactly_no_sequential_saturation`、`tree_queries_match_a_linear_fold`、`selection_digest_differences_name_their_source_ranges`。
- review focus の 5 項目: `profile_measured_again_mid_step_resets_and_matches_reference`、`suppression_flip_mid_scan_refuses_older_entries`、`tab_prefix_replacement_mid_step_stops_replay`、`huge_readings_saturate_like_reference`、`three_keys_alternating_evict_and_restart_exactly`。
- dirty ルール（`ruby/accumulate.rs` の `Dirty`）。D1 new: 初めて訪れた位置は計測する。D2 clipped: 切り出された unit が変わった位置。D3 neighbour: 新しく選ばれた event unit ごとに、その視覚的な隣の unit を記録した位置（隣に依存する位置のみ）。D4 profile: 選択された行の profile（高さ・above）が変わったときの profile 依存の位置。D5 edge: 前のステップと異なる partial group・removal・replacement に units か記録した隣が交わる位置。D6 ancestor: 汚れた位置の祖先と、live の計測で値が変わった位置の祖先。再生可能なエントリを持たない位置（unstored）は常に汚れとして扱う。汚れていない run は、ordered segment tree の集計（own effects + calls × profile effects）を 1 回の再生として、既存の厳密な再生の条件（budget・saturation・suppression の一様性）を通して取り込む。T5 の descendant 集計（Σ調整、面積、内容の有無）は木の範囲問い合わせで答える。

baseline/candidate の出力 SHA-256 と build/layout warning の SHA-256 は、全 1728 サンプルで一致した（`digest_mismatch` は空）。

## 残る最悪形（shodo-mc0 へ）

accumulator がなくす 2 乗は「汚れる位置が各ステップで 1 つか少数」の形で、次の入力は今も 2 乗になりうる。実測したのは (1) だけで、残りは設計上の理由で、この記録では計測していない。

1. profile churn: 上端揃えの inline の中で、兄弟 ruby の後にそれまでより大きいグリフが続き、部分 group と profile が毎ステップ変わる。profile 依存の位置がすべて D4 で汚れ、毎ステップ再計測する。`j6_operation_counts_report` の `churn` では accumulator の `container_measures` が r=16/32/64/128 で 233/721/2,465/9,025（倍化比 3.09 / 3.42 / 3.66、参照の 3.9 に近づく）で、`dirty` のヒストグラムは Profile が 120/496/2,016/8,128（r(r−1)/2）。
2. RTL の isolate による隣の入れ替わり: 隣に依存する container が多数あり、新しい unit ごとにそれらの隣が変わる形（D3 が大量の位置を汚す）。
3. atomics を伴う `intrinsic_sizes`: min/max の atomics は revision が異なり、`RangeCache::begin` が切り替えのたびにキャッシュを消すので世代が動いて accumulator がリセットされる。
4. タブ（shodo-b7d）: 別の start の幅問い合わせのたびに tab prefix が置き換わり（世代が動き）、何も保存されない。
5. `max_warnings: None` で不正な atomics が多い形: 警告を出した container は保存しないので、毎ステップ再計測する。
6. 残り budget が上限に近い場合: 拒否される segment の gate が、汚れていない run を live で計測させる。
7. 調整の和が saturate する場合: 再生される run を 1 つずつ足す。
8. 走査が 16,384 container を超える場合: through memo の経路になる。

どの場合も、fallback は参照経路と同じ再計測だけで、fail-closed の上限は設けていない（出力・警告は変わらない）。shodo-mc0 に引き渡すテスト専用のカウンタは `ruby_container_measures` と `ruby_dirty` のヒストグラム。

## 制限

- accumulator は through memo の miss のとき、かつ 2 つ以上の container を訪れる経路でだけ動く。
- メモリ上限（1 container あたり 640 バイト）は設計の見積もり（約 300 バイト）より大きい（倍々確保の余裕）。
- 倍化比 2.07–2.25 は 2 を少し超える。操作数は 2.0 倍なので、計測の外にある wall-clock の要因（d77 の記録の O(D) 処理など）の可能性が高いが、確認していない。
- 制御（`plain`/`ordinary`）の +0.2–2.2% の遅れ（上記）は採否基準の範囲内だが原因未特定。
- shodo-b7d（tab の世代）、shodo-mc0（作業量の fail-closed の上限）、shodo-tj5（キャッシュ状態への依存の会計）は本変更の範囲外で、引き続き別 issue。

## 再計測

```sh
# 操作数（30 行の JSON）
cargo test -p shodo --lib accumulate_tests::j6_operation_counts_report -- --ignored --nocapture | grep '^{' > <dir>/counts.jsonl
# probe（1 サンプル 1 行の JSON）
cargo run --release -p shodo-bench --example sibling_scale -- sample <case> <size> <label> <index> <reps>
# baseline: 70b3568 の worktree に probe と [[example]] name = "sibling_scale" を足し、別の CARGO_TARGET_DIR で build
cargo build --release -p shodo-bench --example sibling_scale
S=dev/bench/scripts/shodo-2j6
# 12 ラウンド ABBA + 12 ラウンド BAAB（jq が必要。約 30 分）
BASE=<baseline の sibling_scale> CAND=<candidate の sibling_scale> OUT=<dir>/samples.jsonl bash $S/run.sh
python3 $S/summarize.py <dir>/samples.jsonl > <dir>/summary.json
python3 $S/tables.py <dir>/summary.json > <dir>/tables.md
# <dir> には binaries.txt（両バイナリの sha256sum 出力）、counts.jsonl、candidate_commit.txt も置く
python3 $S/assemble.py adopted <candidate commit> <dir> <baseline worktree> docs/records/data/shodo-2j6-sibling-ruby-accumulator.json
```

`<case>` は `siblings`, `outer`, `valign`, `rtl`, `ordinary`, `plain` のいずれか。本記録の reps は、siblings/outer/valign/rtl がすべて 1、ordinary 200/800 = 10/2、plain 200/800 = 200/50。サイズは siblings 200/400/800/1600、outer 100/200/400/800、valign と rtl 200/400/800、ordinary と plain 200/800。
