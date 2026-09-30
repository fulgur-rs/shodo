# 実 CSS `::first-line` の caller 接続

`shodo-7ff` の実装・測定記録。`dev/raikiri` の全 git 依存は
`a62ea75b65a8547bcd0e7874addeeebee5b0beee` に更新した。旧 pin の結果資料は
履歴として保持している。

## API と範囲

raikiri-style の `cascade_with_first_line(dom, tree, media, root)` が通常の
`CascadeResult` と、同じ DOM ID に対応した任意の `FirstLineStyles` を返す。
HTML の書き換えや通常 CSS の根要素上書きは行わない。宣言候補を再利用し、
子自身の勝った明示宣言を適用する。同じ値の明示指定と継承を区別し、相対値は
それぞれの親に対して計算する。`rem` / `rlh` の基準は文書根の通常経路を保つ。

先頭行の根スロットは疑似要素の inline スタイルであり、ブロックの位置・幅は
通常の発生元から取得する。カスタムプロパティ、非継承プロパティの明示的
`inherit`、direction / writing-mode / text-orientation は通常の継承経路を使う。
今回追加した CSS-wide `inherit` の解析・解決は color / background-color /
font-size に限る。他の CSS-wide 値の一般実装を意味しない。

API は一つの block と inline/text の子孫を対象とする。存在しない・切り離された・
非ブロックの根、可視の nested block / inline-block、float、absolute / fixed の子孫は理由付きで拒否する。
display:none の部分木は省く。疑似要素規則がない場合は追加スタイルを返さない。

代表 caller は `resolve_html` / `resolve_document` を使い、通常と先頭行の値を
`open_inline_with_first_line` へ接続する。先頭行の実際の文字・font・paint・mapping
を shodo が選び、空の先頭行や幅で折り返した後も次行は通常入力を使う。
span / a / em / br、横書き LTR、静的 normal 400-weight font、絶対 spacing、
対応する whitespace / case transform / solid underline を扱う開発用 caller。
通常・先頭行の computed 値を対応フィールドに照合し、opacity、背景、非対応 font / transform、text-shadow / vertical-align / word-break 等を拒否する。
upstream は先頭行の text-wrap-mode と hanging-punctuation を解決する。
既存 shodo API が通常値を保つ先頭行 wrapping / whitespace の変更と、未対応の
hanging-punctuation は caller が拒否する。通常の nowrap は扱う。
caller が配置できない position / float と ::before / ::after の生成文字列も拒否する。複数の
装飾レイヤーや一般的な CSS レイアウトの完成を主張しない。

## 固定 WPT 比較

WPT commit: `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`。
HTML・元の rel=match 参照・bundled SFNT 88ファイルの SHA と inventory を固定し、
変更を検出したら生成を拒否する。元の DOM/CSS/resources を読み、screen、800×600、
system fonts 無効で測定する。フォントを fixture や Ahem 一種類へ置き換えていない。

通常 raikiri は同じ pin の raikiri-dom / raikiri-paint の実出力。
candidate は同じ DOM の first-line cascade を shodo へ渡す。通常 native の
ブロック位置・content 幅を共通の比較境界とし、glyph・line geometry は各実装が
生成する。candidate 参照も元の参照入力を同じ caller で描画し、backend 差を
分けて記録する。さらに通常 native の参照画像とのピクセル差も保存する。

| WPT | 通常 native 対参照の差 | candidate 対参照の差 | 判定 |
| --- | ---: | ---: | --- |
| CSS2/selectors/first-line-001.xht | 4,352 pixels | 0 pixels | 追加能力 |
| CSS2/selectors/first-line-pseudo-021.xht | 5,632 pixels | 0 pixels | 追加能力 |
| css-pseudo/first-line-opacity-001.html | 1,280 pixels | 未描画 | unsupported: opacity |
| CSS2/selectors/first-line-inherit-003.xht | 0 pixels | 未描画 | unsupported: nested block |

