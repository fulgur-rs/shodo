# Source-link projectionの走査と不要index（shodo-sbp.15）

実CSS/DOM callerの出力組み立ては、リンクがなくてもLineLayoutを作り、各行でparagraph全体のmapping unitsを走査していた。Identity coalesceのある単一nodeを二乗と扱わず、多node・多リンク・変換・Arabic bidi run・nowrap・部分ffi・collapsed linkを小／長入力、幅80/400の32条件に分けた。core API/limitsと既存fresh経路のインターフェースは維持する。保存S4/native geometryは変更せず、本番切り替えの条件にしない。

採用する変更は2つ。eligibleなsource linkを最初に処理するときだけ選択indexを作る。mappingは連続して使う同一identityごとに全sourceの存在を検証し、start/end両方の順序を確認した場合だけ2回のpartition_pointで候補sliceを絞る。順序が不規則なmappingは元の全走査へ戻す。mappingの一般的な順序を仮定せず、元のsource順・Collapsed処理・DOM range投影・per-unit選択queryを維持する。確認済みidentityと順序flagは局所stack状態で、新しい永続indexやheap cacheを追加しない。

単独lazy、単独mapping、両方、固定元callerの4 source statesを独立にA/Bした。全query rangeの重複は0件だったためquery memoizationは加えない。元の全source検証は、受け入れ範囲外やCollapsedのnodeも含めて行い、identity不足のエラーを隠さない。

## 時間と単独A/B

全build／108 caller tests×通常・allocation／fmt／Clippy／docs完了後、CPU10で各stateを4freshプロセス、各条件7samples。state順をoriginal/lazy/mapping/both、逆順、rotation、rotation逆順とし、case順もforward/reverse/reverse/forwardにする。以下は4 process中央値の平均。採用値は訪問counterなしのbinary。prepareは既存IFC walker/build、breakは受入行生成、projectionは実際のindex構築＋source link処理を含む。window sumはsampleごとのprepare＋break＋projectionの合計であり、parse/cascade/font登録/context作成/control/JSON/dropを除く。end-to-endやframe時間ではない。

| 条件 | state | prepare ms | break ms | projection ms | projection 元比 | window sum 元比 |
|---|---|---:|---:|---:|---:|---:|
| single-node/128/80 | original | 0.7191 | 1.4423 | 0.3668 | 1.0000 | 1.0000 |
| single-node/128/80 | lazy | 0.7021 | 1.4143 | 0.0029 | 0.0079 | 0.8391 |
| single-node/128/80 | mapping | 0.6834 | 1.4025 | 0.3465 | 0.9447 | 0.9601 |
| single-node/128/80 | combined | 0.6993 | 1.4079 | 0.0033 | 0.0091 | 0.8374 |
| many-nodes/128/80 | original | 1.3889 | 1.7035 | 0.9086 | 1.0000 | 1.0000 |
| many-nodes/128/80 | lazy | 1.3583 | 1.7076 | 0.5322 | 0.5857 | 0.8981 |
| many-nodes/128/80 | mapping | 1.2969 | 1.6757 | 0.3810 | 0.4193 | 0.8416 |
| many-nodes/128/80 | combined | 1.3691 | 1.6881 | 0.0140 | 0.0154 | 0.7694 |
| many-links/128/80 | original | 2.1261 | 1.7286 | 0.9716 | 1.0000 | 1.0000 |
| many-links/128/80 | lazy | 2.0712 | 1.7365 | 0.9627 | 0.9909 | 0.9894 |
| many-links/128/80 | mapping | 2.0716 | 1.7217 | 0.4287 | 0.4412 | 0.8798 |
| many-links/128/80 | combined | 2.0900 | 1.7261 | 0.4307 | 0.4433 | 0.8826 |
| expanded/128/80 | original | 2.2544 | 0.7032 | 2.6973 | 1.0000 | 1.0000 |
| expanded/128/80 | lazy | 2.2684 | 0.6931 | 2.6406 | 0.9790 | 0.9928 |
| expanded/128/80 | mapping | 2.2300 | 0.6975 | 0.5409 | 0.2005 | 0.6136 |
| expanded/128/80 | combined | 2.2613 | 0.7080 | 0.5426 | 0.2012 | 0.6246 |
| rtl-runs/128/80 | original | 2.3389 | 1.9742 | 0.7355 | 1.0000 | 1.0000 |
| rtl-runs/128/80 | lazy | 2.3152 | 1.9714 | 0.7256 | 0.9865 | 0.9948 |
| rtl-runs/128/80 | mapping | 2.3192 | 1.9517 | 0.3681 | 0.5005 | 0.9217 |
| rtl-runs/128/80 | combined | 2.2859 | 1.9449 | 0.3733 | 0.5075 | 0.9136 |
| nowrap/128/80 | original | 1.8610 | 0.3133 | 0.4492 | 1.0000 | 1.0000 |
| nowrap/128/80 | lazy | 1.7669 | 0.2986 | 0.4145 | 0.9229 | 0.9468 |
| nowrap/128/80 | mapping | 1.7444 | 0.2977 | 0.4195 | 0.9339 | 0.9389 |
| nowrap/128/80 | combined | 1.7722 | 0.2947 | 0.4210 | 0.9374 | 0.9469 |
| partial-glyph/128/80 | original | 4.5379 | 3.9533 | 0.2826 | 1.0000 | 1.0000 |
| partial-glyph/128/80 | lazy | 4.5834 | 3.9998 | 0.2847 | 1.0075 | 1.0092 |
| partial-glyph/128/80 | mapping | 4.5170 | 3.9427 | 0.1491 | 0.5276 | 0.9796 |
| partial-glyph/128/80 | combined | 4.5222 | 3.9839 | 0.1501 | 0.5314 | 0.9843 |
| collapsed-link/128/80 | original | 1.5259 | 1.5665 | 0.7746 | 1.0000 | 1.0000 |
| collapsed-link/128/80 | lazy | 1.7628 | 1.7008 | 0.6300 | 0.8133 | 1.0615 |
| collapsed-link/128/80 | mapping | 1.4637 | 1.5655 | 0.2536 | 0.3274 | 0.8551 |
| collapsed-link/128/80 | combined | 1.5529 | 1.5509 | 0.0141 | 0.0182 | 0.8112 |

