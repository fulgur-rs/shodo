# Documentation

The [project README](../README.md) covers installation, a minimal example,
features, and current limitations. The guides here describe implemented APIs and
their integration contracts. For API reference, run
`cargo doc -p shodo --no-deps --open` from the repository root; add
`--features accesskit` to include the optional adapter.

## Using shodo

| Topic | Guide |
| --- | --- |
| Fonts, incremental layout, output lifetime, limits, and hit testing | [Integration guide](integration.md) |
| Exact caller-resolved normal and `::first-line` styles | [First-line style contract](first-line-style-contract.md) |
| Shared ligatures and Arabic glyph ownership across source nodes | [Shared glyph contract](shared-glyph-contract.md) |
| Line metrics, spacing, source positions, and regression evidence | [Horizontal output contracts](horizontal-layout-contracts.md) |
| Japanese breaks, punctuation spacing, hanging, and justification | [Japanese horizontal layout](japanese-layout.md) |
| Vertical/sideways glyphs and combined text | [Vertical output](vertical-layout.md) |
| Paired readings, coordinated wrapping, source metadata and retained painting | [Ruby layout](ruby.md) |
| Retained colors, underlines, and strike-through | [Text paint](paint-styles.md) |
| Outline painting and caller-sized atomic inlines | [PNG rendering sample](png-render-sample.md) |
| Emoji sequences, font fallback, and caller color drawing | [Emoji layout](emoji.md) |
| Retained text, source anchors, and the optional AccessKit adapter | [Accessibility output](accessibility.md) |
| Caller float placement, withdrawal, and page checkpoints | [Float integration harness](float-integration-harness.md) |

## Developing and validating changes

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for development setup and checks.
Commands in these guides run from the repository root.

| Topic | Guide |
| --- | --- |
| Fixed font assets, original test cases, and reproduction | [Development fixtures](../dev/fixtures/README.md) |
| Exact image/geometry checks and intentional baseline updates | [Snapshot tests](snapshot-tests.md) |
| Saved Chrome measurements, known differences, and recollection | [Browser comparison](browser-comparison.md) |
| Standalone release timing and allocation measurements | [Performance harness](performance-measurements.md) |
| Earlier shaping measurements and their scope | [Shaping measurement record](shaping-measurements.md) |
| An unsent proposal concerning upstream shaping status | [harfrust status proposal](harfrust-shaping-status-proposal.md) |

Performance records describe the recorded machine and toolchain. Snapshot and
browser checks cover fixed cases; they do not certify general CSS or WPT
conformance.

## Design history

The [foundation design](superpowers/specs/2026-09-26-shodo-foundation-design.md),
[design specifications](superpowers/specs/), and
[implementation plans](superpowers/plans/) preserve the project's design history,
mostly in Japanese. They can describe planned work, superseded choices, or APIs
that have since changed. Check current guides, source, and tests before treating
a design document as an implemented contract.
