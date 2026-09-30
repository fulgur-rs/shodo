# 実 CSS `::first-line` を raikiri から shodo に渡す設計

対象は `shodo-7ff`。代表 caller が実際の CSS 疑似要素を解決した入力で先頭行を描画し、次行には通常スタイルを使うことを検証する。既存の `dev/raikiri` の git pin は全体を更新する、という利用者の選択に従う。`shodo` の公開レイアウト API は既存の `ParagraphStyle::first_line` と `ParagraphBuilder::open_inline_with_first_line` を使う。

## 現状と範囲

現在の pin `ab7e619a8f321f03de8b8c8b9342954868e044c8` は `::first-line` を解析できない。`first_line_cascade` と代表 caller は同じ DOM に通常 CSS を追加して別の cascade を作る契約テストであり、疑似要素の生成テストではない。raikiri の後続変更は疑似要素の解析と発生元要素の computed style を提供するが、先頭行にある子孫へ継承を適用した別スタイルを提供しない。この不足を raikiri 側の小さな API で埋める。

対象は静的な一つのブロック内で、代表 caller が現在受け付けるインライン要素、テキスト、`br` と静的フォントである。複数のブロックにまたがる first-line、動的な DOM/CSS 変更、縦書き、未対応のレイアウト要素はこの caller の対象外とし、入力検証で明示的にエラーにする。疑似要素の適用先と物理的な先頭行の決定は shodo の既存の行分割に任せる。

## データの流れと境界

1. raikiri が HTML と実 CSS を一度だけ解析し、通常の cascade と `::first-line` の発生元スタイルを得る。通常 DOM、ノード ID、テキスト、セレクター一致結果は保持する。HTML 書き換えや通常 CSS の根要素上書きは使わない。
2. raikiri-style に、指定したブロックの `::first-line` スタイルを継承元として、同じ DOM のインライン子孫を再解決する API を追加する。結果は通常 computed style と対になるノード ID 対応の first-line computed style とする。子要素自身に勝った明示宣言はその要素で適用し、宣言がない継承プロパティだけ疑似要素または先頭行上の親から継承する。相対値は各経路の継承元に対して一度だけ計算する。対象外の子孫に first-line スタイルがあるかのように見せない。
3. raikiri の疑似要素 cascade は CSS の `::first-line` に適用できるプロパティだけを採用する。CSS として無効な宣言は通常どおり無効にする。継承の対象外である writing-mode、direction、text-orientation と、通常要素が持つカスタムプロパティの継承元は仕様に従って扱う。API は解決の失敗を返せる形にし、通常 computed style を破壊しない。
4. `dev/raikiri` の代表 caller はこの対を `ParagraphStyle::first_line` と各 `open_inline_with_first_line` に渡す。raikiri の computed value から shodo のスタイルへの変換は現在の明示的な検査を維持し、表現できない値やレイアウト要素を受け取ったら理由付きで拒否する。先頭行と次行は同じ DOM 由来のテキストと source mapping を使う。

raikiri の API は CSS 解決の責任だけを持ち、行分割や glyph 描画をしない。shodo caller は DOM/CSS cascade を再実装しない。raikiri の追加は upstream 側で変更として提出し、shodo はその変更を含む一つの確定 SHA に `dev/raikiri/Cargo.toml` と lockfile の依存を揃える。

## pin と既存の検証経路

`dev/raikiri` の実行対象はすべて新 pin へ更新する。新旧 raikiri の公開 API 差分で壊れる例・テストは、意味を保って移行する。`first_line_cascade` と `raikiri_contracts` は実 CSS 供給に更新し、通常 CSS 上書きを疑似要素の証拠として扱わない。契約境界の検証として必要な場合だけ、別名の fixture を明示して残す。

既存の測定 JSON、固定入力の記録、過去の結果に書かれた旧 SHA は履歴の出所なので上書きしない。新 pin で同じ測定を行う場合は新しい run と SHA を記録し、旧結果と混ぜない。pin 更新による他の結果差分は個別に調べ、無関係な期待値の一括更新で隠さない。

## 検証と完了条件

- raikiri の単体テストで、実 `::first-line` の解析、通常/疑似要素 cascade、子の明示指定と継承、相対 font-size、入れ子の子孫、適用対象外プロパティを確認する。同じノード ID の通常スタイルが変わらないことも確認する。
- shodo の代表 caller を実 CSS fixture で実行し、先頭行だけの font size、color、text-transform、glyph paint と source mapping、強制改行または幅による次行の通常スタイルを検証する。未対応の computed value と構造は明示的なエラーにする。
- `dev/raikiri` の既存テストと例を新 pin で通し、`Cargo.lock` と `Cargo.toml` の raikiri SHA を一致させる。pin 更新で生じる失敗は原因を調べて記録する。
- 固定した WPT 入力、参照、フォント、viewport、raikiri/shodo SHA とコマンドを使い、同じ対象で native と candidate の出力を比較する。描画または仕様適用の差分と未対応ケースを記録する。既存 pin で生成器がなかった機能は現時点では追加能力と分類し、native の必須非退行条件へ昇格するかは採用する pin と測定結果を確認して判断する。未計測の WPT 合格数は主張しない。

raikiri 側 API と shodo 側 caller はそれぞれ独立に検証してから統合する。実 CSS の入力、同一 DOM の二つのスタイル経路、先頭行と次行の描画・source、比較記録が揃った時点で `shodo-7ff` を完了とする。
