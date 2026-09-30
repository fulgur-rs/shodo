# Variable font warm-match diagnosis — shodo-sbp.10

Diagnose the existing hash/LRU cache on fixed real Roboto Flex bytes (pinned Google Fonts commit23e54b51ddffbc7713c583748e3bd86f62b1fa4a), with wght/wdth/slnt/opsz, normal/italic/oblique, explicit settings, and size-adjust. This is a bounded diagnostic task; no new matching subsystem or public API.

Separate public match_cluster query normalization from normalized/scripted internal warm hits, negative hits, cold misses, disabled/full caches, generation invalidation, and retained matches. Count actual hits/misses in a counter-only overlay; measure allocation and time in separate executables. Long text N1024/4096/16384 and many styles8/64 must preserve warnings, source/glyph/geometry, instance coords/variations/size and font identities. Treat absent ital axis honestly; exercise ital-axis conversion independently with synthetic CI fixtures if an implementation needs it.

If variation cloning materially contributes to the internal path, A/B a private shared result. Public FontMatch remains Vec<FontVariation> and matching semantics, descriptor clamping, size-adjust, generation invalidation and cache bounds stay unchanged. A shared-result candidate must be judged on miss costs and retention as well as warm allocation. A negative or inconclusive result is a valid deliverable with raw data and explicit adoption decision.

System discovery disabled; default limits unchanged. Do not alter existing fixed fixtures or saved spikes. Performance does not block raikiri switching or S4. Timing, gross, net, peak and unmeasured RSS remain distinct. Archive exact source/config/font/checksums/raw and scope definitions. No universal speed or allocation claims.

Baseline:72 dedicated conditions verified. Variable normalized warm10000 =10000 hits/0 misses and10000 allocation calls/240000 gross bytes, net0/peak24; static and negative internal warm0 allocations. Variable plain16384 build33103 calls/11874316 gross bytes with16388 hits/0 misses. Approximate one axis-vector clone per grapheme is49.5% of call count,3.3% of gross bytes. This warrants a private-sharing A/B, not a dominance claim about total CPU or net retention.
