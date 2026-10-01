# Bounded edge-cache eviction evaluation

Goal: identify whether capacity-triggered whole-cache clearing causes repeated edge shaping, then evaluate minimum sufficient partial eviction under exactly the existing budgets.

Current limits remain256 entries,32768 glyph-equivalent total cost and1024 single-window cost. Owner changes/reset remain whole-cache invalidations. Replacement, warned, saturated and oversized results remain uncacheable; every request still charges the operation budget before lookup. Public API/default limits and all source/glyph/geometry/warnings/progress are unchanged.

First observe actual lookup/hit/miss, owner/reset versus entry/cost clear causes, removed windows, retained cost and two real shaper sites with fixed fonts and cold/warm/width/plan controls. Distinguish fresh empty initialization from clearing retained entries. Then test partial hash-order eviction: remove only enough existing entries to admit the new clean window, with no additional recency storage or per-hit update. Compare with full clear. LRU/CLOCK require extra metadata/touches; this first candidate isolates partial retention without adding bookkeeping allocations.

Adopt only when causal shape/time/allocation benefit justifies actual retained/net/peak/lifetime costs. Otherwise restore the original policy and record measured rejection. Synthetic threshold/locality/churn/one-window controls complement actual shared-ligature, multiple-width/plan, first-line/Ruby/float/forced/atomic/font-generation/resource controls. No cap increases or blanket hit-rate/CPU/RSS claims.

Complete independent full output/warnings and allocator-neutral observer checks before balanced counter-free timing. Measure requested calls/gross/freed/net/peak, actual map/window retention and owner release separately. All raw evidence, logs and host metadata stay local as explicitly requested; publish only implementation/tests/harness and aggregate results. Preserve original diagnostics and saved spikes. No production-switch blocker.
