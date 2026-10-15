# 非線形アルゴリズムの調査と局所的な線形化

2026年10月15日。`$simplify 線形でないアルゴリズムを重点的に` の対象を
shodo 全体として、ライブラリと開発ツールを静的に確認した。基準は
`2dd66f9`。既存の区間索引、走査カーソル、ルビ集計、定数上限を確認し、
入力と出力の大きさに対して不要に増える処理を優先した。

## 修正した2箇所

### 改行ごとの通常インライン階層の再走査

`analysis/whitespace.rs` の `close_bidi_scopes` / `open_bidi_scopes` は、
改行のたびに開いている全インラインを走査していた。深さを D、改行数を B
とすると Θ(DB)。全て `UnicodeBidi::Normal` なら制御文字は生成されず、
この積に相当する出力は存在しない。

制御文字を生成するスコープだけを指す索引スタックを追加した。通常の
open/close 管理は維持し、終了は内側から、再開は外側から行う。
このスコープ管理の処理は O(N + C) となる。N は入力の文字・項目数、C は実際に
生成する制御文字数で、全スコープが通常スタイルなら O(N)。明示的な bidi
スコープが深い場合、必要な制御文字の生成自体は省略しない。
追加の保持領域は、同時に開いている明示的 bidi スコープ数に比例する。

回帰テスト `paragraph_breaks_visit_only_active_bidi_scopes` は D と B を
それぞれ32/128に変え、通常スタイルのみと、外側・内側の2スコープだけが
明示的 bidi である場合を確認する。通常のみの D=32 / B=32 で、修正前は
2,048回の訪問によりテストが失敗した。修正後は全条件で通常スコープの
訪問が0回、明示的スコープ2個の場合は4B回となり、出力文字列も一致する。

`bidi_scope_restarts_keep_order_after_normal_and_active_scopes_close` は、
通常スコープ・明示的スコープを閉じた後の改行、複数文字の制御列、DOMと
生成文字のmappingを検証する。既存テストがblock-in-inline、明示改行、
TextBytes/Items制限を補完する。

### 深いルビへのヒット結果の経路構築

`ruby/hit.rs` の `AnnotationIndex::hit` は、再帰から戻るごとに経路の
先頭へ祖先を挿入していた。深さ D の成功hitでは、長さ D の出力に対し
D(D−1)/2個の既存要素を移動するため Θ(D²) となる。

内部再帰では末尾へ祖先を追加し、公開 `LineLayout::hit_test_ruby` で
一度だけ反転する。経路構築は償却 O(D)、領域 O(D)。経路のroot→leaf順、
最深注釈、親行、source、候補の優先順位、非有限座標の扱いを維持する。
これは経路構築の改善であり、ヒット索引の構築や候補探索全体を O(D) と
主張するものではない。

`nested_hit_path_work_is_linear_and_keeps_root_to_leaf_order` は実フォントの
既存fixtureを使い、深さ4/8/16の成功hitを確認する。要素の追加・移動・
反転を数えるテスト用カウンタは、修正前には深さ4で10となり失敗した。
修正後は全条件で2D以下となり、経路全体と最深ノードのsourceも一致する。
Vecの再割り当てはこのカウンタに含めず、標準の償却計算量として扱う。

以上は決定的な処理回数の比較であり、実時間の高速化率を測定したものでは
ない。Rust 1.91.0、cargo 1.91.0、x86_64、デバッグテストで確認した。

## 残る候補

以下はソースに基づく候補で、今回のパッチでは変更・実時間測定をしていない。
既定の深さ上限512やキャッシュ上限で制限される場合も、設定可能な上限と
入力規模を区別して評価する必要がある。

