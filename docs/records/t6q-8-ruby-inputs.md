# t6q.8: ruby入力metadataを準備処理へ借用する

採用。Paragraph構築中のruby入力をローカル保持し、normal `prepare`とfirst-line `prepare_alternate`へ同じ`&rubies`を渡す。中間`ParagraphData::ruby_inputs`とassign/clone/take/clear/初期化を削除する。API、既定limits、base scope budget、normal→alternateの準備順、警告順、ruby geometry計算は変更しない。

変更前は`6c660693fcc5816db426201af195de596e3ccd81`。Rust 1.96.0、x86_64 Linux、既定release profile、固定CJK font（`shodo_fixtures::FONTS[1]`のfixture checksum）、system discoveryなし。測定用exampleは`dev/bench/examples/ruby_inputs.rs`で既存`support/geometry_snapshot.rs`を再利用する。16 columns×2 annotation levelsの通常、first-line、annotation内nested rubyの3入力だけを測る。

入力RubyContent/ParagraphBuilder、fonts、LayoutContext作成はscope外で、builderを消費するbuildだけを測定する。allocatorは出力Paragraphとcontextを保持してからfinish。入力の作成は事前だが、build中に解放した入力blockはdeallocated/netへ計上する。layout、source/glyph/geometry hash、警告収集、JSONはscope外。timeとallocatorを別feature buildに分け、counts/elapsedをJSON構築前に確定する。共通flock、nice=10、jobs=1、RUST_TEST_THREADS=2、専用target `target/t6q-build/t6q-8`で実行した。

時間は保存binaryのABBA順による2 process/側、各processで11 builds/入力（22標本/側）。fontsは各process内で共有し、contextはbuildごと新規。allocatorは別binaryで11 builds/入力、以下は各fieldの中央値。nested初回にはfont cacheの初期化差があるためrawに全標本を保存した。

| 入力 | Calls 前→後 | Gross allocated bytes 前→後 | Net bytes 前→後 | Peak extra bytes 前→後 |
|---|---:|---:|---:|---:|
| normal | 5,726→5,705 | 775,483→766,747 | 170,205→169,413 | 304,436→295,724 |
| first-line | 6,396→6,375 | 926,532→917,772 | 228,082→226,978 | 366,729→362,273 |
| nested | 5,979→5,953 | 812,130→802,826 | 177,138→176,322 | 316,356→307,620 |

通常とfirst-lineで21回、nestedで26回の確保を削減。これはmetadata Vec群の短命clone除去であり、Arcで共有している注釈本文のdeep copy削減ではない。ParagraphDataから空Vec headerを外すため、このplatformでは各retained ParagraphDataが24 bytes小さくなる。first-lineでは従来clear後も残ったruby_inputs Vecのcapacityも保持しなくなる。net改善は通常792 bytes、first-line1,104 bytes、nested816 bytes。gross/net/peakはRust allocator requested block量で、RSS、stack、native malloc管理領域は含まない。

| 入力 | Build median ms 前→後 | 後/前 |
|---|---:|---:|
| normal | 0.909→0.914 | 1.005 |
| first-line | 0.994→0.996 | 1.003 |
| nested | 0.964→0.953 | 0.988 |

共有ホストは高負荷で時間差は小さく方向も混在する。全般的な速度改善は主張しない。採用根拠は小さいruntime差分、確定したallocation/一時peak/net保持の削減と出力同値性であり、raikiri/S4の必須依存にしない。

各入力の全time/allocator反復でpublic source/glyph/geometry snapshotのSHA-256が前後一致。snapshotはfont binary全体のDebugを展開せず、固定font ID/checksum、glyph/line/annotation位置、ruby transforms、source mappingを記録する既存helperを使う。build/layout warningsは全反復で前後一致（本fixtureはempty）。既定limitsでの受理と、normal/nested max_shaped_glyphs=16、first-line=32による後段prepareでの拒否も一致し、actualは17/33/17だった。既存ruby tests/harnessでfirst-line、nested、hidden/collapsed、source、budget/警告契約も確認する。

性能回帰は実際のRubyInput derived Cloneを既存GlyphStoreと同様のcfg(test) observerで計数する。2-level fixtureをbuilderとして消費する通常/first-lineのbuildで期待0。変更前はactual1でRED、変更後は0でGREEN。shipping field/counterは追加しない。nested RubyContentの共有snapshotから必要なmetadataを複製する既存経路は維持する。

検証: baseline ruby selectorは102 pass/診断ignore1（初回の誤ったruby::tests selectorの0件は証拠にしない）。最終 `cargo test --offline -p shodo` はlib464 pass/既存ignore2、integration/docsも通過。harness first_line/ruby_geometry/ruby_layout/ruby_sourcesは27+21+14+8=70 pass。bench allocator5 pass、fmt、対象3crate all-targets clippy `-D warnings`も通過。geometry.rs/geometry_index.rsの別ライン変更には触れていない。

再現は同じexampleを変更前後に置き、release通常buildと`--features allocation-counting`のbuildを保存して別々に実行する。共通flockとtoolchain/env設定は上記を維持する。

```sh
cargo build --offline --locked --release -p shodo-bench --example ruby_inputs
cargo build --offline --locked --release -p shodo-bench --example ruby_inputs --features allocation-counting
```

Raw: このworktreeの`target/performance-artifacts/t6q-8/`にbinary、全33 rows/processのJSONL、ABBA raw、summary.json、build/check logsと実行scriptを保存した。worktree cleanup前に必要なrawをcontrollerで退避する。
