# shodo-t3t: Ruby base 内の TCY 幅 probe 結果の再利用

## 判断

採用する。Ruby を含む段落では、すべての ruby base が limits を持つため（`crates/shodo/src/ruby/pairing.rs:80`）、TCY の幅 probe は常に BaseScope 経路を通る。この経路で、選択された幅 probe の試行出力を TCY group の最終 shaping に再利用する。再利用は、group の各入力が scoped と global の run 予算の下で 1 window に収まり、かつ状態を変更しない BaseScope preflight が通る場合に限る。条件を満たさない group は従来どおり scoped 経路で再 shaping するため、limit エラー、失敗の分類、警告、出力は従来経路と等しい。

preflight は、触れた各 scope について「`spent` + その scope に届く charge の飽和和」が上限以内かを確かめる。charge は非負で飽和加算は単調なので、これは charge を順に再生したときの全 prefix 検査と同値になる。charge の所有者が 1 つなら和を取ってから scope chain を 1 回たどるだけで O(charges + depth)、所有者が混在する場合も O(charges × depth) で、group ごとの割り当ても全 scope の走査もない。

固定フォントの release build で、4 つの base ケースが約 6–8% 速くなった。ABBA と BAAB のどちらでも 12/12 ラウンドで candidate が速かった。深い入れ子（250 段の ruby base）の `deep-nested-multi` も baseline より約 2% 速い。shaper 呼び出しは base ケースで 25%、深い入れ子で 29% 減り、割り当てバイト数も 3.8–4.7% 減った。ピークは base ケースで最大 +16 B。

## 方法

- baseline は `3334beaf53fa683ab4ee9803b5b413a1b187ad00`（main）。candidate は `4433a11058f67b08b59e394af858878556534738`（ブランチ perf/shodo-t3t-tcy-probe-reuse、再利用実装、scope ごとの合計による preflight、probe を含む）。
- baseline/candidate は target directory を分けた別々の release バイナリとして構築した。計時用の SHA-256 は baseline が `6a40674f2c6e5b65ec8c5420220591143b52eda10ec52b41f938a4181f69ebe6`、candidate が `e60a08aec0f7fa0a0df106f414a686c08f8e1c433b402404c8d653d75fbb7070`。baseline 側には probe の example と `Cargo.toml` の `[[example]]` だけを追加した。
- 計時用バイナリは feature なしで build した。割り当て計測には `--features allocation-counting` を付けた別バイナリを使い、計時サンプルには計数用 allocator が入らないようにした。
- `dev/bench/examples/tcy_base_reuse.rs` は `WIDTH_PROBE_CASES` の 4 形状を公開 API で再現する。TCY 用ラッパーと annotation は CJK（`FONTS[1]`）、区切りの `"x"` は Latin（`FONTS[0]`、19px、TCY なし）。各ケースは TCY group を 64 個、1 つの RubyBase に入れる。
  - `base-two-rl`: `"12"`、vertical-rl LTR
  - `base-three-lr-rtl`: `"123"`、vertical-lr RTL
  - `base-four-rl-multi`: `"1234"`、vertical-rl LTR、複数スタイル（末尾に `"x"`）
  - `base-two-lr-rtl-multi`: `"12"`、vertical-lr RTL、複数スタイル（末尾に `"x"`）
  - `base-fallback`: `base-two-rl` で base の `max_shaping_run_bytes = Some(1)`。全 group が fallback する
  - `plain-tcy`: Ruby なしで `"12"` の TCY 64 group を `"x"` で区切る。scope を使わない対照
  - `deep-nested-multi`: ruby を 250 段入れ子にし、最内の base に TCY `"1日2日"` を 400 group 入れる（各 group の前に Latin の `"x"`）。TCY のフォント指定は Latin、CJK の順で、Latin に `"日"` がないため各 group は 4 つの shape 入力と 4 つの charge を持ち、すべて最深の scope に属する。レビューで修正前の preflight が O(K·D²) になると指摘された形状
