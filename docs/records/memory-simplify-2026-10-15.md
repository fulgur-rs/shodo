# メモリ効率を重点にした simplify

2026年10月15日。基準は `04366458b155bd7af465848922245edea7d07368`。
shodo全体を再利用・品質・効率の3観点で調べ、公開APIと出力を保つ3箇所を変更した。

## 変更

- `paragraph/build.rs`: 分析・shapingの台帳snapshotと再利用用予約を、
  `::first-line` のalternateがある場合だけ作る。通常段落では両phase合わせて
  4回の割り当てを省く。課金そのものや予算・エラーの順序は変更しない。
- `font/matching.rs`: Named familyを1要素の `Vec<String>` にコピーせず、
  元の名前を `named_match` へ借用する。Genericの名前解決と候補順は維持する。
- `line/font_metrics.rs`: Contextをまたぐメトリクスmemoには、
  `same_metric_inputs` が比較する11項目だけをコピーする。
  shaping feature、alternate selector、hyphen文字列をmemoだけのために保持しない。
  同一性・浮動小数点の扱い・世代無効化・警告・置換順序は変えていない。

memoは既存の `InlineStyle` 型と比較関数を再利用する。既定のfamily配列へ
`clone_from` するため、familyが0個・複数個の場合には小さな余剰容量や追加の
再確保が生じ得る。全入力で割り当て数が減るという主張はしない。
専用key型を追加して比較契約を二重管理する変更は避けた。

## memoの保持量を直接確認する回帰テスト

固定フォントを使い、feature 8,192件、styleset 8,192件、character variant
8,192件、64 KiBのhyphen文字列を持つ入力で確認した。これらのpayloadは合計
204,800 Bで、メトリクス計算には使われない。入力とfont cacheのwarmupは計測外。
実際のスレッド別アロケータで `resolve_styles_memo` の保持量と追加peakを測る。
値にはmemoのエントリ配列と返されたmetrics配列も含む。

| requested heap bytes | 修正前 | 修正後 |
| --- | ---: | ---: |
| 保持量 | 206,933 | 2,133 |
| 追加peak | 207,113 | 2,313 |

削減分はどちらも204,800 B。修正前には16 KiBの保持上限でテストが失敗し、
修正後には保持・peakの両上限を満たした。計算結果と警告が一致し、同じ入力の
2回目にはresolverが呼ばれないことも確認する。これはDebugの決定的な
ヒープ回帰テストであり、全段落のメモリが約99%減るという意味ではない。
既存のmetric入力テストにもmemoへの保存・取得を加え、emphasisとem ascentも
検証対象にした。

## 既存releaseプローブの比較

AMD Ryzen 5 5600G、Linux x86_64、論理CPU 2固定、Rust/cargo 1.91.0。
同じcheckout・Cargo.lock・既存fixtureから、修正前をビルドして実行ファイルを
退避した後、同じ設定で修正後をビルドした。release opt-level=3、debug=1、
strip=none、incremental無効。`shodo-bench` の既定complex-scriptsと
`allocation-counting` を有効化し、system fontは無効。

各コマンドを版ごとに独立した3プロセスで実行し、before→after / after→beforeを
交互にした。`whitespace_flags` は各プロセス・ケースで3回warmupした後に9回計測。
表はrequested allocator blocksの中央値。grossは再確保を含む累積要求量、
netはscope終了時に残った増分、peakはscope開始時からの追加高水位。
スタック・malloc管理領域・RSSを含まない。

