# Ruby cursor切断表の疎化（shodo-sbp.6）

2026年10月1日。shodo-sbp.5/7/8適用後のbase b2ad3baから、rubyのpaired cutごとに全lane cursorを複製する保持表を診断した。合法cut数、lane数、論理cell数と、Vecの実capacity・表payload・構築要求byte・net保持・scope peak・時間を分けて測った。

## 診断と採用判断

独立した短いannotationをC個並べる入力では、合法cutがC+1、laneがC、cellがC(C+1)。C512は262,656cell/512変化で、表metadataとcursor Vecが2,142,208bytesを保持した。段落構築scopeのnet7,742,689bytesの約27.67%であり、保持量を減らす試作を行う根拠になった。CPU時間の支配項とは証明していない。

親unitをkeyにせず、paired-cutの行番号を持つLaneCursorsから1個のimmutable表を共有する。laneごとの初期cursorと変更行/値を保存し、変化が多いlaneはdense Vecへ1回変換する。変更数だけでなく実capacityと実cut数でも最終表現を選び、8cut未満では小さなrow-major dense表を使う。measure/source matchingは必要なlaneだけ参照し、行全体を再構成しない。cut metadataも新しいrow viewで小さくなり、確定row数のVecに入る。これらを1つの保持表変更として測った。

旧walkの合法cut判断、mandatory/emergency等のclass優先順位、nearest cursorの選択は変更していない。prepareは旧count→構築の2walkを保ち、ancestor/base/global Itemsには旧count×(1+lane数)を割当前に課金する。物理表が小さくても予算は緩めていない。C1024は旧実装と同じItems actual1,064,969、limit1,048,576で拒否する。切替を止める性能依存やS4変更は追加していない。

## 実表と回帰

実フォントのC16/64/256/512、first-line C512、1つの長いbaseに1/4 levelの全範囲readingを付けたdense条件を診断した。全9表で親unit/class/全cursorのDefaultHasher fingerprint、row/lane/cell数、変化数が旧表と一致する。全範囲のmulti-column spanは内側のcolumn境界を禁止し2cutになるため、dense条件は単一baseに多数のCJK文字を置いて各cutでcursorを進める。初期診断の重複span/範囲指定は入力検証に拒否され、fixtureを修正した。canonical before-finalのみを比較に使う。

C512 normalのmetadata+表payloadは2,142,208→41,024bytes、first-line表は2,121,768→41,024bytes。afterのpayload集計はArc制御block/allocator overheadを除く。外部allocatorのnet差はnormalの差から16bytesを引いた値と一致し、要求・保持・peakの証拠には外部scope countersを使う。dense4 C512は全2,052cell中2,048変化で全4laneがdenseとなり、表payloadは57,376→32,992bytes（afterはArc制御block除外）。

64個の短いlaneについて全rowの期待cursorを直接照合してから16KiB保持上限を確認する回帰を追加した。旧実装は値の照合後に38,400bytesで失敗し、変更後は5,184bytes（Arc制御block除外）で通過した。同じ親unitを持つ9rowの異なるcursor/class、小/大row数と過大な上界でのdense fallbackも直接期待値と照合する。ruby89テストが成功した。

## 専用構築とlayoutのA/B

固定CJK/Latin fonts、既定limits、system discovery無効。30条件×build/layoutの60ケース。C16/64/256/512の短い独立annotationに加え、first-line、spanning、multilevel、empty、merge、overhang、RTL inter-character、nested、resume、全column span、単一長baseのdense1/4を含む。input snapshotとshapingを構築scopeに含め、font load/context初期化と出力検証を除く。layoutは準備済みparagraphを使い、出力をscope終了まで保持する。glyph/geometry/text range、ruby node/level/span/transformのdigestが60ケースすべて一致し、context warningsが空であることを毎回確認する。このprobeはDOM offset mappingの完全serializationを収集していない。source対応は旧表cursor fingerprintと既存normal/first-line/transform/共有clusterの実フォント回帰で確認する。

normal/countは別immutable binary、allocation3回はscopeのcalls/gross/deallocation/net/peakが決定的。JSON結果をscope外で保持するため絶対live/start_liveは回ごとに増える。測定したbuild30条件のnetは減り、layout30条件の全allocation指標は同じ。代表構築条件（bytes）:

| 条件 | gross before → after | net before → after | peak追加 before → after |
| --- | ---: | ---: | ---: |
| plain/16 | 903,964 → 901,708 | 187,121 → 185,025 | 276,473 → 274,297 |
| plain/64 | 3,563,596 → 3,529,468 | 746,273 → 713,073 | 1,091,801 → 1,058,521 |
| plain/256 | 14,570,764 → 14,040,508 | 3,351,521 → 2,825,265 | 4,721,753 → 4,195,417 |
| plain/512 | 30,164,492 → 28,055,228 | 7,742,689 → 5,641,521 | 10,479,193 → 8,377,945 |
| first-line/512 | 36,318,481 → 32,161,193 | 11,452,595 → 7,270,699 | 13,983,979 → 9,920,591 |
| dense-4/512 | 2,485,283 → 2,485,395 | 822,058 → 797,690 | 979,984 → 971,840 |
| full-4/512 | 14,469,884 → 14,469,900 | 2,652,073 → 2,652,025 | 3,755,750 → 3,755,750 |

