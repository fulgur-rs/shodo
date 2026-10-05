# shodo-zb0.13: missing-font scalars without temporary strings and runs

When a shaping input has no matching face, `crates/shodo/src/shape.rs::shape_inputs`
emits one `.notdef` glyph per scalar. The old branch did this per scalar:

- `scalar.c.to_string()` (one `String` allocation),
- `shape_item(...)`, which pushed the glyph with `id = c` and a new
  `ShapedRun` holding `Arc::new(RunInstance::default())` (one `Arc`
  allocation),
- then overwrote the glyph id with 0, popped the run, replaced its instance
  with the shared `missing_instance`, set its orientation and source end, and
  merged it into the previous run when possible.

So every missing scalar allocated and freed a `String` and an `Arc`, and
built and dropped a `ShapedRun`.

## Change

The branch now calls `push_notdef_glyph`, which pushes one glyph with id 0 at
pen zero. Then the branch either extends the window's previous run or pushes
a new run that uses the shared instance directly. Nothing else changed:

- Order of checks: ruby base scope `bases.item(.., 1)`, then
  `LayoutUnit::from_f32_round(font_size)`, then the global
  `max_shaped_glyphs` check, then the push. This keeps
  `LimitExceeded::actual` and saturation counts. `from_f32_round` is still
  called once per scalar, because the `non_finite` and `saturated` counts are
  formatted into warnings.
- Advance and offset: one em, or zero for marks and default ignorables.
  Marks are shifted back half an em. The cluster is the scalar's source
  offset.
- Merging: the same item, a contiguous source range
  (`previous.text.end == scalar.offset`), and
  `pen + advance + next advance <= RUN_PEN_LIMIT`. Merging only happens
  within the current budget window. The old `previous.font == current.font`
  test is gone: every run that this branch pushes in a window uses
  `fonts.primary_font()`, so the test was always true.
- Run fields: `text = scalar.offset..scalar.end` (the source end, not
  `offset + len_utf8`), plus the input's orientation and the shared
  `missing_instance`.
- Warning: still one "missing font; using .notdef glyphs" per window.

`shape_item` had no other non-test callers and is removed. Its four unit
tests now build a paragraph with an empty `FontCollection`, so they test the
real missing-font path: one em per character, combining-mark offsets, pen
restarts before `RUN_PEN_LIMIT`, and the glyph limit before push. A new test,
`missing_font_glyphs_are_notdef_and_runs_share_one_instance`, checks `.notdef`
ids, a zero-advance ZWJ that lets the second pen-limited run take one extra
glyph, run source ranges, and that all runs share one instance. All five
tests pass on both the old and the new `shape.rs`.

## Probe

`dev/bench/examples/missing_font_scalars.rs` has the same modes as the
zb0.7 probe (`digest`, `time`, `alloc`, `loop <case> <n>`) and the same
digest contents. Runs without a retained face (the empty collection's primary
font) hash a fixed sentinel instead of the face bytes.

| case | fonts | content | notes |
| --- | --- | --- | --- |
| short | none | `abc` | per-call floor |
| latin-64 / latin-1024 | none | Latin sentence ×64 / ×1024 | 2,880 / 46,080 glyphs |
| combining-256 | none | base + combining marks ×256 | zero advance, mark offset |
| ignorable-256 | none | ZWJ/ZWSP/ZWNJ between letters ×256 | zero advance |
| latin-split16 | none | Latin ×64, `max_shaping_run_bytes=16` | one warning per window |
| latin-huge-size | none | Latin ×4 at 1e6 px | `RUN_PEN_LIMIT` restarts, saturation warnings |
| cjk-vertical | none | Han ×64, `vertical-rl` | upright orientation copied to runs |
| mixed-items | fixture | Cherokee and Latin ×64 | missing and real items alternate |
| ruby-base-split6 | fixture | 64 Cherokee ruby bases, base budget 6 | ruby base scopes |
| ruby-base-glyph-limit | fixture | same, base `max_shaped_glyphs=4` | `LimitExceeded` path |

## Equivalence

All 11 digests and warning lists are byte-identical between base (f82e873,
probe only) and new (96c6784). This covers `.notdef` ids, clusters, positions,
advances, offsets and origins, run source ranges and orientation, pen-limit
splits, per-window warnings, saturation warnings and
`LimitExceeded { kind: ShapedGlyphs, limit: 4, actual: 5 }`.

## Allocations (`CountingAllocator`, one build)

| case | calls base → new | gross bytes base → new |
| --- | ---: | ---: |
| short | 81 → 75 | 7,583 → 7,028 |
| latin-64 | 6,004 → 244 | 2,353,070 → 1,820,270 |
| latin-1024 | 92,472 → 312 | 37,570,670 → 29,045,870 |
| combining-256 | 4,845 → 237 | 1,603,202 → 1,175,682 |
| ignorable-256 | 3,813 → 229 | 1,269,122 → 936,066 |
| latin-split16 | 6,195 → 435 | 2,399,476 → 1,866,676 |
| latin-huge-size | 538 → 178 | 153,122 → 119,822 |
| cjk-vertical | 2,269 → 221 | 815,632 → 624,144 |
| mixed-items | 3,709 → 2,557 | 918,048 → 810,336 |
| ruby-base-split6 | 12,546 → 11,778 | 2,444,403 → 2,372,595 |
| ruby-base-glyph-limit | 1,449 → 1,441 | 449,094 → 448,346 |

