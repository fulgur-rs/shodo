# Structural style interning keys — 2026-10-08

ParagraphBuilder previously streamed the Debug representation of each normal /
first-line style pair into a hasher, formatted stored keys again to check
collisions, and retained a String key for each unique pair. Root creation paid
this cost once per paragraph. Nonconsecutive reuse repeated formatting even on
a cache hit.

The replacement hashes borrowed fields directly, retaining only fingerprints
and style indices. A single exhaustive field/projection list feeds Hash and
short-circuit equality, including nested paint and font data. All NaN payloads
share an identity and signed zeros remain distinct, matching Debug identity.
The old fast paths used ordinary float PartialEq and could merge signed zeros
depending on reuse order; they now use the same identity as the index. Public
style field types and PartialEq remain unchanged. Eq/Hash derives are added to
non-float style types used by the key. The existing RandomState hasher remains.

StyleBytes now charges 12 logical bytes per fingerprint/index pair rather than
escaped Debug string bytes, alongside the existing owned style slots and
payloads. Checks still precede retained payload cloning; root, first-line and
ruby budgets remain aggregated. The memory regression cases use explicit caps
where removing duplicated keys allows their old input to fit the default cap.

## Measurement

Baseline: `c33658d`, compared with the implementation and harness hashes in the
[JSON record](style-key-hash-2026-10-08.json). AMD Ryzen 5 5600G, Rust 1.96.0,
Linux x86_64, release defaults, complex-scripts enabled. Timing and allocation
instrumentation use separate binaries. Inputs and fixture fonts are prepared
outside measurement. Recording includes builder creation, insertion and drop;
build includes recording, analysis, shaping and drop with a warmed context.
Root records one character; other cases record 512 spans with 1 or 16 styles,
or 64 spans with 64 unique styles. Each process warms up 3 times and samples 9
times. Three process pairs alternate before/after order; the table uses all 27
samples per case. No task build or test ran concurrently with these samples.

| Case | Operation | Before µs | After µs | After/before | Allocated bytes before → after |
| --- | --- | ---: | ---: | ---: | ---: |
| consecutive-1x512 | build | 461.05 | 454.37 | 0.986 | 1,413,519 → 1,409,977 |
| consecutive-1x512 | record | 30.11 | 25.78 | 0.856 | 301,065 → 297,523 |
| first-line-16x512 | build | 6258.99 | 1323.45 | 0.211 | 2,740,541 → 2,682,641 |
| first-line-16x512 | record | 5112.09 | 389.70 | 0.076 | 404,040 → 346,140 |
| reuse-16x512 | build | 2995.29 | 647.78 | 0.216 | 1,469,134 → 1,438,376 |
| reuse-16x512 | record | 2457.35 | 201.56 | 0.082 | 352,566 → 321,808 |
| root | build | 14.50 | 8.87 | 0.612 | 10,974 → 9,246 |
| root | record | 4.83 | 0.63 | 0.129 | 3,832 → 2,104 |
| unique-64 | build | 409.22 | 121.81 | 0.298 | 424,206 → 306,328 |
| unique-64 | record | 323.68 | 35.81 | 0.111 | 263,670 → 145,792 |

Every geometry, paint and warning digest matched across the two revisions and
both measurement modes; real fixture fonts produced zero synthetic glyphs.
This is a standalone builder/build probe. It does not establish a 26%
instruction reduction in a caller's whole-page layout, nor does it time
line-breaking separately. Timings remain sensitive to system scheduling.

An initial eager tuple comparison increased the consecutive-style recording
case from roughly 30 to 52 µs. Inspecting its generated code showed that the
whole borrowed view was materialized before comparison. Field-wise lazy
projection fixed that regression; the table reports the final implementation.
A shared-target cache also initially returned the baseline executable for the
candidate. That run was discarded; changed engine and distinct executable
hashes were verified before the final paired runs. Per-sample timing, round
medians, allocation medians, output digests and fingerprints are preserved in
the JSON record; disposable logs and binaries are removed after integration.

## Reproduction

Run the checked-in `style_interning` example at both engine revisions using the
same copy of the harness and Cargo.lock, machine and toolchain. Use separate
target directories to avoid stale top-level artifacts from another worktree:

```sh
mkdir -p "$HOME/tmp"
probe_dir=$(mktemp -d -p "$HOME/tmp" shodo-style-interning.XXXXXX)
TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=env \
  cargo build --offline --release -p shodo-bench --example style_interning \
  --target-dir "$probe_dir/time-target"
"$probe_dir/time-target/release/examples/style_interning" time
TMPDIR="$HOME/tmp" RUSTC_WRAPPER= CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=env \
  cargo build --offline --release -p shodo-bench --example style_interning \
  --features allocation-counting --target-dir "$probe_dir/alloc-target"
"$probe_dir/alloc-target/release/examples/style_interning" alloc
```

Save each executable before switching revisions, verify distinct executable
SHA256s, and repeat three times in alternating order. Verify output digests
before comparing medians. Remove the temporary targets after recording results.

## Validation

- The new owned-data-budget and signed-zero-order regressions failed against
  the baseline, then passed after implementation.
- `cargo test --offline -p shodo --lib builder::`: 24 passed, including an
  independent Debug oracle over 18 float slots × 12 values, in normal and
  first-line styles. Collision, NaN, style reuse, byte budgets and ruby
  restoration tests passed.
- `cargo test --offline -p shodo-bench --features allocation-counting --test
  style_memory`: accepted roots allocate fewer than three payload copies;
  rejected roots/ruby content do not copy oversized caller data.
- `cargo fmt --all --check`, `git diff --check`, and
  `cargo clippy --offline --workspace --all-targets -- -D warnings` passed.
- Independent code review found no required fixes, including the final lazy
  comparison and allocation regression.
