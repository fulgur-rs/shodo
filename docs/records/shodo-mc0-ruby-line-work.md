# shodo-mc0: ruby 行計測の作業量に fail-closed の上限を設ける

## 結論

上限を入れる。既定の `Limits` の範囲で 1 行の計測が分単位になる入力が残っていたため（「実測」）、次の 3 つを入れた。

1. **行計測の作業予算（新しい公開フィールド `Limits::max_ruby_line_work`、既定 16）。** `next_line` / `intrinsic_sizes` の 1 回の呼び出し（操作）ごとに、ruby container の live 計測の回数を「factor ×（fit probe が覆う unit 数 + 最も広い container walk）」までに抑える。超えたら、その操作の残りの調整用 probe（`candidate_adjustment`）は ruby の調整 0 を返し、警告を 1 回だけ出す（`WarningKind::Unsupported`、`"ruby line measurement budget exceeded; fitting without ruby adjustments"`）。エラーは追加しない。
2. **accumulator の上限（16,384 container）を超える walk の制限。** through memo だけでは毎 step その walk の全 container を測り直すので、start と atomic revision ごとに 1 つの `through` だけを測り、別の `through` を測ろうとした probe で 1. と同じく打ち切る。
3. **キャッシュの保持。** `RangeCache` の `blocks` / `block_effects` に件数の上限（両方で 16,384、超えたら全消去して大きい容量を解放）を入れた。また、`intrinsic_sizes` が min / max の atomics を切り替えるたびにキャッシュ全体を捨てて段落の索引を作り直していたので、revision が行き来したら直前の revision のキャッシュを 1 つだけ退避して戻せるようにした（作業量の制限ではなく、二次の作業そのものをなくす修正）。最初の切り替えでは従来どおり捨てる。最初から退避すると、切り替えが 1 回だけのふつうの `intrinsic_sizes`（ruby 2,000 個、atomics あり）が約 10% 遅くなった（新しい revision の索引が、古い索引を持ったまま新しい領域に確保される。同じバイナリで退避を実行時に切り替えて 71.1 ms 対 63.9 ms）。

既定の上限は、テスト全体（`cargo test --workspace`）で上限に届かなかった操作の作業量比の最大（3.21、「既定値」）の約 5 倍にした。比べた正常系の fixture と probe では、出力と警告が main と byte 一致した。

## 実測（main、ba8444d）

`dev/bench/examples/mc0_scale.rs`（本件で追加）。release、1 回ずつ。時間は 1 回の `break_all` または `intrinsic_sizes`（build を含まない）。

| 形 | size | main | 備考 |
| --- | ---: | ---: | --- |
| siblings（base "12" の ruby を並べた分割不能な 1 行） | 16,000 | 0.90 s | accumulator の上限以内 |
| siblings | 17,000 | 53.7 s / 64.7 s | 上限を超えて through memo の二次経路に落ちる崖 |
| siblings | 32,000 | 900 s で打ち切り | |
| profile churn（上端揃えの span の中で、兄弟 ruby の後に毎回それまでより大きいグリフ。幅 1e7 の 1 行） | 500 / 1,000 / 2,000 / 4,000 | 0.52 s / 2.46 s / 12.5 s / 59.3 s | 倍化比約 5。ruby なしの対照（同じ形で base を素のテキストに）は 2,000 で 3 ms |
| 強制改行で区切った ruby の段（"日日" + ruby、`<br>` を `size` 個）、`intrinsic_sizes`、呼び出し元の atomics あり | 200 / 400 / 1,000 / 2,000 | 0.58 s / 2.49 s / 30.3 s / 75.2 s | min と max の atomics の revision が交互に変わるたびに `RangeCache` を捨てて作り直す |
| 1 つの ruby に多数の base / annotation の組 | 2,000 | build で `LimitExceeded { Items }` | build 時の上限で拒否される。行計測には届かない |

どれも既定の `Limits` で build でき、1 つの段落で 1 行（または 1 回の `intrinsic_sizes`）が分単位になる。

## 残る敵対形状と作業量の上界

shodo-2j6、shodo-b7d、shodo-tj5 の「残る最悪形」と本件で見つけた形の整理。「本件後」は既定値での上界。

