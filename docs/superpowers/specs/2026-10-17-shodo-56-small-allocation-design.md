# SHODO-56: 小さな allocation の削減

## 目的と契約

SHODO-56 は、行配置、glyph itemize、句読点 geometry、reshape の hot path にある小さな allocation と不要な metrics 生成を減らす。出力 geometry、glyph と source の対応、警告順序、bidi と文字列境界の意味、cache の上限・eviction、公開 API を維持する。改善は機能ごとの allocation / work oracle と実測で確認する。

## 1. justification の opportunity 順序

`line::align::expand_opportunities` は各 opportunity の bidi level が**すべて同じ値**の場合に限り、level 配列と visual reorder 用の allocation を省く。level の parity が同じだけでは fast path にしない。uniform level `L` と paragraph base level `B` の関係に従い、`(L % 2) != (B % 2)` のとき opportunity 順を反転し、それ以外は元順を保つ。残余幅の配分と出力 opportunity の順序は現行の visual order と一致させる。mixed level は既存処理を使う。

## 2. fragment glyph start のコピー

`line::fragments` では、continuation がない場合に `starts` をそのまま参照し、glyph start 用の複製を作らない。continuation がある場合に限り複製し、従来の continuation 調整をその複製へ適用する。元の `starts` は変更せず、各 continuation が参照する start、RTL と hanging fragment の配置、描画結果を維持する。

## 3. ShapeItem の前後 context

itemize が保持する `before` / `after` context は最大 5 Unicode scalar の固定 inline 表現にし、最大 20 UTF-8 byte を保持できるようにする。`before` は通常の文字順で直前の最大 5 scalar、`after` は直後の最大 5 scalar とする。grapheme 分割、shaping の hard boundary、source の所有・借用規則は変えない。owned shape-window path も `Vec<char>` と `String` の中間値を作らず、同じ固定表現へ直接構築する。

## 4. 句読点 geometry cache

geometry cache lookup を metrics 初期化より前に行う。bounds の cache hit（`None` の hit を含む）では `GlyphMetrics` を新規作成せず、cache miss または cache されない bounds 計算で必要なときだけ作成する。全角引用符の advance を調べる処理では、その処理が必要とする metrics を生成する。既存の instance 数・glyph 数の上限、eviction、variable-font coordinates、proportional-font の判断と cache 結果を維持する。

現行 `METRIC_PROBES` は geometry cache entry の probe 数を表し、metrics / `FontRef` の生成回数ではない。cache hit で不要な metrics construction がなくなったことを示す独立した test oracle を設ける。cache hit、cached `None`、miss、新規 glyph、引用符、比例幅フォント、size / variation 違い、cache 上限を検証する。

## 5. reshape の split-cluster identity

`line::reshape::initialize_slices` は source cluster ごとに一つ作る placeholder `Arc<SharedCluster>` と、その slice ごとの一時的な Arc clone/drop を除く。現状の placeholder は slice ごとではなく split source cluster ごとに一つであり、この単位を前提に削減する。slice と group の結び付けには軽量な source-cluster ordinal / key を使い、元の cluster の text range、glyph metadata、marker sort に用いる `text.start` を保持する。group の最終 `SharedCluster` は従来どおり必要な単位で生成する。代替 metadata が新たな per-cluster `Vec` allocation を導入し、削減分を相殺しないようにする。

marker の group 間順序、storage split identity、units range、slice 内容、`parent_box`、first-line cursor、同じ cluster を共有する slice の identity、異なる cluster 間の分離、fallback は従来どおり維持する。

## 選択した方式と代案

各 fast path は既存の一般処理を保持し、条件が証明できる場合だけ小さな専用処理を使う。context は既存の 20-byte inline capacity に合わせ、reshape identity は Arc pointer identity から値のある軽量キーへ移す。計測では geometry cache probe 数と metrics construction 数を別々に扱う。一般 path を一律に置き換える方式や、削減のために別の per-item allocation を導入する方式は採用しない。

## 検証

- alignment: level 0/1/2/3 と base parity の組合せ、mixed level、残余幅が割り切れない場合、複数 count、owned overlay。既存の一般 reorder 結果と opportunity order、geometry を比較する。
- fragments: continuation の有無、複数 continuation、RTL、hanging fragment。元 `starts` と glyph start の対応および最終 geometry を比較する。
- context: 4-byte scalar、結合文字、4/5 scalar と 20-byte 上限、hard boundary、owned split/merge。scalar-clone と context content の oracle を更新する。
- punctuation: bounds hit / cached `None` / miss / uncached bounds、新規 glyph、quotes、比例幅、font size / variation、instance と glyph の eviction。geometry と metrics construction の oracle を分離する。
- reshape: marker を含む複数 group、parent box、同一 source cluster 内外の Arc identity、storage split、units range、first-line cursor、fallback。形状と allocation の oracle を比較する。
- 各機能の allocation-counting と benchmark は別 build で採り、実 fixture を使った cold/warm workload の結果と条件を記録する。punctuation は allocation 数に加えて metrics construction 数を測る。reshape は placeholder allocation と Arc clone/drop の減少を分けて確認し、新 metadata の費用も含める。
