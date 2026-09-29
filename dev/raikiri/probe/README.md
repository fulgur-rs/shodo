# raikiri overlay probe sources

The `.rs` files in this directory are **not a Cargo target** in this
workspace. `cargo build`/`cargo test` never compile them, and no
`Cargo.toml` in this repository lists them as a `[[bin]]` or example.

`tools/raikiri/raikiri_overlay.py` and `raikiri_library_overlay.py`
copy these files into a fresh, disposable checkout of the separate
[`fulgur-rs/raikiri`](https://github.com/fulgur-rs/raikiri) repository
(pinned to a specific revision) and add them there as `[[bin]]` targets,
so they can be built and measured against raikiri's own crates
(`raikiri-dom`, `raikiri-style`, `raikiri-traits`, and raikiri's internal
layout integration). `main.rs` and `library.rs` reference
`../../bench/src/allocator.rs` (via `#[path]`) to reuse the same
allocation-counting instrumentation as `shodo-bench`, without adding a
Cargo dependency from `shodo-bench` on raikiri.

See `tools/raikiri/raikiri_overlay.py`, `raikiri_library_overlay.py`,
and `raikiri_measure.py` for the exact overlay, build, and measurement
recipe, and [`docs/raikiri-measurements.md`](../../../docs/records/raikiri-measurements.md)
for the recorded results.
