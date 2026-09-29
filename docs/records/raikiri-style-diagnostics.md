# raikiri S4 residual-style diagnosis

The frozen S4v2 run rejected 303 block attempts in 109 original WPT documents
with `noninitial style not mapped yet`. The development example now reports the
actual error node's field names, values and initial values, keeping its root,
DOM identity and original input hashes. This is a diagnosis of the residual
style gate, not a layout engine, a production adapter, or a WPT verdict.

[The complete classification](../../dev/raikiri/data/raikiri-style-diagnostics.json) contains all
109 documents and 303 blocks, including mixed causes. It retains every residual
value, 111 original resource records, the original 88-font registry, and six
original documents' parser warnings. [Input pins](../../dev/raikiri/data/raikiri-style-input-pins.json)
retain the independently checked font paths, sizes and hashes. Neither WPT
inputs nor font bytes were replaced with development fixtures.

## Reproduce

Use the original WPT checkout and the retained S4 comparison artifact. Run from
this repository's root; output paths below are disposable development artifacts.

```sh
mkdir -p target/style-diagnosis
cargo run -p shodo-raikiri --example raikiri_style_diffs -- \
  /home/mitz/.cache/raikiri/wpt \
  target/worktrees/shodo-s4-v2/target/s4v2/wpt-batch-full/comparison.json \
  target/style-diagnosis/residual.json
python3 tools/raikiri/classify_raikiri_styles.py \
  target/style-diagnosis/residual.json target/style-diagnosis/classified.json
cmp target/style-diagnosis/classified.json dev/raikiri/data/raikiri-style-diagnostics.json
```

The WPT path and original comparison are local investigation inputs, not files
shipped by this crate. Retain the comparison when archiving the unmerged spike.
Its SHA256 is `67434d34bbe6928ab3a67ba43b02120b27407d57e9fb00b95af10daefc3ce01d`.
The WPT Git revision is `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`;
raikiri is compiled from `ab7e619a8f321f03de8b8c8b9342954868e044c8`.
The baseline SHA256 is
`56154e4748a14a4762f1d25cf05f1c009e63151cba726fffd31dc2ebb6be0d65`.
The original comparison's screen viewport is 800 × 600. The parser uses the same
pinned screen media context; these diagnostics do not perform box layout.

The example verifies all original resource hashes before cascading the original
HTML and linked/imported stylesheets. Unexpected fetches, changed resources,
changed warning traces, missing nodes, different root tags and empty residual
reproductions are errors. An incomplete report is written with per-document
errors and the process exits unsuccessfully. Font-resource bytes are verified,
but the example does not register or render fonts. The separate byte audit
confirmed that all 88 original registry hashes still match files under WPT's
`fonts/`; their hashes in this report are provenance, not new rendering evidence.

## Reproduce the caller's input, then compare fields

The frozen S4 `style.rs` hash is
`4a225616ca97c1b83eeadff6c5228cc23c32ab36a033bb3a4f8d66ecdf4f977f`.
The generated accessors contain only field names and reset profiles, and compile
against the actual pinned `ComputedValues`. They do not copy upstream CSS or
layout algorithms. Public differences use typed `PartialEq`, not parsed Debug
strings. All 168 public fields are checked after the exact 50 mapped-field
resets; whole-value equality also covers the two private custom-property fields.

The original measured caller applies another profile **before** that gate:

| Input | Attempts | Additional already-owned fields reset |
| --- | ---: | --- |
| Measured root block | 266 | width/height and ch variants, min/max sizes, min-block-size, box-sizing, margin/border/padding and ch variants (15 fields) |
| Atomic inline | 7 | width, height, box-sizing |
| Plain inline/text | 30 | none |

An initial diagnostic without this preprocessing wrongly counted sizing fields
as residual causes. The committed report uses the corrected profiles. Width,
height and box-sizing are not gaps discovered by this report. The measurement
profile regression checks a root and an atomic with noninitial dimensions and
backgrounds, and requires only their backgrounds to remain in the diagnostic.

For private environments, whole-value equality first proves a real residual;
bounded Debug only exposes local bindings and whether a parent exists. It does
not enumerate inherited bindings. Unreported private differences produce
`unreported_residual`, which the replay rejects, rather than a false empty
success. None of the 303 actual attempts has a private-environment residual.
These selected attempts reproduce ordinary measured/atomic/plain inputs; this
is not a general replay of generated/pseudo-element event ordering or all
earlier style-converter checks. A future pin or new ambiguous case needs a
profile/ownership review.

## Field inventory

Counts are overlapping block/document sets, not WPT failures. Actual values and
initial values for each node are in the JSON. The following ownership decisions
apply to in-flow inputs; absolute boxes receive the separate IFC scope decision.

