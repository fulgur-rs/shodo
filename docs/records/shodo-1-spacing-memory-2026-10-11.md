# SHODO-1: unit spacing の保持メモリ削減

2026-10-11。変更前は `94ac48c8a368eb7e421a26b710ebf8cc2a2b46bf`。
測定した変更後のコードは `4f92a5ef71979173639d66f70c36d7cb1d4f8dfe` に保存した。
SHODO-1 の最初の改善として、段落が保持する spacing 情報だけを圧縮した。
公開 API、段落の幅変更による再利用、ページ高さ不足時の retry は維持する。
測定値と条件の要約は [JSON](shodo-1-spacing-memory-2026-10-11.json) に保存した。

## 実装と範囲

変更前の `UnitSpacing` は単一文字でも first/last の両 edge と i64 cost を
含む完全な `Summary` を保持していた。x86_64 では 88 bytes/unit。
変更後は word/tail/gaps の 32 bytes と、通常は 24 bytes の summary entry に分け、
合計 56 bytes/unit、約36%削減する。配列ヘッダなどの固定費はこの値に含めない。

単一 edge の unit 番号は配列添字と一致する場合だけ省略し、cost は i32 に
収まる場合だけ圧縮する。cost は計算済みの値を保存するため、読み出すたびに
句読点情報を参照し直す必要はない。空の summary は6個の境界フラグを保存する。
複数 edge、大きな cost、添字と異なる unit などは完全な summary を別配列に保存する。
例外配列を含む実際の Vec capacity が完全な summary 配列の容量を超えたら、
一度だけ従来と同じ密な summary 配列に切り替える。構築時から圧縮しており、
全 unit 分の従来形式を先に作ってから圧縮する処理ではない。

units、itemize、breaks、punctuation などは今回の変更対象に含めない。
密な形式への移行中は新旧配列が一時的に共存するので、保持量の上限は
構築中ピークの上限を意味しない。今回の54 workloadでは、いずれの測定 scope でも
追加ピークの増加はなかった。

## 測定条件と判断の限界

