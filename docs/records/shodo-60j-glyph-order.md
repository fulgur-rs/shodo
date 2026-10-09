# shodo-60j: 整列済みglyphのindex Vecを省略

採用。先頭不可視文字の補正後のclusterが非減少なら、shaperの順序を直接使う。逆転がある場合だけ従来のindex Vecとstable sortを使う。同値cluster内順序、source所有範囲、RTL、pen分割、CFF縦位置補正、警告、limitsを維持する。キャッシュや保持容量は増やさない。

速度改善は未確定。採用根拠は整列済みwindowでの確保・書き込みの削減、3入力のCallgrind命令数削減と出力等価性。短時間測定ではCJKが約3.5%遅く、長時間の反復では方向が逆転した。共有ホストの周波数・SMT・他プロセスの影響を制御できておらず、退行がないとも断定しない。

## 実装と比較条件

基準commitは`5db3a90233ef30115f4992a77ff0574698b44b60`（#268後）。フィードバックの`51a196b`以降の差分はline/align.rsのみで、対象処理は同じ。初期案は各glyph参照でOptionを選んだが、CPU固定の短時間測定でCJKとpen probeが約5%遅かった。最終案はwindowごとに直接index／配列indexのgeneric callbackを選び、同じ出力処理を`shape/window.rs`で共有する。glyphごとのOption選択を避ける構造上の変更であり、時間退行の原因がこれだけだったとは確認できていない。

2026-10-09、AMD Ryzen 5 5600G、x86_64 Linux 7.2.5-3-omarchy、rustc/cargo 1.91.0、LLVM 21.1.2。release opt-level=3、debug=0、incremental=false、kache wrapper。同じ拡張済み`shape_window_prep`を実装前後で使い、実行ファイルを各build後に保存。時間計測と`allocation-counting`計測は別実行ファイル。complex-scripts有効、固定Latin/CJK/Arabicフォント、system discovery無効。compiler詳細、ソース・実行ファイル・font・Cargo.lockのSHA-256と結果は[JSON記録](shodo-60j-glyph-order.json)に保存。

入力はprobeの文字列を64回繰り返す。singleはwindow上限なし、split16は16 bytes。ruby baseは6 bytesまたはglyph limit=4。縦書きCJKはVORGありと同じfontからVORGだけを除いたもの。leading-ignorablesはZWSP・soft hyphen、ignorables-onlyはZWSP・soft hyphen・word joiner。Arabicは非昇順の対照、marksは結合文字付き。latin-pen-splitは巨大font sizeによる飽和probeであり、巨大な単一clusterの実分割は既存unit fixtureで別途検証する。

## 確保と出力

builder準備とfont cache warm-upは計測外。fresh LayoutContextでbuildし、Paragraphを保持した状態でallocator scopeを終了。数値はallocatorへの要求量であり、RSSではない。

| 入力 | 描画glyph数 | Calls: 前→後 | Gross bytes差: 後−前 |
|---|---:|---:|---:|
| latin-single | 2880 | 297 → 295 | -46,080 |
| latin-split16 | 2880 | 465 → 285 | -23,040 |
| arabic-single | 1152 | 293 → 293 | +0 |
| arabic-split16 | 1152 | 425 → 425 | +0 |
| cjk-upright-vorg-single | 1024 | 265 → 263 | -16,384 |
| cjk-upright-vorg-split16 | 1024 | 460 → 255 | -8,192 |
| ruby-base-split6 | 384 | 15174 → 14854 | -7,680 |
| ruby-base-glyph-limit | 0 | 1573 → 1571 | -32 |
| cjk-horizontal-single | 1024 | 261 → 259 | -16,384 |
| cjk-horizontal-split16 | 1024 | 456 → 251 | -8,192 |
| combining-single | 256 | 229 → 228 | -2,048 |
| combining-split16 | 256 | 261 → 222 | -2,048 |
| leading-ignorables-single | 192 | 1431 → 1367 | -2,040 |
| leading-ignorables-split16 | 192 | 868 → 804 | -1,792 |
| ignorables-only | 0 | 174 → 174 | +0 |
| arabic-marks-single | 576 | 269 → 269 | +0 |
| arabic-marks-split16 | 576 | 338 → 338 | +0 |
| latin-pen-split | 256 | 225 → 224 | -2,048 |
| cjk-upright-novorg-single | 1024 | 275 → 273 | -16,384 |
| cjk-upright-novorg-split16 | 1024 | 470 → 265 | -8,192 |

