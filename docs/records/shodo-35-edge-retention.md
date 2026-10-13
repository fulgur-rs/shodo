# SHODO-35: 長い段落の edge cache 保持

## 課題と条件

`arabic-long/64` 相当の本文（fixture の Arabic 文を 12 × 64 回、間を空白で連結）を固定幅 420 px で再レイアウトすると、先頭の glyph snapshot が 1 MiB の保持予算を使い、後半の候補が毎回 shaping される。単純な上限増加や LRU の採用だけでは、この反復時の作業量と出力・資源制約の両立を確認できない。

回帰テストは system fonts を無効にした checked-in Arabic fixture、16 px、`lang=ar`、RTL、通常の改行候補を使用する。本文はベンチマークの sentence 数に合わせた。短い 64 sentence の段落は元から再 shaping がゼロだったため、容量不足の回帰テストには使わない。

## 設計

描画用 glyph/run snapshot と、候補の幅差分・部分 cluster の妥当性判定を分離して保持する。判定結果には selected source と remaining suffix の検証を含め、paragraph owner と unit range をキーにする。既存の glyph snapshot は glyph budget をキーに残す。編集された SHY 等の描画経路は従来の shaping を実行する。

判定結果の再利用には既存の `line::replay` を用いる。reshape の合計 charge、受理/拒否の状態、警告の suppression 状態、saturation の差分が一致する場合にのみ replay する。境界を越える charge や警告を発した測定は fresh path に戻る。単なる width の一致を副作用の一致とみなさない。

glyph snapshot の accounted cost と判定レコードの実際の vector capacity を合わせた上限は 1 MiB のままにする。glyph が候補を圧迫しないよう、判定結果用に予算の 1/4 を予約する。owner の変更と `shrink_to` は両方を解放する。

shape-level の advance/cluster/flags snapshot も検討した。各 glyph 最低 9 bytes に加えて vector と entry の metadata が必要で、7,090 件の shaping 窓を保存するのは候補の scalar 判定結果より大きい。候補と描画の glyph 所有化を分離した SHODO-36 を基盤とし、候補 scalar の保持を採用した。

## 初期比較

テスト時の shaping 呼び出し数。時間や allocator の測定値ではない。

| 保持方式 | 初回 420 px | 同幅再反復 | 状態 |
| --- | ---: | ---: | --- |
| 元の glyph cache | 7,090 | 6,321 | 後半を毎回 shaping |
| scalar + inline Effects、1/2 予約 | 7,762 | 2,013 | glyph の保持領域が減る |
| scalar + inline Effects、1/4 予約 | 7,426 | 1,869 | glyph 577、判定 2,301、951,952 accounted bytes |

1/4 の初期案は固定幅の warm shaping を 70.4% 減らした（初回は 4.7% 増）。一方、別幅を追加すると判定領域が 3,584 件で満杯になり、240 px の幅変更初回 10,798・再反復 8,355、480 px は 6,961、320 px は 8,882 となった。各幅の fresh context は順に 10,321・5,902・7,943。幅変更初回を改善と仮定せず、各 phase を別に評価する。

幅変更ごとの全面 clear は、幅を循環させる使い方の再利用を失うので採用しない。同一の Effects を共有し、scalar entry を軽くする方式を採用した。

## 共有 Effects と最終カウンタ

unit range の保持キーは u32、幅差分・妥当性・共有 Effects の index を合わせて entry 16 bytes。範囲外の unit index はキャッシュを使わず測定する。Effects は charges の bytes/min/max/受理・拒否、saturation の両カウンタ、suppression を含む完全な値で intern する。HashMap lookup によって重複を調べ、index 用の append vector は既存 record を移動しない。pool が満杯のときの更新も通常の測定に戻る。

entry vector と Effects vector の actual capacity、intern table の 4 slot/entry と control bytes の allowance を共通の 256 KiB に数える。glyph snapshot の予算は残りの 768 KiB。新規容量は予測値で拒否してから必要な allocation を行い、actual capacity でも再確認して保持する。異なる Effects を大量に入力した場合も上限を越えないことを試験した。

