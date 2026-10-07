#![cfg(feature = "allocation-counting")]

use shodo::ParagraphBuilder;
use shodo::limits::{LimitKind, Limits};
use shodo::node::{NodeId, TextSource};
use shodo::ruby::RubyContent;
use shodo::style::{FontFamily, InlineStyle, ParagraphStyle};
use shodo_bench::allocator::CountingAllocator;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn rejected_roots_and_ruby_text_do_not_copy_the_oversized_payload() {
    let style = InlineStyle {
        font_families: vec![FontFamily::Named("x".repeat(1024 * 1024))],
        ..Default::default()
    };
    let paragraph = ParagraphStyle {
        root: style.clone(),
        ..Default::default()
    };
    let limits = Limits {
        max_style_bytes: Some(8192),
        ..Default::default()
    };
    let scope = ALLOCATOR.begin().unwrap();
    let builder = ParagraphBuilder::new(&paragraph, &limits);
    let counts = scope.finish();
    println!("root rejection allocated {} bytes", counts.allocated_bytes);
    assert_eq!(builder.error().unwrap().kind, LimitKind::StyleBytes);
    assert!(
        counts.allocated_bytes < 8192,
        "root rejection allocated {} bytes",
        counts.allocated_bytes
    );
    drop(builder);
    let scope = ALLOCATOR.begin().unwrap();
    let content = RubyContent::text(
        TextSource::Generated { node: NodeId(1) },
        "a",
        &style,
        &limits,
    );
    let counts = scope.finish();
    println!("ruby rejection allocated {} bytes", counts.allocated_bytes);
    assert!(
        counts.allocated_bytes < 8192,
        "ruby rejection allocated {} bytes",
        counts.allocated_bytes
    );
    drop(content);

    // An accepted root owns two family payload copies (paragraph + style
    // slot). Interning must not allocate a third copy for a string key.
    let payload_bytes = 64 * 1024;
    let paragraph = ParagraphStyle {
        root: InlineStyle {
            font_families: vec![FontFamily::Named("x".repeat(payload_bytes))],
            ..Default::default()
        },
        ..Default::default()
    };
    let scope = ALLOCATOR.begin().unwrap();
    let builder = ParagraphBuilder::new(&paragraph, &Limits::unlimited());
    let counts = scope.finish();
    assert_eq!(builder.error(), None);
    assert!(
        counts.allocated_bytes < (3 * payload_bytes) as u64,
        "accepted root allocated {} bytes; interning must borrow its key payload",
        counts.allocated_bytes
    );
    drop(builder);
}
