# 短い段落の初回buildを重点にした simplify

2026年10月15日。基準コミットは `0351b43f07b648c5fe01c6e7619fb31d90fb5b58`（PR #353マージ後）。
前回のメモリ改善に続き、通常の短い段落をfreshなLayoutContextで初めてbuildする経路を調べた。
公開API・計算結果・警告・予算判定を保つ3点を変更した。測定時点ではコミット・stage・pushを行っていない。

## 変更と直接の検証

- `line/font_metrics.rs`: 取得済みFontDataを既存の `metrics_from_data` へ渡し、横・縦metricsを一度のFontRef解析で求める。
  字形計測側でも使うヘルパーを再利用し、size検査・variation・stub・縦metricsの扱いを保つ。
  固定フォントの既存テストではFontRef openが10→9、fallbackケースは9→8。
- 同ファイルの `resolve_styles_memo`: styleが1個なら、その段落内に後続の同一styleはないため、
  段落内のdeduplication HashMapを使わず直接resolveする。複数styleでも最初のcontext memo missまでmapを初期化しない。
  context memoの検索・挿入、警告checkpoint、font世代の検査は維持する。
  追加テストで2回の呼出に対しresolveは1回、段落内キーhashは1→0、metrics同一・警告なしを確認。
- `paragraph/build.rs`: normal分析へParagraphStyleをmoveする。alternateが必要なときだけ
  `normal.style` からcloneし、通常段落の余剰cloneと解放を省く。分析関数はstyleを変更しない。

テストを先に変更して修正前の3失敗（hash 1対0、open 10対9 / 9対8）を確認し、
修正後には関連15テストが通った。3変更一括のA/B比較であり、wall timeの個別寄与は分離していない。
3観点（再利用・品質・効率）のread-onlyレビューと、最終コード・測定手順のレビューで修正必須事項はなかった。

## 測定条件

AMD Ryzen 5 5600G、Linux x86_64、論理CPU 2固定、Rust 1.91.0、Valgrind 3.25.1。
基準のgit archiveを隔離ソースへ展開し、同じprobeを置いてbeforeをビルド・退避した後、
2ファイルの差分を適用してafterをビルドした。Cargo.lock・依存関係・ソースパス・build設定は同じ。
release opt-level=3、debug=1、strip=none、incremental無効。
`shodo-bench` の既定complex-scriptsのみで、allocation-countingとsystem fontは無効。
固定Latin/CJK/Arabicフォント3個を両版で同じ順に登録する。

新しい `first_build_probe` はfont登録、builderとLayoutContextの生成を計測外で行い、
`#[inline(never)]` / `#[unsafe(no_mangle)]` の `shodo_first_build` 内だけを測る。
`ParagraphBuilder::build` がbuilderを消費するため、その解放は測定に含む。
`cold` はプロセスで最初の1 buildで、font collection内のshaping等のcacheもcold。
`warm` はfont collectionを3 buildでwarmupし、毎回freshなLayoutContextでbuildする。
context memoを段落間で共有する条件ではない。入力・contextは各反復で作り直す。

初回8ケースは各版31独立プロセス（31ペア）。warmは各版11プロセスで、各プロセス101 buildの中央値を採る。
版の順序はペアごとにAB/BAを交互にした。主測定はnative 840プロセス、Callgrind 80プロセス。
時間測定と並行してビルド・テストを動かしていない。
wall欄は各版のプロセス値の中央値、ペア比は対応するafter/beforeの中央値。
別々の中央値の割り算とペア比は一致しないことがある。

RSSは計測対象自身の `/proc/self/status` のVmRSS/VmHWMをbuild後とExitRssで読み、
観測値の最大を各プロセスのpeakとして集計する。Linuxの近似カウンタの差を考慮し、両方の最大を採る。
表はnativeプロセスの中央値。fixture初期化・layout/geometry検証・解放までを含む**プロセス全体の高水位**で、
buildだけの追加メモリではない。wait4のexec前RSSも使っていない。数十～百KiBの変動からRSS削減は結論しない。

