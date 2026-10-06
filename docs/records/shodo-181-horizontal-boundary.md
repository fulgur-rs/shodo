# shodo-181: horizontal trim-all boundary investigation

## Result and ownership

The original horizontal WPT variants still fail with exactly 2,892 mismatched
pixels each. This record does not report them as passing. The original test
omits a `text-spacing-trim` declaration, while its reference applies
`space-all` and an explicit `halt` feature to each punctuation character.
The current CSS Text Level 4 initial value is `normal`; isolated internal
punctuation remains full width under that policy. Applying `trim-all` explicitly
at the Shodo boundary produces the same glyph IDs, positions, advances and
clusters as the explicit-feature reference for every original row and both
original test fonts. No Shodo production defect has been demonstrated by this
investigation. Changing Normal into TrimAll would break the documented policy.

The original issue acceptance requires either both unmodified original WPT
variants to pass EXACT, or a concrete Shodo cause and fix. Neither condition is
satisfied by these results. The issue owner approved resolving this task through a regression-test and
investigation-record PR, treating the absent declaration as an upstream
fixture problem. This decision supersedes the original acceptance requirement;
original WPT failures remain explicitly recorded rather than reclassified.

Primary specification: [CSS Text Level 4, text-spacing-trim](https://drafts.csswg.org/css-text-4/#text-spacing-trim-property).

## Inputs and fresh builds

- Shodo base: `d784bb1` (0.0.18).
- Raikiri verification checkout: `d7dabe7d666e688e34fd9d874b98a35e6964c2a7`.
- Original WPT checkout: `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`.
- Original HTML, reference, variant script and two font SHA-256 hashes, plus
  all 42 shaping results, are in `shodo-181-horizontal-boundary.json`.
- Horizontal only, 20px, language unspecified at the original boundary.
  Original rows: 国（国, 国）国, 国、国, 国・国, 国。国, 国「国, 国」国.
- All original WPT inputs were preserved. No expected-failure list changed.

Copied build caches were cleaned for all workspace packages before accepted
results were collected; third-party dependencies were retained. The first
full-image attempt failed to compile the diagnostic harness because its
non-exhaustive configuration was constructed with a struct literal. The
corrected harness used `ReftestConfig::default()` followed by public field
assignment, then completed successfully. Its test completion means the probe
ran; the actual reftest outcomes below remain Fail.

## Exact original full-page evidence

A detached Raikiri worktree patched its crates.io Shodo dependency to this
investigation checkout. `discover_pairs_for_file_with_wpt_root` loaded the
original test/reference pair, and `run_pair_with_variant` ran each original
horizontal query at 800 by 600 using `Tolerance::EXACT` (maximum channel delta
zero, maximum differing-pixel fraction zero). The diagnostic test did not
assert the reftest outcome.

| Original variant | Differing pixels | Pages test/reference | Actual outcome |
| --- | ---: | --- | --- |
| `?class=halt,htb` | 2,892 / 480,000 | 1 / 1 | Fail |
| `?class=chws,htb` | 2,892 / 480,000 | 1 / 1 | Fail |

Reproduce with the immutable Raikiri checkout, its WPT runner, and a local
`[patch.crates-io]` entry for Shodo. Call the discovery and runner functions
above on `css/css-text/text-spacing-trim/text-spacing-trim-trim-all-001.html`
with the recorded variants/configuration. Temporary patch and probe source are
removed after their evidence is retained; they are not a product change.

## Shodo boundary and regression checks

For each original punctuation character and each original font:

| Policy | Row advance | Punctuation advance |
| --- | ---: | ---: |
| `Normal`, no explicit feature | 60px | 20px |
| `SpaceAll`, explicit `halt=1` on the punctuation | 50px | 10px |
| `TrimAll`, no explicit feature | 50px | 10px |

The last two rows have identical glyph geometry. Explicit-feature results from
the chws test font also equal those from the halt reference font for all seven
rows. This checks the shaping boundary, not a modified full-page reftest.

The always-on test in `tests/japanese.rs` uses the existing repository CJK
fixture, both absent/ja language, and all seven punctuation characters to
protect the distinction between Normal and explicit TrimAll. The ignored
`tests/text_spacing_trim_wpt_boundary.rs` test uses original WPT font bytes and
requires `SHODO_181_WPT_ROOT`; it checks all 42 cases.

Accepted runs in the isolated Shodo target:

- `cargo test --offline --locked -p shodo --test japanese`: all 50 tests passed, including the new regression.
- `SHODO_181_WPT_ROOT=<WPT checkout> cargo test --offline --locked -p shodo --test text_spacing_trim_wpt_boundary -- --ignored --nocapture`: passed.

Use Rust 1.97.1, `TMPDIR=~/tmp`, an exclusive Cargo target, and the repository
supported feature defaults. `cargo clippy --offline --locked -p shodo --tests -- -D warnings`,
`cargo fmt --all --check` and `git diff --check` passed after the final test
was corrected to use the halt reference font for both original variants.
GitHub CI supplies the full workspace, MSRV and wasm verification for the PR.
