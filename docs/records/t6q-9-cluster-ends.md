# build_units の cluster 終端 scratch を省く

issue shodo-t6q.9。base `9a3c3d84d39d2471a10b4cbb3ddae365e9e7006b`（shodo-t6q.2を含む）。各 ShapedRun に glyph 数 G の u32 Vec を確保・初期化・後方充填してから前方で読む処理を、同一 cluster の glyph 群の先読みに置き換える。次 group の cluster start、最後の group では run.text.end を終端にする。unit/grapheme の統合は従来の glyph 走査を残した。計算量は O(G) のまま、4G bytes の一時配列と run ごとの確保を除く。

昇順契約は shape.rs の全 GlyphStore.cluster push 経路を確認した。Harfrust 出力は stable sort_by_key で cluster を論理昇順にし group ごとに store へ追加する。RTL pen-budget storage split の逆順化は同一 cluster 内の parts に限られる。missing-font shape_item は char_indices の byte offset 昇順で追加する。shape_items/with_base_scopes と shape_window/edge 系は同じ shape_inputs/shape_item を使用し、cache はそれを保存する。build_data の build_units はこの output を直接使う。unit の shaping flags/break/min-content、grapheme 統合、storage split、mapping、limits、warning は変更しない。以前の window 借用/parts clone 削減とは別の局所変更。

## RED/GREEN と回帰

baseline shape unit19、harness shaping/line_shaping57 pass。新規実 paragraph fixture: Latin ab は unit range 0..1/1..2、結合 a+acute+b は0..3/3..4、Arabic lam-alef は0..4、空の system-fonts=false collection の missing-font a+acute+b は0..3/3..4。期待は literal で、run内 cluster nondecreasing も確認する。

一時 cfg(test) observer を旧 Vec 確保 site に付け実 scratch slice bytes を記録した。範囲/order assertionsは通過したが、Latin8/結合8/Arabic4/missing12 bytes（期待0）でRED。先読み後は0でGREEN1。実行siteが消えると counter の assertion は永続回帰として弱いため、最終から observer/counter/0要求を外した。この性能根拠はRED/GREEN logと独立 allocator A/Bへ残す。最終unitテストは範囲/orderの意味的契約を検証する。既存の巨大単一cluster、RTL storage split、mark attachment、giant grapheme/run bytesとbreak回帰を併せて実行する。

## A/B 条件

小さな `dev/bench/examples/cluster_end_cost.rs`。固定 Latin/CJK/Arabic fixture font、system discovery無効、空collectionのmissing fallback、既定limits（windowedだけmax_shaping_run_bytes=32）。8入力: Latin1024bytes、CJK、結合文字、Arabic RTL、base+1024 combining marksの巨大cluster、多数のsize違いrun、missing-font、32byte shaping windows。warmup3、time9/alloc3。小さな24scalar oracleを測定入力と分け、幅48/160/1024でlayoutする。巨大snapshotやfont blob Debugは生成しない。

prebuilt builder を消費する build だけを scope とし、fonts/builder生成、line layout、snapshot/JSON、Paragraph dropを外に置く。Paragraph/contextはscope終了時に保持する。時間は counter-free release executable、allocationは別 feature build。grossは累計 requested bytes、netは開始/終了requested live差、peakは開始liveを超えた最大requested live。RSSとは異なる。

測定入力 text/mapping/warnings、小入力 source/glyph ID・cluster・advance・origin・geometry float bits・font ID・paint/variation/transform・TCY/overflow・break reason/warnings、glyph limit0/1/8のsuccess/errorを前後で照合する。前後time/alloc＋保存binaryのABBA4実行、計8出力契約は完全一致。runtime/fixture/probeのsource manifestはunits.rs以外同一。
| 入力 | calls 前→後 | gross bytes 前→後 | time ABBA中央値 µs 前→後 |
| --- | ---: | ---: | ---: |
| latin | 242→241 | 622892→618796 | 505.4→513.5 |
| cjk | 1436→1308 | 303496→302216 | 278.5→279.9 |
| combining | 218→217 | 199980→198700 | 164.2→163.5 |
| rtl | 2406→2150 | 422670→421134 | 660.1→662.0 |
| giant | 203→202 | 400974→396870 | 574.9→570.8 |
| many-runs | 1162→1034 | 414687→413663 | 329.4→331.9 |
| missing | 2203→2075 | 425688→423896 | 237.2→237.2 |
| windowed | 1755→1563 | 397048→395256 | 477.8→483.1 |

全8入力のnetは同じ。peakもgiant以外同じ、giantは163186→159085 bytes。Latinのgross4096B、giant4104B、多数run1024Bなど、glyph数×4Bの累計scratchを除いた。保持メモリやRSS改善は主張しない。ABBAはbefore→after→after→before、各18標本。初回afterのLatin678.8µs等の遅延は対称順序では大幅差として再現せず、最終時間差は小さく方向が混在する。時間改善を採用根拠にしない。全値・min/max・gross/freed/net/peak・executable/source hashは隣接summary JSON。

Rust1.96 direct cargo、offline、jobs1/RUST_TEST_THREADS2、nice10、専用target=t6q-build/t6q-9、RUSTC_WRAPPER空/runner=/usr/bin/env。build/measureは共通build.lockで直列化し、source復元→build→binary copy/hash→測定→候補復帰を同lock区間で実施。timeとallocのbinary/source manifest/toolchainをworktree target/performance-artifacts/t6q-9に保存する。初回cold release依存buildは約3分、測定本体は小さい。

共有高負荷ホストの時間は探索値として扱い、本番改善率を主張しない。採用判断は同一出力契約と確保/gross scratch除去で行う。net/peakは実測を分けて記録する。

## 最終検証

`cargo test --offline -p shodo`: 790pass/2ignored、29suite（doctest含む）。関連harness shaping/line_shaping/horizontal_contracts/analysis_limits:77pass。bench allocator5pass。最終semantic unit1pass。fmt/all-targets clippy(shodo/accesskit,allocation-counting、-D warnings)はexit0。Clippy初回はtestの1要素Range Vecにsingle_range_in_vec_init警告が出たため、期待を開始/終了tupleへ直しunit/clippy/fmtを個別lockで再実行した。runtimeは測定after sourceとrustfmt正規化後にbyte同一で、counterを含まない。Cargo checksは初回batch後に、追加commandごとlockを解放した。全workspace/CI/latest main統合はcontroller担当。
