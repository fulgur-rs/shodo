# SHODO-42: hierarchical limits ledger

Paragraph construction and imported ruby bases now charge one owned ledger.
Each scope keeps its original `Limits` and cumulative costs. Producers check the
owning scope and its ancestors before allocation, and errors name the original
cap and actual usage of the scope that failed. Processed items and shaping items
remain separate counters, preserving the public Items contract.

The change removes reduced `Limits` copies, `RubyBudget`, `BaseScopes`, error
translation, scoped producer twins and `WindowCharge`. Mapping and analysis cost
structs remain usage statistics, with enforcement delegated to the ledger.
Sparse processed-item ownership boundaries avoid a new allocation per character.
The ledger is owned data, preserving Send/Sync without synchronization in the
producer path.

## Trials and reuse

TCY trials use checkpoint, rollback and commit. Unselected outputs do not consume
retained glyph budgets or add warnings. Selected trials retain producer order so
commit reports the first original cap failure without shaping again to recreate
an error. An active outer checkpoint keeps nested producer boundaries rather
than aggregating them.

Paint-only first-line clones reserve the same frame and ancestor costs as the
producer pass. Reservation can select replay when a budget changes the result,
but an unrelated existing warning no longer disables valid reuse. Source-cut
charges shared with the initial pass are excluded from duplicate reservation.
Noncumulative shaping-run windows apply within the current paragraph frame and
its local base chain, rather than inheriting a different annotation frame's cap.

## Review evidence

- Equal remaining room in parent and child scopes now has unambiguous ownership;
  tests cover nested and sibling scopes, overflow and 216 cap combinations.
- Trial undo snapshots are saved once per touched scope. A 128-window trial with
  17 scopes failed with 2,176 snapshots and now uses 17, retaining the linear
  `charges + depth` bound.
- Whitespace capacity grows only after a successful Items charge. A failing
  alternate/ancestor case previously allocated 90,112 bytes before detecting
  its exhausted cap; the allocation regression now passes.
- Nested commit preserves the first producer failure. The regression previously
  reported actual 106 and now reports the expected actual 101.
- Annotation run-cap regression coverage checks warning order with a UTF-8
  annotation and an ancestor's one-byte window cap.
- Existing geometry, warnings and `LimitExceeded` assertions are preserved.
  Changed call-count expectations record eliminated error reproduction and
  valid reuse: root glyph failure 5 to 4 shaper calls, base failure 3 to 2,
  ruby paint-only first-line 3 to 2 passes, and clean analysis with an existing
  unrelated warning 2 to 1 passes.

The independent ledger review found no remaining critical or important findings
after these corrections. Integration also updates the offset-mapping benchmark's
imports to match the producer's test-only `Limits` / `LimitKind` usage.

## Verification

Use native cargo `+stable`, `TMPDIR=~/tmp`, `RUSTC_WRAPPER=''`, a task-owned target
directory, two jobs and the development profile with optimization level 1 and
line-table debug data. No public API or dependency change is required.

The standalone library verification after the journal and capacity corrections
passed 1,026 tests with 8 ignored under default features, and 1,021 tests with
8 ignored under no default features. The final annotation correction passed the
shape, ruby-limit and paragraph-build suites, including its regression under
both feature configurations. Package clippy with all features and all targets
passed with warnings denied. Formatting and diff whitespace checks passed.

Local integration with SHODO-41 on the `ead4063` main baseline, including its
scalar warning-order and bounded scan corrections, passed 2,016 workspace tests
across 109 suites, with 9 ignored.
Workspace lint passed with warnings denied under both default features and
`shodo-harness/accesskit`. CI's allocation-counting group passed all 25 tests
across 15 binaries, including the updated equivalent-geometry suppressed-warning
oracle from SHODO-41. The exact-head CI evidence is recorded in the pull request.

Before publication, upstream added `FontCollection::normal_line_height` and its
regression test. SHODO-41's CI merge tree and an additional local library run
both covered that main change (1,028 library tests passed, 8 ignored), and its
actual merge tree matched the verified tree. The ledger patch was rebased onto
that merged main with an identical patch diff; the PR CI verifies the final
combined source and the extra upstream test.

Raw task logs, task-owned targets and disposable worktrees are removed after the
final results are recorded and both merged changes have been verified.
