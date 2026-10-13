# SHODO-34: overflow glyph 境界の共有キャッシュ

対象は `Line::overflow_rect()` のフォント glyph 境界取得。基準 revision は
`c892cae`。矩形全体の遅延保持は導入せず、各呼び出しで fragment と配置から従来と
同じ順番・浮動小数点演算で矩形を合成する。

## 実装と保持上限

- `FontCollection` の所有レイヤーに独立した `Mutex` を置く。共有レイヤーの顔は
  document collection からも所有レイヤーへ経路を戻し、paragraph 間で再利用する。
- キーは `FontId`、実際に使用する font size の `f32::to_bits()`、正規化座標の
  全スライス、glyph ID。サイズを丸めない。座標の末尾・長さも比較する。
- glyph 境界の `Some(BoundingBox)` と `None` を保持する。skew、RTL 反転、位置、
  emphasis、ruby transform は保存した境界へ従来どおり適用する。
- レイヤーごとに最大 16 instance、instance ごとに最大 1,024 glyph、最大 64 軸。
  65 軸以上は保存せず従来計算へ戻す。instance の 17 件目は FIFO で追い出す。
  glyph の 1,025 件目はその instance の表を空にしてから追加する。
- 合計保存数は最大 16,384 glyph と 1,024 個の 16-bit 座標。HashMap の保持容量も
  1,024 glyph 挿入時を越えて拡大しない。テストでは instance 数、glyph 数、
  座標長、および表の容量が 2,048 以下であることを確認する。
- キャッシュは数値と ID だけを保存し、font blob、paragraph、Line、描画側の
  owner を追加保持しない。フォント選択の generation が変わっても既存 face の
  immutable identity と bytes は変わらないため、境界の invalidation は不要。
- catalog から font data を取得してから境界 cache をロックする。ロックは glyph
  ごとではなく GlyphRun ごとに一度取得し、その scope から ruby へ再帰しない。
  FontRef が得られない防御経路でも fallback metrics は必要時だけ guard の前に
  一度取得するため、glyph ループ内から catalog を取得しない。
  不明なレイヤーは非保存経路を使う。poison は既存 font lock と同じ方針で回復する。

## 正しさと性能契約の検証

最初に実 `skrifa::GlyphMetrics::bounds()` の呼び出しを数えるテストを追加した。
反復する 4 glyph に対して基準コードは 4 回、期待値は 1 回なので RED になった。
キャッシュ導入後は TrueType と CFF のどちらも 1 回で、別 paragraph と反復 query
でも追加計算されず GREEN になった。

追加テストは次を検証する。

- 不正 glyph ID の実 `bounds()` が返す `None` の再利用。
- 別 layer/index、サイズの隣接ビットと signed zero、座標長・末尾値、glyph ID の
  分離。保存結果は直接取得した skrifa 境界の 4 成分のビットと比較する。
- glyph/instance の上限と追い出し後の再計算、64 軸を越す入力の保存拒否。
- cache bypass を使った従来計算と矩形 4 成分の bit 一致。TrueType/CFF/Arabic、
  LTR/RTL、横書き/縦書き、synthetic oblique、emphasis を組み合わせる。
- 温めた Line の clone に ellipsis を適用しても従来計算と一致し、元 Line が
  変わらない。ルビの子 Line の再帰矩形も比較する。
- 実 `fvar`/`gvar` を持つ emoji-mono fixture の weight 300/400/700 と複数サイズ。
  default の all-zero 正規化座標は empty slice になる。Latin fixture では
  font-size-adjust によって authored size と実 size が異なる経路も検証する。
- 共有 face を別 document の Line で再利用し、未保存の glyph を 4 スレッドから
  問い合わせる。blob strong count は増えず、最後の Line/collection を落とすと
  layer observer と blob weak observer が解放を確認する。
- FontRef と保存済み run metrics がない防御経路では、4 glyph の catalog 取得
  5 回に対して期待 2 回で RED を確認した。guard 前の precompute 後は 2 回で
  GREEN。stub metrics の矩形 4 成分の bit は同じまま。

`test -p shodo --lib outline` は 11 tests 成功（既存 punctuation tests 2 件を含む）。
最終 guard 調整後の `text_emphasis` / `text_overflow` は合計 22 tests 成功。

guard 調整前の全体検証は次の結果だった。失敗はなく、ignored は既存の測定用
試験等である。最終 guard 調整後の全体再リンクを重複実行せず、最終 PR exact HEAD
の CI（check / MSRV / accesskit の full suite）を親 task が確認する。その証拠の
確認まで、統合・マージ完了を宣言しない。

