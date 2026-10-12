# SHODO-5: match-cache eviction evaluation

**Decision: keep the production scan.** A bounded lazy recency heap reduces
full-cache insertion work, but the measured default-capacity public warm-hit
case is 11% slower on one thread, repeatedly across all five process pairs.
Its default-capacity thrash benefit is only 7–8% for one/32 query styles and
2.2% for 512 styles. The extra retained memory is 5,168 requested bytes per
full capacity-256 cache, including its two vector headers. A hit-heavy workload
could lose more than it gains. The issue requires considering hit throughput,
contention and retention as well as miss work; this candidate is not adopted.

The production lookup, eviction, query interning and atomic stamps are
unchanged. This change retains standalone public/policy probes, a public
concurrent thrash/catalog regression fixture, and tests of the prototype heap.
The rejected production candidate and all measurement samples are preserved
in the [JSON record](shodo-5-match-cache-2026-10-12.json).

## Conditions and public results

Baseline: `c627fa2bf39ae61f7b713afb3a55845794211512`.
Candidate: direct lower-bound heap at capacities >=256, original scan below
256. Its exact source patch/module and test source are embedded in the JSON;
binary SHA-256 values pin the measured executable. No production candidate
commit was published. The embedded candidate includes the later expanded
collision fixture; its matching tests were separately rerun before recording.

Linux x86_64, AMD Ryzen 5 5600G, performance governor, Rust 1.97.1;
release debug=0/incremental=false, other Cargo defaults, no rustc wrapper.
Single-thread timings use CPU 0; four-thread timings use CPUs 0–3.
Fixed Latin and CJK font paths/hashes are in JSON and the fixture manifest.
Each public operation is `FontCollection::match_cluster`, including query
normalization and handing out/dropping its result. Collection/registration,
input queries/clusters, worker creation and output hashing occur before timing.

Five alternating baseline/candidate pairs with three samples per process give
15 samples per revision/case. Warmup is two full iteration cycles per worker.
Iterations per thread are 2,048 for capacity-256 thrash, 8,192 for large/small
thrash, 32,768 for warm, 65,536 for local; capacity-255 uses 32,640.
All task builds ended before the official timing series. Timing binaries have
no counting allocator; allocation results come from separate builds/scopes.

`warm` fits in the shared cache but exercises the direct-mapped thread cache
with hash collisions; `local` repeats one key. `thrash` exceeds shared capacity.
Keys combine CJK characters and 1/32/512 query styles (weight/style/language).
Large distinct-character cases necessarily include unsupported code points;
they measure negative caching too. Matched-key counts below make this explicit.
Owner-independent complete font selections have identical SHA-256 values in
all ten public cases; benchmark loop checksums also agree. Parallel ns/op is
aggregate elapsed / operations, not individual request latency; raw worker
elapsed values are retained separately.

| Case | Capacity / keys / styles | Threads | Matched keys | Median ns/op, scan → heap | Ratio |
| --- | --- | --- | --- | --- | --- |
| thrash | 256 / 512 / 1 | 1 | 29 | 1407.2 → 1304.2 | 0.9268 |
| thrash | 256 / 512 / 32 | 1 | 512 | 1191.6 → 1099.8 | 0.9229 |
| thrash | 256 / 512 / 512 | 1 | 512 | 2900.7 → 2836.2 | 0.9778 |
| thrash | 4096 / 8192 / 1 | 1 | 32 | 5717.1 → 1570.3 | 0.2747 |
| thrash | 8 / 16 / 1 | 1 | 16 | 784.3 → 783.5 | 0.9990 |
| warm | 256 / 256 / 16 | 1 | 256 | 121.0 → 134.3 | 1.1098 |
| warm | 256 / 256 / 16 | 4 | 256 | 74.2 → 69.4 | 0.9347 |
| local | 256 / 1 / 1 | 1 | 1 | 95.2 → 90.5 | 0.9501 |
| local | 256 / 1 / 1 | 4 | 1 | 38.2 → 29.1 | 0.7633 |
| warm | 255 / 255 / 15 | 4 | 255 | 78.6 → 69.8 | 0.8879 |

