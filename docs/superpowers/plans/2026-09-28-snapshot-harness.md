# Fixed glyph snapshot harness implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Native inline execution selected under the existing autonomous issue workflow; one fresh whole-branch reviewer after implementation.

**Goal:** Detect real-glyph rendering and geometry changes with fixed snapshots, explicit updates and inspectable failure reports.

**Architecture:** Shared fixture/painter modules render accepted glyph output. A report module compares decoded pixels and geometry, stages updates and publishes PNG/HTML artifacts. A registered fixture example exposes the command and ordinary workspace checks.

**Tech Stack:** Rust1.89+, shodo-fixtures, Skrifa0.44.0, Tiny-Skia0.12.0, existing serde/serde_json/SHA256; Python standard library for real CLI regression tests.

**Spec:** docs/superpowers/specs/2026-09-28-snapshot-harness-design.md

## Global constraints

- Issue.7, base7225194da8207a8c6bf23d395defe0e05066eb25; owned target/worktrees/shodo-snapshot, feat/snapshot-harness. .6 excluded and S4 unmerged.
- No renderer dependency in the main library; Rust1.89 floor; fixture publish=false. Pin dev renderer versions0.44.0/0.12.0.
- Exact21 matrix/settings in spec. Canvas512x1024, scale1, opaque white, origin10px, fixed3OFL fonts, no system fonts, unhinted outline painting. Reject clipping/blank/synthesis/missing outlines.
- Paint actual accepted FontData/glyph IDs/coords/baseline once, never re-shape. Numerical geometry excludes process-local IDs and accompanies images.
- Zero decoded-RGBA tolerance, dimension/geometry/font/settings changes and missing/corrupt expectations fail. Normal checks never modify expectations; update complete matrix only, stage all before replacement and retain recovery on failure.
- New report output only; disjoint canonical expected/output paths, reject invalid/unknown options before writes, no unknown-file destruction. HTML escapes text; failure reports and CI artifacts remain inspectable.
- Initial agent-reviewed expectations are provenance-qualified, not represented as separately human-approved or WPT/browser conformance.

## Review focus

- An image can stay identical while whitespace/source/atomic geometry changes: Task2 geometry/fingerprint mismatch tests must fail.
- A painter can omit an accepted glyph or quietly clip an atomic: Task1 count/ink/bounds/literal second-line atomic tests must fail.
- A page move can reset source or float height: Task1 real float driver tests require continuity, fragment1 and remaining30px/right edge80px.
- Output can alias expectations through an ancestor symlink: Task2 real CLI rejects overlap before writes and preserves every original byte.
- A late render/update error can partially overwrite prior expectations: Task2 staged transaction/error tests preserve the old directory or identify a recovery backup; unknown files are preserved by rejection.

### Task 1: Fixed accepted-output renderer and case matrix

**Files:** create dev/fixtures/examples/support/snapshot_cases.rs and tests/snapshot_render.rs; modify fixture Cargo.toml only to pin renderer dev dependencies. Reuse examples/support/glyph_paint.rs and float_flow.rs without duplicating their algorithms.

**Interfaces:** `case_ids() -> Vec<String>` returns corpus12 then the spec's9 variants. `render(id: &str) -> Result<Rendered, String>` returns `Rendered { id: String, image: tiny_skia::Pixmap, settings: serde_json::Value, geometry: serde_json::Value, glyph_count: usize }`. Canvas/font/renderer fingerprint is serialized by `conditions() -> serde_json::Value`. Geometry and settings travel with each rendered case into Task2.

- [x] Write tests first: exact21 unique IDs, unknown ID error; real corpus images contain glyph ink with paint count equal to accepted count; separately loaded fonts yield byte-identical pixels/geometry. Literal structural tests: ffi glyphcount1/first-owner red/middle blue source annotation; four nesting levels and20x20atomic/baseline16 on second line; pre-wrap literal `One  two\tthree\nFour five.` source continuity/tab16; indent10; Arabic multiple lines; Japanese accepted ranges; float fragment1/remaining30/right80/continuous source and one unchanged-token height rejection. Synthetic output and out-of-canvas output must be explicit failures, not empty golden images.
- [x] Run `cargo +stable test --offline -p shodo-fixtures --test snapshot_render`; observe missing renderer/API RED, and functional RED for any contract not yet implemented. Record actual command/exit before coding.
- [x] Implement the spec's fixed builders/caller protocol and painter adapter. Validate each accepted font against fixture bytes, count actual glyphs, retain geometry and numerical page placements, bound retries. Composite float page panels separately in the fixed canvas; do not overlap page origins or silently clip. Pin resolved renderer dependencies. Keep existing painter tests and examples intact.
- [x] Verify focused renderer tests plus `cargo +stable test --offline --workspace`, inspect representative actual PNGs using view_image, and commit Task1. No new expected images are blessed by this task.

