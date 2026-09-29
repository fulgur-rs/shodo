# Fixed glyph snapshots

The snapshot harness renders accepted shodo glyph IDs, retained font data,
variation coordinates, positions and baselines with Skrifa 0.44.0 and Tiny-Skia
0.12.0. It does not shape the text again. Rendering dependencies belong to the
unpublished fixture crate, not the main library.

Run a full check from the repository:

```sh
cargo run -p shodo-harness --example snapshots -- --output target/snapshot-report
```

Open `target/snapshot-report/index.html` to inspect expected, actual and magenta
pixel differences beside each other. `report.json` contains case status and
changed pixel counts. Geometry links retain line source ranges, glyph ownership,
positions, atomics, inline boxes, annotations and page/float placements.
Output must be a new directory; choose another name for subsequent runs.

The full matrix has 46 cases: 26 horizontal cases and 20 vertical/sideways cases
with both directions, mixed/upright/sideways orientation and combined text.
Vertical geometry retains physical origins, public glyph matrices and the
source ranges/squares of combined text. See [vertical output](vertical-layout.md).

For a quick partial check, append `--case shared-ffi-color`. The report labels
this as partial. `--expected /path/to/copied-expectations` checks a separate
baseline. The default expectation path belongs to the fixture package and does
not depend on the command's current directory.

A missing/corrupt image, changed decoded pixel, changed dimensions, geometry,
case settings or font/renderer conditions fails the command. PNG compression
bytes are not compared. Geometry JSON retains exact floating point values;
there is no pixel or geometry tolerance. Normal checks never create or rewrite
expectations. Failure reports retain actual/difference images and diagnostics;
an absent expectation cannot provide an expected image.

## Intentional updates

Review the layout change first, then generate a complete replacement explicitly:

```sh
cargo run -p shodo-harness --example snapshots -- --output target/snapshot-update --update
```

Inspect the HTML, PNGs, manifest and geometry changes in
`dev/fixtures/snapshots`, run an ordinary check with a new output directory,
and include the expectations in the same reviewed PR as the change.
`--update` cannot be combined with `--case`. All 46 cases must render before
replacement. The command stages new files, backs up old expectations and
restores them on publication failure; a recovery failure names the backup.
Unknown files in an expectation directory cause update refusal, preserving
caller files. Expected and output paths must be disjoint even through symlinks.
Run updates exclusively; simultaneous updates are outside this harness's
contract.

## Fixed coverage and provenance

The matrix contains the shared 12 corpus cases and 14 structural cases:
shared ffi ownership/color/source underline, connected Arabic across wrapped
lines, four nested inline boxes and a second-line atomic baseline, preserved
tabs, normal and hanging whitespace, first-line indent/baseline, Japanese
punctuation, five Japanese trim/hanging/justification cases, and a float continued into a new page fragment. Each image uses a
512 by 1024 opaque white canvas, scale 1 and a 10px origin margin. The manifest
records renderer versions, font SHA256/face index and case settings; geometry
records the accepted output. Three OFL fixture fonts are loaded with system
font discovery disabled.

Initial expectations are explicitly generated and visually reviewed by the
implementation agent and committed through the PR workflow. This does not
claim separate human approval. These are regression snapshots, not browser or
WPT conformance results. The existing numerical browser comparison remains an
independent check.

Rendering covers horizontal monochrome TTF/CFF outlines and caller-painted
atomic/float rectangles and source annotations. Synthetic weight/skew and
missing outlines fail. General CSS decoration/DOM, vertical text, color/bitmap
glyphs and arbitrary webfonts are outside this fixed matrix. Arabic uses a
48px line height at 32px to make connected wrapped lines inspectable; it does
not exercise intentional overlapping lines. Japanese uses characters present
in the pinned CJK subset.

`cargo test --workspace` checks the committed matrix on stable and MSRV. Its
reports are retained beneath the Cargo target directory in `snapshot-checks`.
Python CLI tests build the actual example once and exercise explicit update,
normal check, a single changed pixel, missing images, invalid arguments and
path protections using temporary copied expectations. CI runs an additional
ordinary report command and uploads its output even on failure. CI never
updates expectations.
