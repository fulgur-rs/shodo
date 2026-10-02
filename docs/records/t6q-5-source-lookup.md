# Accessible source inverse の共有 mapping 索引

対象: shodo-t6q.5。base `6c660693fcc5816db426201af195de596e3ccd81`。

`AccessibleLayout::from_source` は各 accepted line で段落全体の mapping を走査し、preferred affinity の判定でも再走査していた。既存の lazy `OffsetMapping` interval index を拡張して DOM offset に対応する閉区間候補を得る。dataset 全体で preferred affinity を確定してから候補を text offset 順に並べ、同一 mapping を共有する行には同じ候補を使い、行の閉区間を binary search で選ぶ。初回の索引は既存の DOM/text/generated 索引一式を保持し、query ごとの候補と dataset cache は返却前に解放する。

first-line の変換では等しい data.id が別の mapping を指す場合があるため、cache の同一性は実際の mapping reference の pointer で決める。accepted line の順序と重複は維持し、既存 caret normalization、行内 sort、最後の dedup を使う。Generated source の逆引きは従来の経路。

## 回帰と測定

固定実フォント Latin を使う 64/256/1024 node・forced line 入力。RED は 64 行で 8,192 mapping record 訪問により失敗した。GREEN では shared dataset に対する interval 訪問数を数え、行ごとの全文走査を禁止する。別テストで行外の preferred candidate、first-line の別 dataset、繰返し/逆順/同じ line の重複、Collapsed/Expanded、両 affinity、存在しない source と saturated/empty DOM range を従来の linear oracle と照合する。

`dev/bench/examples/accessibility_source_lookup.rs` は system font を使わない fixture font、unique/repeated source の 32/128/512 行と、小さい first-line/collapse/expand/generated source 入力で A/B を行う。shaping bytes=0、warning cap=0、glyph cap=0、mapping 無効を含む11ケース。大きい入力は geometry/glyph digest、dataset ごとの canonical source hash、逆引き/順引き query hash。小さい入力は full snapshot と全 source query の完全比較を使い、多行ごとの巨大 JSON を作らない。private lazy index 状態は mapping clone で比較から除く。

`AccessibleLayout::new`、cold source inverse、warm 3 回の inverse を別 scope で測る。出力 vector は scope 終了時に保持し、layout/build、source hash、順引き、JSON は inverse scope の外。時間は counter-free release build、allocator は allocation-counting release build。共通 flock、nice=10、jobs=1、RUST_TEST_THREADS=2、Rust 1.96.0、専用 target を使用した。高負荷共有ホストの時間値は探索値であり、一般的な改善率ではない。

## 結果と採否

debug の実計数: RED は元のlinear filterへcfg(test)計数を置き、64 行で 8,192 record 訪問。GREEN は warm lookup で 64/256/1024 行とも interval index 訪問 4 回（各2反復）。node の BTreeMap key 比較はこの計数に含まない。cold の索引構築は一度 dataset 全体を処理する。candidate の query は dataset ごと、行ごとの処理は候補に対する range binary search なので、少数候補での行数×全文 mapping 走査を除く。

before/after を交互に9回ずつ測った counter-free release の中央値（warm は3 query 合計）:

| source | lines | cold before→after µs | warm 3 before→after µs |
|---|---:|---:|---:|
| unique | 32 | 3.283→22.420 | 5.727→2.864 |
| unique | 128 | 20.534→55.177 | 55.247→5.238 |
| unique | 512 | 249.904→143.251 | 730.155→9.010 |
| repeated | 32 | 8.591→12.223 | 20.395→9.499 |
| repeated | 128 | 90.519→26.332 | 253.535→30.801 |
| repeated | 512 | 1369.442→90.589 | 4321.631→150.865 |

small mixed の cold は 2.793→11.873 µs。索引未構築の小さい入力では遅くなり、多行で繰り返す lookup を対象にした変更である。`AccessibleLayout::new` のscopeはsource inverseの前に独立して測定した。全ケースの min/max とallocationは隣接 summary JSONに保存した。

allocation-counting release の3交互反復は各scopeの calls/gross/freed/net/peak が完全一致した。代表 scope は次の通り（bytes、warm は3 query合計、返却vectorを保持）:

| scope | calls before→after | gross before→after | freed before→after | net before→after | peak extra before→after |
|---|---:|---:|---:|---:|---:|
| unique 512 cold | 1→1691 | 128→259648 | 0→154992 | 128→104656 | 128→105056 |
| unique 512 warm | 3→9 | 384→1584 | 0→1200 | 384→384 | 384→784 |
| repeated 512 cold | 8→48 | 32640→164768 | 16256→74040 | 16384→90728 | 16384→95192 |
| repeated 512 warm | 24→51 | 97920→123504 | 48768→74352 | 49152→49152 | 49152→53616 |

cold unique512で既存 lazy index の追加保持は104,528 bytes、repeated512で74,344 bytes。warmの返却vector net保持は変わらず、候補vectorとdataset cacheの一時割当が増える。新たな永続AccessibleLayout cacheは追加しない。索引がすでにtext_to_dom/dom_to_text等で使われているdatasetではこのcold構築を繰り返さない。

初回4buildと交互24runすべての11ケースで、source/glyph/geometry、完全small snapshot・query、warnings/shape fallback/warning suppression/glyph resource error が等価だった。共通lockで競合を避けても共有ホストの負荷・schedulerは残るため、時間比は一般化しない。採用理由はmulti-line反復lookupの走査削減と等価性。cold小入力の時間・保持量とwarm一時alloc増加は明示的なtradeoffとする。

raw: worktree `target/performance-artifacts/t6q-5/`。baseline/候補のtime/alloc binariesも保存し、交互比較は同じprobeのimmutable binariesを使用した。専用targetで他worktreeのcargo成果物混入を避けた。

## 検証

- baseline harness accessibility/accesskit: 24 pass。
- regression RED: 2 pass/1 failure（64行8,192訪問）。GREEN最終fixture: 4 pass。
- `cargo test --offline -p shodo --features accesskit`: 29 suites、794 pass、既存診断2 ignored（selection_source_order_diagnostic / cursor_storage_diagnostic）。
- `cargo test --offline -p shodo-harness --test accessibility --test accessibility_accesskit --test hit_test`: 26 pass。
- `cargo fmt --all -- --check`: pass。
- `cargo clippy --offline -p shodo --all-targets --features accesskit -- -D warnings`: pass。
- `cargo clippy --offline -p shodo-bench --example accessibility_source_lookup --features allocation-counting -- -D warnings`: pass。

