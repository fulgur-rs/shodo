# Ruby候補範囲の計測と列配列縮小（shodo-sbp.5）

2026年9月30日。ruby候補の計測を、合法なpaired endpointまで広げた実際のcolumn/lane範囲に絞った。PreparedRubyのsource順columnと、level順・非重複span順laneをpartition_pointで検索し、paired cutのlookupには元のglobal lane番号を使う。

第1段階はlane走査だけを絞り、列配列を従来の長さに保った。第2段階は列配列を対象範囲だけにし、fragmentに元のcolumn開始番号を持たせた。配列・span・rightmost mapはlocal列番号、source unitsとlane番号はglobalのまま。alignment、placement、block geometry、overhang用metadata参照を開始番号で対応させる。mergeのfont capは、候補の外にある場合も元のlevel最初のlaneから取る。そのlookupも二分検索にした。

PreparedRubyの保持索引やpaired cutsは追加・変更していない。NeighborIndexは元のsource範囲とcontainer境界を使い、周辺依存とnested rubyの調整を維持する。queryはlevelごとの二分検索と選択範囲の走査に変わるが、buildはすべての内容を準備し、dense cut tableなどの保持費用は残る。

## 回帰と全体検証

64/128列・3levelの入力を逆span順で渡してprepared順序も検証する。4列の候補に必要なlaneは7。旧実装では64列でも112laneを訪問して失敗し、範囲検索後はどちらのサイズでも7になった。列計測の旧ループも64列を訪問して失敗し、修正後は4列だけを計測・保持する。

閉じるcontainer境界だけの候補では、laneなし・contentなし・ゼロのcontributionと元のfallbackを維持する。途中の列を含む3行で異なるbase alignment、base/annotation node、base text範囲、DOM mapping unitとglyph source範囲を照合した。generated anchorがDOMと同じ境界を持つ既存規則は変えず、DOM所有のglyph範囲を直接照合する。

rubyの86テスト、workspace 1,064テスト、既定featureなし611テスト、complex-scriptsのみ614テストが成功した。fmt、workspace全targetのClippy（警告をerror化）、docs（同）、固定glyph snapshot比較も成功した。期待画像・font・dependency・limitは変更していない。

## 専用A/B

固定fonts・既定limitsでC=16/64/256/512、base日16px・reading aaa8px・width64pxを使う。C/4行と4C glyphを検証する。さらにC16/64でspanning・多段・空base・merge・overhang・RTL inter-character・nested・first-line・新contextでBreakToken再開を測る。22条件×build/layoutの44行すべてでoriginal/lane-only/compact/finalのdigestが一致した。

buildはinput snapshot作成とshapingを含み、layoutは準備済みParagraphをfresh contextで組む。font load、外側context初期化、結果検証はscope外。resume条件の各line context作成はlayout内。時間と割当は別binary、割当は3回、非instrumented時間は10 samplesで収集した。gross requested bytes、net保持、peak追加を分け、RSSとは扱わない。

通常条件のfresh layout:

| C | calls | gross bytes before → final | net保持 bytes | peak追加 bytes before → final |
| --- | ---: | ---: | ---: | ---: |
| 16 | 6,911（同じ） | 938,334 → 767,470 | 259,096（同じ） | 267,764 → 267,412 |
| 64 | 26,881（同じ） | 6,146,122 → 3,050,618 | 1,034,440（同じ） | 1,044,644 → 1,042,756 |
| 256 | 106,683（同じ） | 63,103,514 → 12,182,730 | 4,135,816（同じ） | 4,158,776 → 4,144,132 |
| 512 | 213,064（同じ） | 228,961,962 → 24,358,746 | 8,270,984（同じ） | 8,318,520 → 8,279,300 |

laneだけの段階では、通常条件のallocation calls/gross/net/peakは変わらなかった。列配列の縮小でgross割当が減り、C256→512のgross増加は約3.63倍から約2.00倍になった。この条件のcallsとnet保持は変わらない。C512のbuildは全段階でcalls128,014、gross30,202,380 bytes、net7,742,689 bytes、peak10,479,193 bytesだった。

すべての検証buildと通常matrixの完了後、CPU affinity 10で各binaryをウォームアップし、original/lane-only/final/final/lane-only/originalの順に非instrumented計測をした。各runの10 samplesのmedian（ms）:

| C | original 2 runs | lane-only 2 runs | final 2 runs |
| --- | ---: | ---: | ---: |
| 16 | 0.619 / 1.162 | 0.618 / 0.616 | 0.638 / 0.582 |
| 64 | 3.491 / 3.596 | 3.378 / 3.311 | 2.402 / 2.415 |
| 256 | 27.189 / 37.899 | 25.116 / 25.084 | 10.486 / 10.534 |
| 512 | 84.562 / 84.434 | 79.923 / 79.750 | 22.282 / 21.888 |

build時間も同じscopeでrawに保存した。時間はこのホスト・入力での観測であり、全入力への改善率を保証しない。通常matrixのquick時間は検証buildと並行したため、そこから改善率は主張しない。

## 通常matrixと証拠

shodo-sbp.4をbaselineとし、同じfont/input/harness/lock/toolchain/profile/CPU affinityで全54ケースを収集した。378 warm操作・108 cold process・54 memory processがrunner検証を通過し、各ケースのwarm/memory各7 digestが一致した。収集後のsource fingerprintも一致した。途中の追加cfg(test)確認を直す際に最初のcollectorを停止し、確定ソースから全54ケースを取り直した。

baseは973293d9aa5df544b7f801217ac79f4a1fdb06b7、実装commitは8a5f7f5と4e69f9a。検証Rustは1.96.0、性能binaryはstable1.97.1。専用probeのexternal dependency versionはworkspace lockと一致する。固定font loaderはsystem discoveryを無効化している。original binaryはtest-only counter追加後の未変更production実装から作り、以降は保存binaryを実行した。

検証manifestと生データarchiveに全段階の割当・時間・digest、54 matrix、probe source/manifest/lock、binary SHA256、source hash/patch、RED/GREENと全体検証ログを保存する。元成果物はtarget/performance-artifactsのsbp5-probe、sbp5-ruby-candidates、sbp5-snapshots。

raikiri切り替えを止める性能依存は追加していない。保存済みS4 spikeへの変更はない。