In every case the reduction is exactly two calls per missing scalar: one
`String` and one `Arc<RunInstance>`. For example, latin-1024 has 46,080
scalars and 92,160 fewer calls, and mixed-items has 576 Cherokee scalars and
1,152 fewer calls. Gross bytes drop by about 185 per scalar. Net retained
bytes and peak extra bytes are the same in every case, because the removed
allocations were freed within the same scalar.

## Instructions (callgrind, per build)

`(Ir(loop n=10) − Ir(loop n=0)) / 10`, release profile:

| case | base | new | change |
| --- | ---: | ---: | ---: |
| short | 126,836 | 125,518 | −1.0% |
| latin-64 | 15,864,357 | 14,628,622 | −7.8% |
| latin-1024 | 270,679,933 | 250,928,365 | −7.3% |
| combining-256 | 10,133,138 | 9,141,447 | −9.8% |
| ignorable-256 | 9,335,764 | 8,561,945 | −8.3% |
| latin-split16 | 16,077,145 | 14,873,161 | −7.5% |
| latin-huge-size | 1,076,186 | 998,908 | −7.2% |
| cjk-vertical | 7,906,127 | 7,439,778 | −5.9% |
| mixed-items | 10,523,362 | 10,253,737 | −2.6% |
| ruby-base-split6 | 40,357,477 | 40,252,474 | −0.3% |

The cost is about 430 instructions per missing scalar, in every all-missing
case. Mixed and ruby cases change less because most of their work is real
shaping.

## Wall clock

Pinned to one CPU (`taskset -c 10`). Each batch runs base, new, new, base.
Each cell is the median of 21 builds (ms), two batches:

| case | base | new |
| --- | ---: | ---: |
| latin-64 | 1.303 / 1.294 / 1.305 / 1.296 | 1.155 / 1.147 / 1.150 / 1.160 |
| combining-256 | 0.702 / 0.697 / 0.704 / 0.703 | 0.602 / 0.591 / 0.597 / 0.605 |
| ignorable-256 | 0.648 / 0.646 / 0.652 / 0.645 | 0.557 / 0.556 / 0.553 / 0.562 |
| latin-split16 | 1.122 / 1.115 / 1.132 / 1.124 | 0.981 / 0.984 / 0.985 / 0.995 |
| latin-huge-size | 0.071 / 0.071 / 0.072 / 0.072 | 0.063 / 0.063 / 0.063 / 0.063 |
| cjk-vertical | 0.534 / 0.527 / 0.538 / 0.532 | 0.484 / 0.481 / 0.485 / 0.488 |
| mixed-items | 0.760 / 0.758 / 0.765 / 0.762 | 0.726 / 0.729 / 0.726 / 0.729 |
| ruby-base-split6 | 2.127 / 2.108 / 2.150 / 2.144 | 2.145 / 2.142 / 2.138 / 2.124 |
| latin-1024 | 19.794 / 19.682 / 19.977 / 19.724 | 20.940 / 20.939 / 20.864 / 21.131 |

The medium-sized all-missing cases are 9–15% faster, consistently across
batches. This matches the instruction drop plus the removed allocator calls.
`mixed-items` is about 4% faster. `ruby-base-split6` is within noise.

The latin-1024 row above is slower for new, but this single-build median is
not stable. Three later batches gave base 23.5–35.0 ms and new 19.6–23.3 ms.
Per-process page faults varied from 27,939 to 49,718 for the same binary.
Each build retains about 13 MB and is dominated by first-touch faults, so
latin-1024 was also measured as 50 builds per process
(`perf stat -e task-clock ... loop latin-1024 50`). That gives base
1,241 / 1,238 / 1,273 ms and new 1,141 / 1,148 / 1,145 ms (about −8%). Cycles
are −10% and page faults −7% (137.9k → 128.9k). Only this loop result is
used for latin-1024.

## Decision

Adopted. Output, warnings and resource-limit behavior are unchanged. The
change removes two allocations and about 430 instructions per missing scalar.
On all-missing text the measured wall-clock gain is about 8–15%. This path
only runs as a fallback when no face covers a scalar, and inputs with real
faces are unaffected (ruby-base-split6 is within noise). The win matters for
long uncovered scripts and for builds with no fonts, but not for typical
documents.

## Reproduction

Data: shodo-zb0-13-missing-font-scalars.tar.gz.
It contains the digest, alloc and time JSONL, callgrind totals (`cg.txt`,
`cg.sh`), the latin-1024 `perf stat` runs and binary SHA-256s. Labels: base
f82e873 (probe only), new 96c6784. Toolchain rustc 1.96.0, valgrind 3.25.1,
release profile.

## Verification

- `cargo test --locked --workspace` with `RUSTFLAGS=-D warnings`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- The five missing-font unit tests pass with both the old and the new
  `shape.rs`.
