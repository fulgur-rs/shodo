# SHODO-15: 段落末尾で行頭の再シェープによる幅増加を確認する

2026-10-12。変更前の実装は `68e13e27ed1fec40bc184cd0e08e873c632c02ae`。
初回修正の実装・性能測定対象は `87397ff355950407add5adda3ed5da1702add89b`。
PRレビュー後の追加修正は `85303f0d0dba1ba84606cbfe7532c7a70d01e61c` に保存した。
閉じ余白の句読点barrierを保持する追加修正は `20727f77ba96c6d91ae89797e8fc2e31b4d5e6f7`。
キャッシュ再試行とハイフン幅の追加修正は `90fb5671782a9a5118a7e3c9118596f90becd6e9`。
末尾空白後の透明要素に対する最終修正は `f41c82fa5af8eb65c4996dcf37fd499c2a3670be`。
測定結果と条件の集計は [JSON](shodo-15-paragraph-end-2026-10-12.json) に保存した。

## 再現と原因

固定 Latin フォント、16px、RTL の `BidiOverride` インライン内に
`VAVAVAVAVAVA` を置き、本文の改行を許可し、閉じ要素の直後を段落末尾にする。
幅28.125pxでは、変更前の最終行 `12..18` が28.75pxになった。
RTL のカーニングが論理的に後ろの glyph に属するため、行頭でペアを切って
再シェープすると幅が増える。

通常の走査は共有 glyph の幅だけを先に確認し、改行禁止位置での再シェープを
遅らせる。段落末尾の glyph・閉じ制御文字にも改行禁止が付くため、共有幅が
収まっていると、最終行の行頭補正を確認せずに `BreakReason::End` になっていた。
強制改行・block-in-inline の検証とは別の終端経路である。

## 修正と回帰テスト

幅による走査で、段落の最後の unit も edge-window の実測対象にする。
実際の幅が超過したら、既存の候補検証で収まる改行位置へ戻る。
途中のすべての改行禁止位置を再シェープする処理は追加しない。
文字数による走査、公開 API、強制改行の扱いは変更しない。

`paragraph_end_checks_the_reshaped_line_start_width` は、幅28.125 / 28.25 /
28.625 / 28.75px、通常・isolate の override、非入れ子・3段のインラインを
組み合わせた16ケースを確認する。全行が指定幅以下、最終行が `End`、閉じ
PDF/PDI とインラインボックスが最後の本文 `A` と同じ行に残り、警告がないことを
検証する。修正前に幅超過で失敗することを確認してから実装した。

読み取り専用のコードレビューで Critical / Important 指摘はなかった。
最後の本文と閉じ要素の同居を明示的に検証する Minor 指摘は反映した。

## 性能測定の条件と範囲

- AMD Ryzen 5 5600G、x86_64 Linux、CPU affinity `[2]`。
- Rust/Cargo 1.97.1、release opt-level 3、debug 0、incremental 無効。
- `complex-scripts` 有効、固定 font/input/lock/harness、system fonts 無効。
- latin-long / arabic-long の scale 1 / 8 / 64、7 operations、Criterion quick
  10 samples・warmup 100ms・measurement 200ms、cold は2プロセス。
- 同じ worktree と隔離した target で、回帰テスト追加後の修正前実装と修正後を
  順にビルドした。保存 executable の hash は変更前後で異なり、テスト補強前後の
  修正済み executable は同一だった。全実行中に測定対象のソースは変更しない。