| phase | shaping 呼び出し | glyph entry | scalar entry | Effects 種類 | accounted bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| 420 px 初回 | 7,426 | 577 | 2,301 | 4 | 824,720 |
| 420 px 同幅反復 | 1,869 | 577 | 2,301 | 4 | 824,720 |
| 240 px 幅変更初回 / 反復 | 10,798 / 2,832 | 577 | 6,294 | 25 | 895,376 |
| 480 px 幅変更初回 / 反復 | 6,859 / 1,998 | 577 | 8,133 | 29 | 926,096 |
| 320 px 幅変更初回 / 反復 | 8,744 / 2,557 | 577 | 10,998 | 35 | 973,456 |

420 px の反復は phase 前後の counter 差分で 2,301 hit / 2 miss、計 2,303 候補中 99.9% が scalar cache hit。残りの描画処理は owned glyph を生成する。幅変更初回は、先に保持した glyph が別幅の新しい window の admission を拒否する影響で fresh context より呼び出し数が増えることがある。この tradeoff を残したまま、反復時には各幅の候補を共有容量内に保持できる。時間と allocator の paired A/B は別途の正式測定を参照する。

共有の完全性は limit・受理/拒否・saturation・suppression が異なる記録、通常の charged/refused replay gate、部分 ligature の suffix、owned path との side effects 一致で確認する。24 個の短い段落を一つの context で切り替えた試験では、fresh context と同じ text range/width/warnings を返し、同じ段落の再反復は再 shaping しなかった。

## 再現

```sh
TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_BUILD_JOBS=3 \
  cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' \
  test -p shodo --lib long_arabic_relayout_reuses_width_measurements_after_glyph_retention_fills -- --nocapture
TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_BUILD_JOBS=3 \
  cargo +stable --config 'target.x86_64-unknown-linux-gnu.runner="env"' \
  test -p shodo --lib line::windows
```

runner の上書きは開発環境の read-only cache runner を回避するためで、通常の環境では不要。親の CPU 2 固定 A/B 測定と重ならないよう、この作業の後続検証は `taskset -c 3-11` を付ける。成果物は repository の record に残し、worktree 内の使い捨て target は統合後に削除する。

## 検証結果と範囲

- 長い fixture の shaping 呼び出し回帰は元の保持方式で 7,090 / 6,321 を確認した RED から開始。6000 件の repeated-effects 保持も inline-effects 方式で RED を確認してから intern 方式で GREEN とした。
- 最終基盤 SHODO-36 の compact specialization を統合した `ee39a4a` 相当では windows 32 件が成功した。その後、Effects の limit/outcome/saturation/suppression 区別と、charge 上限なしの zero-delta edge の回帰を追加した。zero-delta は末尾の unsafe flag を立て、実際の shape call を先に確認して、旧 guard の RED → 最終 guard の GREEN を確認した。新 guard は selected edge または covered partial suffix の仕事がある場合に保持し、empty-noop だけを省く。最新 windows は 34 件成功。
- `ee39a4a` を基盤とした `cargo test --workspace` は最初の shodo unit suite 970 件で 954 成功・8 ignored・8 失敗となり、後続の workspace suites は実行されなかった。失敗はすべて native-font fixture が `~/tmp` に書けない sandbox の `ReadOnlyFilesystem` によるもの。既にビルドした同じ workspace binary の `font::` 180 件を writable environment で再実行し、すべて成功した。full workspace 成功とは扱わない。
- 全 workspace/all-targets の Clippy は intern 版 `575b446` で成功。最終基盤と guard を含む最新版は通常と `--features shodo-harness/accesskit` の両方で `clippy --workspace --all-targets -- -D warnings`、および `cargo fmt --all --check` が成功。最終 PR の exact HEAD に対する full workspace/feature/MSRV は CI で確認する。