長いexpanded/幅80でprojection比0.2012、window sum比0.6246。多node・リンクなしはprojection比0.0154、window sum比0.7694。多リンクではindexが引き続き必要で、projection比0.4433、window sum比0.8826。lazyだけではlinked条件を大きく改善しない。

小さいpartial-glyphではprojection比1.0542（幅80）／1.0514（幅400）と遅い。nowrap128/幅400も1.0037。未変更のprepare/breakにもプロセス間の時間差がある。全条件・個別中央値・不利な対照をraw/manifestに残し、全入力の速度改善や本番/frame/parallel保証にしない。retained-heightのpageごとの出力にも同じprivate関数が使われるが、その性能は今回測っておらず改善を主張しない。

## Indexとlinkの診断窓

standalone index-controlは実projectionの後に構築し、別窓で解放する。これは独立対照で、caller合計に加えたり、差し引いて実link時間を推定しない。訪問overlayは実output内部のindex構築を計時し、全projection内計時からその値を除いたlink-loop時間も保存する。これにはcounterの費用があるため、採用用の速度値には使わない。traceは実outputの訪問を対象とし、standalone controlの直接構築はtime/allocationで観測する。

| 条件 | state | unit loop visits | source lookup | 全source検証 | 順序比較 | window比較 | query | index構築 |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| single-node/128/80 | original | 128 | 128 | 0 | 0 | 0 | 0 | 1 |
| single-node/128/80 | combined | 128 | 128 | 1 | 0 | 256 | 0 | 0 |
| many-nodes/128/80 | original | 49152 | 49152 | 0 | 0 | 0 | 0 | 1 |
| many-nodes/128/80 | combined | 384 | 384 | 384 | 383 | 2560 | 0 | 0 |
| expanded/128/80 | original | 196608 | 196608 | 0 | 0 | 0 | 640 | 1 |
| expanded/128/80 | combined | 768 | 768 | 768 | 767 | 5632 | 640 | 1 |
| nowrap/128/80 | original | 256 | 256 | 0 | 0 | 0 | 128 | 1 |
| nowrap/128/80 | combined | 256 | 256 | 256 | 255 | 18 | 128 | 1 |
| partial-glyph/128/80 | original | 12672 | 12672 | 0 | 0 | 0 | 128 | 1 |
| partial-glyph/128/80 | combined | 384 | 384 | 768 | 766 | 660 | 128 | 1 |