| 形 | 由来 | main での作業量 | 本件後 |
| --- | --- | --- | --- |
| profile churn（D4 で profile 依存の位置が毎 step 汚れる） | 2j6 (1) | 1 行の container 計測が r(r−1)/2 程度 | 操作ごとに予算で有界 |
| RTL isolate による隣の入れ替わり（D3 が大量に汚す） | 2j6 (2) | 二次になりうる（未計測） | 予算で有界 |
| atomics を伴う `intrinsic_sizes`（revision の切り替えでキャッシュを捨てる） | 2j6 (3) | 強制改行ごとに段落全体の索引を作り直す。O(改行数 · 段落の長さ) | revision が行き来したらキャッシュを 2 revision 分保持し、各 dataset を revision ごとに高々 2 回作る（線形） |
| tab prefix の新しい start ごとの作り直し | 2j6 (4)、b7d | 飽和による無効化は tj5 で解消。新しい start ごとに O(範囲内の tab 数 · log n) | 1 つの操作の start は 1 つ（行）か、word ごとの start（範囲が重ならない）と段の start（強制改行ごと）なので、操作内では線形。課金はしない（「残るもの」） |
| `max_warnings: None` で毎回警告する計測（保存されないので毎 probe 測り直す） | 2j6 (5)、tj5 (2) | probe ごとに計測。警告も増え続ける | 計測は予算で有界。警告の数も計測の数で抑えられる |
| 予算の境界をまたいだ計測（受理と拒否が混じる）の再生不能 | 2j6 (6)、tj5 (3) | 各計測が reshape 予算を消費するので回数は有界 | 予算で有界 |
| saturate する和の逐次加算 | 2j6 (7) | 再生される run を 1 つずつ足す（計測なし） | 足した position 数を課金 |
| 16,384 container を超える walk | 2j6 (8) | through memo で毎 step 全 container を計測（二次） | start / revision ごとに 1 つの `through` だけ計測 |
| clear する float ごとの walk の作り直し（`intrinsic_sizes`） | 本件のレビュー | float ごとに行全体の container を歩き直す。O(float 数 · container 数) | walk の歩数を課金し、予算で有界。作り直しそのものは残る（「残るもの」） |
| `blocks` / `block_effects` の map | tj5 (1) | 段落を変えるまで上限なし | 16,384 件で全消去（約 1 MiB）。退避した revision の分を含めて最大 2 倍 |
| `blocks` のヒットも reshape 予算を消費 | tj5 (5) | 操作ごとの reshape 予算（262,144 bytes）で fail-closed | 変更なし |

予算の上界: 1 つの操作の fit probe の計測は「factor ×（覆う unit 数 + 最大 walk）+ 1 walk」以下（最後に受理した probe が測る walk の分だけ超えうる）。受理された行の `apply` はその行の container をもう 1 回だけ測る（線形）。覆う unit 数は行の操作ならその行の走査範囲、`intrinsic_sizes` なら段落全体なので、どの操作も走査する入力に線形。1 回の container 計測のコストは container の大きさに比例しうるが、container の大きさは build 時の `Items` / `RubyCutWork` で有界（2,000 組の ruby は build で拒否される）。

## 判断

### どこで課金するか

- 課金: `ruby::measure::measure_one` の 1 回（live の container 計測）、`memo::advance` の walk が再適用または新しく訪れた container 1 つ、accumulator の saturate する逐次加算で足した position 1 つを、それぞれ 1 単位とする。計測は shodo-2j6 の test 専用カウンタ `ruby_container_measures` と同じ点。walk の課金はレビューで足した。`intrinsic_sizes` で clear する float があると、float ごとに `total_unit` からの probe が word の probe と交互になり、1 つしかない walk の状態が毎回作り直されて行全体を歩き直す。この walk は計測を伴わない（accumulator が clean な run を再生する）ので、計測だけを数えると課金されなかった（レビューの実験で ruby 16,000 個 + clear する float 64,000 個の 1 回の `intrinsic_sizes` が 10.4 s、拒否されない）。
- 判定: `candidate_adjustment` の reuse 経路の入口。累計が許容量以上なら、その probe と操作の残りの probe を拒否する（sticky）。1 つの probe の途中では止めないので、memo と accumulator の状態が中途半端になることはない。
- 許容量: `factor ×（span + walk）`。span は操作内で受理した probe の「最大の end − 最小の start」、walk は 1 つの probe が訪れた container 数の最大値。どちらも最大値なので、同じ probe を繰り返しても増えない。
  - 最初の案は「最長の probe 範囲（end − start）」だったが、`intrinsic_sizes` は word ごとに start が変わるので範囲が短いまま計測が段落全体で積み上がり、ordinary 16,000 の `intrinsic_sizes` が上限に届いた（main では 0.5 s で正常）。覆う範囲（最大の end − 最小の start）に変えた。
  - 定数の予算（操作ごとに一定の回数）は採らなかった。正常な作業量は段落の大きさに線形（ordinary 20,000 の `intrinsic_sizes` は 1 回の操作で段落全体を覆う）、敵対形状は二次なので、定数では大きい正常入力を誤検出するか、敵対形状に分単位を許すかのどちらかになる。
  - 参照経路の作業量（probe ごとの範囲内 container 数の和）でも課金しなかった。キャッシュに依存しないが、兄弟 ruby 1,600 個のような正常な形でも二次になる。
