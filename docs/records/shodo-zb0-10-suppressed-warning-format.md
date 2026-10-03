# shodo-zb0.10: lazily format suppressed warning messages

`crates/shodo/src/sanitize.rs` `finite_or`/`length`/`non_negative*` built a
`format!` `String` before calling `WarningSink::push`, while
`crates/shodo/src/limits.rs::push` drops that message when the sink is already
suppressed or only needs the single `Suppressed` marker. Valid input never hits
this path; only hostile input with many invalid values pays the per-value
formatting cost.

`WarningSink::push_lazy(kind, || format!(...))` checks `suppressed` and
`len >= max` before evaluating the closure. `sanitize.rs` uses it for the four
dynamic messages (non-finite with fallback, saturated clamp, two negative
paths). Static `&str` warnings keep `push`. Message text, warning order,
`Suppressed` marker, `is_suppressed`/`checkpoint`/`remaining_limit`, and the
sanitized values are unchanged.

## Measurement

Target input is 2000 `open_inline` boxes sharing one style, each with 7 bad
edge floats (NaN, infinities, `2e7`, negatives). That is 14000 attempted
sanitize warnings. `ParagraphBuilder::build` with no system fonts keeps all
other limits at default.

Allocator probe (`CountingAllocator`, one `build` in scope):

| cap | baseline calls → candidate | baseline bytes → candidate |
| --- | ---: | ---: |
| `max_warnings=0` | 30309 → 16309 (−14000, −46%) | 7238992 → 6550992 (−688000, −9.5%) |
| `max_warnings=5` | 30315 → 16320 (−13995) | 7239574 → 6551816 (−687758) |
| default (1024) | 31342 → 18366 (−12976, −41%) | 7436505 → 6798817 (−637688, −8.6%) |

Live/peak bytes are unchanged per cap (for example `max_warnings=0` keeps
`live=2653565`, `peak_extra=3205573`); only transient dropped `String`s go
away. Stored warnings keep their heap (`default` live `2835982`).

Valgrind DHAT on `suppressed_warning_format` (15 builds: 5 per cap):

- baseline: `Total: 153,409,141 bytes in 643,772 blocks`
- candidate: `Total: 139,315,019 bytes in 356,975 blocks`
- delta: −14,094,122 bytes (−9.2%), −286,797 blocks (−44.5%)

Timing for the same 2000-box build is unchanged within noise
(about 35 ms/build dev profile before and after; DHAT runs about
1.2–1.7 s/build). No speed claim is made from allocation reduction alone.

## Equivalence

- `max_warnings=0`: exactly `[Suppressed("further warnings suppressed")]`.
- `max_warnings=5`: 5 real warnings in order
  (`NonFinite(margin)`, `Saturated(margin)`, `NonFinite(margin)`,
  `Negative(border)`, `NonFinite(border)`) then the marker.
- default: 1025 warnings (1024 + marker) with
  `(non_finite, saturated, negative) = (585, 147, 292)`, first 12 messages
  byte-identical before/after.
- `push_lazy` unit tests count closure evaluations (0 for `max=0` over 10
  pushes, 5 for `max=5` over 20 pushes) and assert `as_slice`,
  `is_suppressed`, `checkpoint()==None`, and `remaining_limit` match `push`.
- Existing sanitize/warning tests pass: `limits::tests`, `fragments`
  `non_finite_*`, `lines::non_finite_style_values_become_initial_or_zero`.
- `sanitize` still maps NaN/INFINITY to 0, `2e7` to `1e7`, negative
  border/padding to 0, and keeps negative margins; line-cache
  `checkpoint` gating is unchanged because `suppressed` transitions are
  identical.

## Verification

- `cargo test --locked -p shodo --lib limits::tests`
- `cargo test --locked -p shodo --test fragments non_finite`
- `cargo test --locked -p shodo --test lines non_finite_style_values_become_initial_or_zero`
- `cargo test --locked -p shodo-bench --features allocation-counting --test suppressed_warning_format`
- `cargo run --locked -p shodo-bench --example suppressed_warning_format`
- `cargo run --locked -p shodo-bench --features allocation-counting --example suppressed_warning_format`
- `valgrind --tool=dhat ./target/debug/examples/suppressed_warning_format`
- `cargo test --locked --workspace` with `RUSTFLAGS=-D warnings`
- `cargo fmt --all --check`

The allocator integration target is included explicitly in the CI allocator
test command so this regression check runs on every CI build.
