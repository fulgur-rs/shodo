# shodo-zb0.6 first-line whitespace flags の共有

first-line の normal pass と alternate pass は同じ raw text/items、annotation 条件、`white-space-collapse` を使う。`ProcessInput` から flags を一度生成し、読み取り専用 slice として両 pass に渡す。`Processed`、`Processor`、`BaseScopes` は pass ごとに独立させた。alternate pass がない場合は従来どおりその場で flags を生成する。

## 固定フォント A/B

baseline は PR #207 の merge commit `79ace5d`、候補はその commit にこの変更を加えたもの。両方とも Rust 1.96.0、Linux x86_64、release profile、同じ `shodo-fixtures` 固定フォントと同一 benchmark source で測定した。font load、builder 構築、line breaking、warning 整形、output snapshot/hash は build timer と allocation scope の外。timer は `ParagraphBuilder::build` のみを囲む。各 fixture は3回 warm-up 後21回測定し、allocation は9回測定した。

| Fixture | Build time median | Requested bytes / calls | Build peak-extra bytes |
| --- | ---: | ---: | ---: |
| Small first-line | 299,634 → 294,744 ns (−1.6%) | 285,474 → 285,194 / 1,012 → 1,009 | 140,956 → 140,956 |
| Small, no first-line control | 158,897 → 158,548 ns (−0.2%) | 138,701 → 138,701 / 503 → 503 | 66,357 → 66,357 |
| Preserved whitespace, first-line | 411,525 → 410,687 ns (−0.2%) | 451,906 → 451,602 / 1,489 → 1,486 | 227,507 → 227,507 |
| Large first-line (4,096 segments) | 45,375,499 → 44,858,372 ns (−1.1%) | 45,711,298 → 45,670,298 / 99,017 → 99,014 | 23,444,652 → 23,444,652 |
| Ruby annotation first-line | 406,286 → 409,496 ns (+0.8%) | 314,112 → 313,797 / 1,797 → 1,791 | 153,571 → 153,571 |

Build wall time is mixed and small, so no consistent speedup is claimed. The allocation scope shows a repeatable reduction for every first-line fixture, including 41,000 requested bytes and three calls for the large input; the no-first-line control is unchanged. The test-only generation counter confirms one parent flags buffer for first-line and one for no-first-line; Ruby's parent and annotation contexts each generate their own buffer once. Per-pass text/items/mapping/source spans/width origins/indivisible data compare equal when supplied flags are used; text and item limit rejections retain the same kind, limit and actual count.

Every fixture's before/after output SHA-256, build warnings and layout warnings match. The output snapshot includes line geometry and glyph/ruby data. The full allocator scope's `peak_extra_bytes` is identical in every case.

## Massif

Valgrind 3.25.1 Massif ran one build per fixture with `--time-unit=B --stacks=no --detailed-freq=1`. Useful heap peak was identical in each before/after pair. Process-wide total peak (`mem_heap_B + mem_heap_extra_B`) was:

| Fixture | Before → after |
| --- | ---: |
| Small first-line | 1,094,976 → 1,094,928 B |
| Small, no first-line | 1,049,200 → 1,049,200 B |
| Preserved whitespace, first-line | 1,523,752 → 1,524,056 B |
| Large first-line | 128,104,312 → 128,104,296 B |
| Ruby annotation first-line | 1,139,104 → 1,139,088 B |

The largest difference is 304 bytes of allocator extra heap; useful heap is unchanged. Keeping the single flags buffer alive across both passes did not raise the measured peak.

The optimization is retained for its measured allocation reduction with equal output and no peak increase. Timing alone is not conclusive.

Raw samples, Massif files, output hashes, toolchain and binary hashes are in [the capture archive](data/shodo-zb0-6-whitespace-flags.tar.gz). The benchmark source is `dev/bench/examples/whitespace_flags.rs`.
