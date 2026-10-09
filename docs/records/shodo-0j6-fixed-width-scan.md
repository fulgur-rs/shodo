# shodo-0j6: 固定幅途中行のScanコピーを省略

内部の固定幅全走査（`break_all`、`max_inline_size`、Balanceの`break_ends`）では、floatなしの途中行のScanを所有権移動で返す。従来は再試行用に深いcloneを保持したが、受理後にtokenが進むと次行で破棄されていた。最終行のScan、一般の`next_line`／公開`lines`、floatのindexed retryは従来どおり保持する。`first_line_advance`は先頭行だけで止まり、同tokenの再試行が可能なため、途中行も保持する。

公開API、フォント、corpus、依存、snapshotの変更はない。保持容量は増やさない。課題記述の`line_summary`というAPIは現行コードにはなく、全走査のsummary相当として`max_inline_size`を確認した。

## 比較条件

基準はmain `eb5b1fb96b6b379ab80093cc1c900727b4dc5a83`（#269後）。2026-10-09、AMD Ryzen 5 5600G、x86_64 Linux 7.2.5-3-omarchy、rustc/cargo 1.91.0、LLVM 21.1.2。release opt-level=3、debug=0、incremental=false、kache wrapper、complex-scripts有効。時間・Callgrind用と`allocation-counting`用は別実行ファイル。同じ拡張済みprobeを基準と候補でbuildし、各build完了後に実行ファイルを保存した。基準buildの間だけproductionの3ファイルを基準版に置き換え、finallyで候補をbyte-for-byte復元した。compiler、font、Cargo.lock、ソース、実行ファイルのSHA-256と集計は[JSON記録](shodo-0j6-fixed-width-scan.json)に保存する。

固定fixtureを登録し、system discoveryを無効化。`scan_cache_cost`はLatin familyを明示し、font queryを計測前に解決。font load・Paragraph build・入力準備は計測外。各操作でfresh LayoutContextを使い、描画・JSON snapshot生成は計測外。`fixed`は幅96px、`fixed_wide`は10,000px、Balanceは幅96px・既定limit（最大16探索）。longは`"alpha beta gamma delta "`を64回反復し、通常の固定幅結果は129行。one_lineは`"alpha beta"`。ligature、selected soft hyphen overlay、高さ拒否と縮小幅retryも含む。詳細はprobeのCase／operation定義を参照。

## コピー・確保・出力

test-only counterで17行のraw Scan cloneが17→1に減ることをRED/GREENで確認。warmな同幅raw cache、first-line別dataset、forced break、単行でも最終scanを保持し、同tokenの再試行はunitを再走査しない。first_line_advanceの途中scanも再利用する。32行×2回のBalanceテストではraw cloneが64→2で、Line constructor数64・Line clone数0を維持。32 floatの固定幅処理では一度のunit scan／indexで再試行する。

確保は各21 samplesでcalls/gross/net/peakが安定。操作内のLine／plan破棄を含み、返されたLayoutContextを保持してallocator scopeを終了する（context破棄は計測外）。数値はallocatorへの要求量で、RSSではない。

| 入力・操作 | Calls: 前→後 | Gross bytes差: 後−前 | Net bytes差 | Peak extra bytes差 |
|---|---:|---:|---:|---:|
| long / fixed | 9,477 → 9,349 | -5,868 | 0 | 0 |
| long / balance | 116,541 → 114,054 | -82,152 | 0 | -24 |
| long / max_inline_size | 9,469 → 9,341 | -5,868 | 0 | -44 |
| one_line / fixed | 18 → 18 | 0 | 0 | 0 |
| long / first_line_advance | 18 → 18 | 0 | 0 | 0 |
| long / continuous next_line | 9,469 → 9,469 | 0 | 0 | 0 |
| long / height_retry | 26 → 26 | 0 | 0 | 0 |
| long / shrink_4 | 112 → 112 | 0 | 0 | 0 |
| owned_shy_overlay / fixed | 346 → 336 | -448 | 0 | 0 |

5入力×9操作の45組すべてでgeometry、glyph ID／cluster／位置／advance、範囲、break token、次行の出力、warning sequenceまたはsummary／planが前後一致。全45組でnet／peakは増えていない。

`internal_line_clone`のrich入力ではplain、first-line、ruby、first-line-ruby、forced、block、float、edge、grapheme limit等について、internal／公開lines／manualの合計108行の正規化前snapshot・警告・build警告が前後一致。source mappings、ruby、fragment／decoration geometryを含む。このprobeは基準版でもrubyのAPI間比較に失敗していた。`break_all`がcollect後に割り当てるInlineBoxのslice_offsetは、manual／公開linesでは未割当なので、API間assertion専用のコピーからこのフィールドだけ除外した。保存するsnapshotは変更せず、revision間比較では実際のSome／Noneを含む全フィールドを照合した。

テストfixtureの初回失敗も修正した。forced newlineには既定の空白collapseではなくPreserveBreaksが必要だった。32個のfloatは3-byte source anchorを持つので、文字範囲は0..32ではなく0..128である。これらは製品挙動の変更ではない。