C512 plainのcallsは128,014→128,016と少し増える。true full-spanのgrossは16bytes増え、dense4は112bytes増えるため、一律にcalls/grossが下がるとは扱わない。RSSは未測定。元input/source/shapingの保持を削っていない。

全検証buildと54ケースcollector終了後、CPU10で両normal binaryをウォームアップし、before/after/after/beforeの順で各条件10 samplesを測った。構築の各run median（ms）:

| 条件 | before 2 runs | after 2 runs |
| --- | ---: | ---: |
| plain/16 | 0.631 / 0.614 | 0.632 / 0.639 |
| plain/64 | 2.227 / 2.182 | 2.302 / 2.295 |
| plain/256 | 9.053 / 8.770 | 9.318 / 9.306 |
| plain/512 | 19.182 / 18.761 | 39.688 / 20.185 |
| first-line/512 | 24.075 / 23.718 | 49.126 / 25.232 |
| dense-4/512 | 1.834 / 1.707 | 1.712 / 1.713 |
| full-4/512 | 7.208 / 7.289 | 7.078 / 7.053 |

最初のafter runはC512 plain39.688ms/first-line49.126msとbeforeの約2倍になり、多くのlayout条件も遅くなった。次のafter runでは20.185/25.232msに戻った。この差を隠さず全samplesを残す。原因は確定していない。

同じprobeにscope外の条件/操作filterを加え、base mainと変更後からnormal binaryを別途保存した。両build終了後、同一入力だけを各fresh processでbefore/after/after/before比較した。C512 plainのbefore19.511/19.758→after19.529/19.472msで約2倍の遅さは再現しなかった。first-line C512はbefore24.459/24.319→after25.129/25.209msと約3%遅かった。dense4 C512はbefore1.768/1.719→after1.747/1.716ms。first-lineの構築時間増とnet保持36.5%減のトレードオフとして採用する。sparse lookupは変更行の二分探索を使うため、旧dense cellと同じアクセス費用とは主張しない。

fresh processのuser/sys/minor faults/context switchesとfirst-line C512のperfを保存した。first-line process全体のuser instructionsはbefore6.095/6.094 billion→after6.384/6.384 billionと約4.8%増え、task-clockはbefore564.53/556.03→after574.75/567.20ms、minor faultsはbefore9,069/9,070→after7,036/7,032だった。これらはfont初期化・検証用layoutを含むprocess全体で、構築scopeだけのCPU費用ではない。元の約2倍runはperfで追跡しておらず、その原因をallocator/cache/host競合と断定しない。

採用根拠はcut/cursor/limitsを保った実保持量の削減。これらの時間はこのhost・入力・scopeでの観測であり、全入力の高速化は保証しない。旧walkはcutごとに全laneを調べるままで、CPU計算量の改善とは扱わない。candidate/quick matrixの時間は検証buildと重なるため因果的な速度根拠に使わない。全layoutの時間samplesもmanifest/rawに残す。

## 検証と証拠

workspace1,069、既定featureなし616、complex-only619が成功した。fmt、全target Clippy、docs（警告error化）、固定glyph snapshotsも通過し、期待画像は更新していない。既存first-line aggregate Itemsの境界テスト、祖先/baseのbudget回帰を含む。

sbp8と同じfont/input/harness/lock/toolchain/profile/CPU affinityで全54ケースを収集した。warm/memory各7 digest、378 warm操作、108 cold process、54 memory processと最終source fingerprint b259ff581ff4dde3994ed09946fec42b5b0044ab6b1f30348384c998dd9ce00eがrunner検証を通過した。実装a956fcd12d898c8ca1a5298dc33623c55a368201、検証Rust1.96.0、性能stable1.97.1。専用probe外部依存のversionはworkspace lockと一致する。before productionはbaseのまま、ignored診断とRED回帰はrelease binaryに入らない。before source snapshotにはcapture後のcfg(test) fixture修正も含まれるが、productionは変更していない。

[manifest](data/ruby-cursor-storage.json)と[raw archive](data/ruby-cursor-storage-raw.json.gz)に全60/54ケース、割当/時間の全samples、診断、source/config/lock、6binary SHA256、RED/GREEN・required logs・native ledgerと照合scriptを含む。元成果物はtarget/performance-artifacts/sbp6-probe、sbp6-ruby-cursors、sbp6-snapshotsに保持する。
