# Accessibility output

`shodo::accessibility::AccessibleLayout` exposes retained paragraph output to
assistive technology. It is always available and has no accessibility backend,
editor, IME, DOM or platform dependency. The optional `accesskit` feature adds
an AccessKit data/position adapter. Public APIs remain experimental.

## Core data

Build `AccessibleLayout::new(&accepted_lines)` after line layout completes.
It borrows the supplied Lines and builds one shared hit index. Lines retain the
paragraph/font data even after the original Paragraph, LayoutContext and font
handles are dropped. Keep the Lines alive while reading the accessible output.

`logical_text()` concatenates the lines' covered processed text in the supplied
reading order, with no separator at soft wraps. It preserves hard breaks, tabs,
soft hyphens and bidi controls. Callers can apply a separate speech policy to
nonpainting characters. It does not reconstruct text from glyphs or visual run
order. Each line uses its own text dataset: `::first-line` transforms can change
UTF-8 lengths relative to the normal dataset used by subsequent lines.

| Type | Data |
| --- | --- |
| `AccessibleLine` | `index`, borrowed `text`, dataset-relative `text_range`, `characters`, `runs`, `word_starts`, `writing_mode`, used `direction`, `break_reason`, logical `bounds` |
| `AccessibleCharacter` | borrowed `text`, UTF-8 `text_range`, `kind` (`Text`, `HardBreak`, `Atomic(NodeId)`), logical `rect`, logical `leading`/`trailing` caret points |
| `AccessibleRun` | `character_range`, UTF-8 `text_range`, source `node`, borrowed resolved `style`, actual `font: Option<FontId>`, actual `font_size`, `bidi_level`, glyph `orientation`, logical `bounds` |
| `AccessiblePosition` | `snapshot`, `line`, boundary `character` index, `affinity` |
| `AccessibleSelection` | `anchor`, `focus`, retaining selection direction |
| `SourcePosition` | original `TextOrigin` and boundary `Affinity` |

Selectable characters are intervals between actual accepted caret offsets. An
ffi ligature may have three characters using GDEF carets, while a ZWJ emoji,
combining sequence or indivisible transformed source span has one character.
Every accepted covered byte belongs to one interval, including nonpainting
gaps. Empty accepted lines have an empty run and one boundary; an empty supplied
slice has no positions. A character spanning sources/styles uses its first
text scalar's attributes. The source map still covers all constituent sources.
Nonpainting runs may have no glyph font; preserved tabs use primary metrics.

All rectangles/points use shodo's container-relative inline/block coordinates.
The block offset is included. Bidi reading order and physical order differ;
leading/trailing points retain the selected unit's actual direction, including
TCY's internal axis. Convert geometry with each line's writing mode and used
direction, then add the caller's physical container origin.

## Positions, source anchors and reflow

| Method | Contract |
| --- | --- |
| `snapshot_id()` | Identity of this export, independent of Paragraph identity |
| `lines()` | Read output in the supplied logical line order |
| `position(line, character, affinity)` | Validated character boundary, including line end |
| `to_text_position(position)` | Accepted line index and dataset-relative UTF-8 byte offset |
| `from_text_position(position)` | Normalize an interior UTF-8/grapheme/transform position through existing caret snapping |
| `to_source(position)` | Original DOM node/byte offset or generated identity, with normalized affinity |
| `from_source(source)` | Zero or more normalized positions in supplied line order |
| `selection_rects(selection)` | Final logical selection geometry, retaining discontiguous bidi regions |
| `hit_test(inline, block)` | Nearest accepted accessible position; NaN/empty layout returns None |

Invalid indices, offsets and foreign snapshot positions return None or empty
results. Mapping-disabled layouts retain text/geometry but return no original
source conversions. Source offsets are UTF-8 bytes within caller nodes, not
Unicode scalar indices. Collapse and expansion are not bijections: an offset
inside a removed/expanded span normalizes, and soft-wrap boundaries may appear
on both neighbouring lines. Generated content has an identity without a DOM
byte offset; inverse results can contain several matching boundaries.
Repeated DOM ranges retain every occurrence. At a shared DOM offset, affinity
chooses the source range ending there (Upstream) or beginning there (Downstream),
even when generated content separates those ranges. A one-sided source edge
normalizes to its available side.

Before reflow, preserve `SourcePosition` anchors. Build a new AccessibleLayout,
resolve those anchors using `from_source`, and let the caller choose among
ambiguous results. Old line/character positions are rejected even for a reflow
of the same Paragraph. If source content itself changes, the caller reconciles
its source anchors; shodo does not maintain an editing buffer.

`word_starts` contains character indices of ICU word-like starts computed across
the complete accepted logical text. Soft wraps, source/style runs and backend
chunks add no starts. Trailing whitespace/punctuation belongs to the preceding
word; paragraph-leading whitespace starts an implicit word. Atomics are words.
The `complex-scripts` feature uses ICU auto segmentation; without it the existing
non-complex-script policy applies; runs needing context-dependent word breaks
remain unsplit at script-run level and ICU may report a model-unavailable
diagnostic. A word start inside an indivisible unit snaps
to its beginning. An editor with another navigation policy must override the
AccessKit word starts to match its own behaviour.

