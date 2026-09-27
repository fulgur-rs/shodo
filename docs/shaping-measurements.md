# S2 fixed-font shaping measurements

Measured on 2026-09-27 with rustc 1.89.0 after the S2 final review fixes, default
features, the checked-in 12-case corpus, and the three pinned font subsets (404,464 bytes).
Font loading/registration happens before the clock starts. Each iteration builds
all paragraphs through the public fixture API and drops those paragraphs.
The reusable context retains only shaping plans/scratch, not the paragraphs.

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 RUSTFLAGS='-D warnings' \
  cargo +1.89.0 run --offline -p shodo-fixtures --example shape_timing
```

| Context state | Time for all 12 paragraph builds |
| --- | ---: |
| First build with a new context | 52.241 ms |
| Retained context, mean of 100 builds | 53.537 ms |
| First build after `shrink_to(0)` | 54.555 ms |

These are a single local debug-profile sample, not release throughput claims or
a statistical comparison. Font-layer shaping data is already populated after the
first build and is shared across contexts; “after shrink” is a cold context,
not a cold font collection. System discovery, font registration, line layout,
rendering and browser comparison are excluded. Re-run the example on the target
application's hardware and release profile before drawing performance conclusions.

## Retention and word-cache decision

The context's font-qualified plan LRU has at most 64 entries. Its key includes the
shaping plan properties and features, plus font identity; resolved variation
coordinates reach the shaper. Tests verify plan Arc reuse, bounded eviction and
survival of an externally retained plan after context shrink. Scratch is reused
through `GlyphBuffer::clear` and conservatively accounted at a power-of-two capacity
estimate. The current shaping-run budget automatically drops oversized scratch.

Explicit `shrink_to(bytes)` drops all plans because harfrust does not expose their
heap size. Scratch and partial-line buffers share the remaining accounted budget;
scratch is preferred when it fits. `shrink_to(0)` drops plans, their container
allocation, scratch and partial-line ownership. Tests create a real font-backed
first-line paragraph and float retry, verify the nonzero aggregate cap, and verify
that dropping paragraph/collection then shrinking releases the document font layer.
Shared paragraph/font allocations are not counted as context buffer bytes.

S2 deliberately adds no word cache. This sample does not show a consistent
context-reuse gain and does not establish a word-cache benefit. A word cache would need to qualify
text, script, language, features, normalized coordinates, font identity/generation,
pre/post context, source clusters and discretionary line edges, while respecting
retention budgets. A future proposal should measure representative application
repetition, hit rates, retained memory and contextual correctness against this
baseline before introducing that additional invalidation and ownership machinery.
