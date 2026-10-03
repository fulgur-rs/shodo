# shodo-im9.4 paint and hit geometry

`LineGeometry` now consumes an internal `GeometryCluster` iterator. It gets
the glyph range from the existing Shared or Overlay cluster records, visits
each glyph once to accumulate laid-out and shaping advances, and carries the
first glyph ID for GDEF caret lookup. It no longer collects positioned glyphs
into a temporary `Vec` or searches that vector twice per cluster. Combined
clusters use their glyph range directly; empty Combined clusters retain the
previous text-start lookup.

The public `GlyphRunView::clusters()` path keeps its source classification and
values. Both public and geometry iterators use the same private cluster-range
enumerator.

## Measurement

The baseline is `8738b4e39d2c3e516975900053f41d3c3d4aa4ea`. Both builds used
Rust 1.96.0, Linux x86_64, the release profile, default limits, and the checked
in fixture fonts with system font discovery disabled. Paragraph construction
and line breaking happen before the measurement scopes.

`paint_spans` collection and `LineLayout::new` are measured separately. Each
timing row is the median of 21 samples. Allocations use a separate
`allocation-counting` build with 9 samples; report allocated bytes and allocator
calls, since the returned layout is retained when its scope ends. Baseline and
candidate use separate Cargo target directories. Paint hashes, line geometry
hashes, caret and selection hashes, and warnings match exactly across all
workloads and samples.

The captures set `SHODO_GEOMETRY_SAMPLES=21` for timings and
`SHODO_GEOMETRY_SAMPLES=9` with `allocation-counting` for allocation runs. For
example, run these commands from the workspace root in each revision, using a
separate `CARGO_TARGET_DIR` for baseline and candidate:

```sh
SHODO_GEOMETRY_SAMPLES=21 cargo run --release -p shodo-bench --example paint_geometry
SHODO_GEOMETRY_SAMPLES=9 cargo run --release -p shodo-bench --features allocation-counting --example paint_geometry
```

The cases include the original Latin, Arabic, combining, nested atomic and tab
workloads, plus a single glyph, combining marks, an `ffi` ligature, RTL text,
letter and word spacing, justification, a selected soft-hyphen overlay, ruby
with positive caret padding, vertical text-combine, one 1,024-glyph run, and
256 short runs. The `ffi` fixture shapes to one glyph. Unit coverage confirms
the overlay and positive ruby-padding cases, exercises Shared/Overlay/Combined
cluster ranges, and checks that hit geometry does not invoke the public
Cluster iterator.

## Timing results

Times are medians in microseconds. Lower is better.

| Workload | Operation | Baseline | Candidate | Change |
| --- | --- | ---: | ---: | ---: |
| `latin-long` (7,672 glyphs, 144 lines) | `paint_spans` | 1,378.0 | 1,110.8 | −19.4% |
| `latin-long` | `LineLayout::new` | 1,609.8 | 1,362.7 | −15.4% |
| `arabic-long` (6,040 glyphs, 97 lines) | `paint_spans` | 1,031.7 | 840.9 | −18.5% |
| `arabic-long` | `LineLayout::new` | 1,274.2 | 1,065.7 | −16.4% |
| `long-run` (1,024 glyphs) | `paint_spans` | 145.4 | 109.4 | −24.8% |
| `long-run` | `LineLayout::new` | 187.2 | 146.2 | −21.9% |
| `many-short-runs` (256 runs) | `paint_spans` | 67.7 | 61.0 | −9.8% |
| `many-short-runs` | `LineLayout::new` | 51.0 | 46.7 | −8.4% |
| `justification` | `LineLayout::new` | 92.8 | 79.2 | −14.7% |
| `soft-hyphen-overlay` | `LineLayout::new` | 2.864 | 2.794 | −2.4% |

The single-glyph and ruby-padding cases are too small for a meaningful timing
difference in this run. The ligature and vertical-combine cases remain within
the same few-microsecond range.

## Allocation results

Median `allocated_bytes / calls` for `LineLayout::new`:

| Workload | Baseline | Candidate | Difference |
| --- | ---: | ---: | ---: |
| `latin-long` | 5,631,712 / 4,275 | 5,478,272 / 4,035 | −153,440 bytes / −240 calls |
| `long-run` | 830,968 / 56 | 810,488 / 55 | −20,480 bytes / −1 call |
| `many-short-runs` | 218,616 / 300 | 198,136 / 44 | −20,480 bytes / −256 calls |

The three cases also reduce `paint_spans` allocation counts. Net retained bytes
and measured peak extra bytes are unchanged, as expected for temporary glyph
vectors that are dropped before geometry construction returns.

Raw JSONL captures: [before time](data/shodo-im9-4-paint-geometry-before-time.jsonl.gz),
[after time](data/shodo-im9-4-paint-geometry-after-time.jsonl.gz),
[before allocations](data/shodo-im9-4-paint-geometry-before-alloc.jsonl.gz),
[after allocations](data/shodo-im9-4-paint-geometry-after-alloc.jsonl.gz).
