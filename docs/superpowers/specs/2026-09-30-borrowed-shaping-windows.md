# Borrowed shaping windows (shodo-sbp.7)

Remove the per-window scalar Vec and FontMatch/before/after clones in shape_items_with_base_scopes. Retain original ShapeItems for line-edge reshaping. Read immutable metadata from the original item and borrow its selected scalar slice; only the window end is window-specific. Pre/post context selection already uses the original item or adjacent original scalars and must remain unchanged.

Preserve source/glyph/geometry, warnings and limits: missing fonts, giant grapheme and scalar budget progress, glyph pen splitting, ruby base budget, RTL, variation/optical sizing, first-line and edge reshape. Preserve separate owned shape_window_edit input transformation. No performance dependency on switching to raikiri and no changes to saved S4 spikes.

Measure the change independently in build time and allocation calls/gross/net/peak with fixed fonts and matching configuration. Keep uninstrumented time separate from allocation instrumentation and distinguish retention/RSS. Validate all 54 fixture outputs against sbp5; no unmeasured general speed guarantee.