sandbox で失敗した 8 件は次のとおり。production の変更や fixture の削除では回避せず、実行環境を変えて再検証した。

- `font::matching::retention_tests::native_file_choice_does_not_depend_on_lazy_source_id_assignment`
- `font::matching::selection_tests::best_candidate_matches_a_stable_sort_for_every_candidate_order`
- `font::matching::selection_tests::best_candidate_ranks_color_width_style_then_weight`
- `font::source::tests::catalog_index_releases_path_blobs_without_source_cache_eviction`
- `font::source::tests::changed_path_source_never_registers_a_stale_local_name`
- `font::source::tests::local_aliases_share_document_blob_after_source_cache_eviction`
- `font::source::tests::selected_native_verification_limit_stops_before_data_fallback`
- `font::source::tests::unavailable_or_renamed_native_candidate_uses_the_next_exact_name`

## 最終版の独立 A/B

実装 `585387e` と SHODO-36 compact 基盤 `c26aa6f` を、同じ固定 fixture/lock/Rust 1.97.1/release opt-level 3、CPU 2、warmup 3、独立 3 process × 21 samples で交互に測定した。時間と allocator instrumentation は別 executable。構築は範囲外、出力の収集・破棄は範囲内。

| workload / operation | 36のみ ms | 35追加 ms | 比 | allocation calls 36→35 |
| --- | ---: | ---: | ---: | ---: |
| Arabic-long/8 固定反復 | 1.9117 | 1.5888 | 0.8311 | 8,536→6,352 |
| Arabic-long/64 固定反復 | 71.6011 | 31.2010 | 0.4358 | 283,205→113,818 |
| Arabic-long/64 3幅循環 | 195.2361 | 87.2750 | 0.4470 | 848,333→353,803 |
| Latin-long/64 固定反復 | 21.4617 | 13.8325 | 0.6445 | 124,853→69,921 |
| 多数短段落/64 | 1.4586 | 1.4654 | 1.0046 | 9,799→9,991 |
| Latin-short/1 | 0.0164 | 0.0149 | 0.9116 | 104→92 |

多数短段落では owner ごとの compact cache 初期化が 192 allocation calls / 130,304 requested bytes を追加した。時間差は小さいが、allocation 改善とは扱わない。最後の context の live requested bytes 差は 2,164 bytes。長文では gross allocation と保持 bytes が減った。幅変更初回の tradeoff と併せて、全 workload に同じ効果を主張しない。

旧基準 `c892cae` と最終 35+36 の warm Arabic-long/64 一回だけを Callgrind で取得すると Ir は 930,062,939→388,065,804（約58.3%減）。これは命令参照数であり実時間ではない。時間の表は36→35だけの比較、Irは旧基準→35+36の比較なので混同しない。

独立レビューは必須修正なし。813ケースの各public/manual/internal経路の全出力・警告が基準と完全一致し、837,421,847 bytes正規化SHA-256は `d22e6d8a0201bfe2ee40201eba58ef6e5704cedc45929d99e8637c83bc8c0616`。別driverで6 workloads ×3幅×4 cycleのglyph/source mapping digest・矩形bits・警告も一致した。元のedge_cache_eviction内部/manual自己比較は基準でもblock offset差で失敗するので、同経路を変更前後で直接比較する一時driverを使い、元fixture/期待値は変更していない。

[全samples・allocation・profile要約・再現source/manifest/lock](shodo-35-measurements-2026-10-14.json)を保存した。JSON driver_source / snapshot_source / width_validation_source / profile_sourceを~/tmpの一時crateへ置き、manifestのCHECKOUTを該当revisionのcheckoutへ置換してoffline buildする。時間版を保存した後、allocation-counting版を別保存してCPU2で交互に実行する。CPU frequency scaling/boostと固定fixture/standaloneの限界を保持する。最終HEADの全workspace・feature・MSRV・Wasmの証明はPR CIを確認してからmergeする。
