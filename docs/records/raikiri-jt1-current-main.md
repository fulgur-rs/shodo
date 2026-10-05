# Current-main Shodo caller versus saved S4 (shodo-7dt)

This comparison measures the saved S4 candidate and the candidate caller built
from current main against the same pinned native raikiri caller. It separates
the old S4 measurement from the performance present in Shodo main as of
`bb2e87ee`. It is an investigation record, not a WPT verdict or a switching
decision.

## Result

Both saved S4 and current main are slower than native in all 16 pipeline and
layout pairs. Current main is faster than the saved S4 candidate in both
documents and windows. The pipeline gap is smaller on current main, but it is
still measurable: the paired candidate/native ratio is 1.269 for out-of-flow
and 1.164 for auto, compared with 1.368 and 1.351 for the saved S4 candidate.

Times below are medians of the 16 independent per-process warm medians. The
ratio is the median of the 16 paired ratios, so dividing the displayed engine
medians does not have to produce the displayed ratio. `Extra time` is candidate
minus native and is approximate when comparing pipeline with its layout
sub-window, which were measured in separate processes.

| Operation | Reference | Saved S4 native / candidate | S4 ratio | Current main native / candidate | Current ratio | Extra time S4 → current |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `pipeline` | out-of-flow | 919.0 / 1257.2 µs | 1.368 (1.334–1.412) | 896.8 / 1139.3 µs | 1.269 (1.157–1.283) | 338.2 → 242.5 µs (-28.3%) |
| `pipeline` | auto | 811.1 / 1095.8 µs | 1.351 (1.336–1.479) | 791.2 / 920.2 µs | 1.164 (1.150–1.190) | 284.7 → 129.0 µs (-54.7%) |
| `layout` | out-of-flow | 159.1 / 482.1 µs | 3.012 (2.912–3.137) | 159.8 / 386.4 µs | 2.415 (2.355–2.485) | 322.9 → 226.5 µs (-29.8%) |
| `layout` | auto | 117.5 / 394.8 µs | 3.358 (3.205–3.467) | 116.9 / 236.2 µs | 2.023 (1.982–2.075) | 277.2 → 119.3 µs (-57.0%) |

The candidate-minus-native layout time accounts for most of the pipeline gap
in both arms. Subtracting the independently measured windows leaves about
7.5–16.0 µs outside layout. This subtraction is approximate and does not
attribute a particular function as the cause.

Allocation counters show the same direction. They are requested heap bytes and
allocator calls, not RSS; each value is a median of three processes, with nine
warm samples per process.

| Operation | Reference | S4 bytes / calls ratio | Current-main bytes / calls ratio |
| --- | --- | ---: | ---: |
| `pipeline` | out-of-flow | 1.191 / 1.939 | 1.134 / 1.639 |
| `pipeline` | auto | 1.222 / 1.884 | 1.149 / 1.377 |
| `layout` | out-of-flow | 1.530 / 3.990 | 1.293 / 3.019 |
| `layout` | auto | 1.700 / 4.812 | 1.410 / 2.594 |

The pipeline `peak_extra_bytes` and retained `net_bytes` ratios are unchanged
between the two candidates. In the layout window, the candidate still allocates
more, while its retained net bytes are about 0.7–0.8% of native. These owner
and scope differences limit what the allocation comparison says about an
algorithm in isolation.

## Inputs and method

- Original WPT checkout: `97ea26e26a2aac3eec7e770650b25e7049ed4a4e`.
- Native raikiri and saved candidate: `ab7e619a8f321f03de8b8c8b9342954868e044c8`
  and `fe67a281210fbc52d22032911ac3584405aa8198`.
- Current Shodo commit: `bb2e87ee3f75fdf4c9cf24e459db0323274ed5ed`.
- The two original hyphens reference documents, the same ordered 88 font SHA256
  entries per document, and an `800x600` CSS-pixel viewport were used in both
  arms. The original selected-pages file was read only. The filtered S4
  selection and current-main selection are saved alongside the result JSON.
- Each arm ran 16 independent time-process pairs per document and operation.
  Native/candidate order alternated inside each arm; the S4/current-main arm
  order also alternated per pair. Each process parsed/configured the input once,
  then supplied nine warm samples. The process was pinned to CPU 11, the least
  busy allowed CPU at start.
- Allocation counting used three independent pairs per arm, document, and
  operation, with the engine order alternating and the same nine warm samples.
- The environment was labelled `quiet`: 1-minute load average was 0.15 before
  and 0.34 after; the selected CPU measured 0% busy before and after. Load
  during the run was not sampled. The host was x86_64 Linux 7.2.5-3-omarchy on
  an AMD Ryzen 5 5600G.
- Both time binaries used a release profile with opt-level 3 and no debug info;
  memory binaries additionally enabled `allocation-counting`. Both candidates
  enabled `complex-scripts`. The current-main build used rustc 1.96.0 and
  `-D warnings`. The saved S4 build manifest records `cargo +stable` but not the
  exact rustc version, so equality of the compiler patch version is not claimed.