Callgrindは `--collect-atstart=no`、`--toggle-collect=shodo_first_build` と
`--toggle-collect=shodo_memory_build`、cache/branch simulationなし。
各条件・版2独立プロセスのIrは完全一致した。cold表は1 build、warm表は5 build分のIrで、
新probeの3 warmupはwrapper外なので含まない。nativeのwall timeをValgrind下の時間で置き換えていない。

初期のcold英語32文字profile（785,968 Ir）ではGPOS読込・shaping準備が主な費用であった。
最終campaignのbeforeは785,970 Ir。不要な段落内cache・style clone・重複FontRef解析に絞り、
必要なshaping初期化や出力契約には手を入れていない。

## 初回buildの比較

日英混在のcase名は `mixed-32` だが実際の入力は21 Unicode scalar。
paintの4 inlineはrootを含め5 style slotで、単一styleのshortcutに依存しない対照。
first-lineはroot 16px、alternate 18px。表の文字数はコードポイント数。

| 入力 | wall µs 前→後 | ペア比の中央値 | Ir 前→後 | Ir差 | RSS MiB 前→後 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 英語1文字 | 190.606 → 179.710 | -9.90% | 422,965 → 420,022 | -0.696% | 7.676 → 7.656 |
| 英語32文字 | 235.446 → 227.764 | -4.19% | 785,970 → 784,179 | -0.228% | 7.703 → 7.586 |
| 英語128文字 | 301.798 → 287.201 | -5.83% | 1,491,886 → 1,489,477 | -0.161% | 7.938 → 7.855 |
| 日本語32文字 | 282.032 → 270.857 | -10.03% | 1,063,770 → 1,060,677 | -0.291% | 8.199 → 8.133 |
| 日英混在21文字 | 277.702 → 269.740 | -2.37% | 1,004,277 → 1,002,813 | -0.146% | 8.238 → 8.152 |
| アラビア語13文字 | 206.949 → 196.403 | -5.16% | 430,597 → 428,471 | -0.494% | 7.637 → 7.707 |
| paint違い4 inline・128文字 | 336.022 → 317.304 | -6.74% | 1,572,169 → 1,571,869 | -0.019% | 8.027 → 8.035 |
| first-lineサイズ変更・32文字 | 288.667 → 277.492 | -3.42% | 1,167,137 → 1,162,559 | -0.392% | 7.801 → 7.777 |

## font側warm・contextはfresh

wallは1 buildあたりの値、Irは5 build合計。

| 入力 | wall µs 前→後 | ペア比の中央値 | Ir 前→後 | Ir差 | RSS MiB 前→後 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 英語1文字 | 15.227 → 14.667 | -3.22% | 580,458 → 564,857 | -2.688% | 7.660 → 7.562 |
| 英語32文字 | 33.945 → 33.944 | -0.00% | 1,853,909 → 1,839,420 | -0.782% | 7.812 → 7.652 |
| 英語128文字 | 78.296 → 77.877 | -0.63% | 5,396,227 → 5,382,330 | -0.258% | 8.012 → 7.977 |
| 日本語32文字 | 62.022 → 61.672 | -0.56% | 3,396,227 → 3,382,570 | -0.402% | 8.312 → 8.254 |
| 日英混在21文字 | 40.370 → 39.462 | -2.59% | 1,910,426 → 1,891,888 | -0.970% | 8.223 → 8.227 |
| アラビア語13文字 | 32.478 → 31.709 | -2.34% | 1,383,873 → 1,367,210 | -1.204% | 7.688 → 7.668 |
| paint違い4 inline・128文字 | 85.211 → 84.512 | -0.74% | 5,748,637 → 5,736,355 | -0.214% | 8.004 → 7.891 |
| first-lineサイズ変更・32文字 | 67.120 → 66.073 | -1.35% | 3,768,529 → 3,738,791 | -0.789% | 7.773 → 7.762 |