- 16,384 container を超える walk: memo miss で walk を測る前に `line_work::admit_walk` が判定する。同じ start と atomic revision で最初に測った `through` と同じなら測る（`intrinsic_sizes` は行の終わりを min と max の atomics で 2 回聞き、atomics が空なら revision が同じ）。違う `through` なら拒否する。予算だけでは、siblings 17,000 で 1 probe が約 0.2 s かかる walk を factor × 6 回ほど許してしまい、既定値でも 20 s 近くかかった（factor 32 で 40 s、8 で 10 s）。

### 超過時の挙動と互換性

- エラーは追加しない。`break_all` と `intrinsic_sizes` は infallible のまま。警告は reshape 予算（`"line edge reshape budget exceeded; keeping shared glyphs"`）と同じ `Unsupported` で、操作ごとに 1 回。
- 劣化の内容: 調整用 probe が 0 を返すので、行の fit 判定が ruby の annotation のはみ出しを無視する（行がはみ出しうる）。`intrinsic_sizes` は ruby のはみ出しを含まない値になる。受理された行は `apply`（`candidate`）で ruby を全部測るので、配置される ruby の geometry は正確。比べた敵対形状（siblings 17,000、churn 1,000）では、分割不能な行や十分広い行なので出力は main と一致し、警告だけが増えた。
- `Some(0)` は fit で ruby の調整を使わない（毎操作警告する）。`None` は上限を外す（main と同じ）。
- 公開 API: `Limits` に `pub max_ruby_line_work: Option<u64>` を追加した。`Limits` は `#[non_exhaustive]` ではないので、`..Default::default()` を使わない struct literal での構築はコンパイルできなくなる（0.0.x）。リポジトリ内と fulgur / raikiri にそのような構築はない。`Limits::unlimited()` は `None`。
- 新しい警告: 既定値では比べた fixture と probe で出ない（「結果」）。

### 決定性

劣化するかどうかは (段落, atomic revision, 操作の入力, 警告 sink の抑制状態) の関数で、前の操作が残したキャッシュには依存しない。

- memo と accumulator は操作ごとに空から始まる。`RangeCache` のヒットはミスと同じ副作用（shodo-tj5）で、計測の回数を変えない。退避した revision のキャッシュも同じ。
- 警告を出した scan は `PartialLine` として保持されない（既存の `safe` 判定）。
- 保持された行が probe 列を変えるのは `PartialLine::index` だけ。index の probe は狭い幅の cold の scan の probe を含み（hyphen の位置では同じ probe を 2 回聞き、`through == end` の probe は memo に保存されないので測り直す）、繰り返しは作業を足すが許容量（最大値）は増やさない。したがって index が拒否されなければ cold の scan も拒否されない。index が失敗したら（警告が出た場合を含む）、作業量の状態を index の前に戻し、ruby memo を消してから scan し直す。
- index の後の float の位置の probe（1 回の呼び出しに 1 回）は、保持の有無で作業量の状態が違うので、許容量の外で測る（計測は課金するが判定に使わない）。`apply` と同じく線形。
- 残る共有の入力: 失敗した index の reshape 予算の課金（`edge_reshape_spent`）は戻さない（main と同じ）。scan し直したときの再生の gate はそれを読むので、live の計測回数がそこで変わりうる。index の失敗を強制するテスト（下記）では食い違いは見つからず、戻す処理を消しても通る。戻す処理は防御として置いている。
- 抑制状態: 警告を出した計測は抑制されていない sink では保存されず、抑制された sink では保存されるので、live の計測回数が sink の状態で変わりうる。shodo-tj5 で課金の入力として認めた状態と同じ。
- テスト: `degraded_layout_does_not_depend_on_earlier_calls`（cold、温まったコンテキスト、広い幅で保持した `PartialLine` の後で、行・警告・intrinsic が一致）、`failed_index_rescans_like_a_cold_call`、`float_probe_after_index_matches_a_cold_call`。

