# Draft: expose shaping completion status in harfrust

This is an unsent design note, not an upstream issue or a claim that a particular
font triggered a shaping failure. It records the integration limitation observed
in the locally inspected harfrust 0.12.0 source used by shodo S2.

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
mistaking valid font behavior for failure. Sending this proposal requires a
separate user instruction.
