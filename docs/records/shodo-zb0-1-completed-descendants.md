# shodo-zb0.1 completed descendants の再集計調査

2026年10月3日。crates/shodo/src/ruby/measure.rs::candidate_inner は、各 base 幅、whole_area、has_content の計算で completed fragments を別々に走査する。base ruby が深い場合、同じ fragment を祖先ごとに再処理する。

completed fragments を source 順に一度走査し、そのパスから base 幅の飽和加算、whole_area、has_content を同時に集約する候補を測定した。base 幅への加算は従来どおり fragment ごとに LayoutUnit::add し、source 順を保つ。clipped continuation の条件も維持する。候補の正確な差分は raw archive の candidate.patch に保存した。測定後、性能効果を確認できなかったため本体実装と test-only counter は戻し、最適化は採用していない。

## 固定フォントの A/B

baseline は 17a6904。baseline と候補で同じ benchmark example、Rust 1.96.0、release profile、default limits、fixture font を使った。候補は baseline に一時的な差分を適用して測定した。CPU affinity は CPU 0 に固定した。

15条件は以下の組み合わせで構成する。

- base depth 0/1/2/4、各8列
- annotation depth 0/1/2/4、各4列
- 1/4/16/64列、base depth 2
- short/medium/long continuation。各3/18/48文字、幅48/72/96、base depth 1、annotation depth 1

時間と allocation の対象は ParagraphBuilder::build と Paragraph::break_all。fixture 構築、font load、warnings の取得、output hash は計測範囲外。各条件は3回 warm-up 後に11 samplesを取る。allocation は別 binary で測り、allocation-counting が数える Rust allocation bytes/calls と scope 終了時の retained values を比較する。RSSの測定ではない。

line output は既存 geometry_snapshot でSHA256化する。text/source range、offset mapping、glyph ID/cluster/position、line geometry、ruby ranges/nodes/transforms などを含み、build/layout warnings も個別に照合する。全 timed sample で baseline と候補の hash、warnings、line count が一致した。各条件で max_shaped_glyphs=1 の制限結果も一致した。

## 結果と判断

CPU 0 固定の per-case timing は8組の before/candidate process captureで比較した。各 process の11 samplesから case ごとのmedianを取り、paired changeの中央値を集計すると **+0.19%**、候補が速かったのは **120比較中55**。ケース間・run間のばらつきも大きく、例えば base-depth-4 のrun差は **−10.3%から+78.9%** に広がった。基準例の追加 unpinned capture も archive に残すが、この集計には混ぜていない。

補助の16組の whole-process perf stat -e task-clock では、候補の中央値は **+0.98%**、候補が速かったのは **16組中1組**。この値は benchmark setup と制限結果確認も含む。どちらの計測からも、繰り返し確認できる速度向上は得られなかった。

全15条件の allocation calls、gross allocated bytes、deallocated bytes、live bytes、peak extra bytes、net bytes はサンプルごとに同一だった。測った範囲では割当削減も retained-memory 削減もない。

この計測では一括走査を採用する根拠が得られなかったため、candidate_inner は baseline のままにした。候補の実験差分は再検証用に保存し、benchmark example は将来同じ workload を測り直すために残す。

## 再現データ

[manifest](data/shodo-zb0-1-completed-descendants.json) に条件と集計を保存した。[raw archive](data/shodo-zb0-1-completed-descendants-raw.tar.gz) は全 timing/allocation JSONL、CPU task-clock CSV、candidate patch を含む。archive は54 files、59,309 bytes、SHA256 ed72a88365d0901853aa54645374c48b2bdf394c35f5e4fba5ff95704c5e45d7。harness のSHA256はmanifestに記録した。

measurement harness は [ruby_completed_descendants.rs](../../dev/bench/examples/ruby_completed_descendants.rs)。baseline で時間を再測定するコマンド:

    cargo run --locked --release -p shodo-bench --example ruby_completed_descendants -- time

allocation mode:

    cargo run --locked --release -p shodo-bench --features allocation-counting --example ruby_completed_descendants -- alloc

candidate を再測定する場合は raw archive の candidate.patch を baseline 17a6904 に適用し、baseline と候補を別々の CARGO_TARGET_DIR でbuildする。
