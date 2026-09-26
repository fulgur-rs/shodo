# shodo S0: 基盤設計（データモデルと公開 API の骨格）

- beads: shodo-p2m.1（epic: shodo-p2m「M1: parley からの置き換え」）
- 関連する決定: shodo-he2, shodo-5lk, shodo-5ef, shodo-d0j, shodo-iy2, shodo-1m9
- 日付: 2026-09-26
- 改訂: v1 → v2 で第 1 回レビュー（B1–B2, M1–M10, m1–m11）を反映。v2 → v3 で第 2 回レビュー（B1–B2, M1–M7, m1–m10）を反映（`[R2-*]`）。v3 → v4 で第 3 回レビュー（M1, m1–m12）を反映（`[R3-*]`）。第 4 回（最終確認）で Fable レビュアーと合意。v4 → v5 で Codex レビュアーの第 1 回（B1–B2, M3–M11, m12）を反映（`[C1-*]`）。v5 → v6 で Codex の第 2 回（B1, M1, m1–m3）を反映（`[C2-*]`）。Codex の第 3 回で合意（Minor 2 件を申し送りとして v7 に追記、`[C3-*]`）。v7 → v8 で Fable の第 5 回（M1, m1–m6）を反映（`[F5-*]`）。v8 → v9 で Codex の第 4 回（M1, m1）を反映（`[C4-*]`）。Codex の第 5 回で合意し、Minor 1 件を v10 に反映（`[C5-*]`）。v10 → v11 で Fable の第 6 回（M1, m1–m2）を反映（`[F6-*]`）。Codex の第 6 回で合意し、Minor 2 件を v12 に反映（`[C6-*]`）。
- 状態: **PASS**（2026-09-26。Codex（gpt-6-astra）が v11 に「はい」、v12 はその Minor の反映のみ。Fable が v12 に「はい」。Fable の第 7 回の Minor 3 件は本文を変えず beads の S3 / S4 に申し送り）。ユーザー承認済み。

## 0. 背景と目的

shodo は raikiri（fulgur-rs/raikiri）の parley 0.11 依存を置き換えるインライン整形コンテキスト（IFC）エンジン。CSS エンジンをファーストクラスの利用者とし、bevy などのゲームエンジン・GUI も汎用の利用者として取り込む。

raikiri main の現状（要約）: Text ノードごとに `parley::Layout` を作り、行ボックスを taffy の flex で代用。parley の外に多数の回避策（NBSP 移し替え、ZWJ 挿入、line-break:anywhere の override、text-autospace、tab-size、ch/ic 計測、生成コンテンツの paint 時再 shaping など）。毎パスで全 Layout を作り直している（page_pipeline.rs:186–190）。

合意済みの方針: shodo は IFC エンジン / 下の層（icu4x, unicode-bidi, harfrust, skrifa, fontique）は再利用 / 縦書き・ルビ前提のデータモデル、実装は M1 で parley 同等 → raikiri 差し替え / ルビは IFC 内 / ヒットテストまで、編集は含めない / 増分的な行分割器（next_line + BreakToken）/ 公開 API は f32、内部は 1/64 px 固定小数点。

非機能要件（fulgur / raikiri と共通）: 低メモリフットプリント、高速（特にコールドスタート）、セキュリティ（信頼できない HTML・Web フォント入力、fail-closed）。

後続: S1 フォント層 / S2 テキスト解析と shaping / S3 横書きの行組み / S4 raikiri への統合 / S5 日本語組版 / S6 縦書き / S7 ルビ / S8 発展機能。

## 1. crate 構成と公開 API の層

- 当面 `shodo` 1 crate、モジュール分割。`#![forbid(unsafe_code)]`。
- 依存は raikiri と同版: fontique 0.11, harfrust 0.12, skrifa 0.44, icu_segmenter / icu_properties 2.3（compiled_data）, unicode-bidi 0.3, peniko 0.6。
- features:
  - `system-fonts`（既定 on）: fontique のシステムフォント。wasm32 では off。feature が on でも実行時に走査を無効にできる（§5.2）。
  - `complex-scripts`（既定は S1/S2 で raikiri と相談して決定）: タイ語・ラオ語・クメール語・ビルマ語の辞書 / LSTM 改行データ。off の場合、これらの文字は書記素境界で改行する。compiled data は静的にリンクされるため、常駐量（特に wasm）はこの feature の有無で決まる。実行時の遅延生成は常駐量を減らさない。[R2-m8]
  - `rayon`（既定 off）: 複数段落の一括 build。
- 既定構成で wasm32 ビルドが通ることを CI で確認。

モジュール:
| モジュール | 役割 |
|---|---|
| geometry | 論理座標の型、WritingMode、PhysicalConverter、内部用 LayoutUnit（pub(crate)） |
| style | InlineStyle / ParagraphStyle / LineOptions（すべて Default） |
| font | FontCollection（2 層）、FontId、matcher、メトリクス、フォント別の shaping データキャッシュ（S1） |
| builder | ParagraphBuilder（CSS 向け）、RichText（汎用） |
| analysis / shape | 空白処理、text-transform、bidi、改行機会、itemize、shaping（S2） |
| line | Paragraph、next_line、BreakToken、LineResult、揃え、intrinsic sizes（S3） |
| output | Line、Fragment、GlyphRun などの読み出し API |
| mapping | OffsetMapping |
| hit | ヒットテスト（S3） |
| limits | Limits、LimitExceeded、Warning |

2 つの入口（同じコアを通る）:
```rust
// CSS エンジン向け
let mut b = ParagraphBuilder::new(&para_style, &limits);
b.open_inline(node, &inline_style, inline_edges);
b.push_text(TextSource::Dom { node: text_node, offset: 0 }, "本文");
b.push_atomic(img, &inline_style, atomic_edges);
b.close_inline();
let para: Paragraph = b.build(&mut cx, &fonts)?;   // Err は上限超過のみ

let mut token = para.start_token();                 // Paragraph の id を持つ [R3-m2]
loop {
    match para.next_line(&mut cx, token, &line_options, &constraint, &atomics) {
        LineResult::Line(line) => { token = line.break_token(); /* 採用 */ }
        LineResult::Done => break,
        other => { /* §3.2 の契約に従う */ }
    }
}

// 汎用（bevy など）
let para = RichText::new(&para_style)
    .push("Hello ", &style_a)
    .push("世界", &style_b)
    .build(&mut cx, &fonts)?;                        // Paragraph を保持して再利用できる [R2-m6]
let lines = para.break_all(&mut cx, &line_options, max_width, &AtomicSizes::EMPTY); // 失敗しない（§5.3）
```
- `RichText` は Limits を `Limits::default()` で使う（`with_limits` で変更可）。[R2-m6]

## 2. 入力モデル

### 2.1 スタイル（3 種類）[R2-M7]

値は原則として解決済み f32（px）。em / % の解決は呼び出し側。例外として、使用フォントに依存する値は shodo が解決する:
- `line_height: LineHeight { Normal, Px(f32), Number(f32) }` — Normal は run ごとの実使用フォントの ascent + descent + lineGap。
- `font_size_adjust: Option<FontSizeAdjust { metric, value }>`。
- ch / ic 単位は呼び出し側が S1 の API（`fonts.resolve_ch / resolve_ic`）で解決して渡す。

スタイルは「build に影響する値」と「行組みにだけ影響する値」に分ける。後者だけを変えた場合は再 build せずに済む。

