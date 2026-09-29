# Float caller integration harness

The development-only [driver](../../dev/harness/examples/support/float_flow.rs)
connects public `Paragraph::next_line` to real Taffy 0.14 float placement.
[Numeric tests](../../dev/harness/tests/float_flow.rs) and a
[fixed-font PNG example](../../dev/harness/examples/float_png.rs) share it.
It is a state-management example for callers, not part of shodo's library API.
Taffy is a fixture dev dependency only.

```sh
cargo test -p shodo-harness --test float_flow
cargo run -p shodo-harness --example float_png -- target/shodo-floats.png
```

The output path is relative to the working directory; parent directories are
created. The sample uses checked-in Latin font bytes, 16px type, 20px line height,
a 200px content width, a blue 40×40 left float and green 30×20 right float.
The existing outline painter consumes accepted glyph IDs and positions once;
float rectangles have a separate caller paint pass. It does not reshape text.
It shares the fixed outline/synthesis restrictions of the
[PNG sample](png-render-sample.md).

## Responsibilities and state

shodo reports float anchors and displaced floats without placing them. This
harness demonstrates caller placement, provisional trials, rollback and reusable
checkpoints. Production callers must supply DOM/CSS measurements and callbacks;
the harness does not provide a complete raikiri integration or establish browser
equivalence, WPT conformance, or whole-page performance.

`Checkpoint` owns paragraph identity and placement epoch, break token, float
cursor, Taffy context, placement requests/rectangles, pending reports,
withdrawn-this-line records, block position, width and fragment index. An epoch
separates successive paragraphs, including repeated use of the same immutable
Paragraph: their opaque float cursors start over without identifying old
placements as current-line withdrawals. Inputs (Paragraph, options, atomic sizes,
float measurements) remain caller-owned and must be preserved while replaying a
checkpoint. `LayoutContext` is only a cache and need not be checkpointed.

`Driver::trial` borrows a checkpoint and returns a provisional line/state, a
height rejection, completion or block boundary. Accept by assigning `trial.state`.
Discard by retaining the input checkpoint. A rejected-height result returns the
input checkpoint, so placement and cursor cannot disagree after a retry.
Diagnostic `attempts` contain full clones before each `next_line` call; they
allow tests to exercise checkpoints with actual nonempty pending/withdrawn
records. This intentionally retains extra memory in a test harness. Production
callers should keep only required line/page checkpoints; no memory benchmark or
production allocation claim is made here.

Each report is placed only if its measured margin-box width fits the remaining
line width, there is no preceding pending report, and it has not been withdrawn
on this line. Otherwise it is pending. After either action, retry the original
line token with the new cursor and slot. Only the last displaced report is
withdrawn, followed by another call: do not batch withdrawals. Retained requests
are replayed into a fresh Taffy context. A withdrawn anchor that reappears is
always pending. Accept only displacement-free lines, flush pending floats below
the line in source order, then clear the withdrawn set.

Block boundaries return a continuation token for separate caller box layout.
`commit_block` commits a measured block height, pending reports and that token;
it preserves existing float exclusions. `begin_paragraph` requires completion of
the previous paragraph and resets token/cursor while retaining the common BFC.
`Checkpoint::new` starts a completely independent layout.

## Geometry and termination

The reported inline position excludes the float inset. Do not subtract the inset
a second time when checking remaining width. Tabs use the content-box edge,
including the caller's inset, as required by the core contract.

The initial slot is a point query; subsequent trials include every rectangle
intersecting the actual line-height band. Height growth that changes exclusions
causes a band retry. An indivisible line that cannot fit beside a float retries
at successive float bottoms. Before moving a line down, a current-line float must not remain above earlier
inline content belonging to that same line. The driver checks logical glyph
source ranges and atomic source offsets, independently of visual fragment order
or advance. If a middle float would violate that rule, defer the latest current
placement and replay the retained BFC before retrying; repeat one placement at a
time to preserve source order. This geometry deferral keeps the anchor
acknowledged as pending. It is separate from a displaced-anchor source rewind.
A head float with only later inline content can remain high while that content
moves below it. Text-indent and zero-width content do not substitute for source
order. These geometry retries are distinct from the
foundation's `3F+1` report/withdraw/rereport bound at a fixed position/band.
The trial also has a finite defensive call limit derived from source length and
float specs. Candidate y positions strictly increase; no candidate is repeatedly
tried. A page shorter than one line must explicitly retry with no height limit
once on a fresh page or the last available position and accept the advancing
line. The regression tests demonstrate that oversized path. Oversized floats
are deferred and may overflow the empty containing width under Taffy's rules;
following text skips their obstructed band. Zero-width floats still contribute
to clearance and source-order ceilings.

Taffy 0.14 has no public removal operation. Its context can be cloned, but the
borrowed BFC in an external renderer is not a checkpoint. This harness therefore
owns the context and replays placement logs on one-anchor withdrawal.

### Taffy band discrepancy

