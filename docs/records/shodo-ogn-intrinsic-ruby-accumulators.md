# shodo-ogn: protect intrinsic row ruby accumulators

## Decision and scope

Adopt protection for the current intrinsic max-content row. Multiple words with
multiple ruby containers between clearing floats evict the growing row from the
two-entry accumulator LRU. Keeping its exact key avoids remeasuring the row's
prefix at each float. The previously protected look-ahead walk (#260) already
stays linear; this change addresses container measurements separately.

Baseline: main `46cc12cfc695b698d027cd8c998724abce2b31dc` (#272).
Measured implementation: `5c34dd1c611873daa6b8caf953a193cc094b0533`.
The companion [JSON](shodo-ogn-intrinsic-ruby-accumulators.json) archives all
chronological timing samples, process medians, public output bits/digests and
ordered warnings, allocation scope counters, profile totals, memory summaries,
source/font/binary hashes and validation evidence. Integration and exact-head
CI are tracked by the associated pull request.

`RubyMemo::retain_intrinsic_row` pins dataset id/address, atomic revision and
start alongside the existing row-walk pin. Initial row setup, forced breaks and
first-line dataset switches use the max-content atomic revision. Both take and
put evict the oldest non-pinned accumulator. No pin retains the ordinary LRU;
clear releases the pin. Nested returns still remove duplicate keys and respect
the two-entry bound. One row and one word can be retained without adding a slot.

The capacity remains two accumulators, with at most 16,384 containers each and
at most 256 container capacities retained on reset. The accumulator validity,
epoch, dirty/profile, shrinking-end, rollback/replay and work-charge rules are
unchanged. Operation reset and `shrink_to(0)` still release the state. This is
intrinsic-specific protection, with no general claim of linear complexity for
arbitrary ruby structures or other layout paths.

## Results

The ordinary fixture repeats 64 groups of two words, each containing two ruby
containers over `12` with annotation `日日日`, followed by a clear-Both float.
Root size is 24 px, annotation size 12 px, overhang disabled, float min/max 1.
Preparation uses the fixed checked-in CJK font with system discovery disabled.
Controls and variants are defined in
[the probe](../../dev/bench/examples/intrinsic_ruby_rows.rs).

The regression test first failed with container measurement counts
147 → 547 → 2,115 at 8 / 16 / 32 groups. The candidate produces
63 → 127 → 255. Both revisions walk 62 → 126 → 254 steps. This demonstrates
removal of the eviction-related repeated measurements, independent of the walk
fix. Unlimited widths and ordered warnings agree with the reference path.

Ratios are candidate / baseline. Callgrind uses five fresh-context intrinsic
calls per process. The intrinsic column is the single inclusive `intrinsic_sizes`
entry attributed to `crates/shodo/src/line/intrinsic.rs`; inline-source partitions
are not summed. Process Ir additionally includes font/paragraph preparation and
context destruction. Elapsed time measures only each intrinsic call.

| Case | Intrinsic Ir ratio | Process Ir ratio | Intrinsic time ms before → after | Time ratio |
| --- | ---: | ---: | ---: | ---: |
| eviction | 0.145977 | 0.191269 | 25.125 → 4.620 | 0.184 |
| one-word | 0.999411 | 0.999183 | 2.417 → 2.680 | 1.109 |
| one-ruby | 0.999254 | 0.999077 | 2.465 → 2.190 | 0.888 |
| plain | 1.000546 | 1.000227 | 0.060 → 0.061 | 1.010 |
| forced | 0.685466 | 0.751252 | 5.510 → 4.409 | 0.800 |
| first-line | 0.164641 | 0.192634 | 49.779 → 10.283 | 0.207 |
| atomics | 0.235055 | 0.273635 | 29.156 → 9.779 | 0.335 |

Timing uses separate System-allocator binaries, release optimization 3 with debug
level 1, CPU 2 affinity, one warmup and 21 fresh-context trials per process.
Each case is launched in ABBA then BAAB order, four processes per revision.
The ratio divides the medians of the four process medians. Input preparation
and context destruction are outside each interval; all chronological samples
are preserved in JSON. Own builds and Valgrind jobs had finished before timing.
Other cargo-mutants and independent raikiri jobs were active on the host;
SMT sibling load, frequency and scheduling were uncontrolled. The elapsed ratios
are observations, not universal speedup guarantees. Controls show effectively
unchanged instruction/allocations; no speedup or regression claim is made from
their noisy timings (one-word time ratio 1.109, one-ruby 0.888).

The warning0, warning2, budget0 and default-budget timing ratios are respectively
0.186, 0.168, 0.980 and 0.150; their complete samples are in JSON. All eleven
ordinary cases have matching baseline/candidate width bits and ordered warnings,
also between timing and allocation binaries.

**Binding work budgets can change public output.** A separate budget1 probe at
64 groups exhausts the baseline's allowance, producing a budget warning and
degraded max-content width (bits `1172987136`). The candidate avoids that work,
returns the unlimited exact width (bits `1176153728`) and emits no warning.
Min-content width agrees (bits `1116733440`). This case is intentionally excluded
from the eleven-case parity claim. The allowance charges actual live work, so
reducing repeated work can prevent refusal; the charge/refusal definitions are
unchanged. The Reference test mode bypasses the allowance and cannot serve as
an oracle for bounded refusal. Tests instead verify sticky refusal and repeated
vs fresh-context behavior at factors 0, 1 and 16.

## Memory

Allocation scopes begin with prepared paragraph/fonts and a fresh context.
Requested allocations, signed retained net and peak-extra live bytes describe
only the intrinsic call. Context shrink and final context/original warning-vector
drop are measured separately. Their net deltas reconcile to zero in all 22
ordinary comparison runs. Report formatting occurs outside those scopes.

| Case | Allocation calls before → after | Retained net bytes before → after | Peak-extra requested bytes before → after |
| --- | ---: | ---: | ---: |
| eviction | 223,984 → 26,133 | 7,140,140 → 7,075,628 | 7,141,065 → 7,076,553 |
| one-word | 13,212 → 13,212 | 3,540,268 → 3,540,268 | 3,541,193 → 3,541,193 |
| one-ruby | 13,623 → 13,623 | 3,567,588 → 3,567,588 | 3,569,700 → 3,569,700 |
| plain | 129 → 129 | 0 → 0 | 192 → 192 |
| forced | 43,000 → 25,992 | 7,023,820 → 7,023,820 | 7,024,745 → 7,024,745 |
| first-line | 447,040 → 54,503 | 7,140,140 → 7,140,140 | 7,141,065 → 7,141,065 |
| atomics | 238,900 → 41,149 | 14,108,624 → 14,044,112 | 14,110,461 → 14,045,949 |
| warning0 | 224,042 → 26,191 | 7,165,079 → 7,100,567 | 7,166,004 → 7,101,492 |
| warning2 | 224,044 → 26,193 | 7,165,141 → 7,100,629 | 7,166,066 → 7,101,554 |
| budget0 | 260 → 260 | 199 → 199 | 775 → 775 |
| default-budget | 223,984 → 26,133 | 7,140,140 → 7,075,628 | 7,141,065 → 7,076,553 |

The eviction case removes 197,851 allocation calls and 64,512 requested bytes
(63 KiB) of both retained and peak-extra memory. It also reduces gross allocated
bytes from 14,065,290 to 9,582,938. This does not imply that all row contents become
small: retaining the actual growing row is still subject to the existing bound.
The pin increases `size_of::<LayoutContext>()` from 1,720 to 1,760 bytes, a fixed
40-byte inline cost outside heap-only measurements. These are requested memory
figures, not RSS or stack measurements.

Host Massif uses two fresh-context calls after one paragraph/font preparation,
with `--time-unit=B --stacks=no --detailed-freq=1 --max-snapshots=200`. Process
peak heap bytes are 10,886,052 → 10,822,340 for eviction, 5,539,932 → 5,539,932
for one-word and 580,771 → 580,771 for plain. Eviction heap-plus-administration
peaks are 11,125,840 → 11,061,960. These include prepared input and have different
boundaries from the intrinsic allocation scopes; the deltas need not equal.

Memcheck ran eviction, warning0 and plain for both frozen revisions, 32 groups
and two fresh-context calls each, in an ephemeral Debian bookworm container.
All six runs report zero errors and zero definite, indirect or possible lost
bytes. Each revision has the same 456 bytes in one still-reachable allocation
from Rust std's stack-overflow thread-info BTreeMap, confirmed by the allocation
backtrace. Container Valgrind 3.19.0 / glibc 2.36-9+deb12u14 avoid the known host
loader-debuginfo incompatibility; host profiling uses Valgrind 3.25.1. No host
packages were changed and the container runs with `--rm`.

## Validation and reproduction

Six new tests cover measurement growth, the real Reference/step-oracle
boundary matrix, warning order/suppression, operation-local budget behavior,
nested take/put payload and capacity, and repinning dataset/revision/start.
The 96 boundary comparisons combine first-line, forced breaks and min/max atomic
inputs, warning caps None/0/2, accumulator caps None/3 and two repeated calls;
zero shrink clears all accumulator footprints. Real missing-atomic warnings
verify suppression order separately. Existing workspace tests cover epoch,
shrinking ends, rollback/retry, dirty/profile gates and overflow fallback.

Local workspace: 1,741 passed / 9 ignored / 0 failed (library 794 / 8 / 0),
101 result suites. Strict workspace/all-targets Clippy, strict allocation-counting
probe Clippy, format and diff checks pass. The full workspace suite was rerun on the immutable measured commit with
`RUSTFLAGS="-D warnings"` after the final lint-only test/probe cleanup. The final
counter test also passed on that commit. Exact final-head MSRV, AccessKit, Wasm,
no-default/complex-scripts, docs, package and fixture checks belong to PR CI.
Independent read-only review of all five code files found no findings; its
scope did not include independently executing tests. A separate independent
record review verified all 88 archived artifact hashes, four binaries, timing
samples/aggregates, public results and memory/profile counters, found no findings
and supported adoption.

Two new-test setup mistakes were corrected during development: a fixture helper
initially forced unlimited work, and an initial factor-0 comparison incorrectly
used Reference despite its allowance bypass. The final budget tests compare
repeated and fresh bounded contexts. No product changes were made in response
to those setup errors.

Use the same probe and Cargo example declaration on both revisions. Probe SHA256:
`3c602f38a1b27f4439b5367c0dd6c687e2420b22e5386d74a65a4f19731b1cc7`.
Create isolated worktrees following `.claude/worktrees/` convention and a scratch
directory under `~/tmp`; use Rust 1.91.0 with no custom Cargo target runner:

```bash
mkdir -p "$HOME/tmp"
task_scratch=$(mktemp -d -p "$HOME/tmp" shodo-ogn.XXXXXXXX)
export TMPDIR="$task_scratch"
env -u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER \
  CARGO_TARGET_DIR="$task_scratch/target-$revision" \
  CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_INCREMENTAL=false \
  cargo +1.91.0 build --release -p shodo-bench --example intrinsic_ruby_rows
cp "$task_scratch/target-$revision/release/examples/intrinsic_ruby_rows" \
  "$task_scratch/$revision-time"
# Repeat with --features allocation-counting; save $revision-alloc separately.

"$task_scratch/$revision-time" digest eviction 64
"$task_scratch/$revision-alloc" alloc eviction 64
taskset -c 2 "$task_scratch/$revision-time" time eviction 64
valgrind --tool=callgrind --callgrind-out-file="$task_scratch/profile.out" \
  "$task_scratch/$revision-time" loop eviction 64 5
callgrind_annotate --inclusive=yes --threshold=100 --auto=no \
  "$task_scratch/profile.out"
valgrind --tool=massif --time-unit=B --stacks=no --detailed-freq=1 \
  --max-snapshots=200 --massif-out-file="$task_scratch/heap.out" \
  "$task_scratch/$revision-time" loop eviction 64 2
```

Repeat the eleven digest/alloc cases and timing in the table/JSON plus the
separate budget1 digest. Run seven Callgrind cases, three Massif cases and
three Memcheck cases for both revisions as recorded in JSON. Use Memcheck
`--leak-check=full --show-leak-kinds=all
--errors-for-leak-kinds=definite,indirect,possible --error-exitcode=97` and the
recorded image digest. Install Valgrind/libc6-dbg only inside the disposable
container and mount the same frozen binaries.

After archival and independent review, the owned measurement/build scratch
`~/tmp/shodo-ogn.h5C53K3Q` (about 12 GiB) and clean comparison worktree
`.claude/worktrees/shodo-ogn-base` were removed. No Memcheck container remains
mounting that scratch; the preexisting image was retained. Implementation
worktree `.claude/worktrees/shodo-ogn` and branch
`perf/shodo-ogn-row-accumulator` remain for PR review and follow-up. There are no
performance build outputs in the implementation worktree. Preexisting user
files remain untouched.