**ParagraphStyle**（build 時に渡す。変わったら再 build）:
- writing_mode, direction, unicode_bidi_plaintext
- root: InlineStyle（ルートのインラインボックス、strut の元）
- first_line: Option<InlineStyle の差分>（::first-line）

**LineOptions**（next_line / intrinsic_sizes / break_all / plan_breaks に渡す。変えても再 build 不要）:
- text_align, text_align_last, text_justify（伸縮位置は build 時に全種類記録しておく）
- text_indent（length, hanging, each_line）
- hanging_punctuation
- text_wrap_style（auto / balance / pretty / stable）
- text_box_trim（予約）

**InlineStyle**（インライン要素ごと、build 時）:
- フォント: families（リスト）, size, weight, width, style, variations, features, kerning, variant_*（features に展開）, optical_sizing, synthesis（weight / style / small-caps の可否）, font_size_adjust
- lang
- line_height
- letter_spacing, word_spacing
- white_space_collapse, text_wrap_mode
- line_break, word_break, overflow_wrap, hyphens, hyphenate_character
- text_transform, tab_size
- text_autospace, text_spacing_trim
- vertical_align（baseline / sub / super / text-top / text-bottom / middle / top / bottom / length）
- direction, unicode_bidi（normal / embed / isolate / bidi-override / isolate-override / plaintext）
- text_orientation（S6）, text_combine_upright（S6、予約）
- text_emphasis（style, position）— 行ボックスの高さに寄与する
- text_box_edge（予約）
- box_decoration_break（slice / clone）

描画のみの項目（color, text-decoration の種類・色, text-shadow, 下線位置の指定）は受け取らない。呼び出し側が NodeId から引く。

ビルダーはスタイルを内部表に登録（同値は 1 つに共有）し、StyleIndex（u32）で参照。スタイル数は Limits で上限。

### 2.2 ビルダー

```rust
open_inline(node, &InlineStyle, InlineEdges)     // 論理 4 辺の margin / border / padding（下記）
close_inline()
push_text(TextSource, &str)                      // Dom { node, offset } | Generated { node }
push_atomic(node, &InlineStyle, InlineEdges)     // margin を含む
push_out_of_flow(node, OutOfFlowKind)            // Float | Absolute
push_block_in_inline(node)                       // <span>a<div/>b</span> の div
push_forced_break(node)                          // <br>
with_offset_mapping(bool)                        // 既定 true。false なら OffsetMapping を作らない（PDF 出力など）[R2-m1]
// S7 予約: open_ruby / ruby_base / ruby_text / close_ruby
```
- NodeId は不透明な u64。RichText では省略可。
- InlineEdges は論理 4 辺（inline 始端・終端、block 始端・終端）の margin / border / padding を持つ。inline 側は行の中で幅を取り、block 側は行ボックスの高さには寄与しない（CSS 2.1 §10.8.1）が、出力の InlineBox の border box には含める。[C1-M7]
- InlineEdges の inline 側は box-decoration-break: slice（既定）では最初 / 最後の fragment にのみ適用、clone では各 fragment に適用。
- 入れ子の深さ、テキスト長、item 数、スタイル数は **push のたびに増分で** Limits と照合する。超過した時点で builder はエラー状態になり、それ以降の push は何も確保せずに無視され、build が `Err(LimitExceeded)` を返す（push の戻り値を毎回検査しなくても、上限を超えた確保は起きない）。[C1-B2]

### 2.3 item 列（内部）

- テキストを 1 本に連結し、build 時に空白処理（ノードをまたぐ）と text-transform を適用した処理後テキストを作る。
- item 種類: Text, OpenInline, CloseInline, Atomic, OutOfFlow, BlockInInline, Control（強制改行, タブ）, BidiControl, 予約: RubyOpen / RubyBase / RubyText / RubyClose。
- bidi 解決は処理後テキスト上で行う。Atomic / OutOfFlow は U+FFFC、BidiControl は対応する制御文字（LRI / RLI / FSI / PDI / LRE / RLE / PDF / LRO / RLO）として同じ列に置く。
- BlockInInline と強制改行は bidi 段落の境界。BlockInInline の前で開いている embed / isolate は閉じ、後で開き直す（Blink の InlineItemsBuilder の Enter/ExitBlock 相当）。[R2-M5]
- 各 item は処理後テキスト範囲・StyleIndex・NodeId を持つ。

### 2.4 OffsetMapping

- ランレングスの `MappingUnit { kind: Identity | Collapsed | Expanded, node, dom: Range<u32>, text: Range<u32> }` の配列。
  - Identity: 1:1（長さ同じ）
  - Collapsed: N:0（空白の畳み込みで消えた文字）
  - Expanded: 長さが変わる区間（text-transform の ß → SS、full-width、生成された制御文字・境界文字）。区間の内部には対応を持たず、分割不能な単位。粒度は「長さが変わる 1 文字ごと」とし、長さが変わらない変換（ASCII の uppercase）は Identity。[R2-m1]
- `dom_to_text(node, offset) -> Option<(u32, Affinity)>`: Collapsed 区間の内部を指す場合は区間末尾の text 位置、Expanded 区間の内部は区間先頭に丸める。[R2-m1]
- `text_to_dom(offset, Affinity) -> Option<TextOrigin>`。`TextOrigin::Dom { node, offset } | Generated { node }`。[R3-m10]
- 単位は呼び出し側文字列の UTF-8 バイト位置（UTF-16 変換は呼び出し側）。
- TextSource::Generated の文字は dom 範囲を持たない（text_to_dom は TextOrigin::Generated を返す）。
- with_offset_mapping(false) の場合は作らず、該当 API は None を返す。

## 3. 行組み API

### 3.1 Paragraph

- build 後は不変。中身は `Arc<ParagraphData>` で、Clone は安価。
- Send + Sync。
- `para.id()`: Paragraph ごとに一意な識別子（BreakToken が持つ）。[R2-M6]
- `para.start_token()`: この Paragraph の先頭を指す BreakToken。[R3-m2]
- フォント層の保持 [R3-M1]: build 時に、使用したフォント層（共有層と、あれば文書層）の `Arc` ハンドルを ParagraphData に保持する。next_line は `&fonts` を受け取らず、このハンドルから shaper データ（ShaperData）と matcher を引いて、行端の再 shaping、ハイフン glyph の合成（フォールバックを含む）、text-spacing-trim の行頭処理を行う。Line も Arc<ParagraphData> 経由で層を保持するので、`line.font_data(FontId)` / `run.font_data()` で peniko::FontData を得られる（bevy が fonts を持ち回る必要がない）。
- `para.font_generations()`: build 時の (共有層世代, 文書層世代)。現世代と異なれば再 build が必要（Web フォントの遅延到着）。[R2-M2]
- 再利用の契約: ParagraphStyle・InlineStyle・テキスト・フォント世代が変わらない限り、Paragraph を保持したまま、LineOptions・幅・制約・AtomicSizes だけを変えて何度でも行を組める。

### 3.2 next_line と LineResult

