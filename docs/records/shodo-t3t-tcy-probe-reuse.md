# shodo-t3t: Ruby base 内の TCY 幅 probe 結果の再利用

## 判断

採用する。Ruby を含む段落では、すべての ruby base が limits を持つため（`crates/shodo/src/ruby/pairing.rs:80`）、TCY の幅 probe は常に BaseScope 経路を通る。この経路で、選択された幅 probe の試行出力を TCY group の最終 shaping に再利用する。再利用は、group の各入力が scoped と global の run 予算の下で 1 window に収まり、かつ状態を変更しない累積 BaseScope preflight が通る場合に限る。条件を満たさない group は従来どおり scoped 経路で再 shaping するため、limit エラー、失敗の分類、警告、出力は従来経路と等しい。

固定フォントの release build で、4 つの base ケースが約 7–8% 速くなった。ABBA と BAAB のどちらでも 12/12 ラウンドで candidate が速かった。

## 方法

- baseline は `3334beaf53fa683ab4ee9803b5b413a1b187ad00`（main）。candidate は `2280da452d813e594a1145b6233be537be718e1c`（ブランチ perf/shodo-t3t-tcy-probe-reuse の HEAD、再利用実装と probe を含む）。
- baseline/candidate は target directory を分けた別々の release バイナリとして構築した。SHA-256 は baseline が `9bf2449f5605541e0e6cc364669ab6b718afdc9e128eaa3995a91a52ed478918`、candidate が `765a6e91c7f6dc01d88478c9bcb7e92f441b9f64cb6497a98177047856333e30`。baseline 側には probe の example と `Cargo.toml` の `[[example]]` だけを追加した。
- `dev/bench/examples/tcy_base_reuse.rs` は `WIDTH_PROBE_CASES` の 4 形状を公開 API で再現する。TCY 用ラッパーと annotation は CJK（`FONTS[1]`）、区切りの `"x"` は Latin（`FONTS[0]`、19px、TCY なし）。各ケースは TCY group を 64 個、1 つの RubyBase に入れる。
  - `base-two-rl`: `"12"`、vertical-rl LTR
  - `base-three-lr-rtl`: `"123"`、vertical-lr RTL
  - `base-four-rl-multi`: `"1234"`、vertical-rl LTR、複数スタイル（末尾に `"x"`）
  - `base-two-lr-rtl-multi`: `"12"`、vertical-lr RTL、複数スタイル（末尾に `"x"`）
  - `base-fallback`: `base-two-rl` で base の `max_shaping_run_bytes = Some(1)`。全 group が fallback する
  - `plain-tcy`: Ruby なしで `"12"` の TCY 64 group を `"x"` で区切る。scope を使わない対照
- annotation は `"日"` を使う。固定 CJK フォントに `"注"` がなく、`geometry_snapshot` が欠落 glyph を拒否するため。annotation の shaping は baseline/candidate で同一。
- builder と `LayoutContext` を計時前に用意し、1 サンプル内で `builder.build()` を固定回数実行して平均を記録した。フォント読込、warm-up build、`break_all`、出力 digest、warning 取得は計時範囲外。
- 12 ラウンドを ABBA 順（baseline/candidate/candidate/baseline）で測定した後、BAAB 順（candidate/baseline/baseline/candidate）で独立に 12 ラウンド測定した。各ラウンドでは同じラベルの 2 サンプルを平均した。1 サンプルは 60–75 ms 程度。
- rustc 1.96.0、AMD Ryzen 5 5600G、Linux x86_64。scaling governor は `performance`、boost は有効。測定中に他の重い処理は走らせていない。全測定値と環境情報は [raw JSON](data/shodo-t3t-tcy-probe-reuse-ab.json) に記録した。

## 時間

`Δ` は candidate と baseline の中央値の差。負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。

