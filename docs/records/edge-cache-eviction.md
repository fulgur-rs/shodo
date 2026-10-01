# EdgeShapeCache 部分退避の評価

固定ハッシュ順で必要な件数だけ退避する候補は不採用。全消去の費用原因は確認できたが、候補は局所性を損ない、長い入力の再shaping・割当・時間を増やした。既存の全消去方針と256 entries／32768 glyph-equivalent cost／1024 single-window costを維持する。切り替えのブロックは追加しない。

## 原因と比較方法

変更前にcache hit/miss、空cacheの初期化、保持中のowner変更、entry/cost容量消去、oversize、replacement、警告・飽和非cache、layout/metricsの実shaperを観測した。イベントの全sequenceを再生し、消去後に失われたkeyが再missになることを確認した。271条件の費用区間ではentry上限による全消去441回、cost上限による消去0回、全消去で失ったkeyへの再miss80,725回。これは今回の固定入力群の集計であり、以前のcaller記録のrawを再測定した結果ではない。

候補は既存keyの費用を差し引き、hash iterator先頭から必要な分だけ除く。recency metadataやhit時の更新は足していない。上限を増やしていない。owner invalidation、oversize/replacement/warning/saturationの非cache経路、hitでもreshape要求をchargeする処理は同じ。4つの新規テストは旧方針でRED→候補でGREEN。候補のcore444件、既存予算・警告・owner/sharingテスト、fmt/clippy/docs/allocator checksを通した。

同一固定フォント・入力の6ビルド（各状態の通常／requested memory／observer）で271条件の完全なLine/source mapping/glyph/font/geometry/Ruby/警告を照合した。cold/warm、幅400→80→160→400、owner切り替え、反復locality、churn、first-line、Ruby、float、block、forced、atomics、variable/missing-font、disabled/budget controlsを含む。coldはLayoutContext/edge cacheが空であり、fontまで完全coldという意味ではない。各caseの実FontCollectionにもshaper-zero limitsを適用する。

4 scopeは準備・layout・出力解放・context解放。observerは計測前にtrace領域を予約し、通常memory buildとcalls/gross/freed/net/peakが一致した。警告drain、snapshot、JSONはlayout scopeの外。別の117条件で完全なLineResult events・BreakPlan・必要高さ・警告と7 scopeを確認し、既定54条件×7操作の厳密digestも基準mainと一致した。ここでLine-only probeのfloat/block進行検査と、117条件の完全なevent検査を区別する。

チェックと出力・所有権検証の完了後に、11 layout query＋3 plan queryを各4つのfresh process、ABBA順、7 samples／2 warmupsでcounter-free比較した。全56プロセスの出力を再照合した。別プロセスではopaque FontId layerの生成番号のみを正規化し、同一／異なるcollectionの関係とface index・font bytesは保持する。`small-set`は1つのffi入力を指し、実cacheが1件という意味ではない。実際の1-key制御とoversizeは独立probeで確認した。

## 集計結果

時間はprocess内sample中央値の、各状態2 processの中央値。比率は候補／変更前。requested bytesはscope内の要求量で、RSSではない。全40条件の集計と悪化も [manifest](data/edge-cache-eviction.json) に残す。

| 条件 | 時間比 | 実layout shaper 変更前→候補 | gross bytes 変更前→候補 | net bytes 変更前→候補 | peak extra bytes 変更前→候補 |
|---|---:|---:|---:|---:|---:|
| standard/latin-long/64/warm | 3.3968 | 2694 → 23574 | 27098234 → 71188339 | 1320022 → 1394362 | 2106454 → 2180794 |
| standard/arabic-long/64/warm | 2.0895 | 6070 → 13922 | 76136610 → 148076569 | 2192550 → 2303273 | 2585766 → 2696489 |
| rich/links/128/8/default/width/cold | 1.2263 | 4475 → 5638 | 7769209 → 8942622 | 507163 → 639398 | 703771 → 836006 |
| rich/links/128/80/default/width/warm | 0.9288 | 1594 → 1576 | 2383424 → 2342999 | 10754 → 31744 | 50239 → 44032 |
| rich/links/512/8/default/width/locality | 1.3026 | 17945 → 23956 | 30947929 → 37014931 | 1943146 → 1922560 | 2729578 → 2708992 |
| rich/small-set/1/8/default/width/warm | 1.0224 | 16 → 16 | 38195 → 38195 | 3584 → 3584 | 5266 → 5266 |
| rich/edge/1/8/edge-zero/width/cold | 1.0150 | 0 → 0 | 43897 → 43897 | 13065 → 13065 | 25353 → 25353 |
| plain/32/80/default/Balance | 0.9986 | plan control | 5249678 → 5249678 | 119530 → 119530 | 122485 → 122485 |

271条件合計のmissは116,850→237,256、layout実shaperは240,550→360,956。hash-order退避は再利用するwindowも除き、個別sequenceで退避後の再missを確認した。短い無圧力の対照や一部幅では小さな改善もあり、普遍的な悪化率には外挿しない。

独立1-key probeは100回の同じ実shape要求後に1 entry／cost5。2048 glyphの実Arabic windowは2回とも非保持。churn終了時は32 entries／cost183→256 entries／cost1494で、HashMap::capacity()は448→318、mapの実解放量は両状態で21,008 bytes。windowおよび依存RunInstance・保持用Vecの実解放は17,503→140,774 bytes。これはglyph-vectorだけの量ではない。edge cacheと分離したwindowを解放した後、single/oversizeでは段落をdropするとrootは解放され、通常churnでは別のgeometry cacheがrootをcontext dropまで保持する。両状態でcontext解放後にrootは消える。

## 判断と公開範囲

固定ハッシュ順の部分退避は、この予算内では時間・再shaping・保持費用の総合利益を示せなかった。候補の本体と候補専用テストを戻し、最終runtimeは基準mainと同一。LRU/CLOCKは未評価であり、この結果からそれらの効果を断定しない。キャッシュ容量拡大やRSS・一般的なCPU改善の保証はしない。

raw証跡、host情報、binary、producer log、各processの報告・source/font bundleはローカル保管し、公開はコードと匿名集計のみ。公開資料だけでは原報告を復元できない。元診断と保存済みspikeは変更していない。
