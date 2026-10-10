# raikiri overlay probe sources

The `.rs` files in this directory are **not a Cargo target** in this
workspace. `cargo build`/`cargo test` never compile them, and no
`Cargo.toml` in this repository lists them as a `[[bin]]` or example.

`tools/raikiri/raikiri_overlay.py` and `raikiri_library_overlay.py`
create fresh, disposable probe archives with frozen shodo and
[`fulgur-rs/raikiri`](https://github.com/fulgur-rs/raikiri) sources
and reference these files as `[[bin]]` targets using manifest-relative paths,
so they can be built and measured against raikiri's own crates
(`raikiri-dom`, `raikiri-style`, `raikiri-traits`, and raikiri's internal
layout integration). `main.rs` and `library.rs` reference
`../../bench/src/allocator.rs` (via `#[path]`) to reuse the same
allocation-counting instrumentation as `shodo-bench`, without adding a
Cargo dependency from `shodo-bench` on raikiri.

Saved JSON uses `./` for the shodo checkout root and `~/` for the current
home directory, including paths in command arguments, Cargo artifacts and
diagnostics. Other absolute paths, such as `/usr/bin/perf`, remain literal.
These names describe provenance; processes still receive the original paths.
The generated manifest paths resolve from the probe archive to this checkout,
which must remain available at that relative location while building.
The portable-path recipe is included in the saved harness fingerprint.

See `tools/raikiri/raikiri_overlay.py`, `raikiri_library_overlay.py`,
and `raikiri_measure.py` for the exact overlay, build, and measurement
recipe, and [`docs/raikiri-measurements.md`](../../../docs/records/raikiri-measurements.md)
for the recorded results.