## Valgrindのメモリ検証（追加）

Massifではヒープピークはほぼ変わらず、Memcheckでは変更前後とも検出エラー・lost分類のリークは0。主な効果は短命な確保・コピーの削減であり、保持量やピーク量の大幅な削減ではない。

### Massif

ホストのValgrind 3.25.1で、同じrelease probeを各側1 process、longのfixed／balanceを各20操作。`--time-unit=B --stacks=no --pages-as-heap=no --peak-inaccuracy=0 --detailed-freq=1 --max-snapshots=1000`。font load、Paragraph build、context破棄、出力を含むプロセス全体のヒープを測った。RSS、stack、ページ単位の使用量は測っていない。JSONにpeak snapshotと実行ファイルhashを保存した。最後のsampleはプロセス終了時の残存量を表さないので、その値でリークを判断しない。

| 操作 | 総ピーク時の要求ヒープ量: 前→後 (bytes) | 要求量＋Massif推定overheadのピーク: 前→後 (bytes) |
|---|---:|---:|
| long / fixed | 1,135,125 → 1,135,125 | 1,201,360 → 1,201,024 |
| long / balance | 1,097,361 → 1,097,337 | 1,162,808 → 1,162,768 |

推定overheadはMassif既定の8 bytes/blockとalignmentであり、native allocatorの実占有量ではない。ピークtreeにはfixture font、Paragraph data、Line出力等が含まれる。操作だけを測った前節のallocator scopeとは範囲が異なる。`scan_cache_cost`の入力準備には既存の1,472-byte `Box::leak`があり、両版に共通して含む。このprobeで終了時リークの有無は評価していない。

### Memcheck

