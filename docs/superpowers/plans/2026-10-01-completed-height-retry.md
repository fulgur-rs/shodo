# Completed height retry implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Avoid rebuilding clean rejected line geometry on identical height retries.
**Architecture:** Private single owned CompletedLine entry in LayoutContext, exact conservative key, move on accept, capacity accounting and invalidation.
**Tech Stack:** Rust and existing fixed-font full-output benchmark/stdlib Python evidence tools.
**Spec:** docs/superpowers/specs/2026-10-01-completed-height-retry.md

## Global Constraints
All public APIs, limits, warnings, source/glyph/recursive geometry and progress unchanged. Height sanitization and per-line budget reset still execute. Retain at most one 64KiB owned entry; shared owner extension measured separately. Worktree/PR/CI/merge/cleanup already authorized. Protect originals and saved spikes; no switch blockers.

## Review Focus
- Exact key and live font/atomic invalidation; invalid/done/block/first-line/plan paths release owners.
- Warning suppression/saturation/resource fallback bypass, per-line budget reset and recursive Ruby calls.
- Actual capacity accounting includes headers/key/unused capacities; shared owners separately; shrink and accepted move release cache.
- Full Line and non-Line events, continuation/retry independence and actual constructor/clone work.
- All allocator gross/freed/net/whole peak and adverse balanced timings retained; observer neutrality and font licenses pinned.

### Task 1: Bounded owned retry and regression contracts
**Files:** crates/shodo/src/{context.rs,line/mod.rs,line/completed.rs,output.rs,output/owned_bytes.rs,output/line.rs} and focused tests.
**Interfaces:** Private CompletedLine/Key, real test-only constructor counter, no public changes.
- [ ] Write literal same-token four-reject/accept test with actual constructor count1 and clones0; watch baseline construct5 fail.
- [ ] Implement exact eligible retry key, owned transfer, invalidation, conservative warnings/resource controls and capacity accounting.
- [ ] Check key changes, atomics/live font, resource/suppression/height sanitization, plan/first-line/floats, oversize/shrink/owner/independent returned output.
- [ ] Run core/fmt/alltargetClippy/docs and commit; gate cargo test -p shodo --lib.

### Task 2: Fixed-font work, allocation and time evidence
**Files:** height_geometry example (or focused dedicated example), target/performance-artifacts/c91-4-height-retry recipes; docs/records/data.
**Interfaces:** Identical public output/events/ordered warnings; separate actual constructor/metrics/Ruby/scan/shaper observers and counter-free timing.
- [ ] Freeze baseline/candidate time/memory/work producers and licenses; capture full original195 controls plus relevant owner/cap/budget/key cases.
- [ ] Prove output/observer allocator equality, actual eliminated work, calls/gross/freed/net/peak and rejected-only retained ownership/release.
- [ ] Complete checks and strict54 then balanced counter-free sequential and selected fresh A/B; keep adverse cases and measured adoption decision.
- [ ] Losslessly archive raw, source/font/license/producer/binary/check identities and independent verifier.

### Task 3: Record, review and integrate
**Files:** docs/records/completed-height-retry.md, data manifest/raw and native ledger.
**Interfaces:** One fresh Astra branch review of exact final HEAD and independently restored evidence.
- [ ] Verify archive, source/default output/timing/retention claims and checks; review every declined behavior.
- [ ] Fix Critical/Important in one RED/GREEN pass, ledger minor rulings; no unverified success claims.
- [ ] Create PR, exact head all CI SUCCESS, merge, verify actual main/source/protected/saved, bd close readback and owned worktree cleanup; then .5.
