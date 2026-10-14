# 選択済み edge window の追加保持候補

2026-10-14。baseline は `454fb916a9ae9addd6772417b64986a75717be4c`、候補 branch は `perf/selected-edge-retention`。

通常の glyph snapshot が埋まった後、width summary は候補計測を再利用できても、選択された描画行の edge shaping は残る。`Window` の最終検証済み形状だけを追加保持し、未使用の summary 予約を利用する。

## 保持と再現の条件

- 通常の snapshot は従来どおり 768 KiB、summary と追加 snapshot は合わせて 256 KiB。合計の会計上限は 1 MiB、snapshot は合わせて 2048 件まで。
- clean、未編集、小さな形状だけが対象。途中の probe と `MeasuredWindow` は追加登録しない。`UnretainedClean` は通常の登録で拒否された形状の適格性を保持する。
- source unit 範囲と `Option<u64>` の glyph budget を追加 snapshot のキーにも含める。問い合わせごとの reshape charge は lookup より先に発生し、警告・suppression・saturation の処理順を変えない。
- summary の予定容量と追加 snapshot が 256 KiB を超える際は追加 table 全体を解放する。通常 snapshot の登録が件数上限へ達する場合も追加 table を先に解放し、通常の登録条件と resident を維持する。table capacity も解放し、削除済み entry の会計から table の slack が漏れないようにする。
- owner 変更・`clear` は追加 table も解放する。描画出力は owned glyph のままであり、cache 内の vector を変更しない。

## 検証

新しい回帰テストは、通常 snapshot の保持が満杯の状態で `ffi` の ligature と Arabic の cursive edge を二回描画する。baseline では `ffi` の二回目で shape 累計が 1 から 2 になり、期待した保持に失敗した。候補では同じ overlay 全体（glyph ID、advance、pen、offset、cluster、flags、run、text、changes）、reshape charge、警告、saturation を保ち、二回目の shape 呼び出しをなくす。reshape/glyph budget が `None` の場合も検証する。

追加テストは summary 増加、通常 entry 増加、glyph budget の異なるキー、重複登録、owner 変更、table capacity と Arc の解放を確認する。各段階で通常側・summary 側の会計と合計上限を確認する。

実際の long Arabic regression fixture は、固定 420 px の初回・反復に加え 240 / 480 / 320 px の幅変更・反復を新しい context と比較し、出力・警告が一致した。最終版の shape 呼び出し数は次のとおり。計測 workload ではなく、既存の回帰 fixture を使った値である。

| 幅 px | 新しい context | 幅変更時 | 反復時 |
| --- | ---: | ---: | ---: |
| 420 | 7426 | — | 1657 |
| 240 | 10321 | 10798 | 2750 |
| 480 | 5902 | 6835 | 1898 |
| 320 | 7943 | 8744 | 2482 |

固定幅の baseline は初回 7426、反復 1869 で、候補は反復の呼び出しを 11.34% 減らした。途中版の反復値 1587 は、厳密な summary 予約の再検証前の値なので、最終版の値として使用しない。固定幅の最終 cache は通常 snapshot 577 件、追加 snapshot 141 件 / 223062 B、summary 2301 件、合計 1047782 B。幅変更後の追加保持は 113 / 82 / 48 件に減った。

検査条件は stable Rust、`RUSTC_WRAPPER=`、build jobs 2、CPU 5–6、専用 `~/tmp` target、runner `env`。`cargo test -p shodo --lib line:: -- --nocapture` は最終版で 222 passed / 0 failed。windows 38 件、replay 13 件、短段落 owner 切替、長文の固定幅・幅変更を含む。全体 lib テストも開始したが、中央の全 workspace / 最終 head CI に集約する指示により、984 件予定の途中で中断した（exit 130、完了集計なし）。全体の成功とは扱わない。

`cargo fmt --all -- --check` と `cargo clippy -p shodo --all-targets --all-features -- -D warnings` はともに成功した。中央で組み合わせた最終 head の全体検証は別途実行する。

## 制約と採否

追加保持時には snapshot の clone と Arc/table の allocation が発生し、cold layout では費用が増える。summary が伸びる幅変更では追加 table をまとめて解放するため、以前の幅の描画 snapshot を失う。空き容量がなくなった後は新しい追加 window を拒否する。LRU で後続 window を継続的に追い出す方針は採用していない。

この候補の採否は中央の paired timing・allocation・出力検証で決める。形状呼び出し数の削減だけを実行時間の改善とはみなさない。中央記録は `performance-next-2026-10-14.json`。

## 中央の測定と統合検証

固定された baseline `454fb916` と個別実装の反復 A/B は、[正式な記録](performance-edge-ab-2026-10-14.json.gz) に保存した。条件ごとの全サンプル、出力ハッシュ、別バイナリの割当量、同一バイナリ対照、再測定と制約を含む。初回の mutable baseline を使った測定は指紋を保証できず却下し、固定 worktree で測り直した。

4変更を組み合わせた版では、workspace 1952 / AccessKit 1968 / 最小 feature 1412 / complex-scripts 1415 / allocation-counting 25 件が成功、失敗0。fmt・通常/AccessKit Clippy・通常/AccessKit docs・Wasm3構成も exit0。並行して入った main の変更との統合と、最終HEADのCIは別の確認として扱う。

全サンプルとハーネスは gzip 圧縮した JSON に保存している。`gzip -dc docs/records/performance-*-ab-2026-10-14.json.gz` で取り出せる。

固定 baseline の代表中央値（ms、scale は入力の反復数）。個別変更の割合は足し合わせない。

| 操作・入力 | 変更前 | 変更後 | 比率 |
| --- | ---: | ---: | ---: |
| layout → layout, latin-long ×64 | 14.070 | 12.795 | 0.909 |
| layout → layout, arabic-long ×64 | 31.694 | 29.250 | 0.923 |
| widths → widths, arabic-long ×8 | 7.675 | 5.981 | 0.779 |
| cold → cold, arabic-long ×64 | 80.407 | 80.647 | 1.003 |

## 最新 main への統合

並行 PR #324 (`4ee1c97`) は `EdgeShapeCache.cost` を通常 glyph の会計だけに変更した。追加保持は `selected_cost` のみに計上し、summary と合わせた 256 KiB を実確保量で検査する。通常 glyph の 768 KiB、合計 1 MiB、snapshot 2048 件の条件を維持する。

通常登録の予約を追加保持が減らさないテストを加えた。単純統合では最初の登録後の会計が期待 26032 B に対して 287680 B となり失敗し、修正後は比較に通過した。最初のコンパイルは内部 `CandidateMeasurement` の署名変更が旧テストに残って失敗し、署名を修正してから実際の RED を確認した。4変更を組み合わせた `line::` は 228 passed / 0 failed。独立レビューも Critical / Important 指摘なし。固定 baseline の個別 A/B は統合前リビジョンの記録であり、最新 main との時間・割当比較は別途保存する。