## 既定値

一時的な計測で、`cargo test --workspace` のすべての操作の「spent /（span + walk）」を集めた（全範囲を 1 操作で掃くテスト用の sweep を除く。下記）。walk の課金を足した後で、上限に届かなかった操作の最大は 3.21（足す前は 2.34。ruby を数個含む小さな fixture と churn の小さいサイズ）で、ふつうは 1 前後（兄弟 ruby の accumulator は container あたり約 3 回の計測と 1 歩、span は ruby あたり約 5 unit）。既定値 16 はその約 5 倍。

probe での時間（candidate、1 回ずつ）。

| 形 | size | factor 32 | 16（既定） | 8 | 4 |
| --- | ---: | ---: | ---: | ---: | ---: |
| churn（1 行） | 1,000 | 1.75 s（届かない） | 0.54 s | 0.16 s | 0.06 s |
| churn（1 行） | 4,000 | 3.51 s | 0.67 s | 0.28 s | 0.16 s |
| siblings（wide walk の規則なし） | 17,000 | 40.3 s | 23.9 s | 10.4 s | 5.5 s |
| siblings（wide walk の規則あり） | 17,000 | — | 0.85 s | — | — |

`ruby::memo_tests::observe_candidates_in` と `accumulate_tests` の sweep（`sweep_in` など）は、全 (start, end) を 1 つの操作で問い合わせる。レイアウトの呼び出しはこの数の probe を出さず、作業量は二次になるので、これらのテストでは予算を外した（test 専用の `ruby_line_work_disabled`）。目的は再利用の厳密さの確認で、予算は `line_work_tests` が別に確かめる。

## 結果

### 出力と警告（baseline と candidate の digest 比較、1 回ずつ）

レビューの修正（`2f25e1a`）の後に、`mc0_scale` の intrinsic（breaks 400、breaksatomic 200、ordinary 2,000 / 20,000、siblings 2,000 / 17,000、churn 500、pairs 500）と break（siblings 4,000 / 16,000、ordinary 2,000、plain 2,000、breaks 2,000）、`sibling_scale` / `d77_scale` / `tab_scale` の全ケースを比べ直し、すべて一致した。以下は修正前の比較。

一致: `sibling_scale` の siblings 200 / 800、outer 100 / 400、valign 200、rtl 200、ordinary 200 / 800、plain 200 / 800。`d77_scale` の nested 20 / 80、nestedtab 40、siblings 100、ordinary 200 / 800、plain 800。`tab_scale` の nested 40、nestedtab 40、siblings 200、siblingstab 200、plain 200、nestedhugetab 20、siblingshugetab 100。`mc0_scale` の siblings 4,000 / 16,000（break）、siblings 2,000 / 17,000（intrinsic）、ordinary 2,000（break、intrinsic）、ordinary 20,000（intrinsic）、plain 2,000（break、intrinsic）、breaks 200 / 400 / 2,000（intrinsic と break）、breaksatomic 200（intrinsic、break）、pairs 500 / 1,000、churn 64 / 128 / 256 / 512（幅 1e7）、churn 500 / 2,000（幅 96、intrinsic）、churnplain 2,000。

警告だけが変わった: siblings 17,000（break）と churn 1,000（幅 1e7）。出力は一致し、予算超過の警告が 1 件増えた。

### 時間

baseline は `ba8444d`（main）に `mc0_scale.rs`（`MC0_FACTOR` のブロックを除く）を置いたもの、candidate は `35745b8`。target directory を分けた別々の release バイナリ（`mc0_scale` の SHA-256 は baseline が `40fb974c87f6bc08…`、candidate が `b291fab2f1c98cfa…`）。10 ラウンド、偶数ラウンドは ABBA、奇数ラウンドは BAAB の順で実行し、各ラウンドで同じ側の 2 サンプルを平均して、ラウンドの中央値を比べた。時間は probe が測る 1 回の操作（`sibling_scale` 等は `break_all` 1 回あたり）、task-clock は `perf stat -r 1 -e task-clock` のプロセス全体（build を含む）。`intrinsic` は呼び出し元の atomics あり。rustc 1.96.0、AMD Ryzen 5 5600G、Linux 7.2.5-3-omarchy。測定中は別のセッションのビルドが動いており（load average 2–5）、±2% 程度は揺らぎと考える。

