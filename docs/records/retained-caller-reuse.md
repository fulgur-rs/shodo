# Retained raikiri callerの再利用経路（shodo-sbp.14）

既存の代表callerは毎回contextとParagraphを作っていた。実CSS/DOM walker、source link投影を保持したままprepare/outputを分離し、immutableなresolved input・context・prepared Paragraphを所有する開発用sessionを追加した。既存fresh wrapperとcore API/limitsは維持する。保存S4 spikeには変更せず、本番切り替えの条件にしない。

採用するのは明示的なsession再利用経路。contextだけの再利用はParagraph buildを省略せず、計測で改善しない／遅くなる条件もある。Paragraph保持には初期準備と保持費用があり、多リンクの共有glyphでは再レイアウト後も多数のedge shapingが残る。exampleの値をproduction/frameの保証にしない。

## Invalidationとtoken

- sessionはresolved DOM・normal style・first-line cascadeを所有し、mutable inputを公開しない。text/style/first-line変更はreplace_inputでpreparedを捨てる。
- shared/document layer identityと実際の登録generationを各操作の前に照合し、変更やcollection置換で再構築する。documentは指定sharedから作ったlayerであることがcallerの契約。generation変更は操作間を対象とし、並列変更のtransaction保証をしない。
- generic/fallback family設定の変更も共有layerのgenerationを進めるため、sessionは次の操作で自動再構築する。collectionの置換はreplace_fontsで明示し、同じhandleの明示的な再構築にも利用できる。
- font準備中のgeneration変化は拒否し、再構築失敗時に古いpreparedを公開しない。古いtokenは再構築後にInvalidTokenとなる。fresh paragraph IDは当然異なるため、同位置のunit/flagsとcontinuationを照合し、同paragraphのrepeatはtokenを直接比較する。
- 高さ拒否でtokenを進めず再試行する。空ページで高すぎる行は高さ制限を外して受け入れ、height0でも進行する。このinline-only callerは外部float/blockの配置を追加しない。

## 測定と出力契約

固定Latin/CJK/Arabic fixtureをsystem font discoveryなしで構成し、plain・linked shared ffi・複数font-size・実CSS first-line＋uppercaseの1/128反復を使う。幅400→80→160→400、高さ25→100→0→100を比較する。fresh context＋paragraph、reused context＋fresh paragraph、retained paragraph幅変更、retained paragraph高さ変更の計192条件。高さもfresh/reused-contextで同じページ処理を使い対照にする。

24条件の元のfresh callerを別binaryで保存し、幅経路のsource/glyph/fragments/line geometry/mapping/linkを完全一致で確認。全192条件でfreshとの差と4最終時間プロセスの出力差はビット一致。警告controlsは空で一致。Pythonの独立高さoracleはaccepted lineの切れ目・同token retry・oversize進行を確認する。元の全幅出力に既にfloat32で丸められたglobal Y/union高さがあるため、ページ原点へ移す補助比較だけは4 ULP由来の丸め上限を使う。採用gateのpaged link全geometryはfresh-height Paragraphとビット一致を必須にする。一般的なgeometry toleranceではない。

time/memory/trace/original oracleを固定sourceから別binaryにした。通常shapeとfont metrics shapeの製品側2呼出箇所をdisposable overlayで数え、trace tupleは[calls,scalars,metric_calls,metric_scalars]。metrics counterは今回のwarm操作で0だった。native geometryを測定windowに入れず二重shapeを持ち込まない。timeはcounter-free CPU10、全build/関連checks完了後にforward/reverse/reverse/forwardの4freshプロセス、各10samplesの全rawと中央値を保存した。初回time.jsonはcontrolのみ。

