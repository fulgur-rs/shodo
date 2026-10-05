# shodo-d77: ruby 候補計測の through 単位メモ化

## 判断

採用する。ネストした ruby の候補計測から深さの 2 乗の項が消え、深さ 160 の `break_all` が baseline の約 3.44 s から 17.19 ms になった（ABBA −99.5%、BAAB −99.5%、どちらの順でも 12/12 ラウンドで candidate が速い）。深さを倍にしたときの時間の比は baseline が約 5.5–6.1 倍、candidate が 2.6–3.3 倍で、候補計測の操作数は参照経路が倍化ごとに約 3.7–4.1 倍、最適化経路が約 2.00–2.02 倍になる（「操作数」参照）。

ただし wall clock の倍化比は 2 まで下がっていない。candidate の深さ 80→160 の比は 2.58–2.67（20→40 は 3.27–3.29）で、操作数の 2 倍より大きい。この差は操作数では説明できない。レビュー時の perf プロファイル（D=240、build を含むプロセス全体）では `spacing_summary::RangeIndex::query_node` が約 20%、`Summary::join` が約 17%、`whitespace::obstructed` が約 16% を占めており、これが有力な原因と考えている（プロファイルからの推定で、証明はしていない）。`obstructed(data, end)`（`line/whitespace.rs`）は `end` から積み重なった D 個の Close unit を前方に走査し、`decoration::chain` は D 個の祖先をたどるので、`range::width` 1 回あたり O(D) かかる。`range::width` の呼び出し回数は D に線形なので、この部分は全体で D² になる。`RangeIndex` の木の高さも深さとともに伸びる。いずれも本変更より前からあるコードで、定数は小さく、build できる最大の深さでも DoS にはならない。計画の採否基準（倍化比 ≤ 2.5）は満たさないが、管理者の裁定により、ネストで両順序の再現性のある改善があり、対照に退行がないことで採用を判断した。判断の根拠は、すべてのネストサイズで両方の順序の 12/12 ラウンドが改善したことと、操作数が参照経路の 4 倍から 2 倍に下がったこと。

兄弟 ruby（base `"12"` の ruby を横に並べた 1 本の分割不能な行）は定数倍の改善にとどまる。R=400 で 2.54 s から 330.03 ms（−87.0%）になったが、倍化比は baseline 4.14–4.16、candidate 3.92–4.05 のままで、まだ 2 乗で伸びる。線形化は shodo-2j6 で扱う。

通常の breakable な ruby（`ordinary`）は 12/12 ラウンドで速くなり（800 組で −43.6% / −43.6%）、ruby なしの対照（`plain`）には再現性のある退行がない。`plain` は −2.0% から −2.9% で、速い比較は 10–12/12。一部のラウンドでは candidate が同程度か僅かに遅く（ラウンドごとの最大比は 1.003 と 1.096）、ノイズの範囲で、Δ の符号は常に速い側。再現性のある退行ではない。baseline/candidate の出力・警告の SHA-256 は全ケース・全サンプルで一致した。

## 方法

- baseline は `a93a3598e8636d22e1e3eb323855723ca149b953`（`a93a359`、main）。candidate は `f3744e1a4159da6c593054d6e0f32f3cd5709138`（ブランチ fix/shodo-d77-deep-ruby-break、probe を追加した commit。この commit からバイナリを build した）。
- baseline/candidate は target directory を分けた別々の release バイナリ。SHA-256 は baseline が `42c12f42e6f79f301db8d03038fbfef83c9d641b58f79a307bdad4969749bf54`、candidate が `7823ea143e415d9ff6335fe0174a20f141defa8837f720eed0de3ea777c13e1f`。baseline 側には probe の example と `Cargo.toml` の `[[example]]` だけを追加した。
- `dev/bench/examples/d77_scale.rs` の 5 ケース（`INLINE_SIZE = 96`、フォントは固定 CJK の `FONTS[1]`、annotation は `"日"`）。
  - `nested`: `size` 個の ruby を、それぞれ前の ruby を唯一の base として入れ子にする。最内の内容は `"日"`。分割不能な 1 行になる。
  - `nestedtab`: `nested` と同じで、最内の内容が `"日\t日"`（タブを 1 つ含む）。
  - `siblings`: base `"12"` の ruby を `size` 個並べる。分割不能な 1 行になる。
  - `ordinary`: `"日日"` の通常テキストと、base `"日日"` の ruby の組を `size` 個並べる。breakable で、200 組で 134 行になる。
  - `plain`: ruby なし。`"日"` を `4 * size` 個並べる対照。
