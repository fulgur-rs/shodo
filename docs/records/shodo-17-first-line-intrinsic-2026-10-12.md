# SHODO-17: first-line intrinsic の二走査を単一走査にまとめる

2026-10-12。ベースラインは `654c789290215b52f1e71a39357cc64f94182001`、
実装revisionは `b5404e95b461403e60eab8671d85ec2607bb1470`。
測定条件、全サンプル、出力、バイナリとベンチマークソースのSHA-256は
[JSON記録](shodo-17-first-line-intrinsic-2026-10-12.json)に保存した。

## 変更と適用範囲

`intrinsic_sizes` は first-line があると min-content と max-content を別々に
測っていた。通常段落とfirst-line段落のunitが位置ごとに対応する単純な
横書きLTRテキストでは、normal/alternate両方の幅を同じunit走査で加算する。
min-contentは最初の空白区切りまでalternate幅を使い、そこからnormal幅へ
切り替える。max-contentは最初の強制改行までalternate幅を使い、その後normal幅を
使う。

paint-onlyの一般経路では、安全性を段落構築時に一度判定し、first-line dataに
キャッシュする。構築後のparagraph dataは不変なので、`intrinsic_sizes`内で
unit列を事前走査せず、測定時に一回だけ走査する。実行時の`LineOptions`と警告状態も
従来どおり確認する。回帰テストは測定呼び出し中の総unit訪問数を数え、最適化経路が
段落のunit数と同数、強制参照経路が2倍になることを検証する。

特殊化経路はデフォルトの`LineOptions`、先頭・末尾に空白がない英数字・半角空白・
改行テキスト、ゼロのtracking・word spacingを要求する。一般のpaint-only経路も、
非ゼロ/hanging/each-line indent、負advance、加算上限、Ruby、inline box、float、
text-combine、共有ligature cluster、unsafe-to-break/concat、非対応break class、
spacing処理がある場合は従来の二走査へ戻る。候補edge reshape、警告、reshape budgetの
消費順を保つための保守的な範囲である。

負advance、glyph加算途中のsaturation、tracking飽和、unsafe edge、改行、末尾空白、
非有限indentと警告抑制順を含む回帰テストは、min/max幅のビット列と警告列を参照経路と
比較する。

## 性能測定

- AMD Ryzen 5 5600G、Linux x86_64、12論理CPU。Rust/Cargo stable 1.97.1。
- release build、4000語、各ケース9サンプル。3巡をwarm-upに使い、測定巡は
  ケース順を回転した。段落構築と`LayoutContext`作成は計測区間外。
- ベースラインと候補には同一ソース（SHA-256: `0c9eaebec374e93edb6ca9213f44ac5a9b887efc4d7b57d6855ad8c2f1775e76`）を使い、
  別々のCargo target directoryでビルドした。実行は`kache test-runner`経由。
  CPU affinityは固定していないため、数値はこのマシンでの観測値である。
- 計測対象は`Paragraph::intrinsic_sizes`のみ。安全性キャッシュを作る段落構築コストは
  含まれず、この測定はintrinsic呼び出しの改善を示す。構築時判定のコストを含む
  end-to-endの変化は測っていない。
- intrinsic幅と警告は、すべてのケースでベースラインと一致した。

| ケース | ベースライン中央値 | 候補中央値 | 候補 / ベースライン | 出力の要約 |
| --- | ---: | ---: | ---: | --- |
| first-lineなし（英文） | 11.436 ms | 11.626 ms | 1.017 | `60.6875 / 167250.48`, 警告なし |
| first-line同一スタイル（英文） | 23.177 ms | 23.681 ms | 1.022 | `60.6875 / 167250.48`, 警告なし |
| first-line paintのみ（unsafe英文） | 23.376 ms | 23.868 ms | 1.021 | `60.6875 / 167250.48`, 警告なし |
| first-lineなし（単純数字テキスト） | 0.938 ms | 0.914 ms | 0.974 | `36.625 / 94448.97`, 警告なし |
| paint-only（単純数字テキスト） | 1.980 ms | 0.280 ms | 0.141 | 同上 |
| first-lineなし（末尾空白付き） | 0.930 ms | 0.915 ms | 0.985 | 同上 |
| paint-only（末尾空白付き、一般経路） | 1.983 ms | 1.055 ms | 0.532 | 同上 |
| font-size変更（単純数字テキスト） | 1.976 ms | 0.282 ms | 0.143 | `36.625 / 129783.34`, 警告なし |
| font-size変更（ligatureを含む英文） | 23.188 ms | 23.353 ms | 1.007 | `60.6875 / 229931.4`, 警告なし |

単純数字テキストのpaint-only特殊化経路は約7.08倍、末尾空白で特殊化を避けた
paint-only一般経路は約1.88倍だった。英文の`same`、`paint`、`size-ligature`はunsafe
shaping境界があり、従来の二走査にフォールバックするため、この計測では大きな変化は
なかった。ここに記した比率は短いローカル測定であり、一般的な速度保証ではない。

## 検証

- `cargo +stable test --workspace --quiet`: 成功。shodo libは863 passed、8 ignored。
- `cargo +stable test --workspace --features shodo-harness/accesskit --quiet`: 成功。
  shodo libは865 passed、8 ignored。
- 通常構成とaccesskit構成で`RUSTFLAGS="-D warnings" cargo +stable clippy --workspace --all-targets`: 成功。
- `cargo +stable fmt --all --check`、`git diff --check`: 成功。

## 再現

ベースラインrevisionを作業用snapshotに用意し、候補に含まれる同じベンチマーク例を
コピーしてから、それぞれ別のtarget directoryを指定する。

```sh
export TMPDIR="$HOME/tmp"

CARGO_TARGET_DIR="$HOME/tmp/shodo-17-baseline-target" \
  cargo +stable run --release -p shodo-bench \
  --example first_line_intrinsic_scale -- 4000 9

CARGO_TARGET_DIR="$HOME/tmp/shodo-17-intrinsic/target" \
  cargo +stable run --release -p shodo-bench \
  --example first_line_intrinsic_scale -- 4000 9
```

一時baseline snapshot、target directory、ログは測定値と判断をこの記録に保存した後に
片付ける。