| 条件 | 経路 | mean(process median) ms | fresh比 | gross B（中央値） | net B（全範囲） | peak-extra B（全範囲） | 実shape calls（全範囲） |
|---|---|---:|---:|---:|---:|---:|---:|
| plain/1 width step1 | fresh | 0.0604 | 1.0000 | 42321 | 11559 | 18379 | 1 |
| plain/1 width step1 | reused-context | 0.0574 | 0.9514 | 36289 | 2120 | 12320 | 1 |
| plain/1 width step1 | retained | 0.0134 | 0.2216 | 13936 | 2120 | 6488 | 0 |
| plain/128 width step1 | fresh | 2.5651 | 1.0000 | 4241728 | 711379 | 1432312 | 127 |
| plain/128 width step1 | reused-context | 2.6971 | 1.0515 | 4073136 | 180828 | 719120 | 127 |
| plain/128 width step1 | retained | 1.5131 | 0.5899 | 2418237 | 180828 | 718940 | 0 |
| links/128 width step1 | fresh | 9.7862 | 1.0000 | 8050021 | 756830 | 1234249 | 2053 |
| links/128 width step1 | reused-context | 9.8754 | 1.0091 | 7963349 | 164356 | 858278 | 2053 |
| links/128 width step1 | retained | 5.3705 | 0.5488 | 4864903 | 164575–165764 | 436472–437661 | 1538–1541 |
| links/128 height step0 | fresh | 7.8622 | 1.0000 | 7989190 | 730554 | 1028353 | 2216 |
| links/128 height step0 | reused-context | 9.3665 | 1.1913 | 7902518 | 138272 | 858278 | 2216 |
| links/128 height step0 | retained | 7.5720 | 0.9631 | 4806884 | 138272 | 142025 | 1704 |
| styles/128 height step0 | fresh | 9.0769 | 1.0000 | 6438991 | 1097581 | 1153764 | 434 |
| styles/128 height step0 | reused-context | 4.7663 | 0.5251 | 6433919 | 255780 | 1131285 | 434 |
| styles/128 height step0 | retained | 1.7728 | 0.1953 | 3045594 | 255780 | 259476 | 0 |
| first-line/128 width step1 | fresh | 12.9537 | 1.0000 | 13102705 | 2679789 | 3807703 | 258 |
| first-line/128 width step1 | reused-context | 13.7707 | 1.0631 | 12770273 | 412160 | 2414912 | 258 |
| first-line/128 width step1 | retained | 6.1479 | 0.4746 | 4043980 | 412160 | 1153722 | 0 |

長リンクretainedのwidth step0/1/3は割当・shape sequenceが変動する。全sampleと範囲を残し、1値を決定的費用としない。既存EdgeShapeCacheには256 entries／32768 costの上限とclearがあり、状態の周期変動と整合するが、clearイベント自体は計数していないため原因は推論として扱う。出力は全sampleで同じ。context-onlyの遅い対照も除去しない。

## 保持費用

parse/cascade、font登録、preconditioning/warmups、payload/serialization、通常output dropは操作window外。初期setupはprepareと幅400のlayoutを含む。setup net＋setup output release netで、caller scratch/preparedの増分を示す。warm操作netは生存するaccepted outputと既存cacheの増減を含む。owner releaseはsetup前に確保したsession parsed inputも解放し、setup netの逆数ではない。構成済みshared fonts/cacheはowner release後も生存する。gross/net/peakはrequested allocator bytesであり、RSSや絶対process保持量ではない。

| 条件 | 経路 | 初期caller保持増分 B | owner release net B |
|---|---|---:|---:|
| plain/1 | fresh | 0 | 0 |
| plain/1 | reused-context | 11759 | -11759 |
| plain/1 | retained | 11939 | -55618 |
| links/128 | fresh | 0 | 0 |
| links/128 | reused-context | 859263 | -859263 |
| links/128 | retained | 881167 | -4104443 |
| first-line/128 | fresh | 0 | 0 |
| first-line/128 | reused-context | 2408637 | -2560273 |
| first-line/128 | retained | 2430413 | -6429007 |

## 検証・archive

literal ffi glyph367、font32、部分source link幅10.112の独立guardsの後に保持RED→GREEN。入力/normal style/first-line、両font generation、同generationの別collection、旧token、拒否入力の回復、height25/100/0を7testsで確認。default/allocation caller全100 tests、fmt、workspace all-target Clippy -D warnings、docs -D warnings通過。core/標準harness指紋は.13から変わらないため標準54 matrixは再実行せず、CIは通常repo checksを行う。

engine source `0ed16ba57ea875f1dbdcff2d8b2c88ac3e65d8f052f6f7713a1d8d4a3c121aaf`、caller source `5391309428203b32514cbd5be99ecac8f7b696684781938bef04f3151170068c`、source commit `65e13f731336fcc5ae14e80b23b4b343a50dfc17`。manifest [data/retained-caller-reuse.json](data/retained-caller-reuse.json)、raw [data/retained-caller-reuse-raw.json.gz](data/retained-caller-reuse-raw.json.gz) 17029874 bytes／SHA256 `0dbb350c2f8a79bba29fbc7c66db255e39347dd98d7ce7a6801f537030dee9e9`。全192 rows、4最終時間プロセス、原caller/source、font bytes/pins/licenses、binary/harness/overlay、recipe、host、検証ログを含む。

初回overlayはfixturesのinclude JS不足でbuild前に失敗したため、source/log/overlayを保存してコピーを修正した。回復testのabsolute＋first-line fixtureはcascade段階で拒否されたためnormal inputへ修正した。どちらも保持・費用のREDには数えていない。未測定の多script/font variations、並列contention、RSS、production/browser frameの改善は主張しない。