## 前回と同じ入力・rubyの対照

既存 `whitespace_flags` を前回のnative記録から同じ形で復元した。
smallは `"a  日\n b "` の24反復を使い、family 3も先頭でmatchする。
初回は各版31プロセス、warmとrubyは各版11プロセス。
warmのnativeは3 warmup後21 buildの中央値だが、Callgrind wrapperはwarmupにも適用されるので、
Irは**24 build合計**。この値を新probeのwarm 5 build合計と直接比較しない。

| 対照 | wall µs 前→後 | ペア比中央値 | Ir 前→後 | Ir差 | RSS MiB 前→後 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 既存small・初回・family 1 | 371.224 → 390.501 | +4.97% | 1,832,314 → 1,827,427 | -0.267% | 8.680 → 8.648 |
| 既存small・初回・family 3 | 363.471 → 360.258 | -3.54% | 1,859,467 → 1,856,010 | -0.186% | 8.668 → 8.605 |
| 既存small・warm・family 1 | 144.857 → 145.277 | +0.29% | 37,826,636 → 37,862,286 | +0.094% | 8.746 → 8.703 |
| ruby annotation + first-line・初回 | 696.490 → 688.737 | -0.28% | 4,030,361 → 4,041,280 | +0.271% | 8.777 → 8.734 |

命令数は対象の短段落8ケースで減ったが、すべての入力で減ったわけではない。
既存smallのwarmは+0.094%、ruby初回は+0.271%。プロファイルの主差分はlibcの
メモリコピー処理（`0x176400`）で、増分の大部分は `0xa7f20` から呼ばれている。
同一style clone/hash等の減少とは別にallocator内のコピー経路が変わっており、
この小さな実測増分を「誤差なのでゼロ」とは扱わない。raw call graphを保存している。

## wall timeの独立再測定

最初のcampaignで既存small初回が+4.97%だったため、凍結済みの同じバイナリで再測定した。
6条件をround robinで回し、最初の順序をBAに反転して交互にした。coldは各版101プロセス、
small warmは各版31プロセス、計1,072プロセス。全raw sampleを別に保存し、主測定と混ぜていない。
95%区間はペア比中央値の10,000回bootstrap（固定seed 42）で、環境間の不確実性は含まない。

| 入力 | ペア数 | wall µs 前→後 | ペア比中央値 | bootstrap 95%区間 |
| --- | ---: | ---: | ---: | ---: |
| 英語1文字 | 101 | 251.860 → 236.703 | -5.77% | -6.65% ～ -4.68% |
| 英語32文字 | 101 | 249.834 → 240.615 | -3.29% | -5.12% ～ -1.81% |
| 日本語32文字 | 101 | 308.294 → 291.461 | -3.84% | -4.69% ～ -2.64% |
| 既存small・初回 | 101 | 414.877 → 413.759 | +0.22% | -0.48% ～ +0.97% |
| 既存small・warm | 31 | 145.276 → 144.648 | -0.24% | -1.01% ～ +0.19% |
| ruby annotation + first-line・初回 | 101 | 705.220 → 699.563 | -0.07% | -1.76% ～ +0.91% |

3つの短い通常段落では再測定でも改善した。既存smallの初回+4.97%は再現せず、
再測定の+0.22%は区間が0をまたぐ。rubyとsmall warmも時間の差は明確でない。
絶対時間はcampaign間でも変動しており、3～6%のwall差を0.2～0.7%のIr削減だけに帰属させない。
命令数の減少、固定環境でのwall観測、RSSの変動を区別して判断する。

## 出力と通常の検証

