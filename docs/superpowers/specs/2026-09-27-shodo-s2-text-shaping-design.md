# S2: テキスト解析と実フォントshaping

Issue: shodo-p2m.3。基盤仕様: `2026-09-26-shodo-foundation-design.md` §2、§5、§6。

## 目的と合否

raikiriのparley置き換えのため、IFC全体を解析し、公開Paragraph/GlyphRunから実フォントのglyph ID、advance、offset、clusterを取得できるようにする。言語別text-transform、bidi、CSS改行規則、フォント選択、OpenType機能を反映する。幅変更時に段落全体をshapeし直さない。S2 issueの範囲と基盤仕様の申し送りを全て含める。S3の高度な行配置、S4のraikiri差し替え、S6の縦書き字形規則は後続issueの責務であり、本issueの完了条件に混同しない。

Rust 1.89.0、edition2024、unsafe禁止、wasm32、Send+SyncのParagraph/Lineを維持。既定のsystem-fonts/web-fontsを維持し、complex-scriptsは既定onにする。辞書/LSTMデータなしの構成ではThai/Lao/Khmer/Myanmarを書記素境界で分割する。既存の全公開入口を共通解析パイプラインに通す。

## 選択した方式

推奨: 既存IFC/item/OffsetMappingを拡張し、前処理、文字変換、分節、itemize、shapingを責務別に分ける。ノードごとに独立layoutを作る方式は空白・bidi・文脈字形を後退させる。外部layoutエンジンに段落を委譲する方式はBreakToken/floatとLimitsの契約を失う。したがって既存モデルを保持して解析部分を置き換える。

## 前処理とDOM対応

`analysis::process`はraw item全体のテキストを参照する。前後の可視文字と空白runの情報を線形走査で求め、透明なOpen/Close/OutOfFlowとbidi controlsをまたいでcollapseする。Atomicとblock/forced breakは文脈境界。Collapse/PreserveBreaksではLFの前後のcollapsible space/tabを除去。Collapseでは連続LFをcollapseし、ZWSP隣接または両隣がEAW W/F/H（Hangul除外）のLFを消し、その他は空白に変換。PreserveBreaksではLFを保持する。Preserve/BreakSpacesはspace/tab/LFを保持し、PreserveSpacesはtab/LFをspaceにする。

入力はDOM相当のUTF-8: LFがsegment break、CRはCSSのspaceとして扱う。HTMLパーサのCRLF正規化は呼び出し側の責務であり、shodoがDOM CRをLFに変えない。全制御itemに対応する生成マッピングを保持。BlockInInlineと強制改行の前でopen embed/isolateを内側から閉じ、後で外側から開き直す。閉じ直しの生成文字もLimitsに含む。

処理後にtext-transform。IFCの語文脈はノード境界で切らず、各文字の実効style/langを使う。ICU casemapの単文字full mappingとUnicode SpecialCasingの条件（前後のcased、case-ignorable、CCC、soft-dotted）を線形で求め、Turkic/Lithuanian/final sigmaの文脈変換を行う。capitalizeはUnicode word境界で最初のtypographic letter unitをtitlecaseし、後続のcaseを保持。Dutch IJ、Greek tonos/diaeresisを含むraikiriのtailoringを継承する。ASCII/halfwidth kanaのfull-width、Unicode small-kanaのfull-size-kana、case/width/kanaの組み合わせも公開TextTransformで表せるよう追加する。

Mappingは元のUTF-8 scalarごとにIdentity（同バイト長）、Expanded（長さ変更）、Collapsed（削除）を持ち、Generatedを維持。変換によってテキスト長が増えるとき、出力確保前にmax_text_bytes/u32を検査。既存Mappingを合成し、ノードやdom offsetを失わない。無効langはroot localeを使いwarningを記録する。

## 分節、bidi、改行

ICU grapheme/word/line segmenter 2.3とicu_properties 2.3を使う。ParagraphDataに処理後textのgrapheme境界と改行機会を保持。styleのline-breakはICU strictness Auto=Normal、Loose/Normal/Strict/Anywhereへ対応。BCP47のja/zhをcontent localeに渡す。WordBreakはNormal/BreakAll/KeepAll、AutoPhraseはICUにphrase規則が無いためNormal+warning（API予約値の明示した劣化）。TextWrapMode::NoWrapはsoft/emergencyを禁止。NBSP/WJ/ZWJ/combining/emoji sequenceを壊さない（line-break:anywhereはUnicode grapheme境界で通常の禁則を無視する）。

改行はProhibited/Allowed/Mandatory/Emergency/Hyphen。overflow-wrap:anywhereはmin-contentにもEmergencyを算入、break-wordは通常min-contentに算入しない。hyphens:noneではSHYを無視、manual/autoではSHY位置をHyphenにする。auto辞書ハイフネーションは現在利用可能な辞書がないためmanualへ劣化してwarning。SHYは通常描画でゼロadvance・非表示、採用した改行だけハイフン字形をshapeして出力する。line scanner/cache/intrinsic/planで新分類を一貫して使う。

