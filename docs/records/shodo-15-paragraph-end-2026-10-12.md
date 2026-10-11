# SHODO-15: 段落末尾で行頭の再シェープによる幅増加を確認する

2026-10-12。変更前の実装は `68e13e27ed1fec40bc184cd0e08e873c632c02ae`。
変更後の実装は `87397ff355950407add5adda3ed5da1702add89b` に保存した。
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

## 検証結果

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
