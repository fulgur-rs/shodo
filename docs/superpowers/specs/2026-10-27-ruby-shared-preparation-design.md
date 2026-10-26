# Ruby shared preparation

## Intent and approach

Reduce repeated work for accepted shared RubyContent, following the Astra
consultation and the user's instruction to implement it. Reuse successful style
preparation within one append, keyed by immutable ContentInput Arc identity.
This reduces repeated style processing from O(K*S) to expected O(U*S + K),
where K is occurrences, U unique snapshots and S style payload size. Sorting
unique retained indices and materializing output remain additional costs.
In particular, retained index copies still cost O(K*M) for M indices per
occurrence; O(U*S + K) describes the reused comparison/hash/inheritance work,
not total append complexity.

Alternatives considered: caching only the snapshot wrapper summary would leave
style importing repeated; persistent builder caches and shared raw chunks would
require broader lifetime/offset changes. Use the local cache first.

## Constraints

- Keep Rust 1.89, public API, default limits, dependencies and golden fixtures.
- Cache only successful preparation; its parent indices live only in this append.
- Preserve per-occurrence retained style indices, byte charges, imported items,
  warnings, source offsets and nested RubyInput remapping.
- Restore last_interned on cache hits to the final imported index (or wrapper).
- StyleWork measures actual work; skipping work intentionally lowers consumption.
- Skip equality work for Merge, and use Arc equality for single-column identical
  content only after excluding atomic objects. Keep exact comparisons otherwise.
- Do not implement general text hashes, cross-append caches or shared raw chunks.

## Validation

Prove preparation scales with unique snapshots using operation/copy counts;
exercise aliases interleaved with distinct inputs, first-line styles and nesting.
Keep byte-limit boundary checks and compare accepted outputs against independent
equivalent snapshots. Add a reproducible release measurement varying alias count,
unique count and style size; report actual results without whole-pipeline claims.

## Results

Baseline: fef3a7f. Linux x86_64, rustc 1.91.0, release profile, lockfile unchanged.
Run `cargo run --offline --release -p shodo-bench --example ruby_shared_preparation`.
Each snapshot has nine source styles and 16 raw Open/Close items, with a named
first-line font family of the indicated length. Inputs and parent builders are
prepared outside the timer; `push_ruby` is timed, including output materialization.
Default resource limits are enabled. Each case discards four warmups and reports
the median of 31 samples. The table uses the median of three process medians,
alternating baseline/candidate order with the same `kache test-runner`.

The baseline uses its own target directory; the candidate binary was saved
separately before running the control. A preliminary rerun sharing a target
directory reused an overwritten executable; those results were discarded.

| Family bytes | K occurrences | U snapshots | Before (µs) | After (µs) |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 1 | 1 | 8.591 | 10.547 |
| 16 | 16 | 1 | 76.130 | 10.965 |
| 16 | 128 | 1 | 588.581 | 27.239 |
| 16 | 128 | 8 | 651.720 | 120.901 |
| 16 | 128 | 128 | 732.322 | 739.865 |
| 4096 | 1 | 1 | 11.036 | 11.384 |
| 4096 | 16 | 1 | 118.527 | 13.830 |
| 4096 | 128 | 1 | 889.193 | 31.011 |
| 4096 | 128 | 8 | 895.618 | 79.832 |
| 4096 | 128 | 128 | 977.197 | 1006.462 |

Repeated-input cases improve substantially (128 aliases, 4096 bytes: about 29x
in this append-only workload). Independent-input cases do not benefit and show
about 1–3% higher medians here. One-occurrence cases show variable overhead;
these short measurements establish neither a universal speedup nor a precise
regression bound. No shaping, layout or browser timings were measured.

The existing allocation probe (`ruby_admission`, allocation-counting, CI dev
profile) still rejects all five capped cases with zero allocated bytes/calls.
For 128 aliases of 512 raw items, accepted calls drop 411→157 and allocated
bytes 18,951,936→18,948,380. Peak extra bytes remain 9,511,348 and retained net
bytes 9,507,160. The flat output is still materialized on every occurrence.

Regression tests observed RED before implementation: eight aliases generated
32 inherited styles instead of four; Merge and single-column identity each
compared 4096 bytes instead of zero. After implementation, generated inherited
styles equal 4*U for U=1/4 and K/U=1/8/64. Exact two-alias StyleWork boundaries
drop 475→300 and 696→446, while repeated retained-index StyleBytes remains
1264 (1263 fails before normalization). Nested output, warnings, style indices,
independent snapshots, atomic objects and multi-column fallback are covered.

Verification: full workspace/accesskit tests with CI dev settings passed,
including integration tests and doctests; six focused shared-preparation tests
passed after the final accounting test was added. Stable Clippy workspace/all
targets/accesskit, Rust 1.89 workspace/all targets/accesskit, wasm32 no-default/
accesskit checks passed. Astra approved the design and a separate fresh Astra
review found no Critical/Important issues; those reviews were static.
The final library run includes the extra accounting test: 1112 passed, eight
existing ignored. Formatting and diff checks passed. Supplemental Astra review
of the measurement method and additional accounting test also found no
Critical/Important issues.
