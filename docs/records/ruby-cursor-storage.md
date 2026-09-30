# Ruby cursor切断表の疎化（shodo-sbp.6）

2026年10月1日。base b2ad3ba（sbp5/7/8適用済み）との比較。実装a956fcd、レビュー修正0562e02。保持量を減らす変更として採用する。切替を止める性能依存やS4変更は追加していない。

## 診断と表現

独立した短いannotation C512では513cut×512lane=262,656cellに対しcursor変化は512回。元の表metadata/Vec capacityは2,142,208bytes、構築scope net7,742,689bytesの27.67%だった。CPU時間の支配項とは証明していない。

親unitではなくpaired-cut行番号を持つviewからimmutable表を共有し、各laneの初期値と変更行/値を保存する。密なlaneはdense Vec、小表はrow-major dense。全columnの実capacityとmetadata合計がflat arenaを超えれば表全体をdenseにする。全範囲の非空normal lanesでmandatory overrideがない場合は元walkerから直接denseを構築し、不要な転記を避ける。measure/source matchingは必要なlaneだけ参照する。

元の合法cut/class/nearest cursor判断、normal/first-line対応、旧count→構築の2walkを維持する。ancestor/base/global Itemsは旧count×(1+lane数)を割当前に課金する。C1024は同じItems actual1,064,969/limit1,048,576で拒否する。疎な物理表で予算は緩めない。

## 実表とレビュー修正

実フォントC16/64/256/512、first-line C512、単一長baseのdense1/4、計9表でunit/class/全cursor fingerprint・cut/lane/cell/変化数は元と一致。multi-column全範囲spanは内側column境界を禁止するためdense条件には単一長base/CJK readingを用いた。canonical before-finalを旧表証拠とし、拒否された初期overlap fixtureをA/Bに使わない。

C512 normal表payloadは2,142,208→41,024bytes、first-line表は2,121,768→41,024bytes。dense4 C512は57,376→32,864bytes。after payloadはArc制御領域16bytesとallocator overheadを除く。scope net/peakには実際の要求サイズを使う。

短い64laneは全cursor期待値の照合後、旧38,400bytesで16KiB上限のRED、新5,184bytesでGREEN。重複parent-unit行の異なる値/class、過大row上界、dense/sparse混合も直接照合した。

Astra全体レビューはCritical0/Important1/Minor0。8cut×64密laneのcolumn metadataにより旧4,416→試作6,432bytesへ増える指摘を実fixtureで再現した。builder表のみも6,176bytesでRED。総metadata込みのglobal dense fallbackを追加し、実表payload4,384+Arc16=4,400bytes、builder表4,128bytesでGREEN。normal/first-line両方の回帰を追加しruby91件成功。同じレビュー担当がこの1件の解消と追加回帰成功を確認した。小表の固定Arc overheadは残り、全入力の保持量非増加を主張しない。

## 最終専用A/B

固定CJK/Latin fonts、既定limits、system discovery無効。32条件×build/layout=64ケース。短い独立annotation、first-line、span、多level、empty、merge、overhang、RTL inter-character、nested、resume、全column span、dense1/4にdense64 normal/first-lineを加えた。input snapshot/shapingをbuild scopeに含め、fonts/context初期化と検証を除く。layoutはprepared paragraphを使い出力をscope終了まで保持する。

全64glyph/geometry/text range/ruby node・level・span・transform digest一致、warningsは毎回空。DOM offset-mapの完全serializationは収集していない。source対応は9表の旧cursor fingerprintと既存実フォントnormal/first-line/transform/共有cluster回帰で確認する。normal/countは別immutable binary。allocation3回はscope calls/gross/deallocation/net/peakが決定的で、scope外JSON保持による絶対live増加は判定対象外。

全32build netが減り、全32layout allocation指標は同じ。代表構築値（bytes）:

| 条件 | gross before → after | net before → after | peak追加 before → after |
| --- | ---: | ---: | ---: |
| plain/16 | 903,964 → 901,708 | 187,121 → 185,025 | 276,473 → 274,297 |
| plain/64 | 3,563,596 → 3,529,468 | 746,273 → 713,073 | 1,091,801 → 1,058,521 |
| plain/256 | 14,570,764 → 14,040,508 | 3,351,521 → 2,825,265 | 4,721,753 → 4,195,417 |
| plain/512 | 30,164,492 → 28,055,228 | 7,742,689 → 5,641,521 | 10,479,193 → 8,377,945 |
| first-line/512 | 36,318,481 → 32,161,193 | 11,452,595 → 7,270,699 | 13,983,979 → 9,920,591 |
| dense-4/512 | 2,485,283 → 2,501,811 | 822,058 → 797,562 | 979,984 → 971,792 |
| full-4/512 | 14,469,884 → 14,469,900 | 2,652,073 → 2,652,025 | 3,755,750 → 3,755,750 |
| dense-64/7 | 2,400,675 → 2,411,955 | 602,208 → 602,192 | 850,162 → 853,554 |
| dense-first-64/7 | 2,470,092 → 2,492,812 | 622,329 → 622,297 | 861,635 → 865,027 |