- 実行時は Cargo shim を避け、`RUSTC_WRAPPER=""` とし、
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER` を unset した。

quick は修正後に2回実行した。時間計測と allocation 計測は別 executable を使う。
メモリの net / peak は scope 内の allocation で、RSS や native allocation は含まない。
この通常コーパスの出力一致は、意図的に変化する上記の RTL 再現入力を含まない。
数値は単一マシンでの測定結果であり、一般的な速度改善・無回帰を保証しない。

## 初回修正の検証結果

- 整形、Clippy `--workspace --all-targets -- -D warnings`、rustdoc warnings-as-errors: 成功。
- `cargo +stable test --offline --release --workspace --quiet`: 1,771 passed、0 failed、9 ignored。
- `--no-default-features`: 1,233 passed、0 failed、9 ignored。
- `--no-default-features --features complex-scripts`: 1,235 passed、0 failed、9 ignored。
- snapshot: 全46件 match、partial=false、更新なし、changed pixels=0。
- 行走査の15テストが成功。途中の禁止位置を再シェープしない制約、edge-window の
  再利用、行ごとの処理回数・予算、通常文・長い unsafe chain の制約を含む。
- 通常コーパス6 workloadの42 timing digest、42 memory digest が変更前後で一致。
  全scopeの保持allocationは同量。追加ピークの差は0〜2,047 bytesだった。
- Rust 1.89 と Wasm ビルドは今回実行していない。

最終 quick 比較では全行レイアウトの比率は0.978〜1.026、ページ retry は
0.952〜1.010。ラテン文の next_line は scale 1 が1.159、scale 64 が1.198だった。
同じ修正済み executable の最初の quick 実行では scale 64 が0.993であり、
短時間測定にはばらつきがある。

保存済みの変更前後の executable で、next_line を50 samples・warmup 1秒・
measurement 2秒に延長し、前→後を2組測った。scale 1 の約16%遅延はこの
2組では再現しなかった。ただし2組目の scale 64 は原因未特定の大きな遅延が
出た。その項目だけを同条件で再実行すると1.022倍だった。外れ値もJSONに
残しており、この結果から速度の無回帰を断定しない。

| scale | 長めの計測1組目 後/前 | 2組目 後/前 | scale 64のみ再計測 後/前 |
| --- | ---: | ---: | ---: |
| 1 | 1.009 | 0.994 | — |
| 8 | 1.013 | 0.988 | — |
| 64 | 1.017 | 2.245 | 1.022 |

## 再現手順

固定フォントの回帰テストは、変更後のコミットで以下を実行する。

```bash
export TMPDIR="$HOME/tmp"
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=""
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
cargo +stable test --offline --release -p shodo --lib \
  line::tests::paragraph_end_checks_the_reshaped_line_start_width
```

性能比較はリポジトリ直下で以下を実行する。依存はキャッシュ済みとする。
比較対象を固定し、計測中はソースを変更しない。別 worktree には別 target を使う。

```bash
set -e
mkdir -p "$HOME/tmp"
export TMPDIR="$HOME/tmp"
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=""
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
paragraph_run=$(mktemp -d -p "$HOME/tmp" shodo-15-repro-XXXXXXXX)
git worktree add --detach .worktrees/shodo-15-repro-before 68e13e27ed1fec40bc184cd0e08e873c632c02ae
git worktree add --detach .worktrees/shodo-15-repro-after 87397ff355950407add5adda3ed5da1702add89b
for paragraph_case in latin-long arabic-long; do
  (cd .worktrees/shodo-15-repro-before
   CARGO_TARGET_DIR="$paragraph_run/build-before" taskset -c 2 python tools/bench/run.py \
     --output "$paragraph_run/before-$paragraph_case" --quick --case "$paragraph_case" --cold-samples 2)
  (cd .worktrees/shodo-15-repro-after
   CARGO_TARGET_DIR="$paragraph_run/build-after" taskset -c 2 python tools/bench/run.py \
     --output "$paragraph_run/after-$paragraph_case" --quick --case "$paragraph_case" --cold-samples 2 \
     --baseline "$paragraph_run/before-$paragraph_case")
done
git worktree remove .worktrees/shodo-15-repro-before
git worktree remove .worktrees/shodo-15-repro-after
```

長めの next_line 計測は、上記の `before-latin-long/bin/layout` と
`after-latin-long/bin/layout` を交互に使って以下のように実行する。
各実行に別の `SHODO_BENCH_OUTPUT` を指定する。scale 64のみの確認では
filter を `^latin-long/next_line/64$` に変える。

```bash
SHODO_BENCH_CASE=latin-long SHODO_BENCH_OUTPUT="$paragraph_run/long-before-1" \
  taskset -c 2 "$paragraph_run/before-latin-long/bin/layout" --bench --noplot \
  --sample-size 50 --warm-up-time 1 --measurement-time 2 \
  '^latin-long/next_line/(1|8|64)$'