The single-thread warm ranges do not overlap: scan 118.9–127.2 ns/op,
heap 132.1–138.1 ns/op. All five warm process-pair medians are slower with the
heap, so the increase is not dismissed as noise. Four-thread warm/local ranges
overlap substantially; their medians show no regression in this series, but do
not establish a general parallel speedup. Capacity-4096 thrash improves 72.5%,
mostly on negative matches; that is not a representative CJK paragraph speedup.

## Eviction work, interning and memory

The benchmark-only heap keeps one `(stamp, slot)` node and one position per
live slot. Hits update only the original atomic stamps. Exclusive insertion
refreshes stale minimum nodes until it finds the exact original LRU, including
lowest-slot tie order and replacements whose stamp wraps downward. On a
cold stream this uses one stamp load and O(log C) heap work per insertion;
a miss after hits to all entries can refresh C stale roots with O(C log C)
work. This is not a worst-case O(log C) guarantee.

The policy probe excludes font lookup, hash matching, locks and query interning.
It compares scan, heap and a second-chance CLOCK-like stamp-observation policy.
CLOCK has different eviction choices; it is not an exact LRU substitute.
Scan/heap selection digests match for all measured cold/mixed cases. Seven
samples are used; 100,000 iterations for capacities <=256, 10,000 at 4096.

| Capacity | Cold ns/eviction: scan / heap / CLOCK | Mixed ns/eviction: scan / heap / CLOCK |
| --- | --- | --- |
| 1 | 1.2 / 3.0 / 2.5 | 2.7 / 5.6 / 3.6 |
| 8 | 10.8 / 20.9 / 2.5 | 16.3 / 35.9 / 3.9 |
| 32 | 20.3 / 29.5 / 2.5 | 25.3 / 58.1 / 4.3 |
| 64 | 30.8 / 34.4 / 2.5 | 36.6 / 66.9 / 3.9 |
| 256 | 115.0 / 43.0 / 2.5 | 117.5 / 88.9 / 3.9 |
| 4096 | 1873.9 / 64.4 / 2.7 | 1871.1 / 125.2 / 8.0 |


The heap is slower than the scan for small capacities in these streams; the
rejected runtime candidate conservatively starts indexing at 256. CLOCK is
fastest in this isolated workload, but has different retention choices and was
not integrated into production or claimed to have preserved public hit costs.
Neither policy-only improvement establishes a public matching improvement.

With capacity 256, 1,000 cold inserts after filling the cache inspect 256,000
stamps with the scan and 1,000 with the heap. An allocation-free test counter
proved the old scan exceeded the <=2,000 budget (RED), then the heap passed.
Query-intern visits are unchanged and measured separately:

| Distinct styles | Scan/heap intern visits | Scan stamp loads | Heap stamp loads |
| --- | --- | --- | --- |
| 1 | 1,000 | 256,000 | 1,000 |
| 64 | 64,000 | 256,000 | 1,000 |
| 256 | 256,000 | 256,000 | 1,000 |
| 512 | 256,999 | 256,000 | 1,000 |

Interning can dominate diverse-style misses; the small 512-style public benefit
is consistent with that remaining work. The diagnostic patch and test source
are recorded so this separation can be reproduced without instrumentation in
production binaries.

Each heap slot requests 16 bytes for the node plus 4 for its position, with
vector capacities rounded by normal Vec growth. Full power-of-two capacities
256/4096 retain another 5,120/81,920 bytes; their two Vec headers add 48 bytes
inside the collection allocation. At 257 entries, allocation can have room for
512 nodes/positions. Storage does not append hit history and generation reset
releases it. No heap-vector allocation occurs for capacities below 256 in the
hybrid candidate, though its 48-byte headers still enlarge the collection.

Three separate alternating allocation pairs and three samples per process
produce nine steady samples per single-thread public case. In every case,
scan and candidate have exactly equal steady allocation calls, byte traffic,
extra peak and zero net bytes per sample. Their retained live bytes differ by
5,168 at capacity 256, 81,968 at 4096, and 48 at capacity 8. Even the local
one-entry cache allocates minimum Vec capacities: another 80 requested bytes
plus 48 headers. First-fill allocations add 14 calls/10,160 allocated bytes
at capacity 256; net extra heap-vector storage is 5,120. The initial scope also
contains cold font matching and a selection output Vec, and excludes the
collection's own allocation, so it is not a pure cache allocation benchmark.
These are requested allocation bytes, not allocator usable size or RSS.

