# t6q.7: AccessKitのchunk別word startsを範囲検索する

採用。行の整列・重複除去済み`word_starts`から、各chunkの半開区間に入る境界だけを二分探索で切り出す。全境界数W・chunk数Kに対し、従来のO(KW)走査をO(K log W + W)へ減らす。範囲は互いに重ならず、相対offsetは従来どおり0..254。source、glyph、geometry、公開APIと既定limitsは変更しない。

変更前は`a62458009af2f3b3cc83441eb735b70ef8175807`。Rust 1.96.0、x86_64 Linux、release profile、固定Latin font（`shodo_fixtures::FONTS[0]`とfixture manifestのchecksum）、system fontなし。入力は`a `の反復で、単一DOM runと単語ごとのDOM runを分ける。既定limitsで1行、非ゼロglyph、期待単語数を確認する。

測定対象はfresh AccessKitAdapterの生成・update・adapter破棄で、TreeUpdateをscope終了まで保持する。paragraph build、line layout、AccessibleLayout作成、hash、JSON出力はscope外。通常allocatorの時間用binaryとallocation-countingの割当用binaryを分離した。各binaryを保存し、共通flock内でnice=10、jobs=1により直列実行した。初回A/B各10回とABBAの各10回で計30標本/側。割当は別binaryで10回/側。

レビューでJSON keyの確保がscope終了前に混入する問題を発見し、counts/elapsedを先にlocalへ確定するよう修正。以下は修正後に両側を再取得した結果のみで、初回の不適切な測定値は含めない。

| 単語数 | DOM run | Calls 前→後 | Gross bytes 前→後 | Net bytes（同じ） | Peak extra bytes（同じ） |
|---:|---|---:|---:|---:|---:|
| 512 | 単一 | 194→175 | 500,564→499,820 | 15,240 | 246,248 |
| 4,096 | 単一 | 1,304→1,159 | 4,006,116→4,000,220 | 116,264 | 1,964,328 |
| 16,384 | 単一 | 5,026→4,447 | 16,024,884→16,001,276 | 462,632 | 7,854,888 |
| 256 | 単語ごと | 3,650→3,394 | 882,932→880,884 | 241,056 | 443,968 |
| 1,024 | 単語ごと | 14,418→13,394 | 3,534,932→3,526,740 | 963,744 | 1,774,912 |
| 4,096 | 単語ごと | 57,442→53,346 | 14,142,644→14,109,876 | 3,854,496 | 7,098,688 |

割当減少は、filterの可変長収集を正確な長さのslice収集に変えた効果。保持量とpeakの改善は観測していない。値はRust allocatorへ要求したblockであり、RSSやstack量ではない。

| 単語数 | DOM run | 時間中央値 ms 前→後 | 後/前 |
|---:|---|---:|---:|
| 512 | 単一 | 0.164→0.193 | 1.179 |
| 4,096 | 単一 | 1.720→1.931 | 1.122 |
| 16,384 | 単一 | 7.936→12.143 | 1.530 |
| 256 | 単語ごと | 0.284→0.280 | 0.987 |
| 1,024 | 単語ごと | 2.041→1.492 | 0.731 |
| 4,096 | 単語ごと | 17.506→7.058 | 0.403 |

共有ホストの負荷変動が大きく、たとえば16,384単語・単一runのプロセス別中央値は前7.190〜16.247ms、後7.673〜13.864msだった。集計中央値では単一runが悪化しており、一律の速度改善は主張しない。多数の短いrunでは改善を観測したが、その比率も探索値である。採用根拠は実走査回数の上限と割当削減、出力同値性であり、raikiri/S4の必須依存にはしない。

全6入力について、時間・割当の両buildと全反復でTreeUpdate全体のDebug SHA-256が前後一致した。含まれるtext、word starts、node IDs、bounds、関連属性が同じ。描画glyphは本変更の入力であり変更しない。単体テストでは旧filterを独立oracleとして255文字chunkと多数短runを照合し、adapterの初回・再利用両方を検証する。

RED: 2,048単語の旧filterで34,816 visitsとなり上限2,592を超えた。GREENでは全4入力が上限を満たす。実際の二分探索比較と収集の両方を計数し、通常の出力一致だけでは通らない性能回帰テストにした。empty word list、長いwordの後続chunk、atomic/縦書き等は既存AccessKit harnessで確認した。

検証: `cargo test -p shodo --features accesskit`（lib459 pass、既存診断ignore2、integration/docs通過）、`cargo test -p shodo-harness --features accesskit --test accessibility --test accessibility_accesskit`（13+11 pass）、対象crateのall-targets clippyとfmt通過。harness featureなしで0件になった初回AccessKit実行は証拠にせず、正しいfeatureで11件を再実行した。

再現用probe:

```sh
cargo run --release -p shodo-bench --example accesskit_word_starts --features shodo/accesskit
cargo run --release -p shodo-bench --example accesskit_word_starts --features shodo/accesskit,allocation-counting
```

変更前にも同じprobeを配置し、それぞれの実行ファイルを保存して交互に実行する。time/allocatorは同じscope内で併用しない。環境のrustc wrapper/runnerは無効化して計測した。Raw、保存binary、集計はworkspaceの`target/performance-artifacts/t6q-7/`、集計scriptは`target/t6q-control/summarize-7.py`にある。
