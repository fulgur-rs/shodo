# Selectionのsource候補検索（shodo-sbp.9）

2026年10月1日。base ab33dfaから実装65cf3e1・全範囲の修正872d52fへ。選択は元のcaret処理でendpoint/affinityを正規化してから、source候補を絞り、元の視覚sort/mergeを行う。API・source・glyph・geometry・warnings・limits・元input/fontの保持は維持する。切替を止める依存やS4変更はない。

## 原因と実装

元の1文字queryも全S segmentsを走査する。実フォントN1024/4096/16384で正しい1文字rectを確認した後、訪問数1024/4096/16384で64回上限のRED、atomic/tab混在は1027/4099/16387でREDになった。変更後のN16384は通常31回、混在18回でGREEN。query用追加heap割当はない。

segmentsはatomic/tab追加後にglyph由来のsource順segmentsを追加するため、配列全体はsource順とは限らない。実診断でmixed16は19segments/非monotonic、plain/bidi/shared/TCYはmonotonicだった。元segmentsの順序とpaint/accepted_segmentsを維持し、開始・終了がともに非減少なら借用sliceの2つの二分探索を使う。その他は元segment ordinalをsource開始順に並べたimmutable配列と、暗黙のbalanced treeのsubtree max-endを保持する。重複/重なる区間も別geometryとして残し、queryはmax-endで除外しながらstackで訪問する。全source範囲を覆うqueryはmin start/max endの定数検査後に元配列を直接走査する。候補にはempty endpoint等が含まれ得るため元のsource/geometry predicateを維持する。

通常の順序付きrange検索は二分探索+候補走査。一般treeは対数の深さで構築/訪問するが、wide/overlapが多ければ多数のnodeを訪れる。全入力で同じ訪問数や速度を保証しない。元のrect filterと2段の視覚sort/mergeは残した。query1000回だけからO(N²)実測とは呼ばない。

初回の混在全行16queryは2.210→3.744msと約69%遅くなった。期待rectが一致した後、32774訪問/16387候補で余分なtree走査のREDを確認し、全範囲の直接走査でGREENにした。初回source213c55b2の93/54cases、ABBA、validationはprototypeとして保存し、最終sourceの結果と区別する。

## 正しさと実データ

全source交差の旧線形scan/元merge oracleを独立したtest utilityとして保持し、accepted caretの全endpoint/affinity組合せ、逆endpoint、不正位置、通常・bidi/RTL・shared grapheme・atomic/tab・first-line複数dataset・TCY RL/LR・emptyで照合した。部分bidi選択は2rectの視覚gapと逆方向の同一結果を直接検証する。短いqueryの期待rectは候補検索に依存しないcaret座標から確認した。

実shared-grapheme fixtureのaccepted rangeは重複していなかったため、syntheticなunordered/overlap/duplicate source区間に別geometryを割り当て、すべてのIDをliteral期待値で照合した。u32最大endpoint、開区間境界、非monotonic end、内部empty rangeの元predicate、empty datasetも確認する。実glyphで重複区間が観測されたとは主張しない。

固定Latin/Arabic/CJK fonts、system discovery無効、既定limits。15条件（plain/mixed N1024/4096/16384、bidi/RTL、shared256、TCY3種、first-line、empty）のindex/short/last/reverse/affinity/full、bidi gap追加で93cases。short/last/reverse/affinity/gapは1000query、fullは16query、indexは1回。全glyph/source digest、endpoint、rectのfloat bitsと順序が旧/新で一致し、context warningsは空。DOM offset-map全serializationは収集していない。既存実フォントsource/first-line/transform回帰も確認した。

## 構築と保持

index scopeはprepared linesからのLineLayout::newを含み、indexをscope終了まで保持する。fonts/paragraph/shaping/line layoutと検証は除外。query scopeは準備済みindexで固定回数実行し、最後のVecを終了まで保持する。normal/countは別immutable binary。allocation3回のscope calls/gross/deallocation/net/peakは決定的、scope外JSON保持による絶対live増加は判定対象外。

全78query scopesの割当指標は旧/新で同じ。plain short1000回は4000calls・gross256000・net64・peak192bytes。これは割当削減ではなく検索走査の改善である。

index構築の代表値（bytes）:

| 条件 | gross before → after | net before → after | peak追加 before → after |
| --- | ---: | ---: | ---: |
| plain/1024 | 814,544 → 814,568 | 213,272 → 213,296 | 237,752 → 237,776 |
| plain/4096 | 3,259,856 → 3,259,880 | 852,248 → 852,272 | 950,456 → 950,480 |
| plain/16384 | 13,041,104 → 13,041,128 | 3,408,152 → 3,408,176 | 3,801,272 → 3,801,296 |
| mixed/1024 | 1,323,180 → 1,339,636 | 311,912 → 328,368 | 360,968 → 377,424 |
| mixed/4096 | 5,292,204 → 5,357,812 | 1,245,800 → 1,311,408 | 1,442,312 → 1,507,920 |
| mixed/16384 | 21,168,300 → 21,430,516 | 4,981,352 → 5,243,568 | 5,767,688 → 6,029,904 |

