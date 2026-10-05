# shodo-cpb: 透明な区切りの直後の grapheme 開始

## 判断

ネストした内側 ruby base の先頭にある縦中横 group が `hwid`/`twid`/`qwid` の幅 feature 選択から外れていたのは意図した挙動ではない。`itemize` が各 scalar の `grapheme_start` を決める比較対象を `breaks.graphemes` から `breaks.typographic_starts` に替えた。

## 原因

`breaks.graphemes` は `Projection::upstream` で元テキストへ写した grapheme cut で、透明な区切り（`BidiControl` item や `OutOfFlow` placeholder、Text 中の authored bidi control）の直前、つまり前の内容の終端に置かれる。改行や source 消費のための意図的な設計である。一方 `itemize` はこの cut と grapheme 先頭 scalar の offset の一致で `grapheme_start` を決めていたため、区切りの直後にある scalar は常に `false` になっていた。

ruby base は前後を isolate 制御文字（U+2066/U+2069）で囲まれるため、段落先頭以外にある base の先頭 scalar がこれに当たる。ネストした内側 base は外側 base の内容の後ろにあるので必ず該当する。shape.rs の `select_combined_widths` と `next_combined_width_group` は `grapheme_start` の数で group の grapheme 数を数えるので、`"12"` が 1 grapheme と数えられ、幅 feature が選ばれなかった。reference 経路（`CloneReference`）と再利用経路（`ScopedReuse`）は同じ shape item を使うため、どちらも同じ結果だった。

ruby 固有の問題ではなかった。probe では次の 3 つでも同じ `false` を確認した。

- out-of-flow placeholder の直後
- `unicode-bidi: isolate` の inline の先頭
- 段落途中の authored LRM の直後

段落先頭だけは `upstream(0)` が最初の投影文字を指すので `true` になり、位置によって挙動が食い違っていた。

意図した挙動でないと判断した根拠:

- `typographic_starts` は「透明な bidi control の後ろにある実際の文字開始」として `graphemes` と同じ境界列から作られ、文書化されている。
- `itemize/graphemes.rs` の prefix 修復判定は、すでにこれで「完全な grapheme の開始か」を判定している。
- 同じ `itemize` の `revert_width` も combine span の grapheme 数をこれで数えており、`"12"` を 2 grapheme と数える。幅 feature 選択だけが 1 と数えていた。

## 変更後の挙動

`true` になる scalar は投影テキストの grapheme ごとにちょうど 1 つになる。

- 区切り直後の grapheme 先頭 scalar は `true` になる。ruby base 先頭の縦中横 group は他の group と同じく幅 feature を選ぶ。
- authored bidi control の scalar は、段落途中でも段落先頭と同じく `false` になる（投影から除外され、どの grapheme にも属さないため）。この control は default-ignorable で、harfrust が glyph を除去する。shaping window がこの control で始まる場合、`leading_end` が後続 scalar を grapheme 先頭と見なすようになり、control の source を後続 glyph の cluster から分ける処理（shape.rs のコメントの意図）が段落途中でも段落先頭と同じく働く。
- `max_shaping_run_bytes` による shaping window の分割で、区切り直後も grapheme 境界として使えるようになった。これまでは同一 shape item 内で区切りをまたぐと境界が見つからず、「giant grapheme」警告を出して scalar 境界で分割していた。この警告は該当入力で出なくなる。
- 一方で authored bidi control の scalar が `false` になると、control の直前が分割点でなくなる。itemize は control を常に独立した shaping cluster にするので、その直前で分割してよい。レビューで指摘された退行（`"ab\u{200e}"`、`max_shaping_run_bytes: Some(2)` で新たに「giant grapheme」警告が出て `a|b` が分かれる）を防ぐため、splitter は Bidi_Control の scalar も分割点として扱う。
- 結合文字のように区切りの後でも grapheme の途中にある scalar は `false` のまま（既存テスト `source_gap_preserves_the_full_cluster_query` が不変）。

## 同値性

- 区切りを含まない段落では投影が元テキストと連続し、`graphemes` と `typographic_starts` が一致する。shape item は変化しない。ruby なし・区切りなしの段落は影響を受けない。既存の lib テスト 680 件と他の workspace テストは、期待値を変えずに通った（変えたのは誤りになったテストコメントだけ）。新規 5 件を含めて lib は 685 件、workspace の全 91 suite が通過した。
- ruby なしでも区切りを含む段落（out-of-flow、isolate inline、authored control）は、上記の修正後の挙動に変わる。これは同じ不具合の修正である。
- `CloneReference` と `ScopedReuse` は、base 先頭 group について出力 snapshot が一致する。外側・内側 base の glyph limit を掃引した成功・失敗・警告の結果も一致する（`base_leading_combined_group_selects_a_width_feature`）。既存の `WIDTH_PROBE_CASES` とネスト scope の同値性テストもそのまま通る。
- shodo-t3t のテストで先頭に置いた `"x"` は残した。inner base の prefix glyph 数を使う累積 limit 掃引がこれに依存するためで、誤りになったコメントだけ直した。

## 性能・安全性

走査する列を同じ長さの別のソート済み列に替えただけで、線形 cursor、比較回数（`paragraph_grapheme_start_matching_uses_linear_comparisons`）、メモリ使用量は変わらない。run-byte splitter には scalar ごとに Bidi_Control 集合（compiled data の静的な inversion list）の判定が 1 回加わるが、確保は行わない。幅 feature 選択の対象 group が増えるが、group あたりの試行は既存の上限（2–4 grapheme、limits と warning 上限）に従い、新しい計算量の経路は作らない。計測は行っていない。

## テスト

- `analysis::itemize::tests::grapheme_starts_follow_transparent_gaps`: out-of-flow、isolate inline、authored LRM の直後が `true` で、LRM 自身は段落途中（投影済み内容の直後で旧挙動では `true` だった `"x\u{200e}d"` を含む）・段落先頭とも `false`。
- `shape::tests::base_leading_combined_group_selects_a_width_feature`: ネストした内側 base 先頭の `"12"` が両経路で `hwid` になり、snapshot と limit 掃引結果が一致する。
- `shape::tests::combined_group_after_out_of_flow_selects_a_width_feature`: ruby なしで out-of-flow 直後の `"12"` が `hwid` になる。
- `shape::tests::run_byte_budget_splits_after_a_transparent_gap`: `max_shaping_run_bytes: Some(1)` で区切り直後を分割点に使い、「giant grapheme」警告を出さない。
- `shape::tests::run_byte_budget_splits_before_an_authored_bidi_control`: control の直前で分割でき、「giant grapheme」警告を出さない。