```rust
fn next_line(&self, cx: &mut LayoutContext, token: BreakToken, options: &LineOptions,
             constraint: &LineConstraint, atomics: &AtomicSizes) -> LineResult;

#[non_exhaustive]
enum LineResult {
    Line(Line),
    Done,
    BlockSizeExceeded { needed_block_size: f32 },
    FloatEncountered {
        node: NodeId,
        line_start: BreakToken,      // この行の先頭。再開はここから（float の直前ではない）
        inline_position: f32,        // float の直前までに、利用可能領域の中で消費した幅（inline_start_offset を含まない）[C1-M11]
        float_cursor: FloatCursor,   // この float まで処理済みを示す Copy の値
    },
    BlockInInline { node: NodeId, token_after: BreakToken },
    InvalidToken,                    // 別の Paragraph の token、または範囲外 [R2-M6]
}

struct LineConstraint<'a> {
    available_inline_size: f32,
    inline_start_offset: f32,                    // float による始端側の欠け
    block_offset: f32,                           // IFC 内での行の block 位置（Line に引き継ぐ）
    max_block_size: Option<f32>,                 // 分割コンテナの残り高さ、float の脇の高さ
    floats_placed_through: Option<FloatCursor>,  // これ以前の float は再報告しない [R2-B1]
    break_plan: Option<&'a BreakPlan>,           // text-wrap: balance / pretty の分割計画
}
```

**BreakToken**: Copy の不透明な値。`para.start_token()` で得る（固定の START 定数は持たない）。中身は（Paragraph の id、item 番号、テキスト内オフセット、状態フラグ: 段落先頭行か、強制改行直後か、first-line 適用中か）。中身は非公開で、ルビ等のために後から拡張できる。bidi 段落番号はテキストオフセットから復元するので持たない。

**共通の契約**:
- next_line は副作用なし。同じ token を別の constraint でやり直せる。
- 進行保証（例外なし）: `Line` を返すとき、その break_token は入力 token より必ず進む（1 行に最低 1 つの item か分割不能単位を消費する）。BlockInInline 直前の空の行も、OpenInline などの item を少なくとも 1 つ消費する場合にだけ返す（後述）。[R3-m1]
- 入口での正規化: LineConstraint・AtomicSizes・BreakPlan の非有限値・負値は next_line の入口で正規化し（非有限 → 0、負の幅 → 0）、warning に記録する（raikiri で非有限値により parley の break_all_lines が hang した前例への対策）。[R2-M6]

**float の契約** [R2-B1]（CSS 2.1 §9.5.1 と Blink の LineBreaker::HandleFloat と等価な結果を得る）:
1. next_line は行を組む途中で、floats_placed_through より後の最初の float item に到達したら FloatEncountered を返す。行頭の float は inline_position = 0 で返す。ただし、float は改行機会を作らないので、float item を含む分割できない区間（例: 単語の途中の float）を先読みし、その区間が現在の制約で次の行へ送られる場合は、その float を報告しない（次の行で報告される）。報告してから後で改行位置が float より前に戻ると、「報告 → 保留 → 同じ displaced_floats」を繰り返しうるため。先読みは §6.1 の改行機会と累積位置で行い、next_line の純粋性（同じ入力に同じ結果）を保つ。[C6-m1]
2. 呼び出し側（BFC）は:
   - `available_inline_size − inline_position ≥ float の margin box の inline size` かつ保留中の float が無ければ、float を現在の行の block 位置に配置し、除外領域を反映した inline_start_offset / available_inline_size を作る。
   - 収まらない、または保留中の float がある場合は、float を「この行の後に置く保留 float」として記録する（以降の同じ行の float も保留、CSS 2.1 §9.5.1 の順序規則）。
   - どちらの場合も floats_placed_through = float_cursor にして、line_start から next_line を呼び直す。
3. 呼び直しの回数は有界: 1 行の中で、各 float は「報告されて配置または保留になる」「取り消されて未報告に戻る」「同じ行で再び報告されて保留になる」をそれぞれ高々 1 回しか経ない（手順 7）ので、1 行あたりの呼び出しは「行内の float の数 × 3 + 1」以下。[C3-m2][F5-M1][C4-M1][F6-M1]
4. 行が確定したら、保留 float を行の下に配置し、次の行の constraint に反映する。
5. floats_placed_through は**段落内で単調非減少**とする。唯一の例外は手順 7 の取り消しで、配置（または保留）を先に戻してから cursor を取り消した float の直前まで戻すので、二重配置は起きない。呼び出し側が保持し、ページ分割で token を保存するときは cursor も一緒に保存する（Copy の小さな値）。cursor 以前の float item は、どの行で消費されても幅 0 の処理済み item として扱い、再報告しない（行ごとにリセットすると、float の前で改行が起きた場合に同じ float が次の行で再報告され、二重に配置される）。[F5-M1][F6-M1]
6. inline_position は、利用可能領域（inline_start_offset より後、available_inline_size の範囲）の中で消費した幅と定義する。text-indent は含み、inline_start_offset（float による欠け）は含まない。したがって手順 2 の判定 `available_inline_size − inline_position ≥ float の margin box` は欠けを二重に引かない。行頭の float では 0（text-indent があればその分）。[C1-M11]
7. 組み直しと float の取り消し（CSS 2.1 §9.5.1 規則 6）: 意味論は「line_start から組み直す」であり、キャッシュは結果を変えない最適化である。組み直しの結果、配置した float より前で改行が起きる場合がある（tab のように幅が行内位置に依存するもの。tab stop は float による欠けではなくブロックコンテナの content edge を基準にする、CSS Text 3 §4.1.2）。このとき float を元の行の上端に残すと、float より前の内容を含む行ボックスより上に float が来て規則 6 に違反する。そこで:
   - next_line が返す Line は `line.displaced_floats()` を持つ。これは、floats_placed_through 以前に報告された float（配置したものと保留にしたものの両方）のうち、item がこの行の改行位置より後ろ（次の行以降）に来たものの一覧である（line_start と改行位置と cursor から shodo が求める）。[F6-m1]
   - 呼び出し側は、この一覧にある float を**後から報告されたものから 1 件ずつ**取り消す。取り消しとは、配置していれば配置を戻し（除外領域を戻す）、保留にしていれば保留の一覧から外し、floats_placed_through をその float の直前まで戻して**未報告に戻す**ことである（「この行の後に置く保留」にはしない。そうすると float より前の内容が次の行に収まらない場合に、float がその内容を含む行より上に来て規則 6 を再び破る）。取り消した float は「この行で取り消した float」として記録する。1 件取り消すごとに、除外を戻した constraint で line_start から next_line を呼び直し、新しい displaced_floats() で次に取り消すべきものを判定する。一括で取り消すと、この行に残せる float まで次の行へ送り、CSS 2.1 §9.5.1 規則 8（可能な限り高く置く）に反するため。[C5-m1][F6-M1]
   - 呼び直しで、この行で取り消した float が再び FloatEncountered として報告された場合（除外を戻したことで item が再びこの行に載った場合）は、残り幅にかかわらず保留にする（この行には二度と配置しない）。これにより tab 幅の振動は起きず、手順 3 の上限で停止する。displaced_floats() が空の Line だけを採用する。「この行で取り消した float」の記録は行の採用時に空にする。[F6-M1]
   - 再報告されなかった float（item が最終的な改行位置より後ろにあるもの）は、その item が載る行で改めて報告され、そこで手順 2 の判定を受ける。これにより規則 6・規則 8 を満たす。Blink の LineBreaker::RewindFloats も、配置を戻した float を次の行で改めて処理する。[F6-M1]
   - 段落全体の計算量: displaced が行をまたいで繰り返される場合（tab を含む行でのみ起きる）、float の報告と取り消しは段落全体で O(行数 × float 数) になりうる。[F6-M1]
   計算量: 実装は LayoutContext に「組みかけの行」を直近の 1 件だけ（(Paragraph id, line_start, float_cursor の直前) をキーに）保持し、位置に依存しない部分は組み直さずに位置だけずらして再開する。tab を含む部分はキャッシュを使わず組み直す。1 行の処理量は、tab を含まない行で O(行の長さ + 行内の float 数)、tab を含む行で最悪 O(行の長さ × 行内の float 数)。キャッシュは Paragraph の Arc を保持するので、LayoutContext::shrink_to で解放できる。[C1-M5][C2-m2][F5-M1][F5-m4][C4-M1]
