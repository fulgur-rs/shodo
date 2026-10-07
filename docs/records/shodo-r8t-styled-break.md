# shodo-r8t: conditional styled-break struts in quirks mode

## Contract and implementation

`push_forced_break_with_style` supplies its own font/line-height strut under
`line_height_quirk` only when the break's parent has no other metrics on the
line. Non-trimmed text, baseline atomics and that parent's own inline edge
credit it and suppress the break profile. Trimmed trailing spaces do not
count as text. Pending aligned descendants may not credit a baseline parent,
and text or edges outside the break parent do not credit it. The user
explicitly chose this independently measured Chromium rule over the ticket's
initial generalization that any other content on the line suppresses it.
Root-strut opt-in remains independent. Standard-mode, plain breaks and
preserved newlines retain their previous behavior.

The existing `ContentCredit` depth reduction and parent strut credits decide
eligibility. Retained measurement gates break profiles on that decision.
Indexed measurement keeps explicit break profiles and their top/bottom group
extents in conditional summaries, selected with the same decision. They exist
only for a quirks dataset with explicit break styles; other datasets allocate
no extra summary tree. Position-only profiles use a compact conditional
index too, so clipped unsized groups query only their selected profiles,
including reshaped glyph replacements. Queries avoid unit rescans. Synthetic retained ruby ranges can contain several breaks; every
eligible profile is included, rather than only the final one.

Break nodes, explicit-style identity, effective first-line styles and output
metadata remain available even when a break's metric profile is suppressed.
No public API shape, dependency or minimum Rust version changed.

## Independent Chromium evidence

Base: main `66653e3b8790a9f146d80c8c9018c2ecc1cd7e01` (Shodo0.0.20).
Chromium152.0.7977.82 (Arch Linux), headless, `document.compatMode=BackCompat`.
No doctype; `body { margin:0; font-size:20px; line-height:20px }`.
Each case is a separate div; an inline image has width/height2px and a
transparent data-URL GIF. Read each div's `getBoundingClientRect().height`.

| HTML in the div | Chromium / fixed Shodo (px) |
| --- | ---: |
| `<img><br>x` | 22 |
| `<img><br style="line-height:40px">x` | 22 |
| `a<br style="line-height:40px">x` | 40 |
| `<span style="line-height:40px"><img><br>x</span>` | 42 |
| `<span style="line-height:40px"><br></span>y` | 60 |
| `<br>y` | 40 |
| `<br style="line-height:40px">y` | 60 |
| `<span style="line-height:40px"><br style="line-height:10px"></span>y` | 30 |
| `<span><br style="line-height:40px"></span>y` | 60 |

Additional pending-child cases: a baseline10px parent with an empty top60px
child and a styled40px break is40px. Replacing top by text-top makes it0px.
An empty top60px child directly in the root followed by a styled40px break
is also0px. These results were measured independently and are public tests.

A top2px image followed by a styled100px break inside a10px baseline parent
is100px (the top image does not credit that parent). Root text followed by an
empty span containing a styled100px break is100px. An own parent with1px
inline-start padding followed by the same break is10px, but placing that
break inside an empty child of the padded parent is100px. These four measured
cases expose why a global content-presence test is insufficient.

## Regression and review evidence

The unchanged baseline passed all25 existing public quirks tests. New public
assertions failed six tests against the original metrics: atomic/text lines,
identical interned styles, the nine-row matrix, preserved spaces and an
explicit100px font after text. The first matrix row was40px instead of22px.

An initial single-break indexed selection failed existing parity tests:
indexed21.609375 versus retained100, and indexed30 versus retained40.234375.
Synthetic retained ranges span multiple breaks, so conditional summaries now
preserve every eligible profile and grouped extent. The five existing indexed
styled-break tests then passed, including horizontal/vertical-rl/vertical-lr,
first-line data, top/bottom groups, same-side emphasis/ruby and bounded prefix
visits. The new parity test covers trimmed/preserved spaces, padding and
baseline/top/text-top pending children in all three writing modes. A second
public RED failed2px versus100px for the top image, then passed after using
local parent credit. The existing annotation-metrics test incorrectly assumed
a40px break font enlarged a parent's already credited text line; it now
compares height and annotation geometry against the unchanged plain-break
input path in all modes with root-strut opt-in off/on.

