# S1: browser font layer

## Intent and scope

Implement shodo-p2m.2 without regressing the S0 fixed-width layout scaffold.
A browser needs document-local CSS faces, deterministic family mappings,
cluster fallback, locale-aware CJK selection, and metrics for CSS units.
The issue queue workflow authorizes implementation, PR creation, CI gating,
merge and worktree cleanup. S2 remains responsible for paragraph shaping.

## Architecture

Keep FontCollection's Arc layer identity, generation counters, byte/face budgets
and document isolation. Each layer owns an unshared fontique 0.11 collection
and SourceCache behind its existing Mutex. The shared layer supplies generic
and script/locale fallback mappings. System enumeration is lazy, optional at
compile time (default system-fonts), and configurable at runtime. wasm has no
platform backend. SourceCache prune age is configurable.

CSS registration is separate from raw register(Vec<u8>): register_face accepts
one selected sfnt/TTC face plus a descriptor (family, weight range, width range,
style, unicode ranges). Descriptor validation is transactional. fontique supplies
raw metadata and system family enumeration; CSS ranges must be matched by shodo,
not flattened to their lower endpoint. Registration retains the underlying blob
once and preserves stable FontId values. local() resolves installed full/PostScript
names without rewriting the caller's family list; source lists try local and data
in order. Document faces never leak to another document or shared collection.

FontQuery carries families, weight/width/style, script, language and emoji
presentation. Match each family in order, consulting document then shared within
that family before moving to the next. Generic mappings come from the shared
layer. Filter CSS ranges by the cluster's base characters and cmap coverage of
all visible characters, ignoring joiners/variation selectors during nominal cmap
checks. Return one font for the whole supplied grapheme; absence returns an explicit
missing match, not an unrelated partial face. Fallback is configured by script and
locale, then fontique's platform fallback, then registered fonts. Cache keys include
all query fields, the complete cluster and both layer generations; bounded entry
count and invalidation prevent stale results after late font arrival.

Use CSS weight search order including special 400..500 rules. Width matching uses
CSS preferred direction around 100%; style matching prefers normal/italic/oblique
according to requested style. Variable ranges match interior values directly.
Expose effective variation settings and synthesis in matches for S2.

skrifa 0.44 supplies cmap and size/location-dependent metrics. Preserve the old
metrics(id,size) convenience API and add coordinate-aware metrics plus vertical
metrics. resolve_ch uses the selected zero glyph advance (fallback 0.5em);
resolve_ic uses U+6C34 (fallback 1em); report selected FontId as well as advance.
Share harfrust 0.12 ShaperData in an entry-count-bounded LRU. It must be Send+Sync.
Layer IDs must never wrap/reuse; report released IDs via weak-handle observation.

## Input safety and WOFF

Retain pre-registration GSUB/GPOS count checks and strengthen acceptance for GDEF,
Coverage/ClassDef and AAT structures before constructing ShaperData. Count repeated
references, declared lengths and reserved arrays. Respect Limits and fail before
retention, including selected TTC face and descriptor failures.

Foundation spec placed WOFF outside registration. The issue explicitly requests
WOFF/WOFF2 support: provide a distinct decode_web_font(data,limits) helper using
wuff, and a source-list registration entry point that calls it. Raw register still
accepts sfnt/TTC only. Check compressed input and decompressed size budgets; use
bounded custom decoder callbacks if wuff defaults cannot enforce expansion limits.
Never trust totalSfntSize alone. Malformed input returns FontError, never a panic.

## Validation

Use repository-owned synthetic fonts with explicit cmap, metrics, family names,
weights and optional axes, plus deterministic compressed fixtures. No system fonts
in tests. Cover document isolation, family precedence, ranges, local source ordering,
CJK locales, emoji/text preferences, whole-cluster selection, late registrations,
CSS weight boundaries, missing units, actual metrics, cache eviction and malformed
font amplification. All existing tests must remain green. CI: fmt, clippy with
warnings denied, stable and 1.89 tests, wasm build; also no-default-features tests.