- annotation は `"日"` を使う。固定 CJK フォントに `"注"` がなく、`geometry_snapshot` が欠落 glyph を拒否するため。annotation の shaping は baseline/candidate で同一。
- builder と `LayoutContext` を計時前に用意し、1 サンプル内で `builder.build()` を固定回数実行して平均を記録した。フォント読込、warm-up build、`break_all`、出力 digest、warning 取得は計時範囲外。
- 12 ラウンドを ABBA 順（baseline/candidate/candidate/baseline）で測定した後、BAAB 順（candidate/baseline/baseline/candidate）で独立に 12 ラウンド測定した。各ラウンドでは同じラベルの 2 サンプルを平均した。1 サンプルは base ケースで 50–75 ms 程度、`deep-nested-multi` は 2 build で 85 ms 程度。
- `deep-nested-multi` の `break_all` は 250 段の入れ子で 1 段落あたり 15–25 秒かかる（計時範囲外で、両バイナリとも同じ）。そのため計時サンプルでは digest を省略し（`TCY_PROBE_SKIP_DIGEST`）、出力の一致は smoke 実行と割り当て計測の全 build で確認した。
- rustc 1.96.0、AMD Ryzen 5 5600G、Linux x86_64。scaling governor は `performance`、boost は有効。測定中に他の重い処理は走らせていない。全測定値と環境情報は [raw JSON](data/shodo-t3t-tcy-probe-reuse-ab.json) に記録した。

## 時間

`Δ` は candidate と baseline の中央値の差。負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。

| ケース | builds / sample | baseline 中央値 (ABBA) | ABBA Δ | ABBA 速い比較 | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| base-two-rl | 100 | 655.9 µs | −7.7% | 12/12 | −7.7% | 12/12 |
| base-three-lr-rtl | 100 | 714.1 µs | −6.3% | 12/12 | −6.0% | 12/12 |
| base-four-rl-multi | 90 | 762.0 µs | −6.7% | 12/12 | −6.5% | 12/12 |
| base-two-lr-rtl-multi | 100 | 675.2 µs | −7.9% | 12/12 | −6.7% | 12/12 |
| base-fallback | 100 | 699.6 µs | +3.2% | 0/12 | +4.1% | 0/12 |
| plain-tcy | 120 | 519.9 µs | +0.9% | 1/12 | +0.6% | 2/12 |
| deep-nested-multi | 2 | 42.48 ms | −2.0% | 11/12 | −1.9% | 12/12 |

ラウンドごとの比の中央値（candidate/baseline）は base 4 ケースで ABBA 0.924–0.939、BAAB 0.920–0.939。base 4 ケースのラウンドごとの最大比は 0.9714 で、1 ラウンドも 1 を超えなかった。

`base-fallback` は ABBA/BAAB ともに 12 ラウンド全部で candidate が遅く、ラウンド比の中央値は 1.036 / 1.044（中央値の差で +3.2% / +4.1%）。これは preflight と shaping trace、および捨てられる retained trial のコストで、再利用が成立しない group が毎回この追加分を払うことによる。発生するのは、base の run byte 予算が TCY group の byte 数を下回る場合や、glyph 上限がほぼ使い切られている場合だけで、既定の ruby limits では起きない。係数は定数倍で、段落サイズに対して増え続けるものではない。

`plain-tcy`（Ruby なし、scope なし）は +0.6% / +0.9% で、速い比較は 1/12 と 2/12 だった。追加で plain-tcy だけを 24 ラウンド測ると ABBA +0.7%（4/24）、BAAB +1.1%（2/24）。修正前の candidate（`ad9d4d6`、同じ probe）と baseline の 24 ラウンド ABBA でも +0.3%（8/24）で、今回の preflight 変更によるものではない。この経路の変更は、BaseScope と shaping trace がないことを確かめる分岐（harfrust 呼び出しごとの trace 判定を含む）だけで、shaper 呼び出し、割り当て回数・バイト数・ピークはすべて baseline と同一（下表）。約 1% 以内の差として残る。

## 深い入れ子

`deep-nested-multi` は、レビューで指摘された「深い scope chain × 複数入力の TCY group」の形状を probe に入れたもの。修正前の preflight は charge ごとに chain をたどり、各段で保留中の合計を線形探索していたため、group あたり O(K·D²) だった。修正前の candidate（`ad9d4d6` に同じ probe を入れたもの）と baseline の 6 ラウンド ABBA では、中央値 42.80 ms に対して 73.52 ms（+71.8%、ラウンド比 1.679–1.766、0/6）だった。

scope ごとの合計に置き換えた後は、上表のとおり baseline より ABBA −2.0%、BAAB −1.9% で、24 比較中 23 で candidate が速い（唯一の例外のラウンド比は 1.0007）。reference も charge ごとに chain をたどる（O(K·D)）ため、再利用経路の preflight（同一所有者で O(K + D)）が reference を上回ることはない。

