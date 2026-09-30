# LineLayoutのcaret cuts借用（shodo-sbp.8）

2026年10月1日。LineIndex::cutsが確定caret_cutsの部分sliceを毎回Vecへ複製していた。通常/縦中横のcluster集計時にはlen確認だけの短命Vecを作り、最終addでも再度複製していた。cutsの返却値をLineにlifetimeを結び付けた&[u32]にし、addまで借用する。atomic/tabの2要素はstack配列から借用する。

partition_pointの端点を含む範囲、不可分なclusterの結合、GDEFの内部cut数と座標、ruby padding、RTL/縦中横のcaret geometryは変更していない。addはu32を既存のowned Caret/Segmentにコピーする。元Lineのcaret_cutsとindexの保持形態、公開APIは同じ。

## 回帰と検証

実フォントの3個の縦中横boxを別行に確定し、中央行の全/部分/単独/空cut範囲について、値と元datasetのslice pointerを確認する回帰を追加した。旧to_vecは正しい[2,3,4]の値まで到達してpointer一致に失敗する。借用後はVerticalRl/VerticalLr×Ltr/Rtlで成功する。元caret tableのpointer/lengthも維持する。

hit関連7テスト（GDEF/variation・圧縮carets、縦中横のclosed endpoints含む）、workspace1,066、既定featureなし613、complex-only616テストが成功した。文字変換/共有grapheme・ruby・RTL・atomic/tabの既存回帰を含む。fmt、全target Clippy、docs（警告error化）、固定glyph snapshotsも通過し、期待画像は更新していない。Clippyはテストの冗長slice指定を一度検出し、直接比較へ直して再通過した。

## 専用構築A/B

固定Latin/CJK/Arabic fontsで8通常workload（短/長Latin、長Arabic、mixed、combining、many-short、nested-atomic、preserved-tabs）と6追加条件（Uppercase変換、共有grapheme/ffi、VerticalRl縦中横、VerticalLr RTL縦中横、ruby、RTL ruby）をscale1/64で測り、計28条件。既定limits・system discovery無効。通常workloadの既定幅、追加条件は幅280でParagraphと確定linesを先に作り、LineLayout::newだけを測る。font/context/build/layout準備とsource/glyph/geometry/query照合はscope外。indexはscope終了まで保持する。

各lineの全byte offsetと両affinityについてcaret、hit、終端までのselection、論理forward/視覚backward navigationを照合し、retained ruby annotationの変換座標でhitも確認する。列を長さ付きDebug表現からSHA256へ連結し、query数を保存する。計1,926,610 query records/pass。28条件すべてでglyph/source/geometry digest、正確なwarnings/DOM mapping、全query signatureが一致した。実装sourceと照合式はraw archiveに含む。

normalとallocation-countedは別immutable binary。割当3回は各条件で決定的、全28条件でcalls/gross減、net保持と測定scope peakは同じだった。代表例（bytes）:

| 条件 | calls before → after | gross before → after | net保持（同じ） | peak追加（同じ） |
| --- | ---: | ---: | ---: | ---: |
| latin-long/64 | 156,932 → 34,180 | 45,035,264 → 44,053,248 | 14,274,560 | 14,302,208 |
| arabic-long/64 | 119,698 → 23,058 | 31,122,536 → 30,349,416 | 10,346,520 | 10,364,976 |
| mixed-scripts/64 | 7,887 → 2,767 | 2,574,440 → 2,533,480 | 738,360 | 740,208 |
| preserved-tabs/64 | 6,275 → 3,395 | 1,265,644 → 1,242,092 | 472,064 | 476,672 |
| custom-transform/64 | 10,755 → 3,075 | 2,500,608 → 2,443,264 | 830,464 | 833,536 |
| custom-tcy-vrl/64 | 1,852 → 828 | 507,184 → 498,992 | 157,976 | 170,168 |
| custom-ruby/64 | 5,754 → 4,218 | 850,976 → 838,688 | 367,424 | 367,808 |

元Lineとindexの保持は減らない。短命cut Vecの削減をnet保持・peak・RSS削減とは扱わない。RSSは計測していない。

全検証buildと通常collector終了後、CPU10でnormal両binaryをウォームアップし、before/after/after/before順に各10 samplesを測った。各run median（ms）:

| 条件 | before 2 runs | after 2 runs |
| --- | ---: | ---: |
| latin-long/64 | 19.254 / 19.264 | 18.064 / 18.196 |
| arabic-long/64 | 12.131 / 12.093 | 10.958 / 11.130 |
| mixed-scripts/64 | 0.765 / 0.746 | 0.666 / 0.820 |
| preserved-tabs/64 | 0.477 / 0.385 | 0.351 / 0.450 |
| custom-transform/64 | 0.872 / 0.859 | 0.842 / 0.845 |
| custom-tcy-vrl/64 | 0.176 / 0.186 | 0.176 / 0.192 |
| custom-ruby/64 | 0.459 / 0.419 | 0.397 / 0.424 |

長Latin/Arabicではこのホスト・入力・scopeの構築時間が短くなった。短いmixed/1はbefore0.0118/0.0120ms、after0.0100/0.0271msとprocess間の差が大きかった。同じimmutable binaryを入力別fresh-processで比較するとbefore0.0305/0.0286ms、after0.0278/0.0274msで、遅さは再現しなかった。全元samplesとisolated samplesを残す。厳密なcache/allocator原因は確定しておらず、全入力への一律高速化は保証しない。既存indexを使うhit queryの速度改善としても扱わない。通常quick matrix時間は検証buildと重なり、因果的な速度根拠にはしない。

## 通常matrixと証拠

sbp7と同じfont/input/harness/lock/toolchain/profile/CPU affinityで全54ケースを収集した。warm/memory各7 digestが一致し、378 warm操作・108 cold process・54 memory processと最終source fingerprintもrunner検証を通過した。

base550c4a23e4fca6b0a735c59291198adb714af2b2、実装0669ad93a78a4192ef82754e9d748a1630c1d405。検証Rust1.96.0、性能stable1.97.1。専用probe外部依存のversionはworkspace lockと一致する。original binaryはpointer回帰だけ追加した未変更productionから保存した。

[manifest](data/borrowed-caret-cuts.json)と[raw archive](data/borrowed-caret-cuts-raw.json.gz)に全28/54ケース、query signatures、時間/割当samples、source/config/lock、4binary SHA256、RED/GREEN・required logsを含む。元成果物はtarget/performance-artifacts/sbp8-probe、sbp8-caret-cuts、sbp8-snapshots。切替を止める性能依存やS4 spike変更は追加していない。