| 場所 | 入力に対する処理 | 次に必要な検証・設計 |
| --- | --- | --- |
| `ruby/measure.rs` の `Completed` | 深いbaseルビの各祖先で子孫を再集計し、鎖状の深さDで Θ(D²) | `accumulate::Tree` の区間集計を受理行にも再利用できるか確認。加算順序・飽和・clipped continuationの保持が必要。既存 `ruby_descendant_reads` で計数可能。 |
| `ruby/index.rs` の `prepare_cuts` | 入れ子コンテナが覆う同じunitへ繰り返し最終値をコピーし、区間長の総和に比例 | 区間の和集合を一度処理し、祖先コピーが内側の終端復元を上書きする順序も再現する。`safe` / `restrict` にも区間走査があり、コピーだけの変更ではbuild全体は線形にならない。 |
| `accessibility/accesskit/nodes.rs` の `reading` | 各注釈で `overflow_rect` が子孫を再帰走査した後、exportも子へ再帰。注釈ごと一定の描画要素数なら深さDで Θ(D²) | export中だけ子のローカル境界を再利用する。変換済み矩形の直接合成は丸め順序を変えるため避け、境界bit値・ノードID・出力順を比較する。 |
| `font/matching.rs` の `MatchCache::intern` | 空のキャッシュへ異なるQ個のqueryを入れると全live query照合が Θ(Q²)。既定容量は256 | query fingerprint別の照合と、毎挿入で全件走査しないWeak参照の掃除。共有queryの割り当て・保持量を維持する。満杯時のLRU選択にも別の線形走査が残る。 |
| `accessibility/mod.rs` の生成コンテンツの `from_source` | 問い合わせごとに全文字境界を走査。長い1行のN境界へN個の生成ノードを逆引きすると O(N² log N) | 生成ノードごとのmapping候補を索引化し、関連範囲だけを列挙する。affinity、first-line、反復sourceとの一致が必要。 |

`Completed` の過去の3走査融合は
[既存記録](shodo-zb0-1-completed-descendants.md)で実時間の効果が確認できず
不採用となっている。祖先間で集計を共有する案とは異なるが、定数倍の融合だけを
再び採用する根拠はない。

行整列のoverlay探索は生成側で最大2件、その他の小さなキャッシュ走査も
固定容量であるため、入力数に対する二乗処理とは区別した。ソートや出力に
必要な組み合わせの列挙も、非線形であることだけを理由に置き換えていない。

## 再現

リポジトリルートから実行する。一時ファイルは `~/tmp` を使用する。

```sh
TMPDIR="$HOME/tmp" cargo test -p shodo --lib paragraph_breaks_visit_only_active_bidi_scopes
TMPDIR="$HOME/tmp" cargo test -p shodo --lib bidi_scope_restarts_keep_order_after_normal_and_active_scopes_close
TMPDIR="$HOME/tmp" cargo test -p shodo --lib nested_hit_path_work_is_linear_and_keeps_root_to_leaf_order
```

## 検証結果

以下は全て成功。テストの成功件数にはdoc-testsを含む。スキップは既存の
ignoredテストであり、失敗は0件。Rustのテスト実行は全て `--offline`、
一時ディレクトリは `TMPDIR="$HOME/tmp"` を指定した。

| コマンド | 結果 |
| --- | --- |
| `cargo test --workspace --all-features --offline` | 110 suite、2,041件成功、9件スキップ |
| `cargo test -p shodo --no-default-features --offline` | 36 suite、1,464件成功、9件スキップ |
| `cargo test -p shodo --no-default-features --features complex-scripts --offline` | 36 suite、1,467件成功、9件スキップ |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --offline` | 成功 |
| `cargo fmt --all --check` | 成功 |
| `git diff --check` | 成功 |

全体Clippyで既存の `bool_comparison` と `needless_question_mark` が検出された。
`dev/bench/tests/suppressed_warning_format.rs` の `is_empty() == false` を
否定式にし、`dev/bench/src/bin/font_match_probe.rs` の `Ok(build_collection(...)?)`
を直接の戻り値に簡約してから全体Clippyを再実行した。どちらも等価な変更。
該当の警告テストは `cargo test -p shodo-bench --all-features --test
suppressed_warning_format --offline` で1件成功し、計測コードは
`cargo run -p shodo-bench --all-features --bin shodo-font-match-probe --offline
-- --memory registered 1` で正常なレポート出力まで確認した。このスモーク実行の
時間・割り当て値を性能比較には使用していない。

変更箇所は独立した読み取り専用レビューでも確認した。検証用の一時ログは
この記録へ条件・結果を転記後に削除する。元から存在する未追跡の差分ファイルや
mutation-testの生成物は変更していない。

後続のwall time・Callgrind・RSS比較は
[修正前後のベンチマーク記録](nonlinear-benchmarks-2026-10-15.md)を参照。