## Optional AccessKit integration

Enable `shodo`'s `accesskit` feature. The adapter uses AccessKit0.24.1 with default
features disabled. AccessKit types are reexported as
`shodo::accessibility::accesskit::types`; platform backends are not included.
Normal default/no-default core dependency graphs omit AccessKit.

`AccessKitAdapter::new(root_id)` owns the current node registry and position
tables. Its `update(layout, root_node, frame, selection, semantics, allocate_id)`
returns a complete standalone `TreeUpdate`. `frame: PhysicalRect` supplies the
finite origin and nonnegative container dimensions in the same pixel units as
layout. `root_node` supplies the caller's role/attributes; its children, bounds
and text selection are replaced by the export. The standalone tree/focus root
is `root_id`. For embedding, callers adjust tree/parent/focus metadata and use
their application's coordinate transform/platform adapter.

`semantics: Fn(shodo::node::NodeId) -> NodeSemantics` provides caller roles,
optional labels and descriptions. Default text semantics need no wrapper;
custom text roles get source-run wrappers. This does not reconstruct DOM
ancestry. Atomics always receive a semantic wrapper and one TextRun child whose
value is the alternative label, or U+FFFC when absent. The alternative is one
selectable unit, mapped to the atomic's before/after boundaries. Roles, labels
and descriptions remain the caller's responsibility.

TextRun children follow logical reading order, with per-unit UTF-8 byte lengths,
physical bounds/directional positions/widths, font size/weight/style, language,
solid color/decorations and word starts. Runs split at line/attribute/direction/
atomic boundaries and at255 selectable units, with same-line links across
splits. Hard breaks export LF. `to_position` converts a caret and maps a hard
line's end to the break's beginning. `from_position` preserves an explicit
after-break range endpoint, including the consumer's document end after a
trailing LF. `update` retains exact endpoints for nonempty selections, so a
whole-document or LF-only selection includes the break in either direction.
For a collapsed selection it exports one canonical focus caret; equivalent
boundaries on neighbouring lines also count as collapsed. Core byte positions
retain their original representation. Soft wraps add no LF.

`to_position(core_position)` and `from_position(accesskit_position, affinity)`
use the latest successful export. AccessKit positions do not encode affinity;
the caller chooses it when routing selection actions. All indices refer to
selectable units rather than Unicode scalars. Source conversion then uses the
current AccessibleLayout. Removed node IDs and old snapshot positions fail.

Stable keys use source-start DOM node/byte offsets, without line numbers;
generated/unmapped sources use retained identity plus processed start.
Repeated identical anchors are disambiguated in logical order. Reflow preserves
IDs for unchanged starts while new splits receive new IDs. `allocate_id` must
allocate globally disjoint, never-reused IDs, including across adapters/host
trees. Collisions with root/current/new IDs are rejected. The adapter retains
only active IDs, so it cannot detect reuse of IDs retired by older exports.

`AccessKitError` reports `CharacterTooLong` (one selectable unit/alternative
exceeds255 UTF-8 bytes), `InvalidFrame`, `InvalidSelection`, or `DuplicateNodeId`.
Errors preserve the previous registry and position tables and publish no tree.
An allocator may have consumed fresh IDs before a failure. Text is never
truncated or an indivisible unit split. Long names can use a short alternative
plus an unrestricted semantic description; core output has no255-byte limit.

## Reproducible read-only example

```sh
cargo run -p shodo-fixtures --features accesskit,shodo/complex-scripts --example accessibility
cargo test -p shodo-fixtures --features accesskit
```

The fixture crate disables core default features, so the command above explicitly
enables dictionary segmentation for its Japanese text.

[The example](../dev/fixtures/examples/accessibility.rs) uses the actual
AccessKit consumer0.38.0 to read mixed text, obtain physical selection bounds,
route a `SetTextSelection` action to shodo positions and original source offsets,
then apply the resulting tree update. It selects `fi` in `ffi`, returning source
byte offsets1 and3. Checked-in fonts keep geometry reproducible; the bundled
faces lack emoji outlines, while the compound emoji's selectable text remains
available. Supply an appropriate emoji font for rendering.

Callers provide DOM meaning, atomic alternatives, IDs, focus, selection-action
dispatch, host-tree embedding, platform registration and event delivery. This
example requires no editing buffer or IME and demonstrates the data boundary;
it is not end-to-end screen-reader/platform certification.

Pinned API references: [AccessKit schema](https://github.com/AccessKit/accesskit/blob/accesskit-v0.24.1/common/src/lib.rs),
[consumer text API](https://github.com/AccessKit/accesskit/blob/accesskit_consumer-v0.38.0/consumer/src/text.rs).
