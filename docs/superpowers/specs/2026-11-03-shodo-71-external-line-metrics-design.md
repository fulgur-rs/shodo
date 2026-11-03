# SHODO-71: external line metrics

RAIKIRI-1435 needs an outside image marker to enlarge the selected body line,
without inserting an atomic inline into the body's text stream. Marker sizing,
selection of the relevant paragraph/line, and page placement remain caller work.

## Approaches

- A zero-width atomic would reuse existing sizing but changes whitespace,
  break opportunities, source ranges, and empty-line behavior. Reject it.
- A paragraph-wide strut override would affect continuations and require a
  rebuild when the external size changes. Reject it.
- Add optional per-line external metrics to `LineConstraint`. This is the
  implementation choice for the previously agreed shodo/caller responsibility
  split. It supports retries and caller-selected first lines without text edits.

## Contract

`ExternalLineMetrics { line_over: f32, line_under: f32 }` describes nonnegative
occupied extents from the root dominant baseline (`Line::metrics().baseline`),
in line-relative directions. Over is block-end in vertical-lr. The caller
converts its external object's baseline to this baseline before supplying it.
`LineConstraint.external_metrics: Option<ExternalLineMetrics>` defaults to None.
Some zero metrics still contributes the baseline point; it is not a synonym for
None when a line has negative half-leading.

The extents participate before top/bottom-aligned groups are positioned, in both
the ordinary and emphasis-free line profile. They apply only to existing
nonempty lines, including forced breaks, and neither create a line nor make an
empty line nonempty. They do not change widths, breaks, text, fragments, source
mapping or intrinsic sizes. They do change baseline, block size, aligned block
positions, height acceptance and annotation leading reservations. Ruby uses
the enlarged parent line; its annotation lanes do not inherit the input.

An anonymous base with a visible reading becomes nonempty only after the child
lines are materialized. In that exceptional case, reuse the retained children's
actual visibility/emptiness to remeasure the parent profiles before solving ruby
geometry with a new Frame. This does not rebuild child lines or add candidate
queries. Hidden or collapsed readings alone do not activate external metrics.

The completed-line retry key includes both extents. Width-only scan caches and
break plans remain usable. Summary trials fall back to full materialization
when external metrics are supplied. Invalid values follow existing nonnegative
layout-length sanitation and warning rules. Existing struct literals that use
`LineConstraint::new` updates remain compatible; exhaustive literals need the
new field.

## Verification

Use deterministic missing-font public API tests for geometry, extents smaller
than the root, continuation selection, empty/done/block/float events, forced
breaks, first-line styles, height rejection/retry, top/bottom alignment, all
writing modes, negative leading, invalid values and break plans. Use a fixed-font
internal cache regression to prove changed extents invalidate retained lines.
Test annotation occupied-space reporting and ruby integration. Run the workspace
suite, feature-isolation suites, formatting, Clippy and rustdoc checks.

Sol and Astra independently review the API and final patch. Both identified
that annotation leading must account for the occupied external extents.
Astra also found the anonymous-base visible-reading case; its regression failed
before the parent remeasurement and passed afterward, including nested readings.