- 計時範囲は `break_all` の `reps` 回だけ。build、出力 digest、`LayoutContext` の準備は範囲外。1 サンプルの `reps` は表のとおり（baseline の 1 サンプルが約 50–250 ms になるよう選んだが、`nested` 80/160、`nestedtab` 40 以上、`siblings` は 1 回でも 0.15–3.5 秒かかり、その candidate 側は 7–330 ms）。
- 12 ラウンドを ABBA 順（baseline/candidate/candidate/baseline）で測定した後、BAAB 順（candidate/baseline/baseline/candidate）で独立に 12 ラウンド測定した。各ラウンドでは同じラベルの 2 サンプルを平均した。全ケースを同じ回数で測り、遅い点を減らしていない。
- 最大の深さは 160。ruby 1 段が入れ子の深さ 2 を使い、既定の `NestingDepth` 上限 512 で 240 段を超えると build が失敗するため、320 以上は測れない。
- rustc 1.96.0 (ac68faa20 2026-05-25)、AMD Ryzen 5 5600G with Radeon Graphics、Linux 7.2.5-3-omarchy。scaling governor は `performance`。測定中に他の重い処理は走らせていない。全サンプルと環境情報は [raw JSON](data/shodo-d77-ruby-through-memo.json) に記録した。

## 時間

`Δ` は candidate と baseline の中央値の差で、負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。時間は 1 回の `break_all` あたり。

| ケース | size | reps | baseline (ABBA) | candidate (ABBA) | ABBA Δ | ABBA 速い比較 | baseline (BAAB) | candidate (BAAB) | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| nested | 20 | 10 | 17.21 ms | 745.5 µs | −95.7% | 12/12 | 17.27 ms | 739.2 µs | −95.7% | 12/12 |
| nested | 40 | 2 | 104.20 ms | 2.44 ms | −97.7% | 12/12 | 104.63 ms | 2.44 ms | −97.7% | 12/12 |
| nested | 80 | 1 | 576.38 ms | 6.66 ms | −98.8% | 12/12 | 581.98 ms | 6.57 ms | −98.9% | 12/12 |
| nested | 160 | 1 | 3.44 s | 17.19 ms | −99.5% | 12/12 | 3.42 s | 17.53 ms | −99.5% | 12/12 |
| nestedtab | 20 | 6 | 18.85 ms | 908.3 µs | −95.2% | 12/12 | 18.95 ms | 907.0 µs | −95.2% | 12/12 |
| nestedtab | 40 | 1 | 108.53 ms | 2.78 ms | −97.4% | 12/12 | 109.73 ms | 2.77 ms | −97.5% | 12/12 |
| nestedtab | 80 | 1 | 617.78 ms | 7.18 ms | −98.8% | 12/12 | 613.48 ms | 7.26 ms | −98.8% | 12/12 |
| nestedtab | 160 | 1 | 3.54 s | 18.89 ms | −99.5% | 12/12 | 3.58 s | 18.47 ms | −99.5% | 12/12 |
| siblings | 100 | 1 | 147.97 ms | 20.80 ms | −85.9% | 12/12 | 147.56 ms | 20.86 ms | −85.9% | 12/12 |
| siblings | 200 | 1 | 611.95 ms | 81.63 ms | −86.7% | 12/12 | 612.95 ms | 81.74 ms | −86.7% | 12/12 |
| siblings | 400 | 1 | 2.54 s | 330.03 ms | −87.0% | 12/12 | 2.55 s | 331.13 ms | −87.0% | 12/12 |
| ordinary | 200 | 10 | 13.82 ms | 7.58 ms | −45.2% | 12/12 | 13.87 ms | 7.63 ms | −45.0% | 12/12 |
| ordinary | 800 | 2 | 61.61 ms | 34.73 ms | −43.6% | 12/12 | 61.67 ms | 34.81 ms | −43.6% | 12/12 |
| plain | 200 | 200 | 393.4 µs | 384.3 µs | −2.3% | 10/12 | 394.0 µs | 384.8 µs | −2.3% | 12/12 |
| plain | 800 | 50 | 1.57 ms | 1.54 ms | −2.0% | 12/12 | 1.57 ms | 1.53 ms | −2.9% | 11/12 |

サイズを倍にしたときの時間の比（各サイズの中央値の比を小さいサイズから順に）。

