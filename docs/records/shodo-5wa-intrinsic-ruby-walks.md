# shodo-5wa: intrinsic_sizes の word と段の ruby walk を保持する

## 変更と判断

`RubyMemo` の walk を、dataset の id・アドレスと start ごとに最大2つ保持する。
`intrinsic_sizes` は現在の段の `(owner, total_unit)` を保持対象として指定する。
新しい word start は段以外の walk を追い出す。強制改行と first-line の dataset
切り替えでは保持する段のキーも更新する。他の呼び出しは2件の LRU とする。

単純な2件 LRU では足りなかった。float 間で2つ以上の word start を計測すると、
段の walk が追い出され、次の clear float で再走査する。この形を回帰テストに
追加して失敗を確認してから、段のキーを保護する処理を入れた。

atomic revision は walk の入力ではないためキーに含めない。計測結果の memo と
accumulator は従来どおり revision を区別する。小さくなった end は既存の
`advance` で再走査する。入れ子の take/put は最後に返した同一キーの状態を採用し、
保持数を2件以下にする。操作開始と `shrink_to` の `clear` で両 walk の内部
ベクターと段の保持キーを破棄する。walk 歩数の課金は残す。

## 作業量の確認

2026-10-07、変更前 HEAD `bebba58`、Rust 1.96.0。
固定 CJK / Arabic / Latin font を使う既存の ruby テスト用 fixture で計測した。
`max_ruby_line_work=None` とし、予算による打ち切りで線形に見えることを避けた。
時間の測定ではなく、実際に訪問または再適用した container の歩数を比較する。

入力は r 個の sibling ruby（base `12`、annotation `日`）の後に、
`日本語の文字列 + clear:Both float` を 4r 回並べた1段。
各文字が word probe を生成する。r を16、32、64に倍化した。

| float 間の文字列 | 1状態（変更前） | 単純な2件 LRU | 段を保護する2状態（採用） |
| --- | --- | --- | --- |
| `日` | 1,040 / 4,128 / 16,448 | 線形性テスト通過 | 16 / 32 / 64 |
| `日日` | 未計測 | 1,024 / 4,096 / 16,384 | 16 / 32 / 64 |
| `日日日` | 未計測 | 未計測 | 16 / 32 / 64 |

採用した実装の倍化比は全ケースで2.0。64 ruby + 512 float（間に `日`）では、
無制限・factor 1・既定の予算のすべてで、最適化しない参照経路と幅・警告が一致し、
予算超過もない。残る歩数が作業予算に課金されていることも検証した。

再現:

```sh
TMPDIR="$HOME/tmp" cargo test -p shodo --lib clearing_float -- --nocapture
```

状態の再利用・追い出し・clear・入れ子に加え、既存の nested ruby、複数 base、
RTL、first-line の fixture で start と dataset を切り替え、full walk と
`through` および訪問順が一致することを確認するテストも追加した。

## 検証

- `cargo fmt --all --check` と `git diff --check`
- `cargo clippy --offline --workspace --all-targets -- -D warnings`
- `cargo test --offline --workspace --quiet`: 1,708 passed、0 failed、9 ignored
- `cargo test --offline -p shodo --no-default-features --quiet`:
  1,173 passed、0 failed、9 ignored
- `cargo test --offline -p shodo --no-default-features --features complex-scripts --quiet`:
  1,176 passed、0 failed、9 ignored
- `RUSTDOCFLAGS="-D warnings" cargo doc --offline --workspace --no-deps`

環境の `kache` の Cargo shim と test runner は target の保護操作が
read-only filesystem で失敗したため、Cargo 本体を実行し、
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER` を外して検証した。
一時 target とログは `~/tmp` に置き、結果をここに記録した後で削除した。

## 対象外の残る経路

walk の保持と container 計測の accumulator は別である。float 間に、
それぞれ2個以上の ruby container を含む語が2語以上あると、accumulator の
2件 LRU から段の状態が追い出されうる。次の float で累積した段を再計測する
経路は本件では変更していない。通常の `日日` は container がないためこの
経路を通らない。これはレビューによる静的確認で、計測の倍化比は未取得。
調査時は `ruby_walk_steps` と `ruby_container_measures` を分けて測る。

本件は walk の再走査をなくす変更であり、intrinsic_sizes 全体のあらゆる入力を
線形化したという結論や、実時間の高速化率は示さない。