- AMD Ryzen 5 5600G、CPU affinity `[2]`、x86_64 Linux。
- Rust/Cargo 1.97.1、release opt-level 3、debug 0、incremental 無効。
- `complex-scripts` 有効、system fonts 無効、既存の固定 font/input/lock/harness。
- 変更前後を同じ深さの別 worktree に置き、**別々の CARGO_TARGET_DIR** でビルド。
  `PATH="$HOME/.cargo/bin:$PATH"` と `RUSTC_WRAPPER=""` で kache shim を避け、
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER` を unset した。
- 時間は latin-long / japanese-long / combining-latin の各 scale 1/8/64、
  7 operations、Criterion quick 10 samples・warmup 100ms・measurement 200ms。
  cold は各2プロセス。メモリと出力 digest は全54 workloadを確認した。

時間とメモリは別 executable で測定する。メモリの build net は段落と context の
保持 allocation、peak はその scope の追加 allocation ピークであり、
RSS、mmap、allocator metadata や native allocation を含まない。
SHODO-1 の元の fulgur 100ページ入力に対する RSS/heap ピークは今回再測定していない。
quick の単一比較なので速度改善を保証せず、残る日本語の読み出し overhead は
次の最適化・統合計測で確認する。

初期の共通 target による計測は、異なる source hash に対して同一 executable が
再利用されたため破棄した。ここに保存する値は独立した target での再計測だけで、
変更前後の executable hash が異なることも確認している。

## 測定結果

全54 workloadで build 後の保持量が減少した（約0.42〜10.21%）。
長いラテン文の scale 64 は約1.96 MB削減したが、build中ピークは変わらなかった。
日本語の最初の next_line は今回のquick比較で約5〜10%遅く、全行レイアウトは約2〜3%遅い。
メモリ削減と引き換えに summary 復元の処理が増える。63 operationの中央値と比率はJSONに保存した。

| workload | build保持 bytes: 前→後 | build追加ピーク bytes: 前→後 |
| --- | ---: | ---: |
| latin-long/1 | 328,985 → 298,281 | 329,989 → 329,989 |
| latin-long/8 | 2,429,118 → 2,183,598 | 2,453,334 → 2,453,334 |
| latin-long/64 | 19,230,182 → 17,266,134 | 19,440,094 → 19,440,094 |
| japanese-long/1 | 237,621 → 221,093 | 237,621 → 233,309 |
| japanese-long/8 | 1,720,473 → 1,588,361 | 1,720,473 → 1,702,161 |
| japanese-long/64 | 13,583,289 → 12,526,505 | 13,583,289 → 13,452,977 |
| combining-latin/1 | 33,826 → 33,266 | 33,826 → 33,266 |
| combining-latin/8 | 80,957 → 76,589 | 80,957 → 78,709 |
| combining-latin/64 | 458,005 → 423,173 | 458,005 → 456,205 |

| japanese-long scale | next_line後/前 | all_lines後/前 |
| --- | ---: | ---: |
| 1 | 1.050 | 1.017 |
| 8 | 1.103 | 1.025 |
| 64 | 1.083 | 1.031 |

## 検証

- 整形・Clippy `--workspace --all-targets -- -D warnings`・rustdoc warnings-as-errors: 成功。
- `cargo +stable test --offline --release --workspace --quiet`: 1,758 passed、0 failed、9 ignored。
- `--no-default-features`: 1,220 passed、0 failed、9 ignored。
- `--no-default-features --features complex-scripts`: 1,222 passed、0 failed、9 ignored。
- snapshot: 全46件 match、partial=false、更新なし、changed pixels=0。
- 全54 workloadの7種類の memory digest と、時間測定した全63 operationの digest が変更前後で一致。
- 通常4096 unitsを64 bytes/unit以下に収める回帰テストは、変更前に失敗し変更後に成功。
  i64極値、非ローカル unit、非ゼロ添字の Leaf、全64通りの境界フラグ、
  capacity による密な形式への移行と既存 entry の復元を確認した。
- コードレビュー: Critical/Important 指摘なし。例外テストが早期に密な形式へ移行する
  軽微な指摘を修正した。Rust 1.89 と Wasm ビルドは今回実行していない。

## 再現

リポジトリ直下で以下を実行する。依存はキャッシュ済み、元と同じ toolchain を使う。
比較対象は記録時のコミットに固定する。計測が終わるまでソースを変更しない。

```bash
set -e
mkdir -p "$HOME/tmp"
export TMPDIR="$HOME/tmp"
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=""
unset CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER
spacing_run=$(mktemp -d -p "$HOME/tmp" shodo-1-repro-XXXXXXXX)
git worktree add --detach .worktrees/shodo-1-repro-before 94ac48c8a368eb7e421a26b710ebf8cc2a2b46bf
git worktree add --detach .worktrees/shodo-1-repro-after 4f92a5ef71979173639d66f70c36d7cb1d4f8dfe
for spacing_case in latin-long japanese-long combining-latin; do
  (cd .worktrees/shodo-1-repro-before
   CARGO_TARGET_DIR="$spacing_run/build-before" taskset -c 2 python tools/bench/run.py \
     --output "$spacing_run/before-$spacing_case" --quick --case "$spacing_case" --cold-samples 2)
  (cd .worktrees/shodo-1-repro-after
   CARGO_TARGET_DIR="$spacing_run/build-after" taskset -c 2 python tools/bench/run.py \
     --output "$spacing_run/after-$spacing_case" --quick --case "$spacing_case" --cold-samples 2 \
     --baseline "$spacing_run/before-$spacing_case")
done
export spacing_run
taskset -c 2 python - <<'PY'
import json, os, pathlib, subprocess
root = pathlib.Path(os.environ['spacing_run'])
matrices = []
for version in ['before', 'after']:
    bins = root / f'{version}-latin-long/bin'
    settings = json.loads(subprocess.check_output([str(bins/'cold'), '--describe']))
    rows = [json.loads(subprocess.check_output([
        str(bins/'memory'), '--memory', s['id'], str(s['scale'])
    ])) for s in settings]
    (root/f'{version}-memory54.json').write_text(json.dumps(rows, indent=2)+'\n')
    matrices.append(rows)
assert len(matrices[0]) == len(matrices[1]) == 54
for before, after in zip(*matrices):
    assert before['settings'] == after['settings']
    assert before['digests'] == after['digests']
PY
git worktree remove .worktrees/shodo-1-repro-before
git worktree remove .worktrees/shodo-1-repro-after
```

`$spacing_run` の結果を確認し、必要な条件・結果・判断を正式な記録へ保存してから、
今回作ったそのディレクトリだけを削除する。この実装作業の生データ、ログ、
使い捨てビルド、worktreeは要約保存後に片付け、JSONに再現条件と集計値を残した。