全20ケースでnet bytesとpeak extra bytesは前後一致。長い整列済みwindowではindex Vecに加えstable sortのscratchも不要になるため、2回の確保が減る場合がある。未整列Arabicでは確保は減らず、早期終了する非減少判定が追加される。glyph-limit入力は期待された`ShapedGlyphs limit=4 actual=5`のエラーであり、正常buildの速度改善として扱わない。

全20ケースのdigest、glyph数、警告・limitエラーが一致。digestはglyph ID/cluster/位置/advance、text範囲、line/fragment geometryとfont bytesを含む。任意のDOM/source所有契約全体の証明ではなく、sourceやlimitsは既存harnessとunit testでも検証した。leading-ignorables-split16のedge reshape予算警告、巨大font sizeのSaturated警告も一致。

## 時間

短時間測定は各入力を独立プロセスで実行し、CPU 2へ固定。各プロセスのbuild 21回の中央値を使う（builder準備とParagraph破棄は時間外）。ABBAを6 round、BAABを6 round行い、各roundのB 2回の平均／A 2回の平均の中央値を下表に示す。A=基準、B=最終案。1未満が短縮。

| 入力 | 後/前の中央値 | 短縮round | Round比の範囲 |
|---|---:|---:|---:|
| latin-single | 1.0250 | 4/12 | 0.779–1.273 |
| cjk-horizontal-single | 1.0352 | 0/12 | 1.004–1.456 |
| arabic-single | 1.0168 | 4/12 | 0.770–1.313 |
| arabic-split16 | 0.9807 | 7/12 | 0.797–1.048 |
| arabic-marks-single | 0.9718 | 9/12 | 0.797–1.284 |
| latin-pen-split | 1.0261 | 3/12 | 0.874–1.094 |

CJKは12 roundすべて遅く、短時間測定の退行を無視しない。一方、長時間確認では`loop CASE 5000`をCPU 2へ固定し、ABBA/BAABを交互に6 round、perf statのinstructions/cycles/branches/branch-missesも取得した。以下の時間はプロセス起動・font準備・builder生成・Paragraph破棄を含む全wall timeであり、上のbuild単体時間とはscopeが異なる。perf起動と観測も含む。

| 入力 | Wall time後/前の中央値 | Round比の範囲 | Instructions後/前 | Cycles後/前 |
|---|---:|---:|---:|---:|
| cjk-horizontal-single | 0.9334 | 0.765–1.109 | 0.9928 | 0.9789 |
| latin-single | 1.0129 | 0.983–1.055 | 0.9905 | 1.0212 |
| arabic-single | 1.0078 | 0.992–1.017 | 0.9985 | 1.0051 |

命令数・cycleは各側12 processの中央値の比。短時間と長時間で方向が一致せず、速度の一般化や無退行の主張には不十分。短命確保の確実な削減として採用し、速度が重要な用途では静かな環境と対象corpusで再測定する。最初の実装の20入力探索値も方向が混在しており、最終案の速度根拠には使わない。

## Callgrind命令数

追加確認としてValgrind 3.25.1のCallgrindのIrを比較。`loop CASE 20`を各側1 process、`--collect-atstart=no --toggle-collect="*ParagraphBuilder*build*"`によりbuildとその子関数だけ収集した。font load、builder生成、Paragraph破棄は収集外。最初のbuildによるcache warm-upは含む。shape_inputsは`callgrind_annotate --inclusive=yes --threshold=100 --auto=no`で子関数込みのIrを取得。cache/branch simulationは無効。

