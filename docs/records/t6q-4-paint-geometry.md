# t6q.4: paint専用の共有geometry生成

採用。glyph/GDEF/grapheme/TCY等からのsegment生成を`LineGeometry`へ切り出した。hitは同じ生成結果にcaretの整列・visual順・source/spatial/combined索引を仕上げる。paintはsegmentだけを生成し、caretの保持と索引構築を省く。geometryの計算式、GDEF fallback、ruby padding、atomic/tab、bidiと縦中横の扱いは共有のまま。公開API・limits・glyph/source/geometryの意味は変わらない。

基準commitは`6aace4f01d20c02fd0eb61acc4abd9485d71c6da`。Rust 1.96.0、x86_64 Linux、release、固定fixtureフォント（manifestのchecksum）、system fontなし、Limits::default。既存Workloadの5ケースをscale8で使用した。

`paint_geometry` exampleはparagraph build、layout、元のsource/glyph/geometry digestを計測外で用意する。scopeは全accepted lineの`paint_spans`収集だけで、返却spansを保持した状態で終了する。時間とallocatorは別feature buildで、counts/elapsedをlocalへ確定してからJSONを作る。paintの全Debug SHA-256と元のdigest、paragraph警告をscope外で照合する。Font binaryのDebug展開はない。

通常allocatorの時間は初回9標本と保存binaryのABBA各9標本で計27標本/側。割当は別binaryで9標本/側。共通flock、jobs1、nice10で同時build/測定を避けた。全5ケース・全反復・両featureでpaint hash、source/glyph/geometry digest、警告が前後一致した。

| ケース | Calls 前→後 | Gross bytes 前→後 | Net bytes（同じ） | Peak extra bytes 前→後 |
|---|---:|---:|---:|---:|
| latin-long | 5,569→3,697 | 9,073,312→6,624,288 | 1,183,104 | 1,189,588→1,189,588 |
| arabic-long | 3,763→2,508 | 6,293,596→4,556,092 | 789,784 | 797,164→797,164 |
| combining-latin | 162→108 | 216,096→162,112 | 29,304 | 35,276→35,276 |
| nested-atomic | 645→430 | 975,076→726,356 | 132,504 | 138,000→138,000 |
| preserved-tabs | 552→336 | 236,172→161,996 | 41,536 | 42,528→42,328 |

保持するPaintSpanは同じなのでnetは不変。大半のpeakも返却spansに支配されるため変わらず、tabsだけ200B減った。削減は主に即破棄されていた一時索引のgross確保であり、RSSや保持メモリ全般の改善とは扱わない。

| ケース | paint中央値 ms 前→後 | 後/前 |
|---|---:|---:|
| latin-long | 1.753→1.342 | 0.766 |
| arabic-long | 1.343→1.005 | 0.748 |
| combining-latin | 0.031→0.023 | 0.734 |
| nested-atomic | 0.147→0.109 | 0.741 |
| preserved-tabs | 0.050→0.036 | 0.723 |

共有ホストの負荷は変動しており、この限定paint scopeの観測値からアプリ全体の改善率は主張しない。採用根拠は未使用索引の実構築を避けること、確保削減、出力等価性。raikiri/S4切り替えの必須依存にはしない。

回帰テストではTCYのVerticalRl/VerticalLr×LTR/RTL各3行でpaintとhitのsegmentを直接照合し、navigation構築の実入場が0であることを要求する。変更前はactual1/expected0でRED。変更後はGREEN。既存GDEF variation/scale/fallback、combined ligature、閉区間のTCY caret等を含むindex unit6件も通過した。既存paint/hit/ruby harnessを併用し、限定benchmarkだけを全契約の証明にしていない。

検証: baselineの既存unit1・harness30 pass、最終`cargo test -p shodo`793 pass/既存診断ignore2、paint_styles/hit_test/ruby_paint30 pass、shodo/bench all-targets clippy（accesskitとallocation-counting有効）とfmt通過。独立静的レビューではgeometry生成とnavigation仕上げの正規化比較も行い、重要な指摘はなかった。

再現:

```sh
cargo run --release -p shodo-bench --example paint_geometry
cargo run --release -p shodo-bench --example paint_geometry --features allocation-counting
```

変更前に同じexampleを配置し、通常・計数の実行ファイルを保存して交互に実行する。rustc wrapperは空、target runnerは`/usr/bin/env`を使用した。Raw/保存binary/summaryはworkspaceの`target/performance-artifacts/t6q-4/`、検証logと測定scriptは`target/t6q-control/`に保存。