この上限は wall clock に依存しないテストでも固定した。`scoped_output_reuse_preflight_stays_linear_in_deep_nested_bases` は 32 段の入れ子に複数入力の TCY group を 6 個入れ、test 専用の計数で各 preflight の charge 数、たどった chain の深さ、手数（charge の反復 + scope の訪問）を記録する。観測値は全 preflight で charges 4、depth 32、steps 36 で、steps ≤ (charges + 1) × (depth + 1) と、所有者が 1 つの場合の steps ≤ charges + depth を検証する。深さ 32 の preflight が 6 回以上起きることも確認するので、probe が深い位置で走らないまま通ることはない。同じテストは CloneReference との出力比較と、外側 `None, 0, 40, 80` × 最内 `None, 3, 28, 29, 30` の glyph 上限の掃引で結果の同値性も確認する（最内の 28/29 は最後の group の途中で失敗する）。

## 呼び出し回数

harfrust の shape 呼び出し回数は release probe からは読めない（計数は test 専用）。そのため ignored テスト `tcy_base_reuse_probe_shaper_calls` で各 probe ケースを同じフォントファイルで再現し、CloneReference（baseline の方式）と ScopedReuse（本番経路）の回数を数えた。フォントは `"Width CJK"`/`"Width Latin"` の名前で登録し、script fallback は設定していない。全ケースで両モードの出力 snapshot と glyph 数が一致した。

| ケース | reference | ScopedReuse | 差 |
| --- | ---: | ---: | ---: |
| base-two-rl | 256 | 192 | −64 (−25.0%) |
| base-three-lr-rtl | 256 | 192 | −64 (−25.0%) |
| base-four-rl-multi | 257 | 193 | −64 (−24.9%) |
| base-two-lr-rtl-multi | 257 | 193 | −64 (−24.9%) |
| base-fallback | 320 | 320 | 0 |
| plain-tcy | 191 | 191 | 0 |
| deep-nested-multi | 5,450 | 3,850 | −1,600 (−29.4%) |

base ケースは 64 group それぞれの最終 shaping 1 回を省く。`deep-nested-multi` は 400 group × 4 入力の 1,600 回を省く。`base-fallback` は全 group が scoped 経路で再 shaping されるため減らず、`plain-tcy` は scope がなく、両モードとも同じ unscoped 再利用経路を通る。

## 割り当てとピーク

`alloc` サブコマンドで、warm-up build の後に 1 build ずつ計数した（builder と context は計数範囲外で用意）。各ケース 3 build はすべて同じ値だった。calls は alloc と realloc の回数、bytes は要求バイト数、peak は計数開始時点からの最大追加 live バイト数。

| ケース | calls baseline | calls candidate | bytes baseline | bytes candidate | peak baseline | peak candidate |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| base-two-rl | 4,194 | 4,117 (−1.8%) | 733,830 | 699,590 (−4.7%) | 275,171 | 275,187 (+16 B) |
| base-three-lr-rtl | 4,714 | 4,764 (+1.1%) | 778,522 | 748,618 (−3.8%) | 291,091 | 291,091 (±0) |
| base-four-rl-multi | 4,341 | 4,335 (−0.1%) | 822,458 | 789,010 (−4.1%) | 316,928 | 316,928 (±0) |
| base-two-lr-rtl-multi | 4,721 | 4,652 (−1.5%) | 750,830 | 716,886 (−4.5%) | 276,624 | 276,640 (+16 B) |
| base-fallback | 4,259 | 4,822 (+13.2%) | 746,118 | 743,622 (−0.3%) | 281,315 | 281,331 (+16 B) |
| plain-tcy | 3,843 | 3,843 | 517,069 | 517,069 | 198,496 | 198,496 |
| deep-nested-multi | 89,635 | 83,629 (−6.7%) | 14,976,677 | 14,403,709 (−3.8%) | 5,129,358 | 5,129,358 (±0) |

再利用が成立するケースでは、最終 shaping の GlyphStore を作らない分だけ要求バイト数が減る。`base-fallback` は trace の伸長と捨てられる trial のため割り当て回数が 13.2% 増えるが、バイト数はほぼ同じ。ピークの増分は最大 16 B（返る段落が保持するバイト数も同じ 16 B 増）で、preflight の scratch はこれらのケースでは確保されない（所有者が 1 つの group は scratch を使わない）。

## 同値性

出力と挙動の同値性は、reference 経路（再利用なし）との比較で固定した。各テストが掃引する値は次のとおり。