| 入力 | Build Ir後/前 | Shape Ir後/前 | Build Ir: 前→後 | Shape Ir: 前→後 |
|---|---:|---:|---:|---:|
| latin-single | 0.9914 | 0.9601 | 398,399,165 → 394,966,042 | 113,395,552 → 108,873,239 |
| cjk-horizontal-single | 0.9915 | 0.9311 | 168,202,871 → 166,766,023 | 24,522,287 → 22,833,092 |
| arabic-single | 0.9988 | 0.9858 | 211,356,479 → 211,106,368 | 63,011,672 → 62,116,348 |

3入力ともbuild命令数は増えず、整列済みCJKのshape命令数は約6.9%減少した。未整列Arabicでも最終案全体のshape命令数は約1.4%減少しているが、判定走査そのものが無料という意味ではない。抽出・generic化によるcodegenも変わる。Irは計装下で数えた命令数であり、cache missや周波数、実時間改善の証明ではない。

## 回帰検証

新規テストは補正済み非減少・同値・補正による逆転消失・補正後のstable ties・空／単一glyphを確認する。実fontのLatin/CJK横／縦、single／split windowでorder Vec生成回数0を検証。旧実装ではLatin singleが1回となってRED、最終案でGREEN。既存のRTL、pen budgetのLTR/RTL分割、負のadvance、glyph-free prefix、CFF、limits関連テストも通過。

最終版の検証はすべてexit 0。dev profileはCI同様opt-level=1、debug=line-tables-only。以下はdoc testsを含む集計。

| 検証 | 結果 |
|---|---|
| workspace | 1719 passed / 9 ignored |
| accesskit | 1735 passed / 9 ignored |
| no-default | 1184 passed / 9 ignored |
| no-default-complex | 1187 passed / 9 ignored |
| release-shape | 49 passed / 2 ignored |

fmt、workspace/all-targets clippy（通常とaccesskit、-D warnings）、strict docs（通常とaccesskit）、固定glyph snapshotsも成功。初期案で通った結果を最終案の成功として流用していない。独立レビューでも出力処理の移動、入力全体のCFF cache共有、stable順序を確認。

## 再現

同じcompiler/profile/fixtureを使い、実装前後のprobeを別名で保存する。基準checkoutにもこのcommitの`dev/bench/examples/shape_window_prep.rs`だけをコピーし、同じ20入力で比較する。例（`BASELINE`と`CANDIDATE`はそれぞれの保存済み実行ファイル）:

```sh
mkdir -p "$HOME/tmp"
MEASURE_DIR=$(mktemp -d -p "$HOME/tmp" shodo-60j.XXXXXX)
export TMPDIR="$MEASURE_DIR" CARGO_TARGET_DIR="$MEASURE_DIR/target"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
cargo +1.91.0 build --offline --release -p shodo-bench --example shape_window_prep --features complex-scripts
# $CARGO_TARGET_DIR/release/examples/shape_window_prepを保存してからallocator版をbuildする
cargo +1.91.0 build --offline --release -p shodo-bench --example shape_window_prep --features complex-scripts,allocation-counting
# 保存したtime版: digest / time CASE、allocator版: alloc
# time CASEをA B B A / B A A Bで計12 round、各processが21 samplesを出力する
perf stat -e instructions,cycles,branches,branch-misses -- taskset -c 2 "$BASELINE" loop cjk-horizontal-single 5000
perf stat -e instructions,cycles,branches,branch-misses -- taskset -c 2 "$CANDIDATE" loop cjk-horizontal-single 5000
valgrind --tool=callgrind --collect-atstart=no --toggle-collect='*ParagraphBuilder*build*' --callgrind-out-file="$MEASURE_DIR/callgrind.out" "$CANDIDATE" loop cjk-horizontal-single 20
callgrind_annotate --inclusive=yes --threshold=100 --auto=no "$MEASURE_DIR/callgrind.out"
```

結果・条件・判断・再現手順をこの記録とJSONに残し、測定用実行ファイル、target、生ログ、使い捨てworktreeは作業完了時に削除する。