calls/gross/peakは一律には減らない。dense64 normalはnet16bytes減に対しgross11,280bytes増・peak3,392bytes増、first-lineはnet32bytes減・gross22,720bytes増・peak3,392bytes増。短い子paragraphごとの共有表構築費用もscopeに含む。少数の密な表自体が大きな利益とは扱わず、短い独立annotationの大きな保持削減を採用根拠にする。RSS未測定。元input/source/shapingの保持を削っていない。

## 最終時間と試作時の観測

全検証buildと最終54collector終了後、CPU10で両normal binaryをwarmupし、before/after/after/beforeの各条件10samplesを測定。構築run median（ms）:

| 条件 | before 2 runs | after 2 runs |
| --- | ---: | ---: |
| plain/16 | 0.615 / 0.621 | 0.622 / 0.628 |
| plain/64 | 2.216 / 2.255 | 2.279 / 2.246 |
| plain/256 | 8.901 / 9.083 | 9.048 / 9.061 |
| plain/512 | 18.992 / 19.354 | 19.310 / 19.302 |
| first-line/512 | 24.023 / 24.122 | 24.981 / 24.908 |
| dense-4/512 | 1.717 / 1.729 | 1.719 / 1.745 |
| full-4/512 | 7.232 / 7.218 | 7.208 / 7.221 |
| dense-64/7 | 1.856 / 1.843 | 1.864 / 1.859 |
| dense-first-64/7 | 1.914 / 1.901 | 1.914 / 1.920 |

最終ABBAのrun median平均ではC512 normal +0.7%、first-line +3.6%の構築時間差。first-lineのnet保持36.5%減との時間/保持量のトレードオフとして採用する。密な64laneの小さな保持利益や追加gross/peakも隠さず、全条件で高速化とは扱わない。

時間はhost/input/scope固有の観測。旧walkはcutごとに全laneを調べるままで、CPU計算量改善や全入力の高速化は主張しない。疎表lookupは変更行の二分探索を使う。capture/quickの時間は検証buildと重なり因果的速度根拠に使わない。全layout samplesも保存する。

修正前試作のsequential after 1runに約2倍の遅さがあった。別fresh-processでC512 normal19.511/19.758→19.529/19.472msと再現せず、first-line24.459/24.319→25.129/25.209ms（約3%増）。同process perfはuser instructions約4.8%増、minor faults減。font初期化/検証layoutを含むprocess全体でありscope CPUや約2倍変動の原因の証明ではない。この試作時の全samples/perf/記録をrawのprototype_evidenceに残し、最終source測定と区別する。

## 検証と証拠

workspace1,071、既定featureなし618、complex-only621、fmt、全target Clippy、docs（警告error化）、固定glyph snapshots成功。snapshot出力既存directory拒否後、新しい出力先で再実行した。期待画像更新なし。既存base/ancestor/first-line aggregate Items境界回帰を含む。

sbp8と同じfont/input/harness/lock/toolchain/profile/CPU affinityで全54ケースを収集。warm/memory各7digestとsource fingerprint 48fa29dc9309221c0fa55b1982f3866327e3eff84c4b85036084026afef1cf35をrunnerと独立照合scriptで確認。検証Rust1.96.0、性能stable1.97.1。before productionはbaseそのもの。after captureは9389head+保存production diffで最終sourceを記録し、同じsourceを0562e02にcommitした。専用probeの外部依存versionはworkspace lockと一致。

[manifest](data/ruby-cursor-storage.json)と[raw archive](data/ruby-cursor-storage-raw.json.gz)に最終64/54cases、全割当/時間samples、source/config/lock、最終4binary SHA256、9表診断、RED/GREEN・required logs・native ledger・レビュー結果・照合scriptを保存した。初期試作の60/54casesと6binary hashes、isolated/perfも内包する。元成果物はtarget/performance-artifacts/sbp6-final-probe、sbp6-ruby-cursors-final、sbp6-snapshots-final、過去試作sbp6-probe/sbp6-isolated-probeに保存する。
