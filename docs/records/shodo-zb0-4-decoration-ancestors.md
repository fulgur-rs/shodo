# shodo-zb0.4 Ruby base 祖先走査

2026年10月4日。Ruby base の edge 幅計算で祖先 `Vec` と membership `contains` の反復走査を、scalar path と lowest common ancestor による共有祖先走査へ置き換えた。通常の `decoration::width` は変更せず、Close の所有規則、first-line の解決済み style、各 edge の丸め、start から end までの逐次飽和加算順を維持する。

## 固定フォント A/B

baseline は `adf02f0dda2cb41837f371eef5b70b7389e28eea`、候補は `c91a6acbb200a441ae992c3c6c1048729691666a`。Rust 1.96.0 の release binary と同一 workload で比較した。測定入力は固定 Latin font、複数列 Ruby base、深さ32/64の Clone/Slice 混在 inline。baseline と候補の full output snapshots は一致し、build/layout warnings はどちらも空だった。

| 列数 × 深さ | calls baseline → 候補 | gross bytes baseline → 候補 | net / peak-extra |
| --- | ---: | ---: | ---: |
| 8 × 32 | 31,856 → 24,083 (−24.4%) | 3,747,911 → 3,623,543 (−124,368) | 996,332 / 1,155,402 B（同じ） |
| 16 × 64 | 166,213 → 110,932 (−33.3%) | 18,007,247 → 17,122,751 (−884,496) | 3,826,708 / 4,471,698 B（同じ） |

depth64 の4境界・edge queryで parent-link reads は480、legacy Vec＋`contains` oracle の作業カウントは6,592だった。fixture build と font load は allocator scope の外。総割り当て要求は減ったが、scope 終了時の live bytes と peak-extra bytes は変わらない。

時間は run-level median 4回分をさらに集約した参考値。8×32 は3,266,671→3,242,411 ns/layout、16×64 は20,311,015→19,643,353 ns/layout。run間の揺れが大きく、16×64候補の1 runには35,183,472 ns/layoutの外れ値があるため、一定の速度向上は主張しない。全raw samplesと個別run medianはarchiveに保存した。

## Valgrind Massif

Valgrind 3.25.1 で両 release binary を `--tool=massif --time-unit=B --stacks=no --detailed-freq=1`、各ケース1 layoutで実行した。baseline の最大 heap は6,642,416 B（useful 6,577,250 B、extra 65,166 B）、候補は6,642,608 B（useful 6,577,250 B、extra 65,358 B）。差192 Bはextra heapだけで、useful heap peakは同一だった。

両ピークの call tree は `RangeIndex::balance → line::range::width → ruby_base_width` の layout allocations が支配していた。対象の祖先割り当ては gross allocation count で減ったが、別の大きい一時 index が process peak を決めるので、Massif の最大値には減少として現れない。Massif の全snapshot数値、binary hash、実行条件もarchiveに含めた。

## 検証と成果物

最終rebase後に `cargo test --workspace` が成功した。rebase前には workspace feature/accesskit、Clippy、format、no-default-feature、complex-scripts-only、allocation regression も成功。差分は独立reviewで Ready to merge と判定され、Critical/Important 指摘はなかった。

raw measurements、full output snapshots、Massif snapshot series、probe source、manifest