expandedは196608→768 loop visitsだが、全source検証768、順序比較767、window比較5632も必要。この費用を消した数え方にしない。一般的なordered mappingでは全件検証＋順序確認にO(U)、各行の窓にO(log U)、受入候補処理が残る。不規則mappingは元のO(L×U)走査。first-line mappingのidentityが変われば再検証する。単一nodeは元から1unit/行で、反復全paragraph走査の一般的な二乗問題とは異なる。

## Requested allocationと保持

gross/net/peakはrequested allocator bytesでRSSではない。font登録とparse済input、context作成は窓の外。output release後の増分はprepare/break/projection/output-release netの和で、prepared/context scratchの増分を含む。context解放はprepare以前の確保も解放するためsetup netの逆数ではない。index-controlとreleaseのnet和は全sampleで0を確認した。

| 条件 | state | projection gross B | projection net B | projection peak-extra B | output release後のcaller増分 B |
|---|---|---:|---:|---:|---:|
| single-node/128/80 | original | 1148396 | 0 | 410624 | 705916 |
| single-node/128/80 | combined | 0 | 0 | 0 | 705916 |
| many-nodes/128/80 | original | 1161136 | 0 | 410624 | 1038012 |
| many-nodes/128/80 | combined | 0 | 0 | 0 | 1038012 |
| many-links/128/80 | original | 1272400 | 23552 | 431228 | 1109579 |
| many-links/128/80 | combined | 1272400 | 23552 | 431228 | 1109579 |
| expanded/128/80 | original | 1461536 | 95872 | 556791 | 841274 |
| expanded/128/80 | combined | 1461536 | 95872 | 556791 | 841274 |
| partial-glyph/1/80 | original | 2680 | 360 | 1560 | 23109 |
| partial-glyph/1/80 | combined | 2680 | 360 | 1560 | 23109 |

## 出力・証拠・検証

元callerをmerged .14からbyte-for-byte保存。全4states×32条件でsource/processed mapping、受入text、glyph ID/cluster/advance/position、paint/font/metrics/bidi、line geometry、順序付きsource linkの全snapshotがビット一致する。32条件×16最終時間プロセスの出力も一致。ffi glyph367/font32、paint ownerと部分source link nodeの相違、text1..2・link幅10.112のliteral guards、Arabic odd bidi level、nowrap1行を独立確認する。CSS direction:rtl rootは元callerが拒否する契約を維持し、full RTL-root性能を主張しない。

Resource REDは元callerのno-link index1対要求0と、expanded196608訪問対2×768units上限。単独・組み合わせのGREENを確認。重なったExpanded、Collapsed point、接触境界、empty range、start/end不規則のfallbackをliteral2testsで確認し、既存partial/nested/first-line/decoration/paint契約も維持。caller全108 testsを通常・allocationで通過、fmt、workspace all-target Clippy -D warnings、docs -D warnings成功。coreと標準harnessが.13の厳密fingerprintから変わらないので標準54 matrixは再実行せず、repository CIは別gateとする。

測定source commit `8ee399de37990ca2765ff8e21be29609bab53630`、caller fingerprint `aadca97ccbca4ce270cbc87797134becaa1e9622694cf9e0dc0b7d48e68cf3a5`。manifestとrawに固定source、全font bytes/sha/license、24 binary provenance、初期pilot全raw、最終16時間プロセス、全allocator/trace scopes、recipes、失敗logを保存。raw `source-link-projection-raw.json.gz` は15364281 bytes、SHA256 `72dc69c1ecd13a95a6890c570ba362de0e5a916c29d20bdd6d0a8af7836b1bf8`。

Clippyはtest moduleがoutput/paintより前にあった配置で一度失敗し、moduleを末尾へ移して成功した。失敗logも保存。初期index対照は実projection前にあったため、最終harnessでは後へ移し、全4statesを再構築した。初期時間値はcontrolsのみで採用表に使わない。
