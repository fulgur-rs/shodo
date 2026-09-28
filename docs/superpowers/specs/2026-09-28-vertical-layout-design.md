# 縦書きレイアウト設計

対象issueは `shodo-unc.2`。shodo単体で縦書きを組み、呼び出し側が正しい字形と向きで描画できることを目標とする。
writing-mode全5値、text-orientation全3値、text-combine-uprightの既存None/All、
vhea/vmtx、vert/vrt2、central baselineを対象とする。ルビは別issue。
raikiri統合spikeはマージせず、`shodo-p2m.6` には着手しない。

## 前提と選択

baseは `1aa7e5d4900382b7306d11fe91e52670a359f119`。独立worktreeは
`/home/mitz/Work/oss/shodo/target/worktrees/shodo-vertical`、branchは `feat/vertical-layout`。
変更前workspace584テストの成功記録は `target/vertical-artifacts/baseline-workspace-offline.log`。
MSRVは1.89.0。production dependencyを追加しない。

既存の論理inline/block軸を維持し、shaping itemに文字の向き、runに描画変換を追加する方式を採用する。
横書き結果全体を回転する方式は、vmtxと縦字形を利用できない。
縦書き専用の第二レイアウトエンジンは、break/cache/intrinsic/floatの二重実装になる。
採用方式では行組みが共通の論理advanceを消費し、描画だけが物理座標への最終変換を行う。

## 文字の向きとshaping

新しい `src/shape/orientation.rs` に `RunOrientation` を置く。
値は Horizontal、Upright、SidewaysClockwise、SidewaysCounterClockwise、Combined。
`resolve(writing_mode, text_orientation, base_char)` は段落modeとstyle、graphemeの代表文字から向きを決める。
HorizontalTbはHorizontal、SidewaysRl/Lrはそれぞれ時計/反時計回りでありtext-orientationを参照しない。
VerticalRl/LrのSidewaysは時計回り、Uprightは直立、MixedはICUのVerticalOrientationを参照する。
U/Tu/Trは直立、Rは時計回り。同一graphemeのmark/selectorを独立した向きに分割しない。
字形の向きはbidiレベルとは別の属性として保持する。

itemizeにwriting-modeを渡し、ShapeItemにorientationを保持する。
隣接itemの結合条件にorientationを追加する。透明なTextSource境界で共有できるclusterは共有する。
style差でgraphemeが分かれても、そのgraphemeの代表文字を使ってMixedの分類を継承する。
resource window切断、hyphen置換、line-end再shapingは元のorientationを複製し、結合条件にも含める。

直立runはHarfrust TopToBottomでshapeし、`-y_advance` を正の論理advanceとして保存する。
x/y offsetと縦origin補正を一度だけ論理軸に変換する。
sideways/combined runは通常の水平LTR/RTL shapingを使う。
Uprightの水平cursive文字はgraphemeごとに孤立shapingする。
Uprightはused directionをLTRとしてbidi解析し、元のcomputed directionとDOM/source mappingは維持する。
Mixed内のsideways Arabic/Hebrewは水平runの連続shapingとbidi順序を維持する。

CSSの直立デフォルトはvert=1、vrt2=0。sidewaysデフォルトはvert=0、vrt2=0。
明示font_featuresは既存のlast-wins方式を維持する。
作者がvrt2=1を指定した直立runでは、明示vert指定がない限りvertを無効にし、
font内の代替を利用する。CSS Mixed分類によるRの回転とfontのvrt2回転を同一glyphに二重適用しない。
作者指定のhorizontal featureは水平modeでも現行と同じように扱う。
縦kernはvkrnをfont_kerning設定へ対応させる。
cache keyは既存direction/features/instanceを使い、最終的な方向とfeature列を入力する。

## メトリクスとbaseline

縦advance/originはHarfrustの実fontテーブル解釈を使用する。
vmtxのglyphごとのadvance、top bearing、VORG、VVAR/gvarと同一variation instanceを使う。
縦テーブルがないfontはHarfrustの合成metricsを利用し、missing-fontは実サイズの1em advanceを使う。
既存FontCollection.vertical_metricsはvhea/MVARのglobal metricsであり、glyph advanceとは別物として保持する。
run出力には元のhorizontal FontMetricsと必要なvertical metricsを区別して公開する。