正の2例は candidate の元参照・通常 native の元参照の両方と差0だった。
子の `color:inherit` も green を継承する。これは共通 block 境界での inline 比較。
全ページの WPT 合格数へは加算していない。unsupported / failed は理由を保持し、
合格扱いしない。

- [入力 pin](../../dev/raikiri/data/first-line-wpt-pins.json)
- [raw 結果、glyph/font/color/source/geometry、argv](../../dev/raikiri/data/first-line-wpt-results.json)
- [PNG 一式](../../dev/raikiri/data/first-line-wpt/)

```sh
cargo +1.91.0 test --locked -p shodo-raikiri --example first_line_wpt
cargo +1.91.0 run --locked -p shodo-raikiri --example first_line_wpt -- /home/mitz/.cache/raikiri/wpt target/first-line-wpt
cargo +1.91.0 run --locked -p shodo-raikiri --example raikiri_contracts -- target/first-line-caller
```

## 検証

実 CSS の5つの子スタイル、glyph/font metrics、paint、DOM mapping、リンク、
空行・折り返し後の通常スタイル、未対応値、入力変更拒否を検証した。
raikiri style 2,642 unit tests、HTML 既存テスト、workspace 通常テスト、clippy、
公開・private・test cfg doc、patch coverage を実行。

upstream 全体 gate の ignored flex-float WPT 1件は8,944 pixelsの画像不一致。
同じ WPT checkout で変更前 `24c1abf0` にも完全に同じ不一致を再現したため、
既存失敗として記録する。全体 gate の成功は主張しない。
Rust 1.96 での Unicode 分類テストの既存不一致を避け、upstream の指定通り
1.91.0 を使った。

最終レビュー前の `fd5107a` では、shodo の stable と Rust 1.89.0 の通常/accesskit workspace tests、通常/accesskit
clippy、release cache、accessibility/emoji/ruby 実行、描画 snapshot、allocation、
指定 benchmark、no-default/complex-scripts、doc、Python（69件、3 skip）、
fixture 再生成、wasm 3構成、公開 package 内容検査が通過した。
[実行コマンドと結果](../../dev/raikiri/data/first-line-checks.json) を保存した。
両ブランチは draft PR として提出済み：
[raikiri #463](https://github.com/fulgur-rs/raikiri/pull/463)、
[shodo #104](https://github.com/fulgur-rs/shodo/pull/104)。

## 最終レビューの修正

fresh reviewer が両ブランチ全体を確認し、Critical 0 / Important 4 / Minor 0。
4件を受け入れ、1回の TDD 修正工程で扱った。

- `TextWrap` / `HangingPunctuation` の適用を直接宣言と deferred shorthand で確認し、通常スタイルを変えずに解決する。
- caller の通常・先頭行の computed 値を対応フィールドへ照合し、未投影の font / typesetting / paint 値を黙って捨てない。
- API は out-of-flow 子孫を拒否し、caller は位置指定・float・生成文字列を拒否する。
- Rust 1.98 CI が指摘した RGBA 比較の `chunks_exact` を `as_chunks` に変更する。

既存 shodo API は先頭行の wrapping / whitespace 設定を通常値に保つ。
そのため alternate 値が異なる入力を caller で明示的に拒否する。
通常の nowrap は維持する。新たな core 組版機能を追加したことは主張しない。
caller の all-target テストは93件（代表 caller 15件）、upstream の通常
workspace suite は5,120件成功、211件 ignored。最終差分 coverage は通過したが、
567件のテスト/entry ファイル行はツールが報告せず、カバー済みとは扱わない。
再測定で正の2件は元参照の両経路と差0、負の2件は unsupported のまま。

最終ソースで通常 workspace 1,052件、Rust1.91 / MSRV1.89 の caller all-target
各93件、通常/accesskit clippy、doc、fmt が成功した。accesskit workspace は
等価な let-chain 整理前に1,067件成功。実行コマンド・最終ソース SHA と結果は
上記 JSON の `final_review_fix` に区別して保存している。
upstream 最終 HEAD の GitHub CI は fmt/test/clippy、coverage、codecov/patch 成功。
shodo の最終 push 後の check / msrv / wasm を確認して完了する。