A left 20×30 float at (0,0), right 20×50 float at (80,0), followed by a right
10×10 `clear:left` float reproduces an upstream 0.14 result of (90,30), overlapping
the right float. The caller obtains the public clear threshold and passes the
retained rectangle band's left/right insets to real Taffy placement using
`Clear::None` at that threshold. The last float is then at (70,30). The test keeps
both the direct upstream reproducer and the corrected caller result. This is a
local caller workaround, not an upstream Taffy fix. Changes to the Taffy version
must revisit the reproducer and workaround. The 144-case matrix checks independent
CSS ordering, clearance, containment and nonoverlap invariants.

## Page and lookahead policy

`preview` collects actual lines and complete prefix checkpoints. `select(0)`
discards all speculative state; `select(n)` commits exactly n accepted lines,
including the corresponding cursor, BFC and pending/withdrawn state. The caller
must discard unselected output as well as restore the checkpoint. Preview limits
include maximum lines and optional remaining page height; a height rejection does
not commit the rejected trial.

`widow_prefix(capacity,total,orphans,widows,fresh_page)` selects a permitted prefix
using an actual lookahead line count. Keep at least `orphans` current-page lines
and `widows` next-page lines when possible. If an occupied page cannot meet them,
select zero and move the paragraph. On a fresh page, relax impossible limits and
use the available positive capacity. If even one line does not fit, use the
explicit unlimited-height fallback above. Calling code must treat a truncated
preview as incomplete, rather than inventing the total line count.

Before page movement, restore the chosen prefix checkpoint, then call
`next_fragment(consumed_height,new_width)`. The consumed height must include all
accepted content. Only unconsumed float rectangles carry into the new fragment;
resolved old clearance is not reapplied. Right-side placement is rebuilt for the
new width. Token, cursor, pending reports and withdrawn records remain together.
For source ordering, use the actual `Line::text`/`Line::offset_mapping` text set,
which can differ from `Paragraph::text` for first-line transforms. Default
OffsetMapping identifies float markers and accepted atomic markers. With mapping
turned off, register caller-owned `SourceOrder` with
`Driver::register_source_order(paragraph_id, exact_processed_text, order)`.
Its ordered float `(NodeId, byte_offset)` entries cover all float anchors in that
text set; atomic byte offsets identify the in-flow markers. Inline padding/border
or margin edges have no generated text marker: supply `SourceEdge` node, processed
boundary offset and start/end flag in `SourceOrder::edges` when their prefix order
is needed. The driver matches actual accepted box-fragment edge flags. `None`
means unknown and returns `MissingSourceOrder` for an ambiguous edge-only prefix;
`Some(empty)` explicitly asserts no in-flow edges. Zero-length empty inline boxes
before a float use the marker's start boundary; edges after it use the marker's
end boundary. Both directions are regression-tested. Register separate
text sets if first-line transforms alter processed offsets. A missing entry on a
geometry retry returns `MissingSourceOrder`, rather than guessing from an inline
position. Preserve these inputs along with float sizes when replaying checkpoints.
These are provisional float-slicing rules for this harness, not a general CSS
fragmentation implementation with margins, nested BFCs or float painting across
arbitrary pages.

## Numerical evidence and comparison scope

Builtin-font counterexamples use deterministic 10px advances, including a 10px
space. Float markers are U+FFFC and occupy three UTF-8 bytes in processed text;
they have no inline advance. Numeric source ranges therefore include markers.

| Case | Asserted result |
| --- | --- |
| Opposing head floats, content width100 | left x0/right x70; slot start20/width50; 3 calls |
| Midline `aa ` followed by widths80/10 | both deferred in order to y10; 3 calls |
| Width160, F1=20, `a a TAB a `, F2=50 | withdraw only F2; F1 y0/F2 y10; 5 calls |
| Width60, `aa b[F]bbbb`, F=30 | first line0..3 with no report; anchor line3..11 stays y10; F deferred below it to y20; 3 calls |
| Width120, TAB then long span and F=90 | ranges0..5,5..17,17..29,29..33; float first appears at anchor line y30 |
| Tall40px atomic with future right float | band retry then line y30; 5 calls |
| Height5 for a 10px line | placement trial rejected; original checkpoint and float report replayed |
| Width80→100 page with right20×50 float | remaining20×40 rectangle moves x60→80; no duplicate report |
| Widows2/orphans2, five actual lines, capacity4 | commit3; remaining2 preserved; capacity1 on occupied page discards all |

The suite additionally exercises nonempty pending/withdrawn checkpoint rollback,
accepted/rejected lookahead, source epochs, block handoff, oversized and zero-width
floats, head/middle source-order safety, mapping-off and transformed first-line
sets, and real-font deterministic RGBA paint.

Reference: [Parley float test](https://github.com/linebender/parley/blob/main/parley_tests/tests/floats.rs),
read via the GitHub contents API on 2026-09-27; downloaded source SHA256
`55970304f80e244c3182a8f9ba2f6d8a0abad636c0cbc06c1d5fd5e73ecef78f`.
It contains unfinished nonfitting-line rewind and height-exceeded handling. No
source was copied.

No Blink run or browser baseline is included. The tab case is a shodo/CSS-contract
regression, not a Chrome equality assertion. If a later browser recorder finds a
one-line difference, retain both numeric/PNG results and distinguish an allowed
CSS placement difference from a violated float ordering rule. See the separate
[snapshot tests](../dev/snapshot-tests.md) and [browser comparison](../dev/browser-comparison.md)
for the supported rendering and browser checks.
