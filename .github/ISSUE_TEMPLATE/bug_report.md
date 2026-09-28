---
name: Bug report
about: Report a reproducible problem in shodo
title: ''
labels: ''
assignees: ''
---

<!-- Search existing issues first. See CONTRIBUTING.md for reproduction guidance. -->

## Problem

Describe what happened and what you expected instead.

## Minimal reproduction

Include a small Rust example and the command used to run it. For layout bugs,
include the text, computed styles, available width, writing mode, and direction.

```rust
// Reproduction
```

## Fonts and output

Give font family/file identity and version or hash, and whether system discovery
is enabled. If possible, reproduce with the repository's fixed fixture fonts.
Include build/layout warnings and relevant glyph IDs, source ranges, geometry,
or a snapshot report. Attach images if they help explain the difference.

## Environment

- shodo revision or dependency specification:
- Rust version (`rustc --version`):
- OS and target:
- Cargo features (including whether defaults are disabled):

## Additional context

Related issues, a regression's last known working revision, or a browser comparison
with its version and matching fonts/styles, if relevant.
