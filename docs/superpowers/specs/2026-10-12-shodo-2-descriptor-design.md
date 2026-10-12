# SHODO-2: descriptor presence without an owned clone

Source: Plane SHODO-2, retrieved 2026-10-12. Baseline: `732d61aa568558d1343842b8980961063be3b550`.

`selected_match` currently calls `face_descriptor(id).is_some()`. The public method clones a descriptor's family and Unicode ranges under the catalog mutex even though selection only needs its presence.

Use a private `FontCollection::has_face_descriptor(&self, id: FontId) -> bool`. For the current layer it borrows the slot under the same mutex; for a different layer it delegates to the parent. A missing slot, absent descriptor or invisible layer returns false. `selected_match` uses this method. Keep the public owned-returning API unchanged.

A presence field on `Candidate` could additionally remove the mutex acquisition. It would duplicate catalog metadata across native/materialized/registered/prepared candidate paths and change candidate storage. Start with the direct borrowed lookup, measure its effect, and add candidate state only if the remaining lookup cost warrants it. No descriptor caching or longer lock lifetime is introduced.

Acceptance:

- Preserve lookup visibility/order for shared, document and sibling collections, invalid IDs, descriptor-less faces and subsequent registrations.
- Preserve descriptor-controlled slant axes and request-controlled axes for descriptor-less faces, with disabled cache and warmed prepared candidates.
- A warmed static prepared match must not allocate a copy of a large descriptor range payload. Use a real public match and allocation counts, not instrumentation of the clone method.
- Measure release builds before/after using identical fixed font bytes. Separate prepared selection, descriptor-heavy selection and a repeat match-cache hit control; retain raw samples, allocations and selection digests.
- Pass workspace tests, allocation regression, feature configurations, formatting, Clippy, snapshots and CI. Review the final branch, merge only its passing head, record results, remove its worktrees and scratch, and close SHODO-2.

Constraints: Rust MSRV 1.89; no public API/dependency changes. Temporary data belongs under `~/tmp`; preserve unrelated root files.
