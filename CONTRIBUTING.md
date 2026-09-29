# Contributing to shodo

shodo is in early development. Contributions can include reproducible bug
reports, regression tests, documentation improvements, and implementation work.
For a substantial feature or public API change, open an issue describing the use
case and proposed behavior before investing in a large patch.

## Reporting bugs and proposing features

Search [existing issues](https://github.com/fulgur-rs/shodo/issues) first. For a bug,
include a minimal Rust reproduction, expected and actual output, shodo revision,
`rustc --version`, OS/target, and enabled Cargo features. For layout differences,
include the text, computed styles, available width, writing mode/direction, and
font identity. Prefer the checked-in fixture fonts when they reproduce the issue.
Include build/layout warnings and relevant source ranges or geometry; a screenshot
alone often cannot explain a shaping or line-breaking difference.

For a feature request, describe who needs the behavior, a concrete input/output
example, and current workarounds. Link relevant CSS or Unicode requirements if
they explain the expected behavior.

## Development setup

Install Rust 1.89.0 or later with rustfmt and Clippy. On Linux, default-feature
builds need Fontconfig development files and pkg-config. Workspace development
uses the same Rust toolchain as the library.

```sh
git clone https://github.com/fulgur-rs/shodo.git
cd shodo
rustup component add rustfmt clippy
cargo test --workspace
```

Cargo downloads dependencies on the first build. Fixture development dependencies
include raikiri crates pinned to a Git revision; a first workspace build needs
access to that repository. After dependencies are cached, use `--offline` where
appropriate. No browser or font download is needed for ordinary Rust tests.

The published `shodo` package on crates.io only contains `crates/shodo/src/`,
`crates/shodo/tests/`, `crates/shodo/examples/`, and top-level metadata (see
`[package].include` in `crates/shodo/Cargo.toml`); `dev/`, `docs/`, and
`.github/` are not part of the tarball. Building or testing the published
package therefore requires a Git checkout of this repository, not
`cargo download`/tarball extraction, because `crates/shodo/tests/vertical.rs`
and `crates/shodo/tests/japanese.rs` currently load real fonts from
`dev/fixtures/assets/` (tracked as shodo-c6r; the plan is to move that
dependency into `dev/harness` so the published package's own test target no
longer needs it).

| Location | Purpose |
| --- | --- |
| `crates/shodo/src/` | Library implementation and module tests. |
| `crates/shodo/src/test_support/` | Shared fixed-font bytes for `src/` unit tests only (`#[cfg(test)]`); not part of the published crate. |
| `crates/shodo/tests/` | Public API integration tests. |
| `crates/shodo/examples/` | Library examples. |
| `dev/fixtures/` | Fixed fonts, original sample cases, and a minimal loader; no rendering/raikiri dependencies. |
| `dev/harness/` | Rendering, snapshots, browser comparison, float, and AccessKit checks that consume the fixtures. |
| `dev/raikiri/` | raikiri-integration checks (contract verification, source coverage, style diffs); pulls in the git-pinned raikiri crates. |
| `dev/bench/` | Standalone performance and allocation tools. |
| `docs/` | Integration contracts, measurements, and design history. |

`crates/shodo` is the default workspace member. Use `--workspace` to include
development packages; plain `cargo test` does not exercise the entire workspace.

## Checking a change

Run these checks from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p shodo --no-default-features
cargo test -p shodo --no-default-features --features complex-scripts
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Choose additional checks for the paths you change. The
[CI workflow](.github/workflows/ci.yml) is the full list of required automated
checks, including Rust 1.89.0 and Wasm builds.

For accessibility and optional-feature work:

```sh
cargo clippy --workspace --all-targets --features shodo-harness/accesskit -- -D warnings
cargo test --workspace --features shodo-harness/accesskit
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --features shodo-harness/accesskit
cargo run -p shodo-harness --features accesskit,shodo/complex-scripts --example accessibility
```

Run feature-isolation tests with `-p shodo` as above. Workspace packages can
enable additional shodo features through Cargo feature unification.

For Wasm-sensitive changes, install the target and check the CI configurations:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p shodo --target wasm32-unknown-unknown
cargo build -p shodo --target wasm32-unknown-unknown --no-default-features
cargo build -p shodo --target wasm32-unknown-unknown --no-default-features --features accesskit
```

Python tooling checks use Python 3.12 in CI and the pinned FontTools dependency:

```sh
python3 -m venv /tmp/shodo-fonttools
/tmp/shodo-fonttools/bin/python -m pip install -r dev/fixtures/tools/requirements.txt
/tmp/shodo-fonttools/bin/python -m unittest discover -s dev/fixtures/tools -v
/tmp/shodo-fonttools/bin/python -m unittest discover -s dev/bench/tools -v
/tmp/shodo-fonttools/bin/python dev/fixtures/tools/regenerate.py --check
```

Python is for development tooling; ordinary library users do not need it.

## Tests, fixtures, and expected output

Add a regression test for a behavior change. Public API behavior belongs in
`crates/shodo/tests/`; real-font shaping and rendering regressions belong in
`dev/harness/tests/` (data-integrity checks on the checked-in fonts/corpus
themselves stay in `dev/fixtures/tests/`). Use fixed registered fonts with
system discovery disabled for deterministic assertions. Keep expected
geometry or glyph ownership grounded in the behavior being tested, rather
than copying the implementation's calculation.

`crates/shodo/tests/vertical.rs` and `crates/shodo/tests/japanese.rs` are a
known, tracked exception: they load real fonts directly from
`dev/fixtures/assets/` instead of following the rule above. Do not add
further real-font `include_bytes!` calls under `crates/shodo/tests/`;
new real-font regressions belong in `dev/harness/tests/` until these two
files are migrated there (shodo-c6r).

Normal checks do not rewrite expected data:

```sh
cargo run -p shodo-harness --example snapshots -- --output target/snapshot-report
cargo run -p shodo-harness --example browser_compare -- --check
```

Snapshot reports require a new output directory on each run. Open the generated
`index.html` to inspect expected, actual, and difference images. For an intentional
layout change, follow the [snapshot update procedure](docs/snapshot-tests.md),
review both pixel and geometry changes, and include the baseline diff in the same
PR. Do not update expectations just to make an unexplained failure pass.

Font or corpus changes must preserve provenance, hashes, and the separate OFL
notices. Follow [fixture reproduction and updates](dev/fixtures/README.md).
Browser recollection and the exact difference ledger have a separate
[review procedure](docs/browser-comparison.md). Performance work should use the
[standalone harness](docs/performance-measurements.md) and report comparable
machine/toolchain conditions; avoid treating one timing run as proof of a speedup.

## Pull requests

Create a branch from `main` in your checkout or fork and keep the patch focused on
one problem. Explain the trigger and resulting behavior, link related issues,
and list the checks you actually ran. State checks you could not run and why.

Update relevant guides and examples when public contracts change. Describe API,
feature, dependency, and baseline changes so reviewers can assess compatibility.
Keep fixture fonts and rendering/tool dependencies in the development packages
unless the library itself needs them.

Existing commits use prefixes such as `feat:`, `fix:`, `docs:`, and `test:`, often
with a scope. Follow that style when useful. Generated reports and local build
outputs do not belong in a PR unless they are an intentional checked-in fixture
or measurement record.

Code and original sample text use [MIT](LICENSE-MIT) OR
[Apache-2.0](LICENSE-APACHE). Bundled font assets retain their separate licenses
and provenance; see the [fixture guide](dev/fixtures/README.md).