The saved and current selection hashes are respectively
`0398e9da2e8e5238f4b04342ed3ad802253fef4282345b81e77f1411ecc25084` and
`6d98385b44c7019e25b36e2fecea62665cff8d03edecd30ca3d50c335736e5d1`. Both
derive from the read-only original selection (`82891b7369a6de7966c7094fb6ea2ab439216bf1f92f067723e1b747bc3aa448`).
The current candidate selection differs from saved S4 only on seven
out-of-flow nodes: their content width is `96.00018310546875` versus `96.0`,
and border inline size is `102.00018310546875` versus `102.0`. The auto
selection geometry is identical. These differences come from the separately
validated current-main candidate geometry; they were not edited into the saved
S4 selection.

Full per-process warm samples, allocation counters, pair ordering, command
arguments with machine-local paths removed, input/binary hashes, and selection
geometry differences are in
`raikiri-7dt-current-main-comparison.json`.
Current-main build inputs and binary provenance are recorded in
`raikiri-7dt-current-provenance.json`.
The standalone selections are
`raikiri-7dt-saved-selection.json`
and
`raikiri-7dt-current-selection.json`.

## Limits

The native caller prepares all DOM text. The candidate projects only the
paired leaf IFCs. Native retains its laid-out DOM clone and atomic map; the
candidate returns geometry blocks and releases paragraph, line, and context
within the caller while configured font caches remain shared. The two paths
therefore measure different caller input and retained-output ownership.

The probe consumes the pinned reference documents and compares caller output;
it does not run the WPT harness or establish WPT PASS. Cold startup, painting,
and the acceptable performance budget are outside scope. The measured current
main gap does not decide whether a future switch is necessary.

## Reproduction

`tools/raikiri/jt1_current_main_build.py` rebuilds the current-main probes from
the saved S4 source archive, pinned current Shodo commit, and checked-in
selection. The saved S4 binaries, source snapshots, and original full
selection are under the existing ignored `target/8ei-artifacts` artifacts.
From the repository root, set `ARTIFACTS` to that artifact directory, `WPT` to
the pinned checkout, `CURRENT_BUILD` to an empty directory, and `BUILD_TARGET`
to a writable Cargo target directory:

```sh
ARTIFACTS="$PWD/target/8ei-artifacts"
WPT="${WPT:-$HOME/.cache/raikiri/wpt}"
CURRENT_BUILD="$(mktemp -d /tmp/shodo-7dt-current-build.XXXXXX)"
BUILD_TARGET="$PWD/target/jt1-current-main-build"

python3 tools/raikiri/jt1_current_main_build.py \
  --source-archive "$ARTIFACTS/probe" \
  --source-provenance "$ARTIFACTS/probe-archive.json" \
  --expected-source-tree-sha256 f898b0574588d1c190a516c8ff2442fe120859b88ded8b4ba6963cc0a7e9bb0c \
  --toolchain 1.96.0 \
  --shodo-worktree "$PWD" \
  --shodo-revision bb2e87ee3f75fdf4c9cf24e459db0323274ed5ed \
  --raikiri-revision ab7e619a8f321f03de8b8c8b9342954868e044c8 \
  --current-selection dev/raikiri/data/raikiri-7dt-current-selection.json \
  --output-dir "$CURRENT_BUILD" \
  --target-dir "$BUILD_TARGET"

python3 tools/raikiri/jt1_current_main.py \
  --wpt "$WPT" \
  --saved-s4-time "$ARTIFACTS/pipeline-release/measurement-probe-time" \
  --saved-s4-memory "$ARTIFACTS/pipeline-release/measurement-probe-memory" \
  --saved-s4-builds-manifest "$ARTIFACTS/pipeline-release/builds.json" \
  --saved-s4-archive-provenance "$ARTIFACTS/probe-archive.json" \
  --saved-s4-source-archive "$ARTIFACTS/probe" \
  --saved-s4-cargo-lock "$ARTIFACTS/probe/Cargo.lock" \
  --saved-s4-selection dev/raikiri/data/raikiri-7dt-saved-selection.json \
  --original-selection "$ARTIFACTS/selected-pages.json" \
  --current-time "$CURRENT_BUILD/measurement-probe-time" \
  --current-memory "$CURRENT_BUILD/measurement-probe-memory" \
  --current-selection dev/raikiri/data/raikiri-7dt-current-selection.json \
  --current-provenance "$CURRENT_BUILD/current-main-provenance.json" \
  --current-candidate-cargo-lock "$CURRENT_BUILD/current-main-Cargo.lock" \
  --current-source-root "$PWD" \
  --s4-shodo-commit fe67a281210fbc52d22032911ac3584405aa8198 \
  --current-shodo-commit bb2e87ee3f75fdf4c9cf24e459db0323274ed5ed \
  --raikiri-commit ab7e619a8f321f03de8b8c8b9342954868e044c8 \
  --output dev/raikiri/data/raikiri-7dt-current-main-comparison.json \
  --scratch /tmp/shodo-7dt-measurements
```

The runner checks the saved build manifest and source snapshot, archive and
dependency revisions, current probe sources and candidate manifests, selection,
and time/memory binary hashes before it measures. Raw probe JSON files are
written under `--scratch`.