| 計測対象 | 割り当て回数 前→後 | gross B 前→後 | net B 前→後 | peak B 前→後 |
| --- | ---: | ---: | ---: | ---: |
| whitespace: small-no-first-line | 476 → 472 | 260,511 → 260,159 | 122,919 → 122,919 | 126,151 → 126,055 |
| whitespace: small-first-line | 957 → 957 | 527,810 → 527,810 | 247,336 → 247,336 | 254,044 → 253,948 |
| whitespace: preserved-whitespace-first-line | 1,166 → 1,166 | 688,922 → 688,922 | 319,112 → 319,112 | 326,964 → 326,964 |
| whitespace: large-first-line | 66,505 → 66,505 | 74,430,770 → 74,430,770 | 35,230,616 → 35,230,616 | 35,886,660 → 35,886,660 |
| whitespace: ruby-annotation-first-line | 1,809 → 1,809 | 562,300 → 562,300 | 260,500 → 260,500 | 271,006 → 270,910 |
| font: 16 faces / distinct / 20反復 | 7,243 → 5,763 | 225,904 → 205,184 | 20,809 → 20,809 | 24,321 → 24,321 |
| font: 16 faces / repeat / 20反復 | 1,543 → 1,541 | 45,719 → 45,691 | 16,343 → 16,343 | 16,371 → 16,371 |
| font: 16 faces / native metadata / 20反復 | 32,317 → 30,875 | 5,212,843 → 5,183,282 | 21,152 → 21,152 | 26,798 → 26,757 |
| probe: latin-short scale 1 / build | 643 → 585 | 205,946 → 204,433 | 106,116 → 106,116 | 109,348 → 109,252 |
| probe: many-short-latin scale 64 / build | 16,142 → 15,832 | 8,244,999 → 8,221,310 | 3,927,129 → 3,927,129 | 3,930,361 → 3,930,265 |

全36プロセスで、既存の出力hash・警告・glyph/geometry検証結果が版間で一致した。
fontとprobeの表中4指標は3プロセスで完全に一致し、whitespaceの同指標も
27サンプルで一致した。scope開始時のlive bytesにはシリアライズ用buffer等の
差があるため、絶対値を操作の保持量と混同しない。

通常の段落・短いfamily名では保持量は変わらず、主に短命な割り当てが減る。
例えばdistinct font matchの回数は約20.4%、gross bytesは約9.2%減った。
大きな非metric payloadの保持削減は、上の専用回帰テストで切り分けている。

補助値としてinstrumentedプロセス全体のwall timeと `wait4` 最大RSSも保存した。
これらはfixture初期化・検証・出力・アロケータ計測自体を含み、操作単体の時間や
常駐量ではない。RSSにはlauncher/exec前の高水位が混ざる可能性があり、複数の
コマンドで同じ下限が現れたため、RSSの改善は結論しない。
このinstrumented比較からnative wall timeやCallgrind Irの高速化率は主張しない。

## Callgrindによる命令数の確認

追加の依頼に合わせ、allocation-countingを無効にした別バイナリで確認した。
基準コミットを同じ隔離ソースパスへ展開し、beforeをビルド・退避してから
今回の3ファイル差分を適用し、afterをビルド・退避した。release設定は上記と同じ。
既存の `whitespace_flags` と `font_candidate_prep` の計測関数へ、両版で同一の
`#[unsafe(no_mangle)]` / `#[inline(never)]` wrapperを追加した。一時的なprobe変更の
全文を圧縮JSONへ保存しており、リポジトリ内の既存probeは変更していない。

Valgrind 3.25.1、`--collect-atstart=no`、
`--toggle-collect=shodo_memory_build` / `--toggle-collect=shodo_memory_matches`、
cache/branch simulationなし。入力生成・font初期化・最終geometry/hash検証は区間外。
paragraphのbuild区間はbuilderを消費するため、その解放も含む。
各条件・版を独立した2プロセスで計測し、出力一致と正のIrを要求した。
全18条件で同じ版の2回のIrは完全に一致した。

family条件は既定の1件、空、先頭でmatchする3件、大きなpayloadの4種類。
payload条件は64 KiBのhyphen文字列とfeature 4,096件で、上のmemo単体テストとは
入力規模が異なる。表のIrは計測区間全体であり、fontの20反復を1 matchと解釈しない。

| 初回build / font query条件 | Ir 前→後 | 差 |
| --- | ---: | ---: |
| 通常段落 / family 1 | 1,837,725 → 1,832,314 | −0.294% |
| 通常段落 / family 0 | 2,148,386 → 2,130,990 | −0.810% |
| 通常段落 / family 3 | 1,850,728 → 1,859,467 | **+0.472%** |
| 通常段落 / 大きなpayload | 9,404,166 → 9,289,023 | −1.224% |
| first-line / family 1 | 3,571,141 → 3,543,511 | −0.774% |
| first-line / family 0 | 3,872,096 → 3,847,901 | −0.625% |
| first-line / family 3 | 3,594,638 → 3,568,448 | −0.729% |
| first-line / 大きなpayload | 17,814,707 → 17,605,522 | −1.174% |
| preserved-whitespace-first-line | 4,681,228 → 4,652,772 | −0.608% |
| large-first-line | 533,259,167 → 529,081,519 | −0.783% |
| ruby-annotation-first-line | 4,071,963 → 4,030,359 | −1.022% |
| font 16 faces / distinct / 20反復 | 15,102,328 → 14,840,212 | −1.736% |
| font 16 faces / repeat / 20反復 | 875,369 → 831,638 | −4.996% |
| font 16 faces / native metadata / 20反復 | 42,725,740 → 42,351,417 | −0.876% |

