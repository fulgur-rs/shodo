# shodo-im9.2 font matching indexes

Baseline commit: `1e99940ae12ede8d4ef41709af4414841ac441fc`.
Measurements collected 2026-10-03 with release binaries. The probe uses the
pinned `latin` fixture, default limits, and disables system-font loading. The
face-count sweep is 1, 4, 16, 64, 128, and 240; 240 stays within the default
per-layer face limit after accounting for the root stub.

## Workloads and method

`registered` registers F copies of the Latin fixture with distinct explicit
family names, then queries those F names against `水`, which the faces do not
cover. This exercises every named-family candidate before the text fallback.
`native` registers F native faces with the same intrinsic family and queries
`a`; it exercises native `(blob ID, face index)` resolution for each candidate.
The face registrations and queries are identical between the baseline and
indexed builds, and every matched slot (including misses) agrees.

For cold timings, matching caches are disabled. Registration and the first
match are timed separately. Eight baseline/indexed pairs per workload and face
count were run with execution order alternated; the table reports medians.
The raw samples are in
`data/shodo-im9-2-font-match-indexes-cold.csv`.

For allocation measurements, five baseline/indexed pairs were run per case,
again alternating order. The bench `CountingAllocator` reports requested Rust
allocator blocks, not RSS. Registration and cache-miss matching are measured in
separate scopes. Input family strings, queries, and font byte copies are
prepared outside those scopes. The warm-hit check uses a one-family key, primes
the result, and measures a subsequent hit. Raw scope results are in
`data/shodo-im9-2-font-match-indexes-memory.csv`.

## Release timings

Times are microseconds. “Match change” compares indexed against baseline; a
negative value is faster.

| Workload | Faces | Registration baseline → indexed | Cache-miss match baseline → indexed | Match change |
| --- | ---: | ---: | ---: | ---: |
| Registered | 1 | 35.3 → 36.0 | 5.0 → 5.2 | +3.5% |
| Registered | 4 | 42.0 → 41.5 | 7.6 → 7.5 | −1.4% |
| Registered | 16 | 71.0 → 75.9 | 21.2 → 18.2 | −14.3% |
| Registered | 64 | 187.6 → 215.8 | 119.7 → 55.0 | −54.1% |
| Registered | 128 | 437.5 → 423.4 | 341.9 → 121.8 | −64.4% |
| Registered | 240 | 807.6 → 895.0 | 1,032.5 → 211.5 | −79.5% |
| Native | 1 | 37.3 → 35.9 | 6.1 → 6.2 | +1.1% |
| Native | 4 | 46.7 → 49.2 | 5.9 → 5.7 | −3.5% |
| Native | 16 | 104.7 → 92.3 | 15.2 → 12.0 | −21.0% |
| Native | 64 | 360.0 → 313.0 | 33.8 → 31.2 | −7.7% |
| Native | 128 | 807.3 → 800.6 | 67.7 → 70.7 | +4.4% |
| Native | 240 | 1,782.6 → 1,802.5 | 112.8 → 109.1 | −3.3% |

Named-family misses improve with face count. Native matching wall time is
mostly within run-to-run variation in this probe, while the index removes the
linear identity search for every native candidate. Small-face timing changes
are also within that range.

## Allocation scopes

Each cell is the median `baseline → indexed`, in bytes. “Setup net” is retained
net allocation during registration; “miss gross” is total bytes allocated
while matching. Warm-hit gross bytes are unchanged at each size (42–44 bytes
for registered queries; 43 bytes for native queries).

| Workload | Faces | Setup net | Cache-miss gross |
| --- | ---: | ---: | ---: |
| Registered | 1 | 2,566 → 2,924 | 2,417 → 2,435 |
| Registered | 4 | 3,444 → 4,084 | 5,687 → 5,991 |
| Registered | 16 | 8,562 → 10,792 | 24,365 → 22,107 |
| Registered | 64 | 29,058 → 37,672 | 99,197 → 86,715 |
| Registered | 128 | 56,414 → 73,568 | 199,113 → 173,027 |
| Registered | 240 | 59,550 → 93,536 | 381,306 → 324,052 |
| Native | 1 | 3,488 → 3,700 | 1,537 → 1,556 |
| Native | 4 | 4,312 → 4,624 | 1,594 → 1,613 |
| Native | 16 | 10,744 → 11,656 | 7,390 → 7,409 |
| Native | 64 | 34,936 → 38,248 | 45,422 → 45,441 |
| Native | 128 | 67,192 → 73,704 | 91,182 → 91,201 |
| Native | 240 | 80,376 → 93,288 | 178,686 → 178,705 |

At one explicit face, the added setup retention is 358 bytes and miss gross
allocation rises by 18 bytes. At 240 explicit faces, setup retention rises by
33,986 bytes while miss gross allocation falls by 57,254 bytes (15.0%). The
240-face native case retains 12,912 additional setup bytes and changes miss
gross allocation by 19 bytes. Warm-hit allocation is unchanged in both
workloads. These measurements make the small-collection memory cost visible;
the retained index storage grows with registered names and native identities.

## Operation-count checks

RED-run counters showed the original named-candidate scan visiting 129 slots
for a 128-face lookup (including the root stub), versus 3 indexed candidate
slots. For 64 native candidates, the old identity lookup made 2,144 face-ID
comparisons; the indexed code makes 64 `HashMap::get` calls, one per
candidate.
For lazy materialization, a regression test creates 64 catalog candidates,
then forces the highest-slot candidate through the sentinel path to model a
concurrent query materializing it after candidate collection. The old path
visited 65 face slots in the RED run; the passing regression test performs
one identity-map lookup and reuses the existing slot without appending a
duplicate. The native timing probe uses pre-registered fixture faces and does
not time OS catalog startup; this sentinel path is covered by the regression
test. The test counters count explicit vector-slot visits or map lookup
calls; they do not count hash table bucket probes. The constant-average-time
claim follows from the production code replacing the face-vector scans with
`HashMap` operations. All counters are test-only and add no production-path
work.

## Reproduce

Run the indexed timing and allocator probes from this checkout with:

```sh
cargo run --release -p shodo-bench --bin shodo-font-match-probe -- --cold registered 240
cargo run --release -p shodo-bench --features allocation-counting --bin shodo-font-match-probe -- --memory registered 240
```

Use `native` for the native workload or substitute another supported face
count. The baseline binary was built from the baseline commit with the same
probe source and a separate Cargo target directory.
