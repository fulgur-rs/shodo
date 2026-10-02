# shodo-t6q.1: 入れ子ルビのhit失敗時の二重探索

## 採用した変更

`AnnotationIndex::hit`は子孫annotationを先に探索する。失敗後の通常本文のhitを、annotationを再検索しない内部メソッドへ分離する。nested優先、逆順のsibling探索、逆変換、親baseへのcaret帰属は維持する。親矩形による枝刈りは行わない（突出した子孫のhitを保つ）。公開API・limits・配置・source・glyphを変更しない。

## 再現と回帰

共有fixture `dev/bench/examples/support/ruby_hit_fixture.rs` は公開APIで各階層1件のルビを構築する。system fontsを無効にし、`dev/fixtures/assets/fonts/latin.ttf`、20px、`Limits::default()`を使用する。base/readingをa/b交互にしてauto-hiddenを避け、保持されたvisible depthと全glyphの`id != 0`を検証する。

unit testのthread-localカウンターは実際に`AnnotationIndex::hit`へ入った回数を数える。test buildだけに存在する。深度4/8/12/16の両APIで、有限な外側座標(1e6, 1e6)を外した際に各annotationが1回だけ訪問されることを検証する。修正前のREDはdeep4で15回（期待4回）で失敗した。

追加回帰は、本文の外へ突出した子孫hit、回転/倍率/親block offsetの逆変換、deepest local sourceと親base caret帰属、nestedと重なる親本文、重なるsiblingの逆順優先を検証する。変換と重なりは実fontで構築したretained lineの幾何をtest内だけで調整し、配置アルゴリズムから独立してhitの契約を検証する。

## 測定条件

基準commitはcontroller指定の`a62458009af2f3b3cc83441eb735b70ef8175807`。`dev/bench/examples/ruby_hit_miss.rs`を変更前後で同じ条件で実行した。各budget/depthのbuild、line layout、hit index作成、出力snapshot、warm queryを測定scopeの外に置き、外側missを各API3回実行する。nested hitも同様に3回のcontrolを取る。

Rust 1.96.0、debugはunoptimized+debuginfo、releaseはoptimized。時間はallocation-countingなしのbuild、割当は別のrelease buildで`CountingAllocator`を使い、時間を報告しない。共通flockで全agentのbuild/test/probeを直列化し、jobs=1、nice=10、RUST_TEST_THREADS=2を固定した。共通targetで別worktreeのartifactが再利用されたため、controller承認の個別target `target/t6q-build/t6q-1`を使用し、0件の誤った初期実行を証拠から除外した。

時間のscopeには3回のdispatchとassert/black_boxを含む。共有ホストの高負荷と少ない標本のため、時間は探索的な観測値。実処理回数を性能回帰の判定根拠とし、本番改善率を主張しない。

## 出力と資源境界

既存のlossless snapshot helperでsource dataset、mapping、glyph、geometryのfloat bit、ruby transform、node ownership、annotation build warningを比較する。deepest hit、通常本文hit、外側miss、infinity/NaNの両公開API結果も比較する。ownerの内部identityだけは既存helperの方式で正規化する。

budgetはdefault深度4/8/12/16に加えて、深度4のshaping-run=0（警告付きprogress）、同条件warnings=0（Suppressed）、shaper-cache=0、shaped-glyphs=0（LimitExceeded）を含む。警告文字列と順序、抑制marker、error kind/limit/actualを出力oracleへ含める。

## 結果

GREENで両APIの訪問数は深度4/8/12/16で4/8/12/16回。修正前の全深度REDは15/255/4095/65535回（`2^D - 1`）で失敗した。各annotationの二重検索がなくなったことを実際の呼び出し数で確認した。

時間は各scope3回の平均、単位µs。各セルは `hit_test_ruby / hit_test` の順。

