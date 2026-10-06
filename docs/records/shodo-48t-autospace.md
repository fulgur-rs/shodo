# shodo-48t: autospace aliases and explicit classes

## Objective and public contract

Preserve Raikiri's explicit `ideograph-alpha`, `ideograph-numeric`, and
`punctuation` selections in shodo and make each class affect layout.
`TextAutospace::Auto` is a distinct computed value with `Normal` behavior.
`Custom { ideograph_alpha, ideograph_numeric, punctuation }` selects an independent
set, including an empty set. All values use insert behavior. The adapter accepts
Raikiri's omitted/insert modifier and rejects replace explicitly; replacing
source U+0020 is outside this issue.

`Normal` stays equivalent to alpha plus numeric. Existing Chinese UTR #59
Conditional punctuation remains letter-like and is therefore selected by alpha,
using each character's own language. This is separate from French punctuation.
Upright vertical letters/numerals and text-combine-upright remain excluded.
Inter-script boundaries retain visual ordering, 0.125ic, and shared-parent
ownership with existing physical edge barriers.

## French punctuation interpretation

The [CSS Text 4 editor’s draft, section 8.4](https://drafts.csswg.org/css-text-4/#text-autospace-property)
and its [referenced Unicode French guidelines](https://web.archive.org/web/20201112013601/http://unicode.org/udhr/n/notes_fra.html)
were consulted on 2026-10-06. French punctuation requires actual non-breaking
spacing; merely exposing a punctuation bit is insufficient.

The boundary's innermost containing element selects both policy and language.
Its `fr` primary subtag matches case insensitively, including regional subtags.
Descendant language does not override a shared-parent boundary; within the
child, the child's language and selected classes apply. Other languages have
no punctuation-class effect.

- Before `:` and `»`, and after `«`: U+00A0-equivalent word-space advance.
- Before `;`, `!`, `?`: U+202F-equivalent narrow-space advance.
- Punctuation sequences such as `?!` receive one preceding thin space; an
  empty `«»` pair receives none.
- Unicode general category Z separators already at the boundary suppress
  insertion. U+200B separates autospace neighbors; no overlapping gap is added.
- Resolve the actual space glyph with font matching, variation coordinates,
  and effective font-size-adjust. Missing glyphs fall back to the resolved
  word-space advance (NBSP) or one fifth of the effective primary em (NNBSP).
- NBSP reservations include the owner's used word-spacing. Existing tracking
  is additive through the shared boundary summary.
- RTL guillemets use their displayed mirrored forms. Source-boundary protection
  follows the lower embedding level of each adjacent pair, including mixed
  LTR/RTL levels, so logical nonbreaking cuts match visual reservations.

These are virtual layout reservations after bidi reordering, with no new text
characters or glyphs. The existing associative spacing summary drives candidate
fit, intrinsic widths, and accepted glyph placement. Internal shaping-cluster
gaps reuse the existing reservation records. Descendant box geometry excludes
shared-parent reservations. Nonbreaking reservations prohibit associated source
break opportunities, including emergency and discretionary-hyphen cuts, before
ligature slicing. Like other indivisible text, these boundaries remain protected
even when the caller requests `LineBreakOverride::Allow`; callbacks may still
control the surrounding text. Forced breaks, atomic/combine boundaries, zero-width space,
and intervening physical edges continue to interrupt spacing.

Additional font probes and a style-indexed pair of space advances are allocated
only when a French punctuation style is present. Normal retains the existing
style-metric cache and early rejection of nonmatching character pairs.

## Verification

RED before implementation: French `a:` was 16.578125px rather than the expected
21.778122px, and an empty Custom set added 5px to `水a 水1`. Both assertions
failed as intended. Focused tests cover independent class sets, French widths,
separators/locales, nonbreaking cuts, source mapping and glyph identity,
shared-parent language/font/box ownership, tracking/word-spacing composition,
upright exclusions, and bidi visual class selection. A self-contained modified
fixture cmap provides U+202F with a deliberately different glyph advance to
verify that real glyph widths win over the missing-glyph fallback, including
font-size-adjust.

Additional RTL RED cases exposed an unprotected mixed-level `:a` boundary
and missing mirrored-guillemet reservations (`31.59375px` rather than
`41.993744px`). The regression now checks visual widths, nonbreaking behavior
under an Allow callback and emergency wrapping, and both intrinsic sizes.

Build cache is isolated per worktree. Reused third-party build artifacts were
retained, but all workspace package artifacts were cleaned before fresh builds
to avoid stale worktree binaries. Verification used Rust 1.97.1, with two build
jobs for the final checks. All commands below exited successfully.

| Command | Result |
| --- | --- |
| `cargo test --workspace` | 1,601 passed, 8 ignored, 95 suites before the final RTL regression/fix |
| `cargo test -p shodo --no-default-features --lib` | 726 passed, 8 ignored before the final RTL fix |
| `cargo test -p shodo-harness --test spacing` | Final tree: 44 passed, including the complete autospace matrix and RTL regression |
| `cargo test -p shodo --lib autospace` | Final tree: 4 passed, including font-advance/fallback and ownership/reordering oracles |
| `cargo test -p shodo --no-default-features --lib autospace` | Final tree: 4 passed |
| `cargo test -p shodo-harness --features accesskit --test spacing --test accessibility_accesskit` | Final tree: 44 spacing and 11 accessibility tests passed |
| `cargo test -p shodo-harness --test intrinsic_budget` | Final tree: 3 passed |
| `cargo test -p shodo --lib line::tests` | Final tree: 11 passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed before the final RTL fix |
| `cargo clippy -p shodo -p shodo-harness -p shodo-raikiri --all-targets --all-features -- -D warnings` | Final tree: passed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps` | Final tree: passed |
| `cargo fmt --all -- --check` and `git diff --check` | Final tree: passed |

The workspace's combined `--all-targets --all-features` test configuration is
intentionally unsupported: the time benchmark rejects `allocation-counting`
at compile time (`dev/bench/benches/layout.rs`). Adding `--all-features` to
workspace clippy also exposes an existing `bool_comparison` lint in
`dev/bench/tests/suppressed_warning_format.rs:169`, compiled only with
allocation counting. Required normal workspace gates and the relevant scoped
feature gates above passed; unrelated benchmark code was left unchanged.
