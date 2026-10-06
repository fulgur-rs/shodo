# shodo-9kt: pending vertical-align credit

## Scope and reference

Base: `1cae8e8` (shodo-e1h, PR #235). Quirks-mode forced breaks must add
parent struts according to Blink's `HasMetrics()` including pending
vertical-align descendants. Existing direct-text/edge eligibility, glyph
positions and the flag-off path remain unchanged.

Reference: Chromium 152.0.7977.82 (Arch Linux), measured on 2026-10-06.
Blink's [`InlineBoxState::HasMetrics`](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/inline/inline_box_state.h)
includes pending descendants;
[`ApplyBaselineShift`](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/layout/inline/inline_box_state.cc)
queues top/bottom on the nearest top/bottom ancestor or root, and text-top/
text-bottom on the parent. Immediate shifts (sub, super, middle, lengths)
leave empty metrics empty. Therefore “non-baseline Open” must mean these
four pending values, not every value other than baseline.

## Measured empty-child cases

Heights in CSS px, without a doctype (quirks). Parent line-height 40,
root line-height 20; monospace font-size 10. The child is empty and closes
before the forced break. “Nested” is a baseline parent; “root” puts the
child directly in the block; “top parent” makes the parent top-aligned.

| Child vertical-align | Nested | Root | Top parent | Atomic in baseline parent |
| --- | ---: | ---: | ---: | ---: |
| baseline | 40 | 20 | 40 | 2 |
| sub | 40 | 20 | 40 | 2 |
| super | 40 | 20 | 40 | 2 |
| middle | 40 | 20 | 40 | 2 |
| 0px | 40 | 20 | 40 | 2 |
| text-top | 0 | 0 | 0 | 2 |
| text-bottom | 0 | 0 | 0 | 2 |
| top | 40 | 0 | 0 | 40 |
| bottom | 40 | 0 | 0 | 40 |

The original Chromium152 matrix expectations pinned by shodo-e1h are now
asserted directly: BA/BB/BO/CL/CP = 40, CE = 60, DI/CO = 0.

## Static rule and complexity

Root depth is 0. Each content unit carries its nearest inclusive top/bottom
box depth (`tbdepth`), or 0 if none. Top/bottom atomics introduce a barrier
at parent depth + 1. A parent's on-line subtree before its last forced break
is queried, excluding the parent's own Open and trimming collapsible text.
A parent without top/bottom alignment has metrics iff `min(tbdepth) < depth(p)`.
Root and top/bottom parents have metrics iff there is any content credit.
An empty pending Open supplies credit but no strut or height of its own.

Retained measurement seeds only the current line's ancestors and uses an
Open/Close stack: O(line units + initial ancestry), O(depth) scratch space.
The quirk index precomputes box depths in preorder and joins credit by min:
O(n) build and storage, O(log n) range query. No extra allocation or tree
fields are added when `line_height_quirk` is false; the shared Summary is
unchanged. Reshaped edge windows retain static content credit through `bare`
leaves; trailing text uses `kept` leaves.

## Verification

Rust toolchain: `rustc 1.96.0 (ac68faa20 2026-05-25)`.

- Original line-height suite before changes: 14 passed.
- Chromium expectations failed before the fix (BA: 2 vs 40;
  empty text-top nested child: 40 vs 0).
- With retained correction but the old index restored, the new range parity
  test failed on `0..8`: indexed height 30 vs retained 0. The index was then
  restored to the corrected implementation.
- Focused integration tests: 15 passed. Focused library quirk tests: 15 passed,
  including height/baseline range parity and bounded work with 64/128
  pending children. Continuations beginning at Close are included.

Independent read-only code review: Ready, no findings. Reviewed subtree
boundaries, continuation Close units, atomic barriers, pending ghost groups,
trimming, flag-off behavior and query complexity. Existing list-item behavior
(shodo-qu8) remains outside this task.

All checks use `TMPDIR="$HOME/tmp"`. The first full workspace run inherited
a literal `CARGO_TARGET_DIR=~/tmp`, which created caches relative to each
Cargo working directory. Those task-created directories were removed after
the run completed. Follow-up checks and the commands to reproduce these
results use an explicit `CARGO_TARGET_DIR="$PWD/target"`. Temporary
probe/profile/log files are removed after results are recorded.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed |
| `cargo test --offline --workspace` | 92 targets; 1,555 passed, 0 failed, 8 ignored |
| `cargo test --offline -p shodo --no-default-features` | 30 targets; 1,067 passed, 0 failed, 8 ignored |
| `cargo test --offline -p shodo --no-default-features --features complex-scripts` | 30 targets; 1,070 passed, 0 failed, 8 ignored |
| `RUSTDOCFLAGS="-D warnings" cargo doc --offline --workspace --no-deps` | Passed |

## Browser reproduction

Save the following as a temporary HTML file under `~/tmp`. Run:

```sh
TMPDIR="$HOME/tmp" chromium --headless --no-sandbox --disable-gpu \
  --disable-dev-shm-usage --user-data-dir="$HOME/tmp/shodo-9kt-profile" \
  --dump-dom file://"$HOME/tmp/shodo-9kt-probe.html"
```

The `<pre id="out">` contains the measured name/height pairs.
Delete only this probe and its dedicated browser profile after use.

```html
<html><style>body {font:10px monospace;line-height:20px;margin:0}img{width:2px;height:2px;display:inline-block}</style><div id="host"></div><pre id="out"></pre><script>
let cases = [];
for (const va of ['baseline','sub','super','middle','0px','text-top','text-bottom','top','bottom']) {
  cases.push([va + '/empty-inner', `<span style="line-height:40px"><span style="vertical-align:${va}"></span><br></span>`]);
  cases.push([va + '/empty-root', `<span style="vertical-align:${va}"></span><br>`]);
  cases.push([va + '/atomic', `<span style="line-height:40px"><img style="vertical-align:${va}"><br></span>`]);
  cases.push([va + '/top-parent', `<span style="line-height:40px;vertical-align:top"><span style="vertical-align:${va}"></span><br></span>`]);
}
let out=[];for (const [name,html] of cases) {let d=document.createElement('div');d.innerHTML=html;host.appendChild(d);out.push([name,d.getBoundingClientRect().height]);}document.querySelector('#out').textContent=JSON.stringify(out);
</script>
```
