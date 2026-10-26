# SHODO-67: enforce glyph budgets during combined-width trials

The issue supplies the intended boundary: speculative TCY shaping must obey
the current glyph consumption of every owner, including local ruby bases,
before adding glyphs to retained output. Unselected candidates must roll back.

## Approach

Use the existing checked owner-chain charge in `LimitsLedger::shaped_glyphs`
for trials as well as ordinary shaping. Preserve `shaping_start` and
`trial_peak`, which guard first-line reuse. Rejecting only at commit is too
late; introducing another budget counter would duplicate the ledger.

Propagate a failed plain trial without shaping the same input again when
the ordinary scoped run windows match. If scoped windows differ, retain the
ordinary fallback because different windows can change glyph production.
A failed narrow trial continues to fall back to the admitted plain candidate.

This is the implementer's choice within the issue's stated approach, not a
claim of separate human design review. Preserve public APIs, error fields,
warning behavior and successful output under sufficient budgets. Candidate
work that exceeds a budget is intentionally stopped even if another width
candidate might have used fewer glyphs. Harfrust's allocation inside a single
window remains SHODO-23's separate concern.

## Evidence

Pin rejection timing with real-font shaper-call counts, and test local,
ancestor and current committed consumption directly through the ledger.
Exercise failed and successful candidate rollback and exact-cap admission.
Keep existing nested-ruby, scoped-window and first-line differential tests.

## Validation record

The five regression cases failed before the fix: three ledger tests admitted
over-budget trial charges, the local-cap fixture made two shaper calls instead
of one, and the second-group paragraph-cap fixture made four instead of three.
After the fix, the shape test group passed (56 passed, two manual probes ignored).
Existing differential sweeps also required owner-first error precedence; the
checked current charge now runs before the historical snapshot check.

`cargo fmt --all --check`, `cargo +stable clippy --workspace --all-targets
--offline -- -D warnings`, and warning-denied workspace rustdoc passed.
Stable was Rust/Clippy 1.97.1. The initial Clippy 1.91.0 attempt reported an
existing `nonminimal_bool` warning in `ruby/pairing.rs`; stable passed without
changing that unrelated code. A fresh read-only bypass/regression review found
no concrete issues; that review was static, not an independent test run.

`cargo test --workspace --features shodo-harness/accesskit --offline --quiet`
passed, including the final 1,115 library tests and the workspace integration
and documentation tests. Existing manual/external-font tests remain ignored.
Feature-isolation, MSRV and Wasm checks are also exercised by the PR's CI.
