# Reuse finalized line geometry for paint

Baseline: `454fb916a9ae9addd6772417b64986a75717be4c`.

`LineLayout::paint_spans(&self, line: usize) -> Option<Vec<PaintSpan<'a>>>`
uses the layout's retained source segments and the same paint resolver as
`Line::paint_spans()`. It does not call `LineGeometry::new` again. The existing
standalone API keeps its geometry path, including omission of unused caret stops.
Returned style references borrow the original accepted lines and can outlive
the index. An invalid line index returns `None`; a valid line without paintable
text returns an empty vector. Main-text spans do not include annotation lanes.

## Regression evidence

The reuse test counts real `LineGeometry::new` calls through a test-only,
thread-local counter. Before implementation, a temporary test-local adapter
called the existing standalone paint API: painting four indexed lines three
times increased the construction count from 4 to 16, and the test failed.
The final indexed API leaves the count at 4 and passes the same assertion.

Fixed-font paint tests compare indexed and standalone source ranges, rectangle
coordinates, style-reference identity, font/size/metrics and both resolved
decorations, then run the existing literal geometry expectations on indexed
spans. Coverage includes bidi, justification, ffi and combining sources,
soft hyphens, fallback metrics, first-line paint, tabs, all vertical/sideways
modes, TCY, ruby bases and nested annotation lines. A separate regression checks
multiple line indices, block offsets, ellipsis, index-drop lifetimes and
empty/atomic-only bounds. Synthetic GDEF tests on the established real Latin
fixture additionally assert declared caret widths, variation/fallback behavior
and TCY compression.

Validation commands use `cargo +stable`, `TMPDIR="$HOME/tmp"`, an empty
`RUSTC_WRAPPER`, `CARGO_BUILD_JOBS=2`, a task-specific target under `~/tmp`,
and `taskset -c 3-4`. Test commands override the runner with
`--config 'target.x86_64-unknown-linux-gnu.runner="env"'`.

- `test -p shodo --lib paint_spans_reuse_accepted_geometry_without_rebuilding`:
  RED and GREEN observed as above.
- `test -p shodo --lib gdef`: both GDEF/variation and compressed-TCY regressions
  passed.
- `test -p shodo-harness --test paint_styles --test ruby_paint --test hit_test
  --test ruby_geometry`: 57 passed.
- `test -p shodo --test '*'`: 420 passed, 1 existing ignored test, 34 suites.
- `test -p shodo --doc`: 10 passed.
- `fmt --all -- --check`: passed after applying rustfmt.
- `clippy --workspace --all-targets -- -D warnings`: passed.
- `clippy --workspace --all-targets --features shodo-harness/accesskit
  -- -D warnings`: passed.
- Full `test -p shodo --lib`: interrupted with SIGINT at the coordinator's
  request, with no observed failing assertions. This is not a full-suite pass;
  the coordinator runs the full workspace on the combined final changes.
- Optional `clippy --workspace --all-targets --all-features -- -D warnings`:
  failed at the unchanged `dev/bench/src/bin/font_match_probe.rs:224`,
  `clippy::needless_question_mark` in the `allocation-counting` feature's
  `Ok(build_collection(...)?)`. No unrelated source edit was made.

The `source_geometry` example was built and smoke-tested for `plain 16 1` and
`latin-long 1 1` in all four paint/combined modes. Geometry, selection and paint
hashes matched across all four modes in both cases. The debug smoke run is a
correctness check, not a performance measurement.

## Reproducible measurement scopes

`dev/bench/examples/source_geometry.rs` retains the ordinary `paint`, `index`,
`selection`, `combined` and `build` scopes and their output-drop accounting.
`shared-paint` paints through the prepared index; `combined-shared` builds a
fresh index and obtains indexed spans before dropping both outputs. Hashing,
geometry digests, warning checks and fixture preparation remain outside the
measurement. Indexed and standalone paint hashes are checked before and after
measurement, alongside the existing paint and selection stability checks.

```sh
cargo +stable run -p shodo-bench --release --example source_geometry -- combined latin-long 64 21
cargo +stable run -p shodo-bench --release --example source_geometry -- combined-shared latin-long 64 21
cargo +stable run -p shodo-bench --release --example source_geometry -- paint latin-long 64 21
cargo +stable run -p shodo-bench --release --example source_geometry -- shared-paint latin-long 64 21
```

Use separate builds with `--features allocation-counting` for allocation
measurements. These scopes support the coordinator's isolated A/B comparison;
this implementation record makes no latency or allocation-reduction claim.