8. 呼び出し側の仮配置と取り消し: next_line は副作用を持たないが、呼び出し側（BFC）は FloatEncountered に応じて float を配置する。取り消しが必要になるのは、手順 7 の displaced_floats、BlockSizeExceeded によるページ送り、widows / orphans の先読みの破棄である。保存・復元の単位は `(token, floats_placed_through, BFC の状態, 保留 float の一覧, この行で取り消した float の記録)` とし、cursor の単調性（手順 5）は「採用する 1 つの行組みの進行」の中に限る。先読みを破棄するときは cursor も含めて全体を復元し、同じ Paragraph を新たに最初から組む場合は cursor を初期化する（配置だけ戻して cursor を進めたままにすると、再試行で float が報告されず配置が欠落する）。taffy 0.14 の BlockContext は checkpoint / rollback を公開していないので、S4 で taffy の拡張か、行単位の仮配置コンテキストを設計することを必須の申し送りとする。[C1-M5][F5-M1][C4-m1][F6-m2]

**BlockSizeExceeded の契約** [R2-B1]:
- constraint.max_block_size を超える行を組むことになる場合に返す。呼び出し側は、幅・位置を変えて同じ token で再試行するか、ここで分割（ページ送り）する。
- 分割コンテナの先頭の行（または float の脇の最後の候補位置）で超過した場合、呼び出し側は max_block_size = None で呼び直して行を受け入れる。これにより、ページより高い行が 1 つあっても無限ループしない。

**BlockInInline の契約** [R2-M5]:
- ブロックの直前で行を切り、break_reason = BlockInInline の Line を返す。次の呼び出しが BlockInInline { node, token_after } を返す。呼び出し側はブロックを組み、token_after から再開する。
- ブロック直前の内容が空（縁の無いインライン要素の開始だけなど）の場合、Line::is_empty() が true の Line を返す。この Line は少なくとも 1 つの item を消費し、`block_size() == 0` を shodo が保証する。呼び出し側は widows / orphans の行数に数えない（CSS 2.1 §9.4.2）。token がすでに BlockInInline の item を指している場合は、空の Line を返さず直接 BlockInInline を返す。[R3-m1]

**AtomicSizes**: NodeId → { inline_size, block_size, baseline: Option<f32>, margins }。パーセント幅の atomic は next_line の呼び出しごとに再供給してよい。
- baseline は、その atomic の**親インラインボックスの支配的な baseline の種類**での位置（margin box の block 始端から）とする。要求される種類は atomic ごとに `para.required_baseline(node) -> BaselineKind` で取得できる（build 時に確定する。例: vertical-rl / mixed の段落でも、text-orientation: sideways の span の中の atomic は alphabetic）。呼び出し側はその種類の baseline を渡す。[C1-M8][C2-M1]
- None の場合は、同じ種類について合成する: alphabetic なら margin box の block 終端（line-under 側）、central なら margin box の中央（種類の選択は CSS Writing Modes 4 §4.2–4.4、合成の規則は CSS Inline 3 に従う。S6 で出典を確認する）。[C1-M8][C2-M1][F5-m5]
- AtomicSizes は世代番号（内容を変更するたびに増える）を持つ。BreakPlan の一致判定に使う。[C1-M9]コールバックにしない理由は借用（raikiri では taffy の compute_child_layout が &mut Document を要求）。

揃え（text-align, text-align-last, justify）、ぶら下げ、text-indent は next_line 内で LineOptions に従って確定する。

### 3.3 補助 API

- `para.intrinsic_sizes(&mut cx, &options, &atomic_intrinsics) -> IntrinsicSizes { min_content, max_content }`。atomic_intrinsics には atomic の min / max（margin 込み）と、float の `FloatIntrinsic { min_content, max_content, side: Left | Right（論理 start / end で指定可）, clear: None | Left | Right | Both }` を渡す。float 群の max-content 寄与は clear を考慮して合成する（taffy の FloatIntrinsicWidthCalculator::add_float と同じ規則）。[R2-m3][C1-M6] ぶら下げの扱いは S5 で CSS Text 4 に従って決める。first-line がある場合、max-content は first-line のデータ集合で測る。[R2-M4]
- `para.lines(&mut cx, token, &options, constraint_fn, &atomics) -> impl Iterator<Item = LineResult>`: 先読み用（widows / orphans）。constraint_fn は `FnMut(Option<&LineResult>, block_offset: f32) -> LineConstraint`。Item として返すのは Line / BlockInInline / Done（と InvalidToken）だけ。FloatEncountered と BlockSizeExceeded は constraint_fn に渡し、返された constraint で内部で呼び直す。[R2-m2][R3-m6]
- `para.break_all(&mut cx, &options, width, &atomics) -> Vec<Line>`: 単純な利用者向け。float は OutOfFlowAnchor として幅 0 で置き（parley の OutOfFlow 相当）、BlockInInline はその位置で行を切るだけにする。入力は next_line と同じく正規化して warning に記録する。資源エラーは返さない（§5.3、入力量の上限は build で照合済み）。[R2-m2][R3-m3][C1-B1]
- `para.plan_breaks(&mut cx, &options, width, &atomics) -> BreakPlan`: balance / pretty の分割計画。atomic の寸法を使って計画する。窓と反復回数は Limits で上限。BreakPlan は作成時の Paragraph id・幅・options・AtomicSizes の世代を記録し、next_line に渡されたものと一致しない場合は計画を無視して auto で組む。[R3-m9][C1-M9]

### 3.4 ::first-line [R2-M4]

- ParagraphStyle.first_line が Some の場合、build 時に 1 行目用のデータ集合（処理後テキスト、item 列、改行機会、shaping 結果、OffsetMapping）を追加で作る。::first-line には text-transform・letter-spacing・word-spacing が適用できるため、処理後テキストから別に持つ必要がある。
- Limits のテキスト長・item 数は 2 集合分を合算して照合する。
- next_line は token の「first-line 適用中」フラグでデータ集合を選ぶ。Line の fragment は「どちらの集合を指すか」のビットを持つ。
- BlockInInline より後の継続には first-line を適用しない（S3 の詳細）。

## 4. 出力モデル

### 4.1 データの持ち方 [R2-B2]

