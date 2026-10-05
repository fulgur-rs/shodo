# Shaping窓のscalar・metadata借用（shodo-sbp.7）

2026年9月30日。shape_items_with_base_scopesは、budgetで区切った各窓で一時ShapeItemを作り、scalar sliceをVecへコピーし、FontMatchとbefore/after Stringもcloneしていた。選択scalar sliceを借用し、窓の終端だけwindow_endに保持する。font・orientation・level・scriptは元itemを参照する。

窓の最終cluster終端にはoriginal.endではなく元のlast scalar.endを使う。元のShapeItemsは引き続きParagraphDataが保持する。窓のbefore/after cloneは読まれておらず、既存のoriginal/近傍scalarによるpre/post context生成は維持する。別途source editを行うshape_window_editのowned入力、cursor/grapheme/penの分割、ruby base scopeのbudget・glyph加算は変更していない。

## 回帰と全体検証

cfg(test)でScalar::cloneそのものを計測し、productionでは従来どおりderive Cloneを使う。入力作成後にcounterをリセットして実shaperを呼ぶテストを追加した。旧実装はmissing-font・1文字窓で32scalarをcloneし、ゼロ期待に対して失敗する。借用後はmissing/実フォント、budget1/1024の全4条件で0。glyph ID/cluster/advance/pen、runのsource範囲とowner、分割run数・警告、元scalarのpointer/length/endも確認する。

shaping関連21テスト、workspace1,065、既定featureなし612、complex-scriptsのみ615テストが成功した。既存のgiant grapheme/pen分割、variable/optical sizing、ruby base limits、RTL、first-line/edge reshapeも対象に含む。fmt、workspace全targetのClippyとdocs（警告をerror化）、固定glyph snapshotsも成功し、期待画像を更新していない。

## 専用build A/B

固定Latin/CJK/Arabic fonts・既定limitsで、Latin短/長、Arabic長、mixed scripts、combining Latin、many-short Latinのscale1/64を測る。さらにfirst-line、run budget8のLatin/RTL Arabic、budget1のgiant graphemeを使い、計16条件。budget条件だけmax_shaping_run_bytesを明示し、他のlimitsは既定値とする。

scopeはfresh contextのWorkload::buildまたはParagraphBuilderのinput作成・shaping。font loadとcontext初期化、出力・警告・source mappingの照合はscope外。fontsのcacheは期待出力作成でウォームアップする。時間と割当は別binaryで測り、割当3回・非instrumented時間10 samplesを保存した。

16条件すべてでbefore/afterのglyph/source/geometry digest、正確なParagraph警告列、DOM mapping unit列が一致した。すべてcalls/grossが減少し、net保持とpeak追加は同じだった。代表例（bytes）:

| 条件 | calls before → after | gross before → after | net保持（同じ） | peak追加（同じ） |
| --- | ---: | ---: | ---: | ---: |
| latin-long/64 | 61,795 → 61,794 | 48,216,762 → 46,989,242 | 18,963,738 | 19,483,129 |
| arabic-long/64 | 55,675 → 55,673 | 43,286,527 → 42,320,127 | 16,379,516 | 16,716,892 |
| mixed-scripts/64 | 11,306 → 9,388 | 2,647,991 → 2,584,649 | 1,106,830 | 1,119,618 |
| first-line/1 | 3,134 → 3,132 | 2,357,877 → 2,306,677 | 894,945 | 923,285 |
| budget8/1 | 2,359 → 2,199 | 1,059,049 → 1,033,449 | 401,839 | 407,129 |
| rtl-budget8/1 | 5,475 → 5,027 | 1,413,262 → 1,377,422 | 518,513 | 534,044 |
| giant-grapheme/1 | 2,537 → 2,281 | 290,589 → 285,469 | 105,839 | 111,769 |

元データ保持を削っていないため、net保持は減らない。測定scope全体のpeakにも差は出なかった。gross割当の減少を保持量やRSS削減とは扱わない。RSSは計測していない。