| probe | ケース | baseline | candidate | Δ | 速いラウンド | task-clock Δ |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| sibling_scale | siblings 800 | 27.59 ms | 27.96 ms | +1.4% | 2/10 | +0.4% |
| sibling_scale | ordinary 800 | 34.52 ms | 34.28 ms | −0.7% | 7/10 | +0.7% |
| sibling_scale | plain 800 | 1.52 ms | 1.51 ms | −0.8% | 8/10 | −0.3% |
| sibling_scale | outer 400 | 17.77 ms | 17.88 ms | +0.6% | 4/10 | −0.3% |
| d77_scale | nested 80 | 6.21 ms | 5.98 ms | −3.6% | 8/10 | −3.5% |
| tab_scale | siblingstab 800 | 32.38 ms | 32.70 ms | +1.0% | 4/10 | −0.9% |
| mc0_scale | siblings 16000 break | 685.64 ms | 674.43 ms | −1.6% | 6/10 | +0.1% |
| mc0_scale | ordinary 16000 intrinsic | 534.77 ms | 528.51 ms | −1.2% | 10/10 | +1.3% |
| mc0_scale | ordinary 2000 intrinsic | 64.71 ms | 63.83 ms | −1.4% | 6/10 | +0.6% |
| mc0_scale | siblings 2000 intrinsic | 63.54 ms | 62.03 ms | −2.4% | 6/10 | +0.5% |
| mc0_scale | plain 2000 intrinsic | 1.18 ms | 1.10 ms | −6.7% | 9/10 | +0.7% |
| mc0_scale | ordinary 2000 break | 98.22 ms | 95.71 ms | −2.6% | 8/10 | −1.4% |
| mc0_scale | churn 512 break（幅 1e7、上限に届かない） | 435.88 ms | 425.32 ms | −2.4% | 9/10 | −1.7% |
| mc0_scale | breaks 200 intrinsic | 567.75 ms | 8.76 ms | −98.5% | 10/10 | −95.8% |
| mc0_scale | churn 1000 break（幅 1e7） | 1.85 s | 549.12 ms | −70.3% | 10/10 | −63.3% |

`sibling_scale siblings 800` の +1.4%（2/10）は task-clock で +0.4% で、本件のコードは ruby の調整用 probe ごとに比較と最大値の更新を数回足すだけなので、揺らぎと考える（退避の修正の前の candidate で同じ方法で測った 10 ラウンドでは −3.7%、9/10 で速かった。そのときの `intrinsic` 3 ケースの +7–10% が退避のコストで、「結論」の修正で消えた）。

大きい敵対形状は 1 回ずつ（同じ方法の別の実行、別のセッションのビルドで load が高かった）: churn 4,000（幅 1e7）59.3 s → 0.87 s、siblings 17,000 64.7 s → 0.95 s、siblings 32,000 は baseline が 900 s で打ち切り、candidate 1.41 s。breaks 1,000（intrinsic、atomics あり）は baseline 30.3 s、candidate 49 ms。レビューの修正の後の candidate（1 回ずつ）: churn 1,000 0.49 s、churn 4,000 0.62 s、siblings 17,000 0.85 s、siblings 32,000 1.27 s、いずれも警告 1 件。

レビューの修正の後に同じ方法で 10 ラウンド測り直した（siblings 800 +3.7%（2/10、task-clock −0.4%）、ordinary 800 +0.6%、d77 nested 80 −3.1%、mc0 ordinary 2,000 intrinsic −1.3%、siblings 2,000 intrinsic +1.4%、ordinary 2,000 break −1.6%、siblings 16,000 break +0.2%）。siblings 800 は `mc0_scale` で baseline、candidate、candidate の `MC0_FACTOR=none` を 20 回ずつ交互に測り、中央値が 30.22 / 30.14 / 30.13 ms で差がなかった。

## テスト

