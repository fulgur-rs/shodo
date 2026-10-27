# Clone decoration work: allocation oracle recalibration

The new decoration-work limit enlarges the limits and ledger metadata used by
edge shaping. The absolute SHODO-56 allocation baselines therefore need to use
the same metadata as the candidate. This does not change the output digests or
the reduction required by the original allocation tests.

Measured on Linux x86_64 with rustc 1.91.0, the fixed fonts and the existing
`allocation-counting` examples. The candidate includes the Clone-only summary
storage and decoration-work budget. The control applies the reverse of the
source diff `ba62f26^1..ba62f26` to that same candidate, as in
`2026-10-26-shodo-56-allocation-oracles.md`. This reverts SHODO-56 only; the
decoration changes and all later code and dependencies remain present. The
control was measured in a disposable worktree. Both runs used the same fixture
and output SHA-256 values; all hashes matched the existing test fixtures.

The dev run used the CI settings `CARGO_PROFILE_DEV_OPT_LEVEL=1`,
`CARGO_PROFILE_DEV_DEBUG=line-tables-only`, and `CARGO_INCREMENTAL=0`.
The release run used `--release` and `CARGO_INCREMENTAL=0`. Values below are
total allocated bytes in the workload's measured scope, not live bytes or RSS.

| Workload | Dev control | Dev candidate | Release control | Release candidate |
| --- | ---: | ---: | ---: | ---: |
| uniform-ltr | 1,220,941 | 1,185,171 | 1,220,941 | 1,185,171 |
| uniform-rtl | 1,484,625 | 1,475,315 | 1,484,625 | 1,475,315 |
| mixed-levels | 809,933 | 808,937 | 809,933 | 808,937 |
| no-continuation | 312,952 | 312,056 | 312,952 | 312,056 |
| rtl-continuation-hanging | 1,705,521 | 1,705,149 | 1,705,521 | 1,705,149 |
| context-build | 582,636 | 577,372 | 582,588 | 577,324 |
| edge-window | 1,226,867 | 1,222,315 | 1,226,867 | 1,222,315 |
| repeated-opening | 365,348 | 362,388 | 365,300 | 362,340 |
| repeated-closing-bracket | 362,052 | 359,092 | 362,004 | 359,044 |
| fi-splits | 634,368 | 632,736 | 634,344 | 632,712 |
| ffi-splits | 1,194,688 | 1,193,056 | 1,194,664 | 1,193,032 |
| unsplit-control | 534,694 | 534,694 | 534,670 | 534,670 |

Line and reshape thresholds use the measured control; punctuation retains the
existing policy of using the measured optimized value, with the independent
metrics-construction unit test still in force. Context retains its requirement
to save at least 20 bytes per item. No timing claim is made.

For each profile and each of `line_small_allocations`,
`shape_context_allocations`, `punctuation_metrics`, and
`reshape_split_allocations`, the example command was:

```sh
TMPDIR="$HOME/tmp" CARGO_INCREMENTAL=0 cargo run --offline --profile "$profile" \
  -q -p shodo-bench --features allocation-counting --example "$example" -- --alloc
```

The dev invocation additionally set the two CI profile variables above. The
disposable worktree and its build outputs were removed after the results were
recorded.
