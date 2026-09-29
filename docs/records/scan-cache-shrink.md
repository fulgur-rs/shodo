# 長文の幅縮小 retry における広い scan の index 準備を抑える（shodo-idd）

[`scan-cache-cost.md`](scan-cache-cost.md) で、長い非 float 行を 10,000 px から 512 px に縮める最初の retry が汎用 `PartialLine` cache で退行すると確認した。最初の幅縮小で広い scan の全単位を `index()` が再測定し、prefix/frontier を確保する。固定長文を対象にした `perf record -e cycles:u -F 999 -g`（`SHODO_SCAN_CASE=long SHODO_SCAN_OPERATION=shrink_1 SHODO_SCAN_REPEATS=256`）では 3,575 sample・lost 0 のうち `PartialLine::index` 自体が 6.58% self、spacing summary join が 14.69%、端形状の measure_edit が 9.03% self だった。これらは経路の同定であり、個別関数の割合を改善率に足し合わせない。

修正では、まだ index を持たない有効な Raw Scan の初回幅縮小だけ、保存済みの生 advance で安い判定をする。先頭 `floor(N/16)` 単位の合計 advance が要求幅を超えるほど深い縮小なら、広い scan を先に破棄し、狭い幅で fresh scan する。break の選択は従来の scan が spacing・禁則・端形状を含めて決める。既に index がある float や浅い幅縮小の多回 retry、同幅高さ retry は既存経路を使う。後者の浅い retry が index を作った後は、さらに狭めても index を再利用する。

BASE はマージ済み `cd8ed49791049f5b6ee34c0d88c3a5f3a690c289`、修正コードは `4f1fcb646918ceae93e4a73ca9a4fa0f49e28a33`。その 2 版で同一の [`scan_cache_cost.rs`](../../dev/bench/examples/scan_cache_cost.rs)、同一 offline lockfile、固定 Latin フォント byte、release 条件を使った。プローブにケース/操作/反復数の環境変数フィルタを加えたが、通常実行の workload 定義・反復数は変えていない。

| 入力・操作 | 実時間中央値 µs BASE→修正 | 要求割当 byte | 区間末保持 byte | ピーク増分 byte |
| --- | ---: | ---: | ---: | ---: |
| 短文・連続行 | 17.08→16.71 | 18,012→18,012 | 3,491→3,491 | 6,187→6,187 |
| 短文・同幅高さ retry | 6.78→6.71 | 2,304→2,304 | 128→128 | 624→624 |
| 短文・幅縮小 retry 1 回 | 12.54→12.40 | 5,176→5,176 | 1,312→1,312 | 2,104→2,104 |
| 短文・幅縮小 retry 4 回 | 17.15→16.94 | 7,416→7,416 | 1,312→1,312 | 2,104→2,104 |
| 長文・連続行 | 979.54→980.78 | 1,228,395→1,228,395 | 119,437→119,437 | 149,981→149,981 |
| 長文・同幅高さ retry | 189.78→190.09 | 47,200→47,200 | 8,192→8,192 | 13,416→13,416 |
| **長文・幅縮小 retry 1 回** | **362.11→184.54** | **152,976→44,256** | **47,912→256** | **56,480→13,416** |
| **長文・幅縮小 retry 4 回** | **367.87→198.73** | **155,276→51,884** | **47,912→2,408** | **56,480→13,416** |
| `ffi office` 合字・連続行 | 27.11→27.08 | 31,274→31,274 | 4,491→4,491 | 7,079→7,079 |
| `ffi office` 合字・同幅高さ retry | 5.47→5.45 | 2,064→2,064 | 128→128 | 624→624 |
| `ffi office` 合字・幅縮小 retry 1 回 | 9.67→9.76 | 4,032→4,032 | 920→920 | 1,712→1,712 |
| `ffi office` 合字・幅縮小 retry 4 回 | 13.77→13.48 | 6,168→6,168 | 920→920 | 1,712→1,712 |
| owned SHY overlay・連続行 | 41.28→40.68 | 46,294→46,294 | 3,863→3,863 | 7,206→7,206 |
| owned SHY overlay・同幅高さ retry | 13.45→13.18 | 18,124→18,124 | 3,384→3,384 | 5,000→5,000 |
| owned SHY overlay・幅縮小 retry 1 回 | 20.34→20.32 | 25,823→25,823 | 3,168→3,168 | 6,848→6,848 |
| owned SHY overlay・幅縮小 retry 4 回 | 34.66→34.16 | 45,824→45,824 | 3,168→3,168 | 6,848→6,848 |

長文の単発 retry はこの条件で約 49% 短縮し、4 回 retry は約 46% 短縮した。長文以外の割当・保持・ピークは全ケースで byte 単位まで同じで、小さい実時間差は実質的な改善・退行と断定しない。修正後に同じ条件で得た 998 sample・lost 0 の `perf` 診断では `PartialLine::index` の sample はなく、`scan::scan` が観測された。sample 数の異なる profile の比率自体は速度比較に用いず、上表の時間測定を使用する。

各測定は段落構築後に fresh `LayoutContext` で開始し、時間区間は context の破棄まで含めた。時間は 8 warmup、4 反復/サンプル、21 サンプル/ブロックを BASE→修正→修正→BASE で CPU 10 に固定して取得し、各版 42 サンプルの中央値を表にした。割当は別の `allocation-counting` binary で各版 21 回取得し、対象フィールドは 21 回すべて一致した。保持量は行結果を捨てて context を生かしたまま scope を閉じた正味要求 byte、ピークは開始時 live bytes からの最大増分であり、RSS ではない。CPU は AMD Ryzen 5 5600G、Rust 1.96.0、Linux 7.2.5。固定の合成入力と共用機上の計測であり、実アプリ全般の速度や公式 WPT PASS 数は推定しない。

全 16 ケース・6 capture の行 range、break reason、glyph ID/cluster/座標/advance、geometry、break token、および token から fresh context で得た次行出力の署名は一致した。連続行ケースでは同じ context で終端まで進む。長文 deep shrink と可視 SHY overlay の回帰テストは fresh 行と glyph を比較する。既存の justified・float・warning・first-line retry テストを含むワークスペーステストも通過した。全署名・raw samples は [`scan-cache-shrink-raw.json.gz`](data/scan-cache-shrink-raw.json.gz)、集計は [`scan-cache-shrink-summary.json`](data/scan-cache-shrink-summary.json)、コミット・tree・lockfile・フォント・プローブ・4 binary・6 stdout の SHA-256 と環境は [`scan-cache-shrink-manifest.json`](data/scan-cache-shrink-manifest.json) に記録した。

再実行は両固定コミットを別 worktree に配置し、修正版の `dev/bench/examples/scan_cache_cost.rs` と同一の offline `Cargo.lock` を BASE にコピーして行う。

```sh
cargo build --offline --locked -p shodo-bench --release --example scan_cache_cost
taskset -c 10 target/release/examples/scan_cache_cost time
cargo build --offline --locked -p shodo-bench --release --features allocation-counting --example scan_cache_cost
taskset -c 10 target/release/examples/scan_cache_cost alloc
```