通常段落・family 3の初回には8,739 Irの増加が再現した。呼び出し元別の集計では
libcのreallocation内部から呼ばれるコピーが6,605 → 32,710 Irとなっていた。
これは実行命令の差として扱い、単なる時間ノイズとして除外していない。
割り当て順・容量の変更に伴うreallocationの経路差が寄与するが、単一箇所の
変更だけに原因を帰属させるablation測定までは行っていない。

この増加を受け、同じ通常段落を既存 `time` modeでも確認した。3回のwarmupと
21回のsampleで計24回buildし、その全build区間をcollectする。
contextはbuildごとに新しく、font collectionだけを共有する。

| 反復build条件 | 24 build合計 Ir 前→後 | 差 |
| --- | ---: | ---: |
| family 1 | 38,259,082 → 37,935,011 | −0.847% |
| family 0 | 39,174,833 → 38,608,866 | −1.445% |
| family 3 | 38,694,626 → 38,495,011 | −0.516% |
| 大きなpayload | 225,034,058 → 215,806,236 | −4.101% |

18条件中17条件で減少、1条件で+0.472%。命令数の劣化が全ケースでないという
結果ではない。初回の小さな増加と、不要payloadの保持削減・反復時の命令数削減を
併記して今回の変更を採用する。Callgrind実行中の時間・RSSはnative比較に使わない。

## native wall timeとRSSの追加測定

後続の依頼で、allocation-countingとValgrindを使わずに測定した。
基準コミット・3ファイル差分・release設定・固定font・CPU 2は同じ。
隔離ソースの同じパスからbeforeとafterを順にビルドして退避し、両方で同一の
probeを使った。他のビルド・テストは並行実行していない。

Callgrind用に保存したprobeへ、計測区間外で `/proc/self/status` を読む処理を追加した。
各build直後にparagraph/contextを保持した状態のVmRSS/VmHWMを読み、mainの終了時も
Drop guardで読む。RSS表は各プロセス内で観測したVmHWM/VmRSSの最大値の中央値。
`wait4` を使わず、Python親プロセスの高水位が混ざる問題を避けた。
LinuxのRSS値は近似値で、隣接する読み取りでも数ページの差が生じ得る。
RSSにはfixture・setup・計測後のgeometry検証と出力も含む。

各条件11ペアの独立プロセスで、before→after / after→beforeを交互にした。
wallは `Instant` による対象処理区間の時間。プロセス起動・fixtureロード・
geometry/hash検証・RSS読み取りは含めず、プロセス全体のwallは補助値として保存した。

- `single`: 初回build 1回。
- `time`: 3回warmup後、21回のbuildの中央値をプロセスの代表値とする。
  Contextはbuildごとに新しく、font collectionを共有する。
- `font`: 同じクラスタ列を20反復した検索区間全体。
- `memo`: 同じContextへfont-sizeの異なる16件をbuildする区間の合計。
  `ParagraphBuilder::new`・`push_text`・`build`を含む。
  各paragraphとlineは次のbuild前に解放する。hyphen文字列は0 / 64 KiB / 1 MiBで、
  文字列を使わないテキスト `a` を固定fontでshape/layoutし、結果と警告を検証する。
  featureを大量に含む先のmemo単体テストとは別の入力である。

wall表はµs、RSSはMiB。wallとRSSの各値はプロセス代表値の中央値。
wall差は11個のafter/before比の中央値であり、表の独立した中央値同士を割った比と
一致するとは限らない。ばらつきのmin/max・四分位数と全サンプルを保存した。