unicode-bidiは現在の生成controlsを維持し、段落ごとの基本方向とbyte levelを保持。fast pathは基盤仕様の条件に限る。RTL bufferのglyph順をlogical cluster順に正規化し、cluster内のvisual位置を保持して既存の論理座標出力と整合させる。実shaperの出力を二重に反転しない。

## フォント、itemize、機能

各graphemeにscript、bidi level、shaping style、FontMatchを割り当てる。Common/Inherited/script extensionsは前後のstrong scriptへ解決し、括弧と結合文字の文脈を維持。S1 `match_cluster`のFontQueryにfamily/weight/width/style/lang/scriptを渡す。返却variationsにfont-variation-settings/opszを重ね、サイズ調整・合成styleを保存する。欠落はfontなしの.notdefで内容を保持してwarning。OS非依存テストはsystem_fonts:falseと固定fixtureを使う。

連続した同一shapingパラメータのtextは透明なitem境界をまたいでshape可能。atomic/controlや実効font/script/level/features/lang/variation変更でrunを切る。接合だけが必要な境界ではpre/post contextを渡す。描画GlyphRunは元itemのnode/styleを保持して分割する。跨るclusterは1回だけ保持し、cluster開始文字のitemがglyphを所有する。他itemのDOM対応はOffsetMappingとclusterの全text範囲に保持し、glyphを複製してadvanceを二重計上しない。line-breakingはそのclusterを分割しない。

FontVariant型を追加: ligatures、caps、numeric、east-asian、position、alternatesの解決済みfeature指定。優先順はfont標準feature→kerning/variant→font-feature-settings（後勝ち）。feature rangeはshaping runに相対化して渡す。CSS spacingが任意ligatureに与える規則も適用。Synthesis/normalized coordsをGlyphRunから読み出せるようにする。

## shapingと資源

harfrust0.12、FontCollection共有ShaperData、LayoutContextに最大64件のfont/instance/script/lang/direction/features対応ShapePlan LRUを置く。UnicodeBufferはclearして再利用、段落結果はSoAに1回保持。glyphごとにUNSAFE_TO_BREAK/UNSAFE_TO_CONCATを保存しPRODUCE_UNSAFE_TO_CONCATを有効にする。font unitsを実使用size/upemでLayoutUnitに丸め、offsetのblock符号を論理座標へ変換。累積penはRUN_PEN_LIMIT前でrun分割。

max_shaping_run_bytes既定64KiB: 直近grapheme境界で分割。1graphemeが大きすぎるとcodepoint境界で強制分割しwarning（基盤仕様§5.3、内容を拒否しない）。0や1..3bytesでも必ずcodepoint1個を処理して停止性を保つ、最小scalarを超える例外をwarning。max_shaped_glyphsはharfrust出力をSoAへ追加する前に段落全体・first-lineの合計を照合。first-lineのtext/items/styles/shape集合は全入力解析を共有する別の不変データとして保持し、先頭行だけ選び、BlockInInline後は再適用しない。

行端がunsafeなら両側のsafe境界まで再shape。UNSAFE_TO_CONCATの接合を検査し、window上限内で拡張。overlayのglyph数は共有結果と異なってよい: RecordKind/GlyphSourceが共有rangeとoverlayrangeを別に持ち、glyphs/clusters/positions/hitが実sourceを使う。完全cluster単位で差し替え、予算不足なら元clusterを保持してwarning。ハイフン合成も同じ予算を使う。Lineのowned overlayはParagraphを変更しない。

LayoutContext::shrink_toはpartial、buffer、plan cacheの保持量を合算して縮小し、0なら全解放。build/next_lineのLimitsに応じて自動縮小する。!Syncを型で明示する。word cacheは導入せず、固定fixtureのcold/warm測定を記録して将来の導入判断材料にする。harfrust失敗状態は公開されていないため観測できたかのようなwarningを作らず、上流提案用文書を残す（送信は本issueの要件にしない）。

## 検証

ノードをまたぐ空白とsegment break、全transformとlocale、bidi境界、全line-break/word-break/overflow-wrap、SHY、Arabic joining/marks/RTL、Latin ligatures/kerning/features、CJK locale/fallback、cross-node shaping、first-line、所有フォント層破棄後の描画、window/出力予算/巨大grapheme/極小run上限、Mapping往復、warm plan reuse/shrinkを固定fixtureで検証。既存S0行機構テストはfontなし.notdefの決定的幅で機構を維持し、実字形を検証するテストとは目的を区別する。

stable/MSRV workspace、root no-default、complex-scriptsのon/off、fmt、Clippy warnings deny、wasm、rustdoc、fixture Python/checkを実行する。1回のfresh whole-branch reviewを行い、Important/CriticalをRED→GREENで修正する。正確なPR HEADの全CI成功後にmergeし、issue closeとworktree cleanup。

一次資料: [CSS Text 3](https://www.w3.org/TR/css-text-3/)、[ICU casemap](https://docs.rs/icu_casemap/latest/icu_casemap/struct.CaseMapperBorrowed.html)、[Unicode17 SpecialCasing](https://www.unicode.org/Public/17.0.0/ucd/SpecialCasing.txt)。harfrust/ICU segmenterのAPIは手元の0.12/2.3 sourceで確認。