## Warm-hit investigation and lock diagnostic

A task-local diagnostic times LayerCaches read/write acquisition and records
per-thread counts. It runs one local/warm/thrash process for each revision at
one/four threads, including setup, warmup and samples. Warm acquisition counts
are identical: 66,455 reads/288 writes on one thread and 225,779/288 on four.
The writes populate the cache during preparation, rather than warm eviction.
Total read acquisition time is 89,791,360 → 90,097,844 ns on one thread and
342,915,490 → 345,616,135 ns on four, changes of about 0.3%/0.8%. This coarse
proxy does not explain the repeatable 11% uninstrumented serial warm increase.
Parallel thrash counts depend on scheduling, so totals alone cannot prove
reduced contention.

Acquisition timers include uncontended lock operations and timer/recording
overhead, not just blocked waiting; they do not measure lock holding or the
catalog mutex. Instrumented timing outputs are not throughput evidence. Exact
instrumentation patches, raw per-thread values and the script are retained in
JSON. Both source mutations were restored after diagnostic runs.

Generated-code inspection found the original and candidate warm function and
TLS helper inlining differed even though source hit/atomic logic was unchanged.
Outlining insertion (warm median about 132 ns), outlining the full miss tail
(132 ns), and Box storage (131 ns) failed to restore the roughly 121–122 ns
baseline. A baseline with only the same unused field size measured 117 ns,
so structure size alone is not an adequate explanation. TLS inline hints also
failed to remove the increase. The final forced-inline trial had large timing
variation and is exploratory only. These observations suggest code generation
matters, but do not prove the precise cause; no speculative hint, outline or
Box change is adopted. Their samples and modification descriptions are retained.

## Verification and reproduction

Final retained change: workspace tests **1,788 passed, 0 failed, 9 ignored**;
prototype example tests **2 passed**; workspace/all-target Clippy and
allocation-feature example Clippy pass with warnings denied. The public test
covers capacities 0/1/8/255/256/257, four threads, 512 style keys, supported and
unsupported clusters, and catalog changes between two registered generic
families. The prototype tests compare every selected victim against an
independent scan across 1/8/255/256/257/4096, delayed atomic hits, downward
replacement stamps and equal-age ties; retained heap sizes remain bounded.
The historical integrated candidate also passed 1,792 workspace tests, plus
existing allocation regressions, before adoption was rejected.

Run the retained examples with separate timing/allocation builds:

```sh
export TMPDIR="$HOME/tmp" RUSTC_WRAPPER=""
export CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_INCREMENTAL=false
cargo +stable test -p shodo-bench --example match_eviction_algorithms
cargo +stable run --release -p shodo-bench --example match_eviction_algorithms -- heap 256 mixed 100000 7
cargo +stable run --release -p shodo-bench --example match_cache_eviction -- warm 256 256 4 32768 2 3 16
cargo +stable run --release -p shodo-bench --features allocation-counting --example match_cache_eviction -- thrash 256 512 1 2048 2 3 32
```

Use a task-owned target directory under `~/tmp`; preserve the no-counter binary
before building the allocation variant. For the rejected candidate, start a
separate baseline checkout, write `candidate-matching.patch` from JSON and
apply it with `git apply`. Write `candidate-eviction.rs` and
`candidate-eviction-tests.rs` to the matching module's `eviction.rs` and
`eviction_tests.rs`; copy the retained public probe into that checkout. The
public script in JSON expects saved `baseline-time`, `fix-time`,
`baseline-alloc` and `fix-alloc` executables in its argument directory. It
checks complete selection digests before accepting each case. The interning
comparison uses the embedded baseline diagnostic patch/test source; the lock
script restores source after collecting its own separate diagnostic binaries.
No diagnostic instrumentation is included in the production patch.