- `scoped_output_reuse_matches_reference_across_base_and_paragraph_limits`: 4 ケース × `repeats` 1/2 × base glyph 上限 `None, 0, 1, 2, 3, 4, 6` × base run byte 上限 `None, 0, 1, 2, 4` × 段落 glyph 上限 `None, 1, 3, 5`。
- `scoped_output_reuse_matches_reference_for_first_line_cumulative_base_glyphs`: `::first-line` ありで、4 ケース × `repeats` 1/2 × base glyph 上限 `None, 1, 2, 3, 4, 5, 6, 8` × 段落 glyph 上限 `None, 2, 4, 6, 8`。成功と失敗の両方の結果が現れることも確認する。
- `scoped_output_reuse_matches_reference_under_exhausted_warning_caps`: 4 ケース × 警告上限 `Some(0), Some(1), Some(2)` × base run byte 上限 `None, 0, 1`。
- `scoped_output_reuse_matches_reference_for_nested_base_scopes`: 外側 base glyph 上限 `None, 0, 1, 2, 3, 4, 6` × 内側 `None, 0, 1, 2, 4`。成功と失敗の両方が現れることを確認する。
- `scoped_output_reuse_matches_reference_for_multi_input_groups_in_nested_bases`: 外側・内側の glyph 上限を `None, 0, 1..6` で掃引し、複数 shape input の group について、各 charge は単独なら通るが累積では超える境界を再利用経路が到達・再現することを確認する。
- `scoped_output_reuse_preflight_stays_linear_in_deep_nested_bases`: 32 段の入れ子で上記「深い入れ子」の掃引。

結果の同値性だけでは、累積 preflight と charge ごとの preflight を区別できない。commit が reference と同じ順序で charge を再生し、同じように失敗するため。preflight の意味は `crates/shodo/src/ruby/base_budget.rs` の単体テストが固定する。共有祖先への累積、内側の上限と既存の `spent`、状態を変えないこと、飽和（上限なし）の 3 つに加えて、次を追加した。

- `shaped_glyph_preflight_sums_sibling_owners_into_shared_ancestor`: 兄弟の内側 scope それぞれは自分の上限内だが、共有祖先の合計が上限を超える場合に拒否する。
- `shaped_glyph_preflight_saturates_against_finite_limits`: `spent` が `u64::MAX` 近くの有限上限に対し、飽和で上限を超える組と、wrapping なら上限内に戻ってしまう組（`u64::MAX` + 2）を拒否する。
- `shaped_glyph_preflight_resets_scratch_after_failure`: 失敗した preflight の後も scratch が空に戻り、次の preflight が正しく通ること、`spent` と `failure` が変わらないこと。
- `shaped_glyph_preflight_matches_sequential_replay_exhaustively`: 兄弟 2 scope と共有祖先の上限 6 通りずつ、長さ 1–3 の charge 列すべてについて、preflight の結果が `charge` の逐次再生と一致すること。

baseline/candidate の出力 SHA-256 と build/layout warning は、両 run の digest を取った全サンプル、smoke 実行、割り当て計測の全 build で一致した。警告は `deep-nested-multi` の layout warning（`line edge reshape budget exceeded`）を除いて空で、この警告も両者で同じ。

## 再計測

```sh
cargo run --release -p shodo-bench --example tcy_base_reuse -- sample <case> <label> <index> <builds>
cargo run --release -p shodo-bench --features allocation-counting --example tcy_base_reuse -- alloc <case> <label> <builds>
cargo test -p shodo --lib tcy_base_reuse_probe_shaper_calls -- --ignored --nocapture
```

`<case>` は `base-two-rl`, `base-three-lr-rtl`, `base-four-rl-multi`, `base-two-lr-rtl-multi`, `base-fallback`, `plain-tcy`, `deep-nested-multi` のいずれか。`<label>` は出力 JSON に記録する baseline/candidate の名前、`<index>` はサンプル番号、`<builds>` は 1 サンプル内の build 回数（本記録では base ケース 90–120、`deep-nested-multi` は 2）。`TCY_PROBE_SKIP_DIGEST=1` を付けると `sample` は出力 digest を省略する。

A/B 比較では、baseline 用に `git worktree add ../shodo-t3t-baseline 3334bea` を作り、probe の example と `dev/bench/Cargo.toml` の `[[example]] name = "tcy_base_reuse"` を複製する。両方を別々の `CARGO_TARGET_DIR` で release build し（割り当て計測用は feature 付きで別に build）、実行ファイルを分けて、ケースごとに baseline/candidate/candidate/baseline（続けて逆順）で交互に実行する。