直立・縦中横のline boxはcentralを支配baselineとする。
fontにbaseline情報がない場合はemの左右中心を合成する。
sidewaysは水平ascent/descentを回転して利用する。
親子のbaseline合わせ、異なるfont/size、line-height、inline padding/border、
top/bottom/text-top/text-bottom/middle/sub/super/lengthを論理block軸で一貫して処理する。
VerticalLrではline-overとblock-startが逆になるため、shiftとextentsの対応を明示的に切り替える。
atomicsのcentral合成はmargin box中央、alphabeticはunder margin edgeとする。
line widthに相当する値は論理block_sizeであり、vmtx advance-heightをline widthの代用にしない。

## 縦中横

新しい `src/analysis/combine.rs` がProcessed items、inline box境界、styleからcompositionの範囲を準備する。
Allは縦typographic modeだけで有効。横書きとsideways writing-modeでは無効。
TextSourceの分割だけではcompositionを切断しないが、inline box境界、atomic、blockは切断する。
同じAll祖先内でbox境界の両側に連続文字がある場合は、CRのlook-ahead/look-behind規則に従い該当断片をcombineしない。
空boxも境界として扱う。hard breakはcomposition内部では描画・breakを生成しない。
composition開始/終了の空白は独立水平inline blockと同じwhite-space処理を行う。

composition内部はbidi-isolateとして水平shapeし、letter-spacingを無視する。
外部のline breakingには内容のfirst/last文字classを保持し、内部breakは許可しない。
compositionは1em squareを一つのUnitとして行組みに渡す。
その外部Unitは `CombineSpan.units` で選択可能なsource partsを所有し、
各partの `Unit.combine` がその所有者を指す。source partsのflat storageと
glyph ownershipを保持し、外部advanceは最後のpartへ一度だけ割り当てる。
2/3/4 typographic unitsでfontの全対象文字にhwid/twid/qwid代替がある場合は利用する。
部分coverageしかない場合に一部だけを置換して成功扱いにしない。
不足する圧縮は水平scale=min(1, em/自然幅)、短い内容は水平中央へ配置する。
複数文字のfullwidth変換は圧縮前に逆変換し、source offset mapは維持する。
central squareの中に水平glyphを配置し、markの相対位置も同じscaleで変換する。

共有GlyphStoreは元のshaping advanceを保つ。compositionのlayout advanceは1emであり、
run内のpaint penを外側のline penと混同しない。
glyph ownershipは元のTextSourceのまま。node selection/caretはcomposition内部の水平位置を
public transformで対応づけ、source byte境界とgrapheme境界を破壊しない。
spacing/emphasisは一つのcompositionとして扱う。
`Line::text_combinations()` は各compositionのprocessed rangeと1em squareを
一度だけ公開する。内部clusterは個別のemphasis対象から除き、このsquareで圏点を置く。
preserved tabも内部の水平tab stopとsource cutを保持し、glyphが無いcompositionも同じAPIで扱う。

## 公開出力の契約

既存Glyphのinline_positionとblock_offsetの結果は横書き互換を維持する。
`GlyphRunView::glyph_origin(index: usize) -> Option<(f32, f32)>` は、
font outline用の論理inline/block originを返す。block座標にはrunのbaselineを含める。
物理inline軸が負の場合は既存glyph positionに元のshaping advanceを加えたoriginを使う。
layout spacing込みのGlyph.advanceを加算してはいけない。
`GlyphRunView::orientation()` は公開GlyphOrientationを返す。
`GlyphRunView::glyph_transform()` はGlyphTransformを返す。
GlyphTransformはfont-size適用済みのoutline（x右、y下）の局所座標を、
glyph originに加算する論理inline/block displacementへ写す2×2行列である。
font variationとsynthetic skew/emboldenは現在の公開instance情報を利用する。
TCY scaleも行列に含めるため、描画側がcompositionを再計算しない。
PhysicalConverterにpoint/vector変換を追加し、directionによるorigin移動とoutlineの回転を混同しない。
変換は `physical point(glyph_origin(index)) + physical vector(glyph_transform(local outline))` の順序。
matrixはconverterのdirectionを補償するため、glyphの回転を保ちながら鏡像化を防ぐ。
矩形のRTL反転で文字自体を鏡像化しない。横書きLTR/RTLの既存出力は変えない。