| ケース | 側 | ABBA | BAAB |
| --- | --- | --- | --- |
| nested (20→40→80→160) | baseline | 6.05 / 5.53 / 5.97 | 6.06 / 5.56 / 5.88 |
| nested (20→40→80→160) | candidate | 3.27 / 2.73 / 2.58 | 3.29 / 2.70 / 2.67 |
| nestedtab (20→40→80→160) | baseline | 5.76 / 5.69 / 5.74 | 5.79 / 5.59 / 5.83 |
| nestedtab (20→40→80→160) | candidate | 3.06 / 2.59 / 2.63 | 3.05 / 2.63 / 2.54 |
| siblings (100→200→400) | baseline | 4.14 / 4.16 | 4.15 / 4.16 |
| siblings (100→200→400) | candidate | 3.93 / 4.04 | 3.92 / 4.05 |

baseline の `nested` は 2 乗よりも急（約 6 倍/倍化）に伸びる。candidate は約 2.6 倍まで下がるが、2 倍には届かない。

## 操作数

wall clock に依存しない指標として、ignored テスト `d77_operation_counts_report` が 1 つの分割不能なネスト行（`break_all`）と、保持した幅広いスキャンの後に狭い幅で再試行する `PartialLine::index` の 2 経路について、4 つの計数を深さ 8/16/32/64 で数える。参照経路（`cx.ruby_reference`）は D² の項を持つ従来の計測、最適化経路が本変更。

| 経路 | 参照/最適化 | D | range::width | scalar::measure | columns | walk |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| break_all | 参照 | 8 | 1,760 | 792 | 784 | 1,470 |
| break_all | 参照 | 16 | 6,592 | 3,120 | 3,104 | 6,014 |
| break_all | 参照 | 32 | 25,472 | 12,384 | 12,352 | 24,318 |
| break_all | 参照 | 64 | 100,096 | 49,344 | 49,280 | 97,790 |
| break_all | 最適化 | 8 | 240 | 32 | 24 | 126 |
| break_all | 最適化 | 16 | 480 | 64 | 48 | 254 |
| break_all | 最適化 | 32 | 960 | 128 | 96 | 510 |
| break_all | 最適化 | 64 | 1,920 | 256 | 192 | 1,022 |
| index | 参照 | 8 | 1,760 | 784 | 784 | 1,470 |
| index | 参照 | 16 | 6,592 | 3,104 | 3,104 | 6,014 |
| index | 参照 | 32 | 25,472 | 12,352 | 12,352 | 24,318 |
| index | 参照 | 64 | 100,096 | 49,280 | 49,280 | 97,790 |
| index | 最適化 | 8 | 224 | 16 | 16 | 126 |
| index | 最適化 | 16 | 448 | 32 | 32 | 254 |
| index | 最適化 | 32 | 896 | 64 | 64 | 510 |
| index | 最適化 | 64 | 1,792 | 128 | 128 | 1,022 |

参照経路は倍化ごとに 3.7–4.1 倍、最適化経路は 2.00–2.02 倍で伸びる。D=64 で参照経路の `range::width` は 100,096 回、最適化経路は 1,920 回（`break_all`）と 1,792 回（`index`）。この差を wall clock なしで固定するのが、テスト `reference_nested_candidates_grow_quadratically`（参照経路が 3 倍以上で伸びる。計数が仕事を取りこぼしていないことの確認）、`memoized_nested_candidates_measure_linearly`、`incremental_walk_keeps_nested_probes_linear`。

## メモリ

memo（`ruby/memo.rs`）は 1 回の操作（`next_line` か `intrinsic_sizes`）の間だけエントリを持つ。時間を測った `f3744e1` は、look-ahead のない probe（`through == end`）も含めて、汚れのない probe をすべて記録していた。look-ahead のないキーは 1 回の走査で 1 度しか引かれず、そのエントリは計測した経路のどれでも再生に使われなかった（保存してもしなくても操作数は同じ）。`intrinsic_sizes` は `(total_unit, i)` と `(word_unit, i)` を unit ごとに probe するので、ruby 1 個と CJK unit N 個の段落では 1 回の呼び出しの後に約 N 個のエントリが残った（N=16000 で 16,000 エントリ、容量 28,672、約 3 MB。レビューでは N=32000 で容量 57,344、約 6 MB）。`begin_reshape_operation` の消去は容量を残すため、このマップは `shrink_to` までコンテキストに残っていた。main にはない増加で、信頼できない入力では、既定の `max_text_bytes`（16 MiB、CJK で約 560 万 unit）までの長さで数百 MB を超えうる。