このhostでSourceIndex headerは1LineIndexあたり24bytes、Orderedも保持増がある。genericはさらに1segmentあたり16bytesと1allocationを保持する。mixed N16384は262216bytes増（約5.26%のindex net増）。これらはpaint/accessibility等のLineIndex構築にもかかる費用であり、全保持量削減は主張しない。RSS未測定。

## 通常時間と採用判断

全検証buildと最終54collector終了後、CPU10でnormal両binaryをwarmupし、before/after/after/beforeの順で各10samplesを測った。各run median（ms）。indexは1構築、short/lastは1000query、fullは16queryで、行同士の時間を同じ操作数とは扱わない。

| 条件/操作 | before 2 runs | after 2 runs |
| --- | ---: | ---: |
| plain/1024 index | 0.205 / 0.202 | 0.205 / 0.208 |
| plain/1024 short | 0.376 / 0.379 | 0.123 / 0.124 |
| plain/1024 last | 0.607 / 0.604 | 0.123 / 0.125 |
| plain/1024 full | 0.142 / 0.142 | 0.139 / 0.139 |
| plain/4096 index | 1.080 / 1.062 | 1.059 / 1.061 |
| plain/4096 short | 1.169 / 1.186 | 0.159 / 0.157 |
| plain/4096 last | 2.025 / 2.025 | 0.157 / 0.159 |
| plain/4096 full | 0.544 / 0.543 | 0.527 / 0.532 |
| plain/16384 index | 4.598 / 4.598 | 4.766 / 4.770 |
| plain/16384 short | 5.494 / 5.130 | 0.180 / 0.182 |
| plain/16384 last | 8.098 / 7.991 | 0.174 / 0.176 |
| plain/16384 full | 2.148 / 2.149 | 2.099 / 2.101 |
| mixed/1024 index | 0.183 / 0.184 | 0.202 / 0.197 |
| mixed/1024 short | 0.381 / 0.381 | 0.156 / 0.162 |
| mixed/1024 last | 0.609 / 0.844 | 0.148 / 0.162 |
| mixed/1024 full | 0.144 / 0.144 | 0.146 / 0.144 |
| mixed/4096 index | 0.843 / 0.825 | 0.891 / 0.890 |
| mixed/4096 short | 1.167 / 1.169 | 0.170 / 0.169 |
| mixed/4096 last | 2.031 / 2.959 | 0.174 / 0.165 |
| mixed/4096 full | 0.568 / 0.566 | 0.532 / 0.539 |
| mixed/16384 index | 4.633 / 4.601 | 5.357 / 5.380 |
| mixed/16384 short | 5.320 / 5.451 | 0.202 / 0.218 |
| mixed/16384 last | 8.200 / 11.829 | 0.191 / 0.193 |
| mixed/16384 full | 2.215 / 2.219 | 2.119 / 2.115 |

plain N16384: short1000query 5.312→0.181ms、index構築 4.598→4.768ms（+3.7%）、full16query 2.148→2.100ms（-2.3%）。 mixed N16384: short1000query 5.386→0.210ms、index構築 4.617→5.369ms（+16.3%）、full16query 2.217→2.117ms（-4.5%）。 短いqueryを同じindexで繰り返す費用削減として採用する。構築時間/保持増は実費であり、queryごとにindexを再構築するcallerに同じ速度比は適用しない。

host/input/scope固有の観測である。Capture/quickの時間は検証buildと重なるため因果的速度根拠に使わない。全93casesの全normal/割当samplesを残し、短い固定queryだけを選んでindex構築/全行queryコストを隠さない。

旧mixed last1000queryにはsequential run間で8.200/11.829msの変動があった。case/operationをscope外で絞った4fresh processのABBAではbefore8.083/8.074→after0.197/0.204msで、旧11.8msは再現しなかった。元の変動の原因は確定していない。両結果を残し、この変動から一律速度比を主張しない。

## 検証と証拠

hit14、workspace1078、既定featureなし625、complex-only628、fmt、全target Clippy、docs（警告error化）、新出力directoryの固定glyph snapshots成功。focused bidi15/空白16/transform20/vertical63も成功し、期待画像更新なし。

sbp6と同じfont/input/harness/lock/toolchain/profile/CPU affinityで通常54casesを収集し、warm/memory各7digestと最終source fingerprint ed629685f7919a4633725d326859f3492b2614141fd5cbf40d6daf69228ca1fdを照合した。検証Rust1.96.0、性能stable1.97.1。beforeはbase productionそのもの。afterはbase+tracked diffに加えnew-source-filesを同じhashで保存し、65cf3e1へ同じsourceをcommitした。最終captureは65cf3e1+tracked refinement diffを保存し、同じ最終sourceを872d52fへcommitした。

[manifest](data/selection-source-index.json)と[raw](data/selection-source-index-raw.json.gz)に全93/54cases、raw samples、source/config/lock、変更Rust全文、4binary SHA256、RED/GREEN・全required logs・native ledger・照合scriptを保存する。元成果物target/performance-artifacts/sbp9-selection-probe、sbp9-selection-source-final、sbp9-snapshots-final（初回sbp9-selection-source/sbp9-snapshotsも保存）も保持する。
