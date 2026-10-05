# shodo-im9.7 combined width trial shaping

The baseline validates each eligible vertical-combine group by shaping the
plain and width-feature candidates, then shapes the paragraph again. The
focused test measured three HarfBuzz calls for one complete `hwid` group. It
also estimated 152 bytes of item structs, scalar payloads, and contexts copied
by `group.to_vec()`, and measured 592 capacity bytes in the two trial
`GlyphStore`s and their run vectors.

## Measurement

The baseline starts at `3409b7a743cf6dcbb9b6024665e3c34178545ac2` (PR #191
merge); its probe and capture code is commit `822bff4`. The candidate includes
the streaming implementation from `b3e2a63`. Both used Rust and Cargo 1.96.0,
Linux x86_64, the release profile, default limits, the checked-in CJK fixture
(`d8b52a1ddcb511adc93bce9b2caff2d7dcadba609c3bfe25ade99343b7a5aa8b`), and
disabled system font discovery. `twid` and `qwid` cases use copies of that
fixture with its `hwid` GSUB feature record renamed. Each build used 64 inline
groups; setup, font registration, output hashing, and warning collection were
outside the timed or allocation-measured build scope. There were three warmup
builds, 21 timing samples, and 9 allocation samples per fixture. Both builds
used `CARGO_TARGET_DIR=~/tmp/cargo-target`.

The timing median is the middle of 21 `ParagraphBuilder::build` durations.
Allocation medians are requested Rust allocation bytes (`gross`), net bytes,
peak extra live bytes, and allocator call count, each measured over the build.
The allocator figures do not represent process RSS. The probe checks stable
output and warning hashes for all samples within each mode. Every candidate
fixture's output and warning hashes match its baseline hashes in both timing
and allocation captures.

## Timing results

| Fixture | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| `hwid-two-complete` | 375.973 µs | 316.047 µs | −15.9% |
| `twid-three-complete` | 419.138 µs | 355.021 µs | −15.3% |
| `qwid-four-complete` | 462.231 µs | 393.225 µs | −14.9% |
| `hwid-two-partial` | 383.796 µs | 330.017 µs | −14.0% |
| `hwid-two-explicitly-disabled` | 368.850 µs | 325.337 µs | −11.8% |
| `horizontal-control` | 108.608 µs | 109.307 µs | +0.6% |

The horizontal timing difference is within noise for one 21-sample run. The
complete, partial, and explicitly-disabled vertical fixtures all reduced
shaping work by reusing the selected trial result.

## Allocation results

Each field is reported independently as `baseline / candidate` median values.

| Fixture | Gross bytes | Net bytes | Peak extra bytes | Allocator calls |
| --- | ---: | ---: | ---: | ---: |
| `hwid-two-complete` | 364,246 / 315,358 | 94,074 / 94,122 | 109,697 / 109,745 | 3,436 / 2,853 |
| `twid-three-complete` | 400,982 / 350,302 | 113,082 / 113,130 | 128,705 / 128,753 | 3,446 / 2,863 |
| `qwid-four-complete` | 439,254 / 384,734 | 130,042 / 130,090 | 146,689 / 146,737 | 3,576 / 2,929 |
| `hwid-two-partial` | 365,318 / 322,558 | 93,866 / 93,898 | 109,489 / 109,521 | 3,435 / 2,979 |
| `hwid-two-explicitly-disabled` | 370,390 / 322,510 | 94,023 / 94,055 | 109,646 / 109,678 | 3,500 / 2,980 |
| `horizontal-control` | 192,666 / 192,698 | 70,514 / 70,546 | 86,137 / 86,169 | 233 / 233 |

Gross requested bytes fell 11.7–13.4% across vertical fixtures. Net and peak
extra live bytes rose by 32–48 bytes per fixture; the probe does not isolate
temporary trial storage from retained paragraph output. The horizontal
control's allocation medians changed by 32 bytes, with the allocator call
count unchanged.

The focused call counter measured three calls per complete eligible group
before the change and two after it. A tight paragraph glyph cap still reports
the same `LimitExceeded.actual`; Ruby base-scoped glyph caps retain the regular
trial and final-shaping path. The implementation does not retain trial stores
for multiple groups at once.

Raw captures: before timing,
after timing,
before allocations,
after allocations.