修正後の上限は次のとおり。

- look-ahead のある probe（`through > end`）だけを記録する。D² の項はそこからしか生じない。look-ahead のない probe は、同じ `through` で先に記録された look-ahead のエントリがあれば再生し（コンテナの終わりで止まる probe は、1 unit 手前の look-ahead probe とキーが同じ）、なければ参照経路と同じく記録せずに計測する。
- 同時に持つエントリは最大 1,024（`MAX_ENTRIES`、既定の `max_nesting_depth` 512 の 2 倍）。達したら次の挿入の前に全部消す。走査は 1 つの `through` を連続した end の範囲で引くので、いま使っている範囲は消えず、走査ごとの再利用は線形のまま。1 バケットは 105 バイト（キー 40、値 64、制御 1）で、上限でも約 210 KiB。
- 消去（操作の開始時と、注釈レーンの `ruby_line` による操作途中のリセット）では、容量が 256 を超えるマップを解放する。残るマップは最大 256 バケット、約 27 KiB。

`memo_stays_bounded_on_long_paragraphs`（ruby 1 個 + `"日"` × 1000/4000/16000）では、`intrinsic_sizes` の後はエントリ 0、容量 0、1 行の `next_line` の後も容量 3 になった。`memo_bounds_keep_look_ahead_hits` は、nested と siblings が引き続き memo を再生すること、2 base の ruby を 400 個並べた 1 行（エントリが上限を超えて 1 回消える）でも再生回数が ruby あたり 16 回で、100 個のとき（上限に届かない）と同じであること、その操作の後にマップが解放されることを確かめる。`ruby::memo::tests::entries_are_capped_and_large_maps_are_released` は上限と解放を直接固定する。

上限を入れた後も、操作数（上の表）は時間を測った `f3744e1` と全く同じで、`exact_probes_replay_look_ahead_entries` は兄弟 ruby が ruby あたり 12 回再生することを固定する。最初の修正案（look-ahead のない probe を引きもしない）は、兄弟 ruby で再生を ruby あたり 1 回失い（R=100 で `range::width` 23,000→33,100）、release の再確認でも `siblings` 400 が約 20%、`nested` 160 が約 6.5% 遅くなった（その経路が引く `(0, end)` のキーは、直前の look-ahead probe が記録したもの）。採用した形では操作数が変わらないため、時間の表は測り直していない。

## 同値性

出力と挙動の同値性は、reference 経路との比較で固定した。

- `adjustment_only_candidates_match_reference_for_every_range`: フィクスチャごとに全 (start, end)、成長・縮小の両スイープ、pre-spent 3 値 × suppressed 2 値で候補の結果を参照経路と比較する。
- `line_layout_paths_match_reference`: warm/cold の `break_all`、狭い再試行による `PartialLine::index`、`intrinsic_sizes` を参照経路と比較する。フィクスチャは入れ子、兄弟、アラビア語（LTR/RTL）、atomic base、`vertical-align`、overhang、hyphenation、区切り文字、first-line など。
- review focus の 5 項目（`memo_recomputes_when_replay_would_cross_reshape_budget`、`narrow_retry_after_budget_exhausting_index_matches_reference`、焦点 3 の first-line 代替データセット（`walk_fixtures` の `first-line normal`/`first-line alternate`。`incremental_walk_matches_full_walk_for_every_range` が使う）、`intrinsic_min_and_max_atomics_keep_separate_memo_entries` と、min/max の atomics のキーが別になることを直接確かめる `intrinsic_min_and_max_atomics_have_distinct_memo_keys`、`memo_does_not_survive_operations_or_atomic_revisions`）と、`line/replay.rs` の単体テストが、再生条件を個別に固定する。
- 再生条件は「全 charge が受理され合計が最小上限以内」または「全 charge が拒否され現在値が最大上限超」。計画の条件 (a) は、記録時に全 charge が受理されたことまで要求するよう強めた。suppressed 中の拒否は警告が残らず、再生側で区別できなくなるため。

baseline/candidate の出力 SHA-256 と build/layout warning の SHA-256 は、全 1440 サンプル（15 点 × 4 サンプル × 24 ラウンド）で一致した（`digest_mismatch` は空）。

## 制限

