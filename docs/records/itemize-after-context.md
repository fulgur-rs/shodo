# run確定時の後方context構築（shodo-sbp.3）

2026年9月30日。itemizeで同じrunにgraphemeを追加するたびに作っていた後方context Stringを、run境界で一度だけ構築する。途中のsuffixはshapingに使われず、直前のStringを置き換えるだけだった。

次runの `part_start` は直前runの最後の `part_end` と一致するため、同じ最大5 scalarを収集できる。segmentの最後は空contextとなる。追加の索引や保持bufferは使わず、`before`、scalar/source範囲、segment、combine、style/fontの互換性判定、shaping limitsを維持する。

## 回帰と全体検証

長Latin・Arabicの単一run、およびßを展開するUppercase first-lineの構築回数を検証した。旧実装では単一runに2,048回となり失敗し、修正後は通常runが1回、通常とfirst-lineの合計が2回となる。別のテストでは透明なinline境界で分かれた結合文字、styleによるrun分割、paddingのhard boundaryについてliteralの前後contextとsource範囲を確認した。

以下の検証が成功した。期待画像は更新していない。

- workspace全1,057テスト。Arabic shaping、透明DOM grapheme、変換・first-line、細かいshaping budget、巨大grapheme、combineの既存実フォントテストを含む。
- 既定featureなし604テスト、complex-scriptsのみ607テスト。
- fmt、workspace全targetのClippy（`-D warnings`）、workspaceドキュメント（警告をerror化）。
- 固定glyph snapshot比較。
- 独立コードレビュー: 指摘なし。segment末尾、source gap、重複offsetの幅変換、combine境界とshaper側consumerの同値性を確認。

## 割当と出力のA/B

shodo-sbp.2で保存した修正前binaryと今回の修正後binaryを比較した。同じcoverage version 2・font/input/harness/lock/toolchain/profile・CPU affinity 10で全54 workloadを収集し、378 warm操作、108 cold process、54 memory processがrunner検証を通過した。全54ケースでwarmの7種類とmemoryの7種類のdigestが修正前後で一致した。

build scope、scale 1の代表値:

| 入力 | 割当回数 before → after | gross bytes before → after | net保持 bytes | peak追加 bytes |
| --- | ---: | ---: | ---: | ---: |
| latin-long | 2,539 → 1,581 | 848,621 → 840,957 | 320,858（同じ） | 329,256（同じ） |
| arabic-long | 2,841 → 1,453 | 777,926 → 761,750 | 282,690（同じ） | 288,245（同じ） |
| mixed-scripts | 1,162 → 1,118 | 238,598 → 238,134 | 72,230（同じ） | 74,926（同じ） |

全54ケースでbuildの割当回数とgross bytesが減り、net保持とpeak追加は変わらなかった。代表3条件をそれぞれ3回、別processで再測定し、各A/Bで7 digestが一致した。保持メモリやRSSの削減は主張しない。

## 計測条件と証拠

通常のRust検証は1.96.0、性能binaryはstable 1.97.1。全matrixのquick時間は検証buildと一部並行したため、そこから改善率を主張しない。通常の時間測定は割当カウンタを有効にしていない別binaryで実行する。

他の検証buildが終了してから、CPU affinity 10でCriterionの通常設定（100 samples、warmup 3秒、measurement 5秒）を使い、各入力のscale 1をbefore/after/after/beforeの順に測定した。各runのmedianを2回分まとめたmedianは、Latin-longが474.68 → 456.00 µs、Arabic-longが511.03 → 498.96 µsだった。after/beforeはそれぞれ0.9607、0.9764。このホスト・入力での観測値として記録する。特にArabicのafterは486.76 / 511.15 µsとrun間で揺れており、小さい時間差を一般的な速度保証にしない。測定したbinaryのSHA256を記録し、各A/Bの全7 digestも一致した。

baseは `0c8e916a09adef45cbf1586c3c8fe6cde4115f25`。matrix収集時点の変更は未コミットだった。[検証manifest](data/itemize-after-context.json) と[生データarchive](data/itemize-after-context-raw.json.gz) に修正前後の全matrix、反復probe、時間samples、binary SHA256、source hash、red/green・全体検証ログを保存する。

元成果物は `/home/mitz/Work/oss/shodo/target/performance-artifacts/` の `sbp2-fingerprint`、`sbp3-after-context`、`sbp3-normal-timing`、`sbp3-snapshots`。raikiri切り替えへの性能ブロック依存や、保存済みS4 spikeへの変更はない。
