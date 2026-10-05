# t6q.2: glyph clusterの単一partをスタックに保持

採用。`shape_inputs`の通常経路では最後のpartを`Option<(Range<usize>, i64)>`に置き、run pen予算による分割が発生した場合だけ既存Vecへ追加する。公開API、limits、glyphの内部順、分割時のRTL反転、警告分岐、飽和処理は維持する。

変更前は`a62458009af2f3b3cc83441eb735b70ef8175807`。同一worktreeで実装前にcold/memoryのrelease実行ファイルを保存し、実装後の実行ファイルと比較した。Rust 1.96.0 (`ac68faa20`)、x86_64 Linux、既定release profile。`RUSTFLAGS`とrelease profileの環境上書きはなし。固定Latin/CJK/Arabicフォントは`dev/fixtures/assets/manifest.json`と`shodo_fixtures::FONTS`のchecksumを使用し、system discoveryは無効。

共通flockを保持し、jobs=1、nice=10、RUST_TEST_THREADS=2で直列実行した。cold probeは各入力・各側11回、A/B順を交互に切り替えた独立プロセス。context/font初期化はbuild時間の外側であり、buildとall_linesを個別に計測する。allocatorは`allocation-counting`有効の別実行ファイルで各入力1回。計測に時間とallocatorを混在させていない。

対象は以下5入力のscale=8のみ。全54入力の総当たりは実施していない。

| 入力 | Painted glyphs | Build calls: 前→後 | Gross allocated bytes: 前→後 | Net bytes（前後同じ） | Peak extra bytes（前後同じ） |
|---|---:|---:|---:|---:|---:|
| latin-long | 7,672 | 8,372→700 | 5,958,016→5,221,504 | 2,394,575 | 2,449,501 |
| japanese-long | 4,128 | 23,414→19,286 | 4,594,431→4,198,143 | 2,073,076 | 2,073,076 |
| arabic-long | 6,040 | 7,567→1,527 | 5,371,194→4,791,354 | 2,073,427 | 2,102,241 |
| combining-latin | 144 | 608→472 | 205,561→192,505 | 76,558 | 76,558 |
| mixed-scripts | 320 | 2,154→1,834 | 504,155→473,435 | 186,540 | 186,540 |

Buildの削減はLatin 7,672回、CJK 4,128回、Arabic 6,040回、combining Latin 136回、mixed 320回。削減した各確保は実測96 bytesであり、同じ分だけdeallocated bytesも減る。短命partの除去なのでnet保持量とpeakは改善しない。この計数はRust allocatorへ要求したblock量であり、RSS、stack、native mallocの管理領域は含まない。

| 入力 | Warm plain_lines calls: 前→後 | reuse_widths calls: 前→後 | rebuild_widths calls: 前→後 |
|---|---:|---:|---:|
| latin-long | 29,832→26,937 | 79,113→70,531 | 103,084→71,486 |
| japanese-long | 25,598→25,598 | 53,993→53,993 | 124,341→111,847 |
| arabic-long | 68,496→46,983 | 180,437→130,235 | 202,445→133,984 |
| combining-latin | 592→592 | 1,913→1,913 | 4,407→3,623 |
| mixed-scripts | 1,250→1,250 | 4,283→4,283 | 9,025→7,938 |

Latin/Arabicのplain_linesと幅再利用でも確保が減り、端reshapeへの適用を確認した。すべての計測scopeでnet/peakは前後一致。CJKなどshared glyphで完了するplain_linesは変化しない。

| 入力 | Build median ms: 前→後 | 後/前 | all_lines median ms: 前→後 | 後/前 |
|---|---:|---:|---:|---:|
| latin-long | 5.732→5.424 | 0.946 | 3.356→3.524 | 1.050 |
| japanese-long | 7.338→7.508 | 1.023 | 2.503→2.512 | 1.004 |
| arabic-long | 5.324→5.489 | 1.031 | 9.632→9.694 | 1.006 |
| combining-latin | 0.318→0.319 | 1.004 | 0.146→0.145 | 0.997 |
| mixed-scripts | 0.694→0.682 | 0.983 | 0.243→0.223 | 0.917 |

共有ホストは高負荷であり時間比は探索値。Latin buildは約5.4%短く、Latin all_linesは約5.0%長いなど方向が混在する。全般的な速度改善は主張しない。採用根拠は確定した確保削減、通常経路の小さい差分、出力等価性であり、raikiri/S4切り替えの必須依存にしない。

各入力のcold 22結果のdigestが一致し、memory probeのdefault/plain/justify/reuse/rebuild/pages/intrinsic全7digestも前後一致した。digestはsourceのbyte範囲、glyph ID/cluster、glyph位置/advance、line/fragment geometryを含む。通常のDOM owner/資源fallback契約は関連harnessで検証する。

巨大clusterの既存32-glyph GSUB fixtureを非均一glyph/advanceへ変更し、LTR/RTLそれぞれで分割後のvisual orderを直接shaper結果と照合する。pen予算、source範囲、cluster、飽和なし、Unsupported警告1件、break追加なしを検証する。変更前shape.rsと変更後shape.rsで同じ強化テストを実行し、id/cluster/advance/pen/inline・block offsets/flags/run範囲/warnings/Saturationのraw snapshotが完全一致した。raw logは各6.5KBで、フォント全体のDebug展開はしていない。

TDD: 新しいallocator regressionは変更前Latinで8,373 calls / 7,672glyphとなりRED。変更後はLatinの半glyph上限を満たす。初回GREENではCJKの共通半glyph上限が別の確保により失敗した（19,287 calls / 4,128glyph）。CJKのフォント別上限を5 calls/glyphに修正した。旧実装23,414 callsは上限20,640を超え、新実装19,286 callsは下回るため、parts Vecが戻った場合も検出できる。Arabicには半glyph上限を使う。

再現時は同じ環境で次のコマンドを実装前後に実行して各実行ファイルを保存し、coldとmemoryを別々に呼び出す。ベンチ実行中も共通flockを保持する。

```sh
export PATH=~/.rustup/toolchains/1.96.0-x86_64-unknown-linux-gnu/bin:/usr/bin:/bin
export RUSTC_WRAPPER=
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=env
export CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=2
export CARGO_TARGET_DIR=target/t6q-build/t6q-2
flock target/t6q-control/build.lock nice -n 10 cargo build --offline --locked --release -p shodo-bench --bin shodo-probe
# cold binaryを保存してからinstrumented buildを作る
flock target/t6q-control/build.lock nice -n 10 cargo build --offline --locked --release -p shodo-bench --bin shodo-probe --features allocation-counting
# 保存したcold: --cold INPUT 8 / memory: --memory INPUT 8
```

最終checks: `cargo test -p shodo --features accesskit`、関連harness（shaping/line_shaping/analysis_limits/spacing/first_line）、bench allocator/probeの両build、fmt、対象3crateのall-targets clippyを実施。全最終checksは通過。関連harness 117件、bench memory側9件（allocator 5 + probe 4）とcold側2件が通過し、通常allocator regressionもGREENになった。crate suiteには既存診断用ignore 1件がある。通常allocator testのRED/GREENと全最終checksの結果はtask reportにも記録する。

Rawと再現スクリプト: このworktreeの`target/performance-artifacts/t6q-2/`。`summary.json`、`metadata.json`、cold全sample、memory全report、split log、checks log、baseline/candidateの実行ファイルを含む。worktree削除前に必要なrawを退避する。
