# SHODO-13: Skrifa 0.48 upgrade

Date: 2026-10-12. Baseline: `c64b8b3` (SHODO-6 merged).
This record covers the shodo repository, not deployment of a new raikiri release.

## Decision and implementation

Upgrade the shared workspace requirement from Skrifa 0.44 to 0.48 and lock
Skrifa 0.48.0, read-fonts 0.45.0 and font-types 0.12.6. Every workspace member
using `skrifa.workspace = true`, including the local rendering harness, follows
the new version. Skrifa/read-fonts require Rust 1.85; the existing shodo MSRV
remains 1.89. No unrelated dependency requirement is changed.

The published upstream tag is
[skrifa-v0.48.0](https://github.com/googlefonts/fontations/tree/skrifa-v0.48.0),
commit `0e939934004776b1982982428a7ce748e61b0310`. Its
[COLRv1 traversal](https://github.com/googlefonts/fontations/blob/skrifa-v0.48.0/skrifa/src/color/traversal.rs)
has a 16,384-node work budget. This update makes the upstream implementation
available to shodo's direct consumers; it is not an audit of all font processing
or proof that a downstream renderer uses this version.

read-fonts now returns optional deltas rather than `Result` and names VVAR
accessors `advance_delta` and `v_origin_y_delta`. Adapt MVAR/VVAR and GDEF
callers without changing missing/unreadable-data fallback. GDEF carets, manual
CFF vertical-origin corrections and `ic-height` size-adjust retain the previous
whole-design-unit rounding through `F48Dot16::to_i32()` before scaling. This
matches the old variation-store accumulator's `(sum + 0x8000) >> 16` rule;
fractional deltas are not newly exposed by these shodo helpers.

Harfrust still uses read-fonts 0.41. Its `FontRef` cannot use Skrifa's new
`TableProvider` trait. Read the coordinate-storage axis count from the already
opened Skrifa metric face for the same bytes and index, retaining the <=11-axis
check and avoiding another face open or a dependency upgrade solely for a trait.
The existing VVAR test follows the renamed upstream accessor.

## Dependency scope and raikiri follow-up

The workspace lockfile intentionally contains both Skrifa 0.44 and 0.48:

- Local shodo, fixtures, harness and the integration example's direct metadata
  reader use 0.48.0/read-fonts 0.45.0.
- Pinned Parley 0.11.1 and the git-pinned raikiri comparison dependency, including
  Glifo 0.2.0, still require Skrifa 0.44.0. The pinned raikiri revision is
  `fe9aea9ade56ff046d38a6dddd31d02303466bfc`; its old shodo 0.0.18 is a separate
  comparison implementation, not the workspace library.
- Fontique 0.11.1 and Harfrust 0.12.0 retain read-fonts 0.41.0 even in the
  published shodo graph. This change does not upgrade those independent parsing
  paths. font-types remains a single shared 0.12.6 version.

`cargo update -p skrifa --precise 0.48.0` initially failed because it also tried
to update consumers constrained to ^0.44. Resolving the edited workspace
requirement through Cargo instead adds the necessary second version; the
lockfile diff is limited to these font packages and version-qualified edges.

Current raikiri main was checked separately at
[`7cd90e0`](https://github.com/fulgur-rs/raikiri/blob/7cd90e0eace223883fac579a7286d434ee05160c/Cargo.toml).
It pins shodo `=0.0.26` and directly uses Skrifa 0.44. After a shodo version
containing this change is published, update both that exact shodo pin and
raikiri's direct Skrifa requirement. Updating shodo alone does not update
raikiri's own metadata calls.

Actual glyph painting also goes through Anyrender's backend. Current raikiri
uses anyrender_vello_cpu 0.16 and vello_cpu 0.1; the inspected backend dependency
chain is Glifo 0.2 -> Skrifa 0.44. Check and update that chain separately before
claiming that COLRv1/outline hardening reaches raikiri's glyph renderer.
Adding a new direct Skrifa version cannot redirect an older backend's calls.
The shodo-local Tiny-Skia harness does use the updated direct Skrifa.

## Validation and snapshot provenance

On Linux x86_64, Rust 1.97.1, with `RUSTC_WRAPPER=""`, dev opt-level 1 and
line-tables-only debug information:

- `cargo +stable test --workspace`: 1,796 passed, 0 failed, 9 ignored.
  Includes real variable-font metrics, VVAR size-adjust/public advances, GDEF
  variation carets, CFF vertical origins, registration and decoding limits,
  and the complete 46-case fixed image/geometry snapshot matrix.
- Default and `shodo-harness/accesskit` workspace Clippy with all targets and
  `-D warnings`: both pass.
- The snapshot renderer condition changes from `skrifa-0.44.0` to
  `skrifa-0.48.0`. No expected PNG or geometry JSON is regenerated or relaxed.
  An ordinary complete comparison with the updated condition passes all 46
  cases (`partial=false`, `updated=false`, `passed=true`, 46 `match` statuses).
- `cargo +stable fmt --all --check` and `git diff --check`: pass.

The manifest edit is provenance only. Fixed fixture equality does not promise
identical behavior for every variable or malformed font, and no performance
improvement is claimed. Required CI additionally exercises MSRV, Wasm,
feature configurations and rendering on the CI hosts before merge.

Reproduce with `TMPDIR="$HOME/tmp"`, a task-owned `CARGO_TARGET_DIR`, the profile
settings above, and the workspace test/Clippy commands. Then run
`cargo +stable run --locked -p shodo-harness --example snapshots -- --output
<new-task-directory>` for the 46-case report. `--update` is not needed.
Temporary build logs and reports are removed after their results are recorded.
