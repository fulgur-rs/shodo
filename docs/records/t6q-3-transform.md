# Text transform の transient String と casing context の削減

対象 issue: shodo-t6q.3。base は `a62458009af2f3b3cc83441eb735b70ef8175807`。恒等 scalar と単一 scalar の kana/width 変換はスタック UTF-8 バッファを借用し、ICU の borrowed Cow をそのまま使う。case が一つもない style set では logical text clone、全文 casing flags、Unicode 前後走査、word segmentation を省く。case がある場合は無変換 item を含む全文 context を従来どおり生成する。計算量は線形のまま、不要な走査と scalar 単位の割当を減らす。

Greek/Lithuanian の複数 scalar 特殊処理、合成 kana、WidthOrigin の変換前 text、mapping、locale warning の順序、増分 text limit は残した。通常 set と first-line set が同じ transform_inner を通る。shodo-sbp.3 の itemize 文脈改善とは別の処理。

## 回帰検証

実際の context 入場を test-only counter で測る。width/kana/combined/word-space-only の出力を確認し、context 入場ゼロを要求する。invalid locale + auto phrase + warning cap + text limit の順序と actual byte 数も検証する。混在 style の Greek final sigma は無変換前置文字を参照する。

baseline の既存 transform 22、word-space 7、processed limits 2 tests は通過。新規 unit 2 tests の RED は non-case context が 1 回（期待 0）で失敗し、混在 case は通過。String 旧実装に context 省略だけを加えた状態で固定 Latin/font の 1024 scalar build は 4354 割当となり、新規 allocation test の ceiling 2048 に失敗した。両変更後 unit 2 と allocation 1 が GREEN。割当 test は builder 準備を scope 外に置き、実 build 全体を含めるため shaping/cache の他の割当も計数する。counter は release に存在しない。

## 独立 A/B

再現 example: `dev/bench/examples/transform_cost.rs`。`cargo run --release -p shodo-bench --example transform_cost -- time`、割当は別 executable を `--features allocation-counting` で build して `alloc` を渡す。

四状態: baseline、String 削減だけ strings、context 省略だけ context、両方 combined。固定 fixture Latin/CJK/Arabic font、system fonts 無効、Limits::default。font SHA は summary に保存。15 入力: 無変換、部分変換、width、kana、combined、word-space、invalid locale warning、Greek upper/final sigma、Turkic/Lithuanian/Dutch、first-line、vertical TCY WidthOrigin、case+width+kana。入力単位を16反復、warmup4、時間9標本、割当3標本。

時間は allocator counter を含まない executable で測り、allocation は別 build で測る。scope は prebuilt builder を消費する Paragraph build のみ。builder 準備、output serialization、line layout、paragraph drop は外。scope 終了時に Paragraph と LayoutContext が保持される。gross は累計 requested bytes、net は scope 前後の requested live 差、peak は開始 live を超えた最大 requested live（RSS ではない）。

時間入力の text/mapping/warnings と、別の2反復入力を幅48/160/1024で layout した source/cluster/glyph ID・advance・origin・geometry の float bits・font ID・paint/variation/transform・owners・TCY・overflow・warnings を照合。text cap 0/1/8/input bytes の success/error も照合した。4状態×time/allocの8出力契約は完全一致。フォント ID は layer/index で保存し font blob は展開しない。JSON 化は測定 scope 外。

対称順序の時間中央値（µs、各18標本）と別scopeの割当呼出数:

| 入力 | baseline time | strings | context | combined | baseline calls | strings | context | combined |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| none | 183.3 | 179.6 | 179.3 | 180.4 | 799 | 799 | 799 | 799 |
| width | 221.3 | 214.4 | 200.0 | 196.4 | 1245 | 957 | 1243 | 955 |
| kana | 265.5 | 262.4 | 229.2 | 222.6 | 1824 | 1376 | 1662 | 1214 |
| width-kana | 223.9 | 216.6 | 201.7 | 197.3 | 1389 | 957 | 1387 | 955 |
| word-space | 171.3 | 167.1 | 148.9 | 149.0 | 917 | 773 | 867 | 723 |
| first-line | 451.8 | 445.6 | 423.3 | 421.8 | 2561 | 2129 | 2559 | 2127 |
| case-width | 295.1 | 287.2 | 292.3 | 284.6 | 1745 | 1233 | 1745 | 1233 |

全15入力・gross/freed/net/peak と executable/source SHA は `t6q-3-transform.summary.json`。kana の gross は 266408→257832 bytes、word-space は180020→178084、case-width は335096→331608。全入力で net と peak は4状態同じ。既存 retained shaping 等が peak を支配するため、保持メモリや peak 改善は主張しない。無変換は早期 return するので差がない。case 入力では context 省略の寄与がないことも割当で確認した。

採用理由は scalar 単位割当と不要 context の除去、および同一出力での実割当削減。共有ホスト高負荷の小標本なので時間差は探索値であり本番改善率は主張しない。一部時間は揺れて逆転する。baseline→strings→context→combined→combined→context→strings→baseline の対称順序でも保存済み executable を実行した（各18標本）。その別枠の中央値は summary に保存し、初回値を置き換えない。

## 再現・証拠管理

Rust 1.96.0 direct toolchain、offline、jobs1、RUST_TEST_THREADS2、nice10、専用 `target/t6q-build/t6q-3`、共通 build.lock で build/測定を直列化。RUSTC_WRAPPER を空、target runner を /usr/bin/env にする。状態ごと source 復元→build→executable copy/hash→測定→候補復帰を同一 lock 区間で行い、状態間は lock を解放した。

raw: worktree の `target/performance-artifacts/t6q-3/`。各状態 built transform、runtime/fixture/probe manifest と fingerprint、rustc/cargo version、executable SHA、time/alloc JSON を保存。context 初回だけ cargo fmt 後だったため probe SHA に整形差があった。計測ロジックの変更ではない。共通 probe source を復元して context だけ取り直し、最終4状態の probe SHA はすべて b34f1802… で同一。runtime/fixture/probe manifest は transform.rs 以外が完全一致。初期の巨大 snapshot（line offset-mapping の Debug が paragraph を繰返し展開し gzip約263MB）は `initial-large/` に保全し採用値から除外した。小さな snapshot を測定入力とは分離して全4状態を取り直した。実行中 script の書換えで context 取得が一度失敗したが、不変 script で再実行した。初期 kache/runner 環境失敗と共有 target の artifact 混同は専用 target/環境上書きで除外した。

## 最終 checks

`cargo test --offline -p shodo`: 785 pass、2 ignored、29 suites（doctest含む）。関連 harness shaping/line_shaping/first_line/horizontal_contracts/analysis_limits: 104 pass。allocation-counting transform_allocations: 1 pass。`cargo fmt --all --check` と `cargo clippy --offline -p shodo -p shodo-bench --all-targets --features shodo/accesskit,allocation-counting -- -D warnings` は exit0。`cargo test --offline -p shodo --no-default-features --test text_transform --test word_space_transform` は28 pass。全 workspace/CI は controller が統合管理する。