| command | 成功 | 失敗 | ignored |
| --- | ---: | ---: | ---: |
| `test --workspace` | 1,920 | 0 | 9 |
| `test --workspace --features shodo-harness/accesskit` | 1,936 | 0 | 9 |

最終コードの feature 分離は次の結果。通常の library/integration/doc tests を含む。

| command | 成功 | 失敗 | ignored |
| --- | ---: | ---: | ---: |
| `test -p shodo --no-default-features` | 1,382 | 0 | 9 |
| `test -p shodo --no-default-features --features complex-scripts` | 1,385 | 0 | 9 |

最終コードの次の確認もすべて exit 0。

- `clippy --workspace --all-targets -- -D warnings`
- `clippy --workspace --all-targets --features shodo-harness/accesskit -- -D warnings`
- `fmt --all --check` と `git diff --check`

Cargo は stable（Rust 1.97.1）、`RUSTC_WRAPPER=`、`CARGO_BUILD_JOBS=3`、
`TMPDIR="$HOME/tmp"`、専用 `CARGO_TARGET_DIR` を使った。環境固有の kache runner を
避けるため `--config 'target.x86_64-unknown-linux-gnu.runner="env"'` を指定した。
測定と並走する確認は CPU 3〜11、後半は測定 CPU 2 の SMT 兄弟 8 も除く
CPU 3〜7,9〜11 に固定した。

## 性能の再現

`dev/bench/examples/outline_bounds.rs` は固定 fixture を使い、paragraph と Line の
構築を測定の外に置く。初回 query の `cold_ns` と反復 query の平均
`warm_ns_per_query`、矩形の `rect_bits` と checksum を JSON で出力する。

```sh
TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_BUILD_JOBS=3 \
  cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' \
  run --release -p shodo-bench --example outline_bounds -- cjk 512 200
```

同じ example を基準 revision と修正 revision に置き、同一 CPU・同一 toolchain・
同一 feature で `latin` / `cjk` / `arabic` をそれぞれ測る。独立した target directory
を `~/tmp` 下に置き、ビルドとテストの実行が終わってから CPU affinity を固定して
複数回実行する。矩形 bits と checksum が一致することを先に確認する。
初回 query は反復 glyph の同一 run 内での reuse を含む。

cache hit でも既存の font data 取得・font reference/glyph metrics 作成は残る。
キャッシュが再利用するのは境界計算だけなので、CFF/variation の outline 遍歴に
効果がある一方、小さな TrueType query は共有ロックと表検索の費用が支配しうる。
上限を越える instance/glyph の churn や同じレイヤーへの並行 query は正しさを
維持するが、通常の温めた query と同じ高速化を保証しない。

## 暫定 A/B 測定

親 task の共通測定 driver で `c892cae` と本修正の凍結コードを比較した。
Rust 1.97.1、release opt-level=3、`complex-scripts`、system fonts 無効、
AMD Ryzen 5 5600G の CPU 2 に固定。独立 3 process × 21 sample、warmup 3 回。
既存 Line への overflow query と結果 Vec の収集/drop を含み、検証は時間計測外。

| workload | scale | 基準中央値 | 修正中央値 | 修正 / 基準 |
| --- | ---: | ---: | ---: | ---: |
| japanese-short overflow | 1 | 169.0186 µs | 7.0985 µs | 0.0420 |
| japanese-long overflow | 8 | 15.533852 ms | 0.618122 ms | 0.0398 |
| japanese-long overflow | 64 | 134.502910 ms | 5.395845 ms | 0.0401 |

反復 overflow は約 96% 時間を削減した。対照 layout の時間比は short/1 が
1.0204、long/64 が 0.9801。rect bits、layout digest、warnings はすべて一致し、
別の 813 件の revision 横断検証も同一結果になった。温めた overflow の allocation
は両 revision とも結果 Vec の 1 件だけで、各 scale の 48 / 2,560 / 20,480 bytes
も同じ。保存された live bytes の増分は各 scale で +1,976 bytes と一定だった。

この測定時は兄弟課題の build/tests が CPU 3〜11 で動作していた。A/B 内の比較に
限定し、以前の性能調査の絶対時間とは比較しない。親 task が静穏時の再測定と
共通 JSON artifact の保存を行う。最終判断ではその結果も確認する。

一時 build/log は `~/tmp/shodo-34-target` と `~/tmp/shodo-34-checks`。
最終結果を本記録へ移した後、親 task が他課題の成果物と区別して片付ける。