| visible depth | debug before | debug after | release before | release after |
|---:|---:|---:|---:|---:|
| 4 | 6.379 / 6.472 | 2.515 / 2.515 | 1.094 / 1.117 | 0.815 / 0.815 |
| 8 | 106.234 / 93.778 | 4.144 / 4.121 | 9.848 / 9.522 | 1.234 / 1.257 |
| 12 | 1508.832 / 1503.803 | 5.704 / 5.611 | 156.405 / 148.210 | 1.094 / 0.792 |
| 16 | 24120.539 / 24008.461 | 7.380 / 12.525 | 2591.820 / 2703.083 | 1.443 / 1.420 |

深いmissの急増は除去された。この固定入力の観測に限定する。release afterの小さい時間ではdispatchやホストの揺らぎが大きく、改善率や本番性能の推定には使用しない。

全8caseのsource/glyph/geometry、warning order/marker、resource error、hit結果は、before/afterとdebug/release/allocation buildを合わせた6実行ですべて一致した。summaryに各oracleのSHA256と照合結果を保存した。

全miss scopeは変更前後ともcalls=0、gross allocated=0、freed=0、net retained=0、peak extra=0。nested hitのcontrolは以下で両API・変更前後とも一致した。数値は3query合計で、peakはscope内の最大追加live requested bytes。

| depth | alloc/realloc calls | gross allocated bytes | freed bytes | net retained bytes | peak extra bytes |
|---:|---:|---:|---:|---:|---:|
| 4 | 6 | 120 | 120 | 0 | 32 |
| 8 | 9 | 312 | 312 | 0 | 64 |
| 12 | 12 | 696 | 696 | 0 | 128 |
| 16 | 12 | 696 | 696 | 0 | 128 |

CountingAllocatorはalloc/reallocの要求byte数を数える。grossはreallocの新しい要求全体を含み、netはliveの差分、peakは最大追加live requested bytes。query前のfont/paragraph/line/index保持と出力snapshotはscope外。今回の採用理由は重複探索の除去と互換性の確認であり、割当削減を主張しない。

## 検証と再実行

すべて同じ共通flock、jobs=1、nice=10、RUST_TEST_THREADS=2、toolchain直指定、offlineで実行。ローカルCargo設定のkacheを避けるためRUSTC_WRAPPER=''、CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=/usr/bin/envを設定した。

- baseline: `cargo test -p shodo-harness --test ruby_sources --test hit_test`（8+13 pass）
- RED/GREEN: `cargo test -p shodo --lib ruby::hit::tests`（RED: 1 fail/2 pass、GREEN: 3 pass）
- final core: `cargo test -p shodo`（786 pass、2 ignored、29 suite。既存diagnosticのselection_source_order_diagnosticとcursor_storage_diagnosticがignored）
- related harness: `cargo test -p shodo-harness --features accesskit --test ruby_sources --test ruby_input --test ruby_geometry --test ruby_paint --test hit_test`（69 pass）
- `cargo fmt --all --check`（pass）
- `cargo clippy -p shodo -p shodo-bench --all-targets --features shodo/accesskit -- -D warnings`（pass）

clippy初回はfixtureのmodulo式への`manual_is_multiple_of`指摘で失敗し、同じ入力を保つ`is_multiple_of(2)`へ修正して再検証した。

probeコマンドは`cargo run -p shodo-bench --example ruby_hit_miss`、同じコマンドに`--release`、割当用には`--release --features allocation-counting`を加える。baselineのworktreeにも同じexampleとfixtureを置いて実行し、oracle/errorをJSON値として完全比較する。各scopeのnsは3で割った平均。割当buildに時間測定はない。初期probeでJSONキー10bytesがscope内へ入る誤りを検出してscope終了を独立statementに直し、beforeの3実行をすべて取り直した。結果表は取り直し後の値のみ使用する。

要約: `docs/records/t6q-1-ruby-hit.summary.json`。raw: task worktreeの`target/performance-artifacts/t6q-1/{before,after}-{debug,release,alloc}.json`と同ディレクトリのtest/build log。workspace全体と最新mainのruby geometry変更の統合検証はcontrollerが実施する。
