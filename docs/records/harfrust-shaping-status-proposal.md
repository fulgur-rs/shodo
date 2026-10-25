# Draft: expose shaping completion status in harfrust

Update (2026-10-25): Shodo now uses HarfRust 0.15.0. Its new lookup
preprocessing budgets and malformed-lookup-type rejection do not bound the
shaped output of valid MultipleSubst tables more tightly. This dependency
update does not resolve SHODO-23, which remains deferred by user decision.
The update enables `smallvec`'s union representation through `read-fonts` 0.46,
reducing `fontique::FontInfo` from 96 to 88 bytes on 64-bit targets. The local
font-index byte model now charges 88 bytes for that metadata; its existing layout
test checks the new value.
HarfRust now includes CBDT bitmap ink in fallback vertical-origin calculations.
For the fixed 24px color-emoji fixture, its 1898-unit top and 2405-unit height
center in the 2400-unit horizontal line at y=1895, instead of falling back to
the 1900-unit ascender. The independent vertical bitmap placement test records
the resulting 1/16px shift toward the inline start; its bitmap data, painter and
pixel tolerance are unchanged.
The full fixed snapshot check (`cargo run -p shodo-harness --example snapshots
-- --output <new-directory>`) passed all 46 cases with zero changed pixels and
no baseline updates. Its temporary report was removed after verification.

Update (2026-10-13): Shodo now uses HarfRust 0.14.0, whose public
`Buffer::allocation_successful()` reports failure of internal buffer, operation,
or nesting limits. `shape()` returns `ShapeError` for invalid API usage; resource
limit failures use the buffer query. The proposal below describes 0.12.0 and is
retained as historical context. This dependency update does not add a new
interruption policy to Shodo. HarfRust 0.14 still has no caller-selected glyph
ceiling, and its internal ceiling remains `max(256 * input_len, 65_536)`.
SHODO-23 is deferred by user decision; this update does not resolve that finding.

Migration checks: HarfRust 0.14 centers CFF ink when VORG is absent, so Shodo's
existing outline-top-plus-vmtx correction now subtracts the actual shaping
origin. Both `shape::tests::missing_vorg` cases retain their expected positions.
The fixed-font, 120-word Latin fixture in
`line::tests::edge_window_shapes_are_reused_across_candidates_and_relayouts`
now needs 114 distinct edge shapes on its first 200px layout. HarfRust 0.14
marks unsuccessful pair-positioning matches unsafe to concatenate; the earlier
test's fewer-than-60 assumption no longer holds. The test checks one shape per
cache entry and zero additional calls on an identical second layout instead.
These counts describe this fixture, not a general performance guarantee.
The first-line intrinsic budget-sharing fixture likewise uses a 32-byte window
cap instead of 16: the single pass fits, while two passes still exhaust their
shared operation budget. The production budget policy is unchanged.

The allocation-counting `ruby_base_caret_range` fixture now gives preparation
an 8192-byte reshape window cap instead of the 4096-byte default. Its hidden,
four-level, 256-character base otherwise reaches the operation budget before
caret ownership is measured. The warning-free preparation and original caret
allocation assertions remain. Direct shaping of the pinned Latin font with
`PRODUCE_UNSAFE_TO_CONCAT`, default features, and 256 repeated `A`, `B`, or `C`
characters produces identical glyph IDs, clusters, advances, and offsets in
0.12 and 0.14. For `B`, the unsafe-to-concat count increases from 1 to 256.
Disabling kerning changes `C` advances, so the fixture preserves its features
and increases only its explicit preparation budget. With production defaults,
some long inputs can now reach the existing reshape fallback sooner.

This is an unsent design note, not an upstream issue or a claim that a particular
font triggered a shaping failure. It records the integration limitation observed
in the locally inspected harfrust 0.12.0 source used by shodo.

## Problem

`Shaper::shape(UnicodeBuffer, ShapeOptions)` returns `GlyphBuffer`. The public
buffer exposes glyph info/positions, length and allocation reuse through `clear`,
but no shaping completion status. Internally, `hb_buffer_t` has `successful` and
operation accounting, and some paths clear `successful` when work is exhausted.
That private field is not an application contract. A nonempty or empty glyph
buffer cannot reliably distinguish a complete shape from interrupted work;
controls/default ignorables may legitimately produce unusual or empty output.

shodo therefore does not invent a “harfrust failed” warning from output length,
missing glyphs, glyph counts or elapsed time. It validates font structures, bounds
input runs, checks output count before copying into retained glyph storage, bounds
run pen arithmetic, and falls back with warnings where its own reshape budgets
cannot provide a safe cut. The output check happens after harfrust returns and
cannot bound every temporary allocation inside that dependency. Process allocation
failure/panics are also distinct from a recoverable shaping status.

## Suggested API to discuss

Add a completion-status query to the returned buffer, without changing existing
`shape` callers. For example, `GlyphBuffer::status()` could return a non-exhaustive
status with complete and interrupted states; a specific operation-budget reason
should be exposed only if the implementation can identify it reliably. Avoid
promising a recoverable out-of-memory error for ordinary Rust allocation paths.

Document whether interrupted output is partial, safe to render, safe to discard,
and whether `clear()` returns reusable scratch that resets the status for the next
shape. Propagate the status through both implicit-plan and supplied-plan paths.
An optional explicit work/output budget could be discussed separately from this
observation API; it must preserve ordinary behavior when absent.

## Evidence and acceptance questions

The relevant 0.12.0 sources are `src/hb/face.rs` (`Shaper::shape` and plan path) and
`src/hb/buffer.rs` (`hb_buffer_t::successful`, operation accounting, and public
`GlyphBuffer`). This note does not assume that a later release has the same API.

Useful upstream tests would force an internal work-limit interruption, distinguish
it from legitimate empty output, verify ordinary GSUB/GPOS completion, cover both
plan paths, and verify status reset when reusing cleared scratch. shodo could then
report an actual completion failure and apply a documented fallback without
mistaking valid font behavior for failure.