全検証buildと通常matrixの完了後、CPU affinity10で両normal binaryをウォームアップし、before/after/after/beforeの順で計測した。各runの10 samplesのmedian（ms）:

| 条件 | before 2 runs | after 2 runs |
| --- | ---: | ---: |
| latin-long/64 | 33.026 / 33.008 | 35.695 / 35.737 |
| arabic-long/64 | 32.741 / 33.054 | 35.434 / 36.316 |
| mixed-scripts/64 | 2.558 / 2.577 | 2.528 / 2.521 |
| first-line/1 | 1.217 / 1.224 | 1.222 / 1.226 |
| budget8/1 | 0.738 / 0.740 | 0.729 / 0.737 |
| rtl-budget8/1 | 1.629 / 1.640 | 1.621 / 1.639 |
| giant-grapheme/1 | 0.365 / 0.362 | 0.359 / 0.358 |

**既定allocatorでこの16入力を連続実行した際、Latin/Arabic長入力のbuildは借用化後に約8〜10%遅くなった。割当削減を一律の速度改善とは扱わない。** この回帰を調べるため、同じimmutable binaryのperf測定とallocator閾値の反実仮想、同一filterを追加した両実装の入力別fresh-process比較を行った。

perf whole-processではinstructionsが27.925B→27.861B、user時間2.209→2.202秒とほぼ同じ一方、sys時間0.019→0.073秒、cache miss36.667M→40.573M。別のfault測定はminor faults11,559→39,381、major faultsは両方0だった。MALLOC_MMAP_THRESHOLD_だけを128KiBまたは2MiBに固定すると、相対的な遅さは消えた（全体のfault数と時間は既定設定より増えた）。これは診断用であり、アプリのallocator設定は変更していない。

入力ごとに新規processで既定allocator・CPU10・同じbefore/after/after/before順、各10 samplesのnormal medianを比較すると、Latin-long/64はbefore35.851/35.732ms→after32.901/33.390ms、Arabic-long/64は35.066/35.874ms→34.978/35.005msだった。別途perf付きisolated比較でも連続入力時の逆転は再現せず、出力・警告・mappingはすべて一致した。

一時的な大きいscalar割当を除いたことでallocator履歴とページ再利用が変わるという説明を強く支持するが、glibcの内部動的閾値そのものはtraceしておらず、厳密な因果確定とはしない。今回は窓のコピーを除く改善として採用し、連続入力時の長build遅延を制約として残す。allocatorのprime用ダミー割当やglobal設定変更は加えない。今後の比較でもprocess内の入力順・履歴を区別する必要がある。

時間はこのホスト・入力・scopeでの観測であり、全入力への速度改善率は保証しない。通常matrixのquick時間は検証buildと並行しており、改善率の根拠にはしない。

## 通常matrixと証拠

sbp5の同じfont/input/harness/lock/toolchain/profile/CPU affinityをbaselineに全54ケースを収集した。378 warm操作・108 cold process・54 memory processがrunner検証を通過し、各ケースのwarm/memory各7 digestが一致した。収集後のsource fingerprintも一致した。

baseは89ae7f955aaa7f8795bc246a7e3f3064a459ced3、実装commit37a5758。検証Rustは1.96.0、性能binaryはstable1.97.1。専用probeのexternal dependency versionはworkspace lockと一致する。固定font loaderはsystem discoveryを無効化し、original binaryはtest-only clone計測を追加した未変更production実装から作って保存した。

検証manifestとraw archiveに16条件と54 matrix、全時間samples、正確な警告・mapping列、旧・filtered probe source/manifest/lock、全binary SHA256、perf/fault・allocator反実仮想とisolated samples、実装source hash/patch、RED/GREEN・全検証ログを保存する。元成果物はtarget/performance-artifactsのsbp7-probe、sbp7-borrowed-windows、sbp7-snapshots。

raikiri切り替えを止める性能依存は追加していない。保存済みS4 spikeへの変更はない。
