# shodo-zb0.2: avoid RubyHit paths in ordinary hit testing

`LineLayout::hit_test` only needs the owning Ruby entry to map a hit back to its
base caret. It now uses a pathless recursive predicate for Ruby descendants;
`hit_test_ruby` still builds and returns its public parent-to-child path. Both
routes share the same candidate ordering and inverse transform calculation.

The test-first allocator probe used the fixed-font nested Ruby fixture and
counted 32 repeated ordinary hits. Before the change, depth 1 made 32 allocation
calls (256 bytes, 8-byte peak extra); after the change, depths 1, 2, and 4 each
made zero calls and allocated zero bytes. The public `RubyHit` route continues
to allocate its returned path. The integration test also checks path order,
base-source mapping, visible nested Ruby hits, warnings, and real glyphs.

## Verification

- `cargo test --locked -p shodo-bench --features allocation-counting --test ruby_hit_path_allocations`
- `cargo test --locked -p shodo ruby::hit::tests`
- `cargo run --locked -p shodo-bench --example ruby_hit_index`
- `cargo test --locked --workspace` with `RUSTFLAGS=-D warnings`
- `cargo fmt --all --check`

The allocation integration target is included explicitly in the CI allocator
test command so this regression check runs on every CI build.
