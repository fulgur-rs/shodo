# SHODO-42: 階層型 limits ledger

## 目的と契約

段落、first-line データ、ruby annotation と imported base の資源会計を、常時有効な一つの台帳に集約する。公開 `Limits` / `LimitExceeded` と警告の順序を維持する。上限超過は所有 scope の元の上限と、その scope の累積 actual を報告する。残量が等しい scope でも所有情報から決定し、残量を使ってエラーを推測しない。

## 構造

`limits::ledger::LimitsLedger` が scope の上限、累積使用量、親 scope、段落 frame を所有する。段落 root を最初の scope とし、annotation はその paragraph または owning base の子 scope、imported base は raw boundary が決める子 scope にする。frame は raw traversal の current owner、processed item の owner、ruby container の owner を保持する。通常と first-line は別 owner table を使い、同じ scope の累積会計を共有する。

producer は `charge(item, kind, n)` または current/container owner の charge を allocation の前に呼ぶ。全 owning chain を確認した後に累積値を更新する。最も近い所有 scope から root に向かってチェックする。processed items と shaping items は既存どおり別の Items counter を同じ cap と照合する。raw/whitespace text は当該入力の transient counter、transform output は累積 retained counter を使う。

whole-table style の imported base 集計は既存の imported style indices に従う。同じ table slot の ancestor 二重課金を避けるため、paragraph 全体と各 base の inclusive style allowance はそれぞれ直接予約する。mapping と analysis の既存 cost structs は producer 使用量の統計として残せるが、残量上限のコピーや別の上限判定を持たない。

## 試行と再利用

TCY width selection は台帳の checkpoint を作り、試行会計を rollback で分離する。選択済みの会計を commit する際、記録した producer 境界順で元の scope cap を照合する。エラーを再現するための shaping は不要になる。discard された glyph output は累積 retained counter を消費しない。試行の warnings/saturation は選択されたものだけを従来の順序で取り込む。

paint-only first-line の analysis/shape clone は、会計予約が replay と一致する場合に利用する。clone 前に producer 相当の予約を実施し、制約により試行結果または warning が変化する場合は replay に戻す。既存警告があるという理由だけで常時再利用を無効にしない。

## 選択した方式と代案

owned ledger と frame id を採用する。共有 `Rc<RefCell<_>>` は公開 analysis の Send/Sync を変えるため避け、共有 mutex は build の hot path に同期処理を増やすため避ける。旧 RubyBudget と BaseScopes を並べて新しい facade で隠す方式は状態とエラー推測を残すため採用しない。

## 検証

nested/sibling base、同じ残量の root と child、first-line alternate、annotation と ancestor、TCY trial rollback/commit、warning suppression の順序を確認する。既存の allocation oracle、analysis/mapping boundary、TCY reference/reuse sweep を維持する。ライブラリ全体の default / no-default-features / all-features テストは統合作業で行う。line ファイルは SHODO-41 の担当に委ねる。


## 実装上の決定と確認結果

- item 所有表は `(processed item start, scope id)` の境界表にする。所有者が変わらない通常テキストの一文字・一 item ごとの台帳 allocation を避け、base boundary 数で記憶量を制限する。
- 成功した pass の複製予約は当該 frame の local scopes と祖先の cost 差分のみを保持する。別 annotation の scopes を走査しない。共有 source-cut 分は差し引き、root 全体の raw/transform/analysis producer allowance と base inclusive allowance をそのまま replay する。
- width trial は shaping pass 開始時の paragraph/祖先余裕で admission し、imported base の glyph cap は選択結果の commit 時に適用する。従来の width selection を保ちつつ、元の producer charge 順で owning scope のエラーを返す。
- 選択された output の windows が scoped run cap により変化するときは、その別入力を shaping する。glyph cap エラーの再現目的では shaping しない。
- first-line shape clone は選択済み会計に加え、unselected width trial の最大 glyph 数を admission できる場合に使う。retained glyph 数だけで unselected trial を推測しない。
- 回数 assertion の更新は成果の変化を検証するため。glyph 超過を commit で検出するケースでは shaper calls 5→4（paragraph root）/3→2（base）。ruby paint-only first-line は analysis/shaping passes 3→2。既存 warning があるが analysis producer が新しい warning を出さない入力は analysis passes 2→1。各テストは従来の geometry / warning sequence / LimitExceeded 値との比較を維持する。
- 深い base の commit operation count は、新たに常時含む paragraph root scope の 1 段を含める。従来の `charges + depth` の線形 bound は維持する。
- 新規 ledger の型未定義による RED を確認してから実装。nested/sibling/cap tie/rollback、飽和加算、source-cut replay 除外、独立 Items counter と、216 通りの cap 組合せの producer preflight を検証する。

- レビュー追加修正: checkpoint は touched scope の旧 Cost を一度だけ保存し、window producer 記録と分離する。128 windows・17 scopes の regression で undo snapshot が 2176→17 になる RED→GREEN を確認した。nested checkpoint が outer snapshot を復元するテストも追加した。
- レビュー追加修正: whitespace Items は ledger 課金後の push でのみ容量を増やす。cap を使い切った alternate/ancestor の失敗前に raw.len 相当を予約しない。exhausted Items ledger の既存コードが 90112 bytes を先に確保する回帰を RED→GREEN で検証した。
- nested commit は outer journal が存在する場合に producer charge を合算しない。outer rollback 後の再 commit でも最初の失敗 producer の actual を維持する（RED actual 106→GREEN actual 101）。
- `max_shaping_run_bytes` は累積 resource cap ではなく当該 paragraph の run window cap。imported base の local chain には適用し、別 annotation frame の外の ancestor からは継承しない。UTF-8 annotation が ancestor の 1-byte cap により余計な warning を出す回帰を RED→GREEN で検証した。


## 検証記録

共通実行条件: native cargo +stable、`RUSTC_WRAPPER=`、task worktree 内から `CARGO_TARGET_DIR=../../target/shodo-42`、`TMPDIR=$HOME/tmp`、kache test runner を環境から外す。テストの一時フォントファイル作成だけ通常権限で実行した。

- 全 crate default test: lib 1020 pass / 8 ignored、全 integration と doctest 10 pass（最初の全体確認）。
- レビューの journal/capacity/nested commit 修正後 full lib default: 1026 pass / 8 ignored。
- 同時点 full lib no-default-features: 1021 pass / 8 ignored。
- 最終の annotation 非累積 run-cap 修正後: shape 75 pass / 2 ignored、ruby limit 8 pass、paragraph build 51 pass、追加 annotation regression は default/no-default とも pass。
- ledger の journal/capacity を含む対象テスト: 14 pass。
- 最終 clippy: `-p shodo --all-features --all-targets -- -D warnings` pass。
- 最終 fmt と diff whitespace checks pass。

統合後の全 workspace / CI matrix と merge は SHODO-41/42 の統合担当で実施する。ビルド cache `target/shodo-42` は継続 review/統合再現に必要な間保持し、作業完了時にこの task の cache だけを片付ける。