The standard-mode nine-row expectations retain0.0.20 results:40,60,60,80,80,
40,60,80,60px. In particular, pending close units can retain an ancestor strut
on the following line; this change does not alter standard-mode behavior.

The earlier shodo-47n record is corrected: text in a10px parent followed by
an explicit40px break stays10px, and a root2px atomic followed by an explicit
10px break stays2px under the quirk.

## Independent review corrections

The fresh-context read-only review found two Important issues, no Critical
or Minor issues, and no declined judgments. Both are corrected with regressions:

- A wholly selected top/bottom group needs the union of normal and break
  bounds before converting to a height. Separate maximum heights lost the
  displacement: indexed40px versus retained130px in a reproduced range.
- Content normalization and ghost placement need the same break eligibility.
  The reproduced content bottom was24.484375px versus134.484375px when the
  break contributed; a suppressed break wrongly moved it to134.484375px
  instead of28.96875px. Styled breaks are excluded from position-only profiles.
  Selected ghosts include only the clipped range and its continued ancestors.
  A changed conditional group uses its selected raw content and bounds, and
  those positions enter the existing selection digest used for cache replay.

Tests cover all three writing modes, top/bottom, positive/negative shifts,
partial/full ranges, accepted/suppressed profiles and pre-break empty prefixes.
Both height/content tests and a separate suppressed-ghost test failed before
these corrections. Bounded prefix visits cover the accepted-group and unsized-
ghost branches as well as the original mixed-parent suppression branch.

Conditional groups are indexed in source order. A physical forced-break line
has at most one break profile/group; synthetic ranges containing several
breaks revisit only groups whose placement changes, never scan their units.
This avoids allocating a second dense content-geometry tree. The compact
position-only index is allocated only in quirks datasets with styled breaks.

## Reproduction and validation

Local Rust/Cargo1.96.0; MSRV remains1.89.0. Run in the dedicated checkout:

```sh
export TMPDIR="$HOME/tmp"
export CARGO_TARGET_DIR="$HOME/tmp/shodo-r8t-target"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=/usr/bin/env
export CARGO_PROFILE_DEV_OPT_LEVEL=1
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
cargo test -p shodo --test line_height_quirk --test build --test annotation_metrics
cargo test -p shodo --lib styled_break
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p shodo --no-default-features
cargo test -p shodo --no-default-features --features complex-scripts
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
git diff --check
```

Final checks ran after the review corrections, including the conditional
group/ghost scaling regression and allocation/query gates for ordinary datasets.

| Check | Final result |
| --- | --- |
| Public quirks / builder / annotation metrics | 30 /15 /15 passed |
| Indexed styled-break height, baseline, content and scaling | 10 passed |
| Workspace tests and doctests | 1,699 passed,9 existing ignored,100 summaries |
| No default features, including doctests | 1,164 passed,9 existing ignored |
| No default features plus complex-scripts, including doctests | 1,167 passed,9 existing ignored |
| Clippy, workspace/all targets, `-D warnings` | Passed |
| Workspace rustdoc, `RUSTDOCFLAGS=-D warnings` | Passed |
| Formatting and diff whitespace | Passed |

Compile logs identify this task's checkout as the source of all five workspace
packages. The target is exclusively assigned to this work; no copied target
artifacts were used. The independent review's two Important findings were
fixed in one pass, each with RED/GREEN evidence and final whole-suite success.
No Critical/Minor finding or declined judgment remains.
Publication, pin and archive verification evidence are recorded in shodo-r8t
and the GitHub release after shipping. Raw logs, headless browser profile,
build target and dedicated worktree are removed after preserving evidence.
Raikiri caller adoption remains tracked separately as raikiri-spike-6qrnr.17.
