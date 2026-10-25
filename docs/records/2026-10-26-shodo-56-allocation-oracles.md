# SHODO-56 allocation oracle 再検証（2026-10-26）

SHODO-56 は PR #364 (`ba62f26`) で実装済みだったが、Plane は In Progress のままだった。現行 `c686419` で再検証すると4つの allocation oracle のうち3つが古い絶対 byte 基準により失敗した。geometry / source / glyph の既存 SHA-256 は一致した。

## 条件と判断

- Linux x86_64、rustc 1.91.0 (f8297e351 2025-10-28)、HarfRust 0.15、Cargo.lock は `c686419` のもの。
- 同じ専用 worktree と固定 fixture で debug / release を測定。`allocation-counting` を有効化し、時間は測定しない。
- 対照条件は `git diff ba62f26^1 ba62f26 -- crates/shodo/src | git apply --reverse --3way` で SHODO-56 のソース変更だけを取り消す。後続の機能と依存は維持する。測定後は `git restore --source=HEAD --staged --worktree crates/shodo/src` で復元した。
- HarfRust 0.14 の Cargo.toml / Cargo.lock に一時的に戻した対照実験でも3件は失敗した（RTL 1,657,445 bytes、opening punctuation 360,792 bytes、unsplit 534,098 bytes）。0.15 の増分だけでは説明できず、旧機能構成の総量を現行構成の上限に使うことが不適切だった。
- line / reshape の基準は、現行構成で最適化を取り消した実測へ更新。punctuation は現在の最適化済み build の実測を厳密な上限にした。lazy metrics の有効性は既存の `GLYPH_METRICS_CREATED` unit oracle が別に検証する。context も同じ対照実験の warm 基準へ更新し、全ハッシュは変更しない。
- 全最適化を取り消す比較なので、punctuation workload の byte 改善を lazy metrics 単独の効果とは扱わない。時間の改善も主張しない。
- 4つの oracle は CI の明示的な allocation-counting 実行対象に入っていなかったため追加する。

release の既存 context テストには別の条件不一致もあった。単独 example は warm-up 後の build を測るが、テストは cold build を warm 基準と比較していた。現行 release で cold build は 590,335 bytes、warm build は 575,744 bytes だった。テストにも context / edge の warm-up と warm 出力の一致検査を追加し、測定条件を揃えた。context の debug 基準は 608,155 から 581,056 bytes へ厳しくなり、release は 581,008 bytes の実測対照値を使う。いずれも1 item あたり最低20 bytes の削減条件を維持する。

## 測定値

byte は allocator へ要求した累積量、calls は allocator 呼び出し数。各プロファイルの全12ケースで、最適化あり・なしの出力ハッシュは一致し、既存テストのハッシュとも一致した。

### debug

| case | 対照 bytes | 現行 bytes | 削減 bytes | 対照 calls | 現行 calls |
| --- | ---: | ---: | ---: | ---: | ---: |
| uniform-ltr | 1215661 | 1179891 | 35770 | 4129 | 4009 |
| uniform-rtl | 1469585 | 1460275 | 9310 | 7715 | 7643 |
| mixed-levels | 806253 | 805257 | 996 | 3329 | 3300 |
| no-continuation | 312952 | 312056 | 896 | 117 | 116 |
| rtl-continuation-hanging | 1657841 | 1657469 | 372 | 10498 | 10475 |
| context-build | 581056 | 575792 | 5264 | 811 | 529 |
| edge-window | 1201587 | 1197035 | 4552 | 8074 | 7694 |
| repeated-opening | 363768 | 360808 | 2960 | 666 | 418 |
| repeated-closing-bracket | 360472 | 357512 | 2960 | 655 | 407 |
| fi-splits | 613300 | 611668 | 1632 | 1878 | 1819 |
| ffi-splits | 1153140 | 1151508 | 1632 | 3169 | 3110 |
| unsplit-control | 534106 | 534106 | 0 | 335 | 335 |

### release

| case | 対照 bytes | 現行 bytes | 削減 bytes | 対照 calls | 現行 calls |
| --- | ---: | ---: | ---: | ---: | ---: |
| uniform-ltr | 1215661 | 1179891 | 35770 | 4129 | 4009 |
| uniform-rtl | 1469585 | 1460275 | 9310 | 7715 | 7643 |
| mixed-levels | 806253 | 805257 | 996 | 3329 | 3300 |
| no-continuation | 312952 | 312056 | 896 | 117 | 116 |
| rtl-continuation-hanging | 1657841 | 1657469 | 372 | 10498 | 10475 |
| context-build | 581008 | 575744 | 5264 | 809 | 527 |
| edge-window | 1201587 | 1197035 | 4552 | 8074 | 7694 |
| repeated-opening | 363720 | 360760 | 2960 | 664 | 416 |
| repeated-closing-bracket | 360424 | 357464 | 2960 | 653 | 405 |
| fi-splits | 613276 | 611644 | 1632 | 1877 | 1818 |
| ffi-splits | 1153116 | 1151484 | 1632 | 3168 | 3109 |
| unsplit-control | 534082 | 534082 | 0 | 334 | 334 |

## 再現

リポジトリルートで以下を実行する。比較対象の一時的なソース変更は使い捨て worktree 内だけで行い、元に戻す。各 example は入力を準備し、warm-up の後に計測する。段落構築中の cold work も workload が定義した範囲で含む。新たな cold-start や時間比較は行っていない。

```sh
for profile in dev release; do
  for example in line_small_allocations shape_context_allocations punctuation_metrics reshape_split_allocations; do
    TMPDIR="$HOME/tmp" cargo run --offline --profile "$profile" -q \
      -p shodo-bench --features allocation-counting --example "$example" -- --alloc
  done
done

TMPDIR="$HOME/tmp" cargo test --offline -p shodo-bench --features allocation-counting \
  --test line_small_allocations --test shape_context_allocations \
  --test punctuation_metrics --test reshape_split_allocations
```


CI は通常の dev プロファイルと異なり、`CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_DEV_DEBUG=line-tables-only` を指定する。同条件で上記4テストも検証した。

## 最終検証

- CI と同じ dev 設定で4つの allocation oracle がすべて成功。
- release でも同じ4テストがすべて成功。context は条件修正前の失敗、修正後の成功を確認した。
- `cargo test --offline -p shodo` が成功。unit tests は1,087成功・8 ignored、integration tests と11 doctests もすべて成功。
- `cargo fmt --all --check` と `git diff --check` を実行。
- 独立レビューで重大な実装指摘なし。ローカル除外対象の記録文書の明示追加と、CI 条件での検証という指摘に対応した。
- 対照実験のソース変更と依存変更はすべて復元済み。本体実装・依存・固定出力ハッシュは変更していない。
