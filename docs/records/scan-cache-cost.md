# 汎用 PartialLine scan キャッシュの実測（shodo-dc8）

`shodo-j2r.8` のレビューで未計測だった BASE と変更後の損益を、同じ公開 API プローブで比較した。BASE は `c2da251d78924c3ae99594a704ce983938c92dba`、変更後は `1af6a21fc37df1cb7a6a506b96ef2a77000c7d5e`。差分は `src/context.rs`、`src/limits.rs`、`src/line/cache.rs` のみで、後続の最適化やレイアウト変更を含めない。

| 入力・操作 | 実時間中央値 µs BASE→変更後 | 要求割当 byte | 区間末保持 byte | ピーク増分 byte |
| --- | ---: | ---: | ---: | ---: |
| 短文・連続行 | 18.74→18.39 | 18,460→18,588 | 3,459→3,491 | 6,155→6,187 |
| 短文・同幅高さ retry 1 回 | 14.06→8.00 | 3,264→2,448 | 0→128 | 496→624 |
| 短文・幅縮小 retry 1 回 | 14.07→14.28 | 3,264→5,416 | 0→1,312 | 792→2,104 |
| 短文・幅縮小 retry 4 回 | 30.77→19.10 | 7,760→7,800 | 0→1,312 | 792→2,104 |
| 長文・連続行 | 1,055.68→1,056.11 | 1,241,075→1,246,971 | 119,405→119,437 | 150,029→150,029 |
| 長文・同幅高さ retry 1 回 | 477.65→244.05 | 73,920→47,344 | 0→8,192 | 8,512→13,416 |
| **長文・幅縮小 retry 1 回** | **252.33→440.58** | **39,408→153,216** | **0→47,912** | **8,512→56,528** |
| **長文・幅縮小 retry 4 回** | **269.49→445.46** | **44,352→155,660** | **0→47,912** | **8,512→56,528** |
| `ffi office` 合字・連続行 | 27.14→27.13 | 31,618→31,706 | 4,459→4,491 | 7,059→7,079 |
| `ffi office` 合字・同幅高さ retry | 10.56→6.32 | 2,944→2,208 | 0→128 | 496→624 |
| `ffi office` 合字・幅縮小 retry 1 回 | 10.58→10.93 | 2,944→4,272 | 0→920 | 792→1,712 |
| `ffi office` 合字・幅縮小 retry 4 回 | 23.49→15.57 | 7,136→6,552 | 0→920 | 792→1,712 |
| owned SHY overlay・連続行 | 41.48→42.35 | 46,278→46,774 | 3,799→3,863 | 7,254→7,254 |
| owned SHY overlay・同幅高さ retry | 19.98→13.71 | 25,484→18,316 | 1,776→3,384 | 4,680→5,000 |
| owned SHY overlay・幅縮小 retry 1 回 | 20.12→20.98 | 25,468→26,255 | 1,776→3,168 | 5,584→6,848 |
| owned SHY overlay・幅縮小 retry 4 回 | 45.57→35.24 | 56,006→46,688 | 1,776→3,168 | 5,584→6,848 |

同幅高さ retry は全 4 入力で短縮し、幅縮小 4 回も短文・合字・SHY 入力で短縮した。長文の幅縮小は 1 回でも 4 回でも退行した。最初の縮小時に元の広い scan 全体へ prefix/frontier index を準備する費用が、短い再スキャンの節約を上回るという実装上の説明と整合する。これはこの固定入力の測定結果であり、実アプリ全般での速度差や WPT PASS 数は示さない。長文退行の抑制は別 issue に追跡する。

プローブは [`scan_cache_cost.rs`](../../dev/bench/examples/scan_cache_cost.rs)。固定 Noto Latin subset（SHA-256 `7aa5c6687e9a8b72f71ea5abaded28d771ecb54c66a8a9ac49268537b2f94d25`）を使う。短文は 30 文字、長文は `alpha beta gamma delta ` を 64 回、合字入力は `ffi office` を 3 回、owned overlay 入力は `ab\u{ad}cdef\u{ad}ghijkl\u{ad}mn`。合字入力では `ffi` が 1 glyph になり、SHY 入力の 48 px 行では U+00AD の可視 glyph が選択される。SHY 入力は `line/cache.rs` の所有権テストが実 owned overlay の保持を確認する同じ入力・幅を使った。

各サンプルは段落を測定前に組み、fresh `LayoutContext` で操作する。連続行は幅 96 px で終端まで進める。同幅 retry は高さ上限 0 で拒否してから同じ幅で再実行する。幅縮小は初回 10,000 px（SHY は 48 px）後に、短文・長文・合字では 512/320/192/96 px、SHY では 44/40/36/32 px を順に使う。測定区間には初回 scan clone、同幅 hit clone、最初の縮小での広い prefix 準備が含まれる。

実時間は `Instant` で各操作 8 回 warmup、4 反復/サンプル、21 サンプル/ブロックを取り、BASE→変更後→変更後→BASE の 4 ブロックを CPU 10 に固定して交互実行した。表は各版 42 サンプルの中央値。割り当ては別の `allocation-counting` build で 21 回測定し、表の値は全 21 回で一致した。保持量は行結果を捨てた後、`LayoutContext` を生かしたまま allocator scope を閉じた正味要求 byte。ピークは scope 開始時の live bytes からの最大増分で、RSS ではない。計測機は AMD Ryzen 5 5600G / Rust 1.96.0 / Linux 7.2.5。高い共用機負荷による時間揺れがあり、小さい差の有意性は主張しない。長文の高さ retry と幅縮小の方向は両ブロックで一致した。

行 range、break reason、glyph ID/cluster/座標/advance、geometry、break token、およびその token から fresh `LayoutContext` で得る次行出力の署名は全 16 ケース・6 capture で一致した。連続行ケースは同じ context で終端まで測定している。生サンプルと各 capture の署名は [`scan-cache-cost-raw.json.gz`](data/scan-cache-cost-raw.json.gz)、全数値は [`scan-cache-cost-summary.json`](data/scan-cache-cost-summary.json)、コミット・lockfile・プローブ・binary の SHA-256 と環境は [`scan-cache-cost-manifest.json`](data/scan-cache-cost-manifest.json) に保存した。両固定コミットには同一プローブを `dev/bench/examples` にコピーし、BASE で offline 生成した `Cargo.lock` を変更後にもコピーした。実行コマンドは以下（両 worktree で同じ）。

```sh
cargo build --offline --locked -p shodo-bench --release --example scan_cache_cost
taskset -c 10 target/release/examples/scan_cache_cost time
cargo build --offline --locked -p shodo-bench --release --features allocation-counting --example scan_cache_cost
taskset -c 10 target/release/examples/scan_cache_cost alloc
```
