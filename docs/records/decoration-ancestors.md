# decoration幅計算の祖先Vec削減（shodo-sbp.4）

2026年9月30日。line scanがunitごとに呼ぶdecoration幅計算で、親chainをいったんVecへ集める処理を借用iteratorへ置き換えた。内側から外側へ進む順序、Close境界では閉じるbox自身を含める規則、Cloneの選別、各edgeの丸めと逐次飽和加算は維持している。

逆順走査やmembership判定でVecを使う既存caller向けの `chain()` は同じiteratorを収集する。新しい索引・保持cacheは追加していない。祖先走査の深さに比例する費用は残る。Cloneがない場合の短絡や索引化は、このVec削減とは分けて評価する候補とし、今回の変更には含めない。

## 回帰

追加した3テストで、深さ1/16/64/256/512の全境界・start/endの幅計算が祖先Vecを作らないこと、Clone/Sliceが混在するClose境界と各辺の1/64 px丸め、符号付きedgeの内側→外側の飽和順序を確認した。旧実装では深さ1でもVec構築12回となり失敗し、意味を確認する2テストは成功した。修正後は3テストすべて成功した。

飽和テストでは3つのClone辺を2つのSliceで隔て、内側のMAX、次のMAX、外側のMINを順に加える。結果raw -1・飽和4回を期待し、順序変更やまとめて加算する実装を検出する。専用probeのstart/end総edgeは0.125/0.25 pxと異なるため、署名A/Bでは端の取り違えも検出できる。

独立コードレビューで指摘はなかった。`from_fn` は要素数を推定せず、既存Vec callerの順序と成長挙動も維持される。手元のstdソースは1.91だったため、その確認だけに依存せず、使用binaryによる全60条件の保持量・peak・出力一致も確認した。

## 専用probeのA/B

固定Latinフォント・既定limitsで、深さ1/16/64/256/512、64/4096文字、Slice/Clone/混在、通常/first-lineの60条件を検証した。負marginを含む辺を使い、100万px幅の一行をfresh contextで組む。各結果のsource範囲・glyph・line/box geometryのdigestを照合し、glyph数が文字数に等しく欠落glyphがないことも検証する。

時間と割当は別binaryで測定した。割当scopeはfresh `break_all`、context初期化・Paragraph build・出力検証はscope外。各60条件で修正前後のdigestが一致し、callsとgross bytesが減少、net保持とpeak追加は完全に同じだった。

4096文字・Slice・通常lineの例:

| 深さ | calls before → after | gross bytes before → after | net保持 bytes | peak追加 bytes |
| --- | ---: | ---: | ---: | ---: |
| 1 | 4,125 → 28 | 149,904 → 84,352 | 34,600（同じ） | 49,480（同じ） |
| 512 | 40,514 → 578 | 19,884,408 → 396,904 | 110,084（同じ） | 169,556（同じ） |

修正前の値は元のAstra診断と一致した。今回のA/BはwidthのVec削減を対象とし、他の祖先処理を省くshort circuitは入れていない。gross割当の削減を保持メモリやRSSの削減とは扱わない。

非instrumented binaryはCPU affinity 10、各条件1回の出力確認後に10 samplesを測り、before/after/after/beforeの順で実行した。最初の組ではbeforeのrun間に大きい揺れがあったため、全体検証buildの終了後に両binaryを一度ずつウォームアップし、同じ順で再測定した。再測定の4096文字・Slice・通常lineのrun medianは次のとおり。

| 深さ | before 2 runs (ms) | after 2 runs (ms) |
| --- | ---: | ---: |
| 1 | 0.526 / 0.532 | 0.912 / 0.475 |
| 16 | 0.897 / 0.957 | 1.015 / 0.564 |
| 64 | 1.529 / 1.511 | 1.456 / 0.981 |
| 256 | 6.564 / 3.715 | 3.272 / 2.494 |
| 512 | 11.882 / 7.002 | 4.778 / 4.783 |

深い条件では両after runがbeforeより短かったが、小さい深さを含めて時間に揺れが残る。時間値はこのホスト・入力・scopeでの観測として保存し、一律の改善率は主張しない。初回、再測定、ウォームアップの全出力も60条件それぞれで同じdigestだった。通常matrixのquick時間は検証buildと一部並行したため、そこから改善率を主張しない。

## 全体検証

workspace 1,060テスト、既定featureなし607テスト、complex-scriptsのみ610テストが成功した。fmt、workspace全targetのClippy（`-D warnings`）、workspaceドキュメント（警告をerror化）、固定glyph snapshot比較も成功した。期待画像は更新していない。

## 通常matrixと証拠

shodo-sbp.3の保存済み成果物をbaselineに、同じcoverage version 2・font/input/harness/lock/toolchain/profile・CPU affinity 10で全54ケースを収集した。378 warm操作、108 cold process、54 memory processがrunner検証を通過し、全ケースのwarm/memory各7 digestが修正前後で一致した。収集後のsource hashもreportと一致した。

baseは `65d3fafb0e4315db28f179cfcb8db33140d0ce12`。計測時点の変更は未コミットだった。通常のRust検証は1.96.0、性能binaryはstable 1.97.1。専用probeのdependency versionはworkspace lockと一致する。system/web fontsを含む既定featureをビルドするが、固定font loaderはsystem discoveryを無効化している。

[検証manifest](data/decoration-ancestors.json) と[生データarchive](data/decoration-ancestors-raw.json.gz) に全54 matrix、専用60条件、時間raw samples、probe source/manifest/lock、binary SHA256、実装source hash/diff、回帰・全体検証ログを保存する。元成果物は `/home/mitz/Work/oss/shodo/target/performance-artifacts/` の `sbp4-probe`、`sbp4-decoration-ancestors`、`sbp4-snapshots`。保存したbefore binaryは初期のcfg(test)回帰を追加したソースからビルドし、実行時の旧width実装は変更していない。

raikiri切り替えへの性能ブロック依存や、保存済みS4 spikeへの変更はない。
