# Root struts for list-item paragraphs in quirks mode

Issue: `shodo-qu8`. Base: `d784bb1` (shodo 0.0.18).

`ParagraphStyle::force_root_strut` opts into a root strut on every nonempty
line when `line_height_quirk` is enabled. Callers set it for list-item
paragraphs, whose quirks-mode root strut contributes on continuation lines
as well. It defaults to false; child inline fragments continue to suppress
their own struts when they have no direct text or qualifying edges.
Standards-mode root contribution already applies without the option.

Both retained-line metrics and the indexed range metric path honor the
option. It does not make an otherwise empty line consume block space.
The public paragraph style gains a field; exhaustive struct literals must
supply it or use a default struct update.

## Regression evidence

With the two contribution conditions removed, the new integration tests
failed: an atomic-only child line remained 2px instead of the 20px root,
and continuation lines remained 10px instead of 20px. The indexed metric
test independently failed with 10px instead of 20px. Restoring the
implementation makes all three tests pass.

Coverage checks horizontal and vertical lines, an empty 80px child around
a 2px atomic, every forced-break continuation, and indexed-versus-retained
block size and baseline parity for first, second and complete selected
ranges. An independent review found no blocking issues.

The dedicated-output full workspace run passed 1595 tests with zero failures
and 8 ignored. Its log identifies this worktree's rebuilt workspace sources
and all three new strut tests; the separate anchor worktree's test is absent.

Reproduction uses the real stable toolchain with cached offline dependencies
and a dedicated Cargo output directory. Clean all five workspace packages
before reusing a cache copied from another worktree, retaining only the
third-party dependency cache:

```sh
cargo clean -p shodo -p shodo-fixtures -p shodo-harness -p shodo-raikiri -p shodo-bench
cargo test --offline --locked --workspace
cargo test --offline --locked -p shodo --no-default-features --test line_height_quirk
cargo test --offline --locked -p shodo --no-default-features --features complex-scripts --test line_height_quirk
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --workspace --no-deps
cargo fmt --all --check
git diff --check
```

The previous shared-output feature checks are excluded: another worktree's
cached library did not contain the new field. No source fixture or test
expectation was changed to hide that environment failure.

Both feature-isolated quirk suites passed 17 tests each. Workspace Clippy
with warnings denied, rustdoc with warnings denied, formatting and diff
whitespace checks also passed on the rebuilt dedicated output.

The browser behavior is implemented in Blink's
[`LogicalLineBuilder::CreateLine`](https://github.com/chromium/chromium/blob/main/third_party/blink/renderer/core/layout/inline/logical_line_builder.cc),
which retains the root metrics for list items in quirks mode.