全native 1,912プロセスとCallgrind 80プロセスで公開出力signatureがbefore/afterで一致した。
first probeでは各測定buildの成功・警告なしを確認し、**各プロセス最後のParagraph**で全行の
geometry/glyph/cluster/advance/font/paint/metrics/ruby等をsnapshot化してSHA256を照合する。
warmの全101反復について完全なgeometry照合をしたという意味ではない。
既存whitespace probeはwarmupを含め各buildのsnapshotを検証し、出力された各sampleの
build/layout警告とhashを版間で照合する。固定fontのmissing glyphは全snapshotで失敗させる。

- `cargo test --workspace --all-features --offline`: exit 0; 全suite合計 2,055 passed / 9 ignored、0 failed。
- `cargo test -p shodo --no-default-features --offline`: exit 0; 全suite合計 1,478 passed / 9 ignored、0 failed。
- `cargo test -p shodo --no-default-features --features complex-scripts --offline`: exit 0; 全suite合計 1,481 passed / 9 ignored、0 failed。
- `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings`: exit 0。
- `cargo doc --workspace --all-features --no-deps --offline`: exit 0。 `RUSTDOCFLAGS="-D warnings"`。
- `cargo fmt --all --check`: exit 0。
- `git diff --check`: exit 0。

Rust 1.89 / Wasm等のCI環境はこのローカル検証では実行していない。

## 再現と後片付け

圧縮記録: [first-build-simplify-2026-10-15.json.gz](first-build-simplify-2026-10-15.json.gz)
（1,608,539 B、SHA256 `689de5cde85089debed504e86946946cca50666dc2da3b446f156b05d8dc77ad`）。
基準SHAと全patch、両Python driver、probe4ファイルの全文・hash、Cargo.lock全文・hash、
3fixtureのhash、build command/env、4バイナリのhash、全native sample、全80回のCallgrind Ir・annotation・logとraw graphのhash、
代表の英語32文字と命令数微増2条件のraw graph（各版1回、計6個）、
独立再測定、red/green・全検証のlogを保存した。実行バイナリは保存しない。

同じLinux環境・CPU 2・キャッシュ済み依存関係・Cargo/Valgrind/tasksetを用意し、
archiveの `reproduction["reproduce.py"]` を取り出して実行する。保存したRust/Valgrind版と
fixture/probeのhashをbuild前に検査し、driver中のrustcメタデータ取得だけPATH参照へ置き換える。
RUN_DIRにはまだ存在しない `$HOME/tmp` 配下のパスを指定する。
再現実行は新しい測定結果を作るので、元のJSONは変更しない。

```sh
mkdir -p "$HOME/tmp"
python3 - <<'EXTRACT'
import gzip, json, pathlib
p = pathlib.Path("docs/records/first-build-simplify-2026-10-15.json.gz")
r = json.loads(gzip.decompress(p.read_bytes()))
(pathlib.Path.home()/"tmp/shodo-first-build-reproduce.py").write_text(r["reproduction"]["reproduce.py"])
EXTRACT
TMPDIR="$HOME/tmp" python3 "$HOME/tmp/shodo-first-build-reproduce.py" \
  "docs/records/first-build-simplify-2026-10-15.json.gz" "$PWD" \
  "$HOME/tmp/shodo-first-build-reproduction"
```

記録とarchiveの整合性を確認後、この作業で作成した測定用ソース・バイナリ・target・
生logの一時ディレクトリだけを削除した。既存の `271.diff`、`mutants.out/`、`mutants.out.old/` は触っていない。
再現した場合も、必要な結果を記録してから自身のRUN_DIRと抽出スクリプトを片付ける。

## 今回適用しなかった案

- builderのroot internerの遅延化は主にbuilder生成側で、今回のbuild計測境界外。
  styleの再訪・budget検査との整合も必要なので拡張しなかった。
- 三つの文字プローブ間でfont query正規化を共有する案は、fallback・警告・世代・Arc化まで
  影響し得るため、今回の小さな重複削除とは分ける。
- 空sliceの `Arc::default` は標準ライブラリの共有静的値を使うため、heap割り当てという当初の推測は棄却した。
