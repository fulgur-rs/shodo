# shodo-im9.1 grapheme cursor measurements

Baseline commit: `9c205833a848c5860fcd825aafd5bc71d072f5c2`.

The probe builds `"a".repeat(N)` with the pinned `latin-short` fixture style and
`Shodo Fixture Latin` font from `dev/fixtures`. Limits and layout options use
their defaults. Each cold timing is one fresh process. Fifteen baseline/cursor
pairs were run with their order alternated; raw timings are in
[`data/shodo-im9-1-grapheme-cursors-samples.csv`](data/shodo-im9-1-grapheme-cursors-samples.csv)
and the table reports each version's median. `build` and `all_lines` are timed
separately. The allocation run
measures the `build` scope with the bench `CountingAllocator`; these are
requested Rust allocator blocks, not RSS.

## Cursor comparison counts

The regression tests count offset comparisons and verify exact ASCII
boundaries/grapheme starts for 1K, 4K, and 16K scalars. The baseline binary
search counts below are from the failing 1K RED run; post-change counts are
from the passing tests.

| Scalars | Shared cut to scalar index, baseline → cursor | Paragraph grapheme start, baseline → cursor |
| ---: | ---: | ---: |
| 1,024 | 11,253 → 2,046 | 12,288 → 3,071 |
| 4,096 | — → 8,190 | — → 12,287 |
| 16,384 | — → 32,766 | — → 49,151 |

The cursor counts scale linearly across the tested sizes. The counters compile
only in tests.

## Release build and layout timings

| Scalars | Build baseline | Build cursor | Change | All-lines baseline | All-lines cursor | Change |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,024 | 0.822 ms | 0.781 ms | −5.1% | 0.135 ms | 0.129 ms | −4.4% |
| 4,096 | 2.885 ms | 2.814 ms | −2.5% | 0.513 ms | 0.495 ms | −3.5% |
| 16,384 | 11.412 ms | 10.981 ms | −3.8% | 1.979 ms | 2.005 ms | +1.3% |

All three output digests match between versions, with `synthetic_glyphs = 0`:

| Scalars | Output SHA-256 |
| ---: | --- |
| 1,024 | `7a9a43f3cd5b515680e4b55d5f48e7c9a02af2777ebe2a5cd4b7aa7eae022234` |
| 4,096 | `7debff63c2c99a3dcb0046ef17ea3917519f34a3e831c080d2ffccf86da9955d` |
| 16,384 | `83bbd22269fd23d6da4dffadfdfee2b11ee9b1e360538bce01abb0bf4cdacde0` |

The build window improves at each size. All-lines is reported separately; its
measurement follows build in the probe, so its timing can reflect warmed
process state as well as line work and is not attributed to this itemization
change.

## Build allocation scope

The baseline and cursor runs have identical allocation counts for every size.

| Scalars | Calls | Gross bytes | Freed bytes | Net bytes | Peak extra bytes |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,024 | 423 | 754,354 | 414,565 | 339,789 | 345,651 |
| 4,096 | 461 | 2,840,242 | 1,548,133 | 1,292,109 | 1,322,547 |
| 16,384 | 499 | 11,183,794 | 6,082,405 | 5,101,389 | 5,230,131 |

Reproduce the timing and allocation probes with:

```sh
cargo run --release -p shodo-bench --bin shodo-probe -- --cold itemize-graphemes 1024
cargo run --release -p shodo-bench --bin shodo-probe --features allocation-counting -- --memory itemize-graphemes 1024
```

Use `4096` or `16384` for the other input sizes.
