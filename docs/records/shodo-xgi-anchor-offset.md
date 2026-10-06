# Accepted-line offsets on out-of-flow anchors

Issue: `shodo-xgi`. Base: `d784bb1` (shodo 0.0.18).

`AnchorFragment::block_offset` is the logical block offset of its accepted
line in the layout container. It includes `LineConstraint::block_offset`,
including negative placement, and is retained with the line. It is not a
baseline, physical y coordinate, or paragraph line index. Convert it with
`inline_position` using the accepted line's writing mode and used direction;
do not add the line offset a second time. Inline geometry is unchanged.

The public struct gains a field: downstream struct literals must supply
`block_offset`; exhaustive field patterns must include it or `..`.

## Verification

The new public API test first failed because `AnchorFragment` had no
`block_offset`. With the implementation it passes for horizontal-tb,
vertical-rl and vertical-lr, LTR/RTL, the first and second line (offsets
0 and 10), a caller offset of -12.5, and retained clones.

Commands used with the real stable toolchain, cached offline dependencies,
`TMPDIR=~/tmp`, and the goal's own Cargo output directory:

- `cargo test --offline --locked --workspace`: 1595 passed, 0 failed,
  8 ignored.
- `cargo test --offline --locked -p shodo --test fragments out_of_flow`:
  2 passed.
- `cargo test --offline --locked -p shodo --no-default-features --test fragments`:
  17 passed.
- `cargo test --offline --locked -p shodo --no-default-features --features complex-scripts --test fragments`:
  17 passed.
- `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`:
  passed.
- `cargo fmt --all --check`, `git diff --check`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --offline --locked --workspace --no-deps`:
  passed.

Independent review found no blocking issues. Sideways modes, retry and
ellipsis were inspected for unchanged offset propagation but are not
additional assertions in the new test.

An earlier workspace verification was interrupted when a concurrent build
replaced a test executable in a shared output directory. The complete run
reported above used a dedicated output directory and succeeded. No fixture
or test expectation was changed to resolve that environment failure.
