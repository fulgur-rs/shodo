# shodo-zb0.8: Ruby content restoration InputCost scan

## 判断

`ParagraphBuilder::from_ruby_content` の単一集計案は採用しない。RawItem の訪問数は減るが、固定フォントの release build で速度の再現性ある改善を確認できなかった。深さ検査と style bytes 検査の順序、制限エラーの値、復元内容の同値性を確認するテストと、再計測用 probe は残す。

候補が一貫して遅いとまでは結論づけない。今回の測定では採用条件を満たす速度改善の証拠が得られなかった。

## 方法

- baseline は `e1156ee77d99123e3377e8c48d4b4ddd4b88bbcd`。candidate は `from_ruby_content` 内で `InputCost::content` の結果を一度だけ計算する試験変更。
- baseline/candidate は別々の release バイナリとして構築し、SHA-256 はそれぞれ `072b59c5ac3d2de21f4ab3b1ec1e11ade1cccf3410c959a354ff4939ccbd46a5` と `af30db4e8e20505c3f197a95c3861859738deb1e8b86ad83b8f7df3013355598`。
- `dev/bench/examples/ruby_content_restore.rs` で固定 CJK フォント、16 annotation lanes を持つ Ruby 段落を作成した。RawItem 数、入れ子深さ、style 数をそれぞれ変えた9ケースを測定した。
- 1サンプル内で同じ条件の `builder.build()` を32回実行し、その平均時間を記録した。6サンプルを ABBA 順（baseline/candidate/candidate/baseline、向きを交互に反転）に測定した。fixture 構築、出力 digest、`break_all`、warning 取得は計時範囲外。
- baseline/candidate の build output SHA-256 と build/layout warnings は全ケースで一致した。全測定値と環境情報は raw JSON に記録した。
- rustc 1.96.0、AMD Ryzen 5 5600G、Linux x86_64。CPU frequency scaling は有効。

## 時間

`Δ` は candidate と baseline の中央値の差。正値は candidate が遅く、負値は candidate が速い。

| ケース | RawItems / input | Styles / input | baseline ns/build | candidate ns/build | Δ | candidate が速い比較 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| input length 8 | 17 | 1 | 654,205 | 664,733 | +1.61% | 2/6 |
| input length 128 | 257 | 1 | 1,277,928 | 1,282,175 | +0.33% | 2/6 |
| input length 512 | 1,025 | 1 | 2,749,443 | 2,726,589 | −0.83% | 5/6 |
| nested depth 1 | 3 | 1 | 463,637 | 466,818 | +0.69% | 1/6 |
| nested depth 8 | 17 | 1 | 515,201 | 521,629 | +1.25% | 0/6 |
| nested depth 32 | 65 | 1 | 636,519 | 638,407 | +0.30% | 3/6 |
| style count 2 | 3 | 2 | 493,676 | 496,868 | +0.65% | 3/6 |
| style count 9 | 17 | 9 | 726,960 | 731,247 | +0.59% | 0/6 |
| style count 33 | 65 | 33 | 1,464,867 | 1,473,167 | +0.57% | 0/6 |

9ケース中8ケースで candidate の中央値が遅かった。input length 512 の改善は0.83%で、他の入力長、深さ、style 数では改善がそろわなかった。

## 集計回数と意味の同値性

候補は `from_ruby_content` 内の RawItem と style 配列の集計を2回から1回に減らす。`ruby::prepare::prepare` の lane preflight にある集計は別に残るため、build 中の RawItem 集計は通常 input で合計3回から2回になる。訪問数の差だけでは build 時間の効果を示さなかった。

追加したテストで、次の既存契約を固定する。

- nesting depth と style bytes が同時に超過した場合、`NestingDepth` を先に返す。
- nesting/style byte の境界と `LimitKind`, `actual`, `limit` を維持する。
- text、元の items、wrapper marker、normal/first-line style mapping、offset mapping、warning 順を保って復元する。

## 再計測

```sh
cargo run --release -p shodo-bench --example ruby_content_restore -- 0
```

引数はサンプル番号。A/B 比較では同じ probe を baseline と候補の両方で release build し、両者の target directory と実行ファイルを分離する。
