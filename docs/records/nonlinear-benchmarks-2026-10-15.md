# 非線形処理の修正前後: wall time / Callgrind / RSS

2026年10月15日。[非線形アルゴリズムの修正](nonlinear-algorithms-2026-10-15.md)
の2箇所を対象に、同じ入力で修正前後を比較した。

通常インラインの深さ512・改行512では、wall timeは69.5%、Callgrind Irは
73.6%減少した。ルビの成功hitは深さ16/32でwall timeが約5%減ったが、
深さ1では3.7%増えた。RSSに一貫した削減傾向はなく、ルビの変更は浅い
ケースも含めて無条件に速くなるものではない。

## 測定条件

- 修正前: `2dd66f9f7c85bbcdc26fa1fbe62d14636f931d2f`。
- 修正後: 同じコミットへ現在の5ファイルの作業差分を適用。計測される
  ライブラリの変更は `analysis/whitespace.rs`、`hit/mod.rs`、`ruby/hit.rs`。
  残る2ファイルは別の開発ツールのClippy修正で、このprobeでは実行されない。
- 両方へ同一の
  [Rust probe](../../dev/bench/examples/nonlinear_algorithms.rs)を追加。
  同じ隔離ソースパス、同じ `Cargo.lock` と依存キャッシュから順にビルドし、
  それぞれの実行ファイルを別名で保存した。作業ツリーは退避・巻き戻ししていない。
- AMD Ryzen 5 5600G、Linux x86_64、論理CPU 2固定。Rust/cargo 1.91.0、
  LLVM 21.1.2、Valgrind 3.25.1。
- release opt-level=3、debug=1、strip=none、incremental無効。
  `shodo-bench` の既定 `complex-scripts` を有効化し、ライブラリの既定機能は無効。
  システムフォント列挙は無効で、既存の固定実フォントを使用。
- nativeはケースごとに11ペアの独立プロセスを実行し、before→afterと
  after→beforeを交互にした。各ペアの反復回数は同じで、約150 msを目標に
  事前校正した。ビルド・Clippy等を同時実行した予備測定は集計に含めていない。
- 各プロセスで5回warmupした後、同じ関数境界を `Instant` で測定。
  nativeのプロセス全体wall timeも別に保存した。
- Callgrindは各ケース・版で2個の独立プロセスを実行。
  `--collect-atstart=no` と `--toggle-collect=shodo_whitespace_measure` /
  `--toggle-collect=shodo_ruby_measure` により対象ループだけを集計した。
  cache/branch simulationは無効。全ケースで2回のIr総数は完全に一致した。
  Callgrind実行中の時間やRSSはnative比較へ混ぜていない。

## 対象と検証範囲

`ws-D-B` は深さDの通常インラインに `a`・改行・`b` の3文字を
B回繰り返した入力。`mixed-D-B` はその外側と最内側の2スコープだけを
Isolate / Embedにしている。測定範囲にはbuilder生成、style複製、`analyze`、
前の反復結果の解放を含む。改行処理単体のmicrobenchmarkではない。
最後の結果だけを測定後にshapeし、processed text、offset mapping、warningsを
ハッシュ化・比較した。全反復でanalysis成功を要求する。

`ruby-D` は深さDの可視ルビに対する最深注釈への成功hit。入力・レイアウト・
遅延索引構築は測定前に済ませ、queryと返されたpathの確保・解放を測定した。
path全体、最深注釈、parent line、HitResultを測定前後および全プロセスで照合した。
固定フォントのglyphが実在することも確認した。全glyph・全geometryを比較する
ためのharnessではなく、それらの回帰検証は既存テストが担う。

初期候補の `ruby-64` は修正前から既定のruby line work予算を超えるwarningを
生じたため除外し、既定設定で正常に成立する深さ1/8/16/32を使った。

## 結果

wallは1操作あたりµs、Irは1操作あたり実行命令数。各値は中央値。
wallの差は11個のafter/before比の中央値なので、表中の独立した中央値同士を
割った比と完全には一致しない。RSSはnative各プロセスの `VmHWM` の中央値。