- shaping 結果は ParagraphData が平坦な SoA 配列で持つ: glyph id、advance（LayoutUnit）、pen の累積位置（LayoutUnit、prefix sum。幅の計算と行内位置の基準）、glyph ごとの offset（inline 方向と block 方向の 2 つ。GPOS の mark 配置などで advance の累積とは独立に決まる。harfrust の GlyphPosition の x_offset / y_offset に対応し、縦書きでは y_offset が inline 方向になる）、所属 cluster、フラグ（UNSAFE_TO_BREAK / UNSAFE_TO_CONCAT など）。[C1-M3]
- glyph の描画位置 = fragment の原点 + pen の累積位置（fragment 先頭からの差）+ glyph の offset。
- pen の累積位置は run 内の局所座標とし、飽和させない: 累積が LayoutUnit の範囲の半分（2^30 単位 ≈ 1.68e7 px）を超える前に run を分割し、新しい run で累積を 0 から始める。これにより、個々の値が表現可能である限り、累積位置の差は常に正確になる（飽和で位置の差が失われることはない）。[C1-M4]
- Line が持つもの:
  - `Arc<ParagraphData>`
  - 行内の fragment 表（小さな配列。各 fragment は ParagraphData または overlay への範囲、原点、使うデータ集合のビット、使う位置配列のビット）
  - overlay: 行端で shaping し直した部分（ハイフンの合成 glyph を含む）。ParagraphData と同じ SoA 形式で、アクセサは同じ経路で読める。再 shaping の範囲が item 境界をまたぐ場合、fragment は overlay の境界でも分割する。[R2-m9]
  - 行専用の位置配列（任意）: glyph の行内位置が共有データと異なる行だけが持つ（4 バイト / glyph、glyph 本体はコピーしない）。実質的に必要なのは justify の行だけ。tab は Control item で fragment が分かれ、行末の letter-spacing 除外・ぶら下げ・text-indent は fragment の原点と inline_size だけで表せ、text-spacing-trim の行頭は先頭の cluster を overlay に出せば済む。[R3-m8]
- 調整の無い行は、共有の累積位置と fragment の原点だけで glyph の位置が O(1) で決まる。
- Line は Send + Sync、'static（借用なし）。raikiri は Paragraph と Line をノードに保存できる。bevy は Paragraph を捨てても Line だけで描画できる。
- 読み出し: `run.glyphs()` は `ExactSizeIterator` かつ `get(i)` による O(1) のランダムアクセスを提供するビュー（スライスではない）。内部で共有 / 行専用の位置配列を切り替える。
- cluster ビューの advance は、行の伸縮（justify など）を反映した値を返す（ヒットテストとキャレット位置に必要）。shaping 時の値は別のアクセサで得られる。

### 4.2 Line

```rust
line.break_token(), line.break_reason()     // Regular / Forced / Emergency / BlockInInline / End
line.is_last()                               // text-align-last の判定（強制改行直前も「最終行」）
line.is_empty()                              // strut のみで内容なし（BlockInInline 直前など）
line.inline_size(), line.block_size()        // 論理座標
line.block_offset()                          // constraint から引き継いだ値
line.overflow_rect()                         // 描画範囲（ルビ注釈や emphasis の張り出しを含む）。block_size（行送り量）とは別 [C1-m12]
line.baseline(BaselineKind)                  // alphabetic / central / ideographic ...（S6 で拡充）
line.metrics()                               // 行ボックス上下と、ルート inline の text-over / text-under の両方
line.hang_start(), line.hang_end()           // ぶら下げ量（glyph の行内位置は負になりうる）
line.fragments()                             // 視覚順
line.displaced_floats()                      // 報告済み（配置・保留の両方）で item が改行位置より後ろに来た float（§3.2 手順 7）[C4-M1][F6-m1]
```

### 4.3 Fragment

座標の原点: fragment の inline 位置は **IFC のコンテナの content box の inline 始端**を原点とする（inline_start_offset、text-indent、揃えのずれを含んだ値）。block 位置は行ボックスの上端を原点とする。呼び出し側は行ごとの補正を引き回す必要がない。[R2-m7]

| 種類 | 中身 |
|---|---|
| GlyphRun | FontId, font size, normalized coords, synthesis（embolden, skew）, 向き（S6）, bidi レベル, NodeId, StyleIndex, 処理後テキスト範囲, 行上端からの baseline 位置, glyph ビュー（id, inline 位置, block 方向のずれ, advance, 所属 cluster）, cluster ビュー（テキスト範囲, 伸縮後の advance, フラグ: 空白 / 約物 / 合成ハイフン / emphasis 対象外）, run メトリクス（ascent, descent, 下線・取り消し線の位置と太さ） |
| Atomic | NodeId, 矩形（margin box と border box）, baseline |
| InlineBox | NodeId, 矩形（border box。block 方向は content area に InlineEdges の block 側の padding / border を加えたもの）, content area の矩形, 始端・終端の縁を含むか, 親 InlineBox の番号, 主フォント（FontId + size、装飾線の位置計算用） |
| OutOfFlowAnchor | NodeId, 静的位置 |
| 予約（S7） | ルビ注釈の入れ子の行組み結果 |

契約:
- GlyphRun は item 境界（= ノード境界）と overlay の境界で必ず分割する。shaping 単位はノードをまたいでよい。
- InlineBox は「行 × NodeId ごとに 0..N 個、視覚順」。bidi で割れた各断片が縁フラグを持つ。
- text-decoration の線の位置と太さは、装飾を指定した要素の InlineBox の主フォントから `fonts.metrics(FontId, size)` で得る。

### 4.4 フォント識別と物理座標

- FontId: `(layer_id, index)`。層の中で安定。層の破棄後も layer_id は再利用しない。`fonts.font_data(id) -> peniko::FontData`。
- bevy のアトラスは (FontId, glyph id, size, coords) をキーにできる。文書層が破棄されたとき（層の最後の参照が落ちた Drop の時点。文書の破棄より遅れうる）は、その layer_id のエントリをアトラスから削除する契約とする（層の Drop で発火する通知の手段を S1 で用意）。[R2-m5]
- PhysicalConverter::new(writing_mode, direction, container_size) で論理 → 物理変換。縦書きでは run の向きも反映。Line の外で変換（vertical-rl はコンテナ幅が必要）。

## 5. コンテキスト・フォント・並行性・堅牢性

### 5.1 状態の置き場所 [R2-M2][R2-M3]

| 型 | 共有 | 中身 |
|---|---|---|
| FontCollection（共有層） | Send + Sync, 安価な Clone, プロセスに 1 つ | fontique の Collection と SourceCache（`Mutex` 内）、FontId 表、世代番号、フォールバック結果のキャッシュ、フォント別の shaping データ（harfrust の ShaperData / ShaperInstance、Arc、**件数**上限つき LRU） |
| FontCollection（文書層） | Send + Sync, 文書ごと | 独自の unshared な fontique Collection（system_fonts: false）と SourceCache（`Mutex` 内）、`@font-face` の登録、世代番号、同様のキャッシュ。共有層への参照 |
| LayoutContext | Send, !Sync, スレッドごとに 1 つ | harfrust の ShapePlan キャッシュ（script / lang / direction / features 依存の小さなもの）、作業バッファ、complex-scripts 用分節器。fontique の Collection の clone は置かない。フォント層には Paragraph が保持する Arc 経由で到達する [R3-M1] |
| Paragraph / Line | Send + Sync（不変、Arc） | 解析・shaping 結果、行の結果 |

- ShaperData（GSUB / GPOS の解析結果）はフォントごとに 1 回だけ作り、スレッド間で共有する。harfrust は ShaperData の使用 heap 量を公開せず、内部の LookupCache は登録後も遅延で確保するため、キャッシュの上限はバイト数ではなく件数で持つ。1 件あたりの量は、重複参照を展開した subtable の数に比例する。これは blob サイズでは抑えられない（重複参照で増幅できる）ため、§5.2 のフォント受け入れ時の構造検査で上限を課し、合計を「件数 × subtable 数の上限に比例する量」で有界にする。[C1-B2][C2-B1]CJK フォントで解析がスレッド数だけ重複するのを避ける。harfrust の ShaperData が Sync でない場合は層の中で Mutex に包む（S1 で確認）。
- 作業バッファは巨大な段落の後も容量が残るので、`LayoutContext::shrink_to(bytes)` と、Limits による自動縮小を用意する（S2）。[R3-m11]
- フォールバック結果のキャッシュ（shodo 側）を fontique 問い合わせの前段に置く。キャッシュが効けば `Mutex` はほぼ取らない。並列 build で競合が観測された場合に、層ごとのスレッドローカル clone を検討する。
- ICU の通常の分節器（書記素、UAX#14）は compiled data から const で作れるので生成コストは問題にならない。