| ケース | wall µs 前→後 | ペア比の差 | peak RSS MiB 前→後 |
| --- | ---: | ---: | ---: |
| single / small-no-first-line / family 1 | 368.640 → 380.443 | +6.35% | 8.61 → 8.70 |
| single / small-no-first-line / family 0 | 412.153 → 422.909 | −1.48% | 8.63 → 8.57 |
| single / small-no-first-line / family 3 | 392.247 → 389.523 | −7.73% | 8.62 → 8.68 |
| single / small-no-first-line / payload | 938.782 → 915.173 | −2.28% | 9.12 → 9.08 |
| single / small-first-line / family 1 | 533.893 → 540.527 | −0.61% | 8.79 → 8.71 |
| single / small-first-line / family 0 | 610.582 → 631.465 | +3.42% | 8.86 → 8.79 |
| single / small-first-line / family 3 | 570.491 → 565.253 | −1.65% | 8.79 → 8.70 |
| single / small-first-line / payload | 1,562.284 → 1,462.546 | −6.30% | 9.57 → 9.51 |
| time / small-no-first-line / family 1 | 146.884 → 144.927 | −1.06% | 8.71 → 8.80 |
| time / small-no-first-line / family 0 | 151.563 → 149.468 | −1.52% | 8.77 → 8.80 |
| time / small-no-first-line / family 3 | 149.119 → 145.556 | −2.58% | 8.79 → 8.79 |
| time / small-no-first-line / payload | 551.633 → 522.089 | −5.22% | 9.45 → 9.41 |
| single / preserved-whitespace-first-line | 713.323 → 723.939 | +1.38% | 9.11 → 9.21 |
| single / large-first-line | 54,165.602 → 53,296.945 | −1.66% | 124.00 → 123.94 |
| single / ruby-annotation-first-line | 654.723 → 669.461 | −1.34% | 8.74 → 8.84 |
| font / 16 faces / distinct / 20反復 | 1,118.981 → 1,094.674 | −3.54% | 8.36 → 8.21 |
| font / 16 faces / repeat / 20反復 | 86.677 → 77.388 | −11.73% | 8.18 → 8.14 |
| font / 16 faces / native metadata / 20反復 | 3,351.563 → 3,292.125 | −1.31% | 8.28 → 8.15 |
| memo / 文字列なし / 16 build | 515.246 → 499.948 | −3.13% | 7.64 → 7.70 |
| memo / 64 KiB文字列 / 16 build | 1,282.554 → 753.554 | −38.71% | 9.00 → 8.04 |
| memo / 1 MiB文字列 / 16 build | 12,351.809 → 5,697.989 | −54.80% | 28.66 → 13.68 |

初回にwall増加が出た3条件は、同じ実行ファイルでさらに31ペアを測定した。
先の11ペアとは混ぜず、独立した追加campaignとして記録する。

| 再測定した初回条件 | wall µs 前→後 | ペア比の差 | peak RSS MiB 前→後 |
| --- | ---: | ---: | ---: |
| small-no-first-line / family 1 | 354.880 → 373.738 | **+3.77%** | 8.68 → 8.66 |
| small-first-line / family 0 | 593.470 → 594.657 | −0.13% | 8.75 → 8.79 |
| preserved-whitespace-first-line | 676.026 → 663.314 | +0.23% | 9.16 → 9.20 |

通常段落・family 1の初回は、両campaignでwall中央値の増加が出た。
他の2条件の増加は再測定でほぼ同等となった。初回のばらつきは大きく、
例えば最初の通常段落・family 1ではペア比が0.862–1.294だった。
一律に高速化したとは結論しない。Callgrind Irが減っても、wall増加を否定できない。
初回wallの増加を3箇所のどれかに帰属させる測定までは行っていない。

RSSの大きな差は、不要な文字列をmemoに残す入力で確認できた。
16件を保持した時点のVmRSSも、64 KiBでは9.00 → 8.04 MiB、
1 MiBでは28.66 → 13.68 MiBだった。これらは実際のContext保持中の読み取りで、
単なる解放後の残留量ではない。通常入力のRSS差は概ね±0.15 MiB以内で、
全体的なメモリ削減の主張には使わない。

主campaign 462プロセスと追加186プロセスの全648プロセスで出力が一致した。
予備確認の2プロセスは集計に含めていない。プローブだけを追加した測定であり、
ライブラリの3ファイル差分は測定前後で同一。

