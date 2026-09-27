# 日本語横書き組版の設計

## 目的と範囲

`shodo-unc.1` の四項目をコアの実際の行組みに実装する。対象は
`line-break: strict/normal/loose` の禁則差、`text-spacing-trim` の約物詰め、
`hanging-punctuation`、`text-justify: inter-character` の日本語両端揃え。
呼び出し元が受け取る行幅、グリフ位置、source range、hit testing、intrinsic sizes
が同じ組版結果を表すことを成功条件とする。

ユーザーからissueの自律実装、PR、CI成功後のマージが承認されている。
この文書はその実行のための設計であり、別途人間が文書をレビューしたとは扱わない。
raikiri統合spikeはマージせず、`shodo-p2m.6` に着手しない。

## 参照仕様

- [CSS Text 4, 2026-08-14 Working Draft](https://www.w3.org/TR/2026/WD-css-text-4-20260814/):
  §6.2、§7.5、§8.5、§9.2。ドラフトであるため、この版に判断を固定する。
- [JLREQ](https://www.w3.org/TR/jlreq/): §3.1.5 行頭括弧、§3.1.7–11 禁則と分離、
  §3.8 行調整、および文字クラスの付録。

CSSの値の意味を公開APIに反映し、日本語の分類・分離規則にはJLREQを用いる。
CSSとJLREQの伝統的な句点の空き量が異なる場合、指定されたCSSプロパティの
意味を優先する。汎用的なJLREQ完全準拠やWPT全体の合格を主張しない。

## 現状と選択

base `48a2aea9b2a65b2cb4a3ffadfaf2465362ee020d` のworkspaceテストは540件。
`analysis/breaks.rs` はICUのstrictness、word option、localeを用いている。
約物詰めとぶら下がりはstyle定義だけで実行処理がない。
`line/align.rs` はinter-characterを実装しているが、日本語約物の分離禁止を
判定せず文字境界に空きを配る。`Line::hang_start()` は現在常に0。

選択肢は、既存行組みへの統合、日本語専用行組み、描画時の補正の三つ。
既存行組みへの統合を採用する。専用行組みは既存のfloat、bidi、first-line、
source ownershipを二重実装する費用が大きい。描画時だけの補正は折り返し、
intrinsic sizes、hit testingの結果を一致させられない。

## 共通の構造

文字分類と安全な詰め量の解決を `line/punctuation.rs` に分離する。
段落準備時に各typographic characterの分類と実際のrun/font/instanceの
横書きメトリクスを解決する。単なる `font_size / 2` の無条件減算は行わない。
fullwidth opening/closing/middle、U+3000、一般Ps/Pe、およびぶら下げ対象の
分類を保持し、source/style所有者は既存item/typographic boundariesから得る。

隣接関係の詰め量は既存の `spacing_summary` と同じvisual boundaryに組み込む。
行端での詰めとぶら下げは、候補行の開始・終了・first/after-forcedフラグ、
available width、line optionsから解決する共通のedge adjustmentを用いる。
scan、cache、plan selected、intrinsicのそれぞれに別のルールを複製しない。
幅の変化が非単調になることは既存candidate frontierで扱う。

配置はwidth adjustmentとglyph leading adjustmentを分ける。
openingの左側空白を詰める場合、advanceだけでなくinkの開始位置も移動する。
共有されたshaping cluster、mark、owned edge reshapeではglyphを一度だけ
補正し、source sliceごとに同じ補正を繰り返さない。
Paragraphの共有glyph storeを変更せず、採用行のspacing/overlayに記録する。

## 禁則

ICUを基礎としてCSS §6.2の要求を表で検証し、不足だけをtailorする。
ja/zhでU+301C/U+30A0の前はnormal/looseで許可、strictで禁止。
CJ（小書き仮名・長音）、iteration marks、INの連続はnormal/strictで禁止し
looseで許可する。U+2010/U+2013の前は先行文字がIDの場合のみlooseで許可。
ja/zhにおけるlooseのcentered punctuation、East Asian PO/PRも検証する。
langなし・他言語ではlocale依存の緩和を適用しない。

既存のnowrap、keep-all、anywhere、強制改行、grapheme・transformの不可分性、
DOM境界にまたがるbreak projectionを維持する。緩和は任意のsource byteに
breakを作らず、既存のtypographic character boundaryで行う。

## 約物詰め

既存 `Normal/SpaceAll/TrimStart/SpaceFirst` を保持し、
`TrimBoth/TrimAll/Auto` を追加する。Autoは決定的にTrimBothへ解決する。
SpaceAllは隣接・行端を詰めない。Normalは隣接の指定された組合せを詰め、
行頭を詰めず、行末closingは詰めなければ収まらない場合だけ詰める。
TrimStartは行頭openingを詰め、行末はNormalと同じ。
SpaceFirstは段落先頭および強制改行直後のopeningを保持し、折返し行頭は詰める。
TrimBothはopening行頭とclosing行末を常に詰める。
TrimAllはopening/closing/middleの空白を位置にかかわらず詰める。

openingは先行opening/middle/U+3000/一般Ps、または同等以上のサイズのclosing
に隣接すると詰める。closingは後続closing/middle/U+3000/一般Pe、または
より大きいサイズのopeningに隣接すると詰める。このサイズの非対称性を保持する。
隣接は同じ行・同じIFCのvisual orderで判定し、inline text/style境界を越える。
atomic、tab、強制改行、非ゼロの介在するinline border/paddingは境界を遮断する。

Ps/PeかつCJK blockまたはEAW Fullwidth、およびU+2018–201Dの指定されたquoteを
opening/closingとして扱う。日本語のcomma/stopはclosing、colon/semicolonは
middleとして分類する。言語による中国語のcolon/dotの差も分類に反映する。

fullwidth判定と詰め量は実際のfallback face、size adjust、variation instanceを
用いる。proportional punctuationには余白を追加・削除しない。
全角のblank halfを詰める手段を採用し、hwidや異なる字形への置換はしない。
ink boundsから空白が十分でないと分かるglyphは詰めを抑えて衝突を防ぐ。
中点は左右のblankを分けて扱う。字体を縮小・clipして半角に見せない。

## ぶら下がり

firstは最初のformatted lineの始端のPs/Pi/Pf、ASCII quotes、U+3000。
lastは最後のformatted lineの終端のPe/Pi/Pf、ASCII quotes。
force-end/allow-endはCSS §9.2.1に列挙されたcomma/stopを対象にする。
端ごとに最大一文字。介在する非ゼロinline border/paddingはぶら下げを遮断する。
force_endとallow_endの両フラグが立つ場合はforce_endを優先する。

強制ぶら下げのadvanceはfit/alignment/justification/intrinsicsから除くが、
glyphと親inline box、source ownership、hit testingからは除かない。
allow-endはjustification前に自然に収まるなら保持し、収まらない部分だけを
hang量として報告する。min-contentでは条件付き対象を除き、max-contentでは
含める。trimは先に実行し、ぶら下げは詰めた後のadvanceを用いて二重減算しない。

`Line::hang_start()` と `hang_end()` に実際の端のhang量を返す。
既存trailing whitespaceのhang量も保持する。
行のinline_sizeはhangを除いたmeasure、overflow_rectは移動後のinkを含む。
first-line alternate、floatで変わるavailable width、ページ高さによる拒否と再試行、
break plan再利用でも同じtokenから同じ結果を得られることを保証する。

## 日本語両端揃え

既存のspare分配とlast-line alignmentを利用する。日本語のopening/closing、
comma/stop、中点、区切り約物、hyphen、和字間隔の両側には字間の追加を行わず、
連続したdash/ellipsisの分離禁止を守る。Han・仮名相互の境界は対象とする。
日本語以外の明示inter-characterのLatin字間拡張は保持し、cursive結合・mark・
transform continuation内部は分離しない。
word-breakの禁則とjustificationの分離禁止を同じboolで代用しない。
約物がhangしていても親boxと文字自体は行に残す。
機会がなければ既存text-align-lastのfallbackを使う。

## 検証と完了条件

1. CSS禁則matrixをja/zh/他言語、three strictness、inline境界で検証する。
2. 約物の各値、font-size差、強制/soft改行、real proportional/fullwidth fonts、
   inkの位置、複数style、RTL visual edges、mark/shared clusterを検証する。
3. first/last/force-end/allow-end、収まる場合と部分overflow、padding阻害、
   intrinsic min/max、retry、float、first-line、greedy/plannedの一致を検証する。
4. 日本語の分配対象/禁止対象、最後の行、混在Latinとcursiveを検証する。
5. 固定fontのsnapshotに詰め・ぶら下げ・日本語justifyのケースを追加する。
   default Normalが実装されて既存画像が変わる場合は差分を目視し、意図した
   geometry/inkの変化だけを明示updateする。通常checkでbaselineを書き換えない。
6. stable/MSRV 1.89.0、wasm、fmt、Clippy、doc、workspaceと既存dev gatesを通す。
   branch全体の独立レビューを一度実施し、重要指摘を修正する。
7. exact HEADのCI成功を確認してPRをmergeし、issueをcloseして所有worktreeを片付ける。

新たなproduction dependencyを追加しない。既存ICU/skrifaと固定fontを使う。
段落準備は文字数に対し線形、候補評価は既存bounded bidi summaryに従う。
候補ごとの全文再走査やvisual reorderを追加しない。

## 自己レビュー

四項目すべてに実装箇所と観測可能な完了条件がある。
NormalとSpaceFirst、conditional hangの部分量、intrinsic min/maxの差を明記した。
font_sizeだけの詰め、paintだけの変更、cacheを迂回する実装を採用していない。
未実装の縦書きとrubyは後続issueであり、横書きの四項目を縮小する理由にはしない。