### 5.2 フォント層 [R2-M2]

- 2 層: 共有層（システム / バンドル、プロセス全体で 1 つ）と文書層（文書ごと、`FontCollection::for_document(&shared)` で作成）。文書層は、それを使う Paragraph / Line がすべて破棄され、参照が無くなった時点で解放される（raikiri では文書と一緒に破棄される）。[R3-M1]
- 文書層は、共有層とは別の unshared な fontique Collection を持ち、`@font-face` はそこにだけ登録する（fontique の shared コレクションの clone に登録すると全 clone に見えるため、共有層には登録しない）。文書間でフォントは見えない。
- matcher は「文書層 → 共有層」の順に問い合わせる。generic family とフォールバック表は共有層のものを使う。
- 世代番号は層ごと。Paragraph は (共有層世代, 文書層世代) を保存する。
- システムフォントの走査は初回の問い合わせまで遅延し（fontique を system_fonts: false で作り、必要時に load_system_fonts）、実行時の設定で無効にもできる（バンドルフォントだけで完結させる用途）。[R2-m5]
- SourceCache の prune の方針（保持数など）は FontCollection の設定として持つ。[R2-m5]
- unicode-range は S1 の matcher が実装する（fontique は持たない）。
- WOFF / WOFF2 の展開は shodo の外。shodo は sfnt / TTC の Blob だけを受ける。
- 信頼できないフォントは mmap せず Blob（ヒープ）で登録する。
- フォント受け入れ時の構造検査 [C2-B1]: harfrust は lookup index ごとに LookupInfo を、その中の subtable 参照ごとに SubtableInfo をキャッシュし、同じテーブルへの重複参照も個別に展開する（read-fonts はオフセット配列の重複参照を拒否しない）。このため、小さな GSUB / GPOS から巨大な内部キャッシュを作れる（例: 8192 個の lookup が同じ Lookup を参照し、その 8192 個の subtable が同じ SingleSubst を参照すると、約 48 KiB から約 6700 万個の SubtableInfo）。これを防ぐため、フォントを登録する前（harfrust に渡す前）に、GSUB と GPOS の LookupList を走査して、**重複参照を重複として数えた** lookup 数と subtable 数の合計（Extension の解決後）を数え、Limits（max_layout_lookups、max_layout_subtables）を超えるフォントは登録せず `Err(LimitExceeded)` にする。検査自体も、読み取る件数が上限に達した時点で打ち切るので、検査の計算量も有界である。これにより、ShaperData 1 件あたりの内部キャッシュ量は「subtable 数の上限 × 1 要素あたりの量」で有界になり、件数上限の LRU と合わせて合計が有界になる。具体的な閾値と、他に展開が起きる構造の検査は S1 で確定する。対象には少なくとも ClassDef、Coverage、GDEF の mark glyph set（harfrust が各 Coverage から digest を生成する）、AAT の morx / kern / kerx のキャッシュを含め、既存の上限から量を導くか、追加の登録時制限を設ける。検査は解析に成功した subtable だけでなく、宣言された件数（sub_table_count など。harfrust はこれに基づいて先に reserve する）も数える。[C3-m1] 検査は FontCollection の登録経路（両層）に掛かる。fontique が自分で列挙するシステムフォントはこの経路を通らない（信頼するフォントとして扱う）。harfrust の no_std 構成（wasm など）では LookupCache が全 lookup を即時に展開するので、コールドスタート時に検査上限分の確保が一度に起きることを、上限値の決定時に考慮する。[F5-m2]
- 層ごとの資源予算 [C1-B2]: 各層は「登録 face 数」と「保持している blob の合計バイト数」の上限を持ち、登録の前に照合する（超過したら登録せず `Err(LimitExceeded)`、確保は発生しない）。fontique はメモリから登録した blob を SourceCache の外で保持するので prune では解放されない。そのため blob は層の予算で数え、層の破棄でのみ解放される。信頼できない `@font-face` は文書層にだけ登録し、共有層への登録はアプリケーション（信頼できるバンドルフォント）に限る。bevy のような長寿命のコレクションでも、同じ予算を共有層に設定できる。
- フォールバックの単位は書記素クラスタ。UTS#51 の絵文字既定表示と lang による漢字字形の選択は S1 の契約。
- フォールバック結果は (script, lang, 文字, クエリ) でキャッシュする（必須）。

### 5.3 堅牢性と Limits

- 失敗の種類を分ける:
  - 内容の欠落（フォント・グリフがない、未知の lang）: フォールバックか .notdef で組み、warning に記録。失敗しない。
  - 資源の過剰（上限超過）: 入力の量に由来する上限（テキスト長、item 数、スタイル数、入れ子の深さ、shaping 結果の glyph 数、フォントの登録数と blob 合計、フォントの lookup / subtable 数）は、確保の前に照合し、builder の build とフォント登録が `Err(LimitExceeded { kind, limit, actual })` を返す。黙って切り詰めない（fail-closed）。[C1-B2]
  - 計算の劣化（shaping や行組みの計算量を抑えるための打ち切り）: 資源を消費し続けることはなく、結果の精度だけが落ちるもの。内容の欠落と同じ区分で扱い、失敗せずに warning に記録する。[C1-B1]
    - shaping の出力量: harfrust の MultipleSubst は 1 glyph から最大 65,535 glyph を出力でき、入力長の上限だけでは出力量を抑えられない（1 回の shaping の実効上限は harfrust の buffer の上限 `max(入力 glyph 数 × 256, 65536)`。max_shaping_run_bytes の既定値はこの式から見積もる）。[F5-m1]そこで build は、shaping 結果の glyph 数の累積（first-line 集合を含む）を Limits（max_shaped_glyphs）と、Paragraph に追加で確保する前に照合し、超過したら `Err(LimitExceeded)` を返す（入力量の上限と同じ fail-closed の経路）。行端の overlay にも出力 glyph 数の予算を設け、超過した場合は予算内に収まる元の shaping 結果を使う（劣化、warning）。[C2-m1]
    - 分割できない単位: 1 つの書記素クラスタ（基底文字と大量の結合文字など）が max_shaping_run_bytes を超える場合は、コードポイント境界で強制的に分割して shaping する（その部分の字形が不正確になりうる。warning）。文書全体を拒否はしない。[C2-m3]
    - overlay の差し替えは完全な cluster 単位で行う。予算内に閉じられない場合は、その cluster 全体について元の shaping 結果を保持する（元の glyph と新しい glyph の重複や、文字の欠落を起こさない）。[C2-m3]
    - harfrust は GSUB / GPOS の再帰と処理回数に内部の上限を持ち、上限に達すると shaping を打ち切って結果を返す（失敗状態は公開されない）。これは計算量を有界にする仕組みであり、資源の過剰ではない。shodo は 1 回の shaping 呼び出しに渡す入力長を Limits（max_shaping_run_bytes）で抑え、長い run は安全な位置で分割して shaping するので、1 回あたりの計算量は「入力長の上限 × harfrust の内部上限」で有界になる。harfrust が失敗状態を公開した場合に warning を記録できるよう、上流への改善提案を S2 の申し送りとする（実装開始の前提にはしない）。
    - 行端の再 shaping の窓が Limits を超える場合は、窓の範囲だけを再 shaping し、残りは元の shaping 結果を使う（行端の字形がわずかに不正確になりうる）。warning を記録する。
  - このため next_line / lines / break_all / intrinsic_sizes / plan_breaks は資源エラーを返さない。計算量は構成上で有界であり（下記）、入力量の上限は build の時点ですでに照合済みである。[C1-B1]