- タブ（shodo-b7d）: `line/range.rs` は、段落にタブがあると開始位置が変わるたびに tab prefix を作り直して `RangeCache` の世代を進め、世代が変わらない間だけ記録・再生する memo が無効になる、というのが shodo-b7d で想定している劣化。今回の `nestedtab`（最内にタブ 1 つ）では、この劣化は観測されなかった。candidate は `nested` と同じ傾向で、深さ 160 で 18.89 ms（`nested` は 17.19 ms）、倍化比は 2.54–3.06、baseline からは −99.5% になった。単一の開始位置から 1 回の走査を行うこの形ではタブの prefix が走査の間に作り直されないためかもしれないが、`nestedtab` が tab prefix の作り直しから `RangeCache` の世代の更新までの経路を実際に通るかは確認しておらず、この説明は仮説にとどまる。計測の途中で probe を変えて、タブを各段・末尾に置く、兄弟 ruby に付ける、幅広い内容にするなどの形も試したが、いずれも同じ傾向で、memo の無効化は再現しなかった（これらは記録の対象外で、コミットしていない）。したがって shodo-b7d の劣化を起こす入力は未特定で、この記録は「タブを足しても二次に戻る」ことを示してはいない。shodo-b7d は開いたままで、再現する入力（複数の開始位置を使う形など）が見つかれば、同じ probe に `nestedtab` 以外のケースとして足せる。
- 兄弟 ruby は上のとおり定数倍の改善のみ（shodo-2j6）。
- shodo-tj5: キャッシュ状態への依存（`blocks`/`sets` の hit が副作用を省く）の会計は従来から存在する性質で、本変更は記録時の世代と再生時の世代が等しい場合だけ再生することで、それを壊さない。会計の非対称そのもの（キャッシュが冷えているか温まっているかで `edge_reshape_spent` と Saturation が変わる）は既存の挙動で、見直しは shodo-tj5。
- shodo-mc0: 行計測（`break_all`/`intrinsic_sizes`）の作業量に fail-closed の上限を設けるかは shodo-mc0 で検討する。この変更は上限や拒否条件を追加せず、既存の `RubyCutWork` も変えない（memo のエントリ数の上限は内部のキャッシュの大きさで、出力や警告を変えない）。
- 深さ 160 の `nested` でも candidate の倍化比は 2 を超える（80→160 で 2.58–2.67）。操作数は 2 倍で伸びるため、残りの wall-clock の超過は計測の外にある既存の O(D) 処理（`whitespace::obstructed` の Close unit 走査と `decoration::chain`、`RangeIndex` の木の高さ）による可能性が高い（「判断」のプロファイル。証明はしていない）。

## 再計測

```sh
# 操作数（16 行の JSON）
cargo test -p shodo --lib memo_tests::d77_operation_counts_report -- --ignored --nocapture
# probe（1 サンプル 1 行の JSON）
cargo run --release -p shodo-bench --example d77_scale -- sample <case> <size> <label> <index> <reps>
# baseline: a93a359 の worktree に probe と [[example]] name = "d77_scale" を足し、別の CARGO_TARGET_DIR で build
cargo build --release -p shodo-bench --example d77_scale
S=dev/bench/scripts/shodo-d77
# 12 ラウンド ABBA + 12 ラウンド BAAB（jq が必要）
BASE=<baseline の d77_scale> CAND=<candidate の d77_scale> OUT=<dir>/samples.jsonl bash $S/run.sh
python3 $S/summarize.py <dir>/samples.jsonl > <dir>/summary.json
# <dir> には binaries.txt（両バイナリの sha256sum 出力）と counts.jsonl（操作数の出力）も置く
python3 $S/assemble.py adopted <candidate commit> <dir> <baseline worktree> docs/records/data/shodo-d77-ruby-through-memo.json
```

集計スクリプトは `dev/bench/scripts/shodo-d77/` にある。本記録の数値はこれらと同じ処理（測定時は絶対パスを埋め込んだ版）で生成した。

`<case>` は `nested`, `nestedtab`, `siblings`, `ordinary`, `plain` のいずれか。`<label>` は出力 JSON に記録する baseline/candidate の名前、`<index>` はサンプル番号、`<reps>` は 1 サンプル内の `break_all` 回数。本記録のケースとサイズと reps は次のとおり: nested 20/40/80/160 = 10/2/1/1、nestedtab 20/40/80/160 = 6/1/1/1、siblings 100/200/400 = 1/1/1、ordinary 200/800 = 10/2、plain 200/800 = 200/50。
