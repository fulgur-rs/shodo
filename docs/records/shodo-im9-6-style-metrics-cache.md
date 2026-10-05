# shodo-im9.6 paragraph style metric cache

Paragraph construction now resolves identical font metric inputs once per
paragraph build. Paint remains attached to each original style. Cache entries
hold an index into the returned metrics vector, so they do not retain a second
copy of every `StyleMetrics`. Cache misses use one hash-map entry lookup and
only clean, generation-stable results are retained.

## Measurement

The baseline is `1ceab3cceb9aca955481c8df04815c1265cc9884`; the candidate is
implementation commit `f90d463` (the benchmark record is committed separately).
Both used Rust 1.96.0, Linux x86_64, the release profile, default limits, and
the checked-in fixture fonts with system discovery disabled. The same benchmark
source was copied into the baseline worktree. Font loading, builder/style setup,
and `LayoutContext`
creation were outside the measurement scope. Each sample timed only
`ParagraphBuilder::build`; line breaking, paint/caret/selection hashing, and
warning serialization ran after the timer and allocation scope.

Each fixture had three warm-up builds and 21 timing samples. Allocation
measurements used a separate `allocation-counting` build with 9 samples. The
allocator reports requested Rust allocation blocks, not RSS. Baseline and
candidate used separate Cargo target directories.

The five fixtures use one child span per style. `paint-only-*` child styles
share the root's metric inputs and have unique colors. `distinct-*` child
styles each have a distinct font size and unique color; the root adds one more
metric key. Each character is `a`, and the line width keeps the fixture on one
line. Output hashes cover paint ranges/colors, line and paint geometry, every
caret stop, each character's selection rectangles, and warnings. All paired
paint, geometry, and warning hashes and warning lists match exactly.

The capture commands were run in the baseline and candidate worktrees with a
different `CARGO_TARGET_DIR` for each:

```sh
KACHE_CACHE_DIR=/tmp/shodo-im9-6-kache \
  CARGO_TARGET_DIR=/tmp/shodo-im9-6-before-time \
  cargo run --release -p shodo-bench --example style_metrics

KACHE_CACHE_DIR=/tmp/shodo-im9-6-kache \
  CARGO_TARGET_DIR=/tmp/shodo-im9-6-before-alloc \
  cargo run --release -p shodo-bench --features allocation-counting \
    --example style_metrics -- alloc
```

The candidate commands use the same invocations with `after-time` and
`after-alloc` target directories. The timing medians below use the paired
21-sample captures. Allocation values are median requested bytes and calls.

## Timing results

| Fixture | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| `paint-only-1` | 33.80 µs | 21.51 µs | −36.4% |
| `paint-only-64` | 323.94 µs | 113.71 µs | −64.9% |
| `paint-only-1024` | 4,868.38 µs | 1,607.13 µs | −67.0% |
| `distinct-64` | 259.33 µs | 275.96 µs | +6.4% |
| `distinct-1024` | 4,101.21 µs | 4,289.37 µs | +4.6% |

An initial unoptimized candidate timing run showed about a 3× regression on
all-distinct styles. A second run reduced this to about 8%, so the first result
was not repeatable. The cache miss path was then changed to hash once and store
metrics by vector index. The table pairs the repeated baseline capture with a
fresh capture from the final candidate commit. The remaining all-distinct cost
is about 16.6 µs for 64 child styles and 188 µs for 1,024.

## Allocation results

Median `allocated_bytes / calls` during `ParagraphBuilder::build`:

| Fixture | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| `paint-only-1` | 14,544 / 131 | 13,773 / 118 | −5.3% bytes / −9.9% calls |
| `paint-only-64` | 228,435 / 1,434 | 167,751 / 539 | −26.6% bytes / −62.4% calls |
| `paint-only-1024` | 3,522,771 / 19,775 | 2,549,127 / 5,440 | −27.6% bytes / −72.5% calls |
| `distinct-64` | 257,727 / 1,808 | 268,155 / 1,814 | +4.0% bytes / +0.3% calls |
| `distinct-1024` | 3,997,055 / 25,904 | 4,164,987 / 25,914 | +4.2% bytes / +0.04% calls |

Peak extra and net retained bytes were unchanged for each paired fixture.

Raw JSONL captures: before time,
after time,
before allocations,
after allocations.