| Residual field | Blocks | Documents | Owner / follow-up |
| --- | ---: | ---: | --- |
| position | 177 | 59 | 112 Absolute, 65 Relative; IFC boundary / positioned-box caller |
| background_color | 145 | 66 | box paint; existing solid leaf background must be handed off |
| z_index | 48 | 37 | stacking/paint order; all -1 |
| float | 25 | 15 | float-root/BFC placement and existing Taffy/shodo flow protocol |
| left | 17 | 7 | positioned-box caller |
| cssom_writing_mode | 16 | 4 | existing vertical paragraph API; all VerticalRl |
| top | 13 | 3 | positioned-box caller |
| hanging_punctuation | 11 | 4 | existing First parser and shodo line option |
| overflow | 8 | 6 | box clipping/scroll policy; 7 Hidden, 1 Auto |
| clear | 4 | 4 | caller BFC clearance; all Both |
| text_orientation | 4 | 2 | existing vertical inline API; all Upright |
| text_decoration_line | 4 | 2 | existing underline/source paint API and caller propagation |
| outline | 4 | 2 | box outline painter |
| background_image | 1 | 1 | box gradient painter |
| background_position | 1 | 1 | box background geometry |
| background_repeat | 1 | 1 | box background tiling |
| background_size | 1 | 1 | box background geometry |

A noninitial field is evidence of a gate rejection, not proof that shodo needs a
new core API. Solid leaf backgrounds already exist in S4's candidate page paint;
relative offsets, stacking, clip/scroll, outlines, gradients and general box
ownership need caller policy or renderer work. Resetting these properties and
silently dropping their effect would not resolve the gap.

## Classify every document and block

The classifier assigns in-flow fields to `existing-api-unwired` or
`caller-feature-gap`. The latter includes missing general caller ownership and
paint features; it does not assert that every named feature is absent from the
core or from every existing rendering path. `float`/`clear` use existing caller
BFC placement and Taffy `FloatContext`, plus shodo's `FloatCursor`/
`LineConstraint` protocol. A floated root can have its own paragraph, but its
placement and clearance belong to that caller.

If the error node or an ancestor within its recorded root is absolute/fixed,
the attempt is `legitimate-ifc-scope-rejection`: such a box is outside normal IFC
flow. All 112 actual instances are Absolute at the error node. The old root
check's "in-flow" wording actually checks document membership, element kind and
block display; it does not itself reject Absolute. The residual gate prevents
accepting it as an ordinary paragraph. This is a valid component boundary,
**not permission to omit that box in a full-page WPT rendering**. The stored
field values and issue links remain available for whole-page ownership work.

| Category | Blocks | Documents |
| --- | ---: | ---: |
| Existing API not connected | 60 | 25 |
| Caller/paint feature or ownership gap | 151 | 66 |
| Legitimate ordinary-IFC scope rejection | 112 | 57 |

20 in-flow blocks have both an API and a caller/paint gap. Document category
sets also overlap. The following mutually exclusive document groups total 109:

| Document category set | Documents |
| --- | ---: |
| caller gap only | 31 |
| API only | 17 |
| IFC boundary only | 24 |
| caller gap + API | 4 |
| caller gap + IFC boundary | 29 |
| API + IFC boundary | 2 |
| all three | 2 |

Unknown residual fields or incomplete inputs are refused by the classifier.
Every classified block preserves root and error-node IDs, input profile, all
field values and its follow-up issues. This inventory does not classify the
other unsupported reasons or the separate 167 incomplete-source blocks.

## Follow-up issues and baseline implications

Issues are in this repository's `bd` ledger. Existing S4 adoption (`shodo-p2m.5`),
first-line/shared-glyph caller contracts (`shodo-v7f`, implemented representative
caller), and float harness work already cover their broader investigations.
The new issues track concrete production wiring rather than duplicate those
completed core contracts:

| Issue | Scope | Cutover status |
| --- | --- | --- |
| shodo-9an.1 | None/First → real production LineOptions, retaining native leading U+3000 behavior | required by shodo-p2m.6; waits for S4 adoption policy |
| shodo-3v2 | resolved writing-mode/text-orientation → existing vertical APIs | waits for S4; actual native/candidate effects must determine cutover necessity |
| shodo-0zm | separate text validation from BFC/paint properties; preserve actual box effects and source underlines | waits for S4; compare rendering before deciding cutover necessity |

The original `shodo-9an` diagnosis that hanging-punctuation had no field/parser
was wrong. The pinned `property/parse/text.rs:1431` accepts **none/first** and
`raikiri-paint/src/text.rs:379` already paints an LTR leading U+3000 by retaining
the glyph and shifting the first line by its advance. The 11 residual attempts
in four documents are **First already parsed but not wired by S4**. The broader
last/force-end/allow-end/combined CSS values remain separate upstream work in
`shodo-9an`; the existing acceptance was retained when correcting its premise.

`hanging-punctuation-first-002.html` is registered in the pinned baseline.
The original native static-screen pair has 0 changed pixels and satisfies its
exact `match` expectation; the candidate pair reports page paint unavailable.
That existing native capability makes the First connection a cutover dependency.
It does not establish full hanging-punctuation support, bidi/quote equivalence,
or official WPT PASS/FAIL totals.

The diagnostic invokes no candidate layout/paint assertions. Its candidate WPT
image verdict count is **0**, and baseline PASS delta remains **unmeasured**.
Neither protected S4 branch is changed, pushed, PR'd or merged by this work.
