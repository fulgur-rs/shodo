# shodo-47n: forced breaks with their own inline style

## Requirement and decision

Raikiri's `raikiri-spike-glipj` needs to supply a `<br>` element's computed
font and line-height without making an artificial wrapper inline box supply
the quirks strut. Clear handling needs the break's caller node, and terminal
line handling needs the containing line's fragmentation information.

`ParagraphBuilder::push_forced_break_with_style(node, &style)` records an
interned copy of that style. An explicit bit survives raw input processing,
text transforms and normal/first-line datasets, even when the style interns
to the same slot as the current inline style. `push_forced_break(node)` and
preserved text newlines retain their existing container-strut behavior.

The explicit break supplies an independent strut through the shared
`ProfileResolver::forced_break` calculation. Retained lines and the scalar
metric index both use this profile, including ancestor displacement,
top/bottom groups and vertical writing mode baseline conversion. With the
quirk enabled, an explicit break does not credit a text-free ancestor inline
box. A break directly contained by the root retains the existing conditional
root forced-break rule. Existing text, inline edge and ruby root credits
remain applicable. With the quirk disabled, normal root/ancestor struts join
the break strut. Following text continues using its enclosing style.

The minimal output API is `Line::forced_break() -> Option<ForcedBreak<'_>>`.
It reads the last unit in constant time without allocating and exposes:

- the caller node (or source node for a preserved text newline);
- the effective inline style of the accepted line;
- the processed UTF-8 range;
- whether the style was supplied explicitly for the break.

No `RecordKind` or public `Fragment` variant is added. Clear remains caller
work resolved from the node. The caller uses the containing line's block
size, block offset and break token for fragmentation; there is no separate
break rectangle to paint. The terminal empty line following a trailing break
has no ending break metadata. Such an empty line can remain when a real
ancestor's close unit follows the break; a break that consumes the last unit
is followed directly by `Done`. First-line styles use the existing inherited
override contract, and metadata borrows the effective dataset retained by
the line.

## Regression evidence

The focused integration tests first failed to compile because the builder
API was absent. After adding only input style recording, three metric tests
failed with the intended observable differences:

| Input | Existing result | Expected result |
| --- | --- | --- |
| Text-free 40px and 60px ancestors, explicit 10px break | 60px | 10px |
| Text in a 10px parent followed by an explicit 40px break | 10px | 40px |
| Root atomic followed by an explicit 10px break | 2px | 10px |

The new metadata tests first failed to compile because `Line::forced_break`
was absent. After implementation, the public builder/output regressions
passed, including effective first-line styles, two distinct consecutive
break styles, legacy breaks, preserved newlines and unchanged following text.

The indexed tests compare retained and indexed line heights and baselines
over all eligible ranges with horizontal, vertical-rl and vertical-lr modes,
the quirk enabled/disabled, baseline/top/bottom ancestors and
baseline/top/bottom/text-top/text-bottom/middle/length break alignment.
They also cover an orientation change to sideways. A prefix-query visit
test doubles the input from 64 to 128 repeated text pairs and requires less
than three times the visits, guarding against prefix rescanning.

Input tests cover item, interned-style and style-byte caps, latched failures,
and interning a break style identical to the root under a single-style cap.

## Validation procedure

Base commit: `d784bb1`. Local toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`.

All Cargo commands use an isolated target directory with `TMPDIR=~/tmp`.
Before final validation, clean the local workspace packages from any copied
dependency cache so Cargo recompiles this checkout's sources:

```sh
cargo clean -p shodo -p shodo-fixtures -p shodo-harness -p shodo-raikiri -p shodo-bench
cargo test -p shodo --test line_height_quirk --test build
cargo test -p shodo --lib styled_break
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p shodo --no-default-features
cargo test -p shodo --no-default-features --features complex-scripts
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Validation results

All checks completed successfully on the isolated checkout:

| Check | Result |
| --- | --- |
| Focused public API tests | 15 builder tests and 22 quirks tests passed |
| Focused indexed tests | Both styled-break parity/scaling tests passed |
| Workspace tests and doctests | 1,601 passed, 8 existing ignored, 95 result summaries |
| Clippy, workspace/all targets, `-D warnings` | Passed |
| No default features, including doctests | 1,115 passed, 8 existing ignored |
| No default features plus complex-scripts, including doctests | 1,118 passed, 8 existing ignored |
| Workspace rustdoc, `RUSTDOCFLAGS=-D warnings` | Passed |
| Formatting and diff whitespace checks | Passed |

The workspace run preceded two additional edge tests for identical interned
styles and root/pending-close terminal behavior. Both passed in the final
focused run and both complete feature-isolation runs. The implementation
remained unchanged during those checks. The original workspace library
suite had 732 passed and 8 ignored tests. Its new public API doctest passed.

Build caches and raw verification logs needed for integration and CI are
retained until those checks and the merge are complete. Remove this task's
temporary artifacts after preserving the final results in this record;
the commands and assertions above preserve the reproduction conditions.

## Main integration

Merged `origin/main` at `f72ac877c39651d398827864ffadfe9ef07dd73b`, retaining
the published feature branch's history. This includes shodo-qu8's independent
`force_root_strut` flag and shodo-xgi's anchor block offsets. Resolved the test
and guide insertion conflicts by keeping both features' contracts and tests.

The independent review identified that the parity helper skipped ranges
starting at `ForcedBreak`. A break-only paragraph now requires exactly one
height and baseline comparison; it first failed because the existing helper
compared zero ranges. Included forced-break starts in the helper so the normal
retained/indexed comparison covers that continuation boundary too.

The composed regressions cover `force_root_strut` false/true, direct-root and
two nested text-free ancestors, a preceding atomic or no preceding content,
normal and first-line styles, and horizontal/vertical-rl/vertical-lr modes.
They compare indexed heights and alphabetic baselines in both datasets and
assert the public first and continuation line heights, break identity/style,
and zero-height pending-close terminal line. Root opt-in keeps its own 20px
strut (30px on the first line) while the independent 10px break does not credit
the 60px/80px ancestors.

Integration checks used an exclusively assigned dependency cache, cleaned of
all five workspace packages before compiling this checkout, with two build
jobs. Compile logs confirm the assigned feature worktree as the source of all
five packages. No additional production behavior changes were needed.

| Fresh integration check | Result |
| --- | --- |
| Break-only range before helper coverage fix | Failed: 0 comparisons, expected 1 |
| `cargo test -p shodo --lib styled_break` | 4 passed, including break-only and composed dataset parity |
| `cargo test -p shodo --test line_height_quirk --test build` | 25 quirks and 15 builder tests passed |
| `cargo test --workspace` | 1,610 passed, 8 existing ignored, 95 result summaries, zero failures |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | Passed |
| `cargo fmt --all --check` and `git diff --check` | Passed |

All three new test names appear in the fresh workspace log. These gates ran
after the final source changes, including the earlier identical-style and
terminal pending-close regressions.

## Release handoff

Release publication and the pin-ready version are owned by the parent
integration task. After publication, notify `raikiri-spike-glipj` with the
released version and replace the artificial `<br>` wrapper with
`push_forced_break_with_style`. Read `Line::forced_break().node` to identify
the element for clear handling, and retain ordinary line metadata and break
tokens for terminal fragmentation handling. `shodo-qu8` adds independent
root-strut opt-in behavior and is merged into this branch; the composed
quirks and fresh workspace checks cover that integration before release.
When removing the wrapper, preserve the distinction between a final break
followed directly by `Done` and an ending break followed by pending inline
close units. `Line::forced_break` supplies the element identity before that
terminal decision; it does not synthesize an extra line.
