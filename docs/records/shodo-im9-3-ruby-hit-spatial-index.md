# Ruby hit spatial index

Issue: `shodo-im9.3`. Build a spatial index for visible ruby annotations so hit queries test only bounds that contain the point, while retaining reverse sibling and nested priority.

`LineLayout` now indexes each annotation's hit bounds. An annotation bound encloses its child line's body hit geometry and all descendant annotation hit geometry, with descendant corners transformed into the parent line's coordinates. Invalid bounds use the previous exact hit path as a fallback. Candidate hits are still confirmed by the existing transform and hit routines; the index chooses the latest actual hit. Base caret and source attribution remain on their existing path.

## Release measurements

The fixed-font fixture builds one line with the indicated number of visible ruby annotations. Query samples each run 2,000 calls and report the median of seven samples. Build samples run 16 complete `LineLayout` constructions each and report the median per construction. Timing builds and allocation-counting builds are separate. Sparse hit/miss points, body hit points and distant main-text misses are validated before timing.

Before uses `cfad161bb0a2cac935c7ab436bd4cdd5dfab0944`. After uses the `shodo-im9.3` candidate on the same checkout. The measurements ran on x86_64 Linux with rustc 1.96.0 and an AMD Ryzen 5 5600G.

| Visible annotations | Build µs before → after | Ruby hit ns before → after | Ruby miss ns before → after | Body hit ns before → after | Main miss ns before → after |
|---:|---:|---:|---:|---:|---:|
| 16 | 11.960 → 12.318 | 662 → 111 | 544 → 10 | 708 → 91 | 625 → 73 |
| 64 | 53.566 → 57.146 | 2,971 → 127 | 2,614 → 10 | 2,954 → 121 | 2,742 → 93 |
| 256 | 217.867 → 236.638 | 15,875 → 143 | 12,221 → 10 | 14,232 → 147 | 13,385 → 116 |
| 1,024 | 851.331 → 974.228 | 60,537 → 156 | 52,401 → 10 | 56,905 → 172 | 52,439 → 138 |

The candidate-visit regression covers R=16/64/256/1024: a separated annotation hit invokes one exact annotation check, a distant miss invokes zero, and a body hit outside ruby bounds invokes zero. A second regression interleaves block-axis positions in bit-reversal order while keeping inline bounds equal. It checks tree-node visits for both a hit and a gap miss; the previous inline-only split fails this case. The adaptive tree splits each subtree along its wider center-coordinate spread. Overlapping bounds can still require O(R) exact checks.

| Visible annotations | Build retained bytes before → after (delta) | Build peak extra bytes before → after (delta) |
|---:|---:|---:|
| 16 | 19,136 → 22,992 (+3,856) | 19,160 → 23,264 (+4,104) |
| 64 | 75,584 → 90,960 (+15,376) | 75,608 → 92,384 (+16,776) |
| 256 | 301,376 → 362,832 (+61,456) | 301,400 → 368,864 (+67,464) |
| 1,024 | 1,204,544 → 1,450,320 (+245,776) | 1,204,568 → 1,474,784 (+270,216) |

Allocation values count requested allocator bytes, not RSS. Retained bytes are measured after constructing and retaining the `LineLayout`; peak extra bytes include temporary construction storage. Across all R values, 2,000 ruby-hit queries request 16,010 bytes in both versions (2,001 allocation calls, including the returned hit path). Ruby-miss and body-hit scopes each report the same 10-byte single-call harness event before and after. The new query index adds no measured query allocation.

Raw before/after timing and allocation samples are in `data`: `shodo-im9-3-ruby-hit-spatial-index-{before,after}-{timing,memory}.json`. The probe is [ruby_hit_index.rs](../../dev/bench/examples/ruby_hit_index.rs). Reproduce from the repository root with:

```sh
cargo run --offline --release --manifest-path dev/bench/Cargo.toml --example ruby_hit_index
cargo run --offline --release --features allocation-counting --manifest-path dev/bench/Cargo.toml --example ruby_hit_index
```

Correctness coverage includes protruding nested geometry, affine transform inversion, parent block offsets, nested and sibling overlap priority, hidden annotations, non-finite transforms, hit source attribution and base caret attribution.
