# t6q.6: decoration幅のClone祖先へのjump

採用。各InlineBoxInfoに、そのbox自身を含めた最も内側のClone祖先を保持する。Paragraph build時に親→子の一回の走査で設定し、widthは境界のboxからCloneだけを内側→外側へ辿る。Closeでは閉じるbox自身、その他はunit.parent_boxから始める。辺ごとの丸め・符号付き飽和加算を続け、幅の事前合算は行わない。chainのVecを使う既存callerは変更しない。

通常・first-lineのbuild_dataがそれぞれ解決後のstyles/boxesから索引を作る。box-decoration-breakはfirst-line適用プロパティに含まれないため、callerの異なるalternate指定を直接採用しない。公開API、既定limits、source/glyph/geometry、警告・資源fallback、別ラインのruby geometryは変更しない。sbp.4の祖先Vec除去を再計上せず、その後も残っていた全祖先走査だけを対象にする。

Base `6aace4f01d20c02fd0eb61acc4abd9485d71c6da`、Rust1.96.0、x86_64 Linux、release、固定Latin fixture（FONTS[0]のchecksum、system discoveryなし）。深さ1/64/256×Slice/Clone/交互混在の9入力と深さ64のfirst-line3入力、各1024 ASCII文字の計12条件。幅100万pxの一行をfresh contextで組む。probeはdev/bench/examples/decoration_width.rsで既存geometry_snapshot helperを再利用する。font binaryのDebug展開はしない。

時間とCountingAllocatorは別feature binary。buildは事前に作ったbuilderを消費するbuildのみ（入力/font/context作成はscope外）、Paragraphとcontextを保持して終了する。scope外で作った入力のbuild中解放はdeallocated/netに含む。layoutは既存Paragraphにfresh contextでbreak_allするscopeで、返却line/contextを保持して終了する。両scopeともcounts/elapsedをJSON前にlocalへ確定し、snapshot/hash/警告収集/JSONはscope外。buildで増えたParagraph保持量はlayout scope外なので、layoutのnetが不変でも全体保持量が不変とは扱わない。

共通flock、nice10、jobs1、RUST_TEST_THREADS2、専用target t6q-build/t6q-6を使用。時間は保存binaryのABBA順、各process11標本で22標本/側/条件。allocatorは別binary11標本/側/条件のfield中央値。fontsはprocess内共有、contextは操作ごと新規。初回font cache変動を含む全rawを保存した。

| 深さ | 入力 | first-line | layout median µs 前→後 | 後/前 |
|---:|---|:---:|---:|---:|
|1|Slice|なし|126.105→126.280|1.001|
|1|Clone|なし|130.296→131.832|1.012|
|1|混在|なし|128.829→132.670|1.030|
|64|Slice|なし|273.128→144.963|0.531|
|64|Clone|なし|405.972→410.756|1.012|
|64|混在|なし|310.215→286.259|0.923|
|64|Slice|あり|272.918→146.290|0.536|
|64|Clone|あり|401.851→419.661|1.044|
|64|混在|あり|312.346→284.198|0.910|
|256|Slice|なし|778.069→198.010|0.254|
|256|Clone|なし|1397.906→1427.939|1.021|
|256|混在|なし|957.535→834.643|0.872|

深いSlice/混在は短くなったが、Cloneのみ・浅い入力では小さい悪化を観測した。共有高負荷ホストの時間値であり、一律改善やアプリ全体の改善率を主張しない。build時間中央値は通常0.46–0.54ms、first-line約1.02msで前後差は約±1%の範囲だった。

新しい索引Vecは作らず既存box VecへOption<u32>を埋め込む。このtargetではVec capacityあたり8B保持量を増やす。深さ1のcapacity4では32B、64で512B、256で2048B、first-line64の2setで1024B増える。buildのcallsは全条件で不変、grossはVec成長過程を含めて増加する。Sliceの代表値は以下で、同じ深さ/first-lineのClone/混在も増分は同じ。

| 深さ / first-line | Calls（不変） | Build gross B 前→後 | Build net B 前→後 | Build peak extra B 前→後 |
|---|---:|---:|---:|---:|
|1 / なし|268|710278→710310|320628→320660|326514→326546|
|64 / なし|305|794503→795495|342919→343431|358170→358682|
|64 / あり|616|1619086→1621070|700070→701094|724646→725670|
|256 / なし|322|1057239→1061303|414999→417047|455706→457754|

layoutのcalls/gross/net/peakは全12条件で前後同じ。Sliceの深さ1は25 calls/22796 gross B/10024 net B/12648 peak B、深さ64は112/57500/19204/25364、深さ256は314/164316/47620/76628。今回の効果をallocation削減とは扱わない。allocator値はRust requested block量で、RSSやstack、native allocator管理領域は含まない。

性能回帰は実width iteratorの訪問をcfg(test)で計数する。旧widthはSliceの深さ16でactual16/expected0となりRED。GREENでは深さ16/64のSlice訪問0、Clone訪問16/64、交互混在8/32、深さ64でClone一つなら1を要求する。Clusterと最内Close、start/end双方を確認し、幅rawと飽和cleanも検証する。元の全祖先iteratorは各queryで深さDを訪問してからfilterしていたため、処理をO(D)からO(Clone祖先数)にする。全Cloneでは次数は変わらない。

既存の全境界・深さ512までの祖先Vec非構築、Close自身/各辺の丸め、内側MAX→MAX→外側MINのraw -1/飽和4回は維持。新しいfirst-line解決後styleとSibling間でClone祖先を漏らさない回帰も通過した。

検証: baseline decoration3 pass、REDは上記の実訪問数で失敗、GREENの対象5 pass、Sibling追加後の最終crate suiteでdecoration6件を含む795 pass/既存診断ignore2（lib469 pass）。関連harness first_line/horizontal_contracts/intrinsic_budget/line_metricsは27+14+3+8=52 pass、bench allocator5 pass、fmtとshodo/harness/bench全target clippy `-D warnings`もexit0。最終checksはcommandごとに共通lockを取得・解放した。初回probeの未存在cx.warnings APIでのcompile errorはtake_warningsへ修正し、修正前後とも正常に保存binaryを再構築して測定した。

全time/allocator反復でpublic source/glyph/geometry hash、build/layout warnings（空）、既定limitsでの受理とmax_shaped_glyphs64の拒否（actual1024）が前後一致。採用根拠は小さいruntime差分、幅の演算順を保ったままの確実な訪問削減、深いSlice/混在での観測改善。保持量増と全Clone/浅い入力の小悪化を交換条件として明示し、raikiri/S4必須依存にしない。

再現: 同じexampleをbase/candidateに配置しrelease通常とallocation-countingのbinaryを保存して交互実行する。Rust wrapperは空、runner=env、同じtoolchain/target/flock設定を使う。

```sh
cargo build --offline --locked --release -p shodo-bench --example decoration_width
cargo build --offline --locked --release -p shodo-bench --example decoration_width --features allocation-counting
```

Raw/binary/scripts/summary/logはこのworktreeのtarget/performance-artifacts/t6q-6/。cleanup前に必要なrawをcontrollerで退避する。
