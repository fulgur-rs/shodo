# `Cursor::summary` の cold cache と浅い階層の実時間測定 (`shodo-8u4`)

2026-09-30 に、base `c4e34bd9216464b52d85c46c2bee463121a80ae0` と cache 追加後の `edaebb3ac0c23b98cc0a061d0b4a2ad46df0beee` を比較した。対象は `Cursor::summary` だけであり、段落全体の処理時間を表す値ではない。生データは [shodo-8u4-raw.jsonl](shodo-8u4-raw.jsonl) にある。

## 方法

- 同じ [probe](../../tools/bench/spacing_summary_probe.rs) を [installer](../../tools/bench/install_spacing_summary_probe.py) で両方の履歴版へ入れた。installer は `Cursor` のテスト専用 `visits` 計数をこの probe のビルドに限って無効化し、製品コードの `summary` / `push` 本体を変えない。履歴版の通常のテストモジュールも、この計測用 feature ではコンパイルしない。
- 各 depth 1, 4, 127 について、`first` は別々の Cursor 512 個の初回取得、`repeat` は一度読んだ同じ Cursor と `None` context で 50,000 回の反復、`after_push` は別々の Cursor 512 個を一度読んでから `push` した直後、`context_switch` は別々の Cursor 512 個を context A で読んでから context B で読む操作である。両履歴版で順序と入力は同じ。返り値の `Summary` 全体を `black_box` に渡し、`cost` の合計と全フィールドの期待値を照合した。全 960 sample で一致した。
- 各実行で 12 組を各 20 trial。実行順は base → changed → changed → base (ABBA) とし、各組 40 sample の中央値を示す。計測ループの結果は `black_box` に渡す。`setup_ns` は Cursor 準備と、必要な事前読み取り・`push` を含む。`loop_ns` は呼び出し抜きの別ループ、`operation_ns` は `summary` を呼ぶループで、両者は別々に測った。`loop_ns` を `operation_ns` から機械的に差し引いていない。
- Linux 7.2.5-3-omarchy、AMD Ryzen 5 5600G、x86_64、CPU governor `performance`、rustc/cargo 1.96.0、release profile、`--offline --locked`。各履歴版には独立した `CARGO_TARGET_DIR` を使い、依存関係は [保存した Cargo.lock](shodo-8u4-Cargo.lock) で固定した。計測時にはビルドを走らせていない。

## 結果

`operation` と `loop` は 1 回あたりの ns、`setup` は trial あたりの µs。いずれも 40 sample の中央値で、端数は表示時に丸めた。

| depth | 操作 | base operation | base loop | base setup | changed operation | changed loop | changed setup |
| ---: | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | first | 7.5 | 2.9 | 15.7 | 10.1 | 2.9 | 22.5 |
| 1 | repeat | 8.9 | 0.3 | 1.4 | 9.6 | 0.3 | 1.5 |
| 1 | after_push | 7.5 | 2.9 | 25.7 | 9.8 | 2.9 | 33.6 |
| 1 | context_switch | 8.3 | 2.9 | 17.3 | 12.0 | 2.9 | 24.9 |
| 4 | first | 67.0 | 2.9 | 59.2 | 64.2 | 3.3 | 66.6 |
| 4 | repeat | 71.2 | 0.3 | 2.0 | 9.5 | 0.3 | 1.9 |
| 4 | after_push | 67.0 | 2.9 | 108.0 | 64.3 | 3.6 | 124.5 |
| 4 | context_switch | 67.2 | 2.9 | 91.1 | 66.1 | 2.9 | 97.3 |
| 127 | first | 2642.6 | 3.7 | 4127.4 | 2513.2 | 3.8 | 2308.6 |
| 127 | repeat | 2709.3 | 0.3 | 10.3 | 9.6 | 0.3 | 7.2 |
| 127 | after_push | 2634.7 | 3.7 | 5725.6 | 2454.8 | 3.8 | 4035.5 |
| 127 | context_switch | 2657.6 | 3.7 | 5501.9 | 2431.6 | 3.7 | 4516.6 |

同じ context で反復した depth 4 / 127 では cache により再計算がなくなり、中央値は 71.2 → 9.5 ns / 2709.3 → 9.6 ns だった。未更新の深い Cursor を再読する際に毎回 depth に沿って走査しないという仕事量の保証と整合する。

depth 1 では changed の中央値が初回 7.5 → 10.1 ns、同一 context 反復 8.9 → 9.6 ns、`push` 後 7.5 → 9.8 ns、context 切替 8.3 → 12.0 ns だった。浅い階層では cache の定数負担を上回る再計算がない。depth 4 / 127 の初回・`push` 後・context 切替は両版で近い値だった。これらの小さな差を製品全体の改善・退行に外挿しない。セットアップ時間はバッチごとの配置や OS の影響を受けており、summary 呼び出しの時間と別に扱う。

1 呼び出しあたりの 1〜10 ns 台は単発タイマで分解した値ではなく、512 回または 50,000 回のバッチ時間から計算した平均である。個々の呼び出しの経過時間をこの精度で測れたという意味ではない。毎回異なる Cursor を使う操作には、Cursor の配置、キャッシュ、分岐の影響が残る。恒常テストに elapsed 閾値は追加していない。

この probe の `Summary` は `cost` だけを設定し、`first` / `last` は空である。したがって `context_switch` は context の同一性判定と cost-only の再計算を測っており、文字境界に依存する `ParagraphData` の処理時間は含まない。

## 再現と識別子

2 つの履歴版を別々の worktree に展開し、保存した `Cargo.lock` を各ルートへコピーして installer を実行する。その後、各ルートで次のコマンドを異なる `CARGO_TARGET_DIR` で実行する。

```sh
CARGO_TARGET_DIR=/tmp/shodo-8u4-target-base cargo test --offline --locked --release -p shodo --features bench-no-visits --lib line::spacing_summary::probe::measure_cursor_summary --no-run
```

変更版も target 名を `changed` に変える。得られたテスト実行ファイルを、両版とも次の引数で ABBA 順に走らせる。

```sh
TEST_BINARY line::spacing_summary::probe::measure_cursor_summary --exact --ignored --nocapture --test-threads=1
```

出力の `SHODO_8U4=` 行が raw sample である。`Cargo.lock` SHA-256 は `b0303bbbbec151657db2837649f07bcba0b7b8b53ebd50dc86520cb85a835918`。probe は `1a069c3fc298e63f263812b82e0b56de6d72155503a43b30d24d4746de5fab51`、installer は `0354774e86a886c5b7b21451ab322dc0eb89d8db0fe2b8bf57b5c2a363205df7`。installer 適用後の `spacing_summary.rs` は base `4e34ae4c54ed5e8398d89d8e6e1109c020867e09bf23fe441e8f143d9d3ccc7c`、changed `aaec92eea655dc69d2d4f660893e8fe2e92978bc507641d1163ebd1eee006356`。テスト実行ファイルは base `5459d134000dfea3daa4d7e0bdbb23c8b0742991dbedfc924622c03da18a1aaa`、changed `7640c88e9db83b3313e22b5ca468af9bf1641bc463b4669ebb0436b1d6b9afd7`。識別子は raw JSON の metadata にも記録した。
