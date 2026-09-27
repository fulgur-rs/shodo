# Caller float integration harness

## Intent and ownership

Implement shodo-p2m.12: a reusable development-only caller of pure `Paragraph::next_line`, connected to real Taffy 0.14 float placement. S0-B owns core reporting/displacement semantics; this harness owns external placement, retry, rollback and fragmentation examples. S4 retains real DOM measurements, production callbacks, browser/WPT comparisons and timing/memory. The unmerged raikiri spike is neither copied nor merged.

## Architecture

Use `dev/fixtures/examples/support/float_flow.rs`, shared by integration tests and a PNG example. Taffy is a dev dependency only. An owned cloneable checkpoint includes paragraph identity and placement epoch, token, cursor, Taffy context, placement commands and rectangles, pending reports, withdrawn-this-line records, block position, fragment index and width. Fields are private except read-only accessors. Reports require caller-supplied finite, nonnegative margin-box sizes, physical left/right side and clear; duplicate source IDs and invalid inputs produce errors.

Taffy exposes placement/clear and Clone but no removal. Reverse displacement withdraws precisely the latest report; rebuild the BFC by replaying retained placement requests in source order. Preserve earlier paragraphs' BFC entries separately from current paragraph cursors. Reset only token/cursor and line retry state at paragraph handoff. New pages carry the unconsumed height of placed floats, rebuild right floats for the new width and retain acknowledged anchors.

A line trial starts from a borrowed checkpoint and operates on a clone. Report -> place when remaining width permits, no earlier pending report exists and it was not withdrawn on this line; otherwise defer. Restart from the same token. Withdraw only the last displaced anchor and retry before considering another. Rereported withdrawn anchors are always deferred. Accept only a displacement-free line; flush pending in order below that line, then clear withdrawn records. Height rejection returns the original checkpoint with diagnostic traces. No caller-owned mutation leaks from a discarded trial.

Check a band's actual line height against all overlapping float rectangles; increase the provisional band when necessary. If an unbreakable line does not fit beside floats, try successive float-bottom positions. Distinguish these geometry retries from the `3F+1` float protocol bound at a fixed band/position. All position advances are strict and every retry has an explicit finite guard. At a page/last candidate position, allow an oversized line by an explicit unlimited-height retry. Oversized floats are permitted by Taffy's overflow rules; they are deferred below a line when they cannot fit, and following text seeks a slot below them. Zero-sized floats still affect clear/source-order via Taffy.

## Lookahead and page policy

Expose a preview of up to N accepted lines with checkpoints at every prefix. The caller chooses a prefix only after checking widows/orphans; returning the selected checkpoint commits precisely that prefix. Zero commits discard all state including float reports. For a page break, retain at least `orphans` current-page lines and at least `widows` next-page lines when feasible; otherwise move the paragraph to a fresh page once. If a fresh page cannot meet both limits, relax the limits and accept at least one line, including the explicit oversized-line retry. Tests implement this policy through the common preview/checkpoint API, including rejected lookahead and changed-width page continuation. The harness does not prescribe a production pagination policy or handle block-in-inline box layout; that outcome returns its continuation token for caller handoff.

## Evidence required

Numerically assert left/right and every clear combination; line head/middle; multiple/pending order; insufficient width and oversized floats/lines; builtin fixed 10px counterexamples `aa b[F]bbbb`, tab followed by a long span, and F1=20/F2=50 one-at-a-time withdrawal. Assert line ranges, float coordinates, cursor identity, report counts and retry bounds. Test full height rollback, nonempty pending/withdrawn checkpoint restoration, rejected and accepted preview prefixes, repeated paragraphs within one BFC, width-changing pages and block boundary handoff. Real fixture font glyphs and float rectangles also produce a deterministic PNG with pixel checks; do not claim Chrome or full WPT equivalence.

## Reference

Parley upstream `parley_tests/tests/floats.rs` was read through GitHub contents API on 2026-09-27. It demonstrates actual Taffy placement and line slot updates, but retains TODOs for nonfitting-line rollback and maximum-height handling. No source copied. The numeric contract comes from foundation design §§3.2/6.5 and existing core float tests. Tab differences from Blink are classified as implementation differences only when a browser comparison exists; none is claimed by this harness.

## Implementation evidence amendment

A direct Taffy 0.14 reproducer loses an opposite-side inset for left20×30/right20×50 followed by right10×10 clear:left: returns(90,30) overlapping right. Preserve rectangle-band insets when calling real Taffy at its public cleared threshold. Numeric matrix asserts nonoverlap, source order and clear independently; documentation retains the upstream discrepancy rather than calling it an upstream fix. A placement epoch separates reused Paragraph cursor namespaces.
