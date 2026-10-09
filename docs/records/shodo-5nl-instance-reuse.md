# shodo-5nl: per-call font instance preparation reuse

## Decision and scope

Adopt the bounded preparation reuse. Interleaved Latin/CJK/emoji text repeatedly
resolves the same matched face, variations, coordinates and metrics. The cache
shares the fully initialized RunInstance and ShaperInstance within one
`shape_inputs` invocation. Single-input shaping already shared preparation
between budget windows and receives no intended speedup.

Baseline: main `8557359ef88f0f47e2a46281a7485a649cec45c1` (#271).
Measured implementation: `d5c04658abf6878e6cb618cbabcffaf3295ac19e`.
The companion [JSON](shodo-5nl-instance-reuse.json) retains source/font/binary
hashes, all timing samples, allocation counters, output digests, warning
sequences, Callgrind counters, Massif peaks and Memcheck summaries. Independent
code and measurement-record reviews found no unresolved issues.
[PR #272](https://github.com/fulgur-rs/shodo/pull/272) contains the implementation;
its final-head CI status is tracked in the [PR checks](https://github.com/fulgur-rs/shodo/pull/272/checks).

The cache has eight fixed stack slots, FIFO replacement, and a 16,384-byte
retained heap budget. Keys borrow immutable paragraph inputs. Charged heap
payload includes the RunInstance and Arc headers, coordinate/variation vector
capacities, language capacity and the shared feature array. Charging shared
features conservatively also excludes large feature states. Oversized author
vectors are rejected before expensive equality checks. Fonts with more than
11 fvar axes bypass retention: Harfrust 0.12 has 11 inline coordinates, and
clearing default coordinates can leave a private heap capacity behind.

The key includes FontMatch identity/variations/embolden/skew, size, size-adjust,
explicit variations, optical sizing, script, language and the actual feature
array, including orientation and width overrides. Float bits are compared so
signed-zero public metadata survives. Instances are finalized before insertion;
sharing never mutates a retained Arc. Size-adjust warnings are represented as
static effects and pushed once per input, including cache hits and suppressed
attempts, preserving the sink's order and limit behavior.

Preparation identity previously also identified shaping input boundaries in
line-edge compatibility. Sharing it without a separate boundary identifier
failed two ruby memo tests and extended reshape windows. `ShapedRun.shaping_input`
now identifies the actual input, is propagated through budget/storage splits,
and is remapped when shaped outputs are appended. Compatibility and conservative
edge-cache admission still operate on the original input boundaries. This
preserves full preparation sharing without changing reshape budgets or warnings.

## Results

Each ordinary probe has 256 repeats of its short text, 768 glyphs, fixed real
fixture fonts, and no system discovery. The vertical case has 1,024 glyphs.
The nine-size case cycles nine sizes; the oversized case authors 4,096 distinct
variation tags. Full inputs and limits are defined in
[the probe](../../dev/bench/examples/shape_instance_reuse.rs).

Ratios below are candidate / baseline. Callgrind collected whole processes;
the shaping column is the inclusive `shape_inputs` entry attributed to
`crates/shodo/src/shape.rs`, not a sum of its inline-source partitions. The
process column includes setup, analysis, shaping and destruction. Each row
uses 20 builds per process, except oversized variations, which uses one.

| Case | Shaping Ir ratio | Process Ir ratio | Build time ratio | Shape time ratio |
| --- | ---: | ---: | ---: | ---: |
| Latin single input | 1.000757 | 1.000261 | 0.993 | 0.957 |
| Latin/CJK | 0.649517 | 0.826689 | 0.824 | 0.732 |
| Latin/emoji | 0.652452 | 0.829310 | 0.820 | 0.790 |
| CJK/emoji | 0.608316 | 0.836544 | 0.834 | 0.646 |
| Size-adjust | 0.573515 | 0.774774 | 0.787 | 0.690 |
| Vertical mixed | 0.641632 | 0.818173 | 0.821 | 0.762 |
| Nine sizes | 0.912108 | 0.960196 | 0.981 | 0.954 |
| Oversized variations | 1.000003 | 1.000001 | 0.964 | 0.994 |

Timing uses the System allocator, release with debug level 1, CPU 2 affinity,
21 samples per process/case, and four processes per revision in ABBA then BAAB
order. Each ratio is the median of the candidate process medians divided by
the median of the baseline process medians. Build samples measure
`ParagraphBuilder::build` after builder construction; shape samples prepare
analysis before timing `ParagraphAnalysis::shape`. Result destruction is outside
the elapsed interval. Font caches are warmed with two unmeasured builds.

Build timing launches measured all cases. The first all-case shape timing batch
had substantial drift, including apparent single-input regressions; its raw
samples and summaries remain in JSON as `initial_batch_shape_*`. Shape timing
was repeated with ABBA/BAAB separately for each case, shortening the interval
between paired measurements. The table uses this repeated batch. SMT sibling
load, other host jobs, CPU frequency and scheduling remained uncontrolled;
these elapsed ratios are observations, not universal speedup guarantees. The
single-input and oversized controls show effectively unchanged instruction and
allocation costs. No speedup claim is made for those controls.

The operation-count regression first observed 33 coordinate constructions for
33 Latin/CJK inputs. The candidate builds two instances and two coordinate
instances for both Latin/CJK and Latin/emoji. A second independent invocation
rebuilds both: no state survives the call. Size-adjust adds its separate metric
coordinate construction on misses; its warning is still replayed on every hit.

Allocation counting uses separate binaries. Builder or analysis input is already
live at the scope boundary and is consumed by the operation. `net_bytes` is
therefore a signed live-byte delta, not total paragraph memory. The retained
result and its later drop are measured separately. Requested allocation bytes,
peak live requested bytes, Massif heap, allocator administration, stack and RSS
are distinct quantities.

| Build case | Allocation calls before → after | Retained net bytes before → after | Peak extra requested bytes before → after |
| --- | ---: | ---: | ---: |
| Latin single input | 246 → 246 | 222,845 → 222,845 | 265,421 → 265,421 |
| Latin/emoji | 3,062 → 2,551 | 497,605 → 403,581 | 500,117 → 406,093 |
| Vertical mixed | 5,125 → 4,360 | 622,389 → 481,629 | 626,981 → 486,221 |
| Nine sizes | 4,439 → 4,183 | 649,625 → 602,521 | 730,265 → 683,161 |
| Oversized variations | 14,909 → 14,909 | 17,307,589 → 17,307,589 | 17,431,088 → 17,431,088 |

Latin/emoji saves 511 allocations and 94,024 requested bytes (91.8 KiB) of
retained and peak-extra memory. Sharing repeated mandatory RunInstances more
than offsets bounded temporary preparation retention. The fixed stack slots
are outside the heap budget and the heap-only Massif measurement.

Host Massif (`--time-unit=B --stacks=no --detailed-freq=1 --max-snapshots=200`,
two full builds) observed peak heap bytes of 879,814 → 879,814 for the single
input, 1,120,841 → 1,026,817 for Latin/emoji, and 18,086,085 → 18,086,085 for
oversized variations. These process peaks include fixture/font setup and differ
from the allocation scope above. Heap plus administration peaks are in JSON.

Memcheck ran four fixtures for each revision, two full builds each, in an
ephemeral Debian bookworm container using the same frozen host-built binaries.
Host glibc cannot start Memcheck because mandatory loader debuginfo is absent;
container Valgrind 3.19.0 and glibc 2.36-9+deb12u14 provide the working runtime.
All eight runs report zero errors and zero definite, indirect or possible lost
bytes. Both revisions retain the same 456-byte Rust stack-overflow thread-info
allocation at process exit. No container or host package installation remains.

## Verification and reproduction

All ten cases match baseline/candidate public digests and ordered warnings,
including allocation binaries. Digests cover face bytes/index, glyphs/clusters/
positions/advances/origins, line geometry, normalized/design coordinates,
horizontal/vertical metrics, script, language and synthesis. The differential
unit matrix compares cached and bypassed shaping for 18 conditions, four warning
caps and two writing modes, including real fvar/opsz, width features and signed
zero. Capacity, eviction, drop and high-axis/default-coordinate tests also pass.

Local checks: workspace 1,735 passed / 9 ignored; library 788 passed / 8 ignored;
no-default 1,200 passed / 9 ignored; complex-scripts 1,203 passed / 9 ignored;
16 instance-related tests passed. Strict workspace Clippy and the new probe's
allocation-counting Clippy pass. An extra allocation-counting Clippy sweep of
all existing bench targets encountered pre-existing `needless_question_mark`
and `bool_comparison` diagnostics; no unrelated source changes were retained.
Independent read-only reviews of the complete code commit and the measurement
records found no unresolved issues. Official CI, including MSRV, AccessKit,
Wasm, package and fixture checks, is tracked by PR #272 checks.

Use the same probe source and Cargo example declaration on both revisions.
The probe SHA256 is
`360c3bf1bd1e05707d9b22cf811b4616f52b21e39a13c8690460e95054bfc025`.
Create isolated worktrees following `.claude/worktrees/` convention and a
scratch directory under `~/tmp`:

```bash
mkdir -p "$HOME/tmp"
task_scratch=$(mktemp -d -p "$HOME/tmp" shodo-5nl.XXXXXXXX)
export TMPDIR="$task_scratch"
```

Build each revision twice, copying the example executable before switching
allocator features. Rust 1.91.0 was used, with no custom target runner:

```bash
env -u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER \
  CARGO_TARGET_DIR="$task_scratch/target-$revision" \
  CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +1.91.0 build --release -p shodo-bench --example shape_instance_reuse
cp "$task_scratch/target-$revision/release/examples/shape_instance_reuse" \
  "$task_scratch/$revision-time"
# Repeat build with --features allocation-counting, saving $revision-alloc.
```

Run `digest`, `alloc` and `shape-alloc` on both saved binaries and compare output
digests and warning sequences. Run timing binaries in ABBA then BAAB order,
where A is baseline and B is candidate. For `time`, run all cases; for
`shape-time`, repeat the order for each individual case:

```bash
taskset -c 2 "$task_scratch/$revision-time" time
taskset -c 2 "$task_scratch/$revision-time" shape-time latin-emoji
valgrind --tool=callgrind --callgrind-out-file="$task_scratch/profile.out" \
  "$task_scratch/$revision-time" loop latin-emoji 20
callgrind_annotate --inclusive=yes --threshold=100 --auto=no \
  "$task_scratch/profile.out"
valgrind --tool=massif --time-unit=B --stacks=no --detailed-freq=1 \
  --max-snapshots=200 --massif-out-file="$task_scratch/heap.out" \
  "$task_scratch/$revision-time" loop latin-emoji 2
```

Callgrind cases: single, Latin/CJK, Latin/emoji, CJK/emoji, size-adjust, vertical,
nine sizes and oversized variations (one iteration for oversized). Massif cases:
single, Latin/emoji, oversized. Memcheck cases: single, Latin/emoji,
warning-limit0, oversized, in both revisions, using `--leak-check=full
--show-leak-kinds=all --errors-for-leak-kinds=definite,indirect,possible
--error-exitcode=97`. Install Valgrind/libc6-dbg only inside the disposable
container; use the image digest and runtime versions recorded in JSON.

After the results, samples, artifact hashes and reproduction steps were archived
and independently reviewed, the owned measurement/build scratch
`~/tmp/shodo-5nl.fubm2vQy` (16 GiB) and clean comparison worktree
`.claude/worktrees/shodo-5nl-base` were removed. The disposable Memcheck
container left no container mounting that scratch directory; the preexisting
image was retained. Implementation worktree `.claude/worktrees/shodo-5nl` and
branch `perf/shodo-5nl-instance-reuse` remain for PR review. Existing user
files were left untouched.
