# shodo-zb0.3: Ruby base caret の範囲借用

`AnnotationIndex` が親行の caret を `Vec<Caret>` に複製していた処理を、親行と stop 範囲の保存に置き換えた。Ruby hit 時に親 `LineLayout` の stop 範囲を借りる。非表示 annotation は範囲計算前に除外する。範囲の端点規則（`offset < start` を飛ばし、`offset <= end` まで含める）、距離計算、affinity tie-break は維持した。

## 固定フォント A/B

baseline は `729da9da81d35c70e9fc8e0b30d4088c64a9ce82`、候補は `perf/shodo-zb0-3-caret-slice`。Rust 1.96.0 の release build で、同じ固定 Latin font と同じベンチを使い、baseline/candidate それぞれで通常順と逆順を1 runずつ測った。各 run は1 sampleにつき128回 `LineLayout::new` を計測し、9 samples の median を記録する。各呼び出しの elapsed を読み取ってから layout を drop する。font load、fixture 構築、drop、allocation counting は時間計測外。全ケースで base は128文字。可視 depth 1/4/8 と、可視 Ruby が0になる hidden depth 4 を測った。raw samples は [JSON データ](data/shodo-zb0-3-borrowed-base-stops.json) に保存した。

| ケース | baseline median の2 run平均 | 候補 median の2 run平均 | 差分 |
| --- | ---: | ---: | ---: |
| visible depth 1 | 41,865 ns | 41,137 ns | −1.7% |
| visible depth 4 | 112,846 ns | 111,731 ns | −1.0% |
| visible depth 8 | 204,239 ns | 201,614 ns | −1.3% |
| hidden depth 4 | 23,770 ns | 23,496 ns | −1.2% |

各ケースの2 run平均では候補が1.0〜1.7%短かった。時間差は小さく、この run 数から速度向上の大きさまでは断定しない。別 binary の allocation-counting で `LineLayout::new` 1回を測ると、caret 複製分の割り当てが全ケースで減った。

| ケース | allocation calls baseline → 候補 | allocated bytes baseline → 候補 |
| --- | ---: | ---: |
| visible depth 1 | 84 → 83 (−1) | 233,784 → 225,560 (−8,224) |
| visible depth 4 | 216 → 212 (−4) | 598,344 → 565,448 (−32,896) |
| visible depth 8 | 392 → 384 (−8) | 1,084,424 → 1,018,632 (−65,792) |
| hidden depth 4 | 41 → 40 (−1) | 120,456 → 112,264 (−8,192) |

depth 8 では1構築あたり約64 KiBの一時 caret copy をなくした。hidden depth 4 でも、以前は作っていた親 caret copy を1回分除いた。256文字の hidden fixture を8回構築する回帰テストでも allocation calls は baseline の384から候補の376になり、候補の allocated bytes は baseline の2,012,352 bytesを下回る。

## 同値性と制約

`ruby_base_stop_range_includes_exact_closed_endpoints` が開始・終了と空範囲を固定する。`ruby_base_hits_match_owned_closed_slice_and_affinity_ties` は従来の owned closed slice を作り、同一の nearest-caret/affinity comparator で得る `HitResult` と、座標 hit の結果を照合する。端点と in-bounds の中点も調べ、少なくとも1つの tie 点を通す。既存の Ruby hit tests も nested hit と source mapping を検査する。

plain-text control との比較は採用しなかった。Ruby fixture の親行には bidi isolate marker が入り、plain text control と `LineIndex` の allocation 構造が異なるため、差分を caret copy に帰属できない。A/B では同じ Ruby fixture を旧・候補ソースで動かした。

深さ8の256文字 fixture は既定の reshape-window budget warning を発生させるため、測定用には両者で警告のない128文字を使った。隠しケースの allocation regression は深さ4・256文字で別途確認する。

## 再現

時間計測:

    SHODO_RUBY_BASE_CARET_SAMPLES=9 SHODO_RUBY_BASE_CARET_ITERATIONS=128 cargo run --locked --release -p shodo-bench --example ruby_base_caret_range

逆順は同じコマンドに `SHODO_RUBY_BASE_CARET_REVERSE=1` を加える。allocation-counting は別実行で `--features allocation-counting` を加える。baseline と候補は別々の checkout / `CARGO_TARGET_DIR` を使う。

## 判断

時間は両方の測定順で少し短かった。さらに全ての可視深度で割り当て回数と bytes が減り、長文の非表示ケースでも caret 列の複製がなくなった。nearest caret の処理は変更せず、hit test 同値性も通るため、範囲借用の変更を採用する。