- `line::range::tests::block_cap_crossing_matches_a_fresh_context`: 上限 2 で全範囲を 2 周し、各問い合わせの（高さ、Saturation、spent、警告）が新しいコンテキストと一致し、件数が上限を超えない。
- `ruby::line_work_tests`:
  - `churn_measures_are_bounded_by_the_allowance`: churn の container 計測が上限なしでは倍化比 3 以上、factor 1 では 2.2 以下。警告は操作あたり 1 件。
  - `spent_work_stays_within_the_allowance_and_one_walk`: 上限に届いた操作の spent ≤ factor ×（span + walk）+ walk + container 数（`apply`）。
  - `zero_factor_fits_without_ruby_and_places_ruby_exactly`: `Some(0)` で警告 1 件、十分広い行の geometry は `None` と一致。
  - `degraded_layout_does_not_depend_on_earlier_calls`（決定性、上記）。
  - `default_allowance_is_not_reached_by_the_fixtures`: memo の等価性 fixture すべての `observe_layout_in`（warm / cold の `break_all`、`PartialLine::index` を通る再試行、float、intrinsic）と siblings 256、churn 32 で警告が出ない。
  - `one_wide_walk_is_measured_and_the_next_probe_refused`: accumulator の上限を 4 にして siblings 16 の行が 1 回警告し、`intrinsic_sizes`（行の終わりを 2 回聞く）は警告せず上限なしと一致。
  - `alternating_intrinsic_revisions_keep_both_range_caches`: 強制改行で区切った ruby の段と呼び出し元の atomics で、`build` の unit 数が倍化ごとに 2.2 倍以下（修正前は 3.96 倍）。
  - `restarted_walks_are_charged`: ruby 64 個 + clear する float 512 個の `intrinsic_sizes` で、factor 1 なら上限に届き、walk の歩数が許容量以内。上限なしの歩数は許容量の 2 倍を超える。課金を外すと落ちる。
  - `failed_index_rescans_like_a_cold_call`: 保持した広い行の後の狭い呼び出しで index の失敗を強制し（test 専用の `fail_next_index`）、cold と一致。失敗の経路を通ったことも確かめる。
  - `float_probe_after_index_matches_a_cold_call`: float を含む兄弟 ruby で、保持した行の後と cold の float の位置が一致。
- 既存の reference 比較（`line_layout_paths_match_reference`、`warm_context_paths_match_reference`、`sibling_fixtures_match_reference_with_the_step_oracle_*` の `observe_layout_in` など）は既定値のまま通る。参照経路は上限で劣化しないので、既定値でどれかが上限に届けばここで食い違う。

## 残るもの

- 操作の回数 × 予算: 予算は操作ごと。行ごとに上限まで使う段落の総作業量は、入力に線形だが定数は factor に比例する。reshape 予算と同じ形。
- 1 回の container 計測のコストは container の大きさに比例しうる（build 時の上限で有界）。課金は回数で、大きさで重み付けしない（重み付けすると 1,000 組の ruby の `intrinsic_sizes` のような正常入力が上限に届く）。
- tab prefix の新しい start ごとの O(tab 数 · log n) は課金していない（操作内では線形、上の表）。
- clear する float ごとの walk の作り直しは、課金で有界にしただけで、作り直しそのものは残る（word と段の 2 つの start に walk の状態を 1 つずつ持てば消える。shodo-5wa）。
- 劣化した行は ruby のはみ出しを無視して fit するので、行がはみ出しうる。正確な代替値（たとえば container ごとの上界）は入れていない。
- 警告 sink の抑制状態への依存（上記）。

## 再計測

```sh
cargo build --release -p shodo-bench --example mc0_scale
# sample <case> <size> <break|intrinsic> <label> [inline-size]
target/release/examples/mc0_scale sample churn 1000 break candidate 1e7
MC0_FACTOR=none target/release/examples/mc0_scale sample siblings 17000 break candidate
# baseline: ba8444d の worktree に mc0_scale.rs（MC0_FACTOR のブロックを除く）と [[example]] を置き、別の CARGO_TARGET_DIR で build
cargo test -p shodo --lib line_work_tests
```

`<case>` は `siblings`, `churn`, `churnplain`, `ordinary`, `plain`, `pairs`, `breaks`, `breaksatomic`。`intrinsic` は id 9,000,000 の atomic に min 10 / max 20 を与える。生データはローカルに置き、commit していない（#227 の方針）。
