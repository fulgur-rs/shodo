# S3: 横書きの行組み

対象: `shodo-p2m.4`。基点: S2 PR #7 の merge `52ed6d06`。

## 目的と完了条件

raikiri が IFC 全体を増分的に組み、実フォントの出力を描画し、その同じ配置に対して座標・キャレット・選択を問い合わせられること。S3 issue の全項目と基盤設計 §8 の S3 契約を扱う。編集操作、raikiri の本番切替、縦書き shaping、ルビは各既存 issue の責務であり、今回新しく範囲を縮めるものではない。

既存の next_line/token、float の先読み・再試行・取り消し、BlockSizeExceeded、BlockInInline、first-line、intrinsic、BreakPlan、揃えを実フォントと新しい spacing に接続する。既存テストがある項目も、その契約を最終監査の対象に含める。glyph/font/coords/synthesis/source と装飾メトリクスを公開出力から取得できること、異常値と小さい予算でも停止しソースを失わないことを確認する。

## 現状と方式

S2 は GSUB/GPOS、contextual edge windows、joint SHY、first-line と共有 cluster を実装済み。一方、line/metrics.rs は 0.8/0.2em の extents、tab は space=1em、letter/word-spacing と text-autospace は配置に未適用、hit API は未実装。

採用する方式は既存の immutable ParagraphData と行単位の出力を拡張する方式。全面的な行分割器の置換は既存 float/cache 契約を再実装する費用が大きい。描画側だけで spacing/hit を補正する方式は、改行・intrinsic・表示が異なる幅を使うため採用しない。

新しいコードは (1) 解決済みの font/inline metrics、(2) 候補測定と行配置で共有する spacing、(3) 最終 Line 出力を読む hit/index に分ける。段落を行ごとに複製せず、自然な shaping advance と layout advance を引き続き区別する。

## フォントと行メトリクス

FontMetrics に x_height/cap_height を追加し、Skrifa の同一サイズ・normalized coordinates から取得する。未収録値には明示的な em 比率の fallback を使用する。run の実 font_size（font-size-adjust 後）と座標で ascent/descent/line_gap/装飾値を解決し、GlyphRunView::metrics() で公開する。Cluster::source_char は cluster の processed source text の先頭 scalar（generated SHY でも元の U+00AD）であり、DOM の変換前文字は OffsetMapping と呼び出し側 source に委ねる。

strut と inline box の第一フォントは CSS query による primary face を使い、Collection の登録順 primary_font を選択処理の代用にしない。実際の run は fallback face のメトリクスを使う。空の inline box、tab、forced break も所属 inline のメトリクスで参加する。解決結果は build 時に style/run ごとに保持し、同じ line/fragment 問い合わせで font parsing を繰り返さない。

normal の行高さは実 ascent+descent+line_gap。Px/Number は computed font size から解決し、自然な font extents に半 leading を加える。負の半 leading と zero-height strut を保持する。inline border/padding の block 側は line height に加算しない。font fallback と複数 run が同じ inline にある場合は実メトリクスの最大寄与を保持する。

vertical-align の Baseline/Length/Sub/Super/TextTop/TextBottom/Middle/Top/Bottom を扱う。TextTop/Bottom は親の実 font extents、Middle は親の x_height/2、Sub/Super は利用可能な font 情報と文書化した fallback に基づく。Top/Bottom の aligned subtree は行全体の高さ確定後に一度配置する。深い inline の baseline 集計は iterative traversal で行い、unlimited nesting でも再帰 stack overflow を避ける。atomic baseline は required_baseline、margin-box synthesis、top/bottom、負の margin の既存契約を維持する。

## spacing と改行

processed typographic character units を ICU grapheme から得る。透明な inline/out-of-flow/control は文字間の連続性を壊さず、atomic/tab/forced/block はそれぞれの spacing 契約を持つ。text の追加や shaping のやり直しによって疑似 space glyph を挿入しない。

letter-spacing は bidi reordering 後の両隣の各半分を合成し、行の両外側を除く。異なる style の境界では平均を使う。formatting-only units と combining marks に追加しない。連続 atomic は一つの typographic unit として扱う。負の値も飽和演算で扱い、natural advance を変更しない。低レベルの feature により残る ligature 内でも typographic 境界の spacing を失わない。

word-spacing は対象の word separator の layout advance に追加する。preserved/hanging/collapsed spaces、NBSP、generated SHY と default ignorables を区別する。Spaces tab interval は該当 style の選択フォントの U+0020 advance と適用 spacing を使い、Px tab は直接長さを使う。位置・indent・inline-start offset・float retry のたびに tab を再評価する。

TextAutospace::Normal は ideograph と non-ideographic letter/decimal digit の visual 境界に追加する。ICU script-extension/category/EastAsianWidth で分類し、1/8ic は境界を包含する最内 inline の選択フォント・size から解決する。NoAutospace は無効化する。空白・句読点・atomic・非ゼロ margin/border/padding が境界を遮る。inline style/first-line、bidi reorder、透明 float の跨ぎを処理し、改行で切れた境界には追加しない。

候補の幅、cache frontier、float callback 位置、intrinsic、Balance/Pretty が同じ spacing 計算を使う。自然な contextual window の差分に、可視 source units と synthetic hyphen の spacing を合成する。unsafe window が拒否されたときの whole-cluster fallback も同じ規則。選択した行では同じ結果を shared/owned glyph positions と cluster advances に反映し、mark attachments を維持する。justification はその上に追加し、別 Line や Paragraph の glyph arrays を変えない。