- 入口での正規化: build 時は InlineStyle 等の非有限値・範囲外（font-size ≤ 1e6 px、spacing ±1e7 px など）、next_line 時は LineConstraint 等（§3.2）。丸めた内容は warning に記録。
- warning: `Paragraph::warnings()`（build 時）と `LayoutContext::take_warnings()`（next_line 時）。件数は Limits.max_warnings で上限を持ち、超過したら「以降を抑制した」ことを示す合成の 1 件を追加して以降は記録しない（raikiri の max_parse_warnings と同じ扱い）。[R2-M6]
- Limits は raikiri の RenderLimits と同じく `Option<T>`（None で無制限）で持ち、既定は `Some(既定値)`。既定値は raikiri と相談して決める:
  - 段落のテキスト長（バイト、first-line 集合を含めて合算）、item 数、スタイル数、open_inline の入れ子の深さ、shaping 結果の glyph 数の累積（max_shaped_glyphs）
  - 行端の再 shaping の窓（最大バイト数）、1 回の shaping 呼び出しに渡す run の最大バイト数（max_shaping_run_bytes）
  - balance の反復回数、pretty の窓（行数）
  - max_warnings
  - （float による呼び直しは §3.2 で有界なので上限を設けない。BlockSizeExceeded の再試行は呼び出し側の候補位置の数で有界）[R3-m12]
  - フォント blob サイズ、TTC の face 数、axis 数、GSUB / GPOS の lookup 数と subtable 数（重複参照を含めて数える）、層ごとの登録 face 数と blob 合計バイト数、ShaperData キャッシュの件数（S1）
  - bidi の埋め込み深さは unicode-bidi が UAX#9 の 125 に固定しているので不要
- 計算量の保証（アルゴリズム的 DoS の排除）:
  - next_line は行の長さ＋再 shaping 窓に比例。float による呼び直しは 1 行あたり「行内の float 数 × 3 + 1」回以下で有界（§3.2 手順 3・7）。1 行の合計処理量は、tab を含まない行で O(行の長さ + 行内の float 数)、tab を含む行で最悪 O(行の長さ × 行内の float 数)。tab を含む行では、float の取り消しが行をまたいで繰り返されると段落全体で O(行数 × float 数) の報告・取り消しが起きうる（§3.2 の手順 7）。[C3-m2][F5-M1][F6-M1]
  - intrinsic_sizes は段落長に線形。
  - フォールバック探索はキャッシュにより文字種ごとに 1 回。
  - pretty / balance は Limits の窓内。
- panic しないことを cargo-fuzz で継続確認。

### 5.4 LayoutUnit [R2-M1]

- 1/64 px の i32。表せる範囲は約 ±3.3e7 px。
- すべての加減乗算は飽和演算。溢れてもラップして負の幅にならない。飽和の発生は assert ではなく warning として記録する（fuzz は debug assertions 有効で走らせるので、「panic しない」と両立させる）。LayoutUnit は pub(crate) で warning の置き場を持たないため、飽和の回数をカウンタとして LayoutContext（build 時は builder）に持ち、next_line / build の終了時に 1 件の warning にまとめる。[R3-m4]
- f32 からの変換は 2 種類を定義し、用途で使い分ける:
  - `from_f32_round`: advance、glyph や fragment の位置、および呼び出し側から受け取るすべての値（AtomicSizes、LineConstraint など）
  - `from_f32_ceil`: shodo 内部で導出する寸法（フォントメトリクスからの行の高さなど）に限る。呼び出し側の入力に ceil を使うと、taffy の f32 の誤差（100.000001 など）が 1/64 px の系統的なずれになるため [R3-m7]
- 非有限の f32 は 0 に正規化し warning に記録する。
- 公開 API への出力は `to_f32`（1/64 刻みは f32 で約 2^18 px まで正確に表せる。それを超える位置は f32 の精度に従う）。
- 公開 API の f32 入力（AtomicSizes、LineConstraint など）はすべて from_f32_round で取り込む。

## 6. サブプロジェクト間の契約・受け皿・テスト

### 6.1 契約

| 境界 | 契約 |
|---|---|
| S1 → S2 | `FontCollection::matcher(&FontQuery)`（families, weight / width / style, lang）が書記素クラスタごとに FontId を返す（文書層 → 共有層、unicode-range・フォールバック・絵文字表示込み、キャッシュ付き）。`fonts.metrics(FontId, size, coords)`（横書き＋あれば縦書き）。`fonts.shaper_data(FontId)`（共有の ShaperData）。`resolve_ch / resolve_ic`。 |
| S2 → S3 | ParagraphData: 処理後テキスト、item 列、bidi 段落の範囲と bidi レベル、shaping 済み run（SoA、advance と累積位置は LayoutUnit、glyph ごとに UNSAFE_TO_BREAK / UNSAFE_TO_CONCAT）、改行機会（テキスト位置 = 書記素境界ごと: 必須 / 許可 / 禁止 / 緊急時のみ（overflow-wrap）/ ハイフン可）、伸縮位置（justify の全種類、将来の約物詰め）、first-line 用データ集合（任意）、OffsetMapping（任意）。行端の再 shaping: 改行位置が UNSAFE_TO_BREAK のとき、**行末側と次の行の行頭側の両方**を、それぞれ直近の safe 位置から再 shaping する。ハイフンの挿入など内容が変わる場合は UNSAFE_TO_CONCAT で接合境界を判定し、再 shaping した範囲と元の結果との接合部も同じ規則で再検査して、必要なら窓を広げる（窓は Limits、超えた場合は §5.3 の劣化として扱う）。shaping 時は harfrust の PRODUCE_UNSAFE_TO_CONCAT を有効にする。[C1-M10] |
| S3 → 利用者 | LineResult / Line / Fragment |

### 6.2 縦書きの受け皿（S6）

内部幾何は論理座標、WritingMode は ParagraphStyle。run が向きのフィールドを持つ。S1 メトリクスに縦書き項目。text_combine_upright は InlineStyle に予約。Line::baseline は種類を引数に取る。

### 6.3 ルビの受け皿（S7）

Ruby 系 item を予約。注釈は Fragment 内の入れ子の行組み結果。Line::block_size は「行送り量」（次の行の位置を決める寸法、max_block_size が制約する対象）と定義し、描画範囲（注釈を含む ink / overflow の範囲）は `line.overflow_rect()` として別に返す。ルビ注釈は通常 block_size の計算には入らず、line-height が不足する場合にだけ CSS Ruby 1 §3.6 に従って追加の leading として block_size を増やす。BreakToken は非公開なので注釈側の位置を後から追加できる。[C1-m12]

### 6.4 テスト