### Task 2: Pixel/geometry comparison, safe updates, CLI and CI reports

**Files:** create examples/support/snapshot_report.rs, examples/snapshots.rs, tests/snapshot_report.rs, tools/test_snapshots.py, dev/fixtures/snapshots/{manifest.json,21PNG,21geometryJSON}, docs/snapshot-tests.md; modify dev/fixtures/Cargo.toml to register snapshots example test=true and .github/workflows/ci.yml to run/upload reports.

**Interfaces:** consume Task1 Rendered/conditions/case_ids/render. `compare_images(expected: &Pixmap, actual: &Pixmap) -> Result<Difference, String>` returns changed pixel count, dimension equality and magenta/white diff Pixmap. `Options { expected: PathBuf, output: PathBuf, case: Option<String>, update: bool }`; `parse_args(args: impl IntoIterator<Item=OsString>) -> Result<Options, String>`. `run(options: &Options) -> Result<Report, String>` produces case statuses, PNG triples, geometry diagnostics and escaped index.html; `Report::passed()` drives CLI exit status. An internal render dependency may be injected to exercise late failure, while the CLI always uses Task1's real renderer.

- [x] Write pixel/report tests first with literal1x1RGBA fixtures: identical passes; one changed pixel has count1/magenta diff; dimensions differ; PNG decode failure/missing image fails; same pixels but changed geometry/font/settings fail. Stage a late render failure and update-file failure, asserting original expected bytes or explicitly recoverable backup and no false success; unknown expected files reject update and survive. Run tests and retain RED before implementing comparator/publication.
- [x] Implement comparison/expected manifest and safe staging/update/recovery, canonical path checks, case selection, PNG triples and escaped HTML. Default check reads expectations only; full update is explicit and refuses --case. Comparison/render failures produce diagnostic report rows and nonzero CLI; preserve existing output directories. Add registered example with an ordinary test checking the committed matrix; use a unique test report directory under the target output root.
- [x] Write/run Python real CLI tests: build the example once via Cargo JSON artifact selection; explicit full --update creates copied expectations, normal repeat passes; mutate one expected pixel and delete one expected image, assert nonzero plus actual/diff/HTML and immutable remaining expected bytes; missing baseline check creates none; unknown IDs/options, existing output and canonical/symlink overlaps preserve originals. Test update+case rejection. Run RED before implementing missing CLI behavior, then all Python fixture tests GREEN. Do not assert on mocked subprocess outputs.
- [x] Explicitly generate initial21 expectations with the update command after renderer tests pass. Inspect a montage and individual ffi/Arabic/tab/Japanese/atomic/page cases, record provenance and limitations, then ordinary check must pass. Deliberately corrupt a copied expectation and verify CLI failure/artifact triples without touching the committed originals.
- [x] Document check/HTML inspection/update/review, scope/font/version/canvas/pixel rules and agent-reviewed initial provenance. CI runs the ordinary report command with no update flag and uploads its output with `if: always()`; workspace/MSRV tests check expectations too. Preserve existing CI and numerical browser comparison checks.
- [ ] Verify stable/MSRV full workspace, Clippy all-targets, docs warnings denied, fmt/diff, fixture+benchmark Python, regeneration and strict browser comparison, no-default and wasm. Commit Task2. Dispatch one fresh whole-branch reviewer on immutable HEAD; fix material findings once with RED/GREEN/full suite, defer minors, record rulings. Push/create PR, verify exactHEAD check/msrv/wasm success, merge/readback/containment, close .7 and archive/clean owned worktree/localbranch. Re-run bd ready and continue; do not mark the overall goal complete merely because .7 is done.
