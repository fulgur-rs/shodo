# shodo-im9.8 bidi input copy

`analyze_bidi` now borrows the original text when it contains no preserved
U+2028 line separator. If U+2028 is present, the bidi input still owns a copy
with each separator changed to U+2029. The replacement scalars have the same
UTF-8 length, so paragraph and per-byte level offsets stay unchanged.

## Measurement

The baseline is `b8f2c0e9644c1dd7c775af9b3c5a05b59e35547c`. The candidate
implementation and probe are `b970d8d`. Both used Rust/Cargo 1.96.0, Linux
x86_64, release builds, default limits, and the checked-in Arabic fixture font
(`e9885fbcf3bdbddea94d1679d636a80f7421c6b63632a35c6f884a9ee74067d9`), with
system font discovery disabled. The baseline source tree came from
`git archive` at the baseline commit; the same benchmark example was copied
into it before either capture.

Font setup, input construction, `ParagraphBuilder` creation, and
`LayoutContext` creation were outside measurement. Each timing sample measures
only `ParagraphBuilder::build`; line breaking, output hashing, and warning
collection happen after the timer. Each fixture has 3 warmups and 21 samples
per run. Two timing runs were captured with the execution order reversed for
the second run, for 42 samples per fixture. Allocation used a separate build
with 3 warmups and 9 samples per fixture. Gross requested bytes, allocator
calls, net bytes, and peak extra live bytes are reported independently; they
are not RSS measurements.

The primary fixture is 1,024 repetitions of `سلام لا ` (14,336 UTF-8 bytes).
The separator control inserts U+2028 every 64 repetitions (14,381 bytes).
The `plaintext-neutral-inheritance-controls` fixture preserves LF and repeats
24 lines: a Hebrew line with LRI/PDI controls, a digits-only neutral line, and
a Latin line. Its probe test verifies all 24 lines survive whitespace handling
and each neutral line range contains `123`. The final fixture covers upright
Arabic in vertical writing. The probe hashes paragraph text and mapping, line
ranges and geometry, glyph ranges and levels, glyph positions, and warnings.
Every baseline/candidate output hash and warning hash matches across all four
fixtures in timing and allocation captures.

The commands below were run from the baseline and candidate source trees with
separate target directories. Timing was captured twice; the second run used
the reverse tree order.

```sh
KACHE_CACHE_DIR=/home/mitz/tmp/kache \
  CARGO_TARGET_DIR=/home/mitz/tmp/shodo-im9-8-before-time-target \
  cargo run --release --offline -p shodo-bench \
    --example bidi_input_copy -- time

KACHE_CACHE_DIR=/home/mitz/tmp/kache \
  CARGO_TARGET_DIR=/home/mitz/tmp/shodo-im9-8-before-alloc-target \
  cargo run --release --offline -p shodo-bench --features allocation-counting \
    --example bidi_input_copy -- alloc
```

The candidate commands use `after-time-target` and `after-alloc-target` in
place of the `before` target directory names. Raw captures include both
timing rounds and the allocation samples.

## Timing results

Pooled medians from 42 samples per fixture:

| Fixture | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| `rtl-arabic-long-without-u2028` | 4,921.221 µs | 4,845.754 µs | −1.53% |
| `rtl-arabic-long-with-u2028` | 4,607.863 µs | 4,507.915 µs | −2.17% |
| `plaintext-neutral-inheritance-controls` | 156.557 µs | 157.954 µs | +0.89% |
| `vertical-upright-arabic` | 296.142 µs | 291.672 µs | −1.51% |

The timing differences are small and do not establish a reliable wall-clock
change; the allocation reduction is the measurable result of this change.

## Allocation results

Median requested allocations from 9 samples per fixture:

| Fixture | Gross bytes | Allocator calls | Net bytes | Peak extra bytes |
| --- | ---: | ---: | ---: | ---: |
| `rtl-arabic-long-without-u2028` | 5,195,594 → 5,181,258 | 2,384 → 2,383 | 2,215,570 → 2,215,570 | 2,215,570 → 2,215,570 |
| `rtl-arabic-long-with-u2028` | 4,155,282 → 4,155,282 | 2,812 → 2,812 | 1,919,082 → 1,919,082 | 1,919,082 → 1,919,082 |
| `plaintext-neutral-inheritance-controls` | 140,732 → 140,557 | 705 → 704 | 64,918 → 64,918 | 66,377 → 66,377 |
| `vertical-upright-arabic` | 508,338 → 508,338 | 261 → 261 | 221,712 → 221,712 | 221,712 → 221,712 |

The long RTL input without U+2028 saves exactly 14,336 gross bytes and one
allocator call, matching its input byte length. The plaintext fixture saves
175 gross bytes and one call, matching its input byte length. Net and peak
live bytes do not change in either fixture. Inputs that need U+2028 replacement
keep the prior allocation behavior.

Raw captures: [before timing](data/shodo-im9-8-bidi-input-before-time.jsonl.gz),
[after timing](data/shodo-im9-8-bidi-input-after-time.jsonl.gz),
[before allocations](data/shodo-im9-8-bidi-input-before-alloc.jsonl.gz),
[after allocations](data/shodo-im9-8-bidi-input-after-alloc.jsonl.gz).
