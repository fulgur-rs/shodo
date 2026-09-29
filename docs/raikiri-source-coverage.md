# raikiri S4 の incomplete-source 診断

`shodo-t6s` は未マージS4v2の142文書・167ブロックを、元のDOM、screen cascade、フォント、計測済み幅から再実行した。167件すべてで処理済みテキスト長・受け入れた行区間を再現し、236 glyph runの使用フォントSHAも元の記録と一致した。

欠落判定の原因は、親段落から子ブロックへの引き渡しを、行だけを見る診断が数えていないことだった。この167件では未対応入力、投影の脱落、shodoの行組み不具合は再現されなかった。

| 親IFCの出力 | ブロック数 | 診断結果 |
| --- | ---: | --- |
| 行と子ブロックへの引き渡し | 108 | 行区間と生成区間で処理済みsource全域を被覆 |
| 子ブロックへの引き渡しだけ、行0件 | 59 | 生成区間と終端を確認。子のレイアウト・描画は別途必要 |
| 合計 | 167 | 実イベントから233区間の子ブロック引き渡しを確認 |

この結果は親IFCのsource診断に限る。子BFCの組版・描画、candidate全ページ画像、reference assertion、公式WPT verdict、baselineのPASS増減は証明しない。WPT image verdictは0、PASS deltaは未測定のまま。元S4の949 numeric / 395 unsupported / 167 incompleteという記録も書き換えていない。

## 原因と最小再現

```html
<div>ab<p>EF</p></div>
```

親の処理済みテキストは `ab\u{2029}`、UTF-8で5バイトになる。子の本文 `EF` は子BFCの所有で、親段落には入らない。

- `LineResult::Line` の受け入れた区間は `0..2`。
- `LineResult::BlockInInline` は子の元DOM IDを持ち、生成したU+2029の `2..5` を引き渡す。
- 最後の `Done` まで処理すると、親sourceは全域を被覆する。

`Paragraph::break_all` は行だけを返す公開APIで、子ブロックのイベントは返さない。この行区間だけを段落の全テキスト長と比べた元S4の `source_complete` は `2/5` と判定する。`break_all` の契約を変える必要はない。callerが `Paragraph::lines` のブロックイベントも保持する必要がある。

診断CLIは区切り文字の存在だけで穴を埋めない。実イベントの順序、処理済み区間、`OffsetMapping::text_to_dom` の `TextOrigin::Generated { node }` を照合し、行と子ブロック区間が連続し、終端 `Done` で全バイトを消費することを検査する。子のIDと元subtreeも出力し、子のlayout/paintを `pending-separate-BFC` と記録する。行0件の親を本文の脱落成功やWPT PASSとして扱わない。

## 再実行

```sh
cargo +stable run -p shodo-fixtures --example source_coverage -- \
  /path/to/pinned/wpt \
  /path/to/original/s4v2/wpt-batch-full/comparison.json \
  /tmp/source-coverage.json
```

元comparisonの `incomplete-source` だけを全件選択する。元resourceのバイト数/SHA、parser fetchとwarning、DOM ID/root tag、font registryのバイト列と順序、処理済み長、行区間、glyph runのフォントを照合する。空の選択や一件でも再現できない入力は非ゼロ終了し、未検証を成功として集計しない。

元の `measured_content_width` は固定800×600 viewportで得た親のcontent幅であり、この診断では再計測しない。フォントは固定raikiriの公開 `build_wpt_font_ctx` が登録した元の88 SFNTを6 generic familyの順序とともにshodoへ渡す。system fontsは無効化し、Ahemへの一律置換や開発用fixtureへの差し替えを行わない。テストだけは配布済みの固定fixture fontsを使用する。

全167件の親IFCの非初期public CSSフィールドは、`font_family`、`font_size`、`font_weight`、`line_height`、`display`、`direction`、`text_align`、`text_autospace`、`word_break` の9種類だった。実投影はこの観測範囲を型付きで変換し、親BFCの計測済みサイズ・辺15フィールドを分離した後、残余全値の等価比較で範囲外の値を拒否する。子ブロックのCSSは別BFCの所有で、親text styleとして評価しない。親/inlineのactive pseudoは今回0件であり、一般CSS adapterやfirst-line/vertical/atomic/floatの採用実装ではない。

## 証拠と後続作業

[全167ブロックの記録](data/raikiri-source-coverage.json) は文書・root/tag・処理済みテキスト・受け入れた行区間・子ID/区間/subtreeを保持する。[元入力のpin](data/raikiri-source-input-pins.json) はresource、348元ファイル、88フォント、generic順とCSS footprintを保持する。

- raikiri: `ab7e619a8f321f03de8b8c8b9342954868e044c8`
- WPT: `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`
- 元comparison SHA256: `67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d`
- baseline SHA256: `56154e4748a14a4762f1d25cf05f1c009e63151cba726fffd31dc2ebb6be0d65`

11テストは実Paragraphおよび実DOM投影を使い、イベント脱落、node取り違え、DOM本文のliteral U+2029、可視本文/Doneの欠落を拒否する。nested blockの本文を親に重複させず、後続本文も別行区間として保持する。残余CSS検査を無効化すると範囲外入力のテストが失敗することも確認した。

S4 callerへの採用・snapshot修正は `shodo-qm3` に最小再現と受け入れ条件付きで登録した。CSS flow/paint所有値の配線を扱う `shodo-0zm` とは別のsource-event診断の問題。本番切り替え必須性を根拠なく決めず、`shodo-p2m.6` の新たな依存にはしていない。

元spikeとv2 spikeは変更・push・PR・マージせず保持する。この通常issueは独立した診断example・検証記録をmainへ追加する。