```

`$paragraph_run` の条件・結果・判断を正式な記録に保存してから、今回作成した
そのディレクトリだけを削除する。この実装作業の生データ、ログ、ビルド、
使い捨て worktree も要約保存後に片付け、実装はローカルブランチに保持した。

## PR #289 レビュー後の追加修正

レビュー対象は `21d47a62974b2703b83c5a35c77ad455b58140b0`。
以下の2件を固定Latinフォント16px、RTL `BidiOverride` 内の
`VAVAVAVAVAVA` と本文の改行許可で再現し、回帰テストの失敗を確認した。

- [末尾空白の指摘](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243653172):
  ボックスの外に通常の空白を追加すると、幅28.125pxで `End` 行 `12..19` が
  28.75pxになる。最後の空白・タブをぶら下げても、可視本文の行頭補正は
  幅判定に含める。句読点の補正には末尾空白を除いた可視範囲を使い、既存の
  日本語の句読点と保存空白のぶら下げも維持する。
- [閉じ余白の指摘](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243653176):
  inline-end padding 1px・border 0.5px の閉じボックスの外に U+200B を追加すると、
  幅19.171875pxで `Regular` 行 `13..18` が20.671875pxになる。候補を選ぶ際に
  同じ行へ取り込む閉じ要素の実幅も予約し、`Clone` の装飾幅は取り込み後の
  境界で求める。即時・遅延・emergency・hyphen候補に同じ補正を渡す。

追加した3テストは、末尾空白・タブとCollapse/Preserveの16ケース、
非ゼロの閉じ余白と入れ子の14ケース、外側U+200BとSlice/Cloneの8ケースを検証する。
Slice/Clone双方で、自然な1行の実測幅を指定幅にしても1行のままで本文全体を
含むことを確認し、閉じ余白の二重計上による過剰な折り返しも検出する。
静的レビューではCritical / Important指摘なし。上記の自然幅の検証はMinor指摘を反映した。

追加修正の検証結果（Rust/Cargo 1.97.1、隔離target、release debug 0・incremental無効）:

- workspace: 1,774 passed、0 failed、9 ignored。
- no-default-features: 1,236 passed、0 failed、9 ignored。
- complex-scriptsのみ: 1,238 passed、0 failed、9 ignored。
- 整形、Clippy全workspace・all-targetsのwarnings-as-errors、rustdoc warnings-as-errors: 成功。
- snapshot全46件match、partial=false、更新なし、changed pixels=0。
- 自然幅の検証を追加後、行走査の18テストと整形・diff checkを再確認して成功。
  再シェープの回数・予算を制限する既存テストも含む。

再現・回帰確認は上記の環境変数で次を実行する。

```bash
cargo +stable test --offline --release -p shodo --lib line::tests::paragraph_end_checks_
cargo +stable test --offline --release -p shodo --lib line::tests
```

追加修正後の時間・allocationは再測定していない。本記録の性能表・digest比較は
初回修正コミットのみの結果であり、最終修正の速度やallocationを保証しない。
PRの最終コミットのRust 1.89・Wasmを含む検証はGitHubのCIで確認する。
追加修正の再現条件・結果を本記録とJSONに保存し、診断プログラム、生ログ、
隔離ビルド領域、使い捨てworktreeは作業終了時に片付ける。

### 閉じ余白と末尾空白の間にある句読点barrier

`ee6a3e84f9bba54d098f04ce8ade67ab3a8a4a9b` の
[追加レビュー指摘](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243744116)を再現した。
固定LatinとCJKフォント、16px、RTL override内の`VAVAVAVAVAVA`の後に、
別の通常方向インライン内の`,`（inline-end padding 1px・border 0.5px）、
外側の通常空白を置く。本文の改行を許可し、Collapse、
`hanging_punctuation.force_end=true`・`last=true` とすると、
幅24.3125pxで最終`End`行`13..20`が24.953125pxとなった。

末尾空白の可視範囲を`trailing_start`で求めると、透明なCloseも範囲から除かれる。
描画側の`trailing().0`は空白位置を境界にするため、空白直前のCloseのbarrierを保持する。
走査でも同じ境界を使い、描画時に阻止される句読点のぶら下げ・詰めを幅判定で
許可しないようにした。最終unitがhangableである分岐なので、未追加のそのunitを
除く既存widthsで`trailing(data, start, i, &widths, sat).0`を求めても同じ境界になる。

`paragraph_end_keeps_closing_edges_before_hanging_whitespace` は、句読点`、`/`,`/`)`、
Collapse/Preserve、ぶら下げの有無、幅7種類の84ケースで、全行が指定幅以下、
最終行がEndで全文を覆い、警告がないことを確認する。修正前の失敗を確認した。
修正後の行走査19テスト、workspace 1,775件、no-default-features 1,237件、
complex-scriptsのみ1,239件が成功（全体はいずれも0 failed、9 ignored）。
整形・diff check、Clippy・rustdoc warnings-as-errorsも成功。
snapshot全46件match、partial=false、更新なし、changed pixels=0。
静的レビューにCritical / Important / Minor指摘なし。実測性能はこの追加修正後も
再計測しておらず、上記性能表の対象コミットは変えない。

### キャッシュ再試行とハイフン挿入順の整合

`bd097629b36fd4f938e3ec238e9e496d6d7204e0` の追加レビューを確認した。
[キャッシュの閉じ余白](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243804526)
では、幅19.171875pxへの再試行が行`13..18`・20.671875pxとなる失敗を再現した。
走査とキャッシュの候補評価で、取り込むClose/Bidiの実幅、取り込み後のClone装飾幅、
句読点barrierの計算を共有し、通常・emergency・hyphen候補へ同じ補正を渡す。

[ハイフン挿入順](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243804531)
は、固定Latin 16pxのRTL IsolateOverride内に`,VA VA\u{ad}`、ゼロ幅の閉じ要素、
幅2000pxのatomicを続け、`hanging_punctuation.force_end=true`で再現した。
幅1000pxで選ぶハイフン行の実幅46.859375pxをそのまま指定すると、修正前の通常走査は
行`0..20`ではなく`0..10`を選び、収まるハイフン候補を捨てた。
可視ハイフンをClose/Bidiの前へ挿入してから候補のsummaryを求め、描画順と一致させる。
既存のintrinsic sizing呼び出しは元と同じ`i+1`の範囲を維持する。

さらに、通常本文のthresholdが超過しなくても、可視ハイフンの挿入で超過する境界を
再現した。自然幅46.859375pxの1/64px手前（46.84375px）では、修正前のキャッシュが
`0..14`・46.859375pxを返し、通常走査は`0..7`・18.375pxを選んだ（BidiOverride）。
保持した終端を再利用する際もハイフン候補の必要幅を`available - indent`と比較し、
超過時は既存の候補選択へ合流する。改行後に取り込む閉じ要素と、収まる候補がない場合の
既存のフォールバックは維持する。境界幅のテストは修正前に失敗することを確認した。

[末尾空白のキャッシュ指摘](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243804519)
については、調べた入力で修正前のfreshとcacheの差を再現できなかった。
段落末尾のhangable unitも検証する条件を通常走査と揃え、可視本文の句読点summaryを
共有した。キャッシュはタブを含む走査を保持しないため、タブ再試行の失敗とは扱わない。

追加した3テストは、Collapse/Preserveの末尾空白とSlice/Cloneの閉じ余白について、
各継続tokenで広い行からの再試行とfreshの範囲・幅・改行理由・tokenが一致し、
キャッシュを再走査しないことを確認する。ハイフンはBidiOverride/IsolateOverride双方で、
同一contextを自然幅+1/64px、自然幅、自然幅-1/64px、同じ幅の順に再試行して検証する。

最終修正のローカル検証（前記と同じRust/Cargo 1.97.1、隔離target、release debug 0）:

- 行モジュール全186件（行走査22件を含む）成功。
- workspace: 1,778 passed、0 failed、9 ignored。
- no-default-features: 1,240 passed、0 failed、9 ignored。
- complex-scriptsのみ: 1,242 passed、0 failed、9 ignored。
- fmt、diff check、Clippy全workspace/all-targets、rustdoc warnings-as-errors: 成功。
- snapshot全46件match、partial=false、更新なし、changed pixels=0。
- 最終の読み取り専用レビューにCritical / Important指摘なし。

```bash
cargo +stable test --offline --release -p shodo --lib line::
```

最終修正後の時間・allocationは再測定していない。初回修正の性能測定対象は変更せず、
最終コミットのRust 1.89・WasmはPR CIで確認する。再現条件と結果は本記録およびJSONに
保存したため、一時の診断プログラム・ログ・隔離target・worktreeは作業終了時に片付ける。

### 末尾空白後の閉じ要素による過剰な折り返し

`f65f111c249e4bb6c627daa918723c6b67843ff1` の
[追加レビュー指摘](https://github.com/fulgur-rs/shodo/pull/289#discussion_r4243894694)を再現した。
固定CJK 16px、LTRのゼロ余白インライン内に「ああ、 」を置き、
`hanging_punctuation.force_end=true`とする。幅1000pxでは全文`0..10`の実幅32pxなのに、
幅32pxのfresh走査とcache retryは先頭の「あ」`0..3`・16pxだけを返す。
「あ、 」には前の改行候補がないため、この過剰な折り返しは起きなかった。
正式な回帰テストで修正前の失敗を確認した。

空白後にClose/PDI/PDFが続くと最終unitはhangableではないが、描画の可視範囲は
空白前までとなる。複数の閉じ要素では、物理段落末尾より前にも同じ判定が必要となる。
走査とcacheのthreshold評価で、実際の末尾空白境界にある句読点summaryを使う。
空白のadvanceがゼロでも境界を保持し、途中に新しい可視本文があれば保持状態を破棄する。

空白前のsummaryを保持して使い回すため、各閉じ要素からprefixを再構築しない。
通常時と、padding/borderによってPreserveの空白が保持される場合の2境界を保持する。
これによりPreserveとCollapseが混在しても、空白前の閉じbarrierを残せる。
後続の余白の先読みは同じ透明な尾部で1回だけ求め、新しい空白・非透明unitで破棄する。
逐次のprefix/suffix再走査を追加しない。実測の時間・allocationは再測定していない。

`paragraph_end_excludes_controls_after_hanging_whitespace`は、Collapse/Preserve、空白/タブ、
入れ子0/1/2、通常の空白幅/word-spacing -100%、Normal/Isolateの48ケースで、幅32pxに
全文が1行で収まり、fresh/retryの範囲・幅・改行理由が一致することを検証する。
空白retryは再走査しないことを確認する（タブは既存方針どおり再走査してよい）。
入れ子の制御文字とゼロ幅空白も、途中の修正案の不足を再現して補強した。
`hanging_tail_retains_preserved_space_before_outer_closing_edges`は、内側Preserve/外側Collapse、
双方Isolate、外側padding 0/1.5pxで、各prefixの境界・spacing幅が描画の既存trailing境界と
一致し、最終の句読点補正も同じ幅条件で描画準備の結果と一致することを検証する。

最終修正の検証結果（前記と同じRust/Cargo 1.97.1、隔離target、release debug 0）:

- 行モジュール188件（行走査24件を含む）成功。
- workspace: 1,780 passed、0 failed、9 ignored。
- no-default-features: 1,242 passed、0 failed、9 ignored。
- complex-scriptsのみ: 1,244 passed、0 failed、9 ignored。
- fmt、diff check、Clippy全workspace/all-targets、rustdoc warnings-as-errors: 成功。
- snapshot全46件match、partial=false、更新なし、changed pixels=0。
- 最終レビューにCritical / Important / Minor指摘なし。

```bash
cargo +stable test --offline --release -p shodo --lib line::
```

最終コミットのRust 1.89・WasmはPR CIで確認する。再現条件・判断・検証結果を保存したため、
一時の診断プログラム、ログ、隔離target、worktreeは作業終了時に片付ける。
