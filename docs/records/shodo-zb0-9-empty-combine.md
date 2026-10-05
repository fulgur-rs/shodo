# shodo-zb0.9: 空の combine span に対する membership pass

## 判断

`analyze_data` の break opportunity 走査と `build_data` の unit 走査を、`combine_spans` が空でない場合だけ実行する guard は採用しない。combine なし段落では両 pass の外側ループ訪問が 0 になるが、固定フォントの release build で再現性のある速度改善を確認できなかった。combine span の有無による break class、unit membership、拒否警告、line-break override callback の契約を確認するテストと、再計測用 probe は残す。

候補が一貫して遅いとまでは結論づけない。今回の測定では採用条件を満たす速度改善の証拠が得られなかった。

## 方法

- baseline は `043e14320e18c153b8478a136f74c0794900adac`。candidate は上記 2 pass を `!combine_spans.is_empty()` で囲む試験変更。拒否候補の警告と line-break override callback は guard の外に置いた。
- baseline/candidate は target directory を分けた別々の release バイナリとして構築し、SHA-256 はそれぞれ `6fab9a5b8202981b5299aa86f9227ef3fd5aa831b3d912cb00c27ccf06e901b3` と `7feebc13391c5946e77ae062c06bbb9a273f67181a9ae8b36793feea77d6c2d8`。
- `dev/bench/examples/combine_empty_passes.rs` で固定フォントの段落を作成した。combine なしの短文・長文、縦書きの TCY 混在、横書きで無効になる TCY、境界で拒否される TCY 候補、`::first-line` 付き、空段落の 7 ケースを測定した。
- builder と `LayoutContext` を計時前に用意し、1 サンプル内で `builder.build()` を固定回数実行して平均時間を記録した。フォント読込、warm-up build、`break_all`、出力 digest、warning 取得は計時範囲外。
- 12 ラウンドを ABBA 順（baseline/candidate/candidate/baseline）で測定した後、順序を反転した BAAB 順で独立に 12 ラウンド測定した。各ラウンドでは同じラベルの 2 サンプルを平均した。
- baseline/candidate の出力 SHA-256 と build/layout warning は、両 run の全ケース・全サンプルで一致した。全測定値と環境情報は raw JSON に記録した。
- rustc 1.96.0、AMD Ryzen 5 5600G、Linux x86_64。scaling governor は `performance`、boost は有効。

## 時間

`Δ` は candidate と baseline の中央値の差。正値は candidate が遅く、負値は candidate が速い。「速い比較」は 12 ラウンド中で candidate が速かったラウンド数。

| ケース | combine spans | builds / sample | ABBA Δ | ABBA 速い比較 | BAAB Δ | BAAB 速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| plain short | 0 | 4,000 | −1.20% | 11/12 | −0.84% | 8/12 |
| plain long | 0 | 200 | −0.66% | 8/12 | −0.43% | 5/12 |
| TCY mixed | 16 | 600 | +0.51% | 4/12 | −0.07% | 5/12 |
| horizontal disabled TCY | 0 | 500 | +0.58% | 6/12 | +0.62% | 5/12 |
| rejected TCY | 0 | 4,000 | −0.17% | 5/12 | −0.89% | 7/12 |
| first-line | 0 | 300 | +0.18% | 7/12 | −0.18% | 4/12 |
| empty | 0 | 10,000 | −0.48% | 6/12 | −1.24% | 9/12 |

baseline の中央値は plain short が約 23.8 µs、plain long が約 555 µs、first-line が約 310 µs。

plain short の candidate は両 run で中央値が速く、全ケース中で最も一貫した改善だった。ただし改善は約 0.2–0.3 µs で、省いた外側ループ 11 回分（6 opportunities + 5 units）の空 slice に対する `partition_point` として説明できる量を大きく超える。省くループが原因なら、訪問数が数百に増える plain long、2 段落分を build する first-line、64 語の horizontal disabled TCY で改善が大きくなるはずだが、これらは ±0.7% 以内で速い比較もそろわなかった。効果が訪問数に比例しないため、plain short の差はコード配置などの測定ノイズと区別できない。shodo-zb0.8 で不採用にした −0.83% と同じ規模でもある。guard の影響を受けない TCY mixed は ABBA で +0.51%、BAAB で −0.07% だった。

## 訪問回数と意味の同値性

measurement 中だけ置いた test-only のスレッド局所カウンターで、baseline の外側ループ訪問を数えた。短い combine なし段落は 6 opportunities / 5 units、横書きで TCY 指定が無効になる `"12"` は 1 / 0 を訪問し、guard 付きでは combine span が空のすべての段落で 0 になった。非空の span では訪問数は変わらない。訪問数の差だけでは build 時間の効果を示さなかったため、カウンターは削除した。

追加したテストで、次の既存契約を固定する。

- combine なしの短文・長文と `::first-line` alternate、空段落、横書きで無効になる TCY は `combine_spans` を持たず、どの unit も combine に属さない。
- 2 つの TCY span では、span 内部の break opportunity が `Prohibited` かつ min-content 対象外になる。各 span の unit は span index、bidi level、内部 break の禁止、slice advance を保つ。
- 境界で拒否された TCY 候補は combine span を作らず、2 件の警告を出す。line-break override callback は offset 1, 2, 3 の順に呼ばれ、その結果が break class に反映される。

## 再計測

```sh
cargo run --release -p shodo-bench --example combine_empty_passes -- sample <case> <label> <index> <builds>
```

`<case>` は `plain-short`, `plain-long`, `tcy-mixed`, `tcy-horizontal-disabled`, `tcy-rejected`, `first-line`, `empty` のいずれか。`<label>` は出力 JSON に記録する baseline/candidate の名前、`<index>` はサンプル番号、`<builds>` は 1 サンプル内の build 回数。A/B 比較では同じ probe を baseline と候補の両方で release build し、両者の target directory と実行ファイルを分離する。