| ケース | builds / sample | baseline 中央値 (ABBA) | ABBA Δ | ABBA 速い比較 | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| base-two-rl | 100 | 661.8 µs | −8.0% | 12/12 | −8.0% | 12/12 |
| base-three-lr-rtl | 100 | 724.2 µs | −7.2% | 12/12 | −7.5% | 12/12 |
| base-four-rl-multi | 90 | 769.7 µs | −7.5% | 12/12 | −6.5% | 12/12 |
| base-two-lr-rtl-multi | 100 | 680.6 µs | −8.2% | 12/12 | −7.5% | 12/12 |
| base-fallback | 100 | 698.1 µs | +4.4% | 0/12 | +4.4% | 0/12 |
| plain-tcy | 120 | 522.4 µs | +0.1% | 3/12 | +0.2% | 5/12 |

ラウンドごとの比の中央値（candidate/baseline）は base 4 ケースで ABBA 0.920–0.929、BAAB 0.918–0.932。ラウンドごとの最大比は 0.9575 で、1 ラウンドも 1 を超えなかった。

`base-fallback` は ABBA/BAAB ともに 12 ラウンド全部で candidate が遅く、ラウンド比の中央値は 1.040 / 1.045（中央値の差で約 +4.4%）。これは preflight と shaping trace、および捨てられる retained trial のコストで、再利用が成立しない group が毎回この追加分を払うことによる。発生するのは、base の run byte 予算が TCY group の byte 数を下回る場合や、glyph 上限がほぼ使い切られている場合だけで、既定の ruby limits では起きない。係数は定数倍で、段落サイズに対して増え続けるものではない。

`plain-tcy`（Ruby なし、scope なし）は ±0.2% 以内で、速い比較も 3/12 と 5/12 に割れた。この経路は変更しておらず、ノイズの範囲。

## 呼び出し回数と同値性

再利用が成立した group では、TCY の入力ごとに harfrust の shape 呼び出しが 1 回減る。テストでの期待値は、`repeats = 1` でちょうど 1 回減ること、複数入力の group で 10 回中 3 回減ること。

出力と挙動の同値性を、reference 経路（再利用なし）との比較で固定した。

- base の glyph 上限・run byte 上限と段落の glyph 上限の行列（repeats 1/2）で、出力・エラー・警告が reference と一致する（`scoped_output_reuse_matches_reference_across_base_and_paragraph_limits`）。
- `::first-line` を含む累積 base glyph 予算（`scoped_output_reuse_matches_reference_for_first_line_cumulative_base_glyphs`）。
- 警告上限が尽きた状態（`scoped_output_reuse_matches_reference_under_exhausted_warning_caps`）。
- 入れ子の base scope（`scoped_output_reuse_matches_reference_for_nested_base_scopes`）、および入れ子の base 内の複数入力 group（`scoped_output_reuse_matches_reference_for_multi_input_groups_in_nested_bases`）。
- 局所 glyph 上限を保ったまま最終 shape 回数が減ること（`scoped_output_reuse_saves_final_shapes_and_keeps_local_glyph_caps`）。
- 状態を変更しない累積 preflight の単体テスト（`crates/shodo/src/ruby/base_budget.rs` の `shaped_glyph_preflight_*`）。共有祖先の charge の累積、内側と消費済み分の判定で状態が変わらないこと、`charge` と同じ飽和動作を確認する。

baseline/candidate の出力 SHA-256 と build/layout warning は、両 run の全ケース・全サンプルで一致した（警告はいずれも空）。

## 再計測

```sh
cargo run --release -p shodo-bench --example tcy_base_reuse -- sample <case> <label> <index> <builds>
```

`<case>` は `base-two-rl`, `base-three-lr-rtl`, `base-four-rl-multi`, `base-two-lr-rtl-multi`, `base-fallback`, `plain-tcy` のいずれか。`<label>` は出力 JSON に記録する baseline/candidate の名前、`<index>` はサンプル番号、`<builds>` は 1 サンプル内の build 回数。

A/B 比較では、baseline 用に `git worktree add ../shodo-t3t-baseline 3334bea` を作り、probe の example と `dev/bench/Cargo.toml` の `[[example]] name = "tcy_base_reuse"` を複製する。両方を別々の `CARGO_TARGET_DIR` で release build し、実行ファイルを分けて、ケースごとに baseline/candidate/candidate/baseline（続けて逆順）で交互に実行する。
