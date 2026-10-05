# shodo-zb0.5 StyleMetrics probe instance reuse

`StyleMetrics::resolve` now keeps a fixed four-entry, stack-local table of
resolved shaping instances. It reuses an entry only when the complete
`FontMatch` is equal, the font generations are stable, and resolving the entry
did not add a warning. Per-character fallback matching and glyph metric reads
still run for every probe. Warning-producing size-adjust results are resolved
again so duplicate warnings and their order remain unchanged.

## A/B measurement

The baseline is `1d55eb0` (PR #206); the candidate is that commit with this
change. Both builds used Rust 1.97.1, Linux x86_64, the same checked-in Latin,
CJK, and emoji fixture fonts, and separate Cargo target directories. The same
build-only example source was used on both sides. Font loading, style and
builder setup, line breaking, and output hashing were outside the timer and
allocation scope. Each case used 64 child styles with distinct font sizes, 3
warm-up builds, 21 timed builds, and 9 allocation samples.

The fixtures cover Latin, visible CJK fallback (`水`), the variable emoji face
with `wght=700`, and supported `ExHeight` font-size-adjust. The output hashes
include line and paint geometry, selected font IDs, font sizes, horizontal and
vertical metrics, variations, normalized coordinates, source ranges, glyph
IDs, positions, and advances. All baseline and candidate hashes and warning
lists match exactly.

| Fixture | Build time median | Requested bytes | Allocation calls |
| --- | ---: | ---: | ---: |
| Latin, 64 styles | 279,518 → 264,921 ns (−5.2%) | 270,171 → 246,251 (−8.9%) | 1,814 → 1,684 (−7.2%) |
| CJK fallback, 64 styles | 297,050 → 291,951 ns (−1.7%) | 291,220 → 267,300 (−8.2%) | 2,391 → 2,261 (−5.4%) |
| Variable emoji, 64 styles | 346,430 → 337,909 ns (−2.5%) | 309,591 → 291,521 (−5.8%) | 3,024 → 2,764 (−8.6%) |
| Font-size-adjust, 64 styles | 320,099 → 307,246 ns (−4.0%) | 270,171 → 246,251 (−8.9%) | 1,814 → 1,684 (−7.2%) |

Test-only counters on one `resolve` confirm that fallback matching is
unchanged while instance work falls:

| Input | Metric resolves | Instance resolves | Match calls | Skrifa `FontRef` opens |
| --- | ---: | ---: | ---: | ---: |
| Latin, baseline → candidate | 1 → 1 | 3 → 1 | 4 → 4 | 12 → 10 |
| Latin + CJK fallback, baseline → candidate | 1 → 1 | 4 → 2 | 4 → 4 | 11 → 9 |

The CJK counter test warms the water-cluster and primary-match entries before
counting, on both sides. A warning-producing `IcHeight` size-adjust case still
performs all 6 instance resolutions across two styles, and its warning list
matches the uncached sequence, including suppression.

Valgrind 3.25.1 Massif measured a process-wide peak of 866,784 bytes before and
866,736 bytes after (`mem_heap_B + mem_heap_extra_B`, stacks excluded). The
allocator probe's peak extra bytes also stayed equal within every fixture.

The allocation reduction is consistent across all four inputs, timing improves
by 1.7–5.2%, and output is bit-for-bit equivalent in the captured properties;
the bounded cache is retained.

Raw captures:

- Before timing
- After timing
- Before allocations
- After allocations
- Before Massif
- After Massif