native測定の[全結果と再現コード](memory-native-2026-10-15.json.gz)は85,183 bytes、SHA256:

```text
db5ac10f398200dfae53238e95f02223c9847d7064fce5a615dd5b7c103362cc
```

測定ディレクトリ直下の `source/` へ基準コミットを展開し、このJSONの
`primary.metadata.sources` を `source/dev/bench/examples/` 以下へ配置する。
先のCallgrind用ソースを別途適用する必要はない。
同じパスからbeforeをビルドして退避し、`primary.metadata.patch` を適用してafterを
ビルドする。release設定は上記と同じで、`--features allocation-counting` を外し、
`--example metric_memo_native` もビルド対象へ追加する。
`drivers` に保存した `measure.py` / `repeat.py` が、同じディレクトリの
`before-{whitespace,font,memo}` / `after-{whitespace,font,memo}` と `metadata.json`
を使う。metadataは `primary.metadata` から書き出せる。

## 証拠と再現

[圧縮結果JSON](memory-simplify-2026-10-15.json.gz)に全サンプル、入力・出力、各scopeの
割り当て量、基準コミット、正確な差分、実行ファイル・probe・font・lock・設定の
hash、比較ドライバ、Callgrindの関数別注釈と一時probeの全文を保存する。
既存probeは変更していない。

圧縮JSONは632,690 bytes、SHA256:

```text
dad094a6be14b16bdc5ee5e08e5f463d508b8f4289ba33d9f49df936367dc69c
```

基準コミットを隔離コピーへ展開し、以下の設定でbeforeをビルド・退避する。
同じパスへ記録中のpatchを適用し、afterをビルド・退避する。

```sh
TMPDIR="$HOME/tmp" CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_DEBUG=1 \
  CARGO_PROFILE_RELEASE_STRIP=none \
  cargo build --locked --offline --release -p shodo-bench \
    --features allocation-counting --example whitespace_flags \
    --example font_candidate_prep --bin shodo-probe
```

保存した比較ドライバは、同じディレクトリの `metadata.json` と
`before-{whitespace,font,probe}` / `after-{whitespace,font,probe}` を使う。
metadataは圧縮JSONの `metadata` オブジェクトから書き出せる。
コマンド単位でも再現できる。

```sh
taskset -c 2 ./before-whitespace alloc
taskset -c 2 ./before-font alloc 16 distinct 20
taskset -c 2 ./before-font alloc 16 repeat 20
taskset -c 2 ./before-font alloc 16 native 20
taskset -c 2 ./before-probe --memory latin-short 1
taskset -c 2 ./before-probe --memory many-short-latin 64
```

同じコマンドでafterも実行し、条件と出力の一致を確認してから比較する。
一時ファイルは `~/tmp` を使う。測定内容と検証結果をこの記録へ保存した後、
今回のコピーしたbinary・ログ・比較用一時ディレクトリを削除する。
元からあるユーザーの未追跡ファイルには触れない。

## 今回見送った候補

- `line/hyphen.rs` の1文字 `to_string()` はスタック上のUTF-8 bufferに置換可能。
  適用・性能測定はしていない。今回の3箇所とは別の候補評価経路である。
- `analysis/units.rs` の `glyphs.len() + items.len()` の先行確保は、
  大きなgraphemeを1Unitへ併合しても余剰容量を保持する可能性がある。
  grapheme数で抑える案は、style/run分割での再確保増加を比較してから判断する。

## 検証

Rust 1.91.0、`TMPDIR="$HOME/tmp"`、全cargo検証は `--offline`。

| コマンド | 結果 |
| --- | --- |
| `cargo test --workspace --all-features --offline` | 110 suite、2,054件成功、9件ignored、失敗0 |
| `cargo test -p shodo --no-default-features --offline` | 36 suite、1,477件成功、9件ignored、失敗0 |
| `cargo test -p shodo --no-default-features --features complex-scripts --offline` | 36 suite、1,480件成功、9件ignored、失敗0 |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --offline` | 成功 |
| `cargo fmt --all --check` / `git diff --check` | 成功 |

3観点の読み取り専用レビューでコードと測定値を確認した。
