# spacing summary の neutral compact leaf 候補（2026-10-14）

基準は `454fb916a9ae9addd6772417b64986a75717be4c`。既存の
`needs_spacing` による punctuation/autospace 境界計算の省略に加え、
neutral compact leaf を現在の同一 bidi level のフレームへ直接追加し、
full Summary への展開、Summary::join、level stack の push/pop 判定を省く。
ParagraphData、Cursor、compact storage の production フィールドは増やさない。

## 適用条件と保持する意味

- `needs_spacing == false` かつ `first_line.is_none()`。
  `spacing::needed` の既存の検査結果を用い、文字の script だけでは判定しない。
- compact storage の Leaf で、整数の cost と tracking がともに 0。
  Leaf は同一の first/last edge を持ち、6 個の境界フラグがすべて false。
- 現フレームの level が追加する unit の level と同じ。Tab の level は
  既存と同じ base level 補正を適用する。
- 現フレームの cost が 0 で、隣接する既存 edge の tracking も 0。

条件外は既存の `Cursor::push` に戻る。Full/Dense/Empty storage、
level の遷移、明示 letter/word spacing、punctuation/autospace が必要な
段落は一般の合成を保持する。first-line の通常 dataset は適用対象外で、
intrinsic sizing が alternate dataset のエッジを持ち越す際の境界計算を保持する。

偶数 level では first と先頭側フラグを維持して last と末尾側フラグを更新する。
奇数 level ではその逆を行う。空の barrier、U+200B の autospace 分離、
hanging の各フラグも同じ合成則になる。更新時は summary cache を無効にする。
整数 cost の加算と Saturation 処理は条件外の既存経路に残る。

## 検証

`neutral_compact_leaves_skip_level_stack_work` は変更前に cost と端点の
assertion を通過し、generic push の visits が期待する 0 に対して 4 となって
失敗した。変更後は 0 で通過する。
`neutral_leaf_append_rejects_a_nonzero_prior_cost_without_mutation` も拒否条件の
追加前に期待どおり失敗し、追加後の検証に含めた。

参照経路との prefix ごとの同値性を、明示 letter/word spacing、
first-line alternate、vertical writing、U+200B、Tab、forced break、SHY、
Hebrew/Arabic の混在 bidi、cursive、および inline border/padding で検証する。
全 64 通りの境界フラグと 5 level、Text/Cursive/Atomic/Barrier の各 edge を
比較し、cache の無効化と clone の独立性も確認する。

テストと clippy は `TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_BUILD_JOBS=2`、
専用の `CARGO_TARGET_DIR`、`taskset -c 7,9`、`cargo +stable --offline` を用いる。
テスト runner は `--config 'target.x86_64-unknown-linux-gnu.runner="env"'`。

## 検証実行の結果

| focused 実行 | 成功 | 失敗 |
| --- | ---: | ---: |
| shodo library `spacing_summary` | 17 | 0 |
| shodo integration `bidi` | 18 | 0 |
| shodo integration `intrinsic` | 7 | 0 |
| harness `first_line` | 27 | 0 |
| harness `intrinsic_budget` | 3 | 0 |
| harness `spacing` | 44 | 0 |

focused コマンドは以下。すべて exit 0、ignored 0。
上記の環境変数・CPU・target・runner 設定を併用した。

```sh
cargo +stable test --offline -p shodo --lib spacing_summary
cargo +stable test --offline -p shodo --test bidi --test intrinsic
cargo +stable test --offline -p shodo-harness --test spacing --test first_line --test intrinsic_budget
```

- `cargo +stable fmt --all --check` と `git diff --check`: exit 0。
- `cargo +stable clippy --offline -p shodo --all-targets -- -D warnings`: exit 0、警告なし。
- `cargo +stable test --offline -p shodo` は root の指示で意図的に中断。
  中断時は library tests の 757 件が成功、失敗 0 件で、suite の完走結果ではない。
  固有 target の library test プロセスだけへ SIGINT を送り、cargo は exit 101。
  root が最終統合後の全 workspace と exact-head CI を一度実行する。

## 測定判断

root が同じ入力・バイナリ設定で実行する時間/割当の paired A/B 対象として
凍結する。個々の unit の省略回数は elapsed time の改善率を保証しないため、
ここでは時間の改善率を主張しない。root の正式な測定記録と比較し、意味のある
改善が確認できなければ候補を採用しない。

## 中央の測定と統合検証

固定された baseline `454fb916` と個別実装の反復 A/B は、[正式な記録](performance-spacing-ab-2026-10-14.json) に保存した。条件ごとの全サンプル、出力ハッシュ、別バイナリの割当量、同一バイナリ対照、再測定と制約を含む。初回の mutable baseline を使った測定は指紋を保証できず却下し、固定 worktree で測り直した。

4変更を組み合わせた版では、workspace 1952 / AccessKit 1968 / 最小 feature 1412 / complex-scripts 1415 / allocation-counting 25 件が成功、失敗0。fmt・通常/AccessKit Clippy・通常/AccessKit docs・Wasm3構成も exit0。並行して入った main の変更との統合と、最終HEADのCIは別の確認として扱う。