| ケース | wall µs/op 前→後 | wall差 | Callgrind Ir/op 前→後 | Ir差 | peak RSS MiB 前→後 |
| --- | ---: | ---: | ---: | ---: | ---: |
| ws-0-512 | 152.429 → 153.729 | +0.8% | 3,017,684 → 3,011,610 | −0.2% | 8.42 → 8.21 |
| ws-32-32 | 24.583 → 22.153 | −9.4% | 412,040 → 357,963 | −13.1% | 7.48 → 7.45 |
| ws-128-128 | 113.058 → 73.233 | −35.3% | 2,151,335 → 1,283,562 | −40.3% | 7.87 → 7.79 |
| ws-512-512 | 907.774 → 276.497 | −69.5% | 18,876,944 → 4,985,427 | −73.6% | 9.29 → 9.09 |
| mixed-128-128 | 254.214 → 212.151 | −16.6% | 4,340,600 → 3,481,469 | −19.8% | 8.30 → 8.33 |
| ruby-1 | 0.082 → 0.085 | +3.7% | 1,026 → 1,054 | +2.7% | 7.07 → 7.04 |
| ruby-8 | 0.379 → 0.378 | −0.7% | 3,640 → 3,574 | −1.8% | 7.59 → 7.55 |
| ruby-16 | 0.712 → 0.678 | −4.9% | 6,136 → 5,876 | −4.2% | 8.22 → 8.28 |
| ruby-32 | 1.435 → 1.361 | −5.0% | 10,854 → 10,044 | −7.5% | 9.51 → 9.56 |

通常スコープの改行処理は、DとBを増やすほど効果が大きい。
`ws-512-512` の5反復のCallgrindでは、修正前の
`open_bidi_scopes` / `close_bidi_scopes` 本体だけで40,724,480 Ir、さらに両関数の
slice index等にも費用があり、修正後にはこれらの全通常スコープ走査が消えた。
全測定区間は94,384,721 → 24,927,136 Irとなった。

ルビの深いケースには効果があるが、深さ1では最終反転に伴う固定費が残る。
Irも1,026 → 1,054と増え、浅いケースのwall増加を一律にノイズとして扱う根拠はない。
深さ8のwall差はほぼ同等。深さ16/32の約5%削減も、この固定入力・環境での結果。
各ペアの比のmin/maxも保存しており、例えばruby-32では0.916–1.077と
時間にばらつきがある。命令数の削減率とwall timeの削減率は同一ではない。

RSSはプロセス全体の高水位で、入力構築・warmup・計測後のfontロードやshape検証を
含む。操作専用の追加メモリを表すものではない。通常スコープには−0.2 MiB程度の
差もあるが、混在スコープや深いルビでは微増するため、メモリ削減の結論は出さない。
別途保存した `VmRSS` は主要fixture/outputのdrop後の残留値であり、操作中の
常駐量としては使わない。`wait4` の子プロセス最大RSSも補助値として保存した。

## 保存した証拠と再現

[圧縮結果JSON](nonlinear-algorithms-2026-10-15.json.gz)は67,668 bytes、SHA256:

```text
ddc7598d11f94b45a519451737f07caa84792d4c7688465fc5e11e6482026797
```

全native/Callgrindサンプル、プロセス時間、RSS、実際の反復回数、出力検証値、
Callgrindの関数別注釈、before/afterのbinary hash、lock/harness/runner/config hash、
基準コミットと適用差分、計測コードの全文、機械・toolchain・profileを含む。
公開統計は198 nativeプロセスと36 Callgrindプロセスに基づく。

再現時はこのJSONのmetadataから基準コミットを `git archive` し、
`harness_source` を `dev/bench/examples/nonlinear_algorithms.rs` へ配置する。
同じパスからbeforeをビルドして退避し、metadataの `patch` を適用してafterを
ビルド・退避する。必要なビルド設定とコマンドは以下。

```sh
TMPDIR="$HOME/tmp" CARGO_INCREMENTAL=0 \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_DEBUG=1 \
  CARGO_PROFILE_RELEASE_STRIP=none \
  cargo build --release --locked --offline -p shodo-bench --example nonlinear_algorithms
```

同じprofileで生成した両binaryのSHA256をmetadataに設定してから比較する。
以降の出力ディレクトリは新規のものを使う。

```sh
TMPDIR="$HOME/tmp" python tools/bench/compare_nonlinear.py \
  --before "$task_dir/bin/before" --after "$task_dir/bin/after" \
  --metadata "$task_dir/build-metadata.json" --output "$task_dir/results" \
  --cpu 2 --samples 11 --callgrind-samples 2 --target-ms 150
```

Callgrindの区間は上記2関数のenter/leaveで指定する。setup・warmupは区間外。
再現用コードは保存し、隔離コピー、release build生成物、コピーしたbinary、
予備測定、個別ログ、生のCallgrind graphは、この記録と圧縮JSONを検証した後に削除する。
元から存在するユーザーの未追跡ファイルには触れていない。