- テスト用フォント（Ahem、Noto subset）を同梱し、テストではシステムフォントを使わない。
- 行組み結果をテキストでダンプして insta スナップショット。
- 性質テスト / fuzz（debug assertions 有効）: token の進行（例外なし）、float による呼び直しの有界性、OffsetMapping の往復、panic しない、Limits 超過が Err になる、LayoutUnit の飽和。
- CI で wasm32 ビルド。
- ベンチマーク（criterion）: build / next_line / intrinsic_sizes、コールドスタート（FontCollection と LayoutContext の初回生成＋最初の段落、CJK フォントの ShaperData 生成を含む）、メモリ使用量（1 glyph あたり、Line あたり、justify 行あたりのバイト数）。
- M1 の最終合否は S4 で raikiri WPT が後退しないこと、raikiri のベンチ（fulgur_baseline など）が悪化しないこと。

### 6.5 S0 の実装範囲（walking skeleton）

- 公開 API の型をすべて定義（LineResult の全 variant、LineOptions、Limits、Warning を含む）。LayoutUnit（飽和演算と 2 種類の丸め）/ WritingMode / PhysicalConverter / OffsetMapping / Limits を実装。
- 仮実装: 1 フォントの FontCollection（共有層のみ）、固定幅の shaper、空白のみの改行。
- 最初から通す経路:
  - ParagraphBuilder → Paragraph → next_line → Line
  - 行をまたぐ入れ子の InlineBox と縁フラグ
  - Atomic 1 個
  - RTL の run 1 つ（bidi の並べ替えと、1 つのインライン要素が複数の断片に割れる場合）
  - item 境界での GlyphRun の分割
  - OffsetMapping の往復（空白の畳み込みを含む）
  - start_token、token の保存 → 別の幅で再開、別の Paragraph の token で InvalidToken
  - Paragraph が保持するフォント層経由での next_line 内の再 shaping（仮 shaper で可）と line.font_data
  - FloatEncountered → 呼び直し → Line（行頭の float と行の途中の float、inline_start_offset がある行での残り幅判定、組みかけの行のキャッシュによる再開）
  - tab を含む行で float を配置した後に float の前で改行が起き、displaced_floats → 取り消し（未報告に戻す）→ 呼び直し → float の item が載る行で改めて判定、になる経路（規則 6 の反例のテスト）。float より前の内容（tab の後の長い span）が 2 行以上に及ぶ場合に、float がその item の載る行の上端（または下）に置かれることも確認する [F6-M1]
  - 複数の float（例: F1 = 20px、F2 = 50px）で、後から配置した F2 だけを取り消せば F1 がその行に残るケース（規則 8 のテスト）[C5-m1]
  - 単語の途中の float（例: 幅 60px、字幅 10px で `aa b[F]bbbb`、F は 30px）で、単語全体が次の行に送られるため F が現在の行で報告されないこと（停止性の回帰テスト）[C6-m1]
  - BlockInInline（空の行を含む）、BlockSizeExceeded（分割コンテナ先頭での受け入れを含む）
  - justify 行での行専用の位置配列
  - Limits 超過で Err（push ごとの増分検査、フォントの登録予算）、warning の上限
  - pen の累積位置の run 分割（巨大な advance での位置の正確さ）と glyph の offset
  - required_baseline（sideways の span の中の atomic）
  - フォント受け入れ時の構造検査で、重複参照による増幅フォントを拒否する（仮のフォント層でも検査の関数だけは実装してテストする）

## 7. メモリ・速度・セキュリティ（要約）

- メモリ: glyph は ParagraphData に 1 回だけ持ち、Line は Arc と範囲。行ごとの位置が変わる行だけ 4 バイト / glyph を追加。SoA で LayoutUnit（i32）。スタイルは共有。complex-scripts のデータは feature で切る。システムフォントは mmap。文書層は参照が無くなった時点で解放され、Web フォントもそこで解放される。ShaperData はフォントごとに 1 回（件数上限の LRU。1 件あたりの量はフォント受け入れ時の構造検査で有界）。層ごとの予算（face 数・blob 合計）と max_shaped_glyphs。[F5-m3]OffsetMapping は不要なら作らない。
- 速度: システムフォント走査は遅延（実行時に無効化も可）。ShaperData の共有でコールドスタート時の GSUB / GPOS 解析をスレッド数に比例させない。bidi が不要な段落は bidi 解析を省く（条件: RTL / AL 文字と bidi 制御文字が無く、段落方向が LTR、全 item の unicode_bidi が normal かつ direction が ltr）。[R2-m4] Paragraph の再利用（LineOptions の分離で text-align の変更でも再 build 不要）。フォールバック結果のキャッシュ。word cache は S2 でベンチを取って判断。
- セキュリティ: forbid(unsafe_code)、フォント解析は fontations と harfrust（safe Rust）。Limits と fail-closed、warning の上限。入力値の正規化（build 時と next_line 時）。LayoutUnit の飽和。計算量の保証。信頼できないフォントは mmap しない。WOFF 展開は外部。

## 8. 各サブプロジェクトへの申し送り

- S1: フォント受け入れ時の構造検査（重複参照を数えた lookup / subtable 数、閾値、他の展開構造の要否）、required_baseline に必要な情報の提供、2 層コレクション（文書層は unshared の独自 Collection）、matcher（文書層 → 共有層、unicode-range、書記素単位、絵文字表示、lang 別の漢字字形）、フォールバックキャッシュ、ShaperData の共有キャッシュ、縦書きメトリクス、resolve_ch / ic、フォントの Limits、システムフォント走査の遅延と実行時の無効化、SourceCache の prune 設定、破棄された layer_id の通知、ShaperData の Sync 確認。
- S2: 空白処理と segment break 変換、text-transform（言語別）、bidi（段落内の複数 bidi 段落、plaintext、BlockInInline 前後の embed / isolate の閉じ直し）、改行機会（line-break の各値、word-break、overflow-wrap、soft hyphen）、UNSAFE_TO_BREAK の保存、累積位置の計算、first-line データ集合、complex-scripts の既定、word cache の判断、bidi を省く条件の判定、LayoutContext の作業バッファの縮小、両側の再 shaping と UNSAFE_TO_CONCAT、max_shaping_run_bytes での run 分割と巨大クラスタの強制分割、max_shaped_glyphs の累積検査、harfrust への失敗状態公開の上流提案。
- S3: float を含む分割できない区間の先読み（手順 1）、float 後の再利用で位置に依存する部分（tab）の再評価と計算量の上限、overlay の出力予算と cluster 単位の差し替え、required_baseline による atomic の baseline、next_line / LineResult の各契約（float の呼び直し、BlockSizeExceeded、BlockInInline と空の行）、行ボックス構築（vertical-align 全値、top / bottom は行全体確定後、strut、line-height: normal）、text-indent、tab-size、spacing、text-autospace、揃えと行専用の位置配列、intrinsic sizes（float の寄与、first-line で max-content）、ぶら下げの扱い、ハイフン glyph と overlay、ヒットテスト（伸縮後の cluster advance）、BreakPlan（balance / pretty）、BlockInInline 以降に first-line を適用しない規則。
- S4: float の仮配置と取り消し（保存・復元の単位は (token, cursor, BFC の状態, 保留 float の一覧, この行で取り消した float の記録)、[C6-m2]displaced_floats の処理、taffy の BlockContext に checkpoint / rollback が無いため、taffy の拡張か行単位の仮配置コンテキストを設計。必須）、2 層コレクションへの移行、Paragraph の再利用、float と BlockInInline の BFC 側実装（taffy の BlockContext 連携、保留 float の管理）、先読み API による widows / orphans、OffsetMapping の無効化（PDF 出力）、ベンチの比較。
