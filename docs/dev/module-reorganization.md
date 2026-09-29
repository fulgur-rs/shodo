# Internal module reorganization

Phase 4 of `shodo-c6r` keeps one public `shodo` crate and splits large files
inside it. `shodo-c6r.5` records the inventory and individual implementation
issues; completing that inventory does not mean the splits are implemented.
The live issue status is available through `bd show <id>` or
`bd list --parent shodo-c6r.5 --all`.

## Inventory and implementation issues

This inventory is from commit `63b5ab8c58f4324df7508dd0800e5a01d708a462`, after
the workspace reorganization. Counts include inline tests. Reinspect the latest
main and active worktrees before starting a split; these are recorded counts,
not limits on future code growth.

| Issue | Priority | Source | Lines | Initial responsibility boundary |
| --- | --- | --- | ---: | --- |
| `shodo-c6r.5.1` | P3 | [shape.rs](../../crates/shodo/src/shape.rs) | 1,566 | Extract the existing `tests` module to `shape/tests.rs`; retain shaping/storage entry points and the existing cache/features/instance/orientation modules. |
| `shodo-c6r.5.2` | P3 | [output.rs](../../crates/shodo/src/output.rs) | 1,290 | Separate Line operations from glyph/cluster views and iteration; preserve public types, exports, and the existing paint/ruby modules. |
| `shodo-c6r.5.3` | P3 | [paragraph.rs](../../crates/shodo/src/paragraph.rs) | 1,139 | Separate paragraph construction/finalization and first-line cursor preparation from retained data and public input/result types. |
| `shodo-c6r.5.4` | P3 | [font/matching.rs](../../crates/shodo/src/font/matching.rs) | 1,205 | Extract `matching_tests` and `retention_tests` into `font/matching/`; retain matching, cache, fallback, and public font paths. |
| `shodo-c6r.5.5` | P4 | [line/metric_index.rs](../../crates/shodo/src/line/metric_index.rs) | 1,200 | Separate scalar metric queries and content geometry queries with their shared summary state; extracting tests alone does not complete this split. |
| `shodo-c6r.5.6` | P4 | [line/windows.rs](../../crates/shodo/src/line/windows.rs) | 1,585 | Extract `tests` and `edge_shape_cache_tests` into `line/windows/`; retain edge-window entry points, cache ownership, and reshape budgets. |

The shaping and font-matching files each have a substantial existing test
section that can move without exposing new production helpers. Output and
paragraph construction contain distinct responsibilities behind existing public
entry points. Metric queries share selection and geometry state, so that split
requires closer attention to the common boundary. Windows also overlaps ongoing
edge-cache/performance work: check whether that work has merged before choosing
the implementation base. The P4 assignments describe sequencing, not a known
correctness defect or an exemption from the checks below.

## Required contracts for every split

Every issue carries the same requirements, with its own regression focus:

- Preserve the public API: root `pub mod`/`pub use`, public types, methods and
  traits, and font-module paths. Keep features, dependencies, Rust 1.89.0, and
  Wasm support unchanged.
- State every `pub(crate)` addition/removal in the PR, including an explicit
  zero when unchanged. Explain affected symbols and scope changes, including
  `pub(super)`. Prefer private child modules; do not widen an internal API just
  to make a split compile.
- Preserve existing tests and their coverage. Check whether moving a module
  changes a test's qualified name or a CI filter. Shared unit-test font data
  remains in [test_support](../../crates/shodo/src/test_support/fonts.rs);
  rendering and real-font public integration tests follow
  [CONTRIBUTING.md](../../CONTRIBUTING.md).
- Pass `cargo test --workspace` and
  `cargo test --workspace --features shodo-harness/accesskit`.
- Produce warning-free documentation with
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` and the same
  command with `--features shodo-harness/accesskit`.
- Pass the current [CI workflow](../../.github/workflows/ci.yml), including
  formatting, Clippy, feature-isolation tests, MSRV, Wasm, and fixed snapshots.
  Investigate a changed result instead of rewriting its baseline to hide it.
- Keep both saved S4 integration spikes separate; do not modify, push, create
  PRs for, or merge them as part of this reorganization.

The workspace command includes the development crates; plain `cargo test`
selects only `crates/shodo` and is insufficient for the workspace requirement.
A file move does not establish a speedup, lower allocation cost, or increased
WPT coverage.

## Regression focus

| Area | Contracts to retain |
| --- | --- |
| Shaping | Arabic joining/safety flags, CFF vertical origin, run pen limits, negative advances, expanded transforms, resolved-instance sharing, and scratch release. |
| Output | Shared/owned glyphs, shaping advances versus layout spacing, RTL/vertical/Combined coordinates, cluster ownership, ruby views, retained painting, and hit/selection geometry. |
| Paragraph | Limits and warnings, font generations, offset mapping, normal/alternate first-line cursors, retained ownership and Send + Sync, atomic revisions, bidi, vertical text, and ruby construction. |
| Font matching | Script overrides, LRU eviction and generation invalidation, metadata/color reuse, and platform face/blob identity and retention bounds. |
| Metric index | Selected ranges, owned/atomic replacements, top/bottom alignment and baselines, inline-box paint/content bounds, ruby geometry, and agreement with retained output. |
| Edge windows | Soft hyphens and opposite edges, unshapeable interiors/remainders, Combined text beside owned ligatures, different-font unsafe edges, cache entry/cost caps, per-line reshape budgets, warnings, and shrink release. |
