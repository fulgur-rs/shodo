# Documentation

The [project README](../README.md) covers installation, a minimal example,
features, and current limitations. The guides here describe implemented APIs and
their integration contracts. For API reference, run
`cargo doc -p shodo --no-deps --open` from the repository root; add
`--features accesskit` to include the optional adapter.

## Using shodo

| Topic | Guide |
| --- | --- |
| Fonts, incremental layout, output lifetime, limits, and hit testing | [Integration guide](guides/integration.md) |
| Exact caller-resolved normal and `::first-line` styles | [First-line style contract](guides/first-line-style-contract.md) |
| Shared ligatures and Arabic glyph ownership across source nodes | [Shared glyph contract](guides/shared-glyph-contract.md) |
| Line metrics, spacing, source positions, and regression evidence | [Horizontal output contracts](guides/horizontal-layout-contracts.md) |
| Japanese breaks, punctuation spacing, hanging, and justification | [Japanese horizontal layout](guides/japanese-layout.md) |
| Vertical/sideways glyphs and combined text | [Vertical output](guides/vertical-layout.md) |
| Paired readings, coordinated wrapping, source metadata and retained painting | [Ruby layout](guides/ruby.md) |
| Retained colors, underlines, and strike-through | [Text paint](guides/paint-styles.md) |
| Outline painting and caller-sized atomic inlines | [PNG rendering sample](guides/png-render-sample.md) |
| Emoji sequences, font fallback, and caller color drawing | [Emoji layout](guides/emoji.md) |
| Retained text, source anchors, and the optional AccessKit adapter | [Accessibility output](guides/accessibility.md) |
| Caller float placement, withdrawal, and page checkpoints | [Float integration harness](guides/float-integration-harness.md) |

## Developing and validating changes

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for development setup and checks.
Commands in these guides run from the repository root. `docs/dev/` holds
developer-facing verification procedures; the crates behind them are
[`dev/fixtures`](../dev/fixtures/README.md) (data), [`dev/harness`](../dev/harness)
(rendering/snapshot/browser/AccessKit checks), and [`dev/raikiri`](../dev/raikiri)
(raikiri-integration checks).

| Topic | Guide |
| --- | --- |
| Fixed font assets, original test cases, and reproduction | [Development fixtures](../dev/fixtures/README.md) |
| Exact image/geometry checks and intentional baseline updates | [Snapshot tests](dev/snapshot-tests.md) |
| Saved Chrome measurements, known differences, and recollection | [Browser comparison](dev/browser-comparison.md) |
| Standalone release timing and allocation measurements | [Performance harness](dev/performance-measurements.md) |
| Internal module boundaries and required regression checks | [Module reorganization](dev/module-reorganization.md) |

Snapshot and browser checks cover fixed cases; they do not certify general
CSS or WPT conformance.

## Records

Measurement and diagnostic records in `docs/records/`; they describe the
recorded machine, toolchain, and scope at the time, not a live guarantee.

| Topic | Record |
| --- | --- |
| Earlier shaping measurements and their scope | [Shaping measurement record](records/shaping-measurements.md) |
| `ch`/`ic` unit-resolution cost before and after PR #87 | [Font-unit cost record](records/ch-unit-cost.md) |
| Frozen raikiri S4 residual fields, all109 document classification and caller gaps | [raikiri style diagnostics](records/raikiri-style-diagnostics.md) |
| Representative caller wiring of hanging-punctuation none/first | [raikiri hanging-punctuation](records/raikiri-hanging-punctuation.md) |
| Representative caller ownership split of flow/paint CSS | [raikiri style handoff](records/raikiri-style-handoff.md) |
| Reproduction and stage attribution of the saved S4 candidate increase | [raikiri jt1 regression](records/raikiri-jt1-regression.md) |
| An unsent proposal concerning upstream shaping status | [harfrust status proposal](records/harfrust-shaping-status-proposal.md) |

## Design history (`docs/superpowers/`)

The [foundation design](superpowers/specs/2026-09-26-shodo-foundation-design.md),
[design specifications](superpowers/specs/), and
[implementation plans](superpowers/plans/) preserve the project's design history,
mostly in Japanese. They can describe planned work, superseded choices, or APIs
that have since changed. Check current guides, source, and tests before treating
a design document as an implemented contract. This directory's name and layout
are fixed by the superpowers plugin's own conventions and are not reorganized
along with the rest of `docs/`.