candidate ごとに paragraph 全体を bidi reorder しない。bounded bidi-level summary で first/last typographic unit と境界 spacing の合計を維持し、候補検査を文字列長に対して二乗にしない。位置依存 tab は既存 cache の契約に従う。実装の計算量は counter regression と small-prefix の独立 oracle で検証する。

## ヒットテストの公開契約

`pub mod hit` を追加する。`LineLayout<'a>::new(&'a [Line])` は呼び出し側が受理した Line を借用し、immutable な caret/selection index を一度構築する。float/高さ再試行の未受理候補は渡さない。Line ごとの processed dataset をそのまま使い、first-line と normal で offset が異なる場合にも無理に一つの文字列の offset と見なさない。

公開値は `TextPosition { line: usize, offset: u32, affinity: Affinity }`、`HitResult { position, origin: Option<TextOrigin>, inside: bool }`、`Caret { position, rect: LogicalRect }`、`CaretDirection::{Backward,Forward}`、`NavigationOrder::{Logical,Visual}`。offset は指定 line の Line::text の UTF-8 byte offset。origin はその Line の optional OffsetMapping から取得する。mapping 無効時も processed position と幾何問い合わせが動く。

`hit_test(inline, block) -> Option<HitResult>` は Line の block_offset と最終 inline placement の座標空間を使う。領域外は最寄りの行/stop に丸め inside=false、空 layout または NaN input は None。無限大は対応する端に丸める。inline box の装飾を text と誤認せず、atomic の前後も caret stop にする。

`caret(position) -> Option<Caret>` は UTF-8/grapheme/変換の indivisible span の内側を affinity に従って安全な境界へ丸める。範囲外の line/offset は None。bidi 境界の二つの視覚位置、soft wrap の各行側、hard break、空行、SHY、tab、hanging spaces を扱う。caret は幅0の logical rect、block 座標は line offset と参加 font extents に基づく。

ligature 内の独立 grapheme に caret stop を作る。使用可能な GDEF ligature caret をサイズ/variation に従って使い、未収録・対応しない contour-point data・不正値には cluster の grapheme 数による比例配置を使う。この fallback は font の厳密な ink 分割を主張しない。combining/ZWJ grapheme と text-transform の indivisible spans は内部 stop を作らない。final advance（spacing/justification/overlay）を使い、glyph offset を caret advance と混同しない。

`selection_rects(start, end) -> Vec<LogicalRect>` は line-index 順の論理選択を visual intervals に変換する。RTL/bidi では非連続な矩形を保持し、同じ行で実際に接する区間だけを結合する。逆順の endpoints は正規化し、範囲外は空。空選択と非描画 source/control は塗り矩形を作らない。atomic、tab、選択された visible SHY と partial ligature も対象に含む。

`move_caret(position, direction, order) -> Option<TextPosition>` は legal grapheme stops を論理または視覚順に移動し、呼び出し側が与えた Line 順で行を跨ぐ。Logical は同じ offset の bidi affinity を重複した文字移動にしない。Visual は視覚位置の違う同一 offset を保持し、同じ位置への無限往復をしない。両端を超える移動は None。挿入/削除、IME、DOM の編集ポリシーは提供しない。

index の構築は input glyph/text/fragment 数に比例する所有量と O(N log N) 以下の並べ替え。単一 hit/caret/navigation が全 paragraph の glyph 再構築を行わない。index は Line の参照を使い、font bytes/Paragraph を複製しない。

## 検証と完了監査

固定 font fixture と直接 Skrifa/harfrust の oracle を使う。strut/run metrics、mixed fallback/size-adjust/variations、全 vertical-align、tab/letter/word/autospace の line widths と glyph positions、candidate/intrinsic/plans の一致、cached/cold float と page rollback を確認する。

hit は bidi の double affinity、ligature/combining/ZWJ、expanded/collapsed mapping、first-line、owned overlay/joint SHY、justification、atomic/tab/empty line、selection の分断、logical/visual 複数行移動を数値で検証する。異常値・小予算・深い nesting・長い alternating bidi/style の進捗と操作数を検証する。

全 workspace、root no-default と明示 complex-scripts、stable/MSRV1.89、Clippy all-targets warnings deny、fmt、rustdoc、wasm、Python5/asset check を通す。one fresh whole-branch review の Important/Critical を一回の完全な RED/GREEN fix pass で直す。exact HEAD の全 CI 成功後にPRをマージし、issue close、判断記録保存、clean worktree cleanup を行う。

## 根拠

- [CSS Text 3 spacing](https://drafts.csswg.org/css-text-3/#spacing)
- [CSS Text 4 text-autospace](https://drafts.csswg.org/css-text-4/#text-autospace-property)
- [CSS Inline 3 metrics/alignment](https://drafts.csswg.org/css-inline-3/)
- [CSS 2.1 line height](https://www.w3.org/TR/CSS21/visudet.html#line-height)
- 同梱基盤設計 §3–§6/§8、S2 設計、現在の src と固定 fixture、pinned Skrifa/read-fonts の一次ソース。

2026-09-27 に Editor's Draft を参照。unstable な autospace の文字分類を現時点の規則に固定し、将来変更時は回帰期待値と根拠を同時に更新する。