Line::baseline/GlyphRunView::baselineのdocはalphabetic限定をやめ、支配baselineからの論理block位置を説明する。
hit/selectionは既存論理座標APIを維持し、物理入力を同じconverterで逆変換できるようにする。
float geometry、BreakToken、intrinsic、planned/cacheはcomposition Unitの同じadvanceを消費する。
first-line font変更はorientationとcompositionを含めて再準備し、試行行の情報を次行へ漏らさない。

## エラー、予算、性能

既存Limits、WarningSink、Saturationを維持する。glyph budget/byte windowで進捗を保証する。
compositionの全font/glyph budgetは元の段落制限に算入する。
準備は文字・glyph・box数に線形、候補行ごとに全文分類や全文再shapeを行わない。
共有run、owned overlay、cache hitが同じorientation/transformを返す。
未対応fontでも欠落と合成metricsを明示し、orientationを水平へ無言で戻さない。

## 検証と完了条件

1. 公開APIで5 writing-mode ×3 text-orientation ×LTR/RTLのadvance・字形・向き・baselineを検証する。
2. 固定実fontと再生成可能なSFNT fixtureでvhea/vmtx、vert/vrt2の異なるglyph、VORG、variable vmtxを検証する。
   期待値はfont tableまたは独立Harfrust shapeの実測から導き、単にfont_sizeを期待advanceにしない。
3. grapheme跨ぎmark/selector、共有node、Arabic isolation、vertical kerning、explicit disableを検証する。
4. TCYの1/2/3/4/長い文字列、mark、fullwidth、全/部分width-feature coverage、
   box lookaround、empty box、whitespace/hard break、fallback、source/hitを検証する。
5. planned/fresh/cache、intrinsic、float retry、first-line、small resource windowsで同じ結果になることを検証する。
6. fixtures rendererは公開run情報のみを利用し、vertical-rl/lr、sideways-rl/lr、mixed、upright、TCYのPNGを作る。
   既存26画像とgeometryを変更しない。新画像は実際に表示して字形、回転、baseline、改行を確認する。
7. workspace、fmt、Clippy、doc、MSRV1.89、wasm32、feature/allocator、snapshot、fixture regeneration、benchmarkの既存gateを通す。
8. branch全体を一度独立レビューし、重大指摘のRED→GREEN修正後にexact HEADのCI成功を確認してPRをマージする。
   issueをcloseし、このissueのworktree/local branch/ledgerを片付けて次のready issueへ進む。

## 仕様根拠と承認の扱い

- [CSS Writing Modes 4 CR 2019-07-30](https://www.w3.org/TR/2019/CR-css-writing-modes-4-20190730/): §4 baseline、§5 orientation、§9 composition。
- [Unicode UAX50](https://www.unicode.org/reports/tr50/): grapheme orientation分類。分類dataは依存ICU版に固定。
- [OpenType vert/vrt2](https://learn.microsoft.com/en-us/typography/opentype/spec/features_uz): 縦字形とfont側回転の区別。
- [OpenType vmtx](https://learn.microsoft.com/en-us/typography/opentype/spec/vmtx): glyph縦advance/bearing。
- ローカルHarfrust 0.12のglyph_metrics/ot_shape実装: vmtx/VVAR/VORGとTTB出力。

ユーザーはissueの自律実装、push、PR、CI成功後mergeを包括的に指示している。
文書が個別に人間レビューされたとは記録しない。継続承認の範囲内で設計と計画を自己照合し実装を続ける。