ホストのglibc `2.44+r24+g16be1518495f-1`では、ローダーのmandatory `memcmp` redirectionに必要なデバッグ情報が見つからず、Memcheckがmain前に停止した。build ID `1e794eb14f4bccf6186bf8d9aa2018e8963ea668`のdebuginfod照会はnot found、公式core-debugミラーにも同版がなかった。ホストのパッケージや設定を変更せず、既存のDebian bookworm-slimイメージから使い捨てコンテナを作り、そこだけにValgrindと`libc6-dbg`を導入した。[debuginfodによる検索の説明](https://valgrind.org/docs/manual/manual-core.html)も参照。

Valgrind 3.19.0、Debian glibc `2.36-9+deb12u14`。ホストでbuildした同じ実行ファイルを使い、libcとlibgccはコンテナのもの。image digest、package版、実行ファイルhashはJSONに記録。Memcheckはホストのglibc 2.44での検証ではない。

`--leak-check=full --show-leak-kinds=all --errors-for-leak-kinds=definite,indirect,possible --track-origins=yes --num-callers=24 --error-exitcode=99 --vgdb=no`。各側でrich全入力（samples=1）、Balanceのplain／first-line-ruby（32反復・80px・default budget、samples=1）の計3 processを実行した。richは内部fixed／公開lines／manual、first-line、ruby、float、block、forced、grapheme limit等を含む。baselineとcandidate計6 processすべてexit=0、ERROR SUMMARY=0、suppression=0。definitely／indirectly／possibly lostはすべて0 bytes。raw出力・警告を110行で前後照合し、一致した。

still reachableはrichで8,155 bytes／65 blocks、Balanceで456 bytes／1 blockで、前後同じ。backtraceを確認し、richの7,699 bytesはfixture casesの`OnceLock`、共通の456 bytesはRustのstack-overflow `thread_info`に由来する。lost分類のリークと区別する。これらの入力範囲で検出エラーがないことを示す検証であり、任意の入力・環境でのメモリ安全性の証明ではない。

再現は同一probeを2版でbuildし、前節の`~/tmp`内へ実行ファイルを保存して行う。Massifはホストで`SHODO_SCAN_CASE=long SHODO_SCAN_OPERATION=fixed SHODO_SCAN_REPEATS=20`を指定し、上記optionで`scan_cache_cost loop`を実行（balanceへも切り替える）。MemcheckはJSONのimage digestを固定して使い捨てコンテナに`valgrind libc6-dbg`を導入し、`SHODO_CLONE_PREFIX=rich/ SHODO_CLONE_SAMPLES=1`で`internal_line_clone`、`SHODO_BALANCE_CASE=plain/32/80/default/Balance SHODO_BALANCE_SAMPLES=1`で`balance_summary`を実行（first-line-rubyへも切り替える）。コンテナのTMPDIRと出力は測定用ディレクトリのmount内に置き、`--rm`でコンテナを削除する。ログ・一時build・取得したデバッグ情報cacheは正式記録への集計保存後に削除する。

## Callgrind

Valgrind 3.25.1、各側1 process、20操作。`--collect-atstart=no --toggle-collect='*scan_cache_cost*run*'`でrunと子関数のIrを取得し、runのinclusive IrがPROGRAM TOTALSと一致することを確認した。fresh context生成・操作内Line／plan破棄を含み、font load・Paragraph build・返されたcontextの破棄・snapshot生成は収集外。cache／branch simulationは無効。命令数は実時間と区別する。

| 入力・操作 | Ir: 前→後 | 後/前 |
|---|---:|---:|
| long / fixed | 210,518,403 → 208,966,540 | 0.992628 |
| long / balance | 2,280,179,324 → 2,243,582,217 | 0.983950 |
| one_line / fixed | 705,602 → 705,962 | 1.000510 |
| long / first_line_advance | 781,995 → 782,215 | 1.000281 |
| long / height_retry | 55,844,435 → 55,845,035 | 1.000011 |

途中行コピーを省く経路は命令数も減少。保持する対照経路には0.001〜0.051%の増加があり、方針の伝達やcodegenの差も含む。

## 実時間・検証

CPU 2へ固定し、ABBAを6 round、BAABを6 round。各processは8操作warm-up後、21 samples×32操作を計測し、sampleの中央値を代表値とする。各roundの後2回の平均／前2回の平均の中央値を下表に示す。操作内と返されたcontextの破棄を含み、font load・Paragraph build・出力snapshot生成は時間外。

| 入力・操作 | 後/前の中央値 | 短縮round | Round比の範囲 |
|---|---:|---:|---:|
| long / fixed | 0.9857 | 7/12 | 0.766–1.325 |
| long / balance | 1.0197 | 6/12 | 0.741–1.431 |
| one_line / fixed | 0.9989 | 6/12 | 0.918–1.039 |
| long / first_line_advance | 1.0335 | 2/12 | 0.952–1.465 |
| long / height_retry | 1.0179 | 3/12 | 0.979–1.063 |

long fixedは約1.4%短く、Balanceは約2.0%長かった。ただしround比の幅が大きく、共有ホストの周波数・SMT（CPU 2のsibling CPU 8）・他processは制御していない。単行や従来どおり保持する経路でもばらつくため、速度改善や無退行を断定しない。採用根拠は不要なコピー・確保の削減、対象経路のIr削減、出力・再試行契約の維持。実時間の評価が重要な用途では対象corpusと静かな環境で再測定する。

最終ソースで以下を検証した。既存のignoredテスト（各full suiteの9件）を除き、失敗なし。

- `cargo fmt --all --check`、workspace全targetのClippy `-D warnings`（通常／AccessKit）。
- `cargo test --workspace`: 1,722 passed、AccessKit: 1,738 passed。
- `cargo test --release -p shodo --lib line::cache::tests`: 22 passed。
- `cargo test -p shodo --no-default-features`: 1,187 passed、complex-scripts併用: 1,190 passed。
- strict rustdoc（通常／AccessKit）。
- 固定snapshot 46ケース全一致、changed pixels=0、更新なし。
- 製品コード、observer、確保集計、raw digest、時間／Callgrind記録を独立レビューし、指摘なし。

製品commit `01f3a265a50ef12f5a9c4f8af74a17a34853f7d1`のPR CI（run `37901601746`）はcheck／MSRV／Wasmすべて成功。追加のValgrind記録では製品・probeソースhashが不変であることを確認した。

## 再現

一時成果物は`~/tmp`に作る。2つのcheckoutでこのPRの同一probeを使い、別targetへrelease buildし、次を前後で実行する。feature統合を避け、timeとallocの実行ファイルを別保存する。

```sh
mkdir -p "$HOME/tmp"
measure_dir=$(mktemp -d -p "$HOME/tmp" shodo-0j6.XXXXXX)
export TMPDIR="$measure_dir" CARGO_TARGET_DIR="$measure_dir/target"
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
cargo build --release -p shodo-bench --example scan_cache_cost
cp "$CARGO_TARGET_DIR/release/examples/scan_cache_cost" "$measure_dir/time"
cargo build --release -p shodo-bench --features allocation-counting --example scan_cache_cost
cp "$CARGO_TARGET_DIR/release/examples/scan_cache_cost" "$measure_dir/alloc"
"$measure_dir/alloc" alloc > "$measure_dir/alloc.json"
SHODO_SCAN_CASE=long SHODO_SCAN_OPERATION=fixed SHODO_SCAN_REPEATS=32 \
  taskset -c 2 "$measure_dir/time" time > "$measure_dir/time.json"
SHODO_SCAN_CASE=long SHODO_SCAN_OPERATION=fixed SHODO_SCAN_REPEATS=20 \
  valgrind --tool=callgrind --collect-atstart=no \
  --toggle-collect='*scan_cache_cost*run*' \
  --callgrind-out-file="$measure_dir/callgrind.out" "$measure_dir/time" loop
cargo build --release -p shodo-bench --example internal_line_clone
SHODO_CLONE_PREFIX=rich/ SHODO_CLONE_SAMPLES=1 \
  "$CARGO_TARGET_DIR/release/examples/internal_line_clone" > "$measure_dir/rich.json"
```

summary・warning・全raw snapshotをrevision間で照合し、時間はABBA／BAABで順番を入れ替える。集計・測定条件・判断を正式記録へ保存した後に、この作業で作った生データと一時buildだけを片付ける。
